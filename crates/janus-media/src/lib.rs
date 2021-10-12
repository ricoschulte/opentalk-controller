//! # Media Module
//!
//! ## Functionality
//!
//! Handles media related messages and manages their respective forwarding to janus-gateway via rabbitmq.
use anyhow::{bail, Context, Result};
use controller::prelude::*;
use controller::Controller;
use incoming::TargetConfigure;
use janus_client::TrickleCandidate;
use mcu::McuPool;
use mcu::{
    LinkDirection, MediaSessionKey, MediaSessionType, Request, Response, TrickleMessage,
    WebRtcEvent,
};
use outgoing::Link;
use serde::{Deserialize, Serialize};
use sessions::MediaSessions;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

mod incoming;
mod mcu;
mod outgoing;
mod sessions;
mod settings;
mod storage;

pub struct Media {
    id: ParticipantId,
    room: SignalingRoomId,

    mcu: Arc<McuPool>,
    media: MediaSessions,

    state: State,
}

type State = HashMap<MediaSessionType, MediaSessionState>;

#[derive(Debug, Serialize, Deserialize)]
pub struct MediaSessionState {
    pub video: bool,
    pub audio: bool,
}

#[async_trait::async_trait(?Send)]
impl SignalingModule for Media {
    const NAMESPACE: &'static str = "media";

    type Params = Arc<McuPool>;

    type Incoming = incoming::Message;
    type Outgoing = outgoing::Message;
    type RabbitMqMessage = ();

    type ExtEvent = (MediaSessionKey, WebRtcEvent);

    type FrontendData = ();
    type PeerFrontendData = State;

    async fn init(
        mut ctx: InitContext<'_, Self>,
        mcu: &Self::Params,
        _protocol: &'static str,
    ) -> Result<Option<Self>> {
        let (media_sender, janus_events) = mpsc::channel(12);

        let state = HashMap::new();

        let id = ctx.participant_id();
        let room = ctx.room_id();

        storage::set_state(ctx.redis_conn(), room, id, &state).await?;

        ctx.add_event_stream(ReceiverStream::new(janus_events));

        Ok(Some(Self {
            id,
            room,
            mcu: mcu.clone(),
            media: MediaSessions::new(ctx.participant_id(), media_sender),
            state,
        }))
    }

