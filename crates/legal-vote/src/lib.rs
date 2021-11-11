//! # Legal Vote Module
//!
//! ## Functionality
//!
//! Offers full legal vote features including live voting with high safety guards (atomic changes, audit log).
//! Stores the result for further archival storage in postgres.
use crate::rabbitmq::CancelReason;
use anyhow::Context;
use anyhow::Result;
use chrono::{DateTime, Utc};
use controller::db::legal_votes::VoteId;
use controller::db::users::UserId;
use controller::prelude::futures::stream::once;
use controller::prelude::futures::FutureExt;
use controller::prelude::*;
use db_storage::database::Db;
use db_storage::legal_votes::DbLegalVoteEx;
use error::{Error, ErrorKind};
use incoming::VoteMessage;
use outgoing::{Response, VoteFailed, VoteResponse, VoteResults, VoteSuccess, Votes};
use rabbitmq::{Cancel, Invalid, Parameters, StopKind};
use redis::aio::ConnectionManager;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use storage::protocol;
use storage::protocol::{reduce_protocol, ProtocolEntry, Vote, VoteEvent};
use storage::VoteScriptResult;
use tokio::time::sleep;
use validator::Validate;

mod error;
pub mod incoming;
pub mod outgoing;
pub mod rabbitmq;
mod storage;

/// The vote choices
///
/// Abstain can be disabled through the vote parameters (See [`Parameters`](incoming::UserParameters)).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoteOption {
    Yes,
    No,
    Abstain,
}

impl_to_redis_args_se!(VoteOption);
impl_from_redis_value_de!(VoteOption);

/// A TimerEvent used for the vote expiration feature
pub struct TimerEvent {
    vote_id: VoteId,
}

/// The legal vote [`SignalingModule`]
///
/// Holds a database interface and information about the underlying user & room. Vote information is
/// saved and managed in redis via the private `storage` module.
pub struct LegalVote {
    db_ctx: Arc<Db>,
    participant_id: ParticipantId,
    user_id: UserId,
    room_id: SignalingRoomId,
}

#[async_trait::async_trait(?Send)]
impl SignalingModule for LegalVote {
    const NAMESPACE: &'static str = "legal_vote";
    type Params = ();
    type Incoming = incoming::Message;
    type Outgoing = outgoing::Message;
    type RabbitMqMessage = rabbitmq::Event;
    type ExtEvent = TimerEvent;
    type FrontendData = ();
    type PeerFrontendData = ();

    async fn init(
        ctx: InitContext<'_, Self>,
        _params: &Self::Params,
        _protocol: &'static str,
    ) -> anyhow::Result<Option<Self>> {
        if let Participant::User(user) = ctx.participant() {
            Ok(Some(Self {
                db_ctx: ctx.db().clone(),
                participant_id: ctx.participant_id(),
                user_id: user.id,
                room_id: ctx.room_id(),
            }))
        } else {
            Ok(None)
        }
    }

