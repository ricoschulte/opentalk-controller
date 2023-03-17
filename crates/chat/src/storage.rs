// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use crate::{MessageId, Scope};

use anyhow::{Context, Result};
use controller::prelude::*;
use db_storage::groups::{GroupId, GroupName};
use r3dlock::{Mutex, MutexGuard};
use redis::AsyncCommands;
use redis_args::{FromRedisValue, ToRedisArgs};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use types::core::{ParticipantId, RoomId};

/// Message type stores in redis
///
/// This needs to have a inner timestamp.
#[derive(Debug, Deserialize, Serialize, ToRedisArgs, FromRedisValue)]
#[to_redis_args(serde)]
#[from_redis_value(serde)]
pub struct StoredMessage {
    pub id: MessageId,
    pub source: ParticipantId,
    pub timestamp: Timestamp,
    pub content: String,
    #[serde(flatten)]
    pub scope: Scope,
}

/// Key to the chat history inside a room
#[derive(ToRedisArgs)]
#[to_redis_args(fmt = "k3k-signaling:room={room}:chat:history")]
struct RoomChatHistory {
    room: SignalingRoomId,
}

#[tracing::instrument(level = "debug", skip(redis_conn))]
pub async fn get_room_chat_history(
    redis_conn: &mut RedisConnection,
    room: SignalingRoomId,
) -> Result<Vec<StoredMessage>> {
    let messages = redis_conn
        .lrange(RoomChatHistory { room }, 0, -1)
        .await
        .with_context(|| format!("Failed to get chat history: room={room}"))?;

    Ok(messages)
}

#[tracing::instrument(level = "debug", skip(redis_conn, message))]
pub async fn add_message_to_room_chat_history(
    redis_conn: &mut RedisConnection,
    room: SignalingRoomId,
    message: &StoredMessage,
) -> Result<()> {
    redis_conn
        .lpush(RoomChatHistory { room }, message)
        .await
        .with_context(|| format!("Failed to add message to room chat history, room={room}"))?;

    Ok(())
}

#[tracing::instrument(level = "debug", skip(redis_conn))]
pub async fn delete_room_chat_history(
    redis_conn: &mut RedisConnection,
    room: SignalingRoomId,
) -> Result<()> {
    redis_conn
        .del(RoomChatHistory { room })
        .await
        .with_context(|| format!("Failed to delete room chat history, room={room}"))?;

    Ok(())
}

/// If set to true the chat is enabled
#[derive(ToRedisArgs)]
#[to_redis_args(fmt = "k3k-signaling:room={room}:chat_enabled")]
struct ChatEnabled {
    room: RoomId,
}

#[tracing::instrument(level = "debug", skip(redis_conn))]
pub async fn set_chat_enabled(
    redis_conn: &mut RedisConnection,
    room: RoomId,
    enabled: bool,
) -> Result<()> {
    redis_conn
        .set(ChatEnabled { room }, enabled)
        .await
        .context("Failed to SET chat_enabled")
}

#[tracing::instrument(level = "debug", skip(redis_conn))]
pub async fn is_chat_enabled(redis_conn: &mut RedisConnection, room: RoomId) -> Result<bool> {
    redis_conn
        .get(ChatEnabled { room })
        .await
        .context("Failed to GET chat_enabled")
        .map(|result: Option<bool>| result.unwrap_or(true))
}

#[tracing::instrument(level = "debug", skip(redis_conn))]
pub async fn delete_chat_enabled(redis_conn: &mut RedisConnection, room: RoomId) -> Result<()> {
    redis_conn
        .del(ChatEnabled { room })
        .await
        .context("Failed to DEL chat_enabled")
}

/// A hash of last-seen timestamps
#[derive(ToRedisArgs)]
#[to_redis_args(fmt = "k3k-signaling:room={room}:participant={participant}:chat:last_seen:global")]
struct RoomParticipantLastSeenTimestampPrivate {
    room: SignalingRoomId,
    participant: ParticipantId,
}

#[tracing::instrument(level = "debug", skip(redis_conn))]
pub async fn set_last_seen_timestamps_private(
    redis_conn: &mut RedisConnection,
    room: SignalingRoomId,
    participant: ParticipantId,
    timestamps: &[(ParticipantId, Timestamp)],
) -> Result<()> {
    redis_conn
        .hset_multiple(
            RoomParticipantLastSeenTimestampPrivate { room, participant },
            timestamps,
        )
        .await
        .context("Failed to HSET messages last seen timestamp for private chat")
}

