// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use serde::Deserialize;
#[cfg(any(test, feature = "client"))]
use serde::Serialize;
pub mod v1;

/// Versioned Mail Task Protocol
#[derive(Deserialize, PartialEq, Eq, Debug)]
#[cfg_attr(any(test, feature = "client"), derive(Serialize))]
#[serde(tag = "version")]
pub enum MailTask {
    #[serde(rename = "1")]
    V1(v1::Message),
}

#[cfg(feature = "client")]
impl MailTask {
    /// Creates an invite MailTask for a registered invitee
    pub fn registered_event_invite<E, I, U>(inviter: I, event: E, invitee: U) -> MailTask
    where
        I: Into<v1::RegisteredUser>,
        E: Into<v1::Event>,
        U: Into<v1::RegisteredUser>,
    {
        Self::V1(v1::Message::RegisteredEventInvite(
            v1::RegisteredEventInvite {
                invitee: invitee.into(),
                event: event.into(),
                inviter: inviter.into(),
            },
        ))
    }

    /// Creates an invite MailTask for an unregistered invitee
    pub fn unregistered_event_invite<E, I, U>(inviter: I, event: E, invitee: U) -> MailTask
    where
        I: Into<v1::RegisteredUser>,
        E: Into<v1::Event>,
        U: Into<v1::UnregisteredUser>,
    {
        Self::V1(v1::Message::UnregisteredEventInvite(
            v1::UnregisteredEventInvite {
                invitee: invitee.into(),
                event: event.into(),
                inviter: inviter.into(),
            },
        ))
    }

    /// Creates an invite MailTask for an external invitee
    pub fn external_event_invite<E, I, U>(
        inviter: I,
        event: E,
        invitee: U,
        invite_code: String,
    ) -> MailTask
    where
        I: Into<v1::RegisteredUser>,
        E: Into<v1::Event>,
        U: Into<v1::ExternalUser>,
    {
        Self::V1(v1::Message::ExternalEventInvite(v1::ExternalEventInvite {
            invitee: invitee.into(),
            event: event.into(),
            inviter: inviter.into(),
            invite_code,
        }))
    }

    /// Creates an event update MailTask for a registered invitee
    pub fn registered_event_update<E, I, U>(inviter: I, event: E, invitee: U) -> MailTask
    where
        I: Into<v1::RegisteredUser>,
        E: Into<v1::Event>,
        U: Into<v1::RegisteredUser>,
    {
        Self::V1(v1::Message::RegisteredEventUpdate(
            v1::RegisteredEventUpdate {
                invitee: invitee.into(),
                event: event.into(),
                inviter: inviter.into(),
            },
        ))
    }

    /// Creates an event update MailTask for an unregistered invitee
    pub fn unregistered_event_update<E, I, U>(inviter: I, event: E, invitee: U) -> MailTask
    where
        I: Into<v1::RegisteredUser>,
        E: Into<v1::Event>,
        U: Into<v1::UnregisteredUser>,
    {
        Self::V1(v1::Message::UnregisteredEventUpdate(
            v1::UnregisteredEventUpdate {
                invitee: invitee.into(),
                event: event.into(),
                inviter: inviter.into(),
            },
        ))
    }

    /// Creates an event update MailTask for an external invitee
    pub fn external_event_update<E, I, U>(
        inviter: I,
        event: E,
        invitee: U,
        invite_code: String,
    ) -> MailTask
    where
        I: Into<v1::RegisteredUser>,
        E: Into<v1::Event>,
        U: Into<v1::ExternalUser>,
    {
        Self::V1(v1::Message::ExternalEventUpdate(v1::ExternalEventUpdate {
            invitee: invitee.into(),
            event: event.into(),
            inviter: inviter.into(),
            invite_code,
        }))
    }

    /// Creates an event cancellation MailTask for a registered invitee
    pub fn registered_event_cancellation<E, I, U>(inviter: I, event: E, invitee: U) -> MailTask
    where
        I: Into<v1::RegisteredUser>,
        E: Into<v1::Event>,
        U: Into<v1::RegisteredUser>,
    {
        Self::V1(v1::Message::RegisteredEventCancellation(
            v1::RegisteredEventCancellation {
                invitee: invitee.into(),
                event: event.into(),
                inviter: inviter.into(),
            },
        ))
    }

    /// Creates an event cancellation MailTask for an unregistered invitee
    pub fn unregistered_event_cancellation<E, I, U>(inviter: I, event: E, invitee: U) -> MailTask
    where
        I: Into<v1::RegisteredUser>,
        E: Into<v1::Event>,
        U: Into<v1::UnregisteredUser>,
    {
        Self::V1(v1::Message::UnregisteredEventCancellation(
            v1::UnregisteredEventCancellation {
                invitee: invitee.into(),
                event: event.into(),
                inviter: inviter.into(),
            },
        ))
    }

    /// Creates an event cancellation MailTask for an external invitee
    pub fn external_event_cancellation<E, I, U>(inviter: I, event: E, invitee: U) -> MailTask
    where
        I: Into<v1::RegisteredUser>,
        E: Into<v1::Event>,
        U: Into<v1::ExternalUser>,
    {
        Self::V1(v1::Message::ExternalEventCancellation(
            v1::ExternalEventCancellation {
                invitee: invitee.into(),
                event: event.into(),
                inviter: inviter.into(),
            },
        ))
    }

    pub fn as_kind_str(&self) -> &'static str {
        match self {
            MailTask::V1(message) => match message {
                // Invites
                v1::Message::RegisteredEventInvite(_) => "registered_invite",
                v1::Message::UnregisteredEventInvite(_) => "unregistered_invite",
                v1::Message::ExternalEventInvite(_) => "external_invite",
                // Updates
                v1::Message::RegisteredEventUpdate(_) => "registered_update",
                v1::Message::UnregisteredEventUpdate(_) => "unregistered_update",
                v1::Message::ExternalEventUpdate(_) => "external_update",
                // Cancellations
                v1::Message::RegisteredEventCancellation(_) => "registered_cancellation",
                v1::Message::UnregisteredEventCancellation(_) => "unregistered_cancellation",
                v1::Message::ExternalEventCancellation(_) => "external_cancellation",
            },
        }
    }
}

#[cfg(feature = "client")]
impl From<db_storage::users::User> for v1::RegisteredUser {
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
impl From<keycloak_admin::users::User> for v1::UnregisteredUser {
    fn from(val: keycloak_admin::users::User) -> Self {
        Self {
            email: val.email.into(),
            first_name: val.first_name,
            last_name: val.last_name,
        }
    }
}

#[cfg(feature = "client")]
impl From<String> for v1::ExternalUser {
    fn from(email: String) -> Self {
        Self {
            email: email.into(),
        }
    }
}

#[cfg(feature = "client")]
impl From<(chrono::DateTime<chrono::Utc>, types::core::TimeZone)> for v1::Time {
    fn from((time, timezone): (chrono::DateTime<chrono::Utc>, types::core::TimeZone)) -> Self {
        v1::Time {
            time,
            timezone: timezone.to_string(),
        }
    }
}
