use super::modules::{DynEvent, Modules, NoSuchModuleError};
use super::{Namespaced, WebSocket};
use crate::api::signaling::storage::Storage;
use crate::api::signaling::ws_modules::control::outgoing::Participant;
use crate::api::signaling::ws_modules::control::{incoming, outgoing, rabbitmq};
use crate::api::signaling::ParticipantId;
use crate::db::users::User;
use anyhow::{bail, Context, Result};
use async_tungstenite::tungstenite::Message;
use futures::SinkExt;
use lapin::options::QueueDeclareOptions;
use serde::Serialize;
use serde_json::Value;
use std::pin::Pin;
use std::time::Duration;
use tokio::time::{sleep, timeout, Sleep};
use tokio_stream::StreamExt;
use uuid::Uuid;

const PING_INTERVAL: Duration = Duration::from_secs(20);
const WS_MSG_TIMEOUT: Duration = Duration::from_secs(30);

pub struct Ws {
    websocket: WebSocket,
    // Websocket final message timeout, when reached the runner will exit
    timeout: Pin<Box<Sleep>>,
}

impl Ws {
    async fn receive(&mut self) -> Result<Message> {
        loop {
            tokio::select! {
                message = timeout(PING_INTERVAL, self.websocket.next()) => {
                    match message {
                        Ok(Some(Ok(msg))) => {
                            // Received a message, reset timeout after handling as to avoid
                            // triggering the sleep while handling the timeout
                            self.timeout.set(sleep(WS_MSG_TIMEOUT));

                            return Ok(msg);
                        }
                        Ok(Some(Err(e))) => bail!(e),
                        Ok(None) => bail!("WebSocket stream closed unexpectedly"),
                        Err(_) => {
                            // No messages for PING_INTERVAL amount of time
                            self.websocket.send(Message::Ping(vec![])).await?;
                        }
                    }
                }
                _ = self.timeout.as_mut() => {
                    bail!("Websocket timed out, peer no longer responds");
                }
            }
        }
    }
}

pub struct Runner {
    // participant id that the runner is connected to
    id: ParticipantId,

    // ID of the room the participant is inside
    room: Uuid,

    // User behind the participant
    user: User,

    // The control data. Initialized when frontend send join
    control_data: Option<ControlData>,

    // Websocket abstraction which helps detecting timeouts using regular ping-messages
    ws: Ws,

    // All registered and initialized modules
    modules: Modules,

    // Redis storage, which contains the state of all participants inside the room
    storage: Storage,

    // RabbitMQ queue consumer for this participant, will contain any events about room and
    // participant changes
    consumer: lapin::Consumer,

    // RabbitMQ channel to send events
    rabbit_mq_channel: lapin::Channel,

    // When set to true the runner will gracefully exit on next loop
    exit: bool,
}

impl Runner {
    pub async fn init(
        id: ParticipantId,
        room: Uuid,
        user: User,
        modules: Modules,
        mut storage: Storage,
        rabbit_mq_channel: lapin::Channel,
        websocket: WebSocket,
    ) -> Result<Self> {
        // ==== SETUP RABBITMQ CHANNEL ====
        let participant_key = format!("k3k-signaling.room.{}.participant.{}", Uuid::nil(), id);
        let room_key = format!("k3k-signaling.room.{}", Uuid::nil());

        let exchange_name = "k3k-signaling";

        let queue_options = QueueDeclareOptions {
            exclusive: false,
            auto_delete: true,
            ..Default::default()
        };

        let queue = rabbit_mq_channel
            .queue_declare(&participant_key, queue_options, Default::default())
            .await
            .context("Failed to create rabbitmq queue for websocket task")?;

        rabbit_mq_channel
            .exchange_declare(
                exchange_name,
                lapin::ExchangeKind::Topic,
                Default::default(),
                Default::default(),
            )
            .await?;

        rabbit_mq_channel
            .queue_bind(
                queue.name().as_str(),
                exchange_name,
                &participant_key,
                Default::default(),
                Default::default(),
            )
            .await?;

        rabbit_mq_channel
            .queue_bind(
                queue.name().as_str(),
                exchange_name,
                &room_key,
                Default::default(),
                Default::default(),
            )
            .await?;

        let consumer = rabbit_mq_channel
            .basic_consume(
                queue.name().as_str(),
                // Visual aid
                &participant_key,
                Default::default(),
                Default::default(),
            )
            .await?;

        // ==== SETUP BASIC REDIS DATA ====

        storage
            .set_attribute("control", id, "user_id", user.id)
            .await
            .context("Failed to set user id")?;

        Ok(Self {
            id,
            room,
            user,
            control_data: None,
            ws: Ws {
                websocket,
                timeout: Box::pin(sleep(WS_MSG_TIMEOUT)),
            },
            modules,
            storage,
            consumer,
            rabbit_mq_channel,
            exit: false,
        })
    }

