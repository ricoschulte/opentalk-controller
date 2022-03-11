use crate::{
    error, incoming,
    outgoing::{AttachToPlugin, KeepAlive, PluginMessage, TransactionalRequest},
    rabbitmq::{RabbitMqConfig, RabbitMqConnection},
    types::{
        incoming::JanusMessage,
        outgoing::PluginBody,
        outgoing::{JanusRequest, TrickleMessage},
        JanusPlugin, Jsep, TransactionId,
    },
    ClientId, HandleId, PluginRequest, SessionId, Success,
};
use futures::StreamExt;
use lapin::{
    options::{BasicAckOptions, BasicNackOptions},
    Consumer,
};
use parking_lot::Mutex;
use rand::Rng;
use std::{
    collections::HashMap,
    convert::TryInto,
    sync::{Arc, Weak},
    time::Duration,
};
use tokio::sync::{broadcast, mpsc};
use tokio::time::timeout;
use tracing::Instrument;

enum TaskCmd {
    Transaction {
        id: TransactionId,
        span: tracing::Span,
        sender: mpsc::Sender<TaskMessage>,
    },
    TransactionEnd(TransactionId),
}

#[derive(Debug)]
enum TaskMessage {
    Registered,
    JanusMessage(JanusMessage),
}

pub(crate) struct Transaction {
    id: TransactionId,
    span: tracing::Span,
    messages: mpsc::Receiver<TaskMessage>,
    task_sender: mpsc::UnboundedSender<TaskCmd>,
    is_async: bool,

    // Message ordering, for async results before ack
    backlog: Option<JanusMessage>,
}

impl Transaction {
    /// Retrieves the next message from the backing rabbitmq receive task
    async fn next_message(&mut self) -> Option<JanusMessage> {
        match self.messages.recv().await? {
            TaskMessage::Registered => {
                unreachable!("Registered messages must be caught on transaction creation")
            }
            TaskMessage::JanusMessage(msg) => Some(msg),
        }
    }

    /// Receives a single ACK from self.messages.
    /// If it receives a non ACK and exclusive is false, the received msg is put into the backlog.
    /// This is needed as Janus may send the async response before the ACK to our request. ~skrrr~
    async fn do_receive_ack(&mut self, exclusive: bool) -> Result<(), error::Error> {
        loop {
            let receive_result = match timeout(Duration::from_secs(2), self.next_message()).await {
                Ok(Some(msg)) => msg.into_result(),
                Ok(None) => Err(error::Error::NotConnected),
                Err(_) => Err(error::Error::Timeout),
            };

            match receive_result? {
                JanusMessage::Ack(_) => return Ok(()),
                _ if exclusive => return Err(error::Error::InvalidResponse),
                // We received a non-Ack before receiving an ack.
                // Put that into our backlog to retrieve on a Transaction::receive call.
                msg => {
                    self.backlog = Some(msg);
                    continue;
                }
            }
        }
    }

    /// Retrieve the final response from janus which must be a single ACK
    pub async fn receive_ack(mut self) -> Result<(), error::Error> {
        assert!(
            !self.is_async,
            "Transaction type for receive_ack must be a sync request"
        );

        let span = tracing::debug_span!(parent: &self.span, "receive_ack");

        self.do_receive_ack(true).instrument(span).await
    }

    /// Receive the final response from janus.
    ///
    /// Depending on the request type the transaction will first receive an ACK and later the final
    /// response. Out of order responses (ACK after final response received) will be handled.
    pub async fn receive(self) -> Result<JanusMessage, error::Error> {
        let span = tracing::debug_span!(parent: &self.span, "receive");

        self.do_receive().instrument(span).await
    }

    async fn do_receive(mut self) -> Result<JanusMessage, error::Error> {
        let msg_timeout = if self.is_async {
            self.do_receive_ack(false).await?;
            Duration::from_secs(10)
        } else {
            Duration::from_secs(2)
        };

        if let Some(backlog) = self.backlog.take() {
            Ok(backlog)
        } else {
            match timeout(msg_timeout, self.next_message()).await {
                Ok(Some(msg)) => msg.into_result(),
                Ok(None) => Err(error::Error::NotConnected),
                Err(_) => Err(error::Error::Timeout),
            }
        }
    }
}

impl Drop for Transaction {
    fn drop(&mut self) {
        let _ = self
            .task_sender
            .send(TaskCmd::TransactionEnd(self.id.clone()));
    }
}

#[derive(Debug)]
pub(crate) struct InnerClient {
    id: ClientId,

