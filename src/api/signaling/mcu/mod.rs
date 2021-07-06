use crate::settings;
use anyhow::{bail, Context, Result};
use janus_client::types::{SdpAnswer, SdpOffer};
use janus_client::{ClientId, JanusMessage, JsepType, RoomId as JanusRoomId, TrickleCandidate};
use redis::aio::ConnectionManager;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use std::borrow::{Borrow, Cow};
use std::collections::HashSet;
use std::convert::TryInto;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, mpsc, oneshot, RwLock, RwLockReadGuard};
use tokio::time::interval;
use tokio_stream::wrappers::errors::BroadcastStreamRecvError;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

mod types;

pub use types::*;

/// Redis key of the publisher => McuId/JanusRoomId mapping
///
/// This information is used when creating a subscriber
const PUBLISHER_INFO: &str = "k3k-signaling:mcu:publishers";

/// Redis key for a sorted set of mcu-clients.
///
/// The score represents the amounts of subscribers on that mcu and is used to choose the least
/// busy mcu for a new publisher.
const MCU_STATE: &str = "k3k-signaling:mcu:load";

#[derive(Debug, Serialize, Deserialize)]
struct PublisherInfo<'i> {
    room_id: JanusRoomId,
    mcu_id: Cow<'i, str>,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct McuID(Arc<String>);

impl Borrow<String> for McuID {
    fn borrow(&self) -> &String {
        self.0.borrow()
    }
}

/// Pool of one or more configured `McuClient`s
///
/// Distributes new publishers to a available Mcu with the least amount of subscribers
pub struct McuPool {
    config: settings::JanusMcuConfig,

    // Clients shared with the global receive task which sends keep-alive messages
    // and removes clients of vanished  janus instances
    clients: Arc<RwLock<HashSet<McuClient>>>,

    // Sender passed to created janus clients. Needed here to pass them to janus-clients
    // which are being reconnected
    events_sender: mpsc::Sender<(ClientId, Arc<JanusMessage>)>,

    rabbitmq_channel: lapin::Channel,
    redis: ConnectionManager,

    // Mcu shutdown signal to all janus-client tasks.
    // This is separate from the global application shutdown signal since
    // janus-client tasks are needed to gracefully detach/destroy all resources
    shutdown: broadcast::Sender<()>,
}

impl McuPool {
    pub async fn build(
        config: settings::JanusMcuConfig,
        rabbitmq_channel: lapin::Channel,
        mut redis: ConnectionManager,
    ) -> Result<Self> {
        let (shutdown, shutdown_sig) = broadcast::channel(1);

        let mut clients = HashSet::with_capacity(config.connections.len());
        let (events_sender, events) = mpsc::channel(12);

        for connection in config.connections.clone() {
            let client =
                McuClient::connect(rabbitmq_channel.clone(), connection, events_sender.clone())
                    .await
                    .context("Failed to create mcu client")?;

            redis
                .zincr(MCU_STATE, client.id.0.as_str(), 0)
                .await
                .context("Failed to initialize subscriber count")?;

            clients.insert(client);
        }

        let clients = Arc::new(RwLock::new(clients));

        tokio::spawn(global_receive_task(clients.clone(), events, shutdown_sig));

        Ok(Self {
            config,
            clients,
            events_sender,
            rabbitmq_channel,
            redis,
            shutdown,
        })
    }

    pub async fn destroy(&mut self) {
        let _ = self.shutdown.send(());

        let mut clients = self.clients.write().await;

        for client in clients.drain() {
            client.destroy().await;
        }
    }

    pub async fn try_reconnect(&self) {
        let mut clients = self.clients.write().await;

        if clients.len() == self.config.connections.len() {
            log::info!("Nothing to do");
            return;
        }

        for config in &self.config.connections {
            if clients.contains(config.to_janus_routing_key.as_str()) {
                continue;
            }

            match McuClient::connect(
                self.rabbitmq_channel.clone(),
                config.clone(),
                self.events_sender.clone(),
            )
            .await
            {
                Ok(client) => {
                    log::info!("Reconnected mcu {:?}", config.to_janus_routing_key);

                    clients.insert(client);
                }
                Err(e) => {
                    log::error!(
                        "Failed to reconnect to {:?}, {}",
                        config.to_janus_routing_key,
                        e
                    )
                }
            }
        }
    }

