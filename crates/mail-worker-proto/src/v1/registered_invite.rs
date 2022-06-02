use serde::Deserialize;
use serde::Serialize;

use super::{Event, User};

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct RegisteredEventInvite {
    pub invitee: User,
    pub event: Event,
    pub inviter: User,
}
