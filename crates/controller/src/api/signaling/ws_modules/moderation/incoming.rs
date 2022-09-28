use controller_shared::ParticipantId;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Message {
    Kick(Target),
    Ban(Target),

    EnableWaitingRoom,
    DisableWaitingRoom,

    EnableRaiseHands,
    DisableRaiseHands,

    Accept(Target),

    ResetRaisedHands,
}

#[derive(Debug, Deserialize)]
pub struct Target {
    /// The participant to ban/kick from the room
    pub target: ParticipantId,
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn kick() {
        let json = r#"
        {
            "action": "kick",
            "target": "00000000-0000-0000-0000-000000000000"
        }
        "#;

        let msg: Message = serde_json::from_str(json).unwrap();

        if let Message::Kick(Target { target }) = msg {
            assert_eq!(target, ParticipantId::nil());
        } else {
            panic!()
        }
    }

    #[test]
    fn ban() {
        let json = r#"
        {
            "action": "ban",
            "target": "00000000-0000-0000-0000-000000000000"
        }
        "#;

        let msg: Message = serde_json::from_str(json).unwrap();

        if let Message::Ban(Target { target }) = msg {
            assert_eq!(target, ParticipantId::nil());
        } else {
            panic!()
        }
    }

    #[test]
    fn accept() {
        let json = r#"
        {
            "action": "accept",
            "target": "00000000-0000-0000-0000-000000000000"
        }
        "#;

        let msg: Message = serde_json::from_str(json).unwrap();

        if let Message::Accept(Target { target }) = msg {
            assert_eq!(target, ParticipantId::nil());
        } else {
            panic!()
        }
    }
}
