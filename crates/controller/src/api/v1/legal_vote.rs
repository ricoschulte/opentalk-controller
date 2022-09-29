use super::response::error::ApiError;
use super::{ApiResponse, PagePaginationQuery};
use actix_web::get;
use actix_web::web::{Data, Json, Path, Query, ReqData};
use anyhow::Result;
use chrono::{DateTime, Utc};
use database::{Db, DbConnection};
use db_storage::legal_votes::types::protocol::v1::{self, VoteEvent};
use db_storage::legal_votes::types::protocol::{self, Protocol};
use db_storage::legal_votes::types::{
    CancelReason, FinalResults, Invalid, Parameters, UserParameters, VoteOption, Votes,
};
use db_storage::legal_votes::{LegalVote, LegalVoteId};
use db_storage::rooms::RoomId;
use db_storage::users::User;
use kustos::prelude::AccessMethod;
use kustos::{AccessibleResources, Authz};
use serde::Serialize;
use std::collections::HashMap;

/// Wrapper struct to display invalid protocols to the API caller
#[derive(Debug, Serialize)]
pub struct LegalVoteEntry {
    /// The legal vote id
    legal_vote_id: LegalVoteId,
    /// The parsing result of the protocol
    #[serde(flatten)]
    protocol_result: ProtocolResult,
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum ProtocolResult {
    /// Successfully parsed protocol
    Ok(LegalVoteDetails),
    /// Failed to parse protocol
    Error(ProtocolError),
}

#[derive(thiserror::Error, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "error")]
pub enum ProtocolError {
    #[error("LegalVote protocol is invalid")]
    InvalidProtocol,
    #[error("An internal server error occurred while parsing the legal vote protocol")]
    InternalError,
}

/// Details of a passed legal vote
#[derive(Debug, Serialize)]
pub struct LegalVoteDetails {
    /// The legal vote settings
    #[serde(flatten)]
    pub settings: Settings,
    /// A list of participants that voted on the legal vote
    pub voters: Vec<Voter>,
    /// The results of the legal vote
    pub vote_result: VoteResult,
}

/// Settings of a legal vote
#[derive(Debug, Serialize)]
pub struct Settings {
    /// The participant info of the legal vote creator
    pub created_by: ParticipantInfo,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<u64>,
}

/// Represents a participant in a legal vote
#[derive(Clone, Debug, Serialize)]
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

/// A participant that voted in a legal vote
#[derive(Debug, Serialize)]
pub struct Voter {
    #[serde(flatten)]
    participant: ParticipantInfo,
    /// The chosen vote option
    vote_option: VoteOption,
}

/// The results of a legal vote
#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case", tag = "status")]
pub enum VoteResult {
    Success(Success),
    Failed(FailReason),
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct Success {
    #[serde(flatten)]
    stop_kind: StopKind,
    #[serde(flatten)]
    votes: Votes,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case", tag = "stop_kind", content = "stopped_by")]
pub enum StopKind {
    /// A normal vote stop issued by a user. Contains the UserId of the issuer
    ByParticipant(ParticipantInfo),
    /// The vote has been stopped automatically because all allowed users have voted
    Auto,
    /// The vote expired due to a set duration
    Expired,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case", tag = "cause")]
pub enum FailReason {
    Canceled(CancelInfo),
    InvalidResults(Invalid),
}

/// Cancel struct that contains the `[ParticipantInfo]` instead of a `[UserId]` of the cancel issuer
#[derive(Debug, Clone, Serialize)]
pub struct CancelInfo {
    /// The participant info of the cancel issuer
    pub canceled_by: ParticipantInfo,
    /// The reason for the cancel
    #[serde(flatten)]
    pub reason: CancelReason,
}

/// API Endpoint *GET /rooms/{room_id}/legal_votes*
///
/// Returns a JSON array of the legal votes created in this room that the user can access
#[get("/rooms/{room_id}/legal_votes")]
pub async fn get_all_for_room(
    db: Data<Db>,
    authz: Data<Authz>,
    pagination: Query<PagePaginationQuery>,
    room_id: Path<RoomId>,
    current_user: ReqData<User>,
) -> Result<ApiResponse<Vec<LegalVoteEntry>>, ApiError> {
    let room_id = room_id.into_inner();
    let PagePaginationQuery { per_page, page } = pagination.into_inner();

    let accessible_legal_votes: AccessibleResources<LegalVoteId> = authz
        .get_accessible_resources_for_user(current_user.id, AccessMethod::Get)
        .await?;

    let (legal_votes, count) = crate::block(move || -> database::Result<_> {
        let conn = db.get_conn()?;

        let (legal_votes, count) = match accessible_legal_votes {
            AccessibleResources::List(vote_ids) => {
                LegalVote::get_for_room_by_ids_paginated(&conn, room_id, &vote_ids, per_page, page)?
            }
            AccessibleResources::All => {
                LegalVote::get_for_room_paginated(&conn, room_id, per_page, page)?
            }
        };

        let mut detailed_votes = Vec::new();

        for legal_vote in legal_votes {
            match parse_protocol(&conn, legal_vote.protocol) {
                Ok(legal_vote_detailed) => detailed_votes.push(LegalVoteEntry {
                    legal_vote_id: legal_vote.id,
                    protocol_result: ProtocolResult::Ok(legal_vote_detailed),
                }),
                Err(protocol_error) => {
                    detailed_votes.push(LegalVoteEntry {
                        legal_vote_id: legal_vote.id,
                        protocol_result: ProtocolResult::Error(protocol_error),
                    });
                }
            }
        }

        Ok((detailed_votes, count))
    })
    .await??;

    Ok(ApiResponse::new(legal_votes).with_page_pagination(per_page, page, count))
}

/// API Endpoint *GET /legal_votes*
///
/// Returns a JSON array of the legal votes that the user can access
#[get("/legal_votes")]
pub async fn get_all(
    db: Data<Db>,
    authz: Data<Authz>,
    pagination: Query<PagePaginationQuery>,
    current_user: ReqData<User>,
) -> Result<ApiResponse<Vec<LegalVoteEntry>>, ApiError> {
    let PagePaginationQuery { per_page, page } = pagination.into_inner();

    let accessible_legal_votes: AccessibleResources<LegalVoteId> = authz
        .get_accessible_resources_for_user(current_user.id, AccessMethod::Get)
        .await?;

    let (legal_votes, count) = crate::block(move || -> database::Result<_> {
        let conn = db.get_conn()?;

        let (legal_votes, count) = match accessible_legal_votes {
            AccessibleResources::List(vote_ids) => {
                LegalVote::get_by_ids_paginated(&conn, &vote_ids, per_page, page)?
            }
            AccessibleResources::All => LegalVote::get_all_paginated(&conn, per_page, page)?,
        };

        let mut detailed_votes = Vec::new();

        for legal_vote in legal_votes {
            match parse_protocol(&conn, legal_vote.protocol) {
                Ok(legal_vote_detailed) => detailed_votes.push(LegalVoteEntry {
                    legal_vote_id: legal_vote.id,
                    protocol_result: ProtocolResult::Ok(legal_vote_detailed),
                }),
                Err(protocol_error) => {
                    detailed_votes.push(LegalVoteEntry {
                        legal_vote_id: legal_vote.id,
                        protocol_result: ProtocolResult::Error(protocol_error),
                    });
                }
            }
        }

        Ok((detailed_votes, count))
    })
    .await??;

    Ok(ApiResponse::new(legal_votes).with_page_pagination(per_page, page, count))
}

/// API Endpoint *GET /legal_votes/{legal_vote_id}*
///
/// Returns the specified legal vote as [`LegalVoteEntry`]
#[get("/legal_votes/{legal_vote_id}")]
pub async fn get_specific(
    db: Data<Db>,
    authz: Data<Authz>,
    legal_vote_id: Path<LegalVoteId>,
    current_user: ReqData<User>,
) -> Result<Json<LegalVoteEntry>, ApiError> {
    let legal_vote_id = legal_vote_id.into_inner();

    let accessible_legal_votes: AccessibleResources<LegalVoteId> = authz
        .get_accessible_resources_for_user(current_user.id, AccessMethod::Get)
        .await?;

    match accessible_legal_votes {
        kustos::AccessibleResources::List(vote_ids) => {
            if !vote_ids.contains(&legal_vote_id) {
                return Err(ApiError::forbidden());
            }
        }
        kustos::AccessibleResources::All => (),
    }

    let legal_vote_detailed = crate::block(move || -> database::Result<_> {
        let conn = db.get_conn()?;

        let legal_vote = LegalVote::get(&conn, legal_vote_id)?;

        let legal_vote_entry = match parse_protocol(&conn, legal_vote.protocol) {
            Ok(legal_vote_detailed) => LegalVoteEntry {
                legal_vote_id: legal_vote.id,
                protocol_result: ProtocolResult::Ok(legal_vote_detailed),
            },
            Err(protocol_error) => LegalVoteEntry {
                legal_vote_id: legal_vote.id,
                protocol_result: ProtocolResult::Error(protocol_error),
            },
        };

        Ok(legal_vote_entry)
    })
    .await??;

    Ok(Json(legal_vote_detailed))
}

fn parse_protocol(
    conn: &DbConnection,
    protocol: Protocol,
) -> Result<LegalVoteDetails, ProtocolError> {
    match protocol.version {
        1 => {
            let entries: Vec<v1::ProtocolEntry> = serde_json::from_str(protocol.entries.get())
                .map_err(|e| {
                    log::error!("Failed to deserialize v1 protocol entries {}", e);
                    ProtocolError::InvalidProtocol
                })?;

            parse_v1_entries(conn, entries)
        }
        unknown => {
            log::error!("Unknown legal vote protocol version '{}'", unknown);
            Err(ProtocolError::InvalidProtocol)
        }
    }
}

/// Converts a list of v1 protocol entries to [`LegalVoteDetails`]
fn parse_v1_entries(
    conn: &DbConnection,
    entries: Vec<v1::ProtocolEntry>,
) -> Result<LegalVoteDetails, ProtocolError> {
    if entries.is_empty() {
        log::error!("legal vote protocol is empty");
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
                    legal_vote_id: _,
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

                let initiator = User::get(conn, start.issuer)
                    .map_err(|e| {
                        log::error!(
                            "Failed to resolve vote initiator in legal vote protocol, {}",
                            e
                        );
                        ProtocolError::InternalError
                    })?
                    .into();

                settings = Some(Settings {
                    created_by: initiator,
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
                        let participant_info = User::get(conn, user_id)
                            .map_err(|e| {
                                log::error!(
                                    "Failed to resolve stop issuer in legal vote protocol, {}",
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
        let participant_info = User::get(conn, cancel.issuer)
            .map_err(|e| {
                log::error!(
                    "Failed to resolve stop issuer in legal vote protocol, {}",
                    e
                );
                ProtocolError::InternalError
            })?
            .into();

        VoteResult::Failed(FailReason::Canceled(CancelInfo {
            canceled_by: participant_info,
            reason: cancel.reason,
        }))
    } else if let Some(stop_kind) = stop_kind {
        if let Some(final_results) = final_results {
            match final_results {
                FinalResults::Valid(votes) => VoteResult::Success(Success { stop_kind, votes }),
                FinalResults::Invalid(invalid) => {
                    VoteResult::Failed(FailReason::InvalidResults(invalid))
                }
            }
        } else {
            log::error!("Missing final results in legal vote protocol");
            return Err(ProtocolError::InvalidProtocol);
        }
    } else {
        log::error!("Missing `Stop` or `Cancel` entry in legal vote protocol");
        return Err(ProtocolError::InvalidProtocol);
    };

    let users = User::get_all_by_ids(conn, &user_ids).map_err(|e| {
        log::error!(
            "Failed to get users by id while parsing legal vote protocol {}",
            e
        );
        ProtocolError::InternalError
    })?;

    if users.len() != raw_voters.len() {
        log::error!("Could not resolve all voters in legal vote protocol");
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
        log::error!("Missing settings in legal vote protocol");
        ProtocolError::InvalidProtocol
    })?;

    Ok(LegalVoteDetails {
        settings,
        voters,
        vote_result,
    })
}

#[cfg(test)]
mod test {
    use super::*;
    use chrono::TimeZone;
    use test_util::assert_eq_json;
    use uuid::Uuid;

    #[test]
    fn successful_legal_vote_entry() {
        let test_participant = ParticipantInfo {
            firstname: "test".into(),
            lastname: "tester".into(),
            email: "test.tester@heinlein-video.de".into(),
        };

        let legal_vote_entry = LegalVoteEntry {
            legal_vote_id: LegalVoteId::from(Uuid::from_u128(1)),
            protocol_result: ProtocolResult::Ok(LegalVoteDetails {
                settings: Settings {
                    created_by: test_participant.clone(),
                    start_time: Utc.ymd(1970, 1, 1).and_hms(0, 0, 0),
                    max_votes: 1,
                    name: "Test Vote".into(),
                    subtitle: "A subtitle".into(),
                    topic: "Does the test work".into(),
                    enable_abstain: false,
                    auto_stop: false,
                    duration: Some(60),
                },
                voters: vec![Voter {
                    participant: test_participant.clone(),
                    vote_option: VoteOption::Yes,
                }],
                vote_result: VoteResult::Success(Success {
                    stop_kind: StopKind::ByParticipant(test_participant),
                    votes: Votes {
                        yes: 1,
                        no: 0,
                        abstain: None,
                    },
                }),
            }),
        };

        assert_eq_json!(
            legal_vote_entry,
            {
                "legal_vote_id": "00000000-0000-0000-0000-000000000001",
                "created_by": {
                  "firstname": "test",
                  "lastname": "tester",
                  "email": "test.tester@heinlein-video.de"
                },
                "start_time": "1970-01-01T00:00:00Z",
                "max_votes": 1,
                "name": "Test Vote",
                "subtitle": "A subtitle",
                "topic": "Does the test work",
                "enable_abstain": false,
                "auto_stop": false,
                "duration": 60,
                "voters": [
                  {
                    "firstname": "test",
                    "lastname": "tester",
                    "email": "test.tester@heinlein-video.de",
                    "vote_option": "yes"
                  }
                ],
                "vote_result": {
                  "status": "success",
                  "stop_kind": "by_participant",
                  "stopped_by": {
                    "firstname": "test",
                    "lastname": "tester",
                    "email": "test.tester@heinlein-video.de"
                  },
                  "yes": 1,
                  "no": 0
                }
              }
        );
    }

    #[test]
    fn canceled_legal_vote_entry() {
        let test_participant = ParticipantInfo {
            firstname: "test".into(),
            lastname: "tester".into(),
            email: "test.tester@heinlein-video.de".into(),
        };

        let legal_vote_entry = LegalVoteEntry {
            legal_vote_id: LegalVoteId::from(Uuid::from_u128(1)),
            protocol_result: ProtocolResult::Ok(LegalVoteDetails {
                settings: Settings {
                    created_by: test_participant.clone(),
                    start_time: Utc.ymd(1970, 1, 1).and_hms(0, 0, 0),
                    max_votes: 1,
                    name: "Test Vote".into(),
                    subtitle: "A subtitle".into(),
                    topic: "Does the test work".into(),
                    enable_abstain: false,
                    auto_stop: false,
                    duration: None,
                },
                voters: vec![Voter {
                    participant: test_participant.clone(),
                    vote_option: VoteOption::Yes,
                }],
                vote_result: VoteResult::Failed(FailReason::Canceled(CancelInfo {
                    canceled_by: test_participant,
                    reason: CancelReason::Custom("Some custom reason".into()),
                })),
            }),
        };

        assert_eq_json!(
            legal_vote_entry,
            {
                "legal_vote_id": "00000000-0000-0000-0000-000000000001",
                "created_by": {
                  "firstname": "test",
                  "lastname": "tester",
                  "email": "test.tester@heinlein-video.de"
                },
                "start_time": "1970-01-01T00:00:00Z",
                "max_votes": 1,
                "name": "Test Vote",
                "subtitle": "A subtitle",
                "topic": "Does the test work",
                "enable_abstain": false,
                "auto_stop": false,
                "voters": [
                  {
                    "firstname": "test",
                    "lastname": "tester",
                    "email": "test.tester@heinlein-video.de",
                    "vote_option": "yes"
                  }
                ],
                "vote_result": {
                  "status": "failed",
                  "cause": "canceled",
                  "canceled_by": {
                    "firstname": "test",
                    "lastname": "tester",
                    "email": "test.tester@heinlein-video.de"
                  },
                  "reason": "custom",
                  "custom": "Some custom reason"
                }
            }
        );
    }

    #[test]
    fn invalid_legal_vote_entry() {
        let test_participant = ParticipantInfo {
            firstname: "test".into(),
            lastname: "tester".into(),
            email: "test.tester@heinlein-video.de".into(),
        };

        let legal_vote_entry = LegalVoteEntry {
            legal_vote_id: LegalVoteId::from(Uuid::from_u128(1)),
            protocol_result: ProtocolResult::Ok(LegalVoteDetails {
                settings: Settings {
                    created_by: test_participant.clone(),
                    start_time: Utc.ymd(1970, 1, 1).and_hms(0, 0, 0),
                    max_votes: 1,
                    name: "Test Vote".into(),
                    subtitle: "A subtitle".into(),
                    topic: "Does the test work".into(),
                    enable_abstain: false,
                    auto_stop: false,
                    duration: Some(60),
                },
                voters: vec![Voter {
                    participant: test_participant,
                    vote_option: VoteOption::Yes,
                }],
                vote_result: VoteResult::Failed(FailReason::InvalidResults(
                    Invalid::VoteCountInconsistent,
                )),
            }),
        };

        assert_eq_json!(
            legal_vote_entry,
            {
                "legal_vote_id": "00000000-0000-0000-0000-000000000001",
                "created_by": {
                    "firstname": "test",
                    "lastname": "tester",
                    "email": "test.tester@heinlein-video.de"
                },
                "start_time": "1970-01-01T00:00:00Z",
                "max_votes": 1,
                "name": "Test Vote",
                "subtitle": "A subtitle",
                "topic": "Does the test work",
                "enable_abstain": false,
                "auto_stop": false,
                "duration": 60,
                "voters": [
                    {
                        "firstname": "test",
                        "lastname": "tester",
                        "email": "test.tester@heinlein-video.de",
                        "vote_option": "yes"
                    }
                ],
                "vote_result": {
                    "status": "failed",
                    "cause": "invalid_results",
                    "reason": "vote_count_inconsistent"
                }
            }
        );
    }
}