    async fn choose_client<'guard>(
        &self,
        redis: &mut ConnectionManager,
        clients: &'guard RwLockReadGuard<'guard, HashSet<McuClient>>,
    ) -> Result<&'guard McuClient> {
        // Get all mcu's in order lowest to highest
        let ids: Vec<String> = redis.zrangebyscore(MCU_STATE, "-inf", "+inf").await?;

        // choose the first available mcu
        for id in ids {
            if let Some(client) = clients.get(id.as_str()) {
                return Ok(client);
            }
        }

        bail!("Failed to choose client")
    }

    pub async fn new_publisher(
        &self,
        event_sink: mpsc::Sender<(MediaSessionKey, WebRtcEvent)>,
        media_session_key: MediaSessionKey,
    ) -> Result<JanusPublisher> {
        let mut redis = self.redis.clone();

        let clients = self.clients.read().await;
        let client = self
            .choose_client(&mut redis, &clients)
            .await
            .context("Failed to choose McuClient")?;

        let (handle, room_id) = self
            .create_publisher_handle(client, media_session_key)
            .await
            .context("Failed to get or create publisher handle")?;

        let (destroy, destroy_sig) = oneshot::channel();

        let info = serde_json::to_string(&PublisherInfo {
            room_id,
            mcu_id: Cow::Borrowed(&client.id.0.as_str()),
        })
        .context("Failed to serialize publisher info")?;

        redis
            .hset(PUBLISHER_INFO, media_session_key.to_string(), info)
            .await
            .context("Failed to set publisher info")?;

        let publisher = JanusPublisher {
            handle: handle.clone(),
            room_id,
            media_session_key,
            redis,
            destroy,
        };

        tokio::spawn(JanusPublisher::run(
            media_session_key,
            handle,
            event_sink,
            client.shutdown.subscribe(),
            destroy_sig,
        ));

        Ok(publisher)
    }

    async fn create_publisher_handle(
        &self,
        client: &McuClient,
        media_session_key: MediaSessionKey,
    ) -> Result<(janus_client::Handle, JanusRoomId)> {
        let handle = client
            .session
            .attach_to_plugin(janus_client::JanusPlugin::VideoRoom)
            .await
            .context("Failed to attach session to videoroom plugin")?;

        // TODO in the original code there was a check if a room for this publisher exists, check if necessary

        let bitrate = match media_session_key.1 {
            MediaSessionType::Video => self.config.max_video_bitrate,
            MediaSessionType::Screen => self.config.max_screen_bitrate,
        };

        let request = janus_client::outgoing::VideoRoomPluginCreate {
            description: media_session_key.to_string(),
            // We publish every stream in its own Janus room.
            publishers: Some(1),
            // Do not use the video-orientation RTP extension as it breaks video
            // orientation changes in Firefox.
            videoorient_ext: Some(false),
            bitrate: Some(bitrate),
            bitrate_cap: Some(true),
            ..Default::default()
        };

        let (response, _) = handle.send(request).await?;
        let room_id = match response {
            janus_client::incoming::VideoRoomPluginDataCreated::Ok { room, .. } => room,
            janus_client::incoming::VideoRoomPluginDataCreated::Err(e) => {
                bail!("Failed to create videoroom, got error response: {}", e);
            }
        };

        log::trace!(
            "Using Janus Room {} for publisher {} with media_session_type {}",
            room_id,
            media_session_key.0,
            media_session_key.1,
        );

        let join_request = janus_client::outgoing::VideoRoomPluginJoinPublisher {
            room: room_id,
            id: Some(media_session_key.1.into()),
            display: None,
            token: None,
        };

        let (response, _) = handle.send(join_request).await?;

        match response {
            janus_client::incoming::VideoRoomPluginDataJoined::Ok { .. } => {
                log::trace!(
                    "Publisher {} joined room {} successfully",
                    media_session_key.0,
                    room_id
                );

                Ok((handle, room_id))
            }
            janus_client::incoming::VideoRoomPluginDataJoined::Err(e) => {
                bail!("Failed to join videoroom, got error response: {}", e);
            }
        }
    }

    pub async fn new_subscriber(
        &self,
        event_sink: mpsc::Sender<(MediaSessionKey, WebRtcEvent)>,
        media_session_key: MediaSessionKey,
    ) -> Result<JanusSubscriber> {
        let mut redis = self.redis.clone();

        let publisher_info_json: String = redis
            .hget(PUBLISHER_INFO, media_session_key.to_string())
            .await
            .context("Failed to get mcu id for media session key")?;

        let info: PublisherInfo = serde_json::from_str(&publisher_info_json)
            .context("Failed to deserialize publisher info")?;

        let clients = self.clients.read().await;
        let client = clients
            .get(info.mcu_id.as_ref())
            .context("Publisher stored unknown mcu id")?;

        let handle = client
            .session
            .attach_to_plugin(janus_client::JanusPlugin::VideoRoom)
            .await
            .context("Failed to attach to videoroom plugin")?;

        redis
            .zincr(MCU_STATE, info.mcu_id.as_ref(), 1)
            .await
            .context("Failed to increment subscriber count")?;

        let (destroy, destroy_sig) = oneshot::channel();

        let subscriber = JanusSubscriber {
            handle: handle.clone(),
            room_id: info.room_id,
            mcu_id: client.id.clone(),
            media_session_key,
            redis,
            destroy,
        };

        tokio::spawn(JanusSubscriber::run(
            media_session_key,
            handle,
            event_sink,
            client.shutdown.subscribe(),
            destroy_sig,
        ));

        Ok(subscriber)
    }
}

