use crate::storage::init::InitState;
use anyhow::Result;
use controller::prelude::anyhow::Context;
use controller::prelude::chrono::{Duration, Utc};
use controller::prelude::control::storage::get_attribute;
use controller::prelude::redis::aio::ConnectionManager;
use controller::prelude::*;
use controller_shared::ParticipantId;
use etherpad_client::EtherpadClient;
use outgoing::{ReadUrl, WriteUrl};
use rabbitmq::GenerateUrl;

pub mod incoming;
pub mod outgoing;
pub mod rabbitmq;
pub mod storage;

const PAD_NAME: &str = "protocol";

struct SessionInfo {
    group_id: String,
    session_id: String,
}

pub struct Protocol {
    etherpad: EtherpadClient,
    participant_id: ParticipantId,
    role: Role,
    room_id: SignalingRoomId,
}

#[async_trait::async_trait(?Send)]
impl SignalingModule for Protocol {
    const NAMESPACE: &'static str = "protocol";
    type Params = controller_shared::settings::Etherpad;
    type Incoming = incoming::Message;
    type Outgoing = outgoing::Message;
    type RabbitMqMessage = rabbitmq::Event;
    type ExtEvent = ();
    type FrontendData = ();
    type PeerFrontendData = ();

    async fn init(
        ctx: InitContext<'_, Self>,
        params: &Self::Params,
        _protocol: &'static str,
    ) -> Result<Option<Self>> {
        let etherpad = EtherpadClient::new(params.url.clone(), params.api_key.clone());

        Ok(Some(Self {
            etherpad,
            participant_id: ctx.participant_id(),
            role: ctx.role(),
            room_id: ctx.room_id(),
        }))
    }

    async fn on_event(
        &mut self,
        mut ctx: ModuleContext<'_, Self>,
        event: Event<'_, Self>,
    ) -> Result<()> {
        match event {
            Event::Joined { .. } => {
                let state = storage::init::get(ctx.redis_conn(), self.room_id).await?;

                if matches!(state, Some(state) if state == InitState::Initialized) {
                    let read_url = self.generate_readonly_url(&mut ctx).await?;

                    ctx.ws_send(outgoing::Message::ReadUrl(ReadUrl { url: read_url }));
                }
            }
            Event::WsMessage(msg) => {
                self.on_ws_message(&mut ctx, msg).await?;
            }
            Event::RabbitMq(event) => {
                self.on_rabbitmq_event(&mut ctx, event).await?;
            }
            _ => (),
        }

        Ok(())
    }

    async fn on_destroy(self, mut ctx: DestroyContext<'_>) {
        if let Err(e) = self.cleanup_etherpad(ctx.redis_conn()).await {
            log::error!(
                "Failed to cleanup etherpad for room {} in redis: {}",
                self.room_id,
                e
            );
        }

        if let Err(e) = storage::cleanup(ctx.redis_conn(), self.room_id).await {
            log::error!(
                "Failed to cleanup protocol keys for room {} in redis: {}",
                self.room_id,
                e
            );
        }
    }
}

impl Protocol {
    async fn on_ws_message(
        &mut self,
        ctx: &mut ModuleContext<'_, Self>,
        msg: incoming::Message,
    ) -> Result<()> {
        match msg {
            incoming::Message::SelectWriter(select_writer) => {
                let targets = select_writer.participant_ids;

                if self.role != Role::Moderator {
                    ctx.ws_send(outgoing::Message::Error(
                        outgoing::Error::InsufficientPermissions,
                    ));

                    return Ok(());
                }

                // TODO: disallow selecting the same participant twice

                let redis_conn = ctx.redis_conn();

                let init_state = storage::init::try_start_init(redis_conn, self.room_id).await?;

                let first_init = match init_state {
                    Some(state) => {
                        match state {
                            storage::init::InitState::Initializing => {
                                // Some other instance is currently initializing the etherpad
                                ctx.ws_send(outgoing::Message::Error(
                                    outgoing::Error::CurrentlyInitializing,
                                ));
                                return Ok(());
                            }
                            storage::init::InitState::Initialized => false,
                        }
                    }
                    None => {
                        // No init state was set before -> Initialize the etherpad in this module instance
                        if let Err(e) = self.init_etherpad(redis_conn).await {
                            log::error!("Failed to init etherpad for room {}, {}", self.room_id, e);

                            ctx.ws_send(outgoing::Message::Error(
                                outgoing::Error::FailedInitialization,
                            ));

                            return Ok(());
                        }

                        true
                    }
                };

                if first_init {
                    // all participants get access on first initialization
                    ctx.rabbitmq_publish(
                        control::rabbitmq::current_room_exchange_name(self.room_id),
                        control::rabbitmq::room_all_routing_key().into(),
                        rabbitmq::Event::GenerateUrl(GenerateUrl { writer: targets }),
                    );
                } else {
                    // calls after the first init only reach the targeted participants
                    for participant_id in targets {
                        ctx.rabbitmq_publish(
                            control::rabbitmq::current_room_exchange_name(self.room_id),
                            control::rabbitmq::room_participant_routing_key(participant_id),
                            rabbitmq::Event::GenerateUrl(GenerateUrl {
                                writer: vec![participant_id],
                            }),
                        );
                    }
                }
            }
        }

        Ok(())
    }

