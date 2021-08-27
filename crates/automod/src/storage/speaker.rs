//! Single value entry which holds the participant id of the current speaker.
//!
//! If not set, then there is currently no active speaker.
use anyhow::{Context, Result};
use controller::db::rooms::RoomId;
use controller::prelude::*;
use displaydoc::Display;
use redis::aio::ConnectionManager;
use redis::AsyncCommands;

#[derive(Display)]
/// k3k-signaling:room={room}:automod:speaker
#[ignore_extra_doc_attributes]
/// Typed key to the automod's active speaker
pub struct RoomAutoModSpeaker {
    room: RoomId,
}

impl_to_redis_args!(RoomAutoModSpeaker);

/// Get the current speaker. Returns [`None`] if there is no active speaker.
pub async fn get(redis: &mut ConnectionManager, room: RoomId) -> Result<Option<ParticipantId>> {
    redis
        .get(RoomAutoModSpeaker { room })
        .await
        .context("Failed to set active speaker")
}

/// Sets the new current speaker and returns the old one if it was set
pub async fn set(
    redis: &mut ConnectionManager,
    room: RoomId,
    participant: ParticipantId,
) -> Result<Option<ParticipantId>> {
    redis::cmd("SET")
        .arg(RoomAutoModSpeaker { room })
        .arg(participant)
        .arg("GET")
        .query_async(redis)
        .await
        .context("Failed to set active speaker")
}

/// Delete the current speaker and return if there was any speaker.
pub async fn del(redis: &mut ConnectionManager, room: RoomId) -> Result<Option<ParticipantId>> {
    redis::cmd("GETDEL")
        .arg(RoomAutoModSpeaker { room })
        .query_async(redis)
        .await
        .context("Failed to del active speaker")
}
