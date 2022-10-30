// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use crate::TimerId;
use serde::Deserialize;

/// Incoming websocket messages
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case", tag = "action")]
pub enum Message {
    /// Start a new timer
    Start(Start),
    /// Stop a running timer
    Stop(Stop),
    /// Update the ready status
    UpdateReadyStatus(UpdateReadyStatus),
}

/// Start a new timer
#[derive(Debug, Deserialize)]
pub struct Start {
    /// The duration of the timer.
    ///
    /// When not set, the timer behaves like a stopwatch, waiting for a stop by moderator
    pub duration: Option<u64>,
    /// An optional title for the timer
    pub title: Option<String>,
    /// Flag to allow/disallow participants to mark themselves as ready
    #[serde(default)]
    pub enable_ready_check: bool,
}

/// Stop a running timer
#[derive(Debug, Deserialize)]
pub struct Stop {
    /// The timer id
    pub timer_id: TimerId,
    /// An optional reason for the stop
    pub reason: Option<String>,
}

/// Update the ready status
#[derive(Debug, Deserialize)]
pub struct UpdateReadyStatus {
    /// The timer id
    pub timer_id: TimerId,
    /// The new status
    pub status: bool,
}

#[cfg(test)]
mod test {
    use super::*;
    use controller::prelude::uuid::Uuid;
    use pretty_assertions::assert_eq;
    use test_util::serde_json;

    #[test]
    fn start() {
        let json = r#"
        {
            "action": "start",
            "duration": 5,
            "title": "Testing the timer!",
            "enable_ready_check": false
        }
        "#;

        match serde_json::from_str(json).unwrap() {
            Message::Start(Start {
                duration,
                title,
                enable_ready_check,
            }) => {
                assert_eq!(duration, Some(5));
                assert_eq!(title, Some("Testing the timer!".into()));
                assert!(!enable_ready_check);
            }
            unexpected => panic!("Expected start message, got: {:?}", unexpected),
        }
    }

    #[test]
    fn stop() {
        let json = r#"
        {
            "action": "stop",
            "timer_id": "00000000-0000-0000-0000-000000000000",
            "reason": "test"
        }
        "#;

        match serde_json::from_str(json).unwrap() {
            Message::Stop(Stop { timer_id, reason }) => {
                assert_eq!(reason, Some("test".into()));
                assert_eq!(timer_id, TimerId(Uuid::nil()))
            }
            unexpected => panic!("Expected stop message, got: {:?}", unexpected),
        }
    }

    #[test]
    fn update_ready_status() {
        let json = r#"
        {
            "action": "update_ready_status",
            "timer_id": "00000000-0000-0000-0000-000000000000",
            "status": true
        }
        "#;

        match serde_json::from_str(json).unwrap() {
            Message::UpdateReadyStatus(UpdateReadyStatus { timer_id, status }) => {
                assert!(status);
                assert_eq!(timer_id, TimerId(Uuid::nil()))
            }
            unexpected => panic!("Expected ready message, got: {:?}", unexpected),
        }
    }
}