    async fn on_event(
        &mut self,
        mut ctx: ModuleContext<'_, Self>,
        event: Event<'_, Self>,
    ) -> anyhow::Result<()> {
        match event {
            Event::Joined { .. } => match self.handle_joined(ctx.redis_conn()).await {
                Ok(current_vote_info) => {
                    if let Some((parameters, results)) = current_vote_info {
                        let vote_id = parameters.vote_id;

                        ctx.ws_send(outgoing::Message::Started(parameters));
                        ctx.ws_send(outgoing::Message::Updated(VoteResults { vote_id, results }));
                    }
                }
                Err(error) => self.handle_error(&mut ctx, error)?,
            },
            Event::Leaving => {
                if let Err(error) = self.handle_leaving(&mut ctx).await {
                    self.handle_error(&mut ctx, error)?;
                }
            }

            Event::WsMessage(msg) => {
                if let Err(error) = self.handle_ws_message(&mut ctx, msg).await {
                    self.handle_error(&mut ctx, error)?;
                }
            }
            Event::RabbitMq(event) => {
                if let Err(error) = self.handle_rabbitmq_message(&mut ctx, event).await {
                    self.handle_error(&mut ctx, error)?;
                }
            }
            Event::Ext(timer_event) => {
                let vote_status =
                    storage::get_vote_status(ctx.redis_conn(), self.room_id, timer_event.vote_id)
                        .await?;

                match vote_status {
                    storage::VoteStatus::Active => {
                        let stop_kind = StopKind::Expired;

                        let expired_entry =
                            ProtocolEntry::new(protocol::VoteEvent::Stop(stop_kind));

                        if let Err(error) = self
                            .end_vote(&mut ctx, timer_event.vote_id, expired_entry, stop_kind)
                            .await
                        {
                            match error {
                                Error::Vote(kind) => {
                                    log::error!("Failed to stop expired vote, error: {}", kind)
                                }
                                Error::Fatal(fatal) => {
                                    self.handle_fatal_error(&mut ctx, fatal)?;
                                }
                            }
                        }
                    }
                    storage::VoteStatus::Complete => {
                        // vote got stopped manually already, nothing to do
                    }
                    storage::VoteStatus::Unknown => {
                        log::warn!("Legal vote timer contains an unknown vote id");
                    }
                }
            }

            // ignored events
            Event::RaiseHand
            | Event::LowerHand
            | Event::ParticipantJoined(_, _)
            | Event::ParticipantLeft(_)
            | Event::ParticipantUpdated(_, _) => (),
        }

        Ok(())
    }

    async fn on_destroy(self, mut ctx: DestroyContext<'_>) {
        if ctx.destroy_room() {
            match storage::current_vote_id::get(ctx.redis_conn(), self.room_id).await {
                Ok(current_vote_id) => {
                    if let Some(current_vote_id) = current_vote_id {
                        match self
                            .cancel_vote_unchecked(
                                ctx.redis_conn(),
                                current_vote_id,
                                CancelReason::RoomDestroyed,
                            )
                            .await
                        {
                            Ok(()) => {
                                if let Err(e) = self
                                    .save_protocol_in_database(ctx.redis_conn(), current_vote_id)
                                    .await
                                {
                                    log::error!("failed to save protocol to db {:?}", e)
                                }
                            }
                            Err(e) => log::error!(
                                "Failed to cancel active vote while destroying vote module {:?}",
                                e
                            ),
                        }
                    }
                }
                Err(e) => {
                    log::error!(
                        "Failed to get current vote id while destroying vote module, {:?}",
                        e
                    );
                }
            }

            if let Err(e) = self.cleanup_room(ctx.redis_conn()).await {
                log::error!("Failed to cleanup room on destroy, {:?}", e)
            }
        }
    }
}

