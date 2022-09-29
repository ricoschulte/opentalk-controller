use super::{Email, Event, User};
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, PartialEq, Eq, Debug)]
pub struct RegisteredEventInvite {
    pub invitee: User,
    pub event: Event,
    pub inviter: User,
}

#[derive(Deserialize, Serialize, PartialEq, Eq, Debug)]
pub struct UnregisteredEventInvite {
    pub invitee: Email,
    pub event: Event,
    pub inviter: User,
}

#[derive(Deserialize, Serialize, PartialEq, Eq, Debug)]
pub struct ExternalEventInvite {
    pub invitee: Email,
    pub event: Event,
    pub inviter: User,
    pub invite_code: String,
}
