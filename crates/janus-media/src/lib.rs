// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

//! # Media Module
//!
//! ## Functionality
//!
//! Handles media related messages and manages their respective forwarding to janus-gateway via rabbitmq.
use anyhow::{bail, Context, Result};
use controller::prelude::*;
use controller::settings::SharedSettings;
use controller::Controller;
use controller_shared::ParticipantId;
use focus::FocusDetection;
use incoming::{RequestMute, TargetConfigure};
use janus_client::TrickleCandidate;
use mcu::McuPool;
use mcu::PublishConfiguration;
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

mod focus;
mod incoming;
mod mcu;
mod outgoing;
mod rabbitmq;
mod sessions;
mod settings;
mod storage;

pub struct Media {
    id: ParticipantId,
    room: SignalingRoomId,

    mcu: Arc<McuPool>,
    media: MediaSessions,

    state: State,

    focus_detection: FocusDetection,
}

type State = HashMap<MediaSessionType, MediaSessionState>;

#[derive(Serialize)]
pub struct PeerFrontendData {
    #[serde(flatten)]
    state: Option<State>,
    is_presenter: bool,
}

#[derive(Serialize)]
pub struct FrontendData {
    is_presenter: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub struct MediaSessionState {
    pub video: bool,
    pub audio: bool,
}

fn process_metrics_for_media_session_state(
    ctx: &ModuleContext<'_, Media>,
    session_type: &MediaSessionType,
    previous: &Option<MediaSessionState>,
    new: &MediaSessionState,
) {
    if let Some(metrics) = ctx.metrics() {
        let previous = previous.unwrap_or(MediaSessionState {
            video: false,
            audio: false,
        });

        if !previous.audio && new.audio {
            metrics.increment_participants_with_audio_count(session_type.as_type_str());
        } else if previous.audio && !new.audio {
            metrics.decrement_participants_with_audio_count(session_type.as_type_str());
        }

        if !previous.video && new.video {
            metrics.increment_participants_with_video_count(session_type.as_type_str());
        } else if previous.video && !new.video {
            metrics.decrement_participants_with_video_count(session_type.as_type_str());
        }
    }
}

#[async_trait::async_trait(?Send)]
impl SignalingModule for Media {
    const NAMESPACE: &'static str = "media";

    type Params = Arc<McuPool>;

    type Incoming = incoming::Message;
    type Outgoing = outgoing::Message;
    type RabbitMqMessage = rabbitmq::Message;

    type ExtEvent = (MediaSessionKey, WebRtcEvent);

    type FrontendData = FrontendData;
    type PeerFrontendData = PeerFrontendData;

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

        if participants_have_presenter_role(mcu.shared_settings.clone()) {
            storage::set_presenter(ctx.redis_conn(), room, id).await?;
        }

        Ok(Some(Self {
            id,
            room,
            mcu: mcu.clone(),
            media: MediaSessions::new(ctx.participant_id(), media_sender),
            state,
            focus_detection: Default::default(),
        }))
    }