impl LegalVote {
    /// Handle websocket messages send from the user
    async fn handle_ws_message(
        &mut self,
        ctx: &mut ModuleContext<'_, Self>,
        msg: incoming::Message,
    ) -> Result<(), Error> {
        msg.validate()?;

        match msg {
            incoming::Message::Start(incoming_parameters) => {
                // todo: check permissions

                let vote_id = self
                    .new_vote_in_database()
                    .await
                    .context("Failed to create new vote in database")?;

                match self
                    .start_vote_routine(ctx.redis_conn(), vote_id, incoming_parameters)
                    .await
                {
                    Ok(rabbitmq_parameters) => {
                        if let Some(duration) = rabbitmq_parameters.inner.duration {
                            ctx.add_event_stream(once(
                                sleep(Duration::from_secs(duration))
                                    .map(move |_| TimerEvent { vote_id }),
                            ));
                        }

                        ctx.rabbitmq_publish(
                            control::rabbitmq::current_room_exchange_name(self.room_id),
                            control::rabbitmq::room_all_routing_key().into(),
                            rabbitmq::Event::Start(rabbitmq_parameters),
                        );
                    }
                    Err(start_error) => {
                        log::warn!("Failed to start vote, {:?}", start_error);

                        // return the cleanup error in case of failure as its more severe
                        storage::cleanup_vote(ctx.redis_conn(), self.room_id, vote_id).await?;

                        return Err(start_error);
                    }
                }
            }
            incoming::Message::Stop(incoming::Stop { vote_id }) => {
                self.stop_vote_routine(ctx, vote_id).await?;
            }
            incoming::Message::Cancel(incoming::Cancel { vote_id, reason }) => {
                self.cancel_vote(ctx.redis_conn(), vote_id, reason.clone())
                    .await?;

                self.save_protocol_in_database(ctx.redis_conn(), vote_id)
                    .await?;

                ctx.rabbitmq_publish(
                    control::rabbitmq::current_room_exchange_name(self.room_id),
                    control::rabbitmq::room_all_routing_key().into(),
                    rabbitmq::Event::Cancel(Cancel {
                        vote_id,
                        reason: rabbitmq::CancelReason::Custom(reason),
                    }),
                );
            }
            incoming::Message::Vote(vote_message) => {
                let (vote_response, auto_stop) = self.cast_vote(ctx, vote_message).await?;

                if let Response::Success(VoteSuccess {
                    vote_option,
                    issuer,
                }) = vote_response.response
                {
                    let update = rabbitmq::Event::Update(rabbitmq::VoteUpdate {
                        vote_id: vote_message.vote_id,
                    });

                    // Send a vote success message to all participants that have the same user id
                    ctx.rabbitmq_publish(
                        control::rabbitmq::current_room_exchange_name(self.room_id),
                        control::rabbitmq::room_user_routing_key(self.user_id),
                        rabbitmq::Event::Voted(rabbitmq::VoteSuccess {
                            vote_id: vote_message.vote_id,
                            vote_option,
                            issuer,
                        }),
                    );

                    ctx.rabbitmq_publish(
                        control::rabbitmq::current_room_exchange_name(self.room_id),
                        control::rabbitmq::room_all_routing_key().into(),
                        update,
                    );

                    if auto_stop {
                        let stop_kind = StopKind::Auto;

                        let auto_stop_entry =
                            protocol::ProtocolEntry::new(VoteEvent::Stop(stop_kind));

                        self.end_vote(ctx, vote_message.vote_id, auto_stop_entry, stop_kind)
                            .await?;
                    }
                } else {
                    ctx.ws_send(outgoing::Message::Voted(vote_response));
                }
            }
        }
        Ok(())
    }

    /// Handle incoming rabbitmq messages
    async fn handle_rabbitmq_message(
        &mut self,
        ctx: &mut ModuleContext<'_, Self>,
        event: rabbitmq::Event,
    ) -> Result<(), Error> {
        match event {
            rabbitmq::Event::Start(parameters) => {
                ctx.ws_send(outgoing::Message::Started(parameters));
            }
            rabbitmq::Event::Stop(stop) => {
                let final_results = match stop.results {
                    rabbitmq::FinalResults::Valid(votes) => {
                        let protocol =
                            storage::protocol::get(ctx.redis_conn(), self.room_id, stop.vote_id)
                                .await?;

                        outgoing::FinalResults::Valid(outgoing::Results {
                            votes,
                            voters: reduce_protocol(protocol),
                        })
                    }
                    rabbitmq::FinalResults::Invalid(invalid) => {
                        outgoing::FinalResults::Invalid(invalid)
                    }
                };

                let stop = outgoing::Stopped {
                    vote_id: stop.vote_id,
                    kind: stop.kind,
                    results: final_results,
                };

                ctx.ws_send(outgoing::Message::Stopped(stop));
            }
            rabbitmq::Event::Voted(vote_success) => {
                ctx.ws_send(outgoing::Message::Voted(VoteResponse {
                    vote_id: vote_success.vote_id,
                    response: outgoing::Response::Success(outgoing::VoteSuccess {
                        vote_option: vote_success.vote_option,
                        issuer: vote_success.issuer,
                    }),
                }))
            }
            rabbitmq::Event::Cancel(cancel) => {
                ctx.ws_send(outgoing::Message::Canceled(outgoing::Canceled {
                    vote_id: cancel.vote_id,
                    reason: cancel.reason,
                }));
            }
            rabbitmq::Event::Update(update) => {
                let results = self
                    .get_vote_results(ctx.redis_conn(), update.vote_id)
                    .await?;

                ctx.ws_send(outgoing::Message::Updated(VoteResults {
                    vote_id: update.vote_id,
                    results,
                }));
            }
            rabbitmq::Event::FatalServerError => {
                ctx.ws_send(outgoing::Message::Error(outgoing::ErrorKind::Internal));
            }
        }
        Ok(())
    }

