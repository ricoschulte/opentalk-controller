// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use crate::api::signaling::{SignalingRoomId, Timestamp};
use crate::redis_wrapper::RedisConnection;
use anyhow::{Context, Result};
use controller_shared::ParticipantId;
use r3dlock::Mutex;
use redis::{AsyncCommands, FromRedisValue, ToRedisArgs};
use redis_args::ToRedisArgs;
use std::convert::identity;
use std::fmt::Debug;
use std::time::Duration;

/// Describes a set of participants inside a room.
/// This MUST always be locked before accessing it
#[derive(ToRedisArgs)]
#[to_redis_args(fmt = "k3k-signaling:room={room}:participants")]
struct RoomParticipants {
    room: SignalingRoomId,
}

/// Key used for the lock over the room participants set
#[derive(ToRedisArgs)]
#[to_redis_args(fmt = "k3k-signaling:room={room}:participants.lock")]
pub struct RoomLock {
    pub room: SignalingRoomId,
}

/// Key used for the lock over the room participants set
#[derive(ToRedisArgs)]
#[to_redis_args(fmt = "k3k-signaling:room={room}:participants:attributes:{attribute_name}")]
struct RoomParticipantAttributes<'s> {
    room: SignalingRoomId,
    attribute_name: &'s str,
}

/// The room's mutex
///
/// Must be taken when joining and leaving the room.
/// This allows for cleanups when the last user leaves without anyone joining.
///
/// The redlock parameters are set a bit higher than usual to combat contention when a room gets
/// destroyed while a large number of participants are inside it. (e.g. when a breakout room ends)
pub fn room_mutex(room: SignalingRoomId) -> Mutex<RoomLock> {
    Mutex::new(RoomLock { room })
        .with_wait_time(Duration::from_millis(20)..Duration::from_millis(60))
        .with_retries(20)
}

pub async fn participant_set_exists(
    redis_conn: &mut RedisConnection,
    room: SignalingRoomId,
) -> Result<bool> {
    redis_conn
        .exists(RoomParticipants { room })
        .await
        .context("Failed to check if participants exist")
}

#[tracing::instrument(level = "debug", skip(redis_conn))]
pub async fn get_all_participants(
    redis_conn: &mut RedisConnection,
    room: SignalingRoomId,
) -> Result<Vec<ParticipantId>> {
    redis_conn
        .smembers(RoomParticipants { room })
        .await
        .context("Failed to get participants")
}

#[tracing::instrument(level = "debug", skip(redis_conn))]
pub async fn remove_participant_set(
    redis_conn: &mut RedisConnection,
    room: SignalingRoomId,
) -> Result<()> {
    redis_conn
        .del(RoomParticipants { room })
        .await
        .context("Failed to del participants")
}

#[tracing::instrument(level = "debug", skip(redis_conn))]
pub async fn participants_contains(
    redis_conn: &mut RedisConnection,
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
    redis_conn: &mut RedisConnection,
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
    redis_conn: &mut RedisConnection,
    room: SignalingRoomId,
    participant: ParticipantId,
) -> Result<usize> {
    redis_conn
        .sadd(RoomParticipants { room }, participant)
        .await
        .context("Failed to add own participant id to set")
}

#[tracing::instrument(level = "debug", skip(redis_conn))]
pub async fn participants_all_left(
    redis_conn: &mut RedisConnection,
    room: SignalingRoomId,
) -> Result<bool> {
    let participants = get_all_participants(redis_conn, room).await?;

    let left_at_attrs: Vec<Option<Timestamp>> =
        get_attribute_for_participants(redis_conn, room, "left_at", &participants).await?;

    Ok(left_at_attrs.iter().all(Option::is_some))
}

#[tracing::instrument(level = "debug", skip(redis_conn))]
pub async fn remove_attribute_key(
    redis_conn: &mut RedisConnection,
    room: SignalingRoomId,
    name: &str,
) -> Result<()> {
    redis_conn
        .del(RoomParticipantAttributes {
            room,
            attribute_name: name,
        })
        .await
        .with_context(|| format!("Failed to remove participant attribute key, {name}"))
}

#[tracing::instrument(level = "debug", skip(redis_conn))]
pub async fn remove_attribute(
    redis_conn: &mut RedisConnection,
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
        .with_context(|| format!("Failed to remove participant attribute key, {name}"))
}

#[tracing::instrument(level = "debug", skip(redis_conn))]
pub async fn set_attribute<V>(
    redis_conn: &mut RedisConnection,
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
        .with_context(|| format!("Failed to set attribute {name}"))?;

    Ok(())
}

pub struct AttrPipeline {
    room: SignalingRoomId,
    participant: ParticipantId,
    pipe: redis::Pipeline,
}

// FIXME: Make the type inference better. e.g. by passing the type to get and letting get extend the final type.
impl AttrPipeline {
    pub fn new(room: SignalingRoomId, participant: ParticipantId) -> Self {
        let mut pipe = redis::pipe();
        pipe.atomic();

        Self {
            room,
            participant,
            pipe: redis::pipe(),
        }
    }