    task_sender: mpsc::UnboundedSender<TaskCmd>,
    connection: RabbitMqConnection,
    pub(crate) sessions: Arc<Mutex<HashMap<SessionId, Weak<InnerSession>>>>,
}

impl InnerClient {
    /// Constructs a new [`InnerClient`] which is connected to the configured janus via RabbitMQ
    pub(crate) async fn new(
        config: RabbitMqConfig,
        id: ClientId,
        sink: mpsc::Sender<(ClientId, Arc<JanusMessage>)>,
    ) -> Result<Self, error::Error> {
        let (connection, consumer) = config.setup().await?;

        let (task_sender, cmd_receiver) = mpsc::unbounded_channel();

        let sessions: Arc<Mutex<HashMap<SessionId, Weak<InnerSession>>>> = Default::default();

        tokio::spawn(rabbitmq_event_handling_loop(
            id.clone(),
            consumer,
            cmd_receiver,
            sessions.clone(),
            sink,
        ));

        Ok(Self {
            id,
            task_sender,
            connection,
            sessions,
        })
    }

    pub(crate) async fn destroy(&self) {
        self.connection.destroy().await;
    }

    pub(crate) async fn create_transaction(
        &self,
        request: JanusRequest,
        is_async: bool,
    ) -> Result<Transaction, error::Error> {
        let random_string = rand::thread_rng()
            .sample_iter(rand::distributions::Alphanumeric)
            .take(10)
            .map(char::from)
            .collect::<String>();

        let id = TransactionId::new(format!("{}-{}", self.id.0, random_string));

        let (sender, mut messages) = mpsc::channel(3);

        let span = tracing::debug_span!("transaction", id = %id);

        if self
            .task_sender
            .send(TaskCmd::Transaction {
                id: id.clone(),
                span: span.clone(),
                sender,
            })
            .is_err()
        {
            return Err(error::Error::NotConnected);
        };

        let msg = TransactionalRequest {
            transaction: id.clone(),
            request,
        };

        self.send(&msg).instrument(span.clone()).await?;

        if let Some(TaskMessage::Registered) = messages.recv().await {
            Ok(Transaction {
                id,
                span,
                messages,
                task_sender: self.task_sender.clone(),
                is_async,
                backlog: None,
            })
        } else {
            log::error!("Failed to receive 'registered' task-message on new transaction");
            Err(error::Error::NotConnected)
        }
    }

    #[tracing::instrument(level = "trace", skip(self, msg))]
    async fn send(&self, msg: &TransactionalRequest) -> Result<(), error::Error> {
        let json = serde_json::to_string_pretty(msg).unwrap();

        log::trace!("Sending message containing: {}", json);

        self.connection.send(json).await?;

        Ok(())
    }

    /// Sends the request to create a session
    ///
    /// Returns an error when the request could not be sent.
    /// Otherwise it will return a `CreateSessionRequest` future that will
    /// resolve once Janus sent the response to this request
    #[tracing::instrument(level = "debug", skip(self))]
    pub(crate) async fn create_session(&self) -> Result<SessionId, error::Error> {
        let transaction = self
            .create_transaction(JanusRequest::CreateSession, false)
            .await?;

        match transaction.receive().await? {
            JanusMessage::Success(Success::Janus(incoming::JanusSuccess {
                data: Some(data),
                ..
            })) => {
                // Ok we got session!
                Ok(SessionId::from(data.id))
            }
            _ => Err(error::Error::InvalidResponse),
        }
    }

    /// Attaches a session to plugin, returning a `HandleId`
    pub(crate) async fn attach_to_plugin(
        &self,
        session: SessionId,
        plugin: JanusPlugin,
    ) -> Result<HandleId, error::Error> {
        let transaction = self
            .create_transaction(
                JanusRequest::AttachToPlugin(AttachToPlugin {
                    plugin,
                    session_id: session,
                }),
                false,
            )
            .await?;

        match transaction.receive().await? {
            JanusMessage::Success(Success::Janus(incoming::JanusSuccess {
                data: Some(data),
                ..
            })) => {
                // Ok we got handle!
                Ok(HandleId::from(data.id))
            }
            _ => Err(error::Error::InvalidResponse),
        }
    }

