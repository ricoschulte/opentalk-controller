use super::{Email, Event, User};
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct UnregisteredEventInvite {
    pub invitee: Email,
    pub event: Event,
    pub inviter: User,
}