    /// Set all vote related redis keys
    async fn start_vote_routine(
        &self,
        redis_conn: &mut ConnectionManager,
        vote_id: VoteId,
        incoming_parameters: incoming::UserParameters,
    ) -> Result<rabbitmq::Parameters, Error> {
        let start_time = Utc::now();

        let max_votes = self
            .init_allowed_list(
                redis_conn,
                vote_id,
                &incoming_parameters.allowed_participants,
            )
            .await?;

        let parameters = rabbitmq::Parameters {
            initiator_id: self.participant_id,
            vote_id,
            start_time,
            max_votes,
            inner: incoming_parameters,
        };

        storage::parameters::set(redis_conn, self.room_id, vote_id, &parameters).await?;

        self.init_vote_protocol(redis_conn, vote_id, start_time, parameters.clone())
            .await?;

        if !storage::current_vote_id::set(redis_conn, self.room_id, vote_id).await? {
            return Err(ErrorKind::VoteAlreadyActive.into());
        }

        Ok(parameters)
    }

    /// Add the start entry to the protocol of this vote
    async fn init_vote_protocol(
        &self,
        redis_conn: &mut ConnectionManager,
        vote_id: VoteId,
        start_time: DateTime<Utc>,
        parameters: rabbitmq::Parameters,
    ) -> Result<()> {
        let start_entry = ProtocolEntry::new_with_time(
            start_time,
            VoteEvent::Start(protocol::Start {
                issuer: self.user_id,
                parameters,
            }),
        );

        storage::protocol::add_entry(redis_conn, self.room_id, vote_id, start_entry).await?;

        Ok(())
    }

    /// Set the allowed users list for the provided `vote_id` to its initial state
    ///
    /// Returns the maximum number of possible votes
    async fn init_allowed_list(
        &self,
        redis_conn: &mut ConnectionManager,
        vote_id: VoteId,
        allowed_participants: &[ParticipantId],
    ) -> Result<u32, Error> {
        let allowed_users = control::storage::get_attribute_for_participants::<UserId>(
            redis_conn,
            self.room_id,
            "user_id",
            allowed_participants,
        )
        .await?;

        let mut invalid_participants = Vec::new();
        let mut users = Vec::new();

        for (index, maybe_user_id) in allowed_users.into_iter().enumerate() {
            match maybe_user_id {
                Some(user_id) => {
                    if !users.contains(&user_id) {
                        users.push(user_id)
                    }
                }
                None => {
                    if let Some(participant) = allowed_participants.get(index).copied() {
                        invalid_participants.push(participant);
                    } else {
                        // this should never occur
                        log::error!(
                            "Inconsistency in legalvote when checking allowed participants"
                        );
                        return Err(Error::Vote(ErrorKind::Inconsistency));
                    }
                }
            }
        }

        if !invalid_participants.is_empty() {
            return Err(Error::Vote(ErrorKind::AllowlistContainsGuests(
                invalid_participants,
            )));
        }

        let max_votes = users.len();

        storage::allowed_users::set(redis_conn, self.room_id, vote_id, users).await?;

        Ok(max_votes as u32)
    }