    async fn on_event(
        &mut self,
        mut ctx: ModuleContext<'_, Self>,
        event: Event<'_, Self>,
    ) -> Result<()> {
        match event {
            Event::WsMessage(incoming::Message::PublishComplete(info)) => {
                let previous_session_state = self.state.get(&info.media_session_type);

                process_metrics_for_media_session_state(
                    &ctx,
                    &info.media_session_type,
                    &previous_session_state.copied(),
                    &info.media_session_state,
                );

                let old_state = self
                    .state
                    .insert(info.media_session_type, info.media_session_state);

                storage::set_state(ctx.redis_conn(), self.room, self.id, &self.state)
                    .await
                    .context("Failed to set state attribute in storage")?;

                ctx.invalidate_data();

                if Some(info.media_session_state) != old_state {
                    self.handle_publish_state(info.media_session_type, info.media_session_state)
                        .await?;
                }
            }
            Event::WsMessage(incoming::Message::UpdateMediaSession(info)) => {
                if info.media_session_type == MediaSessionType::Screen
                    && ctx.role() != Role::Moderator
                    && !storage::is_presenter(ctx.redis_conn(), self.room, self.id).await?
                {
                    ctx.ws_send(outgoing::Message::Error(outgoing::Error::PermissionDenied));
                    return Ok(());
                }

                let previous_session_state = self.state.get(&info.media_session_type);

                process_metrics_for_media_session_state(
                    &ctx,
                    &info.media_session_type,
                    &previous_session_state.copied(),
                    &info.media_session_state,
                );

                if let Some(state) = self.state.get_mut(&info.media_session_type) {
                    let old_state = *state;
                    *state = info.media_session_state;

                    storage::set_state(ctx.redis_conn(), self.room, self.id, &self.state)
                        .await
                        .context("Failed to set state attribute in storage")?;

                    ctx.invalidate_data();

                    if info.media_session_state != old_state {
                        self.handle_publish_state(
                            info.media_session_type,
                            info.media_session_state,
                        )
                        .await?;
                    }
                }
            }
            Event::WsMessage(incoming::Message::ModeratorMute(moderator_mute)) => {
                self.handle_moderator_mute(&mut ctx, moderator_mute).await?;
            }
            Event::WsMessage(incoming::Message::Unpublish(assoc)) => {
                self.media.remove_publisher(assoc.media_session_type).await;
                let previous_session_state = self.state.remove(&assoc.media_session_type);

                process_metrics_for_media_session_state(
                    &ctx,
                    &assoc.media_session_type,
                    &previous_session_state,
                    &MediaSessionState {
                        audio: false,
                        video: false,
                    },
                );

                storage::set_state(ctx.redis_conn(), self.room, self.id, &self.state)
                    .await
                    .context("Failed to set state attribute in storage")?;

                ctx.invalidate_data();
            }
            Event::WsMessage(incoming::Message::Publish(targeted)) => {
                if targeted.target.media_session_type == MediaSessionType::Screen
                    && ctx.role() != Role::Moderator
                    && !storage::is_presenter(ctx.redis_conn(), self.room, self.id).await?
                {
                    ctx.ws_send(outgoing::Message::Error(outgoing::Error::PermissionDenied));

                    return Ok(());
                }

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
                    ctx.ws_send(outgoing::Message::Error(outgoing::Error::InvalidSdpOffer));
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
                    ctx.ws_send(outgoing::Message::Error(outgoing::Error::HandleSdpAnswer));
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
                    ctx.ws_send(outgoing::Message::Error(outgoing::Error::InvalidCandidate));
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
                    ctx.ws_send(outgoing::Message::Error(
                        outgoing::Error::InvalidEndOfCandidates,
                    ));
                }
            }
            Event::WsMessage(incoming::Message::Subscribe(subscribe)) => {
                // Check that the subscription target is inside the room
                if !control::storage::participants_contains(
                    ctx.redis_conn(),
                    self.room,
                    subscribe.target.target,
                )
                .await?
                {
                    // just discard, shouldn't happen often
                    return Ok(());
                }

                if let Err(e) = self.handle_sdp_request_offer(&mut ctx, subscribe).await {
                    log::error!(
                        "Failed to handle sdp request-offer {:?}, {:?}",
                        subscribe,
                        e
                    );
                    ctx.ws_send(outgoing::Message::Error(
                        outgoing::Error::InvalidRequestOffer(subscribe.target.into()),
                    ));
                }
            }
            Event::WsMessage(incoming::Message::Configure(configure)) => {
                let target = configure.target;
                if let Err(e) = self.handle_configure(configure).await {
                    log::error!("Failed to handle configure request {:?}", e);
                    ctx.ws_send(outgoing::Message::Error(
                        outgoing::Error::InvalidConfigureRequest(target.into()),
                    ));
                }
            }

            Event::WsMessage(incoming::Message::GrantPresenterRole(selection)) => {
                if ctx.role() != Role::Moderator {
                    ctx.ws_send(outgoing::Message::Error(outgoing::Error::PermissionDenied));

                    return Ok(());
                }

                ctx.rabbitmq_publish(
                    control::rabbitmq::current_room_exchange_name(self.room),
                    control::rabbitmq::room_all_routing_key().into(),
                    rabbitmq::Message::PresenterGranted(selection),
                )
            }
            Event::WsMessage(incoming::Message::RevokePresenterRole(selection)) => {
                if ctx.role() != Role::Moderator {
                    ctx.ws_send(outgoing::Message::Error(outgoing::Error::PermissionDenied));

                    return Ok(());
                }

                ctx.rabbitmq_publish(
                    control::rabbitmq::current_room_exchange_name(self.room),
                    control::rabbitmq::room_all_routing_key().into(),
                    rabbitmq::Message::PresenterRevoked(selection),
                )
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
                WebRtcEvent::Media(media) => {
                    ctx.ws_send(outgoing::Message::Media((media_session_key, media).into()))
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
                WebRtcEvent::Trickle(trickle_msg) => match trickle_msg {
                    // This send by Janus when in full-trickle mode.
                    TrickleMessage::Completed => {
                        ctx.ws_send(outgoing::Message::SdpEndCandidates(
                            media_session_key.into(),
                        ));
                    }
                    TrickleMessage::Candidate(candidate) => {
                        ctx.ws_send(outgoing::Message::SdpCandidate(outgoing::SdpCandidate {
                            candidate,
                            source: media_session_key.into(),
                        }));
                    }
                },
                WebRtcEvent::StartedTalking => ctx.rabbitmq_publish(
                    control::rabbitmq::current_room_exchange_name(self.room),
                    control::rabbitmq::room_all_routing_key().into(),
                    rabbitmq::Message::StartedTalking(media_session_key.0),
                ),
                WebRtcEvent::StoppedTalking => ctx.rabbitmq_publish(
                    control::rabbitmq::current_room_exchange_name(self.room),
                    control::rabbitmq::room_all_routing_key().into(),
                    rabbitmq::Message::StoppedTalking(media_session_key.0),
                ),
            },
            Event::RabbitMq(rabbitmq::Message::StartedTalking(id)) => {
                if let Some(focus) = self.focus_detection.on_started_talking(id) {
                    ctx.ws_send(outgoing::Message::FocusUpdate(outgoing::FocusUpdate {
                        focus,
                    }));
                }
            }
            Event::RabbitMq(rabbitmq::Message::StoppedTalking(id)) => {
                if let Some(focus) = self.focus_detection.on_stopped_talking(id) {
                    ctx.ws_send(outgoing::Message::FocusUpdate(outgoing::FocusUpdate {
                        focus,
                    }));
                }
            }
            Event::RabbitMq(rabbitmq::Message::RequestMute(request_mute)) => {
                ctx.ws_send(outgoing::Message::RequestMute(request_mute));
            }
            Event::RabbitMq(rabbitmq::Message::PresenterGranted(selection)) => {
                if !selection.participant_ids.contains(&self.id) {
                    return Ok(());
                }

                if storage::is_presenter(ctx.redis_conn(), self.room, self.id).await? {
                    // already presenter
                    return Ok(());
                }

                storage::set_presenter(ctx.redis_conn(), self.room, self.id).await?;

                ctx.ws_send(outgoing::Message::PresenterGranted);

                ctx.invalidate_data();
            }
            Event::RabbitMq(rabbitmq::Message::PresenterRevoked(selection)) => {
                if !selection.participant_ids.contains(&self.id) {
                    return Ok(());
                }

                if !storage::is_presenter(ctx.redis_conn(), self.room, self.id).await? {
                    // already not a presenter
                    return Ok(());
                }

                storage::delete_presenter(ctx.redis_conn(), self.room, self.id).await?;

                // terminate screen share
                if self.state.contains_key(&MediaSessionType::Screen)
                    && ctx.role() != Role::Moderator
                {
                    self.media.remove_publisher(MediaSessionType::Screen).await;
                    self.state.remove(&MediaSessionType::Screen);

                    storage::set_state(ctx.redis_conn(), self.room, self.id, &self.state)
                        .await
                        .context("Failed to set state attribute in storage")?;
                }

                ctx.ws_send(outgoing::Message::PresenterRevoked);

                ctx.invalidate_data();
            }

            Event::ParticipantJoined(id, evt_state) => {
                let state = storage::get_state(ctx.redis_conn(), self.room, id)
                    .await
                    .context("Failed to get peer participants state")?;

                let is_presenter = storage::is_presenter(ctx.redis_conn(), self.room, id).await?;

                *evt_state = Some(PeerFrontendData {
                    state,
                    is_presenter,
                })
            }
            Event::ParticipantUpdated(id, evt_state) => {
                let state = if let Some(state) = storage::get_state(ctx.redis_conn(), self.room, id)
                    .await
                    .context("Failed to get peer participants state")?
                {
                    self.media.remove_dangling_subscriber(id, &state).await;

                    if let Some(video_state) = state.get(&MediaSessionType::Video) {
                        if !video_state.audio {
                            if let Some(focus) = self.focus_detection.on_stopped_talking(id) {
                                ctx.ws_send(outgoing::Message::FocusUpdate(
                                    outgoing::FocusUpdate { focus },
                                ));
                            }
                        }
                    }

                    Some(state)
                } else {
                    None
                };

                let is_presenter = storage::is_presenter(ctx.redis_conn(), self.room, id).await?;

                *evt_state = Some(PeerFrontendData {
                    state,
                    is_presenter,
                });
            }
            Event::ParticipantLeft(id) => {
                self.media.remove_subscribers(id).await;

                // Unfocus leaving participants
                if let Some(focus) = self.focus_detection.on_stopped_talking(id) {
                    ctx.ws_send(outgoing::Message::FocusUpdate(outgoing::FocusUpdate {
                        focus,
                    }));
                }
            }
            Event::Joined {
                control_data: _,
                frontend_data,
                participants,
            } => {
                for (&id, evt_state) in participants {
                    let state = storage::get_state(ctx.redis_conn(), self.room, id)
                        .await
                        .context("Failed to get peer participants state")?;

                    let is_presenter =
                        storage::is_presenter(ctx.redis_conn(), self.room, id).await?;

                    *evt_state = Some(PeerFrontendData {
                        state,
                        is_presenter,
                    })
                }

                let is_presenter =
                    storage::is_presenter(ctx.redis_conn(), self.room, self.id).await?;

                *frontend_data = Some(FrontendData { is_presenter })
            }
            Event::Leaving => {
                if let Err(e) = storage::del_state(ctx.redis_conn(), self.room, self.id).await {
                    log::error!(
                        "Media module for {} failed to remove its state data from redis, {}",
                        self.id,
                        e
                    );
                }

                // Spawn destroying all the handles as it doesn't need to be synchronized
                // and should not block the leaving process
                tokio::task::spawn_local(self.media.destroy());
            }
            Event::RaiseHand | Event::LowerHand { .. } => {}
        }

        Ok(())
    }

