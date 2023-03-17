// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use super::super::{CancelReason, FinalResults, Parameters, VoteOption};
use crate::{legal_votes::types::Token, users::UserId};
use chrono::{DateTime, Utc};
use redis_args::{FromRedisValue, ToRedisArgs};
use serde::{Deserialize, Serialize};
use types::core::ParticipantId;

/// A legal vote protocol entry
///
/// Contains the event that will be logged in the vote protocol with some meta data.
#[derive(Debug, Clone, Serialize, PartialEq, Eq, Deserialize, ToRedisArgs, FromRedisValue)]
#[to_redis_args(serde)]
#[from_redis_value(serde)]
pub struct ProtocolEntry {
    /// The time that an entry got created
    pub timestamp: Option<DateTime<Utc>>,
    /// The event of this entry
    pub event: VoteEvent,
}

impl ProtocolEntry {
    /// Create a new protocol entry
    pub fn new(event: VoteEvent) -> Self {
        Self::new_with_time(Utc::now(), event)
    }

    pub fn new_with_optional_time(timestamp: Option<DateTime<Utc>>, event: VoteEvent) -> Self {
        Self { timestamp, event }
    }

    /// Create a new protocol entry using the provided `timestamp`
    pub fn new_with_time(timestamp: DateTime<Utc>, event: VoteEvent) -> Self {
        Self::new_with_optional_time(Some(timestamp), event)
    }
}

/// A event related to an active vote
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToRedisArgs, FromRedisValue)]
#[serde(rename_all = "snake_case", tag = "event")]
#[to_redis_args(serde)]
#[from_redis_value(serde)]
pub enum VoteEvent {
    /// The vote started
    Start(Start),
    /// A vote has been casted
    Vote(Vote),
    /// The vote has been stopped
    Stop(StopKind),
    /// The final vote results
    FinalResults(FinalResults),
    /// The vote has been canceled
    Cancel(Cancel),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Start {
    /// User id of the initiator
    pub issuer: UserId,
    /// Vote parameters
    pub parameters: Parameters,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserInfo {
    /// The user id of the voting user
    pub issuer: UserId,
    /// The users participant id, used when reducing the protocol for the frontend
    pub participant_id: ParticipantId,
}

/// A vote entry mapped to a specific user
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Vote {
    /// User info of the voting participant
    ///
    /// Is `None` if the vote is hidden
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(flatten)]
    pub user_info: Option<UserInfo>,
    /// The token used for voting
    pub token: Token,
    /// The chosen vote option
    pub option: VoteOption,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopKind {
    /// A normal vote stop issued by a user. Contains the UserId of the issuer
    ByUser(UserId),
    /// The vote has been stopped automatically because all allowed users have voted
    Auto,
    /// The vote expired due to a set duration
    Expired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cancel {
    /// The user id of the entry issuer
    pub issuer: UserId,
    /// The reason for the cancel
    #[serde(flatten)]
    pub reason: CancelReason,
}
