use anyhow::Context;
use anyhow::Result;
use chrono::{DateTime, Utc};
use controller::db::legal_votes::VoteId;
use controller::db::users::UserId;
use controller::{db::DbInterface, prelude::*};
use error::{Error, ErrorKind};
use incoming::Stop;
use incoming::VoteMessage;
use outgoing::VoteResponse;
use outgoing::{Response, VoteFailed, VoteResults};
use rabbitmq::{Cancel, Parameters, StopVote};
use redis::aio::ConnectionManager;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use storage::protocol::{reduce_protocol, ProtocolEntry, VoteEvent};
use storage::VoteScriptResult;

mod error;
pub mod incoming;
pub mod outgoing;
pub mod rabbitmq;
mod storage;

/// The vote choices
///
/// Abstain can be disabled through the vote parameters (See [`Parameters`](incoming::Parameters)).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoteOption {
    Yes,
    No,
    Abstain,
}

impl_to_redis_args_se!(VoteOption);
impl_from_redis_value_de!(VoteOption);

/// The legal vote [`SignalingModule`]
///
/// Holds a database interface and information about the underlying user & room. Vote information is
/// saved and managed in redis via the [`storage`] module.
pub struct LegalVote {
    db_ctx: Arc<DbInterface>,
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
    type ExtEvent = ();
    type FrontendData = ();
    type PeerFrontendData = ();

