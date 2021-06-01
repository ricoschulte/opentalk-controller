use super::modules::{DynEvent, DynEventCtx, Modules, NoSuchModuleError};
use super::WebSocket;
use actix_web::dev::Extensions;
use async_tungstenite::tungstenite::Message;
use futures::SinkExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::pin::Pin;
use std::time::Duration;
use tokio::time::{sleep, timeout, Sleep};
use tokio_stream::StreamExt;

const PING_INTERVAL: Duration = Duration::from_secs(20);
const WS_MSG_TIMEOUT: Duration = Duration::from_secs(30);

pub struct Runner {
    websocket: WebSocket,
    timeout: Pin<Box<Sleep>>,

    exit: bool,

    modules: Modules,

    local_state: Extensions,
}

impl Runner {
    pub fn new(modules: Modules, websocket: WebSocket) -> Self {
        Self {
            websocket,
            exit: false,
            timeout: Box::pin(sleep(WS_MSG_TIMEOUT)),
            modules,
            local_state: Default::default(),
        }
    }

    pub async fn run(mut self) {
        while !self.exit {
            tokio::select! {
                message = timeout(PING_INTERVAL, self.websocket.next()) => {
                    match message {
                        Ok(Some(Ok(message))) => {
                            self.handle_ws_message(message).await;

                            // Received a message, reset timeout after handling as to avoid
                            // triggering the sleep while handling the timeout
                            self.timeout.set(sleep(WS_MSG_TIMEOUT));
                        }
                        Ok(Some(Err(e))) => {
                            log::warn!("WebSocket error, {}", e);
                            break;
                        }
                        Ok(None) => {
                            log::error!("WebSocket stream closed unexpectedly");
                            break;
                        }
                        Err(_) => {
                            // No messages for PING_INTERVAL amount of time
                            self.ws_send(Message::Ping(vec![])).await;
                        }
                    }
                }
                _ = self.timeout.as_mut() => {
                    log::error!("Websocket timed out");
                    break;
                }
                Some((namespace, event)) = self.modules.poll_ext_events() => {
                    self.module_event(namespace, event)
                        .await
                        .expect("Should not get events from unknown modules");
                }
            }
        }

        log::info!("Stopping ws-runner task");

        self.modules.destroy().await;
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

        let value = match value {
            Ok(value) => value,
            Err(e) => {
                log::error!("Failed to parse namespaced message, {}", e);

                self.ws_send(Message::Text(error("invalid json message")))
                    .await;

                return;
            }
        };

        if let Err(NoSuchModuleError(())) = self
            .module_event(value.namespace, DynEvent::WsMessage(value.payload))
            .await
        {
            self.ws_send(Message::Text(error("unknown namespace")))
                .await;
        }
    }

    async fn ws_send(&mut self, message: Message) {
        if let Err(e) = self.websocket.send(message).await {
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

        let ctx = DynEventCtx {
            ws_messages: &mut ws_messages,
            local_state: &mut self.local_state,
        };

        self.modules.on_event(ctx, module, dyn_event).await?;

        for ws_message in ws_messages {
            self.ws_send(ws_message).await;
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

#[derive(Deserialize, Serialize)]
pub struct Namespaced<'n, O> {
    pub namespace: &'n str,
    pub payload: O,
}

impl<'n, O> Namespaced<'n, O>
where
    O: Serialize,
{
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("Failed to convert namespaced to json")
    }
}
