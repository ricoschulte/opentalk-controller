//! Modules to HTTP APIs

use crate::db::users::{SerialUserId, User};
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

impl From<SerialUserId> for Participant<SerialUserId> {
    fn from(id: SerialUserId) -> Self {
        Participant::User(id)
    }
}

impl From<User> for Participant<User> {
    fn from(user: User) -> Self {
        Participant::User(user)
    }
}

impl Participant<User> {
    /// Returns the SerialUserId when the participant
    pub fn user_id(&self) -> Option<SerialUserId> {
        match self {
            Participant::User(user) => Some(user.id),
            Participant::Guest => None,
            Participant::Sip => None,
        }
    }
}
