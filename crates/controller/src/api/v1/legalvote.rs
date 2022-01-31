use super::{ApiResponse, DefaultApiError, PagePaginationQuery};
use crate::db::legal_votes::LegalVoteId;
use actix_web::get;
use actix_web::web::{Data, Json, Path, Query, ReqData};
use anyhow::Result;
use chrono::{DateTime, Utc};
use database::Db;
use db_storage::legal_votes::types::protocol::v1::{self, Cancel, VoteEvent};
use db_storage::legal_votes::types::protocol::{self, Protocol};
use db_storage::legal_votes::types::{
    FinalResults, Invalid, Parameters, UserParameters, VoteOption, Votes,
};
use db_storage::legal_votes::DbLegalVoteEx;
use db_storage::rooms::RoomId;
use db_storage::users::User;
use db_storage::DbUsersEx;
use kustos::prelude::AccessMethod;
use kustos::{AccessibleResources, Authz};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;

/// Wrapper struct to display invalid protocols to the API caller
#[derive(Debug, Serialize)]
pub struct LegalvoteEntry {
    /// The vote id
    vote_id: LegalVoteId,
    /// The parsing result of the protocol
    #[serde(flatten)]
    protocol_result: ProtocolResult,
}
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum ProtocolResult {
    /// Successfully parsed protocol
    Ok(LegalvoteDetails),
    /// Failed to parse protocol
    Error(ProtocolError),
}

#[derive(thiserror::Error, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "error")]
pub enum ProtocolError {
    #[error("Legalvote protocol is invalid")]
    InvalidProtocol,
    #[error("An internal server error occurred while parsing the legalvote protocol")]
    InternalError,
}

/// Details of a passed legal vote
#[derive(Debug, Serialize)]
pub struct LegalvoteDetails {
    /// The legalvote settings
    #[serde(flatten)]
    pub settings: Settings,
    /// A list of participants that voted on the legalvote
    pub voters: Vec<Voter>,
    /// The results of the legalvote
    pub vote_result: VoteResult,
}

/// Settings of a legalvote
#[derive(Debug, Serialize)]
pub struct Settings {
    /// The participant id of the vote initiator
    pub initiator: ParticipantInfo,
    /// The time the vote got started
    pub start_time: DateTime<Utc>,
    /// The maximum amount of votes possible
    pub max_votes: u32,
    /// The name of the vote
    pub name: String,
    /// The subtitle of the vote
    pub subtitle: String,
    /// The topic that will be voted on
    pub topic: String,
    /// Indicates that the `Abstain` vote option is enabled
    pub enable_abstain: bool,
    /// The vote will automatically stop when every participant voted
    pub auto_stop: bool,
    /// The vote will stop when the duration (in seconds) has passed
    pub duration: Option<u64>,
}

/// Represents a participant in a legalvote
#[derive(Debug, Serialize)]
pub struct ParticipantInfo {
    firstname: String,
    lastname: String,
    email: String,
}

impl From<User> for ParticipantInfo {
    fn from(user: User) -> Self {
        Self {
            firstname: user.firstname,
            lastname: user.lastname,
            email: user.email,
        }
    }
}

/// A participant that voted in a legalvote
#[derive(Debug, Serialize)]
pub struct Voter {
    participant: ParticipantInfo,
    /// The chosen vote option
    vote_option: VoteOption,
}

