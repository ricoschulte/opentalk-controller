use crate::TimerId;
use controller_shared::ParticipantId;
use serde::{Deserialize, Serialize};

/// Outgoing websocket messages
#[derive(Debug, Serialize, PartialEq)]
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

/// A timer has been started
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct Started {
    /// The timer id
    pub timer_id: TimerId,
    /// The timer kind
    pub kind: TimerKind,
    /// The duration of the timer
    ///
    /// When the timer kind is `CountUp`, the duration indicates the already passed duration.
    /// When the timer kind is `CountDown`, the duration indicates the remaining duration
    pub duration: u64,
    /// The optional title of the timer
    pub title: Option<String>,
    /// Flag to allow/disallow participants to mark themselves as ready
    pub ready_check_enabled: bool,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TimerKind {
    /// Count up the timer (like a stopwatch)
    CountUp,
    /// Count down the timer
    CountDown,
}

/// The current timer has been stopped
#[derive(Debug, Serialize, Deserialize, PartialEq)]
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
#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case", tag = "kind", content = "participant_id")]
pub enum StopKind {
    /// The timer has been stopped by a moderator
    ByModerator(ParticipantId),
    /// The timers duration has expired
    Expired,
    /// The creator of the timer has left the room
    CreatorLeft,
}

/// Update the ready status
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct UpdatedReadyStatus {
    /// The timer id that the update is for
    pub timer_id: TimerId,
    /// The participant that updated its status
    pub participant_id: ParticipantId,
    /// The new status
    pub status: bool,
}

#[derive(Debug, Serialize, PartialEq)]
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
    use super::*;
    use controller::prelude::uuid::Uuid;
    use test_util::assert_eq_json;

    #[test]
    fn started() {
        let started = Message::Started(Started {
            timer_id: TimerId(Uuid::nil()),
            kind: TimerKind::CountUp,
            duration: 5,
            title: Some("Testing the timer!".into()),
            ready_check_enabled: false,
        });

        assert_eq_json!(started,
        {
            "message": "started",
            "timer_id": "00000000-0000-0000-0000-000000000000",
            "kind": "count_up",
            "duration": 5,
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
    fn creator_left() {
        let stopped = Message::Stopped(Stopped {
            timer_id: TimerId(Uuid::nil()),
            kind: StopKind::CreatorLeft,
            reason: None,
        });

        assert_eq_json!(stopped,
        {
            "message": "stopped",
            "timer_id": "00000000-0000-0000-0000-000000000000",
            "kind": "creator_left",
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
