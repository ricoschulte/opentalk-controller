//! Modules for external HTTP APIs
//!
//! Versions REST APIs are in v{VERSION}
//! APIs for use with our own frontend lie in internal
//! These directory map to the path prefix `/internal` or `/v1`

use db_storage::users::{User, UserId};
use serde::{Deserialize, Serialize};

pub mod internal;
#[macro_use]
pub mod signaling;
pub mod v1;

#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq)]
pub enum Participant<U> {
    User(U),
    Guest,
    Sip,
}

impl From<UserId> for Participant<UserId> {
    fn from(id: UserId) -> Self {
        Participant::User(id)
    }
}

impl From<User> for Participant<User> {
    fn from(user: User) -> Self {
        Participant::User(user)
    }
}

impl Participant<User> {
    /// Returns the UserId when the participant
    pub fn user_id(&self) -> Option<UserId> {
        match self {
            Participant::User(user) => Some(user.id),
            Participant::Guest => None,
            Participant::Sip => None,
        }
    }
}
