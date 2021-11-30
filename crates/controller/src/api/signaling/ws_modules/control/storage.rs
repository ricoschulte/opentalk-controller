use crate::api::signaling::{SignalingRoomId, Timestamp};
use anyhow::{Context, Result};
use controller_shared::ParticipantId;
use displaydoc::Display;
use r3dlock::Mutex;
use redis::aio::ConnectionManager;
use redis::{AsyncCommands, FromRedisValue, ToRedisArgs};
use std::convert::identity;
use std::fmt::Debug;
use std::time::Duration;

#[derive(Display)]
/// k3k-signaling:room={room}:participants
#[ignore_extra_doc_attributes]
/// Describes a set of participants inside a room.
/// This MUST always be locked before accessing it
struct RoomParticipants {
    room: SignalingRoomId,
}

#[derive(Display)]
/// k3k-signaling:room={room}:participants.lock
#[ignore_extra_doc_attributes]
/// Key used for the lock over the room participants set
pub struct RoomLock {
    pub room: SignalingRoomId,
}

#[derive(Display)]
/// k3k-signaling:room={room}:participants:attributes:{attribute_name}
#[ignore_extra_doc_attributes]
/// Key used for the lock over the room participants set
struct RoomParticipantAttributes<'s> {
    room: SignalingRoomId,
    attribute_name: &'s str,
}

impl_to_redis_args!(RoomParticipants);
impl_to_redis_args!(RoomLock);
impl_to_redis_args!(RoomParticipantAttributes<'_>);

/// The room's mutex
///
/// Must be taken when joining and leaving the room.
/// This allows for cleanups when the last user leaves without anyone joining.
///
/// The redlock parameters are set a bit higher than usual to combat contention when a room gets
/// gets destroyed while a large number of participants are inside it. (e.g. when a breakout room ends)
pub fn room_mutex(room: SignalingRoomId) -> Mutex<RoomLock> {
    Mutex::new(RoomLock { room })
        .with_wait_time(Duration::from_millis(20)..Duration::from_millis(60))
        .with_retries(20)
}

#[tracing::instrument(level = "debug", skip(redis_conn))]
pub async fn get_all_participants(
    redis_conn: &mut ConnectionManager,
    room: SignalingRoomId,
) -> Result<Vec<ParticipantId>> {
    redis_conn
        .smembers(RoomParticipants { room })
        .await
        .context("Failed to get participants")
}

#[tracing::instrument(level = "debug", skip(redis_conn))]
pub async fn remove_participant_set(
    redis_conn: &mut ConnectionManager,
    room: SignalingRoomId,
) -> Result<()> {
    redis_conn
        .del(RoomParticipants { room })
        .await
        .context("Failed to del participants")
}

#[tracing::instrument(level = "debug", skip(redis_conn))]
pub async fn participants_contains(
    redis_conn: &mut ConnectionManager,
    room: SignalingRoomId,
    participant: ParticipantId,
) -> Result<bool> {
    redis_conn
        .sismember(RoomParticipants { room }, participant)
        .await
        .context("Failed to check if participants contains participant")
}

#[tracing::instrument(level = "debug", skip(redis_conn))]
pub async fn check_participants_exist(
    redis_conn: &mut ConnectionManager,
    room: SignalingRoomId,
    participants: &[ParticipantId],
) -> Result<bool> {
    let bools: Vec<bool> = redis::cmd("SMISMEMBER")
        .arg(RoomParticipants { room })
        .arg(participants)
        .query_async(redis_conn)
        .await
        .context("Failed to check if participants contains participant")?;

    Ok(bools.into_iter().all(identity))
}

#[tracing::instrument(level = "debug", skip(redis_conn))]
pub async fn add_participant_to_set(
    redis_conn: &mut ConnectionManager,
    room: SignalingRoomId,
    participant: ParticipantId,
) -> Result<usize> {
    let mut room_mutex = room_mutex(room);

    let guard = room_mutex.lock(redis_conn).await?;

    let sadd_result = redis_conn
        .sadd(RoomParticipants { room }, participant)
        .await
        .context("Failed to add own participant id to set");

    guard.unlock(redis_conn).await?;

    sadd_result
}

/// Mark the given participant in the given room as left.
///
/// # Returns
/// true if all participants in the room are marked as left
#[tracing::instrument(level = "debug", skip(redis_conn))]
pub async fn mark_participant_as_left(
    redis_conn: &mut ConnectionManager,
    room: SignalingRoomId,
    participant: ParticipantId,
) -> Result<bool> {
    set_attribute(redis_conn, room, participant, "left_at", Timestamp::now())
        .await
        .context("failed to set left_at attribute")?;

    let total_participant: usize = redis_conn
        .scard(RoomParticipants { room })
        .await
        .context("Failed to get number of participants in participant set")?;

    let left_participants: usize = redis_conn
        .hlen(RoomParticipantAttributes {
            room,
            attribute_name: "left_at",
        })
        .await
        .context("failed to get attributes len")?;

    Ok(total_participant == left_participants)
}

#[tracing::instrument(level = "debug", skip(redis_conn))]
pub async fn remove_attribute_key(
    redis_conn: &mut ConnectionManager,
    room: SignalingRoomId,
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
pub async fn remove_attribute(
    redis_conn: &mut ConnectionManager,
    room: SignalingRoomId,
    participant: ParticipantId,
    name: &str,
) -> Result<()> {
    redis_conn
        .hdel(
            RoomParticipantAttributes {
                room,
                attribute_name: name,
            },
            participant,
        )
        .await
        .with_context(|| format!("Failed to remove participant attribute key, {}", name))
}

#[tracing::instrument(level = "debug", skip(redis_conn))]
pub async fn set_attribute<V>(
    redis_conn: &mut ConnectionManager,
    room: SignalingRoomId,
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
    room: SignalingRoomId,
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
    room: SignalingRoomId,
    name: &str,
    participants: &[ParticipantId],
) -> Result<Vec<Option<V>>>
where
    V: FromRedisValue,
{
    // Special case: HMGET cannot handle empty arrays (missing arguments)
    if participants.is_empty() {
        Ok(vec![])
    } else {
        // need manual HMGET command as the HGET command wont work with single value vector input
        redis::cmd("HMGET")
            .arg(RoomParticipantAttributes {
                room,
                attribute_name: name,
            })
            .arg(participants)
            .query_async(redis_conn)
            .await
            .with_context(|| format!("Failed to get attribute '{}' for all participants ", name))
    }
}

#[derive(Debug, Display)]
/// k3k-signaling:runner:{id}
pub struct ParticipantIdRunnerLock {
    pub id: ParticipantId,
}

impl_to_redis_args!(ParticipantIdRunnerLock);
