use db_storage::users::{NewUser, NewUserWithGroups};
use serde::Deserialize;

#[derive(Deserialize, PartialEq, Debug)]
pub struct User {
    pub id: uuid::Uuid,
    #[serde(rename = "first-name")]
    pub first_name: String,
    #[serde(rename = "last-name")]
    pub last_name: String,
    pub email: String,
}

impl From<User> for NewUserWithGroups {
    fn from(user: User) -> Self {
        NewUserWithGroups {
            new_user: NewUser {
                oidc_uuid: user.id,
                email: user.email,
                title: "".into(),
                firstname: user.first_name,
                lastname: user.last_name,
                id_token_exp: 0,
                theme: "".into(),
                language: "en".into(),
            },
            groups: vec![],
        }
    }
}