    /// Sends a plugin request, retuning the response
    pub(crate) async fn send_to_plugin(
        &self,
        session: SessionId,
        handle: HandleId,
        data: PluginBody,
        is_async: bool,
        jsep: Option<Jsep>,
    ) -> Result<JanusMessage, error::Error> {
        let transaction = self
            .create_transaction(
                JanusRequest::PluginMessage(PluginMessage {
                    session_id: session,
                    handle_id: handle,
                    body: data,
                    jsep,
                }),
                is_async,
            )
            .await?;

        transaction.receive().await
    }

    /// Sends a trickle request to Janus
    pub(crate) async fn send_trickle(
        &self,
        session: SessionId,
        handle: HandleId,
        trickle: TrickleMessage,
    ) -> Result<(), error::Error> {
        let transaction = self
            .create_transaction(
                JanusRequest::TrickleMessage {
                    session_id: session,
                    handle_id: handle,
                    trickle,
                },
                false,
            )
            .await?;

        transaction.receive_ack().await
    }

    /// Sends a keepalive packet for the given session
    pub(crate) async fn send_keep_alive(&self, session_id: SessionId) -> Result<(), error::Error> {
        let transaction = self
            .create_transaction(JanusRequest::KeepAlive(KeepAlive { session_id }), false)
            .await?;

        transaction.receive_ack().await
    }
}

#[derive(Debug)]
pub(crate) struct InnerSession {
    pub(crate) client: Weak<InnerClient>,
    pub(crate) id: SessionId,
    pub(crate) handles: Mutex<HashMap<HandleId, Weak<InnerHandle>>>,
    destroyed: bool,
}

impl InnerSession {
    /// Creates a new [`InnerSession`], called by [`InnerClient::create_session`]
    pub(crate) fn new(client: Weak<InnerClient>, id: SessionId) -> Self {
        Self {
            client,
            id,
            handles: Mutex::new(HashMap::new()),
            destroyed: false,
        }
    }

    /// Returns the [`InnerHandle`] with the given `HandleId`
    pub(crate) fn find_handle(&self, id: &HandleId) -> Result<Arc<InnerHandle>, error::Error> {
        self.handles
            .lock()
            .get(id)
            .and_then(Weak::upgrade)
            .ok_or(error::Error::InvalidSession)
    }

    /// Attaches to the given plugin
    #[tracing::instrument(
        name = "session_keep_alive",
        level = "debug",
        skip(self),
        fields(session = %self.id),
    )]
    pub(crate) async fn attach_to_plugin(
        &self,
        plugin: JanusPlugin,
    ) -> Result<Arc<InnerHandle>, error::Error> {
        let client = self
            .client
            .upgrade()
            .expect("Failed Weak::upgrade. Expected the client reference to be still valid");

        let handle_id = client.attach_to_plugin(self.id, plugin).await?;

        let handle = Arc::new(InnerHandle::new(
            self.client.clone(),
            self.id,
            handle_id,
            plugin,
        ));

        self.handles
            .lock()
            .insert(handle_id, Arc::downgrade(&handle));

        Ok(handle)
    }

    /// Sends keepalive for this Session
    #[tracing::instrument(
        name = "session_keep_alive",
        level = "debug",
        skip(self),
        fields(session = %self.id),
    )]
    pub(crate) async fn keep_alive(&self) -> Result<(), error::Error> {
        let client = self
            .client
            .upgrade()
            .expect("Failed Weak::upgrade. Expected the client reference to be still valid");

        client.send_keep_alive(self.id).await
    }

    pub(crate) fn assume_destroyed(&mut self) {
        self.destroyed = true;
    }

    pub(crate) async fn destroy(&mut self, client: Arc<InnerClient>) -> Result<(), error::Error> {
        let transaction = client
            .create_transaction(
                JanusRequest::Destroy {
                    session_id: self.id,
                },
                false,
            )
            .await?;

        transaction.receive().await?;

        self.assume_destroyed();

        Ok(())
    }
}

impl Drop for InnerSession {
    fn drop(&mut self) {
        if !self.destroyed {
            log::error!(
                "Session({:?}) has not been destroyed before dropping",
                self.id
            );
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
    detached: bool,
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
            detached: false,
        }
    }

    /// Subscribe to messages that are not the response to a sent request
    pub(crate) fn subscribe(&self) -> broadcast::Receiver<Arc<JanusMessage>> {
        self.sink.subscribe()
    }

