use controller::prelude::{
    anyhow::{Context, Result},
    chrono::{DateTime, Utc},
    futures, RedisConnection, SignalingRoomId,
};
use db_storage::{
    legal_votes::{
        types::{
            protocol::v1::{Cancel, ProtocolEntry, StopKind as ProtocolStopKind, VoteEvent},
            FinalResults, Invalid, Parameters,
        },
        LegalVoteId,
    },
    users::UserId,
};
use serde::Deserialize;
use serde::Serialize;

use crate::{outgoing, storage};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "stop_kind")]
pub enum StopKind {
    /// A normal vote stop issued by a user. Contains the UserId of the issuer
    ByUser { stopped_by: UserId },
    /// The vote has been stopped automatically because all allowed users have voted
    Auto,
    /// The vote expired due to a set duration
    Expired,
}

impl From<ProtocolStopKind> for StopKind {
    fn from(value: ProtocolStopKind) -> Self {
        match value {
            ProtocolStopKind::ByUser(stopped_by) => Self::ByUser { stopped_by },
            ProtocolStopKind::Auto => Self::Auto,
            ProtocolStopKind::Expired => Self::Expired,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum VoteState {
    Started,
    Finished {
        #[serde(flatten)]
        stop_kind: StopKind,
        #[serde(flatten)]
        results: outgoing::Results,
    },
    Canceled(Cancel),
    Invalid(Invalid),
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct VoteSummary {
    #[serde(flatten)]
    pub parameters: Parameters,
    #[serde(flatten)]
    pub state: VoteState,
    pub end_time: Option<DateTime<Utc>>,
}

impl VoteSummary {
    pub async fn load_from_protocol(
        mut redis_conn: RedisConnection,
        room_id: SignalingRoomId,
        vote_id: LegalVoteId,
    ) -> Result<Self> {
        let protocol = storage::protocol::get(&mut redis_conn, room_id, vote_id).await?;
        Self::from_protocol(&protocol)
    }

    pub fn from_protocol(protocol: &[ProtocolEntry]) -> Result<Self> {
        let mut parameters = None;
        let mut state = None;
        let mut end_time = None;
        let mut stop_kind = None;

        for entry in protocol {
            match entry.event.clone() {
                VoteEvent::Start(start) => {
                    parameters = Some(start.parameters.clone());
                    state = Some(VoteState::Started);
                }
                VoteEvent::Vote(_) => {}
                VoteEvent::Stop(kind) => {
                    stop_kind = Some(kind);
                    if entry.timestamp.is_some() {
                        end_time = entry.timestamp;
                    }
                }
                VoteEvent::FinalResults(results) => match results {
                    FinalResults::Valid(tally) => {
                        let voting_record = Some(
                            storage::protocol::extract_voting_record_from_protocol(protocol)?,
                        );
                        let stop_kind = StopKind::from(stop_kind.clone().with_context(|| {
                            "Missing 'Stop' entry before 'FinalResults' in legal vote protocol"
                        })?);
                        state = Some(VoteState::Finished {
                            stop_kind,
                            results: outgoing::Results {
                                tally,
                                voting_record,
                            },
                        });
                    }
                    FinalResults::Invalid(reason) => {
                        state = Some(VoteState::Invalid(reason));
                    }
                },
                VoteEvent::Cancel(cancel) => {
                    state = Some(VoteState::Canceled(cancel.clone()));
                    if entry.timestamp.is_some() {
                        end_time = entry.timestamp;
                    }
                }
            }
        }

        let parameters =
            parameters.with_context(|| "Missing 'Start' entry in legal vote protocol")?;
        let state =
            state.with_context(|| "Unable to determine state of legal vote from protocol")?;

        Ok(Self {
            parameters,
            state,
            end_time,
        })
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct FrontendData {
    pub votes: Vec<VoteSummary>,
}

impl FrontendData {
    pub async fn load_from_history(
        redis_conn: &mut RedisConnection,
        room_id: SignalingRoomId,
    ) -> Result<Self> {
        let current_vote = storage::current_legal_vote_id::get(redis_conn, room_id).await?;
        let vote_futures = storage::history::get(redis_conn, room_id)
            .await?
            .into_iter()
            .chain(current_vote.into_iter())
            .map(|vote_id| VoteSummary::load_from_protocol(redis_conn.clone(), room_id, vote_id))
            .collect::<Vec<_>>();
        let votes = futures::future::join_all(vote_futures)
            .await
            .into_iter()
            .collect::<Result<Vec<_>>>()?;
        Ok(Self { votes })
    }
}

#[cfg(test)]
mod test {
    use std::{collections::HashMap, str::FromStr};

    use super::*;
    use controller::prelude::{chrono::TimeZone, uuid::uuid};
    use controller_shared::ParticipantId;
    use db_storage::{
        legal_votes::{
            types::{Tally, Token, UserParameters, VoteOption},
            LegalVoteId,
        },
        users::UserId,
    };
    use test_util::{assert_eq_json, serde_json::json};

    #[test]
    fn serialize_vote_summary_started() {
        let summary = VoteSummary {
            parameters: Parameters {
                initiator_id: ParticipantId::from(uuid!("2b68f90b-fe35-4f5f-934b-1a3ed59b31f4")),
                legal_vote_id: LegalVoteId::from(uuid!("161b0140-dd33-4f1e-8818-8084e01e158a")),
                start_time: Utc.ymd(1970, 1, 1).and_hms(0, 0, 0),
                max_votes: 3,
                inner: UserParameters {
                    kind: db_storage::legal_votes::types::VoteKind::Pseudonymous,
                    name: "Pseudonymous vote".to_string(),
                    subtitle: None,
                    topic: None,
                    allowed_participants: vec![
                        ParticipantId::from(uuid!("2b68f90b-fe35-4f5f-934b-1a3ed59b31f4")),
                        ParticipantId::from(uuid!("68b9ca2f-f755-4a5f-980e-7fcacd30acc4")),
                        ParticipantId::from(uuid!("d5462f3d-1a78-427c-953b-7d9a95b920d3")),
                    ],
                    enable_abstain: true,
                    auto_close: false,
                    duration: None,
                    create_pdf: false,
                },
                token: None,
            },
            state: VoteState::Started,
            end_time: None,
        };
        let json = json!({
            "kind": "pseudonymous",
            "name": "Pseudonymous vote",
            "subtitle": null,
            "topic": null,
            "initiator_id": "2b68f90b-fe35-4f5f-934b-1a3ed59b31f4",
            "legal_vote_id": "161b0140-dd33-4f1e-8818-8084e01e158a",
            "start_time": "1970-01-01T00:00:00Z",
            "max_votes": 3,
            "allowed_participants": [
                "2b68f90b-fe35-4f5f-934b-1a3ed59b31f4",
                "68b9ca2f-f755-4a5f-980e-7fcacd30acc4",
                "d5462f3d-1a78-427c-953b-7d9a95b920d3",
            ],
            "enable_abstain": true,
            "auto_close": false,
            "duration": null,
            "create_pdf": false,
            "state": "started",
            "end_time": null,
        });
        assert_eq_json!(summary, json);

        assert_eq!(
            serde_json::from_value::<VoteSummary>(json).unwrap(),
            summary
        );
    }

    #[test]
    fn serialize_vote_summary_finished() {
        let summary = VoteSummary {
            parameters: Parameters {
                initiator_id: ParticipantId::from(uuid!("2b68f90b-fe35-4f5f-934b-1a3ed59b31f4")),
                legal_vote_id: LegalVoteId::from(uuid!("161b0140-dd33-4f1e-8818-8084e01e158a")),
                start_time: Utc.ymd(1970, 1, 1).and_hms(0, 0, 0),
                max_votes: 3,
                inner: UserParameters {
                    kind: db_storage::legal_votes::types::VoteKind::Pseudonymous,
                    name: "Pseudonymous vote".to_string(),
                    subtitle: None,
                    topic: None,
                    allowed_participants: vec![
                        ParticipantId::from(uuid!("2b68f90b-fe35-4f5f-934b-1a3ed59b31f4")),
                        ParticipantId::from(uuid!("68b9ca2f-f755-4a5f-980e-7fcacd30acc4")),
                        ParticipantId::from(uuid!("d5462f3d-1a78-427c-953b-7d9a95b920d3")),
                    ],
                    enable_abstain: true,
                    auto_close: false,
                    duration: None,
                    create_pdf: false,
                },
                token: None,
            },
            state: VoteState::Finished {
                stop_kind: StopKind::ByUser {
                    stopped_by: UserId::from(uuid!("67802abe-3f01-4322-8030-a009bd0e4ed5")),
                },
                results: outgoing::Results {
                    tally: Tally {
                        yes: 3,
                        no: 2,
                        abstain: None,
                    },
                    voting_record: Some(outgoing::VotingRecord::TokenVotes(HashMap::from_iter([
                        (Token::from_str("9AMndyeorvB").unwrap(), VoteOption::Yes),
                        (Token::from_str("G9rLx7vkeMD").unwrap(), VoteOption::No),
                        (Token::from_str("Mypgay5rhRj").unwrap(), VoteOption::Yes),
                        (Token::from_str("TjR94viayBf").unwrap(), VoteOption::No),
                        (Token::from_str("UuLLU1sxgPw").unwrap(), VoteOption::Yes),
                    ]))),
                },
            },
            end_time: None,
        };
        let json = json!({
            "kind": "pseudonymous",
            "name": "Pseudonymous vote",
            "subtitle": null,
            "topic": null,
            "initiator_id": "2b68f90b-fe35-4f5f-934b-1a3ed59b31f4",
            "legal_vote_id": "161b0140-dd33-4f1e-8818-8084e01e158a",
            "start_time": "1970-01-01T00:00:00Z",
            "max_votes": 3,
            "allowed_participants": [
                "2b68f90b-fe35-4f5f-934b-1a3ed59b31f4",
                "68b9ca2f-f755-4a5f-980e-7fcacd30acc4",
                "d5462f3d-1a78-427c-953b-7d9a95b920d3",
            ],
            "enable_abstain": true,
            "auto_close": false,
            "duration": null,
            "create_pdf": false,
            "state": "finished",
            "end_time": null,
            "yes": 3,
            "no": 2,
            "voting_record": {
                "9AMndyeorvB":"yes",
                "G9rLx7vkeMD":"no",
                "Mypgay5rhRj":"yes",
                "TjR94viayBf":"no",
                "UuLLU1sxgPw":"yes",
            },
            "stop_kind": "by_user",
            "stopped_by": "67802abe-3f01-4322-8030-a009bd0e4ed5"
        });

        assert_eq_json!(summary, json);
        assert_eq!(
            serde_json::from_value::<VoteSummary>(json).unwrap(),
            summary
        );
    }
}
