use serde::Serialize;

#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "message")]
pub enum Message {
    WriteUrl(WriteUrl),
    ReadUrl(ReadUrl),
    Error(Error),
}

#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct WriteUrl {
    pub url: String,
}

#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ReadUrl {
    pub url: String,
}

#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "error")]
pub enum Error {
    /// The requesting user has insufficient permissions for the operation
    InsufficientPermissions,
    /// Is send when another instance just started initializing and etherpad is not available yet
    CurrentlyInitializing,
    /// The etherpad initialization failed
    FailedInitialization,
}

#[cfg(test)]
mod test {
    use super::*;
    use controller::prelude::serde_json;

    #[test]
    fn write_url() {
        let json_str = r#"{"message":"write_url","url":"http://localhost/auth_session?sessionID=s.session&padName=protocol&groupID=g.group"}"#;

        let message = Message::WriteUrl(WriteUrl {
            url:
                "http://localhost/auth_session?sessionID=s.session&padName=protocol&groupID=g.group"
                    .into(),
        });

        let string = serde_json::to_string(&message).unwrap();

        assert_eq!(string, json_str)
    }

    #[test]
    fn read_url() {
        let json_str = r#"{"message":"read_url","url":"http://localhost:9001/auth_session?sessionID=s.session_id&padName=r.readonly_id"}"#;

        let message = Message::ReadUrl(ReadUrl {
            url: "http://localhost:9001/auth_session?sessionID=s.session_id&padName=r.readonly_id"
                .into(),
        });

        let string = serde_json::to_string(&message).unwrap();

        assert_eq!(string, json_str)
    }

    #[test]
    fn insufficient_permissions() {
        let json_str = r#"{"message":"error","error":"insufficient_permissions"}"#;

        let message = Message::Error(Error::InsufficientPermissions);

        let string = serde_json::to_string(&message).unwrap();

        assert_eq!(string, json_str)
    }

    #[test]
    fn currently_initialization() {
        let json_str = r#"{"message":"error","error":"failed_initialization"}"#;

        let message = Message::Error(Error::FailedInitialization);

        let string = serde_json::to_string(&message).unwrap();

        assert_eq!(string, json_str)
    }

    #[test]
    fn failed_initializing() {
        let json_str = r#"{"message":"error","error":"currently_initializing"}"#;

        let message = Message::Error(Error::CurrentlyInitializing);

        let string = serde_json::to_string(&message).unwrap();

        assert_eq!(string, json_str)
    }
}