    async fn on_rabbitmq_event(
        &mut self,
        ctx: &mut ModuleContext<'_, Self>,
        event: rabbitmq::Event,
    ) -> Result<()> {
        match event {
            rabbitmq::Event::GenerateUrl(GenerateUrl { writer }) => {
                if writer.contains(&self.participant_id) {
                    let write_url = self.generate_write_url(ctx).await?;

                    ctx.ws_send(outgoing::Message::WriteUrl(WriteUrl { url: write_url }));
                } else {
                    let read_url = self.generate_readonly_url(ctx).await?;

                    ctx.ws_send(outgoing::Message::ReadUrl(ReadUrl { url: read_url }));
                }
            }
        }

        Ok(())
    }

    /// Initializes the etherpad-group and -pad for this room
    async fn init_etherpad(&self, redis_conn: &mut ConnectionManager) -> Result<()> {
        let group_id = self
            .etherpad
            .create_group_for(self.room_id.to_string())
            .await?;

        self.etherpad
            .create_group_pad(&group_id, PAD_NAME, None)
            .await?;

        let pad_id = format!("{}${}", group_id, PAD_NAME);

        let readonly_pad_id = self.etherpad.get_readonly_pad_id(&pad_id).await?;

        storage::group::set(redis_conn, self.room_id, &group_id).await?;

        storage::pad::set_readonly(redis_conn, self.room_id, &readonly_pad_id).await?;

        // flag this room as initialized
        storage::init::set_initialized(redis_conn, self.room_id).await?;

        Ok(())
    }

    /// Creates a new etherpad author for the participant
    ///
    /// Returns the generated author id
    async fn create_author(&self, redis_conn: &mut ConnectionManager) -> Result<String> {
        let display_name: String = get_attribute(
            redis_conn,
            self.room_id,
            self.participant_id,
            "display_name",
        )
        .await
        .context("Failed to get display_name attribute")?;

        let author_id = self
            .etherpad
            .create_author_if_not_exits_for(&display_name, &self.participant_id.to_string())
            .await?;

        Ok(author_id)
    }

    /// Creates a new etherpad session
    ///
    /// Returns the generated session id
    async fn create_session(
        &self,
        group_id: &str,
        author_id: &str,
        expire_duration: Duration,
    ) -> Result<String> {
        let expires = Utc::now()
            .checked_add_signed(expire_duration)
            .context("DateTime overflow")?
            .timestamp();

        let session_id = self
            .etherpad
            .create_session(group_id, author_id, expires)
            .await?;

        Ok(session_id)
    }

    /// Creates a new author & session for a the participant
    ///
    /// Returns the `[SessionInfo]`
    async fn prepare_and_create_user_session(
        &mut self,
        redis_conn: &mut ConnectionManager,
    ) -> Result<SessionInfo> {
        let author_id = self
            .create_author(redis_conn)
            .await
            .context("Failed to create author while preparing a new session")?;

        let group_id = storage::group::get(redis_conn, self.room_id)
            .await?
            .context("Missing group for room while preparing a new session")?;

        // Currently there is no proper session refresh in etherpad. Due to the the difficulty of setting new sessions
        // on the client across domains, we set the expire duration to 14 days and hope for the best.
        // Session refresh is merged and will be available in the next release: https://github.com/ether/etherpad-lite/pull/5361
        // TODO: use proper session refresh from etherpad once it's released
        let session_id = self
            .create_session(&group_id, &author_id, Duration::days(14))
            .await?;

        Ok(SessionInfo {
            group_id,
            session_id,
        })
    }

    /// Generates the readonly auth-session url
    async fn generate_readonly_url(&mut self, ctx: &mut ModuleContext<'_, Self>) -> Result<String> {
        let redis_conn = ctx.redis_conn();

        let session_info = self.prepare_and_create_user_session(redis_conn).await?;

        let pad_id = storage::pad::get_readonly(redis_conn, self.room_id).await?;

        let read_url = self
            .etherpad
            .auth_session_url(&session_info.session_id, &pad_id, None)?;

        Ok(read_url.to_string())
    }

    /// Generates the write-access auth-session url
    async fn generate_write_url(&mut self, ctx: &mut ModuleContext<'_, Self>) -> Result<String> {
        let redis_conn = ctx.redis_conn();

        let session_info = self.prepare_and_create_user_session(redis_conn).await?;

        let write_url = self.etherpad.auth_session_url(
            &session_info.session_id,
            PAD_NAME,
            Some(&session_info.group_id),
        )?;

        Ok(write_url.to_string())
    }

    /// Removes the room related pad and group from etherpad
    async fn cleanup_etherpad(&self, redis_conn: &mut ConnectionManager) -> Result<()> {
        let init_state = storage::init::get(redis_conn, self.room_id).await?;

        if init_state.is_none() {
            // Nothing to cleanup
            return Ok(());
        }

        let group_id = storage::group::get(redis_conn, self.room_id)
            .await?
            .unwrap();

        let pad_id = format!("{}${}", group_id, PAD_NAME);

        self.etherpad.delete_pad(&pad_id).await?;

        // invalidate all sessions by deleting the group
        self.etherpad.delete_group(&group_id).await?;

        Ok(())
    }
}

pub fn register(controller: &mut controller::Controller) {
    let etherpad = controller.shared_settings.load_full().etherpad.clone();

    match etherpad {
        Some(etherpad) => {
            controller.signaling.add_module::<Protocol>(etherpad);
        }
        None => {
            log::warn!("Skipping the Protocol module as no etherpad is specified in the config")
        }
    }
}