    pub async fn run(mut self) {
        while !self.exit {
            tokio::select! {
                res = self.ws.receive() => {
                    match res {
                        Ok(msg) => self.handle_ws_message(msg).await,
                        Err(e) => {
                            log::error!("Failed to receive ws message, {}", e);
                            self.exit = true;
                        }
                    }
                }
                res = self.consumer.next() => {
                    match res {
                        Some(Ok((channel, delivery))) => self.handle_consumer_msg(channel, delivery).await,
                        _ => {
                            // None or Some(Err(_)), either way its an error to us
                            log::error!("Failed to receive RabbitMQ message, exiting");
                            self.exit = true;
                        }
                    }
                }
                Some((namespace, event)) = self.modules.poll_ext_events() => {
                    self.module_event(namespace, event)
                        .await
                        .expect("Should not get events from unknown modules");
                }
            }
        }

        log::debug!("Stopping ws-runner task");

        if let Err(e) = self.storage.remove_participant_from_set(self.id).await {
            log::error!("Failed to remove participant from set, {}", e);
        }

        if let Err(e) = self.storage.remove_all_attributes("control", self.id).await {
            log::error!("Failed to remove all control attributes, {}", e);
        }

        self.rabbitmq_send_typed("control", None, rabbitmq::Message::Left(self.id))
            .await;

        self.modules.destroy(&mut self.storage).await;
    }

    async fn handle_ws_message(&mut self, message: Message) {
        let value: Result<Namespaced<'_, Value>, _> = match message {
            Message::Text(ref text) => serde_json::from_str(&text),
            Message::Binary(ref binary) => serde_json::from_slice(&binary),
            Message::Ping(data) => {
                self.ws_send(Message::Pong(data)).await;
                return;
            }
            Message::Pong(_) => {
                // Response to keep alive
                return;
            }
            Message::Close(_) => {
                self.exit = true;
                return;
            }
        };

        let namespaced = match value {
            Ok(value) => value,
            Err(e) => {
                log::error!("Failed to parse namespaced message, {}", e);

                self.ws_send(Message::Text(error("invalid json message")))
                    .await;

                return;
            }
        };