    /// Sends data to the attached plugin
    ///
    /// Checks if the returned result is of the correct type.
    #[tracing::instrument(
        name = "handle_send",
        level = "trace",
        skip(self, request),
        fields(handle = %self.id),
    )]
    pub(crate) async fn send<R: PluginRequest>(
        &self,
        request: R,
    ) -> Result<(R::PluginResponse, Option<Jsep>), error::Error> {
        let client = self
            .client
            .upgrade()
            .expect("Failed Weak::upgrade. Expected the client reference to be still valid");

        let (plugin_data, jsep) = client
            .send_to_plugin(self.session_id, self.id, request.into(), R::IS_ASYNC, None)
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
    #[tracing::instrument(
        name = "handle_send_with_jsep",
        level = "trace",
        skip(self, request, jsep),
        fields(handle = %self.id),
    )]
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
                R::IS_ASYNC,
                Some(jsep),
            )
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
    #[tracing::instrument(
        name = "handle_trickle",
        level = "trace",
        skip(self, msg),
        fields(handle = %self.id),
    )]
    pub(crate) async fn trickle(&self, msg: TrickleMessage) -> Result<(), error::Error> {
        let client = self
            .client
            .upgrade()
            .expect("Failed Weak::upgrade. Expected the client reference to be still valid");

        client.send_trickle(self.session_id, self.id, msg).await?;

        Ok(())
    }

    pub(crate) async fn detach(&mut self, client: Arc<InnerClient>) -> Result<(), error::Error> {
        // Set detached to true before sending request to avoid panicking on well
        // behaving code even when janus or rabbitmq fail
        self.assume_detached();

        let transaction = client
            .create_transaction(
                JanusRequest::Detach {
                    session_id: self.session_id,
                    handle_id: self.id,
                },
                false,
            )
            .await?;

        match transaction.receive().await? {
            JanusMessage::Success(_) => {
                log::trace!("Detached InnerHandle for handle {}", self.id);

                Ok(())
            }
            _ => Err(error::Error::InvalidResponse),
        }
    }

    pub(crate) fn assume_detached(&mut self) {
        self.detached = true;
    }
}

impl Drop for InnerHandle {
    fn drop(&mut self) {
        if !self.detached {
            log::error!("Dropped InnerHandle({}) before detaching", self.id);
        }
    }
}

struct StoredTransaction {
    span: tracing::Span,
    sender: mpsc::Sender<TaskMessage>,
}

async fn rabbitmq_event_handling_loop(
    id: ClientId,
    mut stream: Consumer,
    mut cmd_receiver: mpsc::UnboundedReceiver<TaskCmd>,
    sessions: Arc<Mutex<HashMap<SessionId, Weak<InnerSession>>>>,
    sink: mpsc::Sender<(ClientId, Arc<JanusMessage>)>,
) {
    let mut transactions: HashMap<TransactionId, StoredTransaction> = HashMap::new();

    loop {
        tokio::select! {
            cmd = cmd_receiver.recv() => {
                match cmd {
                    Some(TaskCmd::Transaction { id, span, sender }) => {
                        if sender.send(TaskMessage::Registered).await.is_ok() {
                            transactions.insert(id, StoredTransaction { span, sender });
                        } else {
                            log::error!("Could not send registered message to transaction, receiver dropped.")
                        }
                    }
                    Some(TaskCmd::TransactionEnd(id)) => {
                        transactions.remove(&id);
                    }
                    None => {
                        log::info!("Event handling loop exiting because cmd_receiver was dropped");
                        return;
                    }
                }
            }
            // TODO handle none (disconnect)
            Some(consumer_result) = stream.next() => {
                match consumer_result {
                    Err(e) => {
                        log::error!("Encountered error while receiving from RabbitMQ: {}", e);
                    }
                    Ok(delivery) => {
                        let msg = String::from_utf8_lossy(&delivery.data);

                        let res = event_handling_loop_inner(
                            &id,
                            &msg,
                            &transactions,
                            sessions.clone(),
                            &sink,
                        )
                        .await;

                        // spawn result handling to separate task
                        tokio::spawn(async move {
                            match res {
                                Ok(_) => {
                                    if let Err(e) = delivery.ack(BasicAckOptions::default()).await {
                                        log::error!("Failed to nack rabbitmq message which could not be handled, {}", e)
                                    }
                                }
                                Err(e) => {
                                    log::error!(
                                        "Error handling the incoming msg from Rabbit MQ: {}",
                                        e
                                    );

                                    if let Err(e) = delivery.nack(BasicNackOptions::default()).await {
                                        log::error!("Failed to nack rabbitmq message which could not be handled, {}", e)
                                    }
                                }
                            }
                        });
                    }
                }
            }
        }
    }
}