    /// Stop a vote
    ///
    /// Checks if the provided `vote_id` is currently active & if the participant is the initiator
    /// before stopping the vote.
    /// Fails with `VoteError::InvalidVoteId` when the provided `vote_id` does not match the active vote id.
    /// Adds a `ProtocolEntry` with `VoteEvent::Stop(Stop::UserStop(<user_id>))` to the vote protocol when successful.
    async fn stop_vote_routine(
        &self,
        ctx: &mut ModuleContext<'_, LegalVote>,
        vote_id: VoteId,
    ) -> Result<(), Error> {
        if !self.is_current_vote_id(ctx.redis_conn(), vote_id).await? {
            return Err(ErrorKind::InvalidVoteId.into());
        }

        if !self.is_vote_initiator(ctx.redis_conn(), vote_id).await? {
            return Err(ErrorKind::Ineligible.into());
        }

        let stop_kind = StopKind::ByParticipant(self.participant_id);

        let stop_entry = ProtocolEntry::new(VoteEvent::Stop(stop_kind));

        self.end_vote(ctx, vote_id, stop_entry, stop_kind).await?;

        Ok(())
    }

    /// Cast a vote
    ///
    /// Checks if the provided `vote_message` contains valid values & calls [`storage::vote`].
    /// See [`storage::vote`] & [`storage::VOTE_SCRIPT`] for more details on the vote process.
    ///
    /// # Returns
    /// - Ok([`VoteResponse`], <should_auto_stop>) in case of successfully executing [`storage::vote`].
    ///   Ok contains a tuple with the VoteResponse and a boolean that indicates that this was the last
    ///   vote needed and this vote can now be auto stopped. The boolean can only be true when this feature is enabled.
    /// - Err([`Error`]) in case of an redis error.
    async fn cast_vote(
        &self,
        ctx: &mut ModuleContext<'_, Self>,
        vote_message: VoteMessage,
    ) -> Result<(VoteResponse, bool), Error> {
        let redis_conn = ctx.redis_conn();

        match self
            .is_current_vote_id(redis_conn, vote_message.vote_id)
            .await
        {
            Ok(is_current) => {
                if !is_current {
                    return Ok((
                        VoteResponse {
                            vote_id: vote_message.vote_id,
                            response: Response::Failed(VoteFailed::InvalidVoteId),
                        },
                        false,
                    ));
                }
            }
            Err(error) => {
                if let Error::Vote(_) = error {
                    return Ok((
                        VoteResponse {
                            vote_id: vote_message.vote_id,
                            response: Response::Failed(VoteFailed::InvalidVoteId),
                        },
                        false,
                    ));
                } else {
                    return Err(error);
                }
            }
        }

        let parameters =
            match storage::parameters::get(redis_conn, self.room_id, vote_message.vote_id).await? {
                Some(parameters) => parameters,
                None => {
                    return Ok((
                        VoteResponse {
                            vote_id: vote_message.vote_id,
                            response: Response::Failed(VoteFailed::InvalidVoteId),
                        },
                        false,
                    ));
                }
            };

        if vote_message.option == VoteOption::Abstain && !parameters.inner.enable_abstain {
            return Ok((
                VoteResponse {
                    vote_id: vote_message.vote_id,
                    response: Response::Failed(VoteFailed::InvalidOption),
                },
                false,
            ));
        }

        let vote_event = Vote {
            issuer: self.user_id,
            participant_id: self.participant_id,
            option: vote_message.option,
        };

        let vote_result = storage::vote(
            redis_conn,
            self.room_id,
            vote_message.vote_id,
            self.user_id,
            vote_event,
        )
        .await?;

        let (response, should_auto_stop) = match vote_result {
            VoteScriptResult::Success => (
                Response::Success(VoteSuccess {
                    vote_option: vote_message.option,
                    issuer: self.participant_id,
                }),
                false,
            ),
            VoteScriptResult::SuccessAutoStop => (
                Response::Success(VoteSuccess {
                    vote_option: vote_message.option,
                    issuer: self.participant_id,
                }),
                parameters.inner.auto_stop,
            ),
            VoteScriptResult::InvalidVoteId => (Response::Failed(VoteFailed::InvalidVoteId), false),
            VoteScriptResult::Ineligible => (Response::Failed(VoteFailed::Ineligible), false),
        };

        Ok((
            VoteResponse {
                vote_id: vote_message.vote_id,
                response,
            },
            should_auto_stop,
        ))
    }

