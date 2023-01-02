use db_storage::assets::AssetId;
use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "message")]
pub enum Message {
    /// An access url containing a write session
    WriteUrl(AccessUrl),
    /// An access url containing a readonly session
    ReadUrl(AccessUrl),
    PdfAsset(PdfAsset),
    Error(Error),
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct AccessUrl {
    pub url: String,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PdfAsset {
    pub filename: String,
    pub asset_id: AssetId,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "error")]
pub enum Error {
    /// The requesting user has insufficient permissions for the operation
    InsufficientPermissions,
    /// The request contains invalid participant ids
    InvalidParticipantSelection,
    /// Is send when another instance just started initializing and etherpad is not available yet
    CurrentlyInitializing,
    /// The etherpad initialization failed
    FailedInitialization,
    /// The etherpad is not yet initailized
    NotInitialized,
}

#[cfg(test)]
mod test {
    use super::*;
    use controller::prelude::serde_json;
    use serde_json::json;

    #[test]
    fn write_url() {
        let expected = json!({
            "message": "write_url",
            "url": "http://localhost/auth_session?sessionID=s.session&padName=protocol&groupID=g.group",
        });

        let message = Message::WriteUrl(AccessUrl {
            url:
                "http://localhost/auth_session?sessionID=s.session&padName=protocol&groupID=g.group"
                    .into(),
        });

        let actual = serde_json::to_value(&message).unwrap();

        assert_eq!(expected, actual);
    }

    #[test]
    fn read_url() {
        let expected = json!({
            "message": "read_url",
            "url": "http://localhost:9001/auth_session?sessionID=s.session_id&padName=r.readonly_id",
        });

        let message = Message::ReadUrl(AccessUrl {
            url: "http://localhost:9001/auth_session?sessionID=s.session_id&padName=r.readonly_id"
                .into(),
        });

        let actual = serde_json::to_value(&message).unwrap();

        assert_eq!(expected, actual);
    }

    #[test]
    fn insufficient_permissions() {
        let expected = json!({"message": "error", "error": "insufficient_permissions"});

        let message = Message::Error(Error::InsufficientPermissions);

        let actual = serde_json::to_value(&message).unwrap();

        assert_eq!(expected, actual);
    }

    #[test]
    fn currently_initialization() {
        let expected = json!({"message": "error", "error": "failed_initialization"});

        let message = Message::Error(Error::FailedInitialization);

        let actual = serde_json::to_value(&message).unwrap();

        assert_eq!(expected, actual);
    }

    #[test]
    fn failed_initializing() {
        let expected = json!({"message": "error", "error": "currently_initializing"});

        let message = Message::Error(Error::CurrentlyInitializing);

        let actual = serde_json::to_value(&message).unwrap();

        assert_eq!(expected, actual);
    }

    #[test]
    fn invalid_participant_selection() {
        let expected = json!({"message": "error", "error": "invalid_participant_selection"});

        let message = Message::Error(Error::InvalidParticipantSelection);

        let actual = serde_json::to_value(&message).unwrap();

        assert_eq!(expected, actual);
    }
}