async fn event_handling_loop_inner(
    id: &ClientId,
    msg: &str,
    transactions: &HashMap<TransactionId, StoredTransaction>,
    sessions: Arc<Mutex<HashMap<SessionId, Weak<InnerSession>>>>,
    sink: &mpsc::Sender<(ClientId, Arc<JanusMessage>)>,
) -> Result<(), error::Error> {
    match serde_json::from_str::<JanusMessage>(msg) {
        Ok(janus_message) => {
            match janus_message
                .transaction_id()
                .and_then(|id| transactions.get(id))
            {
                Some(tsx) => {
                    tsx.span.in_scope(|| {
                        log::trace!(
                            "Received RabbitMQ message for transaction containing: {}",
                            msg
                        );
                    });

                    if let Err(e) = tsx
                        .sender
                        .send(TaskMessage::JanusMessage(janus_message))
                        .await
                    {
                        log::error!("Failed to deliver transactional {:?}", e.0);
                    }

                    Ok(())
                }
                None => {
                    log::trace!("Received RabbitMQ message containing: {}", msg);

                    // We could not find a transaction_id in our hashmap, try to route it based on the sessionId
                    route_message(id, &sessions, sink, Arc::new(janus_message)).await;

                    Ok(())
                }
            }
        }
        Err(e) => {
            log::error!(
                "Got invalid json from rabbitmq, {} (raw message: {})",
                e,
                msg
            );
            Err(e.into())
        }
    }
}

/// Routes a message that does not have a matching transaction
// todo can we get this somehow a little bit cleaner?
async fn route_message(
    id: &ClientId,
    sessions: &Arc<Mutex<HashMap<SessionId, Weak<InnerSession>>>>,
    global_sink: &mpsc::Sender<(ClientId, Arc<JanusMessage>)>,
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
        | JanusMessage::Media(incoming::Media {
            sender, session_id, ..
        })
        | JanusMessage::SlowLink(incoming::SlowLink {
            sender, session_id, ..
        })
        | JanusMessage::WebRtcUp(incoming::WebRtcUp { sender, session_id }) => {
            if let Some(handle) = get_handle_from_sender(sessions, session_id, sender) {
                match handle {
                    Ok(handle) => {
                        if let Err(e) = handle.sink.send(janus_result.clone()) {
                            log::error!(
                                "Encountered error while sending Janus Event {:?} to handle channel: {}",
                                &janus_result,
                                e
                            );
                        }
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
        JanusMessage::Detached(incoming::Detached { sender, session_id }) => {
            if let Some(handle) = get_handle_from_sender(sessions, session_id, sender) {
                match handle {
                    Ok(handle) => {
                        if let Err(e) = handle.sink.send(janus_result.clone()) {
                            log::error!(
                                "Encountered error while sending Janus Event {:?} to handle channel: {}",
                                &janus_result,
                                e
                            );
                        }
                    }
                    Err(e) => {
                        // it's very likely that this operation will result in this error as we probably initiated the detach and therefore
                        // already removed the handle.
                        log::trace!(
                            "Received detach on closed handle, {:?}, caused by: {}",
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
            if let Some(handle) = get_handle_from_sender(sessions, session_id, sender) {
                match handle {
                    Ok(handle) => {
                        if let Err(e) = handle.sink.send(janus_result.clone()) {
                            log::error!(
                                "Encountered error while sending Janus Event {:?} to handle channel: {}",
                                &janus_result,
                                e
                            );
                        }
                    }
                    Err(e) => {
                        if reason == "Close PC" || reason == "DTLS alert" {
                            log::trace!(
                                "Received hangup on closed handle: {:?} caused by: {}",
                                &janus_result,
                                e
                            );
                        } else {
                            log::error!(
                                "Could not get handle for incoming message: {:?} caused by: {}",
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
            if let Err(e) = global_sink.send((id.clone(), janus_result.clone())).await {
                log::error!(
                    "Failed to send JanusResult to general channel: {} - {:?}",
                    e,
                    janus_result
                );
            }
        }
    }
}

/// Get the associated InnerHandle from a janus requests sender field
///
/// Returns the InnerHandle or None if the provided session could not be found
fn get_handle_from_sender(
    sessions: &Arc<Mutex<HashMap<SessionId, Weak<InnerSession>>>>,
    session_id: &SessionId,
    sender: &HandleId,
) -> Option<Result<Arc<InnerHandle>, error::Error>> {
    let session = sessions.lock().get(session_id).cloned();
    session
        .and_then(|weak| weak.upgrade())
        .map(|session| session.find_handle(sender))
}