    /// Check if the provided `vote_id` equals the current active vote id
    ///
    /// Returns [`ErrorKind::NoVoteActive`] when no vote is active.
    async fn is_current_vote_id(
        &self,
        redis_conn: &mut ConnectionManager,
        vote_id: VoteId,
    ) -> Result<bool, Error> {
        if let Some(current_vote_id) =
            storage::current_vote_id::get(redis_conn, self.room_id).await?
        {
            Ok(current_vote_id == vote_id)
        } else {
            Err(ErrorKind::NoVoteActive.into())
        }
    }

    /// Check if the participant is the vote initiator of the provided `vote_id`
    async fn is_vote_initiator(
        &self,
        redis_conn: &mut ConnectionManager,
        vote_id: VoteId,
    ) -> Result<bool, Error> {
        let parameters = storage::parameters::get(redis_conn, self.room_id, vote_id)
            .await?
            .ok_or(ErrorKind::InvalidVoteId)?;

        Ok(parameters.initiator_id == self.participant_id)
    }

    /// Cancel the active vote if the leaving participant is the initiator
    async fn handle_leaving(&self, ctx: &mut ModuleContext<'_, Self>) -> Result<(), Error> {
        let redis_conn = ctx.redis_conn();

        let current_vote_id = match storage::current_vote_id::get(redis_conn, self.room_id).await? {
            Some(current_vote_id) => current_vote_id,
            None => return Ok(()),
        };

        let parameters = storage::parameters::get(redis_conn, self.room_id, current_vote_id)
            .await?
            .ok_or(ErrorKind::InvalidVoteId)?;

        if parameters.initiator_id == self.participant_id {
            let reason = CancelReason::InitiatorLeft;

            self.cancel_vote_unchecked(redis_conn, current_vote_id, reason.clone())
                .await?;

            self.save_protocol_in_database(ctx.redis_conn(), current_vote_id)
                .await?;

            ctx.rabbitmq_publish(
                control::rabbitmq::current_room_exchange_name(self.room_id),
                control::rabbitmq::room_all_routing_key().into(),
                rabbitmq::Event::Cancel(rabbitmq::Cancel {
                    vote_id: current_vote_id,
                    reason,
                }),
            );
        }

        Ok(())
    }

    /// Get the vote results for the specified `vote_id`
    async fn get_vote_results(
        &self,
        redis_conn: &mut ConnectionManager,
        vote_id: VoteId,
    ) -> Result<outgoing::Results, Error> {
        let parameters = storage::parameters::get(redis_conn, self.room_id, vote_id)
            .await?
            .ok_or(ErrorKind::InvalidVoteId)?;

        let votes = storage::vote_count::get(
            redis_conn,
            self.room_id,
            vote_id,
            parameters.inner.enable_abstain,
        )
        .await?;

        let protocol = storage::protocol::get(redis_conn, self.room_id, vote_id).await?;

        Ok(outgoing::Results {
            votes,
            voters: reduce_protocol(protocol),
        })
    }

    /// Cancel a vote
    ///
    /// Checks if the provided vote id is currently active & if the participant is the initiator
    /// before canceling the vote.
    async fn cancel_vote(
        &self,
        redis_conn: &mut ConnectionManager,
        vote_id: VoteId,
        reason: String,
    ) -> Result<(), Error> {
        if !self.is_current_vote_id(redis_conn, vote_id).await? {
            return Err(ErrorKind::InvalidVoteId.into());
        }

        if !self.is_vote_initiator(redis_conn, vote_id).await? {
            return Err(ErrorKind::Ineligible.into());
        }

        self.cancel_vote_unchecked(redis_conn, vote_id, CancelReason::Custom(reason))
            .await
    }

