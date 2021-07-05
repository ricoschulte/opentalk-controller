use crate::api::signaling::mcu::{
    MediaSessionKey, MediaSessionType, Request, Response, TrickleMessage,
};
use crate::api::signaling::ws::{
    DestroyContext, Event, InitContext, ModuleContext, SignalingModule,
};
use crate::api::signaling::{JanusMcu, ParticipantId};
use anyhow::{bail, Context, Result};
use janus_client::TrickleCandidate;
use serde::{Deserialize, Serialize};
use sessions::MediaSessions;
use std::collections::HashMap;
use std::sync::{Arc, Weak};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use uuid::Uuid;

mod incoming;
mod outgoing;
mod sessions;
mod storage;

pub struct Media {
    id: ParticipantId,
    room: Uuid,

    mcu: Arc<JanusMcu>,
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

    type Params = Weak<JanusMcu>;

    type Incoming = incoming::Message;
    type Outgoing = outgoing::Message;
    type RabbitMqMessage = ();

    type ExtEvent = (MediaSessionKey, TrickleMessage);

    type FrontendData = ();
    type PeerFrontendData = State;

    async fn init(
        mut ctx: InitContext<'_, Self>,
        mcu: &Self::Params,
        _protocol: &'static str,
    ) -> Result<Self> {
        let (media_sender, janus_events) = mpsc::channel(12);

        let state = HashMap::new();

        let id = ctx.participant_id();
        let room = ctx.room_id();

        storage::set_state(ctx.redis_conn(), room, id, &state).await?;

        ctx.add_event_stream(ReceiverStream::new(janus_events));

        Ok(Self {
            id,
            room,
            mcu: mcu.upgrade().unwrap(),
            media: MediaSessions::new(ctx.participant_id(), media_sender),
            state,
        })
    }

    async fn on_event(
        &mut self,
        mut ctx: ModuleContext<'_, Self>,
        event: Event<'_, Self>,
    ) -> Result<()> {
        match event {
            Event::WsMessage(incoming::Message::PublishComplete(info))
            | Event::WsMessage(incoming::Message::UpdateMediaSession(info)) => {
                self.state
                    .insert(info.media_session_type, info.media_session_state);

                storage::set_state(ctx.redis_conn(), self.room, self.id, &self.state)
                    .await
                    .context("Failed to set state attribute in storage")?;

                ctx.invalidate_data();
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
                    log::error!("Failed to handle sdp offer, {}", e);
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
                    log::error!("Failed to handle sdp answer, {}", e);
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
                    log::error!("Failed to handle sdp candidate, {}", e);
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
                    log::error!("Failed to handle sdp end-of-candidates, {}", e);
                    ctx.ws_send(outgoing::Message::Error {
                        text: "failed to process endOfCandidates",
                    });
                }
            }
            Event::WsMessage(incoming::Message::Subscribe(target)) => {
                if let Err(e) = self
                    .handle_sdp_request_offer(&mut ctx, target.target, target.media_session_type)
                    .await
                {
                    log::error!("Failed to handle sdp request-offer, {}", e);
                    ctx.ws_send(outgoing::Message::Error {
                        text: "failed to process requestOffer",
                    });
                }
            }
            Event::Ext((k, m)) => match m {
                TrickleMessage::Completed => {
                    log::warn!("Unimplemented TrickleMessage::Completed received");
                    // TODO find out when janus sends this, never actually got this message yet
                }
                TrickleMessage::Candidate(candidate) => {
                    ctx.ws_send(outgoing::Message::SdpCandidate(outgoing::SdpCandidate {
                        candidate,
                        source: outgoing::Source {
                            source: k.0,
                            media_session_type: k.1,
                        },
                    }));
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
        }

        Ok(())
    }

    async fn on_destroy(self, mut ctx: DestroyContext<'_>) {
        if let Err(e) = storage::del_state(ctx.redis_conn(), self.room, self.id).await {
            log::warn!(
                "Media module for {} failed to remove its data from redis, {}",
                self.id,
                e
            );
        }

        self.media.destroy().await;
    }
}

impl Media {
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

    async fn handle_sdp_answer(
        &mut self,
        target: ParticipantId,
        media_session_type: MediaSessionType,
        answer: String,
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
            publisher.send_message(Request::SdpAnswer(answer)).await?;
        } else {
            let subscriber =
                if let Some(subscriber) = self.media.get_subscriber(target, media_session_type) {
                    subscriber
                } else {
                    self.media
                        .create_subscriber(self.mcu.as_ref(), target, media_session_type)
                        .await?
                };

            subscriber.send_message(Request::SdpAnswer(answer)).await?;
        }

        Ok(())
    }

    async fn handle_sdp_candidate(
        &mut self,
        target: ParticipantId,
        media_session_type: MediaSessionType,
        candidate: TrickleCandidate,
    ) -> Result<()> {
        let req = Request::Candidate(candidate);

        if target == self.id {
            let publisher = if let Some(publisher) = self.media.get_publisher(media_session_type) {
                publisher
            } else {
                self.media
                    .create_publisher(&self.mcu, media_session_type)
                    .await?
            };

            publisher.send_message(req).await?;
        } else {
            let subscriber =
                if let Some(subscriber) = self.media.get_subscriber(target, media_session_type) {
                    subscriber
                } else {
                    self.media
                        .create_subscriber(self.mcu.as_ref(), target, media_session_type)
                        .await?
                };

            subscriber.send_message(req).await?;
        }

        Ok(())
    }

    async fn handle_sdp_end_of_candidates(
        &mut self,
        target: ParticipantId,
        media_session_type: MediaSessionType,
    ) -> Result<()> {
        if target == self.id {
            let publisher = if let Some(publisher) = self.media.get_publisher(media_session_type) {
                publisher
            } else {
                self.media
                    .create_publisher(&self.mcu, media_session_type)
                    .await?
            };

            publisher.send_message(Request::EndOfCandidates).await?;
        } else {
            let subscriber =
                if let Some(subscriber) = self.media.get_subscriber(target, media_session_type) {
                    subscriber
                } else {
                    self.media
                        .create_subscriber(self.mcu.as_ref(), target, media_session_type)
                        .await?
                };

            subscriber.send_message(Request::EndOfCandidates).await?;
        }

        Ok(())
    }

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
}