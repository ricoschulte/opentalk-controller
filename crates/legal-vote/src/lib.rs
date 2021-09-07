use anyhow::Context;
use anyhow::Result;
use chrono::{DateTime, Utc};
use controller::db::legal_votes::VoteId;
use controller::db::users::UserId;
use controller::{db::DbInterface, prelude::*};
use error::{Error, ErrorKind};
use incoming::Stop;
use incoming::VoteMessage;
use outgoing::{VoteFailed, VoteResponse, VoteResults};
use rabbitmq::{Cancel, Parameters, StopVote};
use redis::aio::ConnectionManager;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use storage::protocol::{reduce_protocol, ProtocolEntry, VoteEvent};
use storage::VoteScriptResult;

mod error;
mod incoming;
mod outgoing;
mod rabbitmq;
mod storage;

/// The vote choices
///
/// Abstain can be disabled through the vote parameters (See [`Parameters`](incoming::Parameters)).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum VoteOption {
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
struct LegalVote {
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
                        ctx.ws_send(outgoing::Message::Start(parameters));
                        ctx.ws_send(outgoing::Message::Update(results));
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

                ctx.ws_send(outgoing::Message::VoteResponse(vote_response));

                if vote_response == VoteResponse::Success {
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
                ctx.ws_send(outgoing::Message::Start(parameters));
            }
            rabbitmq::Event::Stop(stop) => {
                let results = self
                    .get_vote_results(ctx.redis_conn(), stop.vote_id)
                    .await?;

                ctx.ws_send(outgoing::Message::Stop(results));
            }
            rabbitmq::Event::Cancel(cancel) => {
                ctx.ws_send(outgoing::Message::Cancel(outgoing::Cancel {
                    vote_id: cancel.vote_id,
                    reason: cancel.reason.into(),
                }));
            }
            rabbitmq::Event::Update(update) => {
                let results = self
                    .get_vote_results(ctx.redis_conn(), update.vote_id)
                    .await?;

                ctx.ws_send(outgoing::Message::Update(results));
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
            return Err(ErrorKind::InvalidVoteId.into());
        }

        let parameters = storage::parameters::get(redis_conn, self.room_id, vote_message.vote_id)
            .await?
            .ok_or(ErrorKind::InvalidVoteId)?;

