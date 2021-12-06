use anyhow::Result;
use chrono::Utc;
use controller::prelude::*;
use futures::stream::once;
use futures::FutureExt;
use redis::{self, FromRedisValue, RedisResult};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::{from_utf8, FromStr};
use std::time::Duration;
use tokio::time::sleep;
use uuid::Uuid;

pub mod incoming;
pub mod outgoing;
pub mod rabbitmq;
mod storage;

pub struct ExpiredEvent(PollId);

pub struct Polls {
    role: Role,
    room: SignalingRoomId,
    config: Option<Config>,
}

#[async_trait::async_trait(?Send)]
impl SignalingModule for Polls {
    const NAMESPACE: &'static str = "polls";

    type Params = ();

    type Incoming = incoming::Message;
    type Outgoing = outgoing::Message;
    type RabbitMqMessage = rabbitmq::Message;

    type ExtEvent = ExpiredEvent;

    type FrontendData = Config;
    type PeerFrontendData = ();

    async fn init(
        ctx: InitContext<'_, Self>,
        _: &Self::Params,
        _: &'static str,
    ) -> Result<Option<Self>> {
        Ok(Some(Self {
            role: ctx.role(),
            room: ctx.room_id(),
            config: None,
        }))
    }

    async fn on_event(
        &mut self,
        mut ctx: ModuleContext<'_, Self>,
        event: Event<'_, Self>,
    ) -> Result<()> {
        match event {
            Event::Joined {
                control_data: _,
                frontend_data,
                participants: _,
            } => {
                if let Some(config) = storage::get_config(ctx.redis_conn(), self.room).await? {
                    if let Some(duration) = config.remaining() {
                        let id = config.id;

                        self.config = Some(config.clone());
                        *frontend_data = Some(config);

                        ctx.add_event_stream(once(sleep(duration).map(move |_| ExpiredEvent(id))));
                    }
                }

                Ok(())
            }
            Event::Leaving => Ok(()),
            Event::RaiseHand => Ok(()),
            Event::LowerHand => Ok(()),
            Event::ParticipantJoined(_, _) => Ok(()),
            Event::ParticipantLeft(_) => Ok(()),
            Event::ParticipantUpdated(_, _) => Ok(()),
            Event::WsMessage(msg) => self.on_ws_message(ctx, msg).await,
            Event::RabbitMq(msg) => self.on_rabbitmq_message(ctx, msg).await,
            Event::Ext(ExpiredEvent(id)) => {
                if let Some(config) = self.config.as_ref().filter(|config| config.id == id) {
                    let results =
                        storage::poll_results(ctx.redis_conn(), self.room, config).await?;

                    ctx.ws_send(outgoing::Message::Done(outgoing::Results { id, results }));
                }

                Ok(())
            }
        }
    }

    async fn on_destroy(self, mut ctx: DestroyContext<'_>) {
        if ctx.destroy_room() {
            if let Err(e) = storage::del_config(ctx.redis_conn(), self.room).await {
                log::error!("failed to remove config from redis: {:?}", e);
            }

            let list = match storage::list_members(ctx.redis_conn(), self.room).await {
                Ok(list) => list,
                Err(e) => {
                    log::error!("failed to get list of poll results to clean up, {:?}", e);
                    return;
                }
            };

            for id in list {
                if let Err(e) = storage::del_results(ctx.redis_conn(), self.room, id).await {
                    log::error!("failed to remove poll results for id {}, {:?}", id, e);
                }
            }
        }
    }
}

impl Polls {
    fn is_running(&self) -> bool {
        self.config
            .as_ref()
            .map(|config| !config.is_expired())
            .unwrap_or_default()
    }