async fn global_receive_task(
    clients: Arc<RwLock<HashSet<McuClient>>>,
    mut events: mpsc::Receiver<(ClientId, Arc<JanusMessage>)>,
    mut shutdown_sig: broadcast::Receiver<()>,
) {
    let mut keep_alive_interval = interval(Duration::from_secs(10));

    loop {
        tokio::select! {
            _ = keep_alive_interval.tick() => {
                keep_alive(clients.as_ref()).await
            }
            _ = shutdown_sig.recv() => {
                log::debug!("receive/keepalive task got shutdown signal, exiting");
                return;
            }
            Some((id, msg)) = events.recv() => {
                log::warn!("Unhandled janus message mcu={:?} msg={:?}",id, msg);
                // TODO Find out what we want to with these messages
                // most of them are events which are not interesting to us
                // and others expose where we ignore responses from janus
            }
        }
    }
}

async fn keep_alive(mcu_clients: &RwLock<HashSet<McuClient>>) {
    let clients = mcu_clients.read().await;

    let mut dead = vec![];

    for client in clients.iter() {
        if let Err(e) = client.session.keep_alive().await {
            log::error!(
                "Failed to keep alive session for mcu {:?}, {}",
                client.id,
                e
            );

            dead.push(client.id.clone());
        }
    }

    if dead.is_empty() {
        return;
    }

    drop(clients);

    let mut clients = mcu_clients.write().await;

    // Destroy all dead McuClients
    for dead_client in dead {
        if let Some(client) = clients.take(dead_client.0.as_str()) {
            client.destroy().await;
        }
    }
}

struct McuClient {
    id: McuID,

    session: janus_client::Session,
    client: janus_client::Client,

    // shutdown signal specific to this client
    shutdown: broadcast::Sender<()>,
}

impl PartialEq for McuClient {
    fn eq(&self, other: &Self) -> bool {
        self.id.eq(&other.id)
    }
}

impl Eq for McuClient {}

impl Hash for McuClient {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state)
    }
}

impl Borrow<str> for McuClient {
    fn borrow(&self) -> &str {
        self.id.0.as_str()
    }
}