    pub fn set<V: ToRedisArgs>(&mut self, name: &str, value: V) -> &mut Self {
        self.pipe
            .hset(
                RoomParticipantAttributes {
                    room: self.room,
                    attribute_name: name,
                },
                self.participant,
                value,
            )
            .ignore();

        self
    }

    pub fn get(&mut self, name: &str) -> &mut Self {
        self.pipe.hget(
            RoomParticipantAttributes {
                room: self.room,
                attribute_name: name,
            },
            self.participant,
        );

        self
    }

    pub fn del(&mut self, name: &str) -> &mut Self {
        self.pipe
            .hdel(
                RoomParticipantAttributes {
                    room: self.room,
                    attribute_name: name,
                },
                self.participant,
            )
            .ignore();

        self
    }

    pub async fn query_async<T: FromRedisValue>(
        &mut self,
        redis_conn: &mut RedisConnection,
    ) -> redis::RedisResult<T> {
        self.pipe.query_async(redis_conn).await
    }
}

#[tracing::instrument(level = "debug", skip(redis_conn))]
pub async fn get_attribute<V>(
    redis_conn: &mut RedisConnection,
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
        .with_context(|| format!("Failed to get attribute {name}"))?;

    Ok(value)
}

/// Get attribute values for multiple participants
///
/// The index of the attributes in the returned vector is a direct mapping to the provided list of participants.
pub async fn get_attribute_for_participants<V>(
    redis_conn: &mut RedisConnection,
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
            .with_context(|| format!("Failed to get attribute '{name}' for all participants "))
    }
}

#[derive(Debug, ToRedisArgs)]
#[to_redis_args(fmt = "k3k-signaling:runner:{id}")]
pub struct ParticipantIdRunnerLock {
    pub id: ParticipantId,
}

pub async fn participant_id_in_use(
    redis_conn: &mut RedisConnection,
    participant_id: ParticipantId,
) -> Result<bool> {
    redis_conn
        .exists(ParticipantIdRunnerLock { id: participant_id })
        .await
        .context("failed to check if participant id is in use")
}

/// Key used for setting the `skip_waiting_room` attribute for a participant
#[derive(Debug, ToRedisArgs)]
#[to_redis_args(fmt = "k3k-signaling:participant={participant}:skip_waiting_room")]
pub struct SkipWaitingRoom {
    participant: ParticipantId,
}

/// Set the `skip_waiting_room` key for participant with an expiry in seconds.
#[tracing::instrument(level = "debug", skip(redis_conn))]
pub async fn set_skip_waiting_room_with_expiry(
    redis_conn: &mut RedisConnection,
    participant: ParticipantId,
    value: bool,
    expiration: usize,
) -> Result<()> {
    redis_conn
        .set_ex(SkipWaitingRoom { participant }, value, expiration)
        .await
        .with_context(|| {
            format!(
                "Failed to set skip_waiting_room key to {} for participant {}",
                value, participant,
            )
        })?;

    Ok(())
}

/// Set the `skip_waiting_room` key for participant with an expiry in seconds
/// if the key does not exist.
#[tracing::instrument(level = "debug", skip(redis_conn))]
pub async fn set_skip_waiting_room_with_expiry_nx(
    redis_conn: &mut RedisConnection,
    participant: ParticipantId,
    value: bool,
    expiry: usize,
) -> Result<()> {
    redis::pipe()
        .atomic()
        .set_nx(SkipWaitingRoom { participant }, value)
        .expire(SkipWaitingRoom { participant }, expiry)
        .query_async(redis_conn)
        .await
        .with_context(|| {
            format!(
                "Failed to set SkipWaitingRoom key to {} for participant {}",
                value, participant,
            )
        })?;

    Ok(())
}

/// Extend the `skip_waiting_room` key for participant with an expiry in seconds.
#[tracing::instrument(level = "debug", skip(redis_conn))]
pub async fn reset_skip_waiting_room_expiry(
    redis_conn: &mut RedisConnection,
    participant: ParticipantId,
    expiry: usize,
) -> Result<()> {
    redis_conn
        .expire(SkipWaitingRoom { participant }, expiry)
        .await
        .with_context(|| {
            format!(
                "Failed to extend skip_waiting_room key expiry for participant {}",
                participant,
            )
        })?;

    Ok(())
}

/// Get the `skip_waiting_room` value for participant. If no value is set for the key,
/// false is returned.
#[tracing::instrument(level = "debug", skip(redis_conn))]
pub async fn get_skip_waiting_room(
    redis_conn: &mut RedisConnection,
    participant: ParticipantId,
) -> Result<bool> {
    let value: Option<bool> = redis_conn.get(SkipWaitingRoom { participant }).await?;
    Ok(value.unwrap_or_default())
}
