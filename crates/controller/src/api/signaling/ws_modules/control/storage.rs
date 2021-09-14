use crate::api::signaling::ParticipantId;
use crate::db::rooms::RoomId;
use anyhow::{Context, Result};
use displaydoc::Display;
use r3dlock::{Mutex, MutexGuard};
use redis::aio::ConnectionManager;
use redis::{AsyncCommands, FromRedisValue, ToRedisArgs};
use std::collections::HashSet;
use std::fmt::Debug;
use std::time::Duration;

#[derive(Display)]
/// k3k-signaling:room={room}:participants
#[ignore_extra_doc_attributes]
/// Describes a set of participants inside a room.
/// This MUST always be locked before accessing it
struct RoomParticipants {
    room: RoomId,
}

#[derive(Display)]
/// k3k-signaling:room={room}:participants.lock
#[ignore_extra_doc_attributes]
/// Key used for the lock over the room participants set
pub struct RoomParticipantsLock {
    pub room: RoomId,
}

#[derive(Display)]
/// k3k-signaling:room={room}:participants:attributes:{attribute_name}
#[ignore_extra_doc_attributes]
/// Key used for the lock over the room participants set
struct RoomParticipantAttributes<'s> {
    room: RoomId,
    attribute_name: &'s str,
}

impl_to_redis_args!(RoomParticipants);
impl_to_redis_args!(RoomParticipantsLock);
impl_to_redis_args!(RoomParticipantAttributes<'_>);

/// The participant set mutex parameters are set very high since it is being held while destroying all modules which can take while
pub fn participant_set_mutex(room: RoomId) -> Mutex<RoomParticipantsLock> {
    Mutex::new(RoomParticipantsLock { room })
        .with_wait_time(Duration::from_millis(20)..Duration::from_millis(60))
        .with_retries(20)
}

#[tracing::instrument(level = "debug", skip(redis_conn))]
pub async fn get_all_participants(
    redis_conn: &mut ConnectionManager,
    room: RoomId,
) -> Result<HashSet<ParticipantId>> {
    let mut mutex = participant_set_mutex(room);

    let guard = mutex
        .lock(redis_conn)
        .await
        .context("Failed to lock participant list")?;

    let participants_result: Result<HashSet<ParticipantId>> = redis_conn
        .smembers(RoomParticipants { room })
        .await
        .context("Failed to get participants");

    guard
        .unlock(redis_conn)
        .await
        .context("Failed to unlock participant list")?;

    participants_result
}

#[tracing::instrument(level = "debug", skip(redis_conn))]
pub async fn participants_contains(
    redis_conn: &mut ConnectionManager,
    room: RoomId,
    participant: ParticipantId,
) -> Result<bool> {
    let mut mutex = participant_set_mutex(room);

    let guard = mutex
        .lock(redis_conn)
        .await
        .context("Failed to lock participant list")?;

    let is_member_result: Result<bool> = redis_conn
        .sismember(RoomParticipants { room }, participant)
        .await
        .context("Failed to check if participants contains participant");

    guard
        .unlock(redis_conn)
        .await
        .context("Failed to unlock participant list")?;

    is_member_result
}

#[tracing::instrument(level = "debug", skip(redis_conn))]
pub async fn add_participant_to_set(
    redis_conn: &mut ConnectionManager,
    room: RoomId,
    participant: ParticipantId,
) -> Result<()> {
    let mut mutex = participant_set_mutex(room);

    let guard = mutex
        .lock(redis_conn)
        .await
        .context("Failed to lock participant list")?;

    let add_result = redis_conn
        .sadd(RoomParticipants { room }, participant)
        .await
        .context("Failed to add own participant id to set");

    guard
        .unlock(redis_conn)
        .await
        .context("Failed to unlock participant list")?;

    add_result
}

/// Removes the given participant from the room's participant set and returns the number of
/// remaining participants
#[tracing::instrument(level = "debug", skip(_set_guard, redis_conn))]
pub async fn remove_participant_from_set(
    _set_guard: &MutexGuard<'_, RoomParticipantsLock>,
    redis_conn: &mut ConnectionManager,
    room: RoomId,
    participant: ParticipantId,
) -> Result<usize> {
    redis_conn
        .srem(RoomParticipants { room }, participant)
        .await
        .context("Failed to remove participant from participants-set")?;

    redis_conn
        .scard(RoomParticipants { room })
        .await
        .context("Failed to get number of remaining participants inside the set")
}

#[tracing::instrument(level = "debug", skip(redis_conn))]
pub async fn remove_attribute_key(
    redis_conn: &mut ConnectionManager,
    room: RoomId,
    name: &str,
) -> Result<()> {
    redis_conn
        .del(RoomParticipantAttributes {
            room,
            attribute_name: name,
        })
        .await
        .with_context(|| format!("Failed to remove participant attribute key, {}", name))
}

#[tracing::instrument(level = "debug", skip(redis_conn))]
pub async fn set_attribute<V>(
    redis_conn: &mut ConnectionManager,
    room: RoomId,
    participant: ParticipantId,
    name: &str,
    value: V,
) -> Result<()>
where
    V: Debug + ToRedisArgs + Send + Sync,
{
    redis_conn
        .hset(
            RoomParticipantAttributes {
                room,
                attribute_name: name,
            },
            participant,
            value,
        )
        .await
        .with_context(|| format!("Failed to set attribute {}", name))?;

    Ok(())
}

#[tracing::instrument(level = "debug", skip(redis_conn))]
pub async fn get_attribute<V>(
    redis_conn: &mut ConnectionManager,
    room: RoomId,
    participant: ParticipantId,
    name: &str,
) -> Result<V>
where
    V: FromRedisValue,
{
    let value = redis_conn
        .hget(
            RoomParticipantAttributes {
                room,
                attribute_name: name,
            },
            participant,
        )
        .await
        .with_context(|| format!("Failed to get attribute {}", name))?;

    Ok(value)
}

/// Get attribute values for multiple participants
///
/// The index of the attributes in the returned vector is a direct mapping to the provided list of participants.
pub async fn get_attribute_for_participants<V>(
    redis_conn: &mut ConnectionManager,
    room: RoomId,
    name: &str,
    participants: &[ParticipantId],
) -> Result<Vec<Option<V>>>
where
    V: FromRedisValue,
{
    redis_conn
        .hget(
            RoomParticipantAttributes {
                room,
                attribute_name: name,
            },
            participants,
        )
        .await
        .with_context(|| format!("Failed to get attribute '{}' for all participants ", name))
}
