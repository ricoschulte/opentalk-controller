// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use crate::error::Error;

mod rabbitmq;
mod websocket;

pub use rabbitmq::RabbitMqConfig;
pub use websocket::{WebSocketConfig, WebSocketStream};

#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub enum TransportConfig {
    RabbitMq(RabbitMqConfig),
    WebSocket(WebSocketConfig),
}

impl From<RabbitMqConfig> for TransportConfig {
    fn from(cfg: RabbitMqConfig) -> Self {
        Self::RabbitMq(cfg)
    }
}

impl From<WebSocketConfig> for TransportConfig {
    fn from(cfg: WebSocketConfig) -> Self {
        Self::WebSocket(cfg)
    }
}

#[derive(Debug)]
pub(crate) enum Transport {
    RabbitMq(rabbitmq::RabbitMqConnection),
    WebSocket(websocket::WebSocketSink),
}

impl Transport {
    pub(crate) async fn connect_rabbitmq(
        config: RabbitMqConfig,
    ) -> Result<(Self, lapin::Consumer), Error> {
        let (sink, consumer) = config.setup().await?;

        Ok((Self::RabbitMq(sink), consumer))
    }

    pub(crate) async fn connect_websocket(
        config: WebSocketConfig,
    ) -> Result<(Self, websocket::WebSocketStream), Error> {
        let (sink, stream) = config.setup().await?;

        Ok((Self::WebSocket(sink), stream))
    }

    pub(crate) async fn send<T: Into<String> + AsRef<[u8]>>(&self, msg: T) -> Result<(), Error> {
        match self {
            Self::RabbitMq(rmq) => rmq.send(msg).await.map(|_| ()),
            Self::WebSocket(ws) => {
                ws.send(msg.into()).await?;
                Ok(())
            }
        }
    }

    pub async fn destroy(&self) {
        match self {
            Transport::RabbitMq(rmq) => rmq.destroy().await,
            Transport::WebSocket(ws) => {
                if let Err(e) = ws.destroy().await {
                    log::error!("Failed to close websocket connection, {}", e);
                }
            }
        }
    }
}