    /// Cancel a vote without checking permissions
    ///
    /// Fails with `VoteError::InvalidVoteId` when the provided `vote_id` does not match the active vote id.
    /// Adds a `ProtocolEntry` with `VoteEvent::Cancel` to the vote protocol when successful.
    async fn cancel_vote_unchecked(
        &self,
        redis_conn: &mut ConnectionManager,
        vote_id: VoteId,
        reason: CancelReason,
    ) -> Result<(), Error> {
        let cancel_entry = ProtocolEntry::new(VoteEvent::Cancel(protocol::Cancel {
            issuer: self.user_id,
            reason,
        }));

        if !storage::end_current_vote(redis_conn, self.room_id, vote_id, cancel_entry).await? {
            return Err(ErrorKind::InvalidVoteId.into());
        }

        Ok(())
    }

    /// End the vote behind `vote_id` using the provided parameters as stop parameters
    async fn end_vote(
        &self,
        ctx: &mut ModuleContext<'_, Self>,
        vote_id: VoteId,
        end_entry: ProtocolEntry,
        stop_kind: StopKind,
    ) -> Result<(), Error> {
        if !storage::end_current_vote(ctx.redis_conn(), self.room_id, vote_id, end_entry).await? {
            return Err(ErrorKind::InvalidVoteId.into());
        }

        let final_results = self.validate_vote_results(ctx, vote_id).await?;

        let result_entry = ProtocolEntry::new(VoteEvent::FinalResults(final_results));

        protocol::add_entry(ctx.redis_conn(), self.room_id, vote_id, result_entry).await?;

        self.save_protocol_in_database(ctx.redis_conn(), vote_id)
            .await?;

        ctx.rabbitmq_publish(
            control::rabbitmq::current_room_exchange_name(self.room_id),
            control::rabbitmq::room_all_routing_key().into(),
            rabbitmq::Event::Stop(rabbitmq::Stop {
                vote_id,
                kind: stop_kind,
                results: final_results,
            }),
        );

        Ok(())
    }

    /// Checks if the vote results for `vote_id` are equal to the protocols vote entries.
    async fn validate_vote_results(
        &self,
        ctx: &mut ModuleContext<'_, Self>,
        vote_id: VoteId,
    ) -> Result<rabbitmq::FinalResults, Error> {
        let parameters = storage::parameters::get(ctx.redis_conn(), self.room_id, vote_id)
            .await?
            .ok_or(ErrorKind::InvalidVoteId)?;

        let protocol = storage::protocol::get(ctx.redis_conn(), self.room_id, vote_id).await?;
        let voters = reduce_protocol(protocol);

        let mut protocol_vote_count = Votes {
            yes: 0,
            no: 0,
            abstain: {
                if parameters.inner.enable_abstain {
                    Some(0)
                } else {
                    None
                }
            },
        };

        let mut total_votes = 0;

        for (_, vote_option) in voters {
            total_votes += 1;

            match vote_option {
                VoteOption::Yes => protocol_vote_count.yes += 1,
                VoteOption::No => protocol_vote_count.no += 1,
                VoteOption::Abstain => {
                    if let Some(abstain) = &mut protocol_vote_count.abstain {
                        *abstain += 1;
                    } else {
                        return Ok(rabbitmq::FinalResults::Invalid(Invalid::AbstainDisabled));
                    }
                }
            }
        }

        let vote_count = storage::vote_count::get(
            ctx.redis_conn(),
            self.room_id,
            vote_id,
            parameters.inner.enable_abstain,
        )
        .await?;

        if protocol_vote_count == vote_count && total_votes <= parameters.max_votes {
            Ok(rabbitmq::FinalResults::Valid(vote_count))
        } else {
            Ok(rabbitmq::FinalResults::Invalid(
                Invalid::VoteCountInconsistent,
            ))
        }
    }