        if vote_message.option == VoteOption::Abstain && !parameters.inner.enable_abstain {
            return Ok(VoteResponse::Failed(VoteFailed::InvalidOption));
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

        Ok(match vote_result {
            VoteScriptResult::Success => VoteResponse::Success,
            VoteScriptResult::InvalidVoteId => VoteResponse::Failed(VoteFailed::InvalidVoteId),
            VoteScriptResult::Ineligible => VoteResponse::Failed(VoteFailed::Ineligible),
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

        let vote_count = storage::vote_count::get(redis_conn, self.room_id, vote_id).await?;

        // todo: Do we want to check if the reduced sum is equal to the vote_count?
        let results = if parameters.inner.secret {
            let secret_results = outgoing::SecretResults { votes: vote_count };

            outgoing::Results::Secret(secret_results)
        } else {
            let protocol = storage::protocol::get(redis_conn, self.room_id, vote_id).await?;

            let public_results = outgoing::PublicResults {
                votes: vote_count,
                voters: reduce_protocol(protocol),
            };

            outgoing::Results::Public(public_results)
        };

        Ok(outgoing::VoteResults { vote_id, results })
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

#[cfg(test)]
mod test {
    use super::*;
    use controller::db::migrations::migrate_from_url;
    use controller::db::rooms::RoomId;
    use serial_test::serial;
    use uuid::Uuid;

    const VOTE_ID: VoteId = VoteId::from(uuid::Uuid::from_u128(1000));
    const ROOM_ID: SignalingRoomId =
        SignalingRoomId::new_test(RoomId::from(uuid::Uuid::from_u128(2000)));

    const USER_1: UserId = UserId::from(1);
    const PARTICIPANT_1: ParticipantId = ParticipantId::new_test(1);

    const USER_2: UserId = UserId::from(2);
    const PARTICIPANT_2: ParticipantId = ParticipantId::new_test(2);

    const USER_3: UserId = UserId::from(3);
    const PARTICIPANT_3: ParticipantId = ParticipantId::new_test(3);

    async fn setup_redis() -> ConnectionManager {
        let redis_url =
            std::env::var("REDIS_ADDR").unwrap_or_else(|_| "redis://0.0.0.0:6379/".to_owned());
        let redis = redis::Client::open(redis_url).expect("Invalid redis url");

        let mut redis_conn = ConnectionManager::new(redis).await.unwrap();

        redis::cmd("FLUSHALL")
            .query_async::<_, ()>(&mut redis_conn)
            .await
            .unwrap();

        control::storage::set_attribute(&mut redis_conn, ROOM_ID, PARTICIPANT_1, "user_id", USER_1)
            .await
            .unwrap();
        control::storage::set_attribute(&mut redis_conn, ROOM_ID, PARTICIPANT_2, "user_id", USER_2)
            .await
            .unwrap();
        control::storage::set_attribute(&mut redis_conn, ROOM_ID, PARTICIPANT_3, "user_id", USER_3)
            .await
            .unwrap();

        redis_conn
    }

    async fn setup_postgres() -> Arc<DbInterface> {
        let database_url = std::env::var("DATABASE_URL")
            .unwrap_or_else(|_| "postgres://postgres:password123@localhost:5432/k3k".to_owned());

        migrate_from_url(&database_url).await.unwrap();

        Arc::new(DbInterface::connect_url(&database_url, 5, None).unwrap())
    }

    fn add_test_user_to_db(db_ctx: Arc<DbInterface>) {
        let new_user = controller::db::users::NewUser {
            oidc_uuid: Uuid::new_v4(),
            email: "test@invalid-mx.heinlein-video.de".into(),
            title: "".into(),
            firstname: "test".into(),
            lastname: "tester".into(),
            id_token_exp: 0,
            theme: "".into(),
            language: "en".into(),
        };

        let new_user_with_groups = controller::db::users::NewUserWithGroups {
            new_user,
            groups: vec![],
        };

        if let Ok(None) = db_ctx.get_user_by_id(UserId::from(1)) {
            let _ = db_ctx.create_user(new_user_with_groups);
        }
    }

    async fn count_redis_keys(redis_conn: &mut ConnectionManager) -> i64 {
        redis::cmd("DBSIZE").query_async(redis_conn).await.unwrap()
    }

    #[tokio::test]
    #[serial]
    async fn vote_start() {
        let mut redis_conn = setup_redis().await;
        let db_ctx = setup_postgres().await;

        let module = LegalVote {
            db_ctx,
            participant_id: PARTICIPANT_1,
            user_id: USER_1,
            room_id: ROOM_ID,
        };

        let incoming_parameters = incoming::UserParameters {
            name: "TestVote".into(),
            topic: "Yes or No?".into(),
            allowed_participants: vec![PARTICIPANT_1, PARTICIPANT_2, PARTICIPANT_3],
            enable_abstain: false,
            secret: false,
            auto_stop: false,
            duration: None,
        };

        module
            .start_vote_routine(&mut redis_conn, VOTE_ID, incoming_parameters.clone())
            .await
            .unwrap();

        module
            .is_vote_initiator(&mut redis_conn, VOTE_ID)
            .await
            .unwrap();

        let current_vote_id = storage::current_vote_id::get(&mut redis_conn, ROOM_ID)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(VOTE_ID, current_vote_id);

        let parameters = storage::parameters::get(&mut redis_conn, ROOM_ID, VOTE_ID)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(incoming_parameters, parameters.inner);

        let protocol = storage::protocol::get(&mut redis_conn, ROOM_ID, VOTE_ID)
            .await
            .unwrap();

        assert_eq!(1, protocol.len());
        match &protocol[0].event {
            VoteEvent::Start(start_parameters) => assert_eq!(&parameters, start_parameters),
            _ => panic!("Expected start event"),
        }
    }

    #[tokio::test]
    #[serial]
    async fn cast_vote() {
        let mut redis_conn = setup_redis().await;
        let db_ctx = setup_postgres().await;

        let module = LegalVote {
            db_ctx,
            participant_id: PARTICIPANT_1,
            user_id: USER_1,
            room_id: ROOM_ID,
        };

        let incoming_parameters = incoming::UserParameters {
            name: "TestVote".into(),
            topic: "Yes or No?".into(),
            allowed_participants: vec![PARTICIPANT_1, PARTICIPANT_2, PARTICIPANT_3],
            enable_abstain: false,
            secret: false,
            auto_stop: false,
            duration: None,
        };

        module
            .start_vote_routine(&mut redis_conn, VOTE_ID, incoming_parameters.clone())
            .await
            .unwrap();

        let vote_message = incoming::VoteMessage {
            vote_id: VOTE_ID,
            option: VoteOption::Yes,
        };

        module
            .cast_vote(&mut redis_conn, vote_message)
            .await
            .unwrap();

        let vote_result = module
            .get_vote_results(&mut redis_conn, VOTE_ID)
            .await
            .unwrap();

        assert_eq!(VOTE_ID, vote_result.vote_id);

        match vote_result.results {
            outgoing::Results::Public(public_results) => {
                assert_eq!(1, public_results.voters.len());
                assert_eq!(
                    VoteOption::Yes,
                    *public_results.voters.get(&PARTICIPANT_1).unwrap()
                );

                assert_eq!(1, *public_results.votes.get(&VoteOption::Yes).unwrap())
            }
            outgoing::Results::Secret(_) => panic!("Expected public vote results"),
        }

        let protocol = storage::protocol::get(&mut redis_conn, ROOM_ID, VOTE_ID)
            .await
            .unwrap();

        assert_eq!(2, protocol.len());
        match protocol[0].event {
            VoteEvent::Start(_) => (),
            _ => panic!("Expected start event"),
        }

        match protocol[1].event {
            VoteEvent::Vote(vote_option) => (assert_eq!(VoteOption::Yes, vote_option)),
            _ => panic!("Expected vote event"),
        }
    }

    #[tokio::test]
    #[serial]
    async fn stop_vote() {
        let mut redis_conn = setup_redis().await;
        let db_ctx = setup_postgres().await;

        let module = LegalVote {
            db_ctx,
            participant_id: PARTICIPANT_1,
            user_id: USER_1,
            room_id: ROOM_ID,
        };

        let incoming_parameters = incoming::UserParameters {
            name: "TestVote".into(),
            topic: "Yes or No?".into(),
            allowed_participants: vec![PARTICIPANT_1, PARTICIPANT_2, PARTICIPANT_3],
            enable_abstain: false,
            secret: false,
            auto_stop: false,
            duration: None,
        };

        module
            .start_vote_routine(&mut redis_conn, VOTE_ID, incoming_parameters.clone())
            .await
            .unwrap();

        module.stop_vote(&mut redis_conn, VOTE_ID).await.unwrap();

        let current_vote_id = storage::current_vote_id::get(&mut redis_conn, ROOM_ID)
            .await
            .unwrap();

        assert_eq!(None, current_vote_id);

        let history = storage::history::get(&mut redis_conn, ROOM_ID)
            .await
            .unwrap();

        assert!(history.contains(&VOTE_ID));

        let protocol = storage::protocol::get(&mut redis_conn, ROOM_ID, VOTE_ID)
            .await
            .unwrap();

        assert_eq!(2, protocol.len());
        match protocol[0].event {
            VoteEvent::Start(_) => (),
            _ => panic!("Expected start event"),
        }

        match protocol[1].event {
            VoteEvent::Stop => (),
            _ => panic!("Expected stop event"),
        }
    }

    #[tokio::test]
    #[serial]
    async fn cancel_vote() {
        let mut redis_conn = setup_redis().await;
        let db_ctx = setup_postgres().await;

        let module = LegalVote {
            db_ctx,
            participant_id: PARTICIPANT_1,
            user_id: USER_1,
            room_id: ROOM_ID,
        };

        let incoming_parameters = incoming::UserParameters {
            name: "TestVote".into(),
            topic: "Yes or No?".into(),
            allowed_participants: vec![PARTICIPANT_1, PARTICIPANT_2, PARTICIPANT_3],
            enable_abstain: false,
            secret: false,
            auto_stop: false,
            duration: None,
        };

        module
            .start_vote_routine(&mut redis_conn, VOTE_ID, incoming_parameters.clone())
            .await
            .unwrap();

        let cancel_reason = String::from("test reason");

        module
            .cancel_vote(&mut redis_conn, VOTE_ID, cancel_reason.clone())
            .await
            .unwrap();

        let current_vote_id = storage::current_vote_id::get(&mut redis_conn, ROOM_ID)
            .await
            .unwrap();

        assert_eq!(None, current_vote_id);

        let history = storage::history::get(&mut redis_conn, ROOM_ID)
            .await
            .unwrap();

        assert!(history.contains(&VOTE_ID));

        let protocol = storage::protocol::get(&mut redis_conn, ROOM_ID, VOTE_ID)
            .await
            .unwrap();

        assert_eq!(2, protocol.len());
        match protocol[0].event {
            VoteEvent::Start(_) => (),
            _ => panic!("Expected start event"),
        }

        match &protocol[1].event {
            VoteEvent::Cancel(reason) => match reason {
                storage::protocol::Reason::Custom(reason) => assert_eq!(&cancel_reason, reason),
                _ => panic!("Expected custom reason "),
            },
            _ => panic!("Expected cancel event"),
        }
    }

    #[tokio::test]
    #[serial]
    async fn database() {
        let mut redis_conn = setup_redis().await;
        let db_ctx = setup_postgres().await;

        // Create a user in the database as the legal vote has a foreign key to the users table
        let _ = add_test_user_to_db(db_ctx.clone());

        let module = LegalVote {
            db_ctx,
            participant_id: PARTICIPANT_1,
            user_id: USER_1,
            room_id: ROOM_ID,
        };

        let incoming_parameters = incoming::UserParameters {
            name: "TestVote".into(),
            topic: "Yes or No?".into(),
            allowed_participants: vec![PARTICIPANT_1, PARTICIPANT_2, PARTICIPANT_3],
            enable_abstain: false,
            secret: false,
            auto_stop: false,
            duration: None,
        };

        let vote_id = module.new_vote_in_database().await.unwrap();

        module
            .start_vote_routine(&mut redis_conn, vote_id, incoming_parameters.clone())
            .await
            .unwrap();

        module.stop_vote(&mut redis_conn, vote_id).await.unwrap();

        let protocol = storage::protocol::get(&mut redis_conn, ROOM_ID, vote_id)
            .await
            .unwrap();

        module
            .save_protocol_in_database(&mut redis_conn, vote_id)
            .await
            .unwrap();

        let db_legal_vote = module.db_ctx.get_legal_vote(vote_id).unwrap().unwrap();

        let db_protocol =
            serde_json::from_value::<Vec<ProtocolEntry>>(db_legal_vote.protocol).unwrap();

        assert_eq!(protocol, db_protocol);
    }

    #[tokio::test]
    #[serial]
    // todo: split test, and check weird combinations of USER_1 with Participant_2
    async fn basic_yes_no() {
        let mut redis_conn = setup_redis().await;
        let db_ctx = setup_postgres().await;

        let module1 = LegalVote {
            db_ctx: db_ctx.clone(),
            participant_id: PARTICIPANT_1,
            user_id: USER_1,
            room_id: ROOM_ID,
        };

        let module2 = LegalVote {
            db_ctx: db_ctx.clone(),
            participant_id: PARTICIPANT_2,
            user_id: USER_2,
            room_id: ROOM_ID,
        };

        let module3 = LegalVote {
            db_ctx,
            participant_id: PARTICIPANT_3,
            user_id: USER_3,
            room_id: ROOM_ID,
        };

        let parameters = incoming::UserParameters {
            name: "TestVote".into(),
            topic: "Yes or No?".into(),
            allowed_participants: vec![PARTICIPANT_1, PARTICIPANT_2, PARTICIPANT_3],
            enable_abstain: false,
            secret: false,
            auto_stop: false,
            duration: None,
        };

        let _rabbitmq_params = module1
            .start_vote_routine(&mut redis_conn, VOTE_ID, parameters)
            .await
            .unwrap();

        assert!(module1
            .is_vote_initiator(&mut redis_conn, VOTE_ID)
            .await
            .unwrap());

        let vote_message = VoteMessage {
            vote_id: VOTE_ID,
            option: VoteOption::Yes,
        };

        // PARTICIPANT_1 vote for yes
        let vote_response = module1
            .cast_vote(&mut redis_conn, vote_message)
            .await
            .unwrap();

        assert_eq!(VoteResponse::Success, vote_response);

        // PARTICIPANT_1 try to vote twice
        let vote_response = module1
            .cast_vote(&mut redis_conn, vote_message)
            .await
            .unwrap();

        assert_eq!(VoteResponse::Failed(VoteFailed::Ineligible), vote_response);

        // PARTICIPANT_2 vote for no
        let vote_message = VoteMessage {
            vote_id: VOTE_ID,
            option: VoteOption::No,
        };

        let vote_response = module2
            .cast_vote(&mut redis_conn, vote_message)
            .await
            .unwrap();

        assert_eq!(VoteResponse::Success, vote_response);

        // PARTICIPANT_3 vote for abstain
        let vote_message = VoteMessage {
            vote_id: VOTE_ID,
            option: VoteOption::Abstain,
        };

        let vote_response = module3
            .cast_vote(&mut redis_conn, vote_message)
            .await
            .unwrap();

        assert_eq!(
            VoteResponse::Failed(VoteFailed::InvalidOption),
            vote_response,
        );

        // PARTICIPANT_3 vote for yes
        let vote_message = VoteMessage {
            vote_id: VOTE_ID,
            option: VoteOption::Yes,
        };

        let vote_response = module3
            .cast_vote(&mut redis_conn, vote_message)
            .await
            .unwrap();

        assert_eq!(VoteResponse::Success, vote_response);

        assert!(module1
            .is_vote_initiator(&mut redis_conn, VOTE_ID)
            .await
            .unwrap());

        let vote_results = module1
            .get_vote_results(&mut redis_conn, VOTE_ID)
            .await
            .unwrap();

        assert_eq!(VOTE_ID, vote_results.vote_id);

        match vote_results.results {
            outgoing::Results::Public(results) => {
                assert_eq!(
                    &VoteOption::Yes,
                    results.voters.get(&PARTICIPANT_1).unwrap()
                );
                assert_eq!(&VoteOption::No, results.voters.get(&PARTICIPANT_2).unwrap());
                assert_eq!(
                    &VoteOption::Yes,
                    results.voters.get(&PARTICIPANT_3).unwrap()
                );

                let yes_count = results.votes.get(&VoteOption::Yes).unwrap();
                assert_eq!(&2, yes_count);

                let no_count = results.votes.get(&VoteOption::No).unwrap();
                assert_eq!(&1, no_count);
            }
            outgoing::Results::Secret(_) => panic!("Expected public results"),
        }

        module1.cleanup_room(&mut redis_conn).await.unwrap();

        let key_count = count_redis_keys(&mut redis_conn).await;

        // expect only the user_id attribute key to exist
        assert_eq!(1, key_count);
    }
}