    async fn on_event(
        &mut self,
        mut ctx: ModuleContext<'_, Self>,
        event: Event<'_, Self>,
    ) -> Result<()> {
        match event {
            Event::WsMessage(incoming::Message::PublishComplete(info)) => {
                self.state
                    .insert(info.media_session_type, info.media_session_state);

                storage::set_state(ctx.redis_conn(), self.room, self.id, &self.state)
                    .await
                    .context("Failed to set state attribute in storage")?;

                ctx.invalidate_data();
            }
            Event::WsMessage(incoming::Message::UpdateMediaSession(info)) => {
                if let Some(state) = self.state.get_mut(&info.media_session_type) {
                    *state = info.media_session_state;

                    storage::set_state(ctx.redis_conn(), self.room, self.id, &self.state)
                        .await
                        .context("Failed to set state attribute in storage")?;

                    ctx.invalidate_data();
                }
            }
            Event::WsMessage(incoming::Message::Unpublish(assoc)) => {
                self.media.remove_publisher(assoc.media_session_type).await;
                self.state.remove(&assoc.media_session_type);

                storage::set_state(ctx.redis_conn(), self.room, self.id, &self.state)
                    .await
                    .context("Failed to set state attribute in storage")?;

                ctx.invalidate_data();
            }
            Event::WsMessage(incoming::Message::Publish(targeted)) => {
                if let Err(e) = self
                    .handle_sdp_offer(
                        &mut ctx,
                        targeted.target.target,
                        targeted.target.media_session_type,
                        targeted.sdp,
                    )
                    .await
                {
                    log::error!(
                        "Failed to handle sdp offer for {:?}, {:?}",
                        targeted.target,
                        e
                    );
                    ctx.ws_send(outgoing::Message::Error {
                        text: "failed to process offer",
                    });
                }
            }
            Event::WsMessage(incoming::Message::SdpAnswer(targeted)) => {
                if let Err(e) = self
                    .handle_sdp_answer(
                        targeted.target.target,
                        targeted.target.media_session_type,
                        targeted.sdp,
                    )
                    .await
                {
                    log::error!("Failed to handle sdp answer {:?}, {:?}", targeted.target, e);
                    ctx.ws_send(outgoing::Message::Error {
                        text: "failed to process answer",
                    });
                }
            }
            Event::WsMessage(incoming::Message::SdpCandidate(targeted)) => {
                if let Err(e) = self
                    .handle_sdp_candidate(
                        targeted.target.target,
                        targeted.target.media_session_type,
                        targeted.candidate,
                    )
                    .await
                {
                    log::error!(
                        "Failed to handle sdp candidate {:?}, {:?}",
                        targeted.target,
                        e
                    );
                    ctx.ws_send(outgoing::Message::Error {
                        text: "failed to process candidate",
                    });
                }
            }
            Event::WsMessage(incoming::Message::SdpEndOfCandidates(target)) => {
                if let Err(e) = self
                    .handle_sdp_end_of_candidates(target.target, target.media_session_type)
                    .await
                {
                    log::error!(
                        "Failed to handle sdp end-of-candidates {:?}, {:?}",
                        target,
                        e
                    );
                    ctx.ws_send(outgoing::Message::Error {
                        text: "failed to process endOfCandidates",
                    });
                }
            }
            Event::WsMessage(incoming::Message::Subscribe(target)) => {
                // Check that the subscribtion target is inside the room
                if !control::storage::participants_contains(
                    ctx.redis_conn(),
                    self.room,
                    target.target,
                )
                .await?
                {
                    // just dicard, shouldn't happen often
                    return Ok(());
                }

                if let Err(e) = self
                    .handle_sdp_request_offer(&mut ctx, target.target, target.media_session_type)
                    .await
                {
                    log::error!("Failed to handle sdp request-offer {:?}, {:?}", target, e);
                    ctx.ws_send(outgoing::Message::Error {
                        text: "failed to process requestOffer",
                    });
                }
            }
            Event::WsMessage(incoming::Message::Configure(target)) => {
                if let Err(e) = self.handle_configure(target).await {
                    log::error!("Failed to handle configure request {:?}", e);
                    ctx.ws_send(outgoing::Message::Error {
                        text: "Failed to process configure request",
                    });
                }
            }
            Event::Ext((media_session_key, message)) => match message {
                WebRtcEvent::AssociatedMcuDied => {
                    self.remove_broken_media_session(&mut ctx, media_session_key)
                        .await?;
                    ctx.ws_send(outgoing::Message::WebRtcDown(media_session_key.into()))
                }
                WebRtcEvent::WebRtcUp => {
                    ctx.ws_send(outgoing::Message::WebRtcUp(media_session_key.into()))
                }
                WebRtcEvent::WebRtcDown => {
                    ctx.ws_send(outgoing::Message::WebRtcDown(media_session_key.into()));

                    self.gracefully_remove_media_session(&mut ctx, media_session_key)
                        .await?;
                }
                WebRtcEvent::SlowLink(link_direction) => {
                    let direction = match link_direction {
                        LinkDirection::Upstream => outgoing::LinkDirection::Upstream,
                        LinkDirection::Downstream => outgoing::LinkDirection::Downstream,
                    };

                    ctx.ws_send(outgoing::Message::WebRtcSlow(Link {
                        direction,
                        source: media_session_key.into(),
                    }))
                }

                WebRtcEvent::Trickle(trickle_msg) => {
                    match trickle_msg {
                        TrickleMessage::Completed => {
                            log::warn!("Unimplemented TrickleMessage::Completed received");
                            // TODO find out when janus sends this, never actually got this message yet
                        }
                        TrickleMessage::Candidate(candidate) => {
                            ctx.ws_send(outgoing::Message::SdpCandidate(outgoing::SdpCandidate {
                                candidate,
                                source: media_session_key.into(),
                            }));
                        }
                    }
                }
            },
            Event::RabbitMq(_) => {}
            Event::ParticipantJoined(id, evt_state) => {
                *evt_state = Some(
                    storage::get_state(ctx.redis_conn(), self.room, id)
                        .await
                        .context("Failed to get peer participants state")?,
                );
            }
            Event::ParticipantUpdated(id, evt_state) => {
                let state = storage::get_state(ctx.redis_conn(), self.room, id)
                    .await
                    .context("Failed to get peer participants state")?;

                self.media.remove_dangling_subscriber(id, &state).await;

                *evt_state = Some(state);
            }
            Event::ParticipantLeft(id) => {
                self.media.remove_subscribers(id).await;
            }
            Event::Joined {
                control_data: _,

                frontend_data: _,
                participants,
            } => {
                for (id, evt_state) in participants {
                    let state = storage::get_state(ctx.redis_conn(), self.room, *id)
                        .await
                        .context("Failed to get peer participants state")?;

                    *evt_state = Some(state);
                }
            }
            Event::Leaving | Event::RaiseHand | Event::LowerHand { .. } => {}
        }

        Ok(())
    }

