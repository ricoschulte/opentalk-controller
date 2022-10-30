// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use crate::error::Error;
use futures::prelude::*;
use futures::stream::{SplitSink, SplitStream};
use std::borrow::Cow;
use std::convert::TryInto;
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::header::SEC_WEBSOCKET_PROTOCOL;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::MaybeTlsStream;

pub type WebSocketStream =
    SplitStream<tokio_tungstenite::WebSocketStream<MaybeTlsStream<TcpStream>>>;
#[derive(Debug)]
pub struct WebSocketConfig {
    pub url: String,
}

impl WebSocketConfig {
    pub fn new<S: Into<String>>(url: S) -> Self {
        Self { url: url.into() }
    }

    pub async fn setup(self) -> Result<(WebSocketSink, WebSocketStream), Error> {
        let mut req = self.url.into_client_request()?;
        req.headers_mut().insert(
            SEC_WEBSOCKET_PROTOCOL,
            "janus-protocol"
                .try_into()
                .expect("'janus-protocol' to be a valid header-value"),
        );

        let (stream, _) = tokio_tungstenite::connect_async(req).await?;

        let (sink, stream) = stream.split();

        Ok((
            WebSocketSink {
                sink: Mutex::new(sink),
            },
            stream,
        ))
    }
}

#[derive(Debug)]
pub struct WebSocketSink {
    sink: Mutex<SplitSink<tokio_tungstenite::WebSocketStream<MaybeTlsStream<TcpStream>>, Message>>,
}

impl WebSocketSink {
    pub(crate) async fn send<T: Into<String> + AsRef<[u8]>>(&self, msg: T) -> Result<(), Error> {
        self.sink
            .lock()
            .await
            .send(Message::Text(msg.into()))
            .await?;

        Ok(())
    }

    pub async fn destroy(&self) -> Result<(), Error> {
        let mut sink = self.sink.lock().await;

        sink.send(Message::Close(Some(CloseFrame {
            code: CloseCode::Away,
            reason: Cow::Borrowed("going away"),
        })))
        .await?;

        sink.close().await?;

        Ok(())
    }
}
