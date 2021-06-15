use std::{
    collections::HashMap,
    convert::TryInto,
    sync::{Arc, Weak},
    time::Duration,
};

use futures::StreamExt;
use lapin::{
    options::{BasicAckOptions, BasicNackOptions},
    Consumer,
};
use parking_lot::Mutex;
use tokio::{
    sync::{broadcast, mpsc, oneshot},
    time::timeout,
};

use crate::{
    async_types::{
        AttachToPluginRequest, CreateSessionRequest, SendToPluginRequest, SendTrickleRequest,
    },
    error, incoming,
    outgoing::{AttachToPlugin, CreateSession, KeepAlive, PluginMessage},
    rabbitmq::{RabbitMqConfig, RabbitMqConnection},
    types::{
        incoming::JanusMessage,
        outgoing::PluginBody,
        outgoing::{JanusRequest, TrickleMessage},
        JanusPlugin, Jsep, TransactionId,
    },
    HandleId, PluginRequest, SessionId,
};

/// Stores all ongoing transactions by ID.
///
/// To send messages to the recipient it uses a oneshot sender and a flag which tells it if ACK
/// messages should be sent or ignored (true = ignored)
type TransactionMap = HashMap<TransactionId, Transaction>;

#[derive(Debug)]
struct Transaction {
    ignore_ack: bool,
    channel: oneshot::Sender<JanusMessage>,
}

#[derive(Debug)]
pub(crate) struct InnerClient {
    connection_config: RabbitMqConfig,
    /// Used to map Outgoing request to incoming messages from Janus
    // todo make the ignore_ack flag typed
    transactions: Arc<Mutex<TransactionMap>>,
    connection: Option<RabbitMqConnection>,
    /// Sink for general messages from Janus that are not part of a request, such as notifications, etc.
    server_notification_sink: mpsc::Sender<Arc<JanusMessage>>,
    pub(crate) sessions: Arc<Mutex<HashMap<SessionId, Weak<InnerSession>>>>,
    /// shutdown signal
    shutdown: broadcast::Sender<()>,
}

impl InnerClient {
    /// Returns a new InnerClient
    ///
    /// At this time the InnerClient is not connected to the janus api websocket
    /// To connect call [connect](InnerClient::Connect)
    pub(crate) fn new(
        config: RabbitMqConfig,
        server_notification_sink: mpsc::Sender<Arc<JanusMessage>>,
        shutdown: broadcast::Sender<()>,
    ) -> Self {
        Self {
            connection_config: config,
            transactions: Default::default(),
            connection: None,
            server_notification_sink,
            sessions: Default::default(),
            shutdown,
        }
    }

    // todo Might be needed to reconnect, thus this is split out from [`new()`](Self::new())
    pub(crate) async fn connect(&mut self) -> Result<(), error::Error> {
        let connection = self.connection_config.clone().setup().await?;
        let consumer = connection.consumer.clone();
        self.connection = Some(connection);

        tokio::spawn(rabbitmq_event_handling_loop(
            self.shutdown.subscribe(),
            consumer,
            self.transactions.clone(),
            self.sessions.clone(),
            self.server_notification_sink.clone(),
        ));

        Ok(())
    }

    fn get_tx_id() -> TransactionId {
        let random: u64 = rand::random();
        TransactionId::new(random.to_string())
    }

    async fn send(&self, msg: &JanusRequest) -> Result<(), error::Error> {
        log::trace!(
            "Sending message containing: {}",
            serde_json::to_string(msg).unwrap()
        );
        if let Some(connection) = &self.connection {
            connection.send(serde_json::to_string(msg).unwrap()).await?;
            Ok(())
        } else {
            Err(error::Error::NotConnected)
        }
    }

    /// Sends the request to create a session
    ///
    /// Returns an error when the request could not be sent.
    /// Otherwise it will return a `CreateSessionRequest` future that will
    /// resolve once Janus sent the response to this request
    pub(crate) async fn request_create_session(
        &self,
    ) -> Result<CreateSessionRequest, error::Error> {
        let tx_id = Self::get_tx_id();
        let (tx, rx) = oneshot::channel();
        self.transactions.lock().insert(
            tx_id.clone(),
            Transaction {
                ignore_ack: true,
                channel: tx,
            },
        );
        self.send(&JanusRequest::CreateSession(CreateSession {
            transaction: tx_id,
        }))
        .await?;
        Ok(CreateSessionRequest { rx })
    }