impl McuClient {
    pub async fn connect(
        rabbitmq_channel: lapin::Channel,
        config: settings::JanusRabbitMqConnection,
        events_sender: mpsc::Sender<(ClientId, Arc<JanusMessage>)>,
    ) -> Result<Self> {
        let (shutdown, _) = broadcast::channel(1);

        let id = McuID(Arc::new(config.to_janus_routing_key.clone()));

        let rabbit_mq_config = janus_client::RabbitMqConfig::new_from_channel(
            rabbitmq_channel.clone(),
            config.to_janus_routing_key,
            config.janus_exchange,
            config.from_janus_routing_key,
            format!("k3k-signaling-janus-{}", id.0),
        );

        let mut client = janus_client::Client::new(
            rabbit_mq_config,
            ClientId(id.0.clone()),
            events_sender.clone(),
            shutdown.clone(),
        )
        .await
        .context("Failed to create janus client")?;

        let session = match client.create_session().await {
            Ok(session) => session,
            Err(e) => {
                // destroy client to clean up rabbitmq consumer
                client.destroy().await;
                bail!("Failed to create session, {}", e);
            }
        };

        Ok(Self {
            id,
            session,
            client,
            shutdown,
        })
    }

    async fn destroy(mut self) {
        if let Err(e) = self.session.destroy(true).await {
            log::error!("Failed to destroy broken session, {}", e);
        }

        let _ = self.shutdown.send(());

        self.client.destroy().await;
    }
}

pub struct JanusPublisher {
    handle: janus_client::Handle,
    room_id: JanusRoomId,
    media_session_key: MediaSessionKey,
    redis: ConnectionManager,
    destroy: oneshot::Sender<()>,
}

impl JanusPublisher {
    pub async fn send_message(&self, request: Request) -> Result<Response> {
        match request {
            Request::SdpOffer(offer) => {
                let response: janus_client::Jsep =
                    send_offer(&self.handle, (janus_client::JsepType::Offer, offer).into())
                        .await
                        .context("Failed to send SDP offer")?
                        .into();

                log::trace!("Publisher Send received: {:?}", &response);
                Ok(Response::SdpAnswer(response))
            }
            Request::Candidate(candidate) => {
                send_candidate(&self.handle, candidate)
                    .await
                    .context("Failed to send SDP candidate")?;

                Ok(Response::None)
            }
            Request::EndOfCandidates => {
                send_end_of_candidates(&self.handle)
                    .await
                    .context("Failed to send SDP end-of-candidates")?;

                Ok(Response::None)
            }
            _ => panic!("Invalid request passed to JanusPublisher::send_message"),
        }
    }

    pub async fn destroy(mut self) -> Result<()> {
        self.handle
            .send(janus_client::types::outgoing::VideoRoomPluginDestroy {
                room: self.room_id,
                secret: None,
                permanent: None,
                token: None,
            })
            .await?;

        let _ = self.destroy.send(());

        self.handle.detach().await?;

        if let Err(e) = self
            .redis
            .hdel::<_, _, ()>(PUBLISHER_INFO, self.media_session_key.to_string())
            .await
        {
            log::error!("Failed to remove publisher info, {}", e);
        }

        Ok(())
    }

    /// Event handler for a Publisher
    ///
    /// Stops when all Senders of the handle [Receiver](tokio::sync::broadcast::Receiver) are dropped.
    async fn run(
        media_session_key: MediaSessionKey,
        handle: janus_client::Handle,
        event_sink: mpsc::Sender<(MediaSessionKey, WebRtcEvent)>,
        mut client_shutdown: broadcast::Receiver<()>,
        mut destroy_sig: oneshot::Receiver<()>,
    ) {
        let mut stream = BroadcastStream::new(handle.subscribe());

        loop {
            tokio::select! {
                _ = &mut destroy_sig => {
                    return;
                }
                _ = client_shutdown.recv() => {
                    // TODO notify over evt_sink that this publisher is broken
                }
                message = stream.next() => {
                    let message = match message {
                        Some(Ok(message)) => message,
                        Some(Err(BroadcastStreamRecvError::Lagged(n))) => {
                            log::error!("Publisher {} run task dropped {} messages", media_session_key, n);
                            continue;
                        }
                        None => return,
                    };

                    log::debug!("Publisher {} received JanusMessage: {:?}", media_session_key, &*message);

                    if let Err(e) = forward_janus_message(&*message, media_session_key, &event_sink).await {
                        log::error!("Publisher {} failed to forward JanusMessage to the Media module,- killing this publisher, {}",
                            media_session_key,
                            e);
                        return;
                    }
                }
            }
        }
    }
}

pub struct JanusSubscriber {
    handle: janus_client::Handle,
    room_id: JanusRoomId,
    mcu_id: McuID,
    media_session_key: MediaSessionKey,
    redis: ConnectionManager,
    destroy: oneshot::Sender<()>,
}