#[tracing::instrument(level = "debug", skip(redis_conn))]
pub async fn get_last_seen_timestamps_private(
    redis_conn: &mut RedisConnection,
    room: SignalingRoomId,
    participant: ParticipantId,
) -> Result<HashMap<ParticipantId, Timestamp>> {
    redis_conn
        .hgetall(RoomParticipantLastSeenTimestampPrivate { room, participant })
        .await
        .context("Failed to HGETALL messages last seen timestamps for private chats")
}

#[tracing::instrument(level = "debug", skip(redis_conn))]
pub async fn delete_last_seen_timestamps_private(
    redis_conn: &mut RedisConnection,
    room: SignalingRoomId,
    participant: ParticipantId,
) -> Result<()> {
    redis_conn
        .del(RoomParticipantLastSeenTimestampPrivate { room, participant })
        .await
        .context("Failed to DEL messages last seen timestamps for private chats")
}

/// A hash of last-seen timestamps
#[derive(ToRedisArgs)]
#[to_redis_args(fmt = "k3k-signaling:room={room}:participant={participant}:chat:last_seen:group")]
struct RoomParticipantLastSeenTimestampsGroup {
    room: SignalingRoomId,
    participant: ParticipantId,
}

#[tracing::instrument(level = "debug", skip(redis_conn))]
pub async fn set_last_seen_timestamps_group(
    redis_conn: &mut RedisConnection,
    room: SignalingRoomId,
    participant: ParticipantId,
    timestamps: &[(GroupName, Timestamp)],
) -> Result<()> {
    redis_conn
        .hset_multiple(
            RoomParticipantLastSeenTimestampsGroup { room, participant },
            timestamps,
        )
        .await
        .context("Failed to HSET messages last seen timestamp for group chats")
}

#[tracing::instrument(level = "debug", skip(redis_conn))]
pub async fn get_last_seen_timestamps_group(
    redis_conn: &mut RedisConnection,
    room: SignalingRoomId,
    participant: ParticipantId,
) -> Result<HashMap<GroupName, Timestamp>> {
    redis_conn
        .hgetall(RoomParticipantLastSeenTimestampsGroup { room, participant })
        .await
        .context("Failed to HGETALL messages last seen timestamp for group chats")
}

#[tracing::instrument(level = "debug", skip(redis_conn))]
pub async fn delete_last_seen_timestamps_group(
    redis_conn: &mut RedisConnection,
    room: SignalingRoomId,
    participant: ParticipantId,
) -> Result<()> {
    redis_conn
        .del(RoomParticipantLastSeenTimestampsGroup { room, participant })
        .await
        .context("Failed to DEL last seen timestamp for group chats")
}

/// A hash of last-seen timestamps
#[derive(ToRedisArgs)]
#[to_redis_args(fmt = "k3k-signaling:room={room}:participant={participant}:chat:last_seen:private")]
struct RoomParticipantLastSeenTimestampGlobal {
    room: SignalingRoomId,
    participant: ParticipantId,
}

#[tracing::instrument(level = "debug", skip(redis_conn))]
pub async fn set_last_seen_timestamp_global(
    redis_conn: &mut RedisConnection,
    room: SignalingRoomId,
    participant: ParticipantId,
    timestamp: Timestamp,
) -> Result<()> {
    redis_conn
        .set(
            RoomParticipantLastSeenTimestampGlobal { room, participant },
            timestamp,
        )
        .await
        .context("Failed to HSET messages last seen timestamp for global chat")
}

#[tracing::instrument(level = "debug", skip(redis_conn))]
pub async fn get_last_seen_timestamp_global(
    redis_conn: &mut RedisConnection,
    room: SignalingRoomId,
    participant: ParticipantId,
) -> Result<Option<Timestamp>> {
    let key = RoomParticipantLastSeenTimestampGlobal { room, participant };
    redis_conn
        .get(&key)
        .await
        .context("Failed to GET messages last seen timestamp for global chat")
}

#[tracing::instrument(level = "debug", skip(redis_conn))]
pub async fn delete_last_seen_timestamp_global(
    redis_conn: &mut RedisConnection,
    room: SignalingRoomId,
    participant: ParticipantId,
) -> Result<()> {
    redis_conn
        .del(RoomParticipantLastSeenTimestampGlobal { room, participant })
        .await
        .context("Failed to DEL messages last seen timestamp for global chat")
}

#[cfg(test)]
mod test {
    use super::*;
    use chrono::{DateTime, Utc};
    use redis::aio::ConnectionManager;
    use redis::ToRedisArgs;
    use serial_test::serial;
    use std::time::{Duration, SystemTime};
    use types::core::RoomId;
    use uuid::uuid;