    /// Remove the all vote related redis keys belonging to this room
    async fn cleanup_room(&self, redis_conn: &mut ConnectionManager) -> Result<()> {
        let vote_history = storage::history::get(redis_conn, self.room_id).await?;

        for vote_id in vote_history.iter() {
            storage::cleanup_vote(redis_conn, self.room_id, *vote_id).await?
        }

        storage::history::delete(redis_conn, self.room_id).await?;

        if let Some(current_vote_id) =
            storage::current_vote_id::get(redis_conn, self.room_id).await?
        {
            storage::cleanup_vote(redis_conn, self.room_id, current_vote_id).await?;
            storage::current_vote_id::delete(redis_conn, self.room_id).await?;
        }

        Ok(())
    }

    /// Creates a new vote in the database
    ///
    /// Adds a new vote with an empty protocol to the database. Returns the [`VoteId`] of the new vote.
    async fn new_vote_in_database(&self) -> Result<VoteId> {
        let empty_protocol = serde_json::to_value(Vec::<ProtocolEntry>::new())
            .context("Unable to serialize empty protocol")?;

        let db_ctx = self.db_ctx.clone();

        let user_id = self.user_id;

        let legal_vote =
            controller::block(move || db_ctx.new_legal_vote(user_id, empty_protocol)).await??;

        Ok(legal_vote.id)
    }

    /// Save the protocol for `vote_id` in the database
    async fn save_protocol_in_database(
        &self,
        redis_conn: &mut ConnectionManager,
        vote_id: VoteId,
    ) -> Result<()> {
        let protocol = storage::protocol::get(redis_conn, self.room_id, vote_id).await?;

        let protocol = serde_json::to_value(protocol).context("Unable to serialize protocol")?;

        let db_ctx = self.db_ctx.clone();

        controller::block(move || db_ctx.set_protocol(vote_id, protocol)).await??;

        Ok(())
    }

    /// Returns the parameters and results of the current vote
    async fn handle_joined(
        &self,
        redis_conn: &mut ConnectionManager,
    ) -> Result<Option<(Parameters, outgoing::Results)>, Error> {
        if let Some(current_vote_id) =
            storage::current_vote_id::get(redis_conn, self.room_id).await?
        {
            let vote_parameters =
                storage::parameters::get(redis_conn, self.room_id, current_vote_id)
                    .await?
                    .ok_or(ErrorKind::InvalidVoteId)?;

            let vote_results = self.get_vote_results(redis_conn, current_vote_id).await?;

            Ok(Some((vote_parameters, vote_results)))
        } else {
            Ok(None)
        }
    }

    /// Check the provided `error` and handles the error cases
    fn handle_error(&self, ctx: &mut ModuleContext<'_, Self>, error: Error) -> Result<()> {
        match error {
            Error::Vote(error_kind) => {
                log::debug!("Error in legal_vote module {:?}", error_kind);
                ctx.ws_send(outgoing::Message::Error(error_kind.into()));
                Ok(())
            }
            Error::Fatal(fatal) => {
                // todo: redis errors should be handled by the controller and not this module
                self.handle_fatal_error(ctx, fatal)
            }
        }
    }

    /// Handle a fatal error
    ///
    /// Send a `FatalServerError` message to all participants and add context to the error
    fn handle_fatal_error(
        &self,
        ctx: &mut ModuleContext<'_, Self>,
        fatal: anyhow::Error,
    ) -> Result<()> {
        ctx.rabbitmq_publish(
            control::rabbitmq::current_room_exchange_name(self.room_id),
            control::rabbitmq::room_all_routing_key().into(),
            rabbitmq::Event::FatalServerError,
        );

        Err(fatal.context("Fatal error in legal_vote module"))
    }
}

pub fn register(controller: &mut controller::Controller) {
    controller.signaling.add_module::<LegalVote>(());
}
