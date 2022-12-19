//! # Legal Vote Module
//!
//! ## Functionality
//!
//! Offers full legal vote features including live voting with high safety guards (atomic changes, audit log).
//! Stores the result for further archival storage in postgres.
use anyhow::Context;
use anyhow::Result;
use chrono::{DateTime, Utc};
use controller::prelude::bytes::Bytes;
use controller::prelude::futures::stream::once;
use controller::prelude::futures::FutureExt;
use controller::prelude::*;
use controller::storage::assets::save_asset;
use controller::storage::ObjectStorage;
use controller_shared::ParticipantId;
use database::Db;
use db_storage::legal_votes::set_protocol;
use db_storage::legal_votes::types::protocol::v1::{
    Cancel, ProtocolEntry, Start, UserInfo, Vote, VoteEvent,
};
use db_storage::legal_votes::types::{
    protocol as db_protocol, protocol::NewProtocol, CancelReason, FinalResults, Invalid,
    Parameters, Tally, Token, UserParameters, VoteKind, VoteOption,
};
use db_storage::legal_votes::{LegalVoteId, NewLegalVote};
use db_storage::users::UserId;
use error::{Error, ErrorKind};
use incoming::VoteMessage;
use kustos::prelude::AccessMethod;
use kustos::Authz;
use kustos::Resource;
use outgoing::{PdfAsset, Response, VoteFailed, VoteResponse, VoteResults, VoteSuccess};
use rabbitmq::Canceled;
use rabbitmq::StopKind;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use storage::protocol;
use storage::protocol::extract_voting_record_from_protocol;
use storage::VoteScriptResult;
use tokio::time::sleep;
use validator::Validate;

mod error;
pub mod frontend_data;
pub mod incoming;
pub mod outgoing;
mod pdf;
pub mod rabbitmq;
mod storage;

/// A TimerEvent used for the vote expiration feature
pub struct TimerEvent {
    legal_vote_id: LegalVoteId,
}

/// The legal vote [`SignalingModule`]
///
/// Holds a database interface and information about the underlying user & room. Vote information is
/// saved and managed in redis via the private `storage` module.
pub struct LegalVote {
    db: Arc<Db>,
    storage: Arc<ObjectStorage>,
    authz: Arc<Authz>,
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
    type FrontendData = frontend_data::FrontendData;
    type PeerFrontendData = ();