impl JanusSubscriber {
    pub async fn send_message(&self, request: Request) -> Result<Response> {
        match request {
            Request::RequestOffer => {
                let response: janus_client::Jsep =
                    self.join_room().await.context("Failed to join room")?;

                Ok(Response::SdpOffer(response))
            }
            Request::SdpAnswer(e) => {
                send_answer(&self.handle, (JsepType::Answer, e).into())
                    .await
                    .context("Failed to send SDP answer")?;

                Ok(Response::None)
            }
            Request::Candidate(candidate) => {
                send_candidate(&self.handle, candidate)
                    .await
                    .context("Failed to send SDP candidate")?;

                Ok(Response::None)
            }
            Request::EndOfCandidates => {
                send_end_of_candidates(&self.handle)
                    .await
                    .context("Failed to send SDP end-of-candidates")?;

                Ok(Response::None)
            }
            _ => panic!("Invalid request passed to JanusSubscriber::send_message"),
        }
    }

    pub async fn destroy(mut self) -> Result<()> {
        let _ = self.destroy.send(());

        self.handle.detach().await?;

        self.redis
            .zincr(MCU_STATE, self.mcu_id.0.as_str(), -1)
            .await
            .context("Failed to decrease subscriber count")?;

        Ok(())
    }

    /// Joins the room of the publisher this [JanusSubscriber](JanusSubscriber) is subscriber to
    async fn join_room(&self) -> Result<janus_client::Jsep> {
        let join_response = self
            .handle
            .send(janus_client::outgoing::VideoRoomPluginJoinSubscriber {
                room: self.room_id,
                feed: janus_client::FeedId::new(self.media_session_key.1.into()),
                private_id: None,
                close_pc: None,
                audio: None,
                video: None,
                data: None,
                offer_audio: None,
                offer_video: None,
                offer_data: None,
                substream: None,
                temporal_layer: None,
                spatial_layer: None,
                temporal: None,
            })
            .await;

        match join_response {
            Ok((janus_client::incoming::VideoRoomPluginDataAttached { .. }, Some(jsep))) => {
                Ok(jsep)
            }
            Ok((_, None)) => bail!("Got invalid response on join_room, missing jsep"),
            Err(e) => bail!("Failed to join room, {}", e),
        }
    }

    async fn run(
        media_session_key: MediaSessionKey,
        handle: janus_client::Handle,
        event_sink: mpsc::Sender<(MediaSessionKey, WebRtcEvent)>,
        mut client_shutdown: broadcast::Receiver<()>,
        mut destroy_sig: oneshot::Receiver<()>,
    ) {
        let mut stream = BroadcastStream::new(handle.subscribe());

        loop {
            tokio::select! {
                _ = &mut destroy_sig => {
                    return;
                }
                _ = client_shutdown.recv() => {
                    // TODO notify over evt_sink that this subscriber is broken
                }
                message = stream.next() => {
                    let message = match message {
                        Some(Ok(message)) => message,
                        Some(Err(BroadcastStreamRecvError::Lagged(n))) => {
                            log::error!("Subscriber {} run task dropped {} messages", media_session_key, n);
                            continue;
                        }
                        None => return,
                    };

                    log::debug!("Subscriber {} received JanusMessage: {:?}", media_session_key, &*message);

                    if let Err(e) = forward_janus_message(&*message, media_session_key, &event_sink).await {
                        log::error!("Subscriber {} failed to forward JanusMessage to the Media module, shutting down this subscriber, {}",
                            media_session_key,
                            e);
                        return;
                    }
                }
            }
        }
    }
}

impl From<janus_client::incoming::TrickleMessage> for TrickleMessage {
    fn from(value: janus_client::incoming::TrickleMessage) -> Self {
        if value.completed == Some(true) {
            Self::Completed
        } else {
            Self::Candidate(TrickleCandidate {
                sdp_m_id: value.candidate.sdp_m_id,
                sdp_m_line_index: value.candidate.sdp_m_line_index,
                candidate: value.candidate.candidate,
            })
        }
    }
}

// ==== HELPER FUNCTIONS ====

