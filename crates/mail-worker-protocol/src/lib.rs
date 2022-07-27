use serde::Deserialize;
#[cfg(any(test, feature = "client"))]
use serde::Serialize;
pub mod v1;

/// Versioned Mail Task Protocol
#[derive(Deserialize, PartialEq, Debug)]
#[cfg_attr(any(test, feature = "client"), derive(Serialize))]
#[serde(tag = "version")]
pub enum MailTask {
    #[serde(rename = "1")]
    V1(v1::Message),
}

#[cfg(feature = "client")]
impl MailTask {
    /// Creates a MailTask for an registered invite
    pub fn registered_invite<E, I, U>(inviter: I, event: E, invitee: U) -> MailTask
    where
        I: Into<v1::User>,
        E: Into<v1::Event>,
        U: Into<v1::User>,
    {
        Self::V1(v1::Message::RegisteredEventInvite(
            v1::RegisteredEventInvite {
                invitee: invitee.into(),
                event: event.into(),
                inviter: inviter.into(),
            },
        ))
    }

    /// Creates a MailTask for an unregistered invite
    pub fn unregistered_invite<E, I, U>(inviter: I, event: E, invitee: U) -> MailTask
    where
        I: Into<v1::User>,
        E: Into<v1::Event>,
        U: Into<v1::Email>,
    {
        Self::V1(v1::Message::UnregisteredEventInvite(
            v1::UnregisteredEventInvite {
                invitee: invitee.into(),
                event: event.into(),
                inviter: inviter.into(),
            },
        ))
    }

    /// Creates a MailTask for an unregistered invite
    pub fn external_invite<E, I, U>(
        inviter: I,
        event: E,
        invitee: U,
        invite_code: String,
    ) -> MailTask
    where
        I: Into<v1::User>,
        E: Into<v1::Event>,
        U: Into<v1::Email>,
    {
        Self::V1(v1::Message::ExternalEventInvite(v1::ExternalEventInvite {
            invitee: invitee.into(),
            event: event.into(),
            inviter: inviter.into(),
            invite_code,
        }))
    }

    pub fn as_kind_str(&self) -> &'static str {
        match self {
            MailTask::V1(message) => match message {
                v1::Message::RegisteredEventInvite(_) => "registered",
                v1::Message::UnregisteredEventInvite(_) => "unregistered",
                v1::Message::ExternalEventInvite(_) => "external",
            },
        }
    }
}

#[cfg(feature = "client")]
impl From<db_storage::users::User> for v1::User {
    fn from(val: db_storage::users::User) -> Self {
        Self {
            email: val.email.into(),
            title: val.title,
            first_name: val.firstname,
            last_name: val.lastname,
            language: val.language,
        }
    }
}

#[cfg(feature = "client")]
impl From<(chrono::DateTime<chrono::Utc>, db_storage::events::TimeZone)> for v1::Time {
    fn from(
        (time, timezone): (chrono::DateTime<chrono::Utc>, db_storage::events::TimeZone),
    ) -> Self {
        v1::Time {
            time,
            timezone: timezone.0.to_string(),
        }
    }
}
