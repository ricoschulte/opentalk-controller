// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use actix::{Actor, ActorContext, AsyncContext, Handler, StreamHandler};
use actix_http::ws::{CloseCode, CloseReason, Item, ProtocolError};
use actix_web_actors::ws::{Message, WebsocketContext};
use bytes::BytesMut;
use bytestring::ByteString;
use std::convert::TryFrom;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::UnboundedSender;

/// Define HTTP Websocket actor
///
/// This actor will relay all text and binary received websocket messages to the given unbounded sender
/// It is up to the receiver of the channel to extract the underlying message.
///
/// Handling timeouts is also done in this actor.
pub struct WebSocketActor {
    /// Sender to signaling runner
    sender: UnboundedSender<Message>,

    /// Timestamp of last pong received
    last_pong: Instant,

    /// State for receiving fragmented messages
    continuation: Option<Continuation>,
}

struct Continuation {
    buffer: BytesMut,
    is_text: bool,
}

impl WebSocketActor {
    pub fn new(sender: UnboundedSender<Message>) -> Self {
        Self {
            sender,
            last_pong: Instant::now(),
            continuation: None,
        }
    }

    /// Forward a websocket message to the runner
    ///
    /// Closes the websocket if the channel to the runner is disconnected
    fn forward_to_runner(&mut self, ctx: &mut WebsocketContext<Self>, msg: Message) {
        if self.sender.send(msg).is_err() {
            ctx.close(Some(CloseReason {
                code: CloseCode::Abnormal,
                description: None,
            }));
        }
    }

    /// Handle continuation packages by saving them in a separate buffer
    fn handle_continuation(&mut self, ctx: &mut WebsocketContext<Self>, item: Item) {
        match item {
            Item::FirstText(bytes) => {
                if self.continuation.is_some() {
                    log::warn!("Got continuation while processing one");
                }

                self.continuation = Some(Continuation {
                    buffer: BytesMut::from(&bytes[..]),
                    is_text: true,
                });
            }
            Item::FirstBinary(bytes) => {
                if self.continuation.is_some() {
                    log::warn!("Got continuation while processing one");
                }

                self.continuation = Some(Continuation {
                    buffer: BytesMut::from(&bytes[..]),
                    is_text: false,
                });
            }
            Item::Continue(bytes) => {
                if let Some(continuation) = &mut self.continuation {
                    continuation.buffer.extend_from_slice(&bytes);

                    if continuation.buffer.len() >= 1_000_000 {
                        log::error!("Fragmented message over 1 MB, stopping actor");
                        ctx.stop();
                    }
                } else {
                    log::warn!("Got continuation continue message without a continuation set");
                }
            }
            Item::Last(bytes) => {
                if let Some(mut continuation) = self.continuation.take() {
                    continuation.buffer.extend_from_slice(&bytes);

                    if continuation.is_text {
                        match ByteString::try_from(continuation.buffer) {
                            Ok(string) => {
                                self.forward_to_runner(ctx, Message::Text(string));
                            }
                            Err(_) => {
                                log::warn!("Got text continuation item but it wasn't valid UTF8, discarding");
                            }
                        }
                    } else {
                        self.forward_to_runner(ctx, Message::Binary(continuation.buffer.freeze()));
                    }
                } else {
                    log::warn!("Got continuation last message without a continuation set");
                }
            }
        }
    }
}

impl Actor for WebSocketActor {
    type Context = WebsocketContext<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        // Start an interval for connection checks via ping-pong
        ctx.run_interval(Duration::from_secs(15), |this, ctx| {
            if Instant::now().duration_since(this.last_pong) > Duration::from_secs(20) {
                // no response to ping, exit
                ctx.stop();
            } else {
                ctx.ping(b"heartbeat");
            }
        });
    }
}

/// Handle incoming websocket messages
impl StreamHandler<Result<Message, ProtocolError>> for WebSocketActor {
    fn handle(&mut self, msg: Result<Message, ProtocolError>, ctx: &mut Self::Context) {
        match msg {
            Ok(Message::Ping(msg)) => ctx.pong(&msg),
            Ok(Message::Pong(msg)) => {
                if msg == b"heartbeat"[..] {
                    self.last_pong = Instant::now();
                }
            }
            Ok(msg @ Message::Text(_)) => {
                self.forward_to_runner(ctx, msg);
            }
            Ok(msg @ Message::Binary(_)) => {
                self.forward_to_runner(ctx, msg);
            }
            Ok(Message::Continuation(item)) => self.handle_continuation(ctx, item),
            Ok(Message::Close(_)) => {
                ctx.close(Some(CloseReason {
                    code: CloseCode::Normal,
                    description: None,
                }));
                ctx.stop();
            }
            Ok(Message::Nop) => {}
            Err(e) => {
                log::warn!("Protocol error in websocket - exiting, {}", e);

                ctx.stop();
            }
        }
    }
}

/// Command for the WebSocketActor sent by the runner
#[derive(actix::Message)]
#[rtype(result = "()")]
pub enum WsCommand {
    Ws(Message),
    Close(CloseReason),
}

/// Handle websocket messages produced by the runner, to be sent to the client
impl Handler<WsCommand> for WebSocketActor {
    type Result = ();

    fn handle(&mut self, msg: WsCommand, ctx: &mut Self::Context) -> Self::Result {
        match msg {
            WsCommand::Ws(msg) => ctx.write_raw(msg),
            WsCommand::Close(reason) => ctx.close(Some(reason)),
        }
    }
}