/// Forwards a janus message to the media module
///
/// Uses the provided `event_sink` to forward the janus messages to the media module.
///
/// # Errors
///
/// Returns an error if the receiving part of the `event_sink` is closed.
async fn forward_janus_message(
    message: &JanusMessage,
    media_session_key: MediaSessionKey,
    event_sink: &mpsc::Sender<(MediaSessionKey, WebRtcEvent)>,
) -> Result<()> {
    match message {
        janus_client::JanusMessage::Event(event) => {
            let janus_client::incoming::Event { plugindata, .. } = event;
            if let janus_client::PluginData::VideoRoom(plugindata) = plugindata {
                match plugindata {
                    janus_client::incoming::VideoRoomPluginData::Destroyed(_) => {
                        log::trace!(
                            "Participant {}: The room of this participant got destroyed",
                            media_session_key
                        );
                    }
                    _ => log::warn!(
                        "Invalid handle event for participant {}: {:?}",
                        media_session_key,
                        event
                    ),
                }
            }
        }
        janus_client::JanusMessage::Hangup(_) => {
            event_sink
                .send((media_session_key, WebRtcEvent::WebRtcDown))
                .await?;
            return Ok(());
        }
        janus_client::JanusMessage::Detached(_) => {
            event_sink
                .send((media_session_key, WebRtcEvent::WebRtcDown))
                .await?;
            return Ok(());
        }
        janus_client::JanusMessage::Media(event) => {
            log::debug!(
                "Participant {}: Received Media Event: {:?}",
                media_session_key,
                event
            );
        }
        janus_client::JanusMessage::WebRtcUp(_) => {
            event_sink
                .send((media_session_key, WebRtcEvent::WebRtcUp))
                .await?;
        }
        janus_client::JanusMessage::SlowLink(event) => {
            let slow_link = if event.uplink {
                WebRtcEvent::SlowLink(LinkDirection::Upstream)
            } else {
                WebRtcEvent::SlowLink(LinkDirection::Downstream)
            };

            event_sink.send((media_session_key, slow_link)).await?;
        }
        janus_client::JanusMessage::Trickle(event) => {
            event_sink
                .send((
                    media_session_key,
                    WebRtcEvent::Trickle(event.clone().into()),
                ))
                .await?;
        }
        event => {
            log::debug!(
                "Participant {} received unwelcome Event {:?}",
                media_session_key,
                event
            );
        }
    }

    Ok(())
}

async fn send_offer(handle: &janus_client::Handle, offer: SdpOffer) -> Result<SdpAnswer> {
    match handle
        .send_with_jsep(
            janus_client::outgoing::VideoRoomPluginConfigurePublisher {
                audio: Some(true),
                video: Some(true),
                data: Some(true),
                ..Default::default()
            },
            offer.into(),
        )
        .await
    {
        Ok((_, Some(answer))) => Ok(answer
            .try_into()
            .context("Failed to convert response to SdpAnswer")?),
        Ok((_, None)) => bail!("Invalid response from send_offer, missing jsep"),

        Err(e) => bail!("Failed to send sdp offer, {}", e),
    }
}

async fn send_answer(handle: &janus_client::Handle, answer: SdpAnswer) -> Result<()> {
    match handle
        .send_with_jsep(
            janus_client::outgoing::VideoRoomPluginStart {},
            answer.into(),
        )
        .await
    {
        Ok(_) => Ok(()),
        Err(e) => bail!("Failed to send sdp answer, {}", e),
    }
}

async fn send_candidate(
    handle: &janus_client::Handle,
    candidate: janus_client::TrickleCandidate,
) -> Result<()> {
    match handle
        .trickle(
            janus_client::types::outgoing::TrickleMessage::new(&[candidate])
                .context("Failed to create trickle message from candidates")?,
        )
        .await
    {
        Ok(_) => Ok(()),
        Err(e) => bail!("Failed to send sdp candidate, {}", e),
    }
}

async fn send_end_of_candidates(handle: &janus_client::Handle) -> Result<()> {
    match handle
        .trickle(janus_client::types::outgoing::TrickleMessage::end())
        .await
    {
        Ok(_) => Ok(()),
        Err(e) => bail!("Failed to send sdp end-of-candidates, {}", e),
    }
}
