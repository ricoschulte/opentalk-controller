use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(tag = "action")]
pub enum Message {
    #[serde(rename = "join")]
    Join(Join),
}

#[derive(Debug, Deserialize)]
pub struct Join {
    /// The users display name
    pub display_name: String,
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn hello() {
        let json = r#"
        {
            "action": "join",
            "display_name": "Test!"
        }
        "#;

        let msg: Message = serde_json::from_str(json).unwrap();

        match msg {
            Message::Join(Join { display_name }) => {
                assert_eq!(display_name, "Test!");
            }
        }
    }
}