    async fn on_destroy(self, mut ctx: DestroyContext<'_>) {
        if let Err(e) = storage::del_state(ctx.redis_conn(), self.room, self.id).await {
            log::error!(
                "Media module for {} failed to remove its data from redis, {}",
                self.id,
                e
            );
        }

        // Spawn the destroy task since it doesn't need to be synchronized and errors cannot
        // be handled in this context anyway.
        // Not spawning it would make the runner take a lot of time to shutdown increasing
        // contention on the participants redlock where it is not needed.
        tokio::task::spawn_local(self.media.destroy());
    }
}

impl Media {
    /// Gracefully removes the media session that is associated with the provided MediaSessionKey
    ///
    /// Send detach and destroy messages to janus in order to remove a media session gracefully.
    #[tracing::instrument(level = "debug", skip(self, ctx))]
    async fn gracefully_remove_media_session(
        &mut self,
        ctx: &mut ModuleContext<'_, Self>,
        media_session_key: MediaSessionKey,
    ) -> Result<()> {
        if media_session_key.0 == self.id {
            log::trace!("Removing publisher {}", media_session_key);
            self.media.remove_publisher(media_session_key.1).await;
            self.state.remove(&media_session_key.1);

            storage::set_state(ctx.redis_conn(), self.room, self.id, &self.state)
                .await
                .context("Failed to set state attribute in storage")?;

            ctx.invalidate_data();
        } else {
            log::trace!("Removing subscriber {}", media_session_key);
            self.media.remove_subscriber(&media_session_key).await;
        }
        Ok(())
    }

    /// Kills a media session
    ///
    /// Opposed to [`Media::gracefully_remove_media_session`], this function will not inform janus
    /// about any changes to the media session.
    #[tracing::instrument(level = "debug", skip(self, ctx))]
    async fn remove_broken_media_session(
        &mut self,
        ctx: &mut ModuleContext<'_, Self>,
        media_session_key: MediaSessionKey,
    ) -> Result<()> {
        if media_session_key.0 == self.id {
            log::trace!("Removing broken publisher {}", media_session_key);
            self.media
                .remove_broken_publisher(media_session_key.1)
                .await;
            self.state.remove(&media_session_key.1);

            storage::set_state(ctx.redis_conn(), self.room, self.id, &self.state)
                .await
                .context("Failed to set state attribute in storage")?;

            ctx.invalidate_data();
        } else {
            log::trace!("Removing broken subscriber {}", media_session_key);
            self.media
                .remove_broken_subscriber(&media_session_key)
                .await;
        }
        Ok(())
    }

    #[tracing::instrument(level = "debug", skip(self, ctx, offer))]
    async fn handle_sdp_offer(
        &mut self,
        ctx: &mut ModuleContext<'_, Self>,
        target: ParticipantId,
        media_session_type: MediaSessionType,
        offer: String,
    ) -> Result<()> {
        if target == self.id {
            // Get the publisher and create if it doesnt exists
            let publisher = if let Some(publisher) = self.media.get_publisher(media_session_type) {
                publisher
            } else {
                self.media
                    .create_publisher(&self.mcu, media_session_type)
                    .await?
            };

            // Send to offer and await the result
            let response = publisher.send_message(Request::SdpOffer(offer)).await?;

            match response {
                Response::SdpAnswer(answer) => {
                    ctx.ws_send(outgoing::Message::SdpAnswer(outgoing::Sdp {
                        sdp: answer.sdp(),
                        source: outgoing::Source {
                            source: target,
                            media_session_type,
                        },
                    }));
                }
                Response::SdpOffer(_) | Response::None => {
                    bail!("Expected McuResponse::SdpAnswer(..), got {:?}", response)
                }
            }

            Ok(())
        } else {
            bail!("Invalid target id, cannot send offer to other participants");
        }
    }

