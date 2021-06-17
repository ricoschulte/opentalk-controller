use super::Result;
use crate::api::v1::parse_json_response;
use crate::K3KSession;
use serde::{Deserialize, Serialize};

/// Public user details.
///
/// Contains general "public" information about a user. Is accessible to all other users.
#[derive(Debug, Deserialize, Eq, PartialEq)]
pub struct UserDetails {
    pub id: i64,
    pub email: String,
    pub title: String,
    pub firstname: String,
    pub lastname: String,
}

/// Private user profile.
///
/// Similar to [`UserDetails`], but contains additional "private" information about a user.
/// Is only accessible to the user himself.
/// Is used on */users/me* endpoints.
#[derive(Debug, Deserialize)]
pub struct UserProfile {
    pub id: i64,
    pub email: String,
    pub title: String,
    pub firstname: String,
    pub lastname: String,
    pub theme: String,
    pub language: String,
}

/// Used to modify user settings
#[derive(Debug, Serialize)]
pub struct ModifyUser {
    pub title: Option<String>,
    pub theme: Option<String>,
    pub language: Option<String>,
}

impl K3KSession {
    /// Calls *GET '/v1/users*
    pub async fn all_users(&self) -> Result<Vec<UserDetails>> {
        let response = self.get_authenticated("/v1/users").await?;

        parse_json_response(response).await
    }

    /// Calls *GET '/v1/users/me*
    pub async fn get_current_user(&self) -> Result<UserProfile> {
        let response = self.get_authenticated("/v1/users/me").await?;

        parse_json_response(response).await
    }

    /// Calls *PUT '/v1/users/me/*
    pub async fn modify_current_user(&self, modify_user: &ModifyUser) -> Result<UserProfile> {
        let response = self
            .put_json_authenticated("/v1/users/me", &modify_user)
            .await?;

        parse_json_response(response).await
    }

    /// Calls *PUT '/v1/users/{id}*
    pub async fn get_user_details(&self, id: i64) -> Result<UserDetails> {
        let response = self.get_authenticated(&format!("/v1/users/{}", id)).await?;

        parse_json_response(response).await
    }
}