    async fn init(
        ctx: InitContext<'_, Self>,
        _params: &Self::Params,
        _protocol: &'static str,
    ) -> anyhow::Result<Option<Self>> {
        if let Participant::User(user) = ctx.participant() {
            Ok(Some(Self {
                db: ctx.db().clone(),
                storage: ctx.storage().clone(),
                authz: ctx.authz().clone(),
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
            Event::Joined { frontend_data, .. } => {
                *frontend_data = Some(
                    frontend_data::FrontendData::load_from_history(ctx.redis_conn(), self.room_id)
                        .await?,
                );
            }
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
                let vote_status = storage::get_vote_status(
                    ctx.redis_conn(),
                    self.room_id,
                    timer_event.legal_vote_id,
                )
                .await?;

                match vote_status {
                    storage::VoteStatus::Active => {
                        let stop_kind = StopKind::Expired;

                        let expired_entry =
                            ProtocolEntry::new(VoteEvent::Stop(db_protocol::v1::StopKind::Expired));

                        if let Err(error) = self
                            .end_vote(
                                &mut ctx,
                                timer_event.legal_vote_id,
                                expired_entry,
                                stop_kind,
                            )
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
            match storage::current_legal_vote_id::get(ctx.redis_conn(), self.room_id).await {
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
                            Ok(_) => {
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
                if !matches!(ctx.role(), Role::Moderator) {
                    return Err(ErrorKind::InsufficientPermissions.into());
                }

                self.handle_start_message(ctx, incoming_parameters).await?;
            }
            incoming::Message::Stop(incoming::Stop { legal_vote_id }) => {
                if !matches!(ctx.role(), Role::Moderator) {
                    return Err(ErrorKind::InsufficientPermissions.into());
                }

                self.stop_vote_routine(ctx, legal_vote_id).await?;
            }
            incoming::Message::Cancel(incoming::Cancel {
                legal_vote_id,
                reason,
            }) => {
                if !matches!(ctx.role(), Role::Moderator) {
                    return Err(ErrorKind::InsufficientPermissions.into());
                }

                let entry = self
                    .cancel_vote(ctx.redis_conn(), legal_vote_id, reason.clone())
                    .await?;

                self.save_protocol_in_database(ctx.redis_conn(), legal_vote_id)
                    .await?;

                ctx.rabbitmq_publish(
                    control::rabbitmq::current_room_exchange_name(self.room_id),
                    control::rabbitmq::room_all_routing_key().into(),
                    rabbitmq::Event::Cancel(Canceled {
                        legal_vote_id,
                        reason: CancelReason::Custom(reason),
                        end_time: entry
                            .timestamp
                            .expect("Missing timestamp for cancel vote ProtocolEntry"),
                    }),
                );

                let parameters =
                    storage::parameters::get(ctx.redis_conn(), self.room_id, legal_vote_id)
                        .await?
                        .ok_or(ErrorKind::InvalidVoteId)?;

                if parameters.inner.create_pdf {
                    self.save_pdf(ctx, legal_vote_id, self.user_id).await?
                }
            }
            incoming::Message::Vote(vote_message) => {
                let (vote_response, auto_close) = self.cast_vote(ctx, vote_message).await?;

                if let Response::Success(VoteSuccess {
                    vote_option,
                    issuer,
                    consumed_token,
                }) = vote_response.response
                {
                    let update = rabbitmq::Event::Update(rabbitmq::VoteUpdate {
                        legal_vote_id: vote_message.legal_vote_id,
                    });

                    // Send a vote success message to all participants that have the same user id
                    ctx.rabbitmq_publish(
                        control::rabbitmq::current_room_exchange_name(self.room_id),
                        control::rabbitmq::room_user_routing_key(self.user_id),
                        rabbitmq::Event::Voted(rabbitmq::VoteSuccess {
                            legal_vote_id: vote_message.legal_vote_id,
                            vote_option,
                            issuer,
                            consumed_token,
                        }),
                    );

                    ctx.rabbitmq_publish(
                        control::rabbitmq::current_room_exchange_name(self.room_id),
                        control::rabbitmq::room_all_routing_key().into(),
                        update,
                    );

                    if auto_close {
                        let stop_kind = StopKind::Auto;

                        let auto_close_entry =
                            ProtocolEntry::new(VoteEvent::Stop(db_protocol::v1::StopKind::Auto));

                        self.end_vote(ctx, vote_message.legal_vote_id, auto_close_entry, stop_kind)
                            .await?;
                    }
                } else {
                    ctx.ws_send(outgoing::Message::Voted(vote_response));
                }
            }
            incoming::Message::GeneratePdf(generate) => {
                if !matches!(ctx.role(), Role::Moderator) {
                    return Err(ErrorKind::InsufficientPermissions.into());
                }

                // check if the vote passed already
                if !storage::history::contains(
                    ctx.redis_conn(),
                    self.room_id,
                    generate.legal_vote_id,
                )
                .await?
                {
                    return Err(ErrorKind::InvalidVoteId.into());
                }

                self.save_pdf(ctx, generate.legal_vote_id, self.user_id)
                    .await?;
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
            rabbitmq::Event::Stop(stopped) => {
                ctx.ws_send(outgoing::Message::Stopped(stopped));
            }
            rabbitmq::Event::Voted(vote_success) => {
                ctx.ws_send(outgoing::Message::Voted(VoteResponse {
                    legal_vote_id: vote_success.legal_vote_id,
                    response: outgoing::Response::Success(outgoing::VoteSuccess {
                        vote_option: vote_success.vote_option,
                        issuer: vote_success.issuer,
                        consumed_token: vote_success.consumed_token,
                    }),
                }))
            }
            rabbitmq::Event::Cancel(cancel) => {
                ctx.ws_send(outgoing::Message::Canceled(cancel));
            }
            rabbitmq::Event::Update(update) => {
                let parameters =
                    storage::parameters::get(ctx.redis_conn(), self.room_id, update.legal_vote_id)
                        .await?
                        .ok_or(ErrorKind::InvalidVoteId)?;
                let results = self
                    .get_vote_results(
                        ctx.redis_conn(),
                        update.legal_vote_id,
                        !parameters.inner.kind.is_hidden(),
                    )
                    .await?;

                ctx.ws_send(outgoing::Message::Updated(VoteResults {
                    legal_vote_id: update.legal_vote_id,
                    results,
                }));
            }
            rabbitmq::Event::FatalServerError => {
                ctx.ws_send(outgoing::Message::Error(outgoing::ErrorKind::Internal));
            }
            rabbitmq::Event::PdfAsset(pdf_asset) => {
                ctx.ws_send(outgoing::Message::PdfAsset(pdf_asset))
            }
        }
        Ok(())
    }

    async fn handle_start_message(
        &mut self,
        ctx: &mut ModuleContext<'_, LegalVote>,
        incoming_parameters: UserParameters,
    ) -> Result<(), Error> {
        let legal_vote_id = self
            .new_vote_in_database()
            .await
            .context("Failed to create new vote in database")?;
        match self
            .start_vote_routine(ctx.redis_conn(), legal_vote_id, incoming_parameters)
            .await
        {
            Ok((rabbitmq_parameters, tokens)) => {
                if let Err(e) = self
                    .authz
                    .grant_user_access(
                        self.user_id,
                        &[(
                            &rabbitmq_parameters.legal_vote_id.resource_id(),
                            &[AccessMethod::Get, AccessMethod::Put, AccessMethod::Delete],
                        )],
                    )
                    .await
                {
                    log::error!("Failed to add RBAC policy for legal vote: {}", e);
                    storage::cleanup_vote(ctx.redis_conn(), self.room_id, legal_vote_id).await?;

                    return Err(ErrorKind::PermissionError.into());
                }

                if let Some(duration) = rabbitmq_parameters.inner.duration {
                    ctx.add_event_stream(once(
                        sleep(Duration::from_secs(duration))
                            .map(move |_| TimerEvent { legal_vote_id }),
                    ));
                }

                for participant_id in
                    control::storage::get_all_participants(ctx.redis_conn(), self.room_id).await?
                {
                    let mut parameters = rabbitmq_parameters.clone();
                    parameters.token = tokens.get(&participant_id).copied();
                    ctx.rabbitmq_publish(
                        control::rabbitmq::current_room_exchange_name(self.room_id),
                        control::rabbitmq::room_participant_routing_key(participant_id),
                        rabbitmq::Event::Start(parameters),
                    );
                }
            }
            Err(start_error) => {
                log::warn!("Failed to start vote, {:?}", start_error);

                // return the cleanup error in case of failure as its more severe
                storage::cleanup_vote(ctx.redis_conn(), self.room_id, legal_vote_id).await?;

                return Err(start_error);
            }
        };
        Ok(())
    }

    /// Set all vote related redis keys
    async fn start_vote_routine(
        &self,
        redis_conn: &mut RedisConnection,
        legal_vote_id: LegalVoteId,
        incoming_parameters: UserParameters,
    ) -> Result<(Parameters, HashMap<ParticipantId, Token>), Error> {
        let start_time = Utc::now();

        let (max_votes, participant_tokens) = self
            .init_allowed_tokens(
                redis_conn,
                legal_vote_id,
                &incoming_parameters.allowed_participants,
            )
            .await?;

        let parameters = Parameters {
            initiator_id: self.participant_id,
            legal_vote_id,
            start_time,
            max_votes,
            inner: incoming_parameters,
            token: None,
        };

        storage::parameters::set(redis_conn, self.room_id, legal_vote_id, &parameters).await?;

        self.init_vote_protocol(redis_conn, legal_vote_id, start_time, parameters.clone())
            .await?;

        if !storage::current_legal_vote_id::set(redis_conn, self.room_id, legal_vote_id).await? {
            return Err(ErrorKind::VoteAlreadyActive.into());
        }

        Ok((parameters, participant_tokens))
    }

    /// Add the start entry to the protocol of this vote
    async fn init_vote_protocol(
        &self,
        redis_conn: &mut RedisConnection,
        legal_vote_id: LegalVoteId,
        start_time: DateTime<Utc>,
        parameters: Parameters,
    ) -> Result<()> {
        let start_entry = ProtocolEntry::new_with_time(
            start_time,
            VoteEvent::Start(Start {
                issuer: self.user_id,
                parameters,
            }),
        );

        storage::protocol::add_entry(redis_conn, self.room_id, legal_vote_id, start_entry).await?;

        Ok(())
    }

    /// Set the allowed users list for the provided `legal_vote_id` to its initial state
    ///
    /// Returns the maximum number of possible votes
    async fn init_allowed_tokens(
        &self,
        redis_conn: &mut RedisConnection,
        legal_vote_id: LegalVoteId,
        allowed_participants: &[ParticipantId],
    ) -> Result<(u32, HashMap<ParticipantId, Token>), Error> {
        let allowed_users = control::storage::get_attribute_for_participants::<UserId>(
            redis_conn,
            self.room_id,
            "user_id",
            allowed_participants,
        )
        .await?;

        let mut invalid_participants = Vec::new();
        let mut user_tokens = HashMap::new();
        let mut participant_tokens = HashMap::new();

        for (participant_id, maybe_user_id) in allowed_participants.iter().zip(allowed_users) {
            match maybe_user_id {
                Some(user_id) => {
                    let token = user_tokens.entry(user_id).or_insert_with(Token::generate);
                    participant_tokens.insert(*participant_id, *token);
                }
                None => {
                    invalid_participants.push(*participant_id);
                }
            }
        }

        if !invalid_participants.is_empty() {
            return Err(Error::Vote(ErrorKind::AllowlistContainsGuests(
                invalid_participants,
            )));
        }

        let max_votes = user_tokens.len();

        let tokens = user_tokens.values().copied().collect::<Vec<Token>>();
        storage::allowed_tokens::set(redis_conn, self.room_id, legal_vote_id, tokens).await?;

        Ok((max_votes as u32, participant_tokens))
    }

    /// Stop a vote
    ///
    /// Checks if the provided `legal_vote_id` is currently active before stopping the vote.
    ///
    /// Fails with `VoteError::InvalidVoteId` when the provided `legal_vote_id` does not match the active vote id.
    /// Adds a `ProtocolEntry` with `VoteEvent::Stop(Stop::UserStop(<user_id>))` to the vote protocol when successful.
    async fn stop_vote_routine(
        &self,
        ctx: &mut ModuleContext<'_, LegalVote>,
        legal_vote_id: LegalVoteId,
    ) -> Result<(), Error> {
        if !self
            .is_current_vote_id(ctx.redis_conn(), legal_vote_id)
            .await?
        {
            return Err(ErrorKind::InvalidVoteId.into());
        }

        let stop_kind = StopKind::ByParticipant(self.participant_id);

        let stop_entry = ProtocolEntry::new(VoteEvent::Stop(db_protocol::v1::StopKind::ByUser(
            self.user_id,
        )));

        self.end_vote(ctx, legal_vote_id, stop_entry, stop_kind)
            .await?;

        Ok(())
    }

    /// Cast a vote
    ///
    /// Checks if the provided `vote_message` contains valid values & calls [`storage::vote`].
    /// See [`storage::vote`] & [`storage::VOTE_SCRIPT`] for more details on the vote process.
    ///
    /// # Returns
    /// - Ok([`VoteResponse`], <should_auto_close>) in case of successfully executing [`storage::vote`].
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
            .is_current_vote_id(redis_conn, vote_message.legal_vote_id)
            .await
        {
            Ok(is_current) => {
                if !is_current {
                    return Ok((
                        VoteResponse {
                            legal_vote_id: vote_message.legal_vote_id,
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
                            legal_vote_id: vote_message.legal_vote_id,
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
            match storage::parameters::get(redis_conn, self.room_id, vote_message.legal_vote_id)
                .await?
            {
                Some(parameters) => parameters,
                None => {
                    return Ok((
                        VoteResponse {
                            legal_vote_id: vote_message.legal_vote_id,
                            response: Response::Failed(VoteFailed::InvalidVoteId),
                        },
                        false,
                    ));
                }
            };

        if vote_message.option == VoteOption::Abstain && !parameters.inner.enable_abstain {
            return Ok((
                VoteResponse {
                    legal_vote_id: vote_message.legal_vote_id,
                    response: Response::Failed(VoteFailed::InvalidOption),
                },
                false,
            ));
        }

        let user_info = match parameters.inner.kind {
            VoteKind::Pseudonymous => None,
            VoteKind::RollCall => Some(UserInfo {
                issuer: self.user_id,
                participant_id: self.participant_id,
            }),
        };

        let vote_event = Vote {
            user_info,
            option: vote_message.option,
            token: vote_message.token,
        };

        let vote_result = storage::vote(
            redis_conn,
            self.room_id,
            vote_message.legal_vote_id,
            vote_message.token,
            vote_event,
        )
        .await?;

        let (response, should_auto_close) = match vote_result {
            VoteScriptResult::Success => (
                Response::Success(VoteSuccess {
                    vote_option: vote_message.option,
                    issuer: self.participant_id,
                    consumed_token: vote_message.token,
                }),
                false,
            ),
            VoteScriptResult::SuccessAutoClose => (
                Response::Success(VoteSuccess {
                    vote_option: vote_message.option,
                    issuer: self.participant_id,
                    consumed_token: vote_message.token,
                }),
                parameters.inner.auto_close,
            ),
            VoteScriptResult::InvalidVoteId => (Response::Failed(VoteFailed::InvalidVoteId), false),
            VoteScriptResult::Ineligible => (Response::Failed(VoteFailed::Ineligible), false),
        };

        Ok((
            VoteResponse {
                legal_vote_id: vote_message.legal_vote_id,
                response,
            },
            should_auto_close,
        ))
    }

    /// Check if the provided `legal_vote_id` equals the current active vote id
    ///
    /// Returns [`ErrorKind::NoVoteActive`] when no vote is active.
    async fn is_current_vote_id(
        &self,
        redis_conn: &mut RedisConnection,
        legal_vote_id: LegalVoteId,
    ) -> Result<bool, Error> {
        if let Some(current_legal_vote_id) =
            storage::current_legal_vote_id::get(redis_conn, self.room_id).await?
        {
            Ok(current_legal_vote_id == legal_vote_id)
        } else {
            Err(ErrorKind::NoVoteActive.into())
        }
    }

    /// Cancel the active vote if the leaving participant is the initiator
    async fn handle_leaving(&self, ctx: &mut ModuleContext<'_, Self>) -> Result<(), Error> {
        let redis_conn = ctx.redis_conn();

        let current_vote_id =
            match storage::current_legal_vote_id::get(redis_conn, self.room_id).await? {
                Some(current_vote_id) => current_vote_id,
                None => return Ok(()),
            };

        let parameters = storage::parameters::get(redis_conn, self.room_id, current_vote_id)
            .await?
            .ok_or(ErrorKind::InvalidVoteId)?;

        if parameters.initiator_id == self.participant_id {
            let reason = CancelReason::InitiatorLeft;

            let entry = self
                .cancel_vote_unchecked(redis_conn, current_vote_id, reason.clone())
                .await?;

            self.save_protocol_in_database(ctx.redis_conn(), current_vote_id)
                .await?;

            ctx.rabbitmq_publish(
                control::rabbitmq::current_room_exchange_name(self.room_id),
                control::rabbitmq::room_all_routing_key().into(),
                rabbitmq::Event::Cancel(rabbitmq::Canceled {
                    legal_vote_id: current_vote_id,
                    reason,
                    end_time: entry
                        .timestamp
                        .expect("Missing timestamp on cancel vote ProtocolEntry"),
                }),
            );

            if parameters.inner.create_pdf {
                self.save_pdf(ctx, current_vote_id, self.user_id).await?;
            }
        }

        Ok(())
    }

    /// Get the vote results for the specified `legal_vote_id`
    async fn get_vote_results(
        &self,
        redis_conn: &mut RedisConnection,
        legal_vote_id: LegalVoteId,
        include_voting_record: bool,
    ) -> Result<outgoing::Results, Error> {
        let parameters = storage::parameters::get(redis_conn, self.room_id, legal_vote_id)
            .await?
            .ok_or(ErrorKind::InvalidVoteId)?;

        let tally = storage::vote_count::get(
            redis_conn,
            self.room_id,
            legal_vote_id,
            parameters.inner.enable_abstain,
        )
        .await?;

        let voting_record = if include_voting_record {
            let protocol = storage::protocol::get(redis_conn, self.room_id, legal_vote_id).await?;
            Some(extract_voting_record_from_protocol(&protocol)?)
        } else {
            None
        };

        Ok(outgoing::Results {
            tally,
            voting_record,
        })
    }

    /// Cancel a vote
    ///
    /// Checks if the provided vote id is currently active before canceling the vote.
    async fn cancel_vote(
        &self,
        redis_conn: &mut RedisConnection,
        legal_vote_id: LegalVoteId,
        reason: String,
    ) -> Result<ProtocolEntry, Error> {
        if !self.is_current_vote_id(redis_conn, legal_vote_id).await? {
            return Err(ErrorKind::InvalidVoteId.into());
        }

        self.cancel_vote_unchecked(redis_conn, legal_vote_id, CancelReason::Custom(reason))
            .await
    }

    /// Cancel a vote without checking permissions
    ///
    /// Fails with `VoteError::InvalidVoteId` when the provided `legal_vote_id` does not match the active vote id.
    /// Adds a `ProtocolEntry` with `VoteEvent::Cancel` to the vote protocol when successful.
    async fn cancel_vote_unchecked(
        &self,
        redis_conn: &mut RedisConnection,
        legal_vote_id: LegalVoteId,
        reason: CancelReason,
    ) -> Result<ProtocolEntry, Error> {
        let cancel_entry = ProtocolEntry::new(VoteEvent::Cancel(Cancel {
            issuer: self.user_id,
            reason,
        }));

        if !storage::end_current_vote(redis_conn, self.room_id, legal_vote_id, &cancel_entry)
            .await?
        {
            return Err(ErrorKind::InvalidVoteId.into());
        }

        Ok(cancel_entry)
    }

    /// End the vote behind `legal_vote_id` using the provided parameters as stop parameters
    async fn end_vote(
        &self,
        ctx: &mut ModuleContext<'_, Self>,
        legal_vote_id: LegalVoteId,
        end_entry: ProtocolEntry,
        stop_kind: StopKind,
    ) -> Result<(), Error> {
        if !storage::end_current_vote(ctx.redis_conn(), self.room_id, legal_vote_id, &end_entry)
            .await?
        {
            return Err(ErrorKind::InvalidVoteId.into());
        }

        let final_results = self.validate_vote_results(ctx, legal_vote_id).await?;

        let final_results_entry = match &final_results {
            outgoing::FinalResults::Valid(results) => {
                ProtocolEntry::new(VoteEvent::FinalResults(FinalResults::Valid(results.tally)))
            }
            outgoing::FinalResults::Invalid(invalid) => {
                ProtocolEntry::new(VoteEvent::FinalResults(FinalResults::Invalid(*invalid)))
            }
        };

        protocol::add_entry(
            ctx.redis_conn(),
            self.room_id,
            legal_vote_id,
            final_results_entry,
        )
        .await?;

        self.save_protocol_in_database(ctx.redis_conn(), legal_vote_id)
            .await?;

        ctx.rabbitmq_publish(
            control::rabbitmq::current_room_exchange_name(self.room_id),
            control::rabbitmq::room_all_routing_key().into(),
            rabbitmq::Event::Stop(outgoing::Stopped {
                legal_vote_id,
                kind: stop_kind,
                results: final_results,
                end_time: end_entry
                    .timestamp
                    .expect("Missing timestamp for end vote ProtocolEntry"),
            }),
        );

        let parameters = storage::parameters::get(ctx.redis_conn(), self.room_id, legal_vote_id)
            .await?
            .ok_or(ErrorKind::InvalidVoteId)?;

        if parameters.inner.create_pdf {
            match stop_kind {
                // Send the pdf message to the participant id of the vote initiator in case of an auto stop
                StopKind::Auto => {
                    let protocol =
                        storage::protocol::get(ctx.redis_conn(), self.room_id, legal_vote_id)
                            .await?;

                    let pdf_asset = self
                        .create_pdf_asset(legal_vote_id, ctx.timestamp(), protocol)
                        .await?;

                    ctx.rabbitmq_publish(
                        control::rabbitmq::current_room_exchange_name(self.room_id),
                        control::rabbitmq::room_participant_routing_key(parameters.initiator_id),
                        rabbitmq::Event::PdfAsset(pdf_asset),
                    );
                }
                _ => {
                    self.save_pdf(ctx, legal_vote_id, self.user_id).await?;
                }
            }
        }

        Ok(())
    }

    /// Checks if the vote results for `legal_vote_id` are equal to the protocols vote entries.
    ///
    /// Returns
    async fn validate_vote_results(
        &self,
        ctx: &mut ModuleContext<'_, Self>,
        legal_vote_id: LegalVoteId,
    ) -> Result<outgoing::FinalResults, Error> {
        let parameters = storage::parameters::get(ctx.redis_conn(), self.room_id, legal_vote_id)
            .await?
            .ok_or(ErrorKind::InvalidVoteId)?;

        let protocol =
            storage::protocol::get(ctx.redis_conn(), self.room_id, legal_vote_id).await?;
        let voting_record = extract_voting_record_from_protocol(&protocol)?;

        let vote_options = voting_record.vote_option_list();

        let mut protocol_tally = Tally {
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

        for vote_option in &vote_options {
            total_votes += 1;

            match vote_option {
                VoteOption::Yes => protocol_tally.yes += 1,
                VoteOption::No => protocol_tally.no += 1,
                VoteOption::Abstain => {
                    if let Some(abstain) = &mut protocol_tally.abstain {
                        *abstain += 1;
                    } else {
                        return Ok(outgoing::FinalResults::Invalid(Invalid::AbstainDisabled));
                    }
                }
            }
        }

        let tally = storage::vote_count::get(
            ctx.redis_conn(),
            self.room_id,
            legal_vote_id,
            parameters.inner.enable_abstain,
        )
        .await?;

        if protocol_tally == tally && total_votes <= parameters.max_votes {
            Ok(outgoing::FinalResults::Valid(outgoing::Results {
                tally,
                voting_record: Some(voting_record),
            }))
        } else {
            Ok(outgoing::FinalResults::Invalid(
                Invalid::VoteCountInconsistent,
            ))
        }
    }

    /// Remove the all vote related redis keys belonging to this room
    async fn cleanup_room(&self, redis_conn: &mut RedisConnection) -> Result<()> {
        let vote_history = storage::history::get(redis_conn, self.room_id).await?;

        for legal_vote_id in vote_history.iter() {
            storage::cleanup_vote(redis_conn, self.room_id, *legal_vote_id).await?
        }

        storage::history::delete(redis_conn, self.room_id).await?;

        if let Some(current_vote_id) =
            storage::current_legal_vote_id::get(redis_conn, self.room_id).await?
        {
            storage::cleanup_vote(redis_conn, self.room_id, current_vote_id).await?;
            storage::current_legal_vote_id::delete(redis_conn, self.room_id).await?;
        }

        Ok(())
    }

    /// Creates a new vote in the database
    ///
    /// Adds a new vote with an empty protocol to the database. Returns the [`VoteId`] of the new vote.
    async fn new_vote_in_database(&self) -> Result<LegalVoteId> {
        let db = self.db.clone();

        let user_id = self.user_id;
        let room_id = self.room_id.room_id();

        let legal_vote = controller::block(move || {
            let mut conn = db.get_conn()?;

            NewLegalVote::new(user_id, room_id).insert(&mut conn)
        })
        .await??;

        Ok(legal_vote.id)
    }

    /// Save the protocol for `legal_vote_id` in the database
    async fn save_protocol_in_database(
        &self,
        redis_conn: &mut RedisConnection,
        legal_vote_id: LegalVoteId,
    ) -> Result<()> {
        let entries = storage::protocol::get(redis_conn, self.room_id, legal_vote_id).await?;

        let protocol = NewProtocol::new(entries);

        let db = self.db.clone();

        controller::block(move || {
            let mut conn = db.get_conn()?;

            set_protocol(&mut conn, legal_vote_id, protocol)
        })
        .await??;

        Ok(())
    }

    /// Save the legal vote protocol as PDF
    ///
    /// Sends the [`Event::PdfAsset`] to the provided `msg_target`
    async fn save_pdf(
        &self,
        ctx: &mut ModuleContext<'_, Self>,
        legal_vote_id: LegalVoteId,
        msg_target: UserId,
    ) -> Result<()> {
        let protocol =
            storage::protocol::get(ctx.redis_conn(), self.room_id, legal_vote_id).await?;

        let pdf_asset = self
            .create_pdf_asset(legal_vote_id, ctx.timestamp(), protocol)
            .await?;

        ctx.rabbitmq_publish(
            control::rabbitmq::current_room_exchange_name(self.room_id),
            control::rabbitmq::room_user_routing_key(msg_target),
            rabbitmq::Event::PdfAsset(pdf_asset),
        );

        Ok(())
    }

    async fn create_pdf_asset(
        &self,
        vote_id: LegalVoteId,
        ts: Timestamp,
        protocol: Vec<ProtocolEntry>,
    ) -> Result<PdfAsset> {
        let db = self.db.clone();

        let pdf_data = controller::block(|| {
            let mut data = vec![];

            pdf::generate_pdf(db, protocol, &mut data).unwrap();

            data
        })
        .await?;

        // convert the data to a applicable type for the `save_asset()` fn
        let data = tokio_stream::once(anyhow::Ok(Bytes::from(pdf_data)));

        let filename = format!("vote_protocol_{}.pdf", ts.format("%Y-%m-%d_%H-%M-%S-%Z"));

        let asset_id = save_asset(
            &self.storage,
            self.db.clone(),
            self.room_id.room_id(),
            Some(Self::NAMESPACE),
            &filename,
            "protocol_pdf",
            data,
        )
        .await?;

        Ok(PdfAsset {
            filename,
            legal_vote_id: vote_id,
            asset_id,
        })
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
