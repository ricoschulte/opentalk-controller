use super::Result;
use crate::KeycloakClient;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct User {
    pub id: String,
    pub username: String,
    pub enabled: bool,
    pub email_verified: bool,
    pub first_name: String,
    pub last_name: String,
    pub email: String,
}

#[derive(Serialize)]
struct SearchQuery<'s> {
    search: &'s str,
}

impl KeycloakClient {
    /// Query keycloak for users using the given search string
    pub async fn search_user(&self, search_str: &str) -> Result<Vec<User>> {
        let url = self.url(["admin", "realms", &self.realm, "users"])?;

        let query = SearchQuery { search: search_str };

        let response = self
            .send_authorized(move |c| c.get(url.clone()).query(&query))
            .await?;

        let found_users = response.json().await?;

        Ok(found_users)
    }
}
