use chrono::Utc;
use serde::Deserialize;
use serde::Serialize;
use uuid::Uuid;

mod registered_invite;
mod unregistered_invite;

pub use registered_invite::RegisteredEventInvite;
pub use unregistered_invite::UnregisteredEventInvite;

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct User {
    pub email: String,
    pub title: String,
    pub first_name: String,
    pub last_name: String,
    pub language: String,
}

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct Time {
    pub time: chrono::DateTime<Utc>,
    pub timezone: String,
}

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct Event {
    pub id: Uuid,
    pub name: String,
    pub start_time: Option<Time>,
    pub end_time: Option<Time>,
    pub rrule: Option<String>,
    pub description: String,
    pub room: Room,
    pub call_in: CallIn,
}

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct Room {
    pub id: Uuid,
    pub password: String,
}

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct CallIn {
    pub sip_tel: Option<String>,
    pub sip_id: Option<String>,
    pub sip_password: Option<String>,
}

/// The different kinds of MailTasks that are currently supported
#[derive(Deserialize, PartialEq, Debug)]
#[cfg_attr(any(test, feature = "client"), derive(Serialize))]
#[serde(tag = "message", rename_all = "snake_case")]
pub enum Message {
    /// A mail sent to registered users on invite
    RegisteredEventInvite(RegisteredEventInvite),
    /// A mail sent to unregistered users on invite
    UnregisteredEventInvite(UnregisteredEventInvite),
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::*;
    use chrono::FixedOffset;
    use pretty_assertions::assert_eq;

    #[test]
    fn test_basic_format() {
        let basic_invite = MailTask::V1(Message::RegisteredEventInvite(RegisteredEventInvite {
            inviter: User {
                email: "bob@example.org".into(),
                title: "Prof. Dr.".into(),
                first_name: "Bob".into(),
                last_name: "Inviter".into(),
                language: "de".into(),
            },
            event: Event {
                id: Uuid::from_u128(1),
                name: "Guten Morgen Meeting".into(),
                description: "".into(),
                start_time: Some(Time {
                    time: chrono::DateTime::<FixedOffset>::parse_from_rfc3339(
                        "2021-12-29T15:00:00+02:00",
                    )
                    .unwrap()
                    .into(),
                    timezone: "Europe/Berlin".into(),
                }),
                end_time: Some(Time {
                    time: chrono::DateTime::<FixedOffset>::parse_from_rfc3339(
                        "2021-12-29T15:30:00+02:00",
                    )
                    .unwrap()
                    .into(),
                    timezone: "Europe/Berlin".into(),
                }),
                rrule: None,
                room: Room {
                    id: Uuid::from_u128(0),
                    password: "".into(),
                },
                call_in: CallIn {
                    sip_tel: Some("+497652917".into()),
                    sip_id: Some("2".into()),
                    sip_password: Some("987".into()),
                },
            },
            invitee: User {
                email: "lastname@example.org".into(),
                title: "Prof. Dr.".into(),
                first_name: "FirstName".into(),
                last_name: "LastName".into(),
                language: "de".into(),
            },
        }));

        assert_eq!(
            basic_invite,
            serde_json::from_value(serde_json::json!({
                "version": "1",
                "message": "registered_event_invite",
                "event": {
                    "id": Uuid::from_u128(1),
                    "name": "Guten Morgen Meeting",
                    "description": "",
                    "start_time": {"time":"2021-12-29T15:00:00+02:00", "timezone": "Europe/Berlin"},
                    "end_time": {"time": "2021-12-29T15:30:00+02:00", "timezone": "Europe/Berlin"},
                    "room": {
                        "id": Uuid::from_u128(0),
                        "password": ""
                    },
                    "call_in": {
                        "sip_tel": "+497652917",
                        "sip_id": "2",
                        "sip_password": "987"
                    }
                },
                "invitee": {
                    "email": "lastname@example.org",
                    "title": "Prof. Dr.",
                    "first_name": "FirstName",
                    "last_name": "LastName",
                    "language": "de"
                },
                "inviter": {
                    "email": "bob@example.org",
                    "title": "Prof. Dr.",
                    "first_name": "Bob",
                    "last_name": "Inviter",
                    "language": "de"
                }
            }))
            .unwrap()
        );
    }

    #[test]
    fn test_no_time() {
        let basic_invite = MailTask::V1(Message::RegisteredEventInvite(RegisteredEventInvite {
            inviter: User {
                email: "bob@example.org".into(),
                title: "Prof. Dr.".into(),
                first_name: "Bob".into(),
                last_name: "Inviter".into(),
                language: "de".into(),
            },
            event: Event {
                id: Uuid::from_u128(1),
                name: "Guten Morgen Meeting".into(),
                description: "".into(),
                start_time: None,
                end_time: None,
                rrule: None,
                room: Room {
                    id: Uuid::from_u128(0),
                    password: "".into(),
                },
                call_in: CallIn {
                    sip_tel: Some("+497652917".into()),
                    sip_id: Some("2".into()),
                    sip_password: Some("987".into()),
                },
            },
            invitee: User {
                email: "lastname@example.org".into(),
                title: "Prof. Dr.".into(),
                first_name: "FirstName".into(),
                last_name: "LastName".into(),
                language: "de".into(),
            },
        }));

        assert_eq!(
            basic_invite,
            serde_json::from_value(serde_json::json!({
                "version": "1",
                "message": "registered_event_invite",
                "event": {
                    "id": Uuid::from_u128(1),
                    "name": "Guten Morgen Meeting",
                    "description": "",
                    "room": {
                        "id": Uuid::from_u128(0),
                        "password": ""
                    },
                    "call_in": {
                        "sip_tel": "+497652917",
                        "sip_id": "2",
                        "sip_password": "987"
                    }
                },
                "invitee": {
                    "email": "lastname@example.org",
                    "title": "Prof. Dr.",
                    "first_name": "FirstName",
                    "last_name": "LastName",
                    "language": "de"
                },
                "inviter": {
                    "email": "bob@example.org",
                    "title": "Prof. Dr.",
                    "first_name": "Bob",
                    "last_name": "Inviter",
                    "language": "de"
                }
            }))
            .unwrap()
        );
    }
}
