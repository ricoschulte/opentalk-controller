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
use controller_shared::ParticipantId;
use redis::AsyncCommands;
use redis_args::{FromRedisValue, ToRedisArgs};
use serde::{Deserialize, Serialize};

/// Typed key to the automod history
#[derive(ToRedisArgs)]
#[to_redis_args(fmt = "k3k-signaling:room={room}:automod:history")]
struct RoomAutoModHistory {
    room: SignalingRoomId,
}

/// Entry inside the automod history
#[derive(Debug, Serialize, Deserialize, ToRedisArgs, FromRedisValue)]
#[to_redis_args(serde)]
#[from_redis_value(serde)]
pub struct Entry {
    pub timestamp: DateTime<Utc>,
    pub participant: ParticipantId,
    pub kind: EntryKind,
}

/// The kind of history entry.  
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
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
#[tracing::instrument(name = "add_history", level = "debug", skip(redis_conn, entry))]
pub async fn add(
    redis_conn: &mut RedisConnection,
    room: SignalingRoomId,
    entry: &Entry,
) -> Result<()> {
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
#[tracing::instrument(name = "get_history", level = "debug", skip(redis_conn))]
pub async fn get(
    redis_conn: &mut RedisConnection,
    room: SignalingRoomId,
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
#[tracing::instrument(name = "del_history", level = "debug", skip(redis_conn))]
pub async fn del(redis_conn: &mut RedisConnection, room: SignalingRoomId) -> Result<()> {
    redis_conn
        .del(RoomAutoModHistory { room })
        .await
        .context("Failed to del history")
}

#[cfg(test)]
pub(crate) async fn get_entries(
    redis_conn: &mut RedisConnection,
    room: SignalingRoomId,
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
