use serde::Serialize;

#[derive(Debug, Serialize, PartialEq)]
#[serde(tag = "message", rename_all = "snake_case")]
pub enum Message {
    Kicked,
    Banned,
    Error(Error),
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(tag = "error", rename_all = "snake_case")]
pub enum Error {
    CannotBanGuest,
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn kicked() {
        let expected = r#"{"message":"kicked"}"#;

        let produced = serde_json::to_string(&Message::Kicked).unwrap();

        assert_eq!(expected, produced);
    }

    #[test]
    fn banned() {
        let expected = r#"{"message":"banned"}"#;

        let produced = serde_json::to_string(&Message::Banned).unwrap();

        assert_eq!(expected, produced);
    }
}