        if namespaced.namespace == "control" {
            match serde_json::from_value(namespaced.payload) {
                Ok(msg) => self.handle_control_msg(msg).await,
                Err(e) => {
                    log::error!("Failed to parse control payload, {}", e);

                    self.ws_send(Message::Text(error("invalid json payload")))
                        .await;

                    return;
                }
            }
        } else if self.control_data.is_some() {
            if let Err(NoSuchModuleError(())) = self
                .module_event(
                    namespaced.namespace,
                    DynEvent::WsMessage(namespaced.payload),
                )
                .await
            {
                self.ws_send(Message::Text(error("unknown namespace")))
                    .await;
            }
        }
    }

    async fn handle_control_msg(&mut self, msg: incoming::Message) {
        if let Err(e) = self.try_handle_control_msg(msg).await {
            log::error!("Failed to handle control msg, {}", e);
            self.exit = true;
        }
    }

    async fn try_handle_control_msg(&mut self, msg: incoming::Message) -> Result<()> {
        match msg {
            incoming::Message::Join(join) => {
                self.storage
                    .set_attribute("control", self.id, "display_name", &join.display_name)
                    .await
                    .context("Failed to set display_name")?;

                self.control_data = Some(ControlData {
                    display_name: join.display_name,
                });

                let participant_set = self
                    .storage
                    .get_participants()
                    .await
                    .context("Failed to get all active participants")?;

                self.storage
                    .add_participant_to_set(self.id)
                    .await
                    .context("Failed to add self to participants set")?;

                let mut participants = vec![];

                for id in participant_set {
                    participants.push(self.build_participant(id).await?);
                }

                self.ws_send(Message::Text(
                    Namespaced {
                        namespace: "control",
                        payload: outgoing::Message::JoinSuccess(outgoing::JoinSuccess {
                            id: self.id,
                            participants,
                        }),
                    }
                    .to_json(),
                ))
                .await;

                self.rabbitmq_send_typed("control", None, rabbitmq::Message::Joined(self.id))
                    .await;
            }
        }

        Ok(())
    }

    async fn build_participant(&mut self, id: ParticipantId) -> Result<Participant> {
        let mut participant = outgoing::Participant {
            id,
            module_data: Default::default(),
        };

        let display_name: String = self
            .storage
            .get_attribute("control", id, "display_name")
            .await?;

        participant.module_data.insert(
            String::from("control"),
            serde_json::to_value(ControlData { display_name })
                .expect("Failed to convert ControlData to serde_json::Value"),
        );

        self.modules
            .collect_participant_data(&mut self.storage, &mut participant)
            .await
            .context("Failed to collect module frontend data")?;

        Ok(participant)
    }

    async fn handle_consumer_msg(&mut self, _: lapin::Channel, delivery: lapin::message::Delivery) {
        if let Err(e) = delivery.acker.ack(Default::default()).await {
            log::warn!("Failed to ACK incoming delivery, {}", e);
        }

        let namespaced = match serde_json::from_slice::<Namespaced<Value>>(&delivery.data) {
            Ok(namespaced) => namespaced,
            Err(e) => {
                log::error!("Failed to read incoming rabbit-mq message, {}", e);
                return;
            }
        };

        if namespaced.namespace == "control" {
            let msg = match serde_json::from_value::<rabbitmq::Message>(namespaced.payload) {
                Ok(msg) => msg,
                Err(e) => {
                    log::error!("Failed to read incoming control rabbit-mq message, {}", e);
                    return;
                }
            };

            if let Err(e) = self.handle_rabbitmq_control_msg(msg).await {
                log::error!("Failed to handle incoming rabbitmq control msg, {}", e);
                return;
            }
        } else {
            todo!("RabbitMQ messages for modules")
        }
    }

    async fn handle_rabbitmq_control_msg(&mut self, msg: rabbitmq::Message) -> Result<()> {
        log::debug!("Received RabbitMQ control message {:?}", msg);

        match msg {
            rabbitmq::Message::Joined(id) => {
                if self.id == id {
                    return Ok(());
                }

                let participant = self.build_participant(id).await?;

                self.ws_send(Message::Text(
                    Namespaced {
                        namespace: "control",
                        payload: outgoing::Message::Joined(participant),
                    }
                    .to_json(),
                ))
                .await;
            }
            rabbitmq::Message::Left(id) => {
                if self.id == id {
                    return Ok(());
                }

                self.ws_send(Message::Text(
                    Namespaced {
                        namespace: "control",
                        payload: outgoing::Message::Left(outgoing::AssociatedParticipant { id }),
                    }
                    .to_json(),
                ))
                .await;
            }
            rabbitmq::Message::Update(id) => {
                if self.id == id {
                    return Ok(());
                }

                let participant = self.build_participant(id).await?;

                self.ws_send(Message::Text(
                    Namespaced {
                        namespace: "control",
                        payload: outgoing::Message::Update(participant),
                    }
                    .to_json(),
                ))
                .await;
            }
        }

        Ok(())
    }

    async fn rabbitmq_send_typed<T>(
        &mut self,
        namespace: &str,
        recipient: Option<ParticipantId>,
        message: T,
    ) where
        T: Serialize,
    {
        let message = Namespaced {
            namespace,
            payload: message,
        };

        self.rabbitmq_send(recipient, message.to_json()).await;
    }

    async fn rabbitmq_send(&mut self, recipient: Option<ParticipantId>, message: String) {
        let routing_key = if let Some(recipient) = recipient {
            format!("k3k-signaling.room.{}.participant.{}", self.room, recipient)
        } else {
            format!("k3k-signaling.room.{}", self.room)
        };

        if let Err(e) = self
            .rabbit_mq_channel
            .basic_publish(
                "k3k-signaling",
                &routing_key,
                Default::default(),
                message.into_bytes(),
                Default::default(),
            )
            .await
        {
            log::error!("Failed to send message over rabbitmq, {}", e);
            self.exit = true;
            return;
        }
    }

    async fn ws_send(&mut self, message: Message) {
        if let Err(e) = self.ws.websocket.send(message).await {
            log::error!("Failed to send websocket message, {}", e);
            self.exit = true;
        }
    }

    async fn module_event(
        &mut self,
        module: &str,
        dyn_event: DynEvent,
    ) -> Result<(), NoSuchModuleError> {
        let mut ws_messages = vec![];
        let mut rabbitmq_messages = vec![];
        let mut invalidate_data = false;

        self.modules
            .on_event(
                self.id,
                &mut ws_messages,
                &mut rabbitmq_messages,
                &mut self.storage,
                &mut invalidate_data,
                module,
                dyn_event,
            )
            .await?;

        for ws_message in ws_messages {
            self.ws_send(ws_message).await;
        }

        for (recipient, rabbitmq_message) in rabbitmq_messages {
            self.rabbitmq_send(recipient, rabbitmq_message).await;
        }

        if invalidate_data {
            self.rabbitmq_send_typed("control", None, rabbitmq::Message::Update(self.id))
                .await;

            // TODO Collect self participant and send update?
        }

        Ok(())
    }
}

fn error(text: &str) -> String {
    Namespaced {
        namespace: "error",
        payload: text,
    }
    .to_json()
}

#[derive(Serialize)]
struct ControlData {
    display_name: String,
}