/// The results of a legalvote
#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VoteResult {
    Success(Success),
    Failed(FailReason),
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct Success {
    stop_kind: StopKind,
    votes: Votes,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StopKind {
    /// A normal vote stop issued by a user. Contains the SerialUserId of the issuer
    ByParticipant(ParticipantInfo),
    /// The vote has been stopped automatically because all allowed users have voted
    Auto,
    /// The vote expired due to a set duration
    Expired,
}

#[derive(Debug, Serialize)]
pub enum FailReason {
    Canceled(Cancel),
    InvalidResults(Invalid),
}

/// API Endpoint *GET /rooms/{room_id}/legalvote*
///
/// Returns a JSON array of the legalvotes created in this room that the user can access
#[get("/rooms/{room_id}/legalvote")]
pub async fn get_all_for_room(
    db_ctx: Data<Db>,
    authz: Data<Authz>,
    pagination: Query<PagePaginationQuery>,
    room_id: Path<RoomId>,
    current_user: ReqData<User>,
) -> Result<ApiResponse<Vec<LegalvoteEntry>>, DefaultApiError> {
    let db_ctx = db_ctx.into_inner();
    let room_id = room_id.into_inner();
    let PagePaginationQuery { per_page, page } = pagination.into_inner();

    let accessible_legal_votes: AccessibleResources<LegalVoteId> = authz
        .get_accessible_resources_for_user(current_user.oidc_uuid, AccessMethod::Get)
        .await
        .map_err(|_| DefaultApiError::Internal)?;

    let (legal_votes, count) = crate::block(
        move || -> Result<(Vec<LegalvoteEntry>, i64), DefaultApiError> {
            let (legal_votes, count) = match accessible_legal_votes {
                AccessibleResources::List(vote_ids) => db_ctx
                    .get_legal_votes_by_id_for_room_paginated(room_id, &vote_ids, per_page, page)?,
                AccessibleResources::All => {
                    db_ctx.get_all_legal_votes_for_room_paginated(room_id, per_page, page)?
                }
            };

            let mut detailed_votes = Vec::new();

            for legal_vote in legal_votes {
                match parse_protocol(legal_vote.protocol, db_ctx.clone()) {
                    Ok(legalvote_detailed) => detailed_votes.push(LegalvoteEntry {
                        vote_id: legal_vote.id,
                        protocol_result: ProtocolResult::Ok(legalvote_detailed),
                    }),
                    Err(protocol_error) => {
                        detailed_votes.push(LegalvoteEntry {
                            vote_id: legal_vote.id,
                            protocol_result: ProtocolResult::Error(protocol_error),
                        });
                    }
                }
            }

            Ok((detailed_votes, count))
        },
    )
    .await
    .map_err(|e| {
        log::error!("BlockingError on GET /rooms/{{room_id}}/legalvote - {}", e);
        DefaultApiError::Internal
    })??;

    Ok(ApiResponse::new(legal_votes).with_page_pagination(per_page, page, count))
}

/// API Endpoint *GET /legalvote*
///
/// Returns a JSON array of the legalvotes that the user can access
#[get("/legalvote")]
pub async fn get_all(
    db_ctx: Data<Db>,
    authz: Data<Authz>,
    pagination: Query<PagePaginationQuery>,
    current_user: ReqData<User>,
) -> Result<ApiResponse<Vec<LegalvoteEntry>>, DefaultApiError> {
    let PagePaginationQuery { per_page, page } = pagination.into_inner();

    let accessible_legal_votes: AccessibleResources<LegalVoteId> = authz
        .get_accessible_resources_for_user(current_user.oidc_uuid, AccessMethod::Get)
        .await
        .map_err(|_| DefaultApiError::Internal)?;

    let db_ctx = db_ctx.into_inner();

    let (legal_votes, count) = crate::block(
        move || -> Result<(Vec<LegalvoteEntry>, i64), DefaultApiError> {
            let (legal_votes, count) = match accessible_legal_votes {
                AccessibleResources::List(vote_ids) => {
                    db_ctx.get_legal_votes_by_id_paginated(&vote_ids, per_page, page)?
                }
                AccessibleResources::All => db_ctx.get_all_legal_votes_paginated(per_page, page)?,
            };

            let mut detailed_votes = Vec::new();

            for legal_vote in legal_votes {
                match parse_protocol(legal_vote.protocol, db_ctx.clone()) {
                    Ok(legalvote_detailed) => detailed_votes.push(LegalvoteEntry {
                        vote_id: legal_vote.id,
                        protocol_result: ProtocolResult::Ok(legalvote_detailed),
                    }),
                    Err(protocol_error) => {
                        detailed_votes.push(LegalvoteEntry {
                            vote_id: legal_vote.id,
                            protocol_result: ProtocolResult::Error(protocol_error),
                        });
                    }
                }
            }

            Ok((detailed_votes, count))
        },
    )
    .await
    .map_err(|e| {
        log::error!("BlockingError on GET /legalvote - {}", e);
        DefaultApiError::Internal
    })??;

    Ok(ApiResponse::new(legal_votes).with_page_pagination(per_page, page, count))
}

/// API Endpoint *GET /legalvote/{vote_id}*
///
/// Returns the specified legalvote as [`LegalvoteEntry`]
#[get("/legalvote/{vote_id}")]
pub async fn get_specific(
    db_ctx: Data<Db>,
    authz: Data<Authz>,
    vote_id: Path<LegalVoteId>,
    current_user: ReqData<User>,
) -> Result<Json<LegalvoteEntry>, DefaultApiError> {
    let vote_id = vote_id.into_inner();

    let accessible_legal_votes: AccessibleResources<LegalVoteId> = authz
        .get_accessible_resources_for_user(current_user.oidc_uuid, AccessMethod::Get)
        .await
        .map_err(|_| DefaultApiError::Internal)?;

    match accessible_legal_votes {
        kustos::AccessibleResources::List(vote_ids) => {
            if !vote_ids.contains(&vote_id) {
                return Err(DefaultApiError::InsufficientPermission);
            }
        }
        kustos::AccessibleResources::All => (),
    }

    let legal_vote_detailed = crate::block(move || -> Result<LegalvoteEntry, DefaultApiError> {
        let legal_vote = db_ctx
            .get_legal_vote(vote_id)?
            .ok_or(DefaultApiError::NotFound)?;

        let legalvote_entry = match parse_protocol(legal_vote.protocol, db_ctx.into_inner()) {
            Ok(legalvote_detailed) => LegalvoteEntry {
                vote_id: legal_vote.id,
                protocol_result: ProtocolResult::Ok(legalvote_detailed),
            },
            Err(protocol_error) => LegalvoteEntry {
                vote_id: legal_vote.id,
                protocol_result: ProtocolResult::Error(protocol_error),
            },
        };

        Ok(legalvote_entry)
    })
    .await
    .map_err(|e| {
        log::error!("BlockingError on GET /legalvote/{{vote_id}} - {}", e);
        DefaultApiError::Internal
    })??;

    Ok(Json(legal_vote_detailed))
}

fn parse_protocol(protocol: Protocol, db_ctx: Arc<Db>) -> Result<LegalvoteDetails, ProtocolError> {
    match protocol.version {
        1 => {
            let entries: Vec<v1::ProtocolEntry> = serde_json::from_str(protocol.entries.get())
                .map_err(|e| {
                    log::error!("Failed to deserialize v1 protocol entries {}", e);
                    ProtocolError::InvalidProtocol
                })?;

            parse_v1_entries(entries, db_ctx)
        }
        unknown => {
            log::error!("Unknown legalvote protocol version '{}'", unknown);
            Err(ProtocolError::InvalidProtocol)
        }
    }
}

/// Converts a list of v1 protocol entries to [`LegalVoteDetails`]
fn parse_v1_entries(
    entries: Vec<v1::ProtocolEntry>,
    db_ctx: Arc<Db>,
) -> Result<LegalvoteDetails, ProtocolError> {
    if entries.is_empty() {
        log::error!("Legalvote protocol is empty");
        return Err(ProtocolError::InvalidProtocol);
    }

    let mut settings = None;
    let mut stop_kind = None;
    let mut final_results = None;
    let mut cancel = None;

    let mut raw_voters = HashMap::new();
    let mut user_ids = vec![];

    for entry in entries {
        match entry.event {
            VoteEvent::Start(start) => {
                let Parameters {
                    initiator_id: _,
                    vote_id: _,
                    start_time,
                    max_votes,
                    inner:
                        UserParameters {
                            name,
                            subtitle,
                            topic,
                            allowed_participants: _,
                            enable_abstain,
                            auto_stop,
                            duration,
                        },
                } = start.parameters;

                let initiator = db_ctx
                    .get_user_by_id(start.issuer)
                    .map_err(|e| {
                        log::error!(
                            "Failed to resolve vote initiator in legalvote protocol, {}",
                            e
                        );
                        ProtocolError::InternalError
                    })?
                    .into();

                settings = Some(Settings {
                    initiator,
                    start_time,
                    max_votes,
                    name,
                    subtitle,
                    topic,
                    enable_abstain,
                    auto_stop,
                    duration,
                });
            }
            VoteEvent::Vote(vote) => {
                user_ids.push(vote.issuer);
                raw_voters.insert(vote.issuer, vote.option);
            }
            VoteEvent::Stop(kind) => {
                stop_kind = Some(match kind {
                    protocol::v1::StopKind::Auto => StopKind::Auto,
                    protocol::v1::StopKind::Expired => StopKind::Expired,
                    protocol::v1::StopKind::ByUser(user_id) => {
                        let participant_info = db_ctx
                            .get_user_by_id(user_id)
                            .map_err(|e| {
                                log::error!(
                                    "Failed to resolve stop issuer in legalvote protocol, {}",
                                    e
                                );
                                ProtocolError::InternalError
                            })?
                            .into();

                        StopKind::ByParticipant(participant_info)
                    }
                })
            }
            VoteEvent::FinalResults(results) => {
                final_results = Some(results);
            }
            VoteEvent::Cancel(c) => cancel = Some(c),
        }
    }

    let vote_result = if let Some(cancel) = cancel {
        VoteResult::Failed(FailReason::Canceled(cancel))
    } else if let Some(stop_kind) = stop_kind {
        if let Some(final_results) = final_results {
            match final_results {
                FinalResults::Valid(votes) => VoteResult::Success(Success { stop_kind, votes }),
                FinalResults::Invalid(invalid) => {
                    VoteResult::Failed(FailReason::InvalidResults(invalid))
                }
            }
        } else {
            log::error!("Missing final results in legalvote protocol");
            return Err(ProtocolError::InvalidProtocol);
        }
    } else {
        log::error!("Missing `Stop` or `Cancel` entry in legalvote protocol");
        return Err(ProtocolError::InvalidProtocol);
    };

    let users = db_ctx.get_users_by_ids(&user_ids).map_err(|e| {
        log::error!(
            "Failed to get users by id while parsing legalvote protocol {}",
            e
        );
        ProtocolError::InternalError
    })?;

    if users.len() != raw_voters.len() {
        log::error!("Could not resolve all voters in legalvote protocol");
        return Err(ProtocolError::InternalError);
    }

    let mut voters = vec![];

    for user in users {
        let vote_option = raw_voters.remove(&user.id).ok_or_else(|| {
            log::error!("Missing user while mapping vote options in legal vote protocol parsing");
            ProtocolError::InvalidProtocol
        })?;

        let participant = ParticipantInfo {
            firstname: user.firstname,
            lastname: user.lastname,
            email: user.email,
        };

        voters.push(Voter {
            participant,
            vote_option,
        });
    }

    let settings = settings.ok_or_else(|| {
        log::error!("Missing settings in legalvote protocol");
        ProtocolError::InvalidProtocol
    })?;

    Ok(LegalvoteDetails {
        settings,
        voters,
        vote_result,
    })
}
