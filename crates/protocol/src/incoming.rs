use controller_shared::ParticipantId;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case", tag = "action")]
pub enum Message {
    SelectWriter(SelectWriter),
}

/// Give a list of participants write access to the protocol
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SelectWriter {
    /// The targeted participants
    pub participant_ids: Vec<ParticipantId>,
}

#[cfg(test)]
mod test {
    use super::*;
    use controller::prelude::serde_json;

    #[test]
    fn select_writer() {
        let json_str = r#"
        {
            "action": "select_writer",
            "participant_ids": ["00000000-0000-0000-0000-000000000000", "00000000-0000-0000-0000-000000000001"]
        }
        "#;

        let select_writer: Message = serde_json::from_str(json_str).unwrap();

        let Message::SelectWriter(SelectWriter { participant_ids }) = select_writer;

        assert_eq!(
            vec![ParticipantId::new_test(0), ParticipantId::new_test(1)],
            participant_ids
        );
    }
}
