use anyhow::Result;
use client::SpacedeckClient;
use controller::prelude::{
    async_trait, control, log, DestroyContext, Event, InitContext, ModuleContext, RedisConnection,
    Role, SignalingModule, SignalingRoomId,
};
use outgoing::AccessUrl;
use serde::Serialize;
use state::{InitState, SpaceInfo};
use url::Url;

mod client;
mod incoming;
mod outgoing;
mod rabbitmq;
mod state;

struct Spacedeck {
    room_id: SignalingRoomId,
    client: SpacedeckClient,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case", tag = "status", content = "url")]
enum FrontendData {
    NotInitialized,
    Initializing,
    Initialized(Url),
}

impl From<InitState> for FrontendData {
    fn from(init_state: InitState) -> Self {
        match init_state {
            InitState::Initializing => Self::Initializing,
            InitState::Initialized(info) => Self::Initialized(info.url),
        }
    }
}

#[async_trait::async_trait(?Send)]
impl SignalingModule for Spacedeck {
    const NAMESPACE: &'static str = "spacedeck";

    type Params = controller_shared::settings::Spacedeck;

    type Incoming = incoming::Message;

    type Outgoing = outgoing::Message;

    type RabbitMqMessage = rabbitmq::Event;

    type ExtEvent = ();

    type FrontendData = FrontendData;

    type PeerFrontendData = ();

    async fn init(
        ctx: InitContext<'_, Self>,
        params: &Self::Params,
        _protocol: &'static str,
    ) -> anyhow::Result<Option<Self>> {
        let client = SpacedeckClient::new(params.url.clone(), params.api_key.clone());

        Ok(Some(Self {
            room_id: ctx.room_id(),
            client,
        }))
    }

    async fn on_event(
        &mut self,
        mut ctx: ModuleContext<'_, Self>,
        event: Event<'_, Self>,
    ) -> anyhow::Result<()> {
        match event {
            Event::Joined {
                control_data: _,
                frontend_data,
                participants: _,
            } => {
                let data = match state::get(ctx.redis_conn(), self.room_id).await? {
                    Some(state) => state.into(),
                    None => FrontendData::NotInitialized,
                };

                *frontend_data = Some(data);

                Ok(())
            }
            Event::RabbitMq(event) => {
                match event {
                    rabbitmq::Event::Initialized => {
                        if let Some(InitState::Initialized(space_info)) =
                            state::get(ctx.redis_conn(), self.room_id).await?
                        {
                            ctx.ws_send(outgoing::Message::SpaceUrl(AccessUrl {
                                url: space_info.url,
                            }));
                        } else {
                            log::error!("Spacedeck module received `Initialized` but spacedeck was not initialized");
                        }
                    }
                    rabbitmq::Event::PdfUrl(url) => {
                        ctx.ws_send(outgoing::Message::PdfUrl(AccessUrl { url }));
                    }
                }
                Ok(())
            }

            Event::WsMessage(message) => {
                match message {
                    incoming::Message::Initialize => {
                        if ctx.role() != Role::Moderator {
                            ctx.ws_send(outgoing::Message::Error(
                                outgoing::Error::InsufficientPermissions,
                            ));
                            return Ok(());
                        }

                        if let Err(err) = self.create_space(&mut ctx).await {
                            log::error!(
                                "Failed to initialize spacedeck for room '{}': {}",
                                self.room_id,
                                err
                            );

                            self.cleanup(ctx.redis_conn()).await?;

                            ctx.ws_send(outgoing::Message::Error(
                                outgoing::Error::InitializationFailed,
                            ));
                        }
                    }

                    incoming::Message::GeneratePdf => {
                        if ctx.role() != Role::Moderator {
                            ctx.ws_send(outgoing::Message::Error(
                                outgoing::Error::InsufficientPermissions,
                            ));
                            return Ok(());
                        }

                        if let Some(state::InitState::Initialized(info)) =
                            state::get(ctx.redis_conn(), self.room_id).await?
                        {
                            let url = self.client.get_pdf(&info.id).await?;

                            ctx.rabbitmq_publish(
                                control::rabbitmq::current_room_exchange_name(self.room_id),
                                control::rabbitmq::room_all_routing_key().into(),
                                rabbitmq::Event::PdfUrl(url),
                            );
                        }
                    }
                }
                Ok(())
            }
            // ignored events
            Event::Leaving
            | Event::RaiseHand
            | Event::LowerHand
            | Event::ParticipantJoined(_, _)
            | Event::ParticipantLeft(_)
            | Event::ParticipantUpdated(_, _)
            | Event::Ext(_) => Ok(()),
        }
    }

    async fn on_destroy(self, mut ctx: DestroyContext<'_>) {
        // TODO: save space as PDF once we have a S3 storage solution

        if ctx.destroy_room() {
            if let Err(err) = self.cleanup(ctx.redis_conn()).await {
                log::error!(
                    "Failed to cleanup spacedeck for room `{}`: {}",
                    self.room_id,
                    err
                );
            }
        }
    }
}

impl Spacedeck {
    /// Creates a new spacedeck space
    ///
    /// When spacedeck gets initialized here, this function will send the [`rabbitmq::Event::Initialized`] to all
    /// participants in the room
    async fn create_space(&self, ctx: &mut ModuleContext<'_, Self>) -> Result<()> {
        match state::try_start_init(ctx.redis_conn(), self.room_id).await? {
            Some(state) => match state {
                InitState::Initializing => ctx.ws_send(outgoing::Message::Error(
                    outgoing::Error::CurrentlyInitializing,
                )),
                InitState::Initialized(_) => ctx.ws_send(outgoing::Message::Error(
                    outgoing::Error::AlreadyInitialized,
                )),
            },
            None => {
                let response = self
                    .client
                    .create_space(&self.room_id.to_string(), None)
                    .await?;

                let url = self.client.base_url.join(&format!(
                    "s/{hash}-{slug}",
                    hash = response.edit_hash,
                    slug = response.edit_slug
                ))?;

                let space_info = SpaceInfo {
                    id: response.id,
                    url,
                };

                state::set_initialized(ctx.redis_conn(), self.room_id, space_info).await?;

                ctx.rabbitmq_publish(
                    control::rabbitmq::current_room_exchange_name(self.room_id),
                    control::rabbitmq::room_all_routing_key().into(),
                    rabbitmq::Event::Initialized,
                );
            }
        }
        Ok(())
    }

    async fn cleanup(&self, redis_conn: &mut RedisConnection) -> Result<()> {
        let state = match state::get(redis_conn, self.room_id).await? {
            Some(state) => state,
            None => return Ok(()),
        };

        state::del(redis_conn, self.room_id).await?;

        if let InitState::Initialized(space_info) = state {
            self.client.delete_space(&space_info.id).await?;
        }

        Ok(())
    }
}

pub fn register(controller: &mut controller::Controller) {
    let spacedeck = controller.shared_settings.load_full().spacedeck.clone();

    match spacedeck {
        Some(spacedeck) => {
            controller.signaling.add_module::<Spacedeck>(spacedeck);
        }
        None => {
            log::warn!("Skipping the Spacedeck module as no spacedeck is specified in the config")
        }
    }
}
