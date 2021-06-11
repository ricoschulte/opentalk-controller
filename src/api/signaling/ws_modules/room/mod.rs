use crate::api::signaling::local::{MemberKind, Room, RoomEvent};
use crate::api::signaling::mcu::{
    MediaSessionKey, MediaSessionType, Request, Response, TrickleMessage,
};
use crate::api::signaling::media::Media;
use crate::api::signaling::{JanusMcu, ParticipantId};
use crate::db::users::User;
use crate::modules::http::ws::{Event, WebSocketModule, WsCtx};
use anyhow::{bail, Result};
use futures::stream::{select, StreamExt};
use janus_client::TrickleCandidate;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tokio_stream::wrappers::ReceiverStream;

mod incoming;
mod outgoing;

pub struct RoomControl {
    id: ParticipantId,

    mcu: Arc<JanusMcu>,
    media: Media,

    local: Arc<RwLock<Room>>,

    room_evt_sender: mpsc::Sender<RoomEvent>,
}

pub enum ExtEvent {
    Room(RoomEvent),
    Media((MediaSessionKey, TrickleMessage)),
}

#[async_trait::async_trait(?Send)]
impl WebSocketModule for RoomControl {
    const NAMESPACE: &'static str = "room";

    type Params = (Arc<JanusMcu>, Arc<RwLock<Room>>);

    type Incoming = incoming::Message;
    type Outgoing = outgoing::Message;

    type ExtEvent = ExtEvent;

    async fn init(
        mut ctx: WsCtx<'_, Self>,
        (mcu, room): &Self::Params,
        _protocol: &'static str,
    ) -> Self {
        let participant_id = ParticipantId::new();

        let (room_evt_sender, room_events) = mpsc::channel(12);
        let (media_sender, janus_events) = mpsc::channel(12);

        ctx.add_event_stream(select(
            ReceiverStream::new(room_events).map(ExtEvent::Room),
            ReceiverStream::new(janus_events).map(ExtEvent::Media),
        ));

        Self {
            id: participant_id,
            mcu: mcu.clone(),
            media: Media::new(participant_id, media_sender),
            local: room.clone(),
            room_evt_sender,
        }
    }

    async fn on_event(&mut self, mut ctx: WsCtx<'_, Self>, event: Event<Self>) {
        match event {
            Event::WsMessage(incoming::Message::Join(join)) => {
                let user_id = ctx
                    .local_state()
                    .get::<User>()
                    .expect("User must be in local_state")
                    .id;

                let mut room = self.local.write().await;

                let participants = room
                    .members
                    .iter()
                    .map(|member| member.participant.clone())
                    .collect();

                room.add_member(
                    self.id,
                    join.display_name,
                    MemberKind::User(user_id),
                    self.room_evt_sender.clone(),
                )
                .await;

                ctx.ws_send(outgoing::Message::JoinSuccess(outgoing::JoinSuccess {
                    id: self.id,
                    participants,
                }));
            }
            Event::WsMessage(incoming::Message::PublishComplete(info))
            | Event::WsMessage(incoming::Message::UpdateMediaSession(info)) => {
                let mut room = self.local.write().await;

                for member in &mut room.members {
                    if member.participant.id == self.id {
                        member
                            .participant
                            .publishing
                            .entry(info.media_session_type)
                            .or_insert(info.media_session_state);
                    } else {
                        member.send(RoomEvent::Update(self.id)).await;
                    }
                }
            }
            Event::WsMessage(incoming::Message::Unpublish(assoc)) => {
                self.media.remove_publisher(assoc.media_session_type);

                let mut room = self.local.write().await;

                for member in &mut room.members {
                    if member.participant.id == self.id {
                        member
                            .participant
                            .publishing
                            .remove(&assoc.media_session_type);
                    } else {
                        member.send(RoomEvent::Update(self.id)).await;
                    }
                }
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
            Event::Ext(ExtEvent::Media((k, m))) => match m {
                TrickleMessage::Completed => {
                    log::warn!("Unimplemented TrickleMessage::Completed received");
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
            Event::Ext(ExtEvent::Room(RoomEvent::Update(id))) => {
                let room = self.local.read().await;
                let member = room.members.iter().find(|m| m.participant.id == id);

                if let Some(member) = member {
                    self.media
                        .remove_dangling_subscribers(id, &member.participant.publishing);

                    let participant = member.participant.clone();

                    drop(room);

                    ctx.ws_send(outgoing::Message::Update(participant));
                } else {
                    log::error!("No member with id {} in room", id);
                }
            }
            Event::Ext(ExtEvent::Room(RoomEvent::Joined(participant))) => {
                ctx.ws_send(outgoing::Message::Joined(participant));
            }
            Event::Ext(ExtEvent::Room(RoomEvent::Left(id))) => {
                self.media.remove_subscribers(id);
                ctx.ws_send(outgoing::Message::Left(outgoing::AssociatedParticipant {
                    id,
                }));
            }
        }
    }

    async fn on_destroy(self) {
        let mut room = self.local.write().await;
        room.remove_member(self.id).await;
    }
}

impl RoomControl {
    async fn handle_sdp_offer(
        &mut self,
        ctx: &mut WsCtx<'_, Self>,
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
            todo!("Create subscriber with offer?")
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
            // TODO did we need a response here?
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
        ctx: &mut WsCtx<'_, Self>,
        target: ParticipantId,
        media_session_type: MediaSessionType,
    ) -> Result<()> {
        if target == self.id {
            log::error!("Got requestOffer for own participant-id");
            // TODO SEND ERROR RESPONSE VIA NEW RETURN ERROR TYPE
        } else {
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
        }

        Ok(())
    }
}