    #[tracing::instrument(level = "debug", skip(self, answer))]
    async fn handle_sdp_answer(
        &mut self,
        target: ParticipantId,
        media_session_type: MediaSessionType,
        answer: String,
    ) -> Result<()> {
        if target == self.id {
            // Get the publisher and create if it doesnt exists
            let publisher = self
                .media
                .get_publisher(media_session_type)
                .context("SDP Answer for nonexistent publisher received")?;

            // Send to offer and await the result
            publisher.send_message(Request::SdpAnswer(answer)).await?;
        } else {
            let subscriber = self
                .media
                .get_subscriber(target, media_session_type)
                .context("SDP Answer for nonexisting subscriber received")?;

            subscriber.send_message(Request::SdpAnswer(answer)).await?;
        }

        Ok(())
    }

    #[tracing::instrument(level = "debug", skip(self, candidate))]
    async fn handle_sdp_candidate(
        &mut self,
        target: ParticipantId,
        media_session_type: MediaSessionType,
        candidate: TrickleCandidate,
    ) -> Result<()> {
        let req = Request::Candidate(candidate);

        if target == self.id {
            let publisher = self
                .media
                .get_publisher(media_session_type)
                .context("SDP candidate for nonexistent publisher received")?;

            publisher.send_message(req).await?;
        } else {
            let subscriber = self
                .media
                .get_subscriber(target, media_session_type)
                .context("SDP candidate for nonexisting subscriber received")?;

            subscriber.send_message(req).await?;
        }

        Ok(())
    }

    #[tracing::instrument(level = "debug", skip(self))]
    async fn handle_sdp_end_of_candidates(
        &mut self,
        target: ParticipantId,
        media_session_type: MediaSessionType,
    ) -> Result<()> {
        if target == self.id {
            let publisher = self
                .media
                .get_publisher(media_session_type)
                .context("SDP end-of-candidates for nonexistent publisher received")?;

            publisher.send_message(Request::EndOfCandidates).await?;
        } else {
            let subscriber = self
                .media
                .get_subscriber(target, media_session_type)
                .context("SDP end-of-candidates for nonexisting subscriber received")?;

            subscriber.send_message(Request::EndOfCandidates).await?;
        }

        Ok(())
    }

    #[tracing::instrument(level = "debug", skip(self, ctx))]
    async fn handle_sdp_request_offer(
        &mut self,
        ctx: &mut ModuleContext<'_, Self>,
        target: ParticipantId,
        media_session_type: MediaSessionType,
    ) -> Result<()> {
        if self.id == target {
            // Usually subscribing to self should be possible but cannot be realized with the
            // current messaging model. The frontend wouldn't know if a sdp-offer is an update
            // to the publish or a response to the requestOffer (subscribe)
            bail!("Cannot request offer for self");
        }

        let subscriber =
            if let Some(subscriber) = self.media.get_subscriber(target, media_session_type) {
                subscriber
            } else {
                self.media
                    .create_subscriber(self.mcu.as_ref(), target, media_session_type)
                    .await?
            };

        let response = subscriber.send_message(Request::RequestOffer).await?;

        match response {
            Response::SdpOffer(offer) => {
                ctx.ws_send(outgoing::Message::SdpOffer(outgoing::Sdp {
                    sdp: offer.sdp(),
                    source: outgoing::Source {
                        source: target,
                        media_session_type,
                    },
                }));
            }
            Response::SdpAnswer(_) | Response::None => {
                bail!("Expected McuResponse::SdpOffer(..) got {:?}", response)
            }
        }

        Ok(())
    }

    #[tracing::instrument(level = "debug", skip(self))]
    async fn handle_configure(&mut self, target: TargetConfigure) -> Result<()> {
        if let Some(subscriber) = self
            .media
            .get_subscriber(target.target, target.media_session_type)
        {
            subscriber
                .send_message(Request::Configure(target.configuration))
                .await?;
        } else {
            log::info!("Attempt to configure none existing subscriber");
        }

        Ok(())
    }
}

pub async fn register(controller: &mut Controller) -> Result<()> {
    let mcu_pool = McuPool::build(
        &controller.startup_settings,
        controller.shared_settings.clone(),
        controller.rabbitmq.clone(),
        controller.redis.clone(),
        controller.shutdown.subscribe(),
        controller.reload.subscribe(),
    )
    .await?;

    controller.signaling.add_module::<Media>(mcu_pool);

    Ok(())
}
