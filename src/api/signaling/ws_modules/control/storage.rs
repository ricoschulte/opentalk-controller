use crate::api::signaling::ParticipantId;
use anyhow::{Context, Result};
use displaydoc::Display;
use r3dlock::{Mutex, MutexGuard};
use redis::aio::ConnectionManager;
use redis::{AsyncCommands, FromRedisValue, ToRedisArgs};
use std::collections::HashSet;
use std::fmt::Debug;
use std::time::Duration;
use uuid::Uuid;

#[derive(Display)]
/// k3k-signaling:room={room}:participants
#[ignore_extra_doc_attributes]
/// Describes a set of participants inside a room.
/// This MUST always be locked before accessing it
struct RoomParticipants {
    room: Uuid,
}

#[derive(Display)]
/// k3k-signaling:room={room}:participants.lock
#[ignore_extra_doc_attributes]
/// Key used for the lock over the room participants set
pub struct RoomParticipantsLock {
    pub room: Uuid,
}

#[derive(Display)]
/// k3k-signaling:room={room}:participant={participant}:attributes
#[ignore_extra_doc_attributes]
/// Key used for the lock over the room participants set
struct RoomParticipantAttributes {
    room: Uuid,
    participant: ParticipantId,
}

impl_to_redis_args!(RoomParticipants);
impl_to_redis_args!(RoomParticipantsLock);
impl_to_redis_args!(RoomParticipantAttributes);

/// The participant set mutex parameters are set very high since it is being held while destroying all modules which can take while
pub fn participant_set_mutex(room: Uuid) -> Mutex<RoomParticipantsLock> {
    Mutex::new(RoomParticipantsLock { room })
        .with_wait_time(Duration::from_millis(20)..Duration::from_millis(60))
        .with_retries(20)
}

pub async fn get_all_participants(
    redis_conn: &mut ConnectionManager,
    room: Uuid,
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

pub async fn participants_contains(
    redis_conn: &mut ConnectionManager,
    room: Uuid,
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

pub async fn add_participant_to_set(
    redis_conn: &mut ConnectionManager,
    room: Uuid,
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
pub async fn remove_participant_from_set(
    _set_guard: &MutexGuard<'_, RoomParticipantsLock>,
    redis_conn: &mut ConnectionManager,
    room: Uuid,
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

pub async fn remove_all_attributes(
    redis_conn: &mut ConnectionManager,
    room: Uuid,
    participant: ParticipantId,
) -> Result<()> {
    redis_conn
        .del(RoomParticipantAttributes { room, participant })
        .await
        .context("Failed to remove participant attributes")
}

pub async fn set_attribute<K, V>(
    redis_conn: &mut ConnectionManager,
    room: Uuid,
    participant: ParticipantId,
    key: K,
    value: V,
) -> Result<()>
where
    K: ToRedisArgs + Send + Sync + Debug + Copy,
    V: ToRedisArgs + Send + Sync,
{
    redis_conn
        .hset(RoomParticipantAttributes { room, participant }, key, value)
        .await
        .with_context(|| format!("Failed to set attribute {:?}", key))?;

    Ok(())
}

pub async fn get_attribute<K, V>(
    redis_conn: &mut ConnectionManager,
    room: Uuid,
    participant: ParticipantId,
    key: K,
) -> Result<V>
where
    K: ToRedisArgs + Send + Sync + Debug + Copy,
    V: FromRedisValue,
{
    let value = redis_conn
        .hget(RoomParticipantAttributes { room, participant }, key)
        .await
        .with_context(|| format!("Failed to get attribute {:?}", key))?;

    Ok(value)
}
