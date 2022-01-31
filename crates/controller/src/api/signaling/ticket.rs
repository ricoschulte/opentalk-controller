use super::resumption::ResumptionToken;
use crate::db::rooms::RoomId;
use crate::db::users::SerialUserId;
use crate::prelude::*;
use controller_shared::ParticipantId;
use displaydoc::Display;
use rand::Rng;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TicketToken(String);

impl TicketToken {
    pub fn generate() -> Self {
        let token = rand::thread_rng()
            .sample_iter(rand::distributions::Alphanumeric)
            .take(64)
            .map(char::from)
            .collect();

        Self(token)
    }

    pub fn redis_key(&self) -> TicketRedisKey<'_> {
        TicketRedisKey { ticket: &self.0 }
    }
}

#[derive(Display, Debug, Copy, Clone)]
/// k3k-signaling:ticket={ticket}
#[ignore_extra_doc_attributes]
/// Typed redis key for a signaling ticket containing [`TicketData`]
pub struct TicketRedisKey<'s> {
    pub ticket: &'s str,
}

impl_to_redis_args!(TicketRedisKey<'_>);

/// Data stored behind the [`Ticket`] key.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TicketData {
    pub participant_id: ParticipantId,
    pub participant: Participant<SerialUserId>,
    pub room: RoomId,
    pub breakout_room: Option<BreakoutRoomId>,
    pub resumption: ResumptionToken,
}

impl_from_redis_value_de!(TicketData);
impl_to_redis_args_se!(&TicketData);