    /// Sends the request to create a session
    ///
    /// Returns an error when the request could not be sent.
    /// Otherwise it will return a `CreateSessionRequest` future that will
    /// resolve once Janus sent the response to this request
    pub(crate) async fn request_attach_to_plugin(
        &self,
        session: SessionId,
        plugin: JanusPlugin,
    ) -> Result<AttachToPluginRequest, error::Error> {
        let tx_id = Self::get_tx_id();
        let (tx, rx) = oneshot::channel();
        self.transactions.lock().insert(
            tx_id.clone(),
            Transaction {
                ignore_ack: true,
                channel: tx,
            },
        );
        self.send(&JanusRequest::AttachToPlugin(AttachToPlugin {
            transaction: tx_id,
            plugin,
            session_id: session,
        }))
        .await?;

        Ok(AttachToPluginRequest { rx })
    }

    /// Sends the request to talk to a plugin
    ///
    /// Returns an error when the request could not be sent.
    /// Otherwise it will return a `SendToPluginRequest` future that will
    /// resolve once Janus sent the response to this request
    pub(crate) async fn send_to_plugin(
        &self,
        session: SessionId,
        handle: HandleId,
        data: PluginBody,
        ignore_ack: bool,
        jsep: Option<Jsep>,
    ) -> Result<SendToPluginRequest, error::Error> {
        let tx_id = Self::get_tx_id();
        let (tx, rx) = oneshot::channel();
        self.transactions.lock().insert(
            tx_id.clone(),
            Transaction {
                ignore_ack,
                channel: tx,
            },
        );
        self.send(&JanusRequest::PluginMessage(PluginMessage {
            transaction: tx_id,
            session_id: session,
            handle_id: handle,
            body: data,
            jsep,
        }))
        .await?;

        Ok(SendToPluginRequest { rx })
    }

    /// Sends a trickle request to Janus
    pub(crate) async fn send_trickle(
        &self,
        session: SessionId,
        handle: HandleId,
        trickle: TrickleMessage,
    ) -> Result<SendTrickleRequest, error::Error> {
        let tx_id = Self::get_tx_id();
        let (tx, rx) = oneshot::channel();
        self.transactions.lock().insert(
            tx_id.clone(),
            Transaction {
                ignore_ack: false,
                channel: tx,
            },
        );
        self.send(&JanusRequest::TrickleMessage {
            transaction: tx_id,
            session_id: session,
            handle_id: handle,
            trickle,
        })
        .await?;

        Ok(SendTrickleRequest { rx })
    }

    /// Sends a keepalive packet for the given session
    pub(crate) async fn send_keep_alive(&self, session_id: SessionId) -> Result<(), error::Error> {
        let tx_id = Self::get_tx_id();
        let (tx, rx) = oneshot::channel();
        self.transactions.lock().insert(
            tx_id.clone(),
            Transaction {
                ignore_ack: false,
                channel: tx,
            },
        );
        self.send(&JanusRequest::KeepAlive(KeepAlive {
            session_id,
            transaction: tx_id,
        }))
        .await?;
        // todo figure out if 50ms is good.
        let _ = timeout(Duration::from_millis(50), rx)
            .await
            .map_err(|_| error::Error::Timeout)?;
        Ok(())
    }
}

#[derive(Debug)]
pub(crate) struct InnerSession {
    pub(crate) client: Weak<InnerClient>,
    pub(crate) id: SessionId,
    pub(crate) handles: Mutex<HashMap<HandleId, Arc<InnerHandle>>>,
    destroyed: bool,
}

impl InnerSession {
    /// Creates a new [`Session](Session)
    pub(crate) fn new(client: Weak<InnerClient>, id: SessionId) -> Self {
        Self {
            client,
            id,
            handles: Mutex::new(HashMap::new()),
            destroyed: false,
        }
    }

