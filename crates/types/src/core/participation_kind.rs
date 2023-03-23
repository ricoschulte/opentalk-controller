// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use strum::{AsRefStr, Display, EnumCount, EnumIter, EnumString, EnumVariantNames, IntoStaticStr};

use crate::imports::*;

/// The kinds of participants in a meeting room.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    AsRefStr,
    Display,
    EnumCount,
    EnumIter,
    EnumString,
    EnumVariantNames,
    IntoStaticStr,
)]
#[cfg_attr(
    feature = "serde",
    derive(Deserialize, Serialize),
    serde(rename_all = "snake_case")
)]
#[cfg_attr(
    feature = "redis",
    derive(ToRedisArgs, FromRedisValue),
    to_redis_args(Display),
    from_redis_value(FromStr)
)]
#[strum(serialize_all = "snake_case")]
pub enum ParticipationKind {
    /// User participation kind is used for regular participants who have an account
    /// in the authentication service.
    User,

    /// Guest participation kind is used for regular participants who do not have
    /// an account in the authentication service.
    Guest,

    /// Sip participation kind is used for participants joining on behalf
    /// of a dial-in user.
    Sip,

    /// Recorder participation kind is used for a participant joining as a
    /// recording service.
    Recorder,
}

impl ParticipationKind {
    /// Returns `true` if the participant kind is visible to other participants
    /// in the room.
    pub fn is_visible(&self) -> bool {
        !matches!(self, Self::Recorder)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::str::FromStr;

    #[test]
    fn to_string() {
        assert_eq!(ParticipationKind::Guest.as_ref(), "guest");
        assert_eq!(ParticipationKind::User.as_ref(), "user");
        assert_eq!(ParticipationKind::Sip.as_ref(), "sip");
        assert_eq!(ParticipationKind::Recorder.as_ref(), "recorder");
    }

    #[test]
    fn from_str() {
        assert_eq!(
            ParticipationKind::from_str("guest"),
            Ok(ParticipationKind::Guest)
        );
        assert_eq!(
            ParticipationKind::from_str("user"),
            Ok(ParticipationKind::User)
        );
        assert_eq!(
            ParticipationKind::from_str("sip"),
            Ok(ParticipationKind::Sip)
        );
        assert_eq!(
            ParticipationKind::from_str("recorder"),
            Ok(ParticipationKind::Recorder)
        );
    }

    #[test]
    fn is_visible() {
        assert!(ParticipationKind::Guest.is_visible());
        assert!(ParticipationKind::User.is_visible());
        assert!(ParticipationKind::Sip.is_visible());
        assert!(!ParticipationKind::Recorder.is_visible());
    }
}