    async fn on_ws_message(
        &mut self,
        mut ctx: ModuleContext<'_, Self>,
        msg: incoming::Message,
    ) -> Result<()> {
        match msg {
            incoming::Message::Start(incoming::Start {
                topic,
                live,
                choices,
                duration,
            }) => {
                if self.role != Role::Moderator {
                    ctx.ws_send(outgoing::Message::Error(
                        outgoing::Error::InsufficientPermissions,
                    ));

                    return Ok(());
                }

                if self.is_running() {
                    ctx.ws_send(outgoing::Message::Error(outgoing::Error::StillRunning));

                    return Ok(());
                }

                // TODO(k.balt): Minimal duration 2 secs for tests but thats unreasonably low real world applications
                let min = Duration::from_secs(2);
                let max = Duration::from_secs(3600);

                if duration > max || duration < min {
                    ctx.ws_send(outgoing::Message::Error(outgoing::Error::InvalidDuration));

                    return Ok(());
                }

                if let 2..=64 = choices.len() {
                    // OK
                } else {
                    ctx.ws_send(outgoing::Message::Error(
                        outgoing::Error::InvalidChoiceCount,
                    ));

                    return Ok(());
                }

                let choices = choices
                    .into_iter()
                    .enumerate()
                    .map(|(i, content)| Choice {
                        id: ChoiceId(i as u32),
                        content,
                    })
                    .collect();

                let config = Config {
                    id: PollId(Uuid::new_v4()),
                    topic,
                    live,
                    choices,
                    started: ctx.timestamp(),
                    duration,
                    voted: false,
                };

                let set = storage::set_config(ctx.redis_conn(), self.room, &config).await?;

                if !set {
                    ctx.ws_send(outgoing::Message::Error(outgoing::Error::StillRunning));

                    return Ok(());
                }

                storage::list_add(ctx.redis_conn(), self.room, config.id).await?;

                ctx.rabbitmq_publish(
                    control::rabbitmq::current_room_exchange_name(self.room),
                    control::rabbitmq::room_all_routing_key().into(),
                    rabbitmq::Message::Started(config),
                );

                Ok(())
            }
            incoming::Message::Vote(incoming::Vote { poll_id, choice_id }) => {
                if let Some(config) = self
                    .config
                    .as_mut()
                    .filter(|config| config.id == poll_id && !config.is_expired())
                {
                    if config.voted {
                        ctx.ws_send(outgoing::Message::Error(outgoing::Error::VotedAlready));

                        return Ok(());
                    }

                    if config.choices.iter().any(|choice| choice.id == choice_id) {
                        storage::vote(ctx.redis_conn(), self.room, config.id, choice_id).await?;

                        config.voted = true;

                        if config.live {
                            ctx.rabbitmq_publish(
                                control::rabbitmq::current_room_exchange_name(self.room),
                                control::rabbitmq::room_all_routing_key().into(),
                                rabbitmq::Message::Update(poll_id),
                            );
                        }
                    } else {
                        ctx.ws_send(outgoing::Message::Error(outgoing::Error::InvalidChoiceId));
                    }
                } else {
                    ctx.ws_send(outgoing::Message::Error(outgoing::Error::InvalidPollId));
                }

                Ok(())
            }
            incoming::Message::Finish(finish) => {
                if self.role != Role::Moderator {
                    ctx.ws_send(outgoing::Message::Error(
                        outgoing::Error::InsufficientPermissions,
                    ));

                    return Ok(());
                }

                if self
                    .config
                    .as_ref()
                    .filter(|config| config.id == finish.id && !config.is_expired())
                    .is_some()
                {
                    // Delete config from redis to stop vote
                    storage::del_config(ctx.redis_conn(), self.room).await?;

                    ctx.rabbitmq_publish(
                        control::rabbitmq::current_room_exchange_name(self.room),
                        control::rabbitmq::room_all_routing_key().into(),
                        rabbitmq::Message::Finish(finish.id),
                    );
                } else {
                    ctx.ws_send(outgoing::Message::Error(outgoing::Error::InvalidPollId));
                }

                Ok(())
            }
        }
    }

    async fn on_rabbitmq_message(
        &mut self,
        mut ctx: ModuleContext<'_, Self>,
        msg: rabbitmq::Message,
    ) -> Result<()> {
        match msg {
            rabbitmq::Message::Started(config) => {
                let id = config.id;

                ctx.ws_send_overwrite_timestamp(
                    outgoing::Message::Started(outgoing::Started {
                        id,
                        topic: config.topic.clone(),
                        live: config.live,
                        choices: config.choices.clone(),
                        duration: config.duration,
                    }),
                    config.started,
                );

                ctx.add_event_stream(once(sleep(config.duration).map(move |_| ExpiredEvent(id))));

                self.config = Some(config);

                Ok(())
            }
            rabbitmq::Message::Update(id) => {
                if let Some(config) = &self.config {
                    let results =
                        storage::poll_results(ctx.redis_conn(), self.room, config).await?;

                    ctx.ws_send(outgoing::Message::LiveUpdate(outgoing::Results {
                        id,
                        results,
                    }));
                }

                Ok(())
            }
            rabbitmq::Message::Finish(id) => {
                if let Some(config) = self.config.take() {
                    let results =
                        storage::poll_results(ctx.redis_conn(), self.room, &config).await?;

                    ctx.ws_send(outgoing::Message::Done(outgoing::Results { id, results }));
                }

                Ok(())
            }
        }
    }
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct PollId(pub Uuid);
impl_to_redis_args!(PollId);

impl FromRedisValue for PollId {
    fn from_redis_value(v: &redis::Value) -> RedisResult<Self> {
        match v {
            redis::Value::Data(bytes) => {
                Uuid::from_str(from_utf8(bytes)?).map(Self).map_err(|_| {
                    redis::RedisError::from((
                        redis::ErrorKind::TypeError,
                        "invalid data for ParticipantId",
                    ))
                })
            }
            _ => RedisResult::Err(redis::RedisError::from((
                redis::ErrorKind::TypeError,
                "invalid data type for ParticipantId",
            ))),
        }
    }
}

impl fmt::Display for PollId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct ChoiceId(pub u32);

impl FromRedisValue for ChoiceId {
    fn from_redis_value(v: &redis::Value) -> RedisResult<Self> {
        u32::from_redis_value(v).map(Self)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Choice {
    pub id: ChoiceId,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    id: PollId,
    topic: String,
    live: bool,
    choices: Vec<Choice>,
    started: Timestamp,
    #[serde(with = "duration_millis")]
    duration: Duration,

    // skip flag, not serialized into redis and always false when reading from it
    // Indicates if the user of the module has already voted for this config
    #[serde(skip, default)]
    voted: bool,
}
impl_from_redis_value_de!(Config);
impl_to_redis_args_se!(&Config);

impl Config {
    fn remaining(&self) -> Option<Duration> {
        let duration = chrono::Duration::from_std(self.duration)
            .expect("duration as secs should never be larger than i64::MAX");

        let expire = (*self.started) + duration;
        let now = Utc::now();

        // difference will be negative duration if expired.
        // Conversion to std duration will fail -> returning None
        (expire - now).to_std().ok()
    }

    fn is_expired(&self) -> bool {
        self.remaining().is_none()
    }
}

mod duration_millis {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Duration, D::Error>
    where
        D: Deserializer<'de>,
    {
        u64::deserialize(deserializer).map(Duration::from_millis)
    }

    pub fn serialize<S>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u64(duration.as_millis() as u64)
    }
}