    /// Returns the [`Handle`](Handle) with the given `HandleId`
    pub(crate) fn find_handle(&self, id: &HandleId) -> Result<Arc<InnerHandle>, error::Error> {
        self.handles
            .lock()
            .get(id)
            .cloned()
            .ok_or(error::Error::InvalidSession)
    }

    /// Attaches to the given plugin
    ///
    /// Returns a [`Handle`](Handle) or [`Error`](error::Error) if something went wrong
    pub(crate) async fn attach_to_plugin(
        &self,
        plugin: JanusPlugin,
    ) -> Result<Arc<InnerHandle>, error::Error> {
        let client = self
            .client
            .upgrade()
            .expect("Failed Weak::upgrade. Expected the client reference to be still valid");

        let handle_id = client
            .request_attach_to_plugin(self.id, plugin)
            .await?
            .await
            .ok_or(error::Error::FailedToAttachToPlugin)?;

        let handle = Arc::new(InnerHandle::new(
            self.client.clone(),
            self.id,
            handle_id,
            plugin,
        ));
        self.handles.lock().insert(handle_id, handle.clone());
        Ok(handle)
    }
    /// Sends keepalive for this Session
    pub(crate) async fn keep_alive(&self) -> Result<(), error::Error> {
        let client = self
            .client
            .upgrade()
            .expect("Failed Weak::upgrade. Expected the client reference to be still valid");

        client.send_keep_alive(self.id).await
    }

    pub(crate) fn detach_handle(&self, id: &HandleId) {
        self.handles.lock().remove(id);
    }

    pub(crate) async fn destroy(&mut self, client: Arc<InnerClient>) -> Result<(), error::Error> {
        let request = JanusRequest::Destroy {
            session_id: self.id,
        };

        client
            .send(&request)
            .await
            .expect("Failed to send destroy for session on drop");

        self.destroyed = true;

        Ok(())
    }
}

impl Drop for InnerSession {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            debug_assert!(self.destroyed, "call Session::destroy before dropping it");
        }
    }
}

#[derive(Debug)]
pub(crate) struct InnerHandle {
    pub(crate) client: Weak<InnerClient>,
    pub(crate) session_id: SessionId,
    pub(crate) id: HandleId,
    pub(crate) plugin_type: JanusPlugin,
    pub(crate) sink: broadcast::Sender<Arc<JanusMessage>>,
}

impl InnerHandle {
    fn new(
        client: Weak<InnerClient>,
        session_id: SessionId,
        id: HandleId,
        plugin_type: JanusPlugin,
    ) -> Self {
        let (sink, _) = broadcast::channel::<Arc<JanusMessage>>(10);
        Self {
            client,
            session_id,
            id,
            plugin_type,
            sink,
        }
    }

    /// Subscribe to messages that are not the response to a sent request
    pub(crate) fn subscribe(&self) -> broadcast::Receiver<Arc<JanusMessage>> {
        self.sink.subscribe()
    }

    /// Sends data to the attached plugin
    ///
    /// Checks if the returned result is of the correct type.
    pub(crate) async fn send<R: PluginRequest>(
        &self,
        request: R,
    ) -> Result<(R::PluginResponse, Option<Jsep>), error::Error> {
        let client = self
            .client
            .upgrade()
            .expect("Failed Weak::upgrade. Expected the client reference to be still valid");

        let (plugin_data, jsep) = client
            .send_to_plugin(
                self.session_id,
                self.id,
                request.into(),
                R::IGNORE_ACK,
                None,
            )
            .await?
            .await?
            .try_into()?;

        log::trace!("Send got this response: {:?}, {:?}", plugin_data, jsep);

        let response = plugin_data
            .try_into()
            .map_err(|_| error::Error::InvalidResponse)?;
        Ok((response, jsep))
    }

    /// Sends data to the attached plugin together with jsep
    ///
    /// Checks if the returned result is of the correct type.
    pub(crate) async fn send_with_jsep<R: PluginRequest>(
        &self,
        request: R,
        jsep: Jsep,
    ) -> Result<(R::PluginResponse, Option<Jsep>), error::Error> {
        let client = self
            .client
            .upgrade()
            .expect("Failed Weak::upgrade. Expected the client reference to be still valid");

        let (plugin_data, jsep) = client
            .send_to_plugin(
                self.session_id,
                self.id,
                request.into(),
                R::IGNORE_ACK,
                Some(jsep),
            )
            .await?
            .await?
            .try_into()?;

        log::trace!(
            "SendWithJsep got this response: {:?}, {:?}",
            plugin_data,
            jsep
        );

        let response = plugin_data
            .try_into()
            .map_err(|_| error::Error::InvalidResponse)?;
        Ok((response, jsep))
    }

