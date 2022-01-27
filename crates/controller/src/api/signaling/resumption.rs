//! Resumption tokens are generated by the start endpoint and refreshed by the websocket runner.
//!
//! On start the frontend will be provided with the token in the response.
//!
//! In the case that the websocket disconnects the resumption token can be used by the frontend
//! to receive the same participant when reconnecting to the room. This enables all participant id
//! based features to recognize the reconnected client as the previously disconnected one.

use super::ws_modules::breakout::BreakoutRoomId;
use crate::api::Participant;
use crate::db::rooms::RoomId;
use crate::db::users::UserId;
use anyhow::{bail, Context, Result};
use controller_shared::ParticipantId;
use displaydoc::Display;
use rand::Rng;
use redis::aio::ConnectionManager;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use tokio::time::sleep_until;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResumptionToken(String);

impl ResumptionToken {
    pub fn generate() -> Self {
        let token = rand::thread_rng()
            .sample_iter(rand::distributions::Alphanumeric)
            .take(64)
            .map(char::from)
            .collect();

        Self(token)
    }

    pub fn into_redis_key(self) -> ResumptionRedisKey {
        ResumptionRedisKey { resumption: self.0 }
    }
}

#[derive(Debug, Display)]
/// k3k-signaling:resumption={resumption}
#[ignore_extra_doc_attributes]
/// Redis key for a resumption token containing [`ResumptionData`].
pub struct ResumptionRedisKey {
    pub resumption: String,
}

impl_to_redis_args!(&ResumptionRedisKey);

/// Data saved in redis behind the [`ResumptionRedisKey`]
#[derive(Debug, Serialize, Deserialize)]
pub struct ResumptionData {
    pub participant_id: ParticipantId,
    pub participant: Participant<UserId>,
    pub room: RoomId,
    pub breakout_room: Option<BreakoutRoomId>,
}

impl_from_redis_value_de!(ResumptionData);
impl_to_redis_args_se!(&ResumptionData);

/// Token refresh loop used in websocket runner to keep the resumption token alive and valid
pub struct ResumptionTokenKeepAlive {
    redis_key: ResumptionRedisKey,
    data: ResumptionData,
    next_refresh: Instant,
}

#[derive(Debug, thiserror::Error)]
#[error("resumption token could not be refreshed as it was used")]
pub struct ResumptionTokenUsed(());

impl ResumptionTokenKeepAlive {
    pub fn new(token: ResumptionToken, data: ResumptionData) -> Self {
        Self {
            redis_key: ResumptionRedisKey {
                resumption: token.0,
            },
            data,
            next_refresh: Instant::now() + Duration::from_secs(60),
        }
    }

    pub async fn set_initial(&mut self, redis_conn: &mut ConnectionManager) -> Result<()> {
        redis::cmd("SET")
            .arg(&self.redis_key)
            .arg(&self.data)
            .arg("EX")
            .arg(120)
            .arg("NX")
            .query_async(redis_conn)
            .await
            .context("failed to set initial resumption token")
    }

    pub async fn wait(&mut self) {
        sleep_until(self.next_refresh.into()).await;
    }

    pub async fn refresh(&mut self, redis_conn: &mut ConnectionManager) -> Result<()> {
        self.next_refresh = Instant::now() + Duration::from_secs(60);

        // Set the value with an timeout of 120 seconds (EX 120)
        // and only if it already exists
        let value: redis::Value = redis::cmd("SET")
            .arg(&self.redis_key)
            .arg(&self.data)
            .arg("EX")
            .arg(120)
            .arg("XX")
            .query_async(redis_conn)
            .await
            .context("failed to SET EX XX resumption data")?;

        match value {
            redis::Value::Nil => bail!(ResumptionTokenUsed(())),
            redis::Value::Okay => Ok(()),
            _ => bail!("unexpected redis response expected OK/nil got {:?}", value),
        }
    }
}
