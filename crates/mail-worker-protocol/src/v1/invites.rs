use super::{Event, ExternalUser, RegisteredUser, UnregisteredUser};
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, PartialEq, Eq, Debug)]
pub struct RegisteredEventInvite {
    pub invitee: RegisteredUser,
    pub event: Event,
    pub inviter: RegisteredUser,
}

#[derive(Deserialize, Serialize, PartialEq, Eq, Debug)]
pub struct UnregisteredEventInvite {
    pub invitee: UnregisteredUser,
    pub event: Event,
    pub inviter: RegisteredUser,
}

#[derive(Deserialize, Serialize, PartialEq, Eq, Debug)]
pub struct ExternalEventInvite {
    pub invitee: ExternalUser,
    pub event: Event,
    pub inviter: RegisteredUser,
    pub invite_code: String,
}

#[derive(Deserialize, Serialize, PartialEq, Eq, Debug)]
pub struct RegisteredEventUpdate {
    pub invitee: RegisteredUser,
    pub event: Event,
    pub inviter: RegisteredUser,
}

#[derive(Deserialize, Serialize, PartialEq, Eq, Debug)]
pub struct UnregisteredEventUpdate {
    pub invitee: UnregisteredUser,
    pub event: Event,
    pub inviter: RegisteredUser,
}

#[derive(Deserialize, Serialize, PartialEq, Eq, Debug)]
pub struct ExternalEventUpdate {
    pub invitee: ExternalUser,
    pub event: Event,
    pub inviter: RegisteredUser,
    pub invite_code: String,
}

#[derive(Deserialize, Serialize, PartialEq, Eq, Debug)]
pub struct RegisteredEventCancellation {
    pub invitee: RegisteredUser,
    pub event: Event,
    pub inviter: RegisteredUser,
}

#[derive(Deserialize, Serialize, PartialEq, Eq, Debug)]
pub struct UnregisteredEventCancellation {
    pub invitee: UnregisteredUser,
    pub event: Event,
    pub inviter: RegisteredUser,
}

#[derive(Deserialize, Serialize, PartialEq, Eq, Debug)]
pub struct ExternalEventCancellation {
    pub invitee: ExternalUser,
    pub event: Event,
    pub inviter: RegisteredUser,
}
