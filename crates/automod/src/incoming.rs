//! All websocket commands that can be issued by the frontend

use crate::config::Parameter;
use controller_shared::ParticipantId;
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

    /// Depending on the selection strategy, the list of Participant that can be chosen from.
    ///
    ///
    /// - Strategy = `none`, `random` or `nomination`: The allow_list acts as pool of participants which can
    ///   be selected (by nomination or randomly etc).
    ///
    /// - Strategy = `playlist` The allow_list does not get used by this strategy.
    #[serde(default)]
    pub allow_list: Vec<ParticipantId>,

    /// Ordered list of queued participants
    ///
    /// - Strategy = `none`, `random` or `nomination`: The playlist does not get used by these strategies.
    ///
    /// - Strategy = `playlist` The playlist is a ordered list of participants which will get used to select
    ///     the next participant when yielding. It is also used as a pool to select participants
    ///     randomly from (moderator command `Select`).
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

    /// Advance the moderation depending on the selection strategy.
    /// Can just unset the current speaker if selection strategy is nomination
    Next,

    /// Select a specific participant
    Specific(SelectSpecific),
}

/// Fields that are provided when issuing the [`Select::Specific`] command
#[derive(Debug, Deserialize)]
pub struct SelectSpecific {
    /// The participant to be selected
    pub participant: ParticipantId,

    /// If true the selected participant will not be removed from either the allow- or playlist
    pub keep_in_remaining: bool,
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
    use crate::config::SelectionStrategy;
    use controller::prelude::*;
    use pretty_assertions::assert_eq;
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
            "allow_double_selection": false,
            "animation_on_random": true,
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
                    allow_double_selection,
                    animation_on_random,
                },
            allow_list,
            playlist,
        }) = start
        {
            assert_eq!(selection_strategy, SelectionStrategy::None);
            assert!(show_list);
            assert!(!consider_hand_raise);
            assert_eq!(time_limit, Some(Duration::from_secs(10)));
            assert!(!allow_double_selection);
            assert!(animation_on_random);

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
            "participant": "00000000-0000-0000-0000-000000000000",
            "keep_in_remaining": true
        }
        "#;

        let start: Message = serde_json::from_str(json_str).unwrap();

        if let Message::Select(Select::Specific(SelectSpecific {
            participant,
            keep_in_remaining,
        })) = start
        {
            assert_eq!(participant, ParticipantId::nil());
            assert!(keep_in_remaining);
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
