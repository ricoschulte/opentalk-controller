use serde::Deserialize;
use serde::Serialize;

use super::{Email, Event, User};

#[derive(Deserialize, PartialEq, Debug)]
#[cfg_attr(any(test, feature = "client"), derive(Serialize))]
pub struct UnregisteredEventInvite {
    pub invitee: Email,
    pub event: Event,
    pub inviter: User,
}
