//! All websocket commands that can be issued by the frontend

use super::config::Parameter;
use crate::api::signaling::ParticipantId;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case", tag = "action")]
pub enum Message {
    /// Start the auto-moderation with a provided config
    Start(Start),

    /// Set either the allow_list or playlist or both
    Edit(Edit),

    /// Stop the auto-moderation
    Stop,

    /// Select a user to be active speaker
    Select(Select),

    /// User yields it's speaker status
    Yield(Yield),
}

impl Message {
    /// Returns if the issued message requires the participant to be a moderator
    pub fn requires_moderator_privileges(&self) -> bool {
        match self {
            Message::Start(_) | Message::Edit(_) | Message::Stop | Message::Select(_) => true,
            Message::Yield(_) => false,
        }
    }
}

/// Fields that are provided when issuing the start message
#[derive(Debug, Deserialize)]
pub struct Start {
    /// The parameters for the automod session
    #[serde(flatten)]
    pub parameter: Parameter,

    /// See [`super::storage::allow_list`]
    #[serde(default)]
    pub allow_list: Vec<ParticipantId>,

    /// See [`super::storage::playlist`]
    #[serde(default)]
    pub playlist: Vec<ParticipantId>,
}

/// Fields that are provided when issuing the edit message
#[derive(Debug, Deserialize)]
pub struct Edit {
    /// Edit the `allow_list`. If `None`, it should not be edited.
    pub allow_list: Option<Vec<ParticipantId>>,

    /// Edit the `playlist`. If `None`, it should not be edited.
    pub playlist: Option<Vec<ParticipantId>>,
}

/// Moderator command, select the speaker
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case", tag = "how")]
pub enum Select {
    /// Revoke speaker status if exists, do no select a new one
    None,

    /// Select a random speaker
    Random,

    /// Select a specific participant
    Specific(SelectSpecific),
}

/// Fields that are provided when issuing the [`Select::Specific`] command
#[derive(Debug, Deserialize)]
pub struct SelectSpecific {
    /// The participant to be selected
    pub participant: ParticipantId,
}

/// Fields that are provided when issuing the yield message
#[derive(Debug, Deserialize)]
pub struct Yield {
    /// In some cases a user must select the next participant to be speaker
    pub next: Option<ParticipantId>,
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::api::signaling::ws_modules::ee::automod::config::SelectionStrategy;
    use std::time::Duration;

    #[test]
    fn start_message() {
        let json_str = r#"
        {
            "action": "start",
            "selection_strategy": "none",
            "show_list": true,
            "consider_hand_raise": false,
            "time_limit": 10000,
            "pause_time": null,
            "allow_double_selection": false,
            "allow_list": ["00000000-0000-0000-0000-000000000000"],
            "playlist": ["00000000-0000-0000-0000-000000000000"]
        }
        "#;

        let start: Message = serde_json::from_str(json_str).unwrap();

        if let Message::Start(Start {
            parameter:
                Parameter {
                    selection_strategy,
                    show_list,
                    consider_hand_raise,
                    time_limit,
                    pause_time,
                    allow_double_selection,
                },
            allow_list,
            playlist,
        }) = start
        {
            assert_eq!(selection_strategy, SelectionStrategy::None);
            assert!(show_list);
            assert!(!consider_hand_raise);
            assert_eq!(time_limit, Some(Duration::from_secs(10)));
            assert_eq!(pause_time, None);
            assert!(!allow_double_selection);

            assert_eq!(allow_list, vec![ParticipantId::nil()]);
            assert_eq!(playlist, vec![ParticipantId::nil()]);
        } else {
            panic!()
        }
    }

    #[test]
    fn edit_message() {
        let json_str = r#"
        {
            "action": "edit",
            "allow_list": ["00000000-0000-0000-0000-000000000000"]
        }
        "#;

        let start: Message = serde_json::from_str(json_str).unwrap();

        if let Message::Edit(Edit {
            allow_list,
            playlist,
        }) = start
        {
            assert_eq!(allow_list, Some(vec![ParticipantId::nil()]));
            assert_eq!(playlist, None);
        } else {
            panic!()
        }
    }

    #[test]
    fn stop_message() {
        let json_str = r#"
        {
            "action": "stop"
        }
        "#;

        let start: Message = serde_json::from_str(json_str).unwrap();

        if let Message::Stop = start {
            // Ok
        } else {
            panic!()
        }
    }

    #[test]
    fn select_none_message() {
        let json_str = r#"
        {
            "action": "select",
            "how": "none"
        }
        "#;

        let start: Message = serde_json::from_str(json_str).unwrap();

        if let Message::Select(Select::None) = start {
            // Ok
        } else {
            panic!()
        }
    }

    #[test]
    fn select_random_message() {
        let json_str = r#"
        {
            "action": "select",
            "how": "random"
        }
        "#;

        let start: Message = serde_json::from_str(json_str).unwrap();

        if let Message::Select(Select::Random) = start {
            // Ok
        } else {
            panic!()
        }
    }

    #[test]
    fn select_specific_message() {
        let json_str = r#"
        {
            "action": "select",
            "how": "specific",
            "participant": "00000000-0000-0000-0000-000000000000"
        }
        "#;

        let start: Message = serde_json::from_str(json_str).unwrap();

        if let Message::Select(Select::Specific(SelectSpecific { participant })) = start {
            assert_eq!(participant, ParticipantId::nil());
        } else {
            panic!()
        }
    }

    #[test]
    fn yield_message() {
        let json_str = r#"
        {
            "action": "yield",
            "next": "00000000-0000-0000-0000-000000000000"
        }
        "#;

        let start: Message = serde_json::from_str(json_str).unwrap();

        if let Message::Yield(Yield { next }) = start {
            assert_eq!(next, Some(ParticipantId::nil()));
        } else {
            panic!()
        }
    }
}
