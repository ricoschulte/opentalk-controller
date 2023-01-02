use controller_shared::ParticipantId;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case", tag = "action")]
pub enum Message {
    SelectWriter(ParticipantSelection),
    DeselectWriter(ParticipantSelection),
    /// Generates a pdf of the current protocol contents.
    GeneratePdf,
}

/// Give a list of participants write access to the protocol
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ParticipantSelection {
    /// The targeted participants
    pub participant_ids: Vec<ParticipantId>,
}

#[cfg(test)]
mod test {
    use super::*;
    use controller::prelude::serde_json;
    use pretty_assertions::assert_eq;

    #[test]
    fn select_writer() {
        let json_str = r#"
        {
            "action": "select_writer",
            "participant_ids": ["00000000-0000-0000-0000-000000000000", "00000000-0000-0000-0000-000000000001"]
        }
        "#;

        if let Message::SelectWriter(ParticipantSelection { participant_ids }) =
            serde_json::from_str(json_str).unwrap()
        {
            assert_eq!(participant_ids[0], ParticipantId::new_test(0));
            assert_eq!(participant_ids[1], ParticipantId::new_test(1));
        } else {
            panic!("expected SelectWriter variant");
        }
    }

    #[test]
    fn deselect_writer() {
        let json_str = r#"
        {
            "action": "deselect_writer",
            "participant_ids": ["00000000-0000-0000-0000-000000000000", "00000000-0000-0000-0000-000000000001"]
        }
        "#;

        if let Message::DeselectWriter(ParticipantSelection { participant_ids }) =
            serde_json::from_str(json_str).unwrap()
        {
            assert_eq!(participant_ids[0], ParticipantId::new_test(0));
            assert_eq!(participant_ids[1], ParticipantId::new_test(1));
        } else {
            panic!("expected SelectWriter variant");
        }
    }

    #[test]
    fn generate_pdf() {
        let json = serde_json::json!({
            "action": "generate_pdf"
        });

        if let Message::GeneratePdf = serde_json::from_value(json).unwrap() {
        } else {
            panic!("expected GeneratePdf variant");
        }
    }
}
