//! History of all speakers inside a room session. The history lives over multiple automod sessions
//! until the room is being torn down.
//!
//! Events are stored inside a redis sorted set.
//! The score of the events is milliseconds of the timestamp.
//!
//! An event is recorded everytime a participant gains and loses its speaker status.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use controller::prelude::*;
use displaydoc::Display;
use redis::aio::ConnectionManager;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Display)]
/// k3k-signaling:room={room}:automod:history
#[ignore_extra_doc_attributes]
/// Typed key to the automod history
struct RoomAutoModHistory {
    room: Uuid,
}

impl_to_redis_args!(RoomAutoModHistory);

#[derive(Debug, Serialize, Deserialize)]
/// Entry inside the automod history
pub struct Entry {
    pub timestamp: DateTime<Utc>,
    pub participant: ParticipantId,
    pub kind: EntryKind,
}

impl_to_redis_args_se!(&Entry);
impl_from_redis_value_de!(Entry);

/// The kind of history entry.  
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub enum EntryKind {
    /// Participant gained its speaker status.
    Start,

    /// Participant lost its speaker status.
    Stop,
}

impl Entry {
    /// Creates a new Start-Entry with the current timestamp and the given participant.
    pub fn start(participant: ParticipantId) -> Self {
        Self {
            timestamp: Utc::now(),
            participant,
            kind: EntryKind::Start,
        }
    }

    /// Creates a new Stop-Entry with the current timestamp and the given participant.
    pub fn stop(participant: ParticipantId) -> Self {
        Self {
            timestamp: Utc::now(),
            participant,
            kind: EntryKind::Stop,
        }
    }
}

/// Adds the given entry to the history
pub async fn add(redis_conn: &mut ConnectionManager, room: Uuid, entry: &Entry) -> Result<()> {
    redis_conn
        .zadd(
            RoomAutoModHistory { room },
            entry,
            entry.timestamp.timestamp_millis(),
        )
        .await
        .context("Failed to add history entry")
}

/// Get a ordered list of participants which appear in the history after the given `since` parameter
/// timestamp.
pub async fn get(
    redis_conn: &mut ConnectionManager,
    room: Uuid,
    since: DateTime<Utc>,
) -> Result<Vec<ParticipantId>> {
    let entries: Vec<Entry> = redis_conn
        .zrangebyscore(
            RoomAutoModHistory { room },
            since.timestamp_millis(),
            "+inf",
        )
        .await
        .context("Failed to get history entries")?;

    let participants = entries
        .into_iter()
        .filter(|entry| matches!(entry.kind, EntryKind::Start))
        .map(|entry| entry.participant)
        .collect();

    Ok(participants)
}

/// Delete the history.
pub async fn del(redis_conn: &mut ConnectionManager, room: Uuid) -> Result<()> {
    redis_conn
        .del(RoomAutoModHistory { room })
        .await
        .context("Failed to del history")
}

#[cfg(test)]
pub(crate) async fn get_entries(
    redis_conn: &mut ConnectionManager,
    room: Uuid,
    since: DateTime<Utc>,
) -> Result<Vec<Entry>> {
    redis_conn
        .zrangebyscore(
            RoomAutoModHistory { room },
            since.timestamp_millis(),
            "+inf",
        )
        .await
        .context("Failed to get history entries")
}
