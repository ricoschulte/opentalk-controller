use super::users::PublicUserProfile;
use controller_shared::settings::Settings;
use database::DbConnection;
use database::Result;
use db_storage::users::{User, UserId};
use db_storage::utils::HasUsers;
use std::collections::HashMap;

/// Utility to fetch user profiles batched
///
/// See [`db_storage::utils::HasUsers`]
#[derive(Default)]
pub struct GetUserProfilesBatched {
    users: Vec<UserId>,
}

impl GetUserProfilesBatched {
    pub fn new() -> Self {
        Self { users: vec![] }
    }

    pub fn add(&mut self, has_users: impl HasUsers) -> &mut Self {
        has_users.populate(&mut self.users);
        self
    }

    pub fn fetch(&mut self, settings: &Settings, conn: &DbConnection) -> Result<UserProfilesBatch> {
        if self.users.is_empty() {
            return Ok(UserProfilesBatch {
                users: HashMap::new(),
            });
        }

        self.users.sort_unstable();
        self.users.dedup();

        User::get_all_by_ids(conn, &self.users)
            .map(|users| {
                users
                    .into_iter()
                    .map(|user| (user.id, PublicUserProfile::from_db(settings, user)))
                    .collect()
            })
            .map(|users| UserProfilesBatch { users })
    }
}

pub struct UserProfilesBatch {
    users: HashMap<UserId, PublicUserProfile>,
}

impl UserProfilesBatch {
    pub fn get(&self, id: UserId) -> PublicUserProfile {
        self.users
            .get(&id)
            .expect("tried to get user-profile without fetching it first")
            .clone()
    }
}
