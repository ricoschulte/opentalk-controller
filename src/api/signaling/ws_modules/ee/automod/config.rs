use crate::api::signaling::ParticipantId;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum SelectionStrategy {
    /// No selection strategy, a moderator will assign privileges
    None,

    /// The next participant is the one next in the list
    Playlist,

    /// The next participant is randomly chosen
    Random,

    /// The current participant will nominate the next one
    Nomination,
}

/// Used to communicate to the frontend
#[derive(Debug, Deserialize, Serialize)]
pub struct FrontendConfig {
    #[serde(flatten)]
    pub parameter: Parameter,

    /// See documentation of [`super::outgoing::SpeakerUpdate`]
    pub history: Vec<ParticipantId>,

    /// See documentation of [`super::outgoing::SpeakerUpdate`]
    pub remaining: Vec<ParticipantId>,
}

impl FrontendConfig {
    /// Converts the config into a public config, which is modified to not show the list of
    /// available participants if configured.
    pub fn into_public(mut self) -> PublicConfig {
        if !self.parameter.show_list {
            self.remaining.clear();
        }

        PublicConfig(self)
    }
}

/// Typed version of the frontend-config that will be sent to the frontend, may only be created
/// using [`FrontendConfig::into_public`]
#[derive(Debug, Deserialize, Serialize)]
pub struct PublicConfig(FrontendConfig);

#[derive(Debug, Deserialize, Serialize)]
pub struct Parameter {
    /// The strategy used to determine the next speaker
    pub selection_strategy: SelectionStrategy,

    /// Is `list` visible to the frontend
    pub show_list: bool,

    /// If a raised hand should add a participant into `list`
    pub consider_hand_raise: bool,

    /// Time limit each speaker has before its speaking status get revoked
    #[serde(with = "duration_millis")]
    #[serde(default)]
    pub time_limit: Option<Duration>,

    /// Time in between selections to leave room for talk or animations
    #[serde(with = "duration_millis")]
    #[serde(default)]
    pub pause_time: Option<Duration>,

    /// Depending on the `selection_strategy` this will prevent participants to become
    /// speaker twice in a single automod session
    pub allow_double_selection: bool,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct StorageConfig {
    pub started: DateTime<Utc>,
    pub parameter: Parameter,
}

impl StorageConfig {
    pub fn new(parameter: Parameter) -> Self {
        Self {
            started: Utc::now(),
            parameter,
        }
    }
}

mod duration_millis {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<Duration>, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(Option::<u64>::deserialize(deserializer)?.map(Duration::from_millis))
    }

    pub fn serialize<S>(duration: &Option<Duration>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if let Some(duration) = duration {
            serializer.serialize_u128(duration.as_millis())
        } else {
            serializer.serialize_none()
        }
    }
}
