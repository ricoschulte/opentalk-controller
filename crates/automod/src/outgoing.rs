use crate::config::PublicConfig;
use controller_shared::ParticipantId;
use serde::Serialize;

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "message")]
pub enum Message {
    /// Signals the start of an automod session
    Started(PublicConfig),

    /// Signals the end of an automod session
    Stopped,

    /// The current speaker has been updated.
    ///
    /// See [`SpeakerUpdated`]
    SpeakerUpdated(SpeakerUpdated),

    /// The remaining list has been updated
    ///
    /// See [`RemainingUpdated`]
    RemainingUpdated(RemainingUpdated),

    /// Tell the frontend to start the animation for random selection
    /// The animation must yield the result specified by this message
    StartAnimation(StartAnimation),

    /// An error has occurred
    ///
    /// See [`Error`]
    Error(Error),
}

/// The current speaker state has changed
///
/// This event will ALWAYS notify of a speaker change, even if the speaker is the same participant
/// as before, it MUST be handled as changed.
///
/// Both `history` and `remaining`: If the field is set it will contains the complete new list.
/// If it doesnt exist it must be treated as unchanged.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct SpeakerUpdated {
    /// Speaker field. If [`None`] no speaker is currently selected.
    pub speaker: Option<ParticipantId>,

    /// Optional modification of the history.
    ///
    /// If set the frontend MUST replace its history with the given one.
    /// If not set the frontend MUST keep its current history.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub history: Option<Vec<ParticipantId>>,

    /// Optional modification of the remaining participants.
    ///
    /// Remaining participants must be interpreted differently depending on the selection strategy.
    /// E.g. in the playlist moderation remaining lists the participants left inside the playlist.
    /// All other strategies will use `remaining` (if at all) to list all participants (if public)
    /// that are eligible to be selected.
    ///
    /// This will only be set when using the `playlist` selection_strategy.
    ///
    /// If set the frontend MUST replace its remaining list with the given one.
    /// If not set the frontend MUST keep its current remaining list.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remaining: Option<Vec<ParticipantId>>,
}

/// A modification of the remaining list has taken place, because someone edited the list by hand or
/// it got modified because a participant left/joined
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct RemainingUpdated {
    pub remaining: Vec<ParticipantId>,
}

/// Tells the frontend to start a 'random' draw animation (e.g. wheel of names)
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct StartAnimation {
    pub pool: Vec<ParticipantId>,
    pub result: ParticipantId,
}

/// A command from the frontend has triggered an error.
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "error")]
pub enum Error {
    /// The selection made by the frontend was invalid.
    ///
    /// Can originate from the `start`, `yield` or `select` command.
    InvalidSelection,

    /// The issued command can only be issued by a moderator, but the issuer isn't one.
    InsufficientPermissions,
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::config::{FrontendConfig, Parameter, SelectionStrategy};
    use std::time::Duration;
    use test_util::assert_eq_json;

    #[test]
    fn started_message() {
        let message = Message::Started(
            FrontendConfig {
                parameter: Parameter {
                    selection_strategy: SelectionStrategy::None,
                    show_list: true,
                    consider_hand_raise: false,
                    time_limit: Some(Duration::from_secs(5)),
                    allow_double_selection: false,
                    animation_on_random: true,
                },
                history: vec![ParticipantId::nil()],
                remaining: vec![ParticipantId::nil()],
            }
            .into_public(),
        );

        assert_eq_json!(
            message,
            {
                "message": "started",
                "selection_strategy": "none",
                "show_list": true,
                "consider_hand_raise": false,
                "time_limit": 5000,
                "allow_double_selection": false,
                "animation_on_random": true,
                "history": [
                    "00000000-0000-0000-0000-000000000000"
                ],
                "remaining": [
                    "00000000-0000-0000-0000-000000000000"
                ]
              }
        );
    }

    #[test]
    fn stopped_message() {
        let message = Message::Stopped;

        assert_eq_json!(
            message,
            {
                "message": "stopped"
            }
        );
    }

    #[test]
    fn speaker_update_message() {
        let message = Message::SpeakerUpdated(SpeakerUpdated {
            speaker: Some(ParticipantId::nil()),
            history: Some(vec![]),
            remaining: Some(vec![ParticipantId::nil()]),
        });

        assert_eq_json!(
            message,
            {
                "message": "speaker_updated",
                "speaker": "00000000-0000-0000-0000-000000000000",
                "history": [],
                "remaining": [
                    "00000000-0000-0000-0000-000000000000"
                ]
            }
        );
    }

    #[test]
    fn remaining_update_message() {
        let message = Message::RemainingUpdated(RemainingUpdated {
            remaining: vec![ParticipantId::new_test(1), ParticipantId::new_test(2)],
        });

        assert_eq_json!(
            message,
            {
                "message": "remaining_updated",
                "remaining": [
                    "00000000-0000-0000-0000-000000000001",
                    "00000000-0000-0000-0000-000000000002"
                ]
            }
        );
    }

    #[test]
    fn error_invalid_selection_message() {
        let message = Message::Error(Error::InvalidSelection);

        assert_eq_json!(
            message,
            {
                "message":"error",
                "error":"invalid_selection"
            }
        );
    }

    #[test]
    fn error_invalid_insufficient_permissions() {
        let message = Message::Error(Error::InsufficientPermissions);

        assert_eq_json!(
            message,
            {
                "message":"error",
                "error":"insufficient_permissions"
            }
        );
    }
}
