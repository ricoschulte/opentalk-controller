// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use super::Result;
use crate::KeycloakAdminClient;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct User {
    pub id: String,
    pub username: String,
    pub enabled: bool,
    pub email_verified: bool,
    pub first_name: String,
    pub last_name: String,
    pub email: String,
    #[serde(default)]
    pub attributes: Attributes,
}

impl User {
    fn is_in_tenant(&self, tenant_id: &str) -> bool {
        self.attributes.tenant_id.iter().any(|t| t == tenant_id)
    }
}

#[derive(Default, Clone, Debug, Deserialize)]
pub struct Attributes {
    #[serde(default)]
    pub tenant_id: Vec<String>,
}

#[derive(Serialize)]
struct SearchQuery<'s> {
    search: &'s str,
    max: i32,
}

#[derive(Serialize)]
struct VerifyEmailQuery<'s> {
    email: &'s str,
    exact: bool,
}

impl KeycloakAdminClient {
    /// Query keycloak for users using the given search string
    pub async fn search_user(&self, tenant_id: &str, search_str: &str) -> Result<Vec<User>> {
        let url = self.url(["admin", "realms", &self.realm, "users"])?;

        // TODO: Fix this code once https://github.com/keycloak/keycloak/issues/16687 is resolved.
        // Currently we let keycloak give us 100 users matching the search_str and then filter by the tenant_id.
        // Ideally we want keycloak to filter these out for us. In a larger user base with more tenants this will
        // will be insufficient to provide a good auto-completion, since more than 100 users outside of the searched
        // tenant might match.
        let query = SearchQuery {
            search: search_str,
            max: 100,
        };

        let response = self
            .send_authorized(move |c| c.get(url.clone()).query(&query))
            .await?;

        let mut found_users: Vec<User> = response.json().await?;

        found_users.retain(|user| user.is_in_tenant(tenant_id));

        found_users.truncate(5);

        Ok(found_users)
    }

    /// Query keycloak to get the first user that matches the given email
    pub async fn get_user_for_email(&self, tenant_id: &str, email: &str) -> Result<Option<User>> {
        let url = self.url(["admin", "realms", &self.realm, "users"])?;

        let query = VerifyEmailQuery { email, exact: true };

        let response = self
            .send_authorized(move |c| c.get(url.clone()).query(&query))
            .await?;

        let found_users: Vec<User> = response.json().await?;
        let first_matching_user = found_users
            .iter()
            .find(|user| user.is_in_tenant(tenant_id) && user.email == email);

        if let Some(user) = first_matching_user {
            return Ok(Some((*user).clone()));
        }
        Ok(None)
    }
}