    /// Sends the candidate SDP string to Janus
    // Todo
    pub(crate) async fn trickle(&self, msg: TrickleMessage) -> Result<(), error::Error> {
        let client = self
            .client
            .upgrade()
            .expect("Failed Weak::upgrade. Expected the client reference to be still valid");

        let _response = client
            .send_trickle(self.session_id, self.id, msg)
            .await?
            .await?;

        Ok(())
    }
}

impl Drop for InnerHandle {
    fn drop(&mut self) {
        let rt = if let Ok(rt) = tokio::runtime::Handle::try_current() {
            rt
        } else {
            log::error!(
                "Could not clean up resources for Handle({}) (runtime stopped)",
                self.id
            );
            return;
        };

        let client = self
            .client
            .upgrade()
            .expect("Failed Weak::upgrade. Expected the client reference to be still valid");

        // Create request outside of task to avoid move
        let request = JanusRequest::Detach {
            session_id: self.session_id,
            handle_id: self.id,
        };
        let handle_id = self.id;
        rt.spawn(async move {
            let _response = client
                .send(&request)
                .await
                .expect("Failed to send detach for handle on drop");
            log::trace!("Dropped InnerHandle for handle {}", handle_id);
        });
    }
}

async fn rabbitmq_event_handling_loop(
    mut shutdown_sig: broadcast::Receiver<()>,
    mut stream: Consumer,
    transactions: Arc<Mutex<TransactionMap>>,
    sessions: Arc<Mutex<HashMap<SessionId, Weak<InnerSession>>>>,
    sink: mpsc::Sender<Arc<JanusMessage>>,
) {
    loop {
        tokio::select! {
            _ = shutdown_sig.recv() => {
                log::debug!("RabbitMQ event handling loop got shutdown signal, exiting task");
                return;
            }
            // TODO handle none (disconnect)
            Some(consumer_result) = stream.next() => {
                match consumer_result {
                    Err(e) => {
                        log::error!("Encountered error while receiving from RabbitMQ: {}", e);
                    }
                    Ok((_, delivery)) => {
                        let msg = String::from_utf8(delivery.data.clone());
                        match msg {
                            Ok(msg) => {
                                log::trace!("Received RabbitMQ message containing: {}", msg);
                                match event_handling_loop_inner(
                                    msg,
                                    transactions.clone(),
                                    sessions.clone(),
                                    &sink,
                                )
                                .await
                                {
                                    Ok(_) => delivery
                                        .ack(BasicAckOptions::default())
                                        .await
                                        // Todo Handle Reconnects:
                                        // return error, bubble up and reconnect
                                        .expect("Could not send ACK. Check connection!"),
                                    Err(e) => {
                                        log::error!(
                                            "Error handling the incoming msg from Rabbit MQ: {}",
                                            e
                                        );
                                        delivery
                                            .nack(BasicNackOptions::default())
                                            .await
                                            // Todo Handle Reconnects:
                                            // return error, bubble up and reconnect
                                            .expect("Could not send NACK. Check connection!")
                                    }
                                }
                            }
                            Err(e) => {
                                log::error!("Received RabbitMQ message with invalid UTF-8: {}", e);
                                delivery
                                    .nack(BasicNackOptions::default())
                                    .await
                                    // Todo Handle Reconnects:
                                    // return error, bubble up and reconnect
                                    .expect("Could not send NACK. Check connection!")
                            }
                        }
                    }
                }
            }
        }
    }
    // Todo reconnect when this connection closed?
    // Todo clear the transaction table on a disconnect
}