    pub const ROOM: SignalingRoomId = SignalingRoomId::new_test(RoomId::from(uuid::Uuid::nil()));
    pub const SELF: ParticipantId = ParticipantId::nil();
    pub const BOB: ParticipantId = ParticipantId::from_u128(0xdeadbeef);
    pub const ALICE: ParticipantId = ParticipantId::from_u128(0xbadcafe);

    async fn setup() -> RedisConnection {
        let redis_url =
            std::env::var("REDIS_ADDR").unwrap_or_else(|_| "redis://0.0.0.0:6379/".to_owned());
        let redis = redis::Client::open(redis_url).expect("Invalid redis url");

        let mut mgr = ConnectionManager::new(redis).await.unwrap();

        redis::cmd("FLUSHALL")
            .query_async::<_, ()>(&mut mgr)
            .await
            .unwrap();

        RedisConnection::new(mgr)
    }

    fn unix_epoch(secs: u64) -> DateTime<Utc> {
        DateTime::from(SystemTime::UNIX_EPOCH + Duration::from_secs(secs))
    }

    #[tokio::test]
    #[serial]
    async fn last_seen_global() {
        let mut redis_conn = setup().await;

        assert!(get_last_seen_timestamp_global(&mut redis_conn, ROOM, SELF)
            .await
            .unwrap()
            .is_none());

        set_last_seen_timestamp_global(&mut redis_conn, ROOM, SELF, unix_epoch(1000).into())
            .await
            .unwrap();

        assert_eq!(
            get_last_seen_timestamp_global(&mut redis_conn, ROOM, SELF)
                .await
                .unwrap(),
            Some(unix_epoch(1000).into())
        );

        delete_last_seen_timestamp_global(&mut redis_conn, ROOM, SELF)
            .await
            .unwrap();

        assert!(get_last_seen_timestamp_global(&mut redis_conn, ROOM, SELF)
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    #[serial]
    async fn last_seen_global_is_personal() {
        let mut redis_conn = setup().await;

        // Set the private last seen timestamps as if BOB and ALICE were the participants in the
        // same room, and ensure this doesn't affect the timestamps of SELF.
        {
            // Set BOB's timestamp
            set_last_seen_timestamp_global(&mut redis_conn, ROOM, BOB, unix_epoch(1000).into())
                .await
                .unwrap();
        }
        {
            // Set ALICE's timestamp
            set_last_seen_timestamp_global(&mut redis_conn, ROOM, ALICE, unix_epoch(2000).into())
                .await
                .unwrap();
        }

        assert!(get_last_seen_timestamp_global(&mut redis_conn, ROOM, SELF)
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    #[serial]
    async fn last_seen_private() {
        let mut redis_conn = setup().await;

        assert!(
            get_last_seen_timestamps_private(&mut redis_conn, ROOM, SELF)
                .await
                .unwrap()
                .is_empty(),
        );

        set_last_seen_timestamps_private(
            &mut redis_conn,
            ROOM,
            SELF,
            &[(BOB, unix_epoch(1000).into())],
        )
        .await
        .unwrap();

        assert_eq!(
            get_last_seen_timestamps_private(&mut redis_conn, ROOM, SELF)
                .await
                .unwrap(),
            HashMap::from_iter([(BOB, unix_epoch(1000).into())])
        );

        set_last_seen_timestamps_private(
            &mut redis_conn,
            ROOM,
            SELF,
            &[(ALICE, unix_epoch(2000).into())],
        )
        .await
        .unwrap();

        assert_eq!(
            get_last_seen_timestamps_private(&mut redis_conn, ROOM, SELF)
                .await
                .unwrap(),
            HashMap::from_iter([
                (BOB, unix_epoch(1000).into()),
                (ALICE, unix_epoch(2000).into()),
            ])
        );

        delete_last_seen_timestamps_private(&mut redis_conn, ROOM, SELF)
            .await
            .unwrap();

        assert!(
            get_last_seen_timestamps_private(&mut redis_conn, ROOM, SELF)
                .await
                .unwrap()
                .is_empty(),
        );
    }

    #[tokio::test]
    #[serial]
    async fn last_seen_private_is_personal() {
        let mut redis_conn = setup().await;

        // Set the private last seen timestamps as if BOB and ALICE were the participants in the
        // same room, and ensure this doesn't affect the timestamps of SELF.
        {
            // Set BOB's personal timestamps
            set_last_seen_timestamps_private(
                &mut redis_conn,
                ROOM,
                BOB,
                &[
                    (ALICE, unix_epoch(1000).into()),
                    (SELF, unix_epoch(2000).into()),
                ],
            )
            .await
            .unwrap();
        }
        {
            // Set ALICE's personal timestamps
            set_last_seen_timestamps_private(
                &mut redis_conn,
                ROOM,
                ALICE,
                &[(SELF, unix_epoch(3000).into())],
            )
            .await
            .unwrap();
        }

        assert!(
            get_last_seen_timestamps_private(&mut redis_conn, ROOM, SELF)
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn redis_args() {
        let room_id = RoomId::from(uuid!("ecead1b3-eed0-4cb9-912e-4bb31a3914bd"));

        {
            let id = RoomChatHistory {
                room: SignalingRoomId::new_test(room_id),
            };
            assert_eq!(
                id.to_redis_args(),
                "k3k-signaling:room=ecead1b3-eed0-4cb9-912e-4bb31a3914bd:chat:history"
                    .to_redis_args()
            );
        }
        {
            let id = ChatEnabled { room: room_id };
            assert_eq!(
                id.to_redis_args(),
                "k3k-signaling:room=ecead1b3-eed0-4cb9-912e-4bb31a3914bd:chat_enabled"
                    .to_redis_args()
            )
        }
    }
}

/// A set of group members inside a room
#[derive(ToRedisArgs)]
#[to_redis_args(fmt = "k3k-signaling:room={room}:group={group}:participants")]
struct RoomGroupParticipants {
    room: SignalingRoomId,
    group: GroupId,
}

/// A lock for the set of group members inside a room
#[derive(ToRedisArgs)]
#[to_redis_args(fmt = "k3k-signaling:room={room}:group={group}:participants.lock")]
pub struct RoomGroupParticipantsLock {
    pub room: SignalingRoomId,
    pub group: GroupId,
}

/// A lock for a group chat history inside a room
#[derive(ToRedisArgs)]
#[to_redis_args(fmt = "k3k-signaling:room={room}:group={group}:chat:history")]
struct RoomGroupChatHistory {
    room: SignalingRoomId,
    group: GroupId,
}

pub async fn add_participant_to_set(
    redis_conn: &mut RedisConnection,
    room: SignalingRoomId,
    group: GroupId,
    participant: ParticipantId,
) -> Result<()> {
    let mut mutex = Mutex::new(RoomGroupParticipantsLock { room, group });

    let guard = mutex
        .lock(redis_conn)
        .await
        .context("Failed to lock participant list")?;

    redis_conn
        .sadd(RoomGroupParticipants { room, group }, participant)
        .await
        .context("Failed to add own participant id to set")?;

    guard
        .unlock(redis_conn)
        .await
        .context("Failed to unlock participant list")?;

    Ok(())
}

pub async fn remove_participant_from_set(
    _set_guard: &MutexGuard<'_, RoomGroupParticipantsLock>,
    redis_conn: &mut RedisConnection,
    room: SignalingRoomId,
    group: GroupId,
    participant: ParticipantId,
) -> Result<usize> {
    redis_conn
        .srem(RoomGroupParticipants { room, group }, participant)
        .await
        .context("Failed to remove participant from participants-set")?;

    redis_conn
        .scard(RoomGroupParticipants { room, group })
        .await
        .context("Failed to get number of remaining participants inside the set")
}

#[tracing::instrument(level = "debug", skip(redis_conn))]
pub async fn get_group_chat_history(
    redis_conn: &mut RedisConnection,
    room: SignalingRoomId,
    group: GroupId,
) -> Result<Vec<StoredMessage>> {
    redis_conn
        .lrange(RoomGroupChatHistory { room, group }, 0, -1)
        .await
        .with_context(|| format!("Failed to get chat history, {room}, group={group}"))
}

#[tracing::instrument(level = "debug", skip(redis_conn, message))]
pub async fn add_message_to_group_chat_history(
    redis_conn: &mut RedisConnection,
    room: SignalingRoomId,
    group: GroupId,
    message: &StoredMessage,
) -> Result<()> {
    redis_conn
        .lpush(RoomGroupChatHistory { room, group }, message)
        .await
        .with_context(|| {
            format!("Failed to add message to room chat history, {room}, group={group}",)
        })
}

#[tracing::instrument(level = "debug", skip(redis_conn))]
pub async fn delete_group_chat_history(
    redis_conn: &mut RedisConnection,
    room: SignalingRoomId,
    group: GroupId,
) -> Result<()> {
    redis_conn
        .del(RoomGroupChatHistory { room, group })
        .await
        .with_context(
            || format!("Failed to delete room group chat history, {room}, group={group}",),
        )
}
