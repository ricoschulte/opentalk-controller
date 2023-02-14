// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use crate::TimerId;
use controller::prelude::Timestamp;
use controller_shared::ParticipantId;
use serde::{Deserialize, Serialize};

/// Outgoing websocket messages
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "message")]
pub enum Message {
    /// A timer has been started
    Started(Started),
    /// The current timer has been stopped
    Stopped(Stopped),
    /// A participant updated its ready status
    UpdatedReadyStatus(UpdatedReadyStatus),
    /// An error occurred
    Error(Error),
}

/// The different timer variations
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum Kind {
    /// The timer continues to run until a moderator stops it.
    Stopwatch,
    /// The timer continues to run until its duration expires or if a moderator stops it beforehand.
    Countdown { ends_at: Timestamp },
}

/// A timer has been started
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Started {
    /// The timer id
    pub timer_id: TimerId,
    /// start time of the timer
    pub started_at: Timestamp,
    /// Timer kind
    #[serde(flatten)]
    pub kind: Kind,
    /// Style to use for the timer. Set by the sender.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub style: Option<String>,
    /// The optional title of the timer
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Flag to allow/disallow participants to mark themselves as ready
    pub ready_check_enabled: bool,
}

/// The current timer has been stopped
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Stopped {
    /// The timer id
    pub timer_id: TimerId,
    /// The stop kind
    #[serde(flatten)]
    pub kind: StopKind,
    /// An optional reason to all participants. Set by moderator
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// The stop reason
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "kind", content = "participant_id")]
pub enum StopKind {
    /// The timer has been stopped by a moderator
    ByModerator(ParticipantId),
    /// The timers duration has expired
    Expired,
}

/// Update the ready status
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct UpdatedReadyStatus {
    /// The timer id that the update is for
    pub timer_id: TimerId,
    /// The participant that updated its status
    pub participant_id: ParticipantId,
    /// The new status
    pub status: bool,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "error")]
pub enum Error {
    /// An invalid timer duration has been configured
    InvalidDuration,
    /// The requesting user has insufficient permissions
    InsufficientPermissions,
    /// A timer is already running
    TimerAlreadyRunning,
}

#[cfg(test)]
mod test {
    use std::time::SystemTime;

    use super::*;
    use controller::prelude::{
        chrono::{DateTime, Duration},
        uuid::Uuid,
    };
    use test_util::assert_eq_json;

    #[test]
    fn countdown_started() {
        let started_at: Timestamp = DateTime::from(SystemTime::UNIX_EPOCH).into();
        let ends_at = started_at
            .checked_add_signed(Duration::seconds(5))
            .map(Timestamp::from)
            .unwrap();

        let started = Message::Started(Started {
            timer_id: TimerId(Uuid::nil()),
            started_at,
            kind: Kind::Countdown { ends_at },
            style: Some("coffee_break".into()),
            title: None,
            ready_check_enabled: true,
        });

        assert_eq_json!(started,
        {
            "message": "started",
            "timer_id": "00000000-0000-0000-0000-000000000000",
            "started_at": "1970-01-01T00:00:00Z",
            "kind": "countdown",
            "ends_at": "1970-01-01T00:00:05Z",
            "style": "coffee_break",
            "ready_check_enabled": true
        });
    }

    #[test]
    fn stopwatch_started() {
        let started_at: Timestamp = DateTime::from(SystemTime::UNIX_EPOCH).into();

        let started = Message::Started(Started {
            timer_id: TimerId(Uuid::nil()),
            started_at,
            kind: Kind::Stopwatch,
            style: None,
            title: Some("Testing the timer!".into()),
            ready_check_enabled: false,
        });

        assert_eq_json!(started,
        {
            "message": "started",
            "timer_id": "00000000-0000-0000-0000-000000000000",
            "started_at": "1970-01-01T00:00:00Z",
            "kind": "stopwatch",
            "title": "Testing the timer!",
            "ready_check_enabled": false
        });
    }

    #[test]
    fn stopped_by_moderator() {
        let stopped = Message::Stopped(Stopped {
            timer_id: TimerId(Uuid::nil()),
            kind: StopKind::ByModerator(ParticipantId::nil()),
            reason: Some("A good reason!".into()),
        });

        assert_eq_json!(stopped,
        {
            "message": "stopped",
            "timer_id": "00000000-0000-0000-0000-000000000000",
            "kind": "by_moderator",
            "participant_id": "00000000-0000-0000-0000-000000000000",
            "reason": "A good reason!"
        });
    }

    #[test]
    fn expired() {
        let stopped = Message::Stopped(Stopped {
            timer_id: TimerId(Uuid::nil()),
            kind: StopKind::Expired,
            reason: None,
        });

        assert_eq_json!(stopped,
        {
            "message": "stopped",
            "timer_id": "00000000-0000-0000-0000-000000000000",
            "kind": "expired",
        });
    }

    #[test]
    fn error_insufficient_permission() {
        let stopped = Message::Error(Error::InsufficientPermissions);

        assert_eq_json!(stopped,
        {
            "message": "error",
            "error": "insufficient_permissions",
        });
    }
}