async fn event_handling_loop_inner(
    msg: String,
    transactions: Arc<Mutex<TransactionMap>>,
    sessions: Arc<Mutex<HashMap<SessionId, Weak<InnerSession>>>>,
    sink: &mpsc::Sender<Arc<JanusMessage>>,
) -> Result<(), error::Error> {
    match serde_json::from_str::<JanusMessage>(&msg) {
        Ok(janus_result) => {
            match janus_result.transaction_id() {
                Some(tx_id) => {
                    let transaction_sink = transactions.lock().remove(tx_id);
                    match transaction_sink {
                        Some(Transaction {
                            ignore_ack: true,
                            channel,
                        }) => {
                            if matches!(janus_result, JanusMessage::Ack { .. }) {
                                transactions.lock().insert(
                                    tx_id.clone(),
                                    Transaction {
                                        ignore_ack: true,
                                        channel,
                                    },
                                );
                            } else if channel.send(janus_result).is_err() {
                                log::error!("Failed to inform the waiting future of the transaction response");
                            }
                        }
                        Some(Transaction {
                            ignore_ack: false,
                            channel,
                        }) => {
                            if channel.send(janus_result).is_err() {
                                log::error!("Failed to inform the waiting future of the transaction response");
                            }
                        }
                        None => {
                            // We could not find a transaction_id in our hashmap, try to route it based on the sessionId
                            route_message(&sessions, &sink, Arc::new(janus_result)).await;
                        }
                    };
                }
                None => {
                    // This is a transactionless msg. Try route it based on the sessionId
                    route_message(&sessions, &sink, Arc::new(janus_result)).await;
                }
            }
            Ok(())
        }
        Err(e) => {
            log::error!("Got invalid JSON from Janus: {} : {:?}", e, msg);
            Err(e.into())
        }
    }
}

/// Routes a message that does not have a matching transaction
// todo can we get this somehow a little bit cleaner?
async fn route_message(
    sessions: &Arc<Mutex<HashMap<SessionId, Weak<InnerSession>>>>,
    global_sink: &mpsc::Sender<Arc<JanusMessage>>,
    janus_result: Arc<JanusMessage>,
) {
    // Route event messages
    match janus_result.as_ref() {
        JanusMessage::Event(incoming::Event {
            sender, session_id, ..
        })
        | JanusMessage::Trickle(incoming::TrickleMessage {
            sender, session_id, ..
        })
        | JanusMessage::WebRtcUpdate(incoming::WebRtcUpdate { sender, session_id }) => {
            let session = sessions.lock().get(&session_id).cloned();
            if let Some(session) = session.and_then(|x| x.upgrade()) {
                let handle = session.find_handle(&sender);
                match handle {
                    Ok(handle) => {
                        if let Err(e) = handle.sink.send(janus_result.clone()) {
                            log::error!(
                                "Encountered error while sending Janus Event to handle channel: {}",
                                e
                            );
                        }
                        // Early return
                        return;
                    }
                    Err(e) => {
                        log::trace!(
                            "Could not get handle for incoming message: {:?} caused by {}",
                            &janus_result,
                            e
                        );
                    }
                }
            }
        }
        JanusMessage::Hangup(incoming::Hangup {
            sender,
            session_id,
            reason,
            ..
        }) => {
            let session = sessions.lock().get(&session_id).cloned();
            if let Some(session) = session.and_then(|x| x.upgrade()) {
                let handle = session.find_handle(&sender);
                match handle {
                    Ok(handle) => {
                        if let Err(e) = handle.sink.send(janus_result.clone()) {
                            log::error!(
                                "Encountered error while sending Janus Event to handle channel: {}",
                                e
                            );
                        }
                        // Early return
                        return;
                    }
                    Err(e) => {
                        if reason == "Close PC" {
                            log::trace!(
                                "Received hangup for PC after we : {:?} caused by {}",
                                &janus_result,
                                e
                            );
                        } else {
                            log::error!(
                                "Could not get handle for incoming message: {:?} caused by {}",
                                &janus_result,
                                e
                            );
                        }
                    }
                }
            }
        }
        _ => {
            // Everything that is not handled before is forwarded to the general channel
            if let Err(e) = global_sink.send(janus_result.clone()).await {
                log::error!(
                    "Failed to send JanusResult to general channel: {} - {:?}",
                    e,
                    janus_result
                );
            }
        }
    }
}