    async fn init(
        ctx: InitContext<'_, Self>,
        _params: &Self::Params,
        _protocol: &'static str,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            db_ctx: ctx.db().clone(),
            participant_id: ctx.participant_id(),
            user_id: ctx.user().id,
            room_id: ctx.room_id(),
        })
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
                        ctx.ws_send(outgoing::Message::Started(parameters));
                        ctx.ws_send(outgoing::Message::Updated(results));
                    }
                }
                Err(error) => self.handle_error(ctx, error)?,
            },
            Event::Leaving => {
                if let Err(error) = self.handle_leaving(&mut ctx).await {
                    self.handle_error(ctx, error)?;
                }
            }

            Event::WsMessage(msg) => {
                if let Err(error) = self.handle_ws_message(&mut ctx, msg).await {
                    self.handle_error(ctx, error)?;
                }
            }
            Event::RabbitMq(event) => {
                if let Err(error) = self.handle_rabbitmq_message(&mut ctx, event).await {
                    self.handle_error(ctx, error)?;
                }
            }
            // ignored events
            Event::Ext(_)
            | Event::RaiseHand
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
                                storage::protocol::Reason::RoomDestroyed,
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
            incoming::Message::Stop(Stop { vote_id }) => {
                self.stop_vote(ctx.redis_conn(), vote_id).await?;

                self.save_protocol_in_database(ctx.redis_conn(), vote_id)
                    .await?;

                ctx.rabbitmq_publish(
                    control::rabbitmq::current_room_exchange_name(self.room_id),
                    control::rabbitmq::room_all_routing_key().into(),
                    rabbitmq::Event::Stop(StopVote { vote_id }),
                );
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
                        reason: rabbitmq::Reason::Custom(reason),
                    }),
                );
            }
            incoming::Message::Vote(vote_message) => {
                let vote_response = self.cast_vote(ctx.redis_conn(), vote_message).await?;

                ctx.ws_send(outgoing::Message::Voted(vote_response));

                if vote_response.response == Response::Success {
                    let update = rabbitmq::Event::Update(rabbitmq::VoteUpdate {
                        vote_id: vote_message.vote_id,
                    });

                    ctx.rabbitmq_publish(
                        control::rabbitmq::current_room_exchange_name(self.room_id),
                        control::rabbitmq::room_all_routing_key().into(),
                        update,
                    );
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
                let results = self
                    .get_vote_results(ctx.redis_conn(), stop.vote_id)
                    .await?;

                ctx.ws_send(outgoing::Message::Stopped(results));
            }
            rabbitmq::Event::Cancel(cancel) => {
                ctx.ws_send(outgoing::Message::Canceled(outgoing::Canceled {
                    vote_id: cancel.vote_id,
                    reason: cancel.reason.into(),
                }));
            }
            rabbitmq::Event::Update(update) => {
                let results = self
                    .get_vote_results(ctx.redis_conn(), update.vote_id)
                    .await?;

                ctx.ws_send(outgoing::Message::Updated(results));
            }
            rabbitmq::Event::FatalServerError => {
                ctx.ws_send(outgoing::Message::Error(outgoing::ErrorKind::Internal));
                // todo: set module state to 'broken' & deny further requests until redis is back?
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

        self.init_allowed_list(
            redis_conn,
            vote_id,
            &incoming_parameters.allowed_participants,
        )
        .await?;

        let parameters = rabbitmq::Parameters {
            initiator_id: self.participant_id,
            vote_id,
            start_time,
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
            self.user_id,
            self.participant_id,
            VoteEvent::Start(parameters),
        );

        storage::protocol::add_entry(redis_conn, self.room_id, vote_id, start_entry).await?;

        Ok(())
    }

    /// Set the allowed users list for the provided `vote_id` to its initial state
    async fn init_allowed_list(
        &self,
        redis_conn: &mut ConnectionManager,
        vote_id: VoteId,
        allowed_participants: &[ParticipantId],
    ) -> Result<()> {
        let allowed_users = control::storage::get_attribute_for_participants::<UserId>(
            redis_conn,
            self.room_id,
            "user_id",
            allowed_participants,
        )
        .await?;

        let allowed_users = allowed_users.into_iter().flatten().collect::<Vec<UserId>>();

        storage::allowed_users::set(redis_conn, self.room_id, vote_id, allowed_users).await?;

        Ok(())
    }

    /// Cast a vote
    ///
    /// Checks if the provided `vote_message` contains valid values & calls [`storage::vote`].
    /// See [`storage::vote`] & [`storage::VOTE_SCRIPT`] for more details on the vote process.
    ///
    /// # Returns
    /// - Ok([`VoteResponse`]) in case of successfully executing [`storage::vote`], this does not necessarily mean
    ///   that the vote itself was successful.
    /// - Err([`Error`]) in case of an redis error or invalid `vote_message` values.
    async fn cast_vote(
        &self,
        redis_conn: &mut ConnectionManager,
        vote_message: VoteMessage,
    ) -> Result<VoteResponse, Error> {
        if !self
            .is_current_vote_id(redis_conn, vote_message.vote_id)
            .await?
        {
            return Ok(VoteResponse {
                vote_id: vote_message.vote_id,
                response: Response::Failed(VoteFailed::InvalidVoteId),
            });
        }

        let parameters =
            match storage::parameters::get(redis_conn, self.room_id, vote_message.vote_id).await? {
                Some(parameters) => parameters,
                None => {
                    return Ok(VoteResponse {
                        vote_id: vote_message.vote_id,
                        response: Response::Failed(VoteFailed::InvalidVoteId),
                    });
                }
            };

        if vote_message.option == VoteOption::Abstain && !parameters.inner.enable_abstain {
            return Ok(VoteResponse {
                vote_id: vote_message.vote_id,
                response: Response::Failed(VoteFailed::InvalidOption),
            });
        }

        let vote_result = storage::vote(
            redis_conn,
            self.room_id,
            vote_message.vote_id,
            self.user_id,
            self.participant_id,
            vote_message.option,
        )
        .await?;

        let response = match vote_result {
            VoteScriptResult::Success => Response::Success,
            VoteScriptResult::InvalidVoteId => Response::Failed(VoteFailed::InvalidVoteId),
            VoteScriptResult::Ineligible => Response::Failed(VoteFailed::Ineligible),
        };

        Ok(VoteResponse {
            vote_id: vote_message.vote_id,
            response,
        })
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
            //todo: only cancel if the setting indicate it?

            self.cancel_vote_unchecked(
                redis_conn,
                current_vote_id,
                storage::protocol::Reason::InitiatorLeft,
            )
            .await?;

            self.save_protocol_in_database(ctx.redis_conn(), current_vote_id)
                .await?;

            ctx.rabbitmq_publish(
                control::rabbitmq::current_room_exchange_name(self.room_id),
                control::rabbitmq::room_all_routing_key().into(),
                rabbitmq::Event::Cancel(rabbitmq::Cancel {
                    vote_id: current_vote_id,
                    reason: rabbitmq::Reason::InitiatorLeft,
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
    ) -> Result<outgoing::VoteResults, Error> {
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

        // todo: Do we want to check if the reduced sum is equal to the vote_count? need to write lua script for that as the state can change between redis calls
        if parameters.inner.secret {
            Ok(outgoing::VoteResults {
                vote_id,
                votes,
                voters: None,
            })
        } else {
            let protocol = storage::protocol::get(redis_conn, self.room_id, vote_id).await?;

            Ok(outgoing::VoteResults {
                vote_id,
                votes,
                voters: Some(reduce_protocol(protocol)),
            })
        }
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

        self.cancel_vote_unchecked(
            redis_conn,
            vote_id,
            storage::protocol::Reason::Custom(reason),
        )
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
        reason: storage::protocol::Reason,
    ) -> Result<(), Error> {
        let cancel_entry =
            ProtocolEntry::new(self.user_id, self.participant_id, VoteEvent::Cancel(reason));

        if !storage::end_current_vote(redis_conn, self.room_id, vote_id, cancel_entry).await? {
            return Err(ErrorKind::InvalidVoteId.into());
        }

        Ok(())
    }

    /// Stop a vote
    ///
    /// Checks if the provided `vote_id` is currently active & if the participant is the initiator
    /// before stopping the vote.
    /// Fails with `VoteError::InvalidVoteId` when the provided `vote_id` does not match the active vote id.
    /// Adds a `ProtocolEntry` with `VoteEvent::Stop` to the vote protocol when successful.
    async fn stop_vote(
        &self,
        redis_conn: &mut ConnectionManager,
        vote_id: VoteId,
    ) -> Result<(), Error> {
        if !self.is_current_vote_id(redis_conn, vote_id).await? {
            return Err(ErrorKind::InvalidVoteId.into());
        }

        if !self.is_vote_initiator(redis_conn, vote_id).await? {
            return Err(ErrorKind::Ineligible.into());
        }

        let stop_entry = ProtocolEntry::new(self.user_id, self.participant_id, VoteEvent::Stop);

        if !storage::end_current_vote(redis_conn, self.room_id, vote_id, stop_entry).await? {
            return Err(ErrorKind::InvalidVoteId.into());
        }

        Ok(())
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
    ) -> Result<Option<(Parameters, VoteResults)>, Error> {
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
    fn handle_error(&self, mut ctx: ModuleContext<'_, Self>, error: Error) -> Result<()> {
        match error {
            Error::Vote(error_kind) => {
                log::debug!("Error in legal_vote module {:?}", error_kind);
                ctx.ws_send(outgoing::Message::Error(error_kind.into()));
                Ok(())
            }
            Error::Fatal(fatal) => {
                // todo: error handling in case of redis error
                ctx.rabbitmq_publish(
                    control::rabbitmq::current_room_exchange_name(self.room_id),
                    control::rabbitmq::room_all_routing_key().into(),
                    rabbitmq::Event::FatalServerError,
                );

                Err(fatal.context("Fatal error in legal_vote module"))
            }
        }
    }
}

pub fn register(controller: &mut controller::Controller) {
    controller.signaling.add_module::<LegalVote>(());
}
