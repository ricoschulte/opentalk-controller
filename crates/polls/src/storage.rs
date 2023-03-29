// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use super::Config;
use crate::{ChoiceId, PollId};
use anyhow::{bail, Context, Result};
use controller::prelude::*;
use redis::AsyncCommands;
use redis_args::ToRedisArgs;
use std::collections::HashMap;

/// Key to the current poll config
#[derive(ToRedisArgs)]
#[to_redis_args(fmt = "k3k-signaling:room={room}:polls:config")]
struct PollConfig {
    room: SignalingRoomId,
}

#[tracing::instrument(level = "debug", skip(redis_conn))]
pub(super) async fn get_config(
    redis_conn: &mut RedisConnection,
    room: SignalingRoomId,
) -> Result<Option<Config>> {
    redis_conn
        .get(PollConfig { room })
        .await
        .context("failed to get current config")
}

/// Set the current config if one doesn't already exist returns true if set was successful
#[tracing::instrument(level = "debug", skip(redis_conn))]
pub(super) async fn set_config(
    redis_conn: &mut RedisConnection,
    room: SignalingRoomId,
    config: &Config,
) -> Result<bool> {
    let value: redis::Value = redis::cmd("SET")
        .arg(PollConfig { room })
        .arg(config)
        .arg("EX")
        .arg(config.duration.as_secs())
        .arg("NX")
        .query_async(redis_conn)
        .await
        .context("failed to set current config")?;

    match value {
        redis::Value::Okay => Ok(true),
        redis::Value::Nil => Ok(false),
        _ => bail!("got invalid value from SET EX NX: {:?}", value),
    }
}

#[tracing::instrument(level = "debug", skip(redis_conn))]
pub(super) async fn del_config(
    redis_conn: &mut RedisConnection,
    room: SignalingRoomId,
) -> Result<()> {
    redis_conn
        .del(PollConfig { room })
        .await
        .context("failed to del current config")
}

/// Key to the current vote results
#[derive(ToRedisArgs)]
#[to_redis_args(fmt = "k3k-signaling:room={room}:poll={poll}:results")]
struct PollResults {
    room: SignalingRoomId,
    poll: PollId,
}

pub(super) async fn del_results(
    redis_conn: &mut RedisConnection,
    room: SignalingRoomId,
    poll_id: PollId,
) -> Result<()> {
    redis_conn
        .del(PollResults {
            room,
            poll: poll_id,
        })
        .await
        .context("failed to delete results")
}

pub(super) async fn vote(
    redis_conn: &mut RedisConnection,
    room: SignalingRoomId,
    poll_id: PollId,
    choice_id: ChoiceId,
) -> Result<()> {
    redis_conn
        .zincr(
            PollResults {
                room,
                poll: poll_id,
            },
            choice_id.0,
            1,
        )
        .await
        .context("failed to cast vote")
}

async fn results(
    redis_conn: &mut RedisConnection,
    room: SignalingRoomId,
    poll: PollId,
) -> Result<HashMap<ChoiceId, u32>> {
    redis_conn
        .zrange_withscores(PollResults { room, poll }, 0, -1)
        .await
        .context("failed to zrange vote results")
}

pub(super) async fn poll_results(
    redis_conn: &mut RedisConnection,
    room: SignalingRoomId,
    config: &Config,
) -> Result<Vec<crate::outgoing::Item>> {
    let votes = results(redis_conn, room, config.id).await?;

    let votes = (0..config.choices.len())
        .map(|i| {
            let id = ChoiceId(i as u32);
            let count = votes.get(&id).copied().unwrap_or_default();
            crate::outgoing::Item { id, count }
        })
        .collect();

    Ok(votes)
}

/// Key to the list of all polls inside the given room
#[derive(ToRedisArgs)]
#[to_redis_args(fmt = "k3k-signaling:room={room}:polls:list")]
struct PollList {
    room: SignalingRoomId,
}

/// Add a poll to the list
pub(super) async fn list_add(
    redis_conn: &mut RedisConnection,
    room: SignalingRoomId,
    poll_id: PollId,
) -> Result<()> {
    redis_conn
        .sadd(PollList { room }, poll_id)
        .await
        .context("failed to sadd poll list")
}

/// Get all polls for the room
pub(super) async fn list_members(
    redis_conn: &mut RedisConnection,
    room: SignalingRoomId,
) -> Result<Vec<PollId>> {
    redis_conn
        .smembers(PollList { room })
        .await
        .context("failed to get members from poll list")
}
