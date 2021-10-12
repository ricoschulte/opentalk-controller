use anyhow::Result;
use serde::Deserialize;

mod room;
mod user;

pub use room::Room;
pub use user::User;

#[derive(Deserialize, PartialEq, Debug)]
pub struct Provision {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rooms: Vec<room::Room>,
    #[serde(default)]
    pub users: Vec<user::User>,
}

impl Provision {
    pub fn from_yaml(yaml: &str) -> Result<Self> {
        Ok(serde_yaml::from_str(yaml)?)
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::Provision;

    #[test]
    fn test_parse_yaml_minimal_rooms() -> anyhow::Result<()> {
        let yaml = "\
        ---
        rooms: []
        ";

        let expected_provision = Provision {
            rooms: vec![],
            users: vec![],
        };

        assert_eq!(Provision::from_yaml(yaml)?, expected_provision);
        Ok(())
    }

    #[test]
    fn test_parse_yaml_minimal_users() -> anyhow::Result<()> {
        let yaml = "\
        ---
        users: []
        ";

        let expected_provision = Provision {
            rooms: vec![],
            users: vec![],
        };

        assert_eq!(Provision::from_yaml(yaml)?, expected_provision);
        Ok(())
    }

    use crate::provision::room::Room;

    #[test]
    fn test_parse_yaml_with_only_rooms() -> anyhow::Result<()> {
        let yaml = "\
        ---
        rooms:
        - owner: b463fdb5-b4d6-4708-828e-b0f8403db12b
          password: \"pW!12#@\"
          wait_for_moderator: false
          listen_only: true
        - uuid: 6246cff9-6615-4018-a00c-d2031c9ec262
          owner: ea059b9e-fef6-4d02-b5d2-ad5da295f1f4
          password: \"pW!12#@\"
          wait_for_moderator: true
          listen_only: false
        ";

        let rooms = vec![
            Room {
                uuid: None,
                owner: uuid::Uuid::from_str("b463fdb5-b4d6-4708-828e-b0f8403db12b")?,
                password: "pW!12#@".into(),
                wait_for_moderator: false,
                listen_only: true,
            },
            Room {
                uuid: Some(uuid::Uuid::from_str(
                    "6246cff9-6615-4018-a00c-d2031c9ec262",
                )?),
                owner: uuid::Uuid::from_str("ea059b9e-fef6-4d02-b5d2-ad5da295f1f4")?,
                password: "pW!12#@".into(),
                wait_for_moderator: true,
                listen_only: false,
            },
        ];

        let expected_provision = Provision {
            rooms,
            users: vec![],
        };

        assert_eq!(Provision::from_yaml(yaml)?, expected_provision);
        Ok(())
    }

    use crate::provision::user::User;

    #[test]
    fn test_parse_yaml_with_only_users() -> anyhow::Result<()> {
        let yaml = "\
        ---
        users:
        - email: alice@example.com
          first-name: Alice
          id: b463fdb5-b4d6-4708-828e-b0f8403db12b
          last-name: Doe
        - email: bob@example.com
          first-name: Bob
          id: ea059b9e-fef6-4d02-b5d2-ad5da295f1f4
          last-name: Doe
        ";

        let users = vec![
            User {
                id: uuid::Uuid::from_str("b463fdb5-b4d6-4708-828e-b0f8403db12b")?,
                first_name: "Alice".into(),
                last_name: "Doe".into(),
                email: "alice@example.com".into(),
            },
            User {
                id: uuid::Uuid::from_str("ea059b9e-fef6-4d02-b5d2-ad5da295f1f4")?,
                first_name: "Bob".into(),
                last_name: "Doe".into(),
                email: "bob@example.com".into(),
            },
        ];

        let expected_provision = Provision {
            users,
            rooms: vec![],
        };

        assert_eq!(Provision::from_yaml(yaml)?, expected_provision);
        Ok(())
    }

    #[test]
    fn test_parse_yaml_with_users_and_rooms() -> anyhow::Result<()> {
        let yaml = "\
        ---
        rooms:
        - owner: b463fdb5-b4d6-4708-828e-b0f8403db12b
          password: \"pW!12#@\"
          wait_for_moderator: false
          listen_only: true
        - uuid: 6246cff9-6615-4018-a00c-d2031c9ec262
          owner: ea059b9e-fef6-4d02-b5d2-ad5da295f1f4
          password: \"pW!12#@\"
          wait_for_moderator: true
          listen_only: false
        users:
        - email: alice@example.com
          first-name: Alice
          id: b463fdb5-b4d6-4708-828e-b0f8403db12b
          last-name: Doe
        - email: bob@example.com
          first-name: Bob
          id: ea059b9e-fef6-4d02-b5d2-ad5da295f1f4
          last-name: Doe
        ";

        let rooms = vec![
            Room {
                uuid: None,
                owner: uuid::Uuid::from_str("b463fdb5-b4d6-4708-828e-b0f8403db12b")?,
                password: "pW!12#@".into(),
                wait_for_moderator: false,
                listen_only: true,
            },
            Room {
                uuid: Some(uuid::Uuid::from_str(
                    "6246cff9-6615-4018-a00c-d2031c9ec262",
                )?),
                owner: uuid::Uuid::from_str("ea059b9e-fef6-4d02-b5d2-ad5da295f1f4")?,
                password: "pW!12#@".into(),
                wait_for_moderator: true,
                listen_only: false,
            },
        ];

        let users = vec![
            User {
                id: uuid::Uuid::from_str("b463fdb5-b4d6-4708-828e-b0f8403db12b")?,
                first_name: "Alice".into(),
                last_name: "Doe".into(),
                email: "alice@example.com".into(),
            },
            User {
                id: uuid::Uuid::from_str("ea059b9e-fef6-4d02-b5d2-ad5da295f1f4")?,
                first_name: "Bob".into(),
                last_name: "Doe".into(),
                email: "bob@example.com".into(),
            },
        ];

        let expected_provision = Provision { users, rooms };

        assert_eq!(Provision::from_yaml(yaml)?, expected_provision);
        Ok(())
    }
}