    async fn on_destroy(self, mut ctx: DestroyContext<'_>) {
        if ctx.destroy_room() {
            if let Err(e) = storage::delete_presenter_key(ctx.redis_conn(), self.room).await {
                log::error!(
                    "Media module for failed to remove presenter key on room destoy, {}",
                    e
                );
            }
        }
    }
}

impl Media {
    /// Send mute requests to the targeted participants
    ///
    /// Fails if the issuing user is not a moderator.
    async fn handle_moderator_mute(
        &self,
        ctx: &mut ModuleContext<'_, Self>,
        moderator_mute: RequestMute,
    ) -> Result<()> {
        if ctx.role() != Role::Moderator {
            ctx.ws_send(outgoing::Message::Error(outgoing::Error::PermissionDenied));

            return Ok(());
        }

        let room_participants =
            control::storage::get_all_participants(ctx.redis_conn(), self.room).await?;

        let request_mute = rabbitmq::RequestMute {
            issuer: self.id,
            force: moderator_mute.force,
        };

        for target in moderator_mute.targets {
            if !room_participants.contains(&target) {
                continue;
            }

            ctx.rabbitmq_publish(
                control::rabbitmq::current_room_exchange_name(self.room),
                control::rabbitmq::room_participant_routing_key(target),
                rabbitmq::Message::RequestMute(request_mute),
            )
        }

        Ok(())
    }

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
            // Get the publisher and create if it doesn't exists
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
            // Get the publisher and create if it doesn't exists
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
        subscribe: incoming::TargetSubscribe,
    ) -> Result<()> {
        let target = subscribe.target.target;
        let media_session_type = subscribe.target.media_session_type;

        if self.id == subscribe.target.target {
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

        let response = subscriber
            .send_message(Request::RequestOffer {
                without_video: subscribe.without_video,
            })
            .await?;

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
    async fn handle_publish_state(
        &mut self,
        media_session_type: MediaSessionType,
        state: MediaSessionState,
    ) -> Result<()> {
        if let Some(publisher) = self.media.get_publisher(media_session_type) {
            publisher
                .send_message(Request::PublisherConfigure(PublishConfiguration {
                    video: state.video,
                    audio: state.audio,
                }))
                .await?;
        } else {
            log::info!("Attempt to configure none existing publisher");
        }

        Ok(())
    }

    #[tracing::instrument(level = "debug", skip(self))]
    async fn handle_configure(&mut self, configure: TargetConfigure) -> Result<()> {
        if let Some(subscriber) = self
            .media
            .get_subscriber(configure.target.target, configure.target.media_session_type)
        {
            subscriber
                .send_message(Request::SubscriberConfigure(configure.configuration))
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
        controller.rabbitmq_pool.make_connection().await?,
        controller.redis.clone(),
        controller.shutdown.subscribe(),
        controller.reload.subscribe(),
    )
    .await?;

    controller.signaling.add_module::<Media>(mcu_pool);

    Ok(())
}

pub fn participants_have_presenter_role(shared_settings: SharedSettings) -> bool {
    shared_settings
        .load()
        .defaults
        .participants_have_presenter_role
}
