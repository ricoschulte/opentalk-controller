use crate::incoming::SubscriberConfiguration;
use crate::settings;
use anyhow::{bail, Context, Result};
use controller::prelude::*;
use controller::settings::SharedSettings;
use janus_client::outgoing::{
    VideoRoomPluginConfigurePublisher, VideoRoomPluginConfigureSubscriber,
};
use janus_client::types::{SdpAnswer, SdpOffer};
use janus_client::{ClientId, JanusMessage, JsepType, RoomId as JanusRoomId, TrickleCandidate};
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use std::borrow::{Borrow, Cow};
use std::collections::HashSet;
use std::convert::{TryFrom, TryInto};
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, mpsc, oneshot, RwLock, RwLockReadGuard};
use tokio::time::{interval, sleep};
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
struct McuId(Arc<str>);

impl McuId {
    pub fn new(to_janus_key: &str, janus_exchange_key: &str, from_janus_key: &str) -> Self {
        let key = Self::generate_id_string(to_janus_key, janus_exchange_key, from_janus_key);

        Self(key.into_boxed_str().into())
    }

    pub fn generate_id_string(
        to_janus_key: &str,
        janus_exchange_key: &str,
        from_janus_key: &str,
    ) -> String {
        format!("{}-{}-{}", to_janus_key, janus_exchange_key, from_janus_key)
    }
}

impl From<&settings::Connection> for McuId {
    fn from(conn: &settings::Connection) -> Self {
        Self::new(&conn.to_routing_key, &conn.exchange, &conn.from_routing_key)
    }
}

/// Pool of one or more configured `McuClient`s
///
/// Distributes new publishers to a available Mcu with the least amount of subscribers
pub struct McuPool {
    // Clients shared with the global receive task which sends keep-alive messages
    // and removes clients of vanished  janus instances
    clients: RwLock<HashSet<McuClient>>,

    shared_settings: SharedSettings,

    // Sender passed to created janus clients. Needed here to pass them to janus-clients
    // which are being reconnected
    events_sender: mpsc::Sender<(ClientId, Arc<JanusMessage>)>,

    rabbitmq: lapin::Connection,
    redis: RedisConnection,

    // Mcu shutdown signal to all janus-client tasks.
    // This is separate from the global application shutdown signal since
    // janus-client tasks are needed to gracefully detach/destroy all resources
    shutdown: broadcast::Sender<()>,
}

impl McuPool {
    pub async fn build(
        settings: &controller::settings::Settings,
        shared_settings: SharedSettings,
        rabbitmq: lapin::Connection,
        mut redis: RedisConnection,
        controller_shutdown_sig: broadcast::Receiver<()>,
        controller_reload_sig: broadcast::Receiver<()>,
    ) -> Result<Arc<Self>> {
        let mcu_config = settings::JanusMcuConfig::extract(settings)?;

        let (shutdown, _) = broadcast::channel(1);

        let mut clients = HashSet::with_capacity(mcu_config.connections.len());
        let (events_sender, events) = mpsc::channel(12);

        let connections = mcu_config.connections.clone();

        for connection in connections {
            let channel = rabbitmq.create_channel().await?;

            let client = McuClient::connect(channel, &mut redis, connection, events_sender.clone())
                .await
                .context("Failed to create mcu client")?;

            clients.insert(client);
        }

        let clients = RwLock::new(clients);

        let mcu_pool = Arc::new(Self {
            clients,
            shared_settings,
            events_sender,
            rabbitmq,
            redis,
            shutdown,
        });

        tokio::spawn(global_receive_task(
            mcu_pool.clone(),
            controller_shutdown_sig,
            controller_reload_sig,
            events,
        ));

        Ok(mcu_pool)
    }

    pub async fn destroy(&self) {
        let _ = self.shutdown.send(());

        let mut clients = self.clients.write().await;

        for client in clients.drain() {
            client.destroy(false).await;
        }
    }

    /// Reload the janus config
    ///
    /// Reads the current state of the [`SharedSettings`] and loads the configured janus
    /// configurations.
    ///
    /// This function will gracefully remove an active janus client if it happens to be missing
    /// in the new config. The Publishers & Subscribers on the removed janus will get a WebRtcDown
    /// event and should reconnect in order to use a different janus.
    pub async fn reload_janus_config(&self) -> Result<()> {
        let settings = self.shared_settings.load_full();
        let mcu_settings = settings::JanusMcuConfig::extract(&*settings)?;

        let mut clients = self.clients.write().await;

        // the new janus client list
        let updated_clients = mcu_settings
            .connections
            .iter()
            .map(McuId::from)
            .collect::<Vec<McuId>>();

        // figure out which old clients to remove
        let removed_clients = clients
            .iter()
            .filter(|client| !updated_clients.contains(&client.id))
            .map(|client| client.id.clone())
            .collect::<Vec<McuId>>();

        // gracefully shutdown all removed clients
        for removed_client in removed_clients {
            if let Some(client) = clients.take(&removed_client) {
                client.destroy(false).await;
            }
        }

        // connect to new clients
        for connection_config in &mcu_settings.connections {
            if clients.contains(&McuId::from(connection_config)) {
                continue;
            }

            let channel = self.rabbitmq.create_channel().await?;

            match McuClient::connect(
                channel,
                &mut self.redis.clone(),
                connection_config.clone(),
                self.events_sender.clone(),
            )
            .await
            {
                Ok(client) => {
                    log::info!("Connected mcu {:?}", connection_config.to_routing_key);

                    clients.insert(client);
                }
                Err(e) => {
                    log::error!(
                        "Failed to connect to {:?}, {}",
                        connection_config.to_routing_key,
                        e
                    )
                }
            }
        }

        Ok(())
    }

    async fn choose_client<'guard>(
        &self,
        redis: &mut RedisConnection,
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
            mcu_id: Cow::Borrowed(client.id.0.as_ref()),
        })
        .context("Failed to serialize publisher info")?;

        redis
            .hset(PUBLISHER_INFO, media_session_key.to_string(), info)
            .await
            .context("Failed to set publisher info")?;

        tokio::spawn(JanusPublisher::run(
            media_session_key,
            BroadcastStream::new(handle.subscribe()),
            event_sink,
            client.pubsub_shutdown.subscribe(),
            destroy_sig,
        ));

        let publisher = JanusPublisher {
            handle,
            room_id,
            media_session_key,
            redis,
            destroy,
        };

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

        let settings = settings::JanusMcuConfig::extract(&self.shared_settings.load())?;
        let bitrate = match media_session_key.1 {
            MediaSessionType::Video => settings.max_video_bitrate,
            MediaSessionType::Screen => settings.max_screen_bitrate,
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
            audiolevel_event: Some(true),
            audiolevel_ext: Some(true),
            audio_active_packets: Some(settings.speaker_focus_packets),
            audio_level_average: Some(settings.speaker_focus_level),
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
            .with_context(|| {
                format!(
                    "Failed to get mcu id for media session key {}",
                    media_session_key
                )
            })?;

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

        tokio::spawn(JanusSubscriber::run(
            media_session_key,
            BroadcastStream::new(handle.subscribe()),
            event_sink,
            client.pubsub_shutdown.subscribe(),
            destroy_sig,
        ));

        let subscriber = JanusSubscriber {
            handle: handle.clone(),
            room_id: info.room_id,
            mcu_id: client.id.clone(),
            media_session_key,
            redis,
            destroy,
        };

        Ok(subscriber)
    }
}

async fn global_receive_task(
    mcu_pool: Arc<McuPool>,
    mut controller_shutdown_sig: broadcast::Receiver<()>,
    mut controller_reload_sig: broadcast::Receiver<()>,
    mut events: mpsc::Receiver<(ClientId, Arc<JanusMessage>)>,
) {
    let mut keep_alive_interval = interval(Duration::from_secs(10));

    loop {
        tokio::select! {
            _ = keep_alive_interval.tick() => {
                keep_alive(&mcu_pool.clients).await
            }
            _ = controller_shutdown_sig.recv() => {
                log::debug!("mcu pool receive/keepalive task got controller shutdown signal, destroying pool");
                mcu_pool.destroy().await;
                return;
            }
            _ = controller_reload_sig.recv() => {
                log::debug!("mcu pool receive/keepalive task got controller reload signal");
                if let Err(e) = mcu_pool.reload_janus_config().await {
                    log::error!("mcu pool reload failed, {:?}", e);
                }
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
    for dead_client_id in dead {
        if let Some(client) = clients.take(&dead_client_id) {
            client.destroy(true).await;
        }
    }
}

#[derive(Debug, Clone)]
pub enum ShutdownSignal {
    // Shutdown signal that shuts down the Publisher/Subscriber loops
    Graceful,
    // Signals that the underlying janus client is gone and all resources should be freed immediately.
    AlreadyDisconnected,
}

struct McuClient {
    pub id: McuId,

    session: janus_client::Session,
    client: janus_client::Client,

    // shutdown signal specific to this client
    pubsub_shutdown: broadcast::Sender<ShutdownSignal>,
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

impl Borrow<McuId> for McuClient {
    fn borrow(&self) -> &McuId {
        &self.id
    }
}

impl Borrow<str> for McuClient {
    fn borrow(&self) -> &str {
        self.id_str()
    }
}

impl McuClient {
    pub fn id_str(&self) -> &str {
        self.id.0.as_ref()
    }

    #[tracing::instrument(level = "debug", skip(rabbitmq_channel, redis, events_sender))]
    pub async fn connect(
        rabbitmq_channel: lapin::Channel,
        redis: &mut RedisConnection,
        config: settings::Connection,
        events_sender: mpsc::Sender<(ClientId, Arc<JanusMessage>)>,
    ) -> Result<Self> {
        // We sent at most two signals
        let (pubsub_shutdown, _) = broadcast::channel::<ShutdownSignal>(1);

        let id = McuId::new(
            &config.to_routing_key,
            &config.exchange,
            &config.from_routing_key,
        );

        let rabbit_mq_config = janus_client::RabbitMqConfig::new_from_channel(
            rabbitmq_channel.clone(),
            config.to_routing_key,
            config.exchange,
            config.from_routing_key,
            format!("k3k-sig-janus-{}", id.0),
        );

        redis
            .zincr(MCU_STATE, id.0.as_ref(), 0)
            .await
            .context("Failed to initialize subscriber count")?;

        let mut client = janus_client::Client::new(
            rabbit_mq_config,
            ClientId(id.0.clone()),
            events_sender.clone(),
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
            pubsub_shutdown,
        })
    }

    #[tracing::instrument(level = "debug", skip(self))]
    async fn destroy(mut self, broken: bool) {
        log::trace!(
            "Destroying McuClient {:?}, waiting for associated publishers & subscriber to shutdown",
            self.id
        );

        if broken {
            let _ = self
                .pubsub_shutdown
                .send(ShutdownSignal::AlreadyDisconnected);
        } else {
            let _ = self.pubsub_shutdown.send(ShutdownSignal::Graceful);
        }

        log::trace!("receiver count {}", self.pubsub_shutdown.receiver_count());

        let mut all_receivers_dropped = false;

        // This loop will block the thread, we wait a maximum 250ms for all subscribers & publishers
        // to drop. If any subscriber or publisher takes longer, we assume they are detached and
        // proceed with destroying the McuClient. This is done to avoid blocking for 10+ seconds as
        // the subscribers and publishers may wait for a janus timeout.
        for _ in 0..10 {
            if self.pubsub_shutdown.receiver_count() == 0 {
                all_receivers_dropped = true;
                break;
            }
            sleep(Duration::from_millis(25)).await;
        }

        if !all_receivers_dropped {
            log::warn!(
                "Destroying McuClient without dropping all receivers, {} receivers are still alive",
                self.pubsub_shutdown.receiver_count()
            );
        }

        log::trace!("All subscribers & publishers shutdown");

        if let Err(e) = self.session.destroy(broken).await {
            log::error!("Failed to destroy broken session, {}", e);
        }

        self.client.destroy().await;

        log::trace!("Successfully destroyed McuClient {:?}", self.id);
    }
}

pub struct JanusPublisher {
    handle: janus_client::Handle,
    room_id: JanusRoomId,
    media_session_key: MediaSessionKey,
    redis: RedisConnection,
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
            Request::PublisherConfigure(configuration) => {
                self.configure_publisher(configuration).await?;
                Ok(Response::None)
            }
            _ => panic!("Invalid request passed to JanusPublisher::send_message"),
        }
    }

    /// Configure the publisher
    async fn configure_publisher(&self, configuration: PublishConfiguration) -> Result<()> {
        let configure_request = VideoRoomPluginConfigurePublisher::new()
            .video(Some(configuration.video))
            .audio(Some(configuration.audio));

        match self.handle.send(configure_request).await {
            Ok((configured_event, Some(jsep))) => {
                log::debug!("Configure publisher got Event: {:?}", configured_event);
                log::debug!("Configure publisher got Jsep/SDP: {:?}", jsep);
                Ok(())
            }
            Ok((configured_event, None)) => {
                log::debug!("Configure publisher got Event: {:?}", configured_event);
                Ok(())
            }
            Err(e) => bail!("Failed to configure publisher, {}", e),
        }
    }

    pub async fn destroy(mut self) -> Result<()> {
        if let Err(e) = self
            .redis
            .hdel::<_, _, ()>(PUBLISHER_INFO, self.media_session_key.to_string())
            .await
        {
            log::error!("Failed to remove publisher info, {}", e);
        }

        if let Err(e) = self
            .handle
            .send(janus_client::types::outgoing::VideoRoomPluginDestroy {
                room: self.room_id,
                secret: None,
                permanent: None,
                token: None,
            })
            .await
        {
            log::error!(
                "Failed to send VideoRoomPluginDestroy event {}, continuing to detach anyway",
                e
            );
        }

        let detach_result = self.handle.detach(false).await;

        let _ = self.destroy.send(());

        detach_result.map_err(From::from)
    }

    pub async fn destroy_broken(mut self) -> Result<()> {
        let _ = self.destroy.send(());

        self.handle.detach(true).await?;

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
        mut stream: BroadcastStream<Arc<JanusMessage>>,
        event_sink: mpsc::Sender<(MediaSessionKey, WebRtcEvent)>,
        mut client_shutdown: broadcast::Receiver<ShutdownSignal>,
        mut destroy_sig: oneshot::Receiver<()>,
    ) {
        loop {
            tokio::select! {
                _ = &mut destroy_sig => {
                    log::debug!("Publisher {} got destroy signal", media_session_key);
                    return;
                }
                shutdown_signal = client_shutdown.recv() => {
                    log::debug!("Publisher {} got client shutdown signal", media_session_key);

                    if let Ok(shutdown_signal) = shutdown_signal {
                        match shutdown_signal {
                            ShutdownSignal::Graceful => {
                                let _ = event_sink.send((media_session_key, WebRtcEvent::WebRtcDown)).await;
                            }
                            ShutdownSignal::AlreadyDisconnected => {
                                let _ = event_sink.send((media_session_key, WebRtcEvent::AssociatedMcuDied)).await;
                            }
                        }
                    } else {
                        let _ = event_sink.send((media_session_key, WebRtcEvent::AssociatedMcuDied)).await;
                    }

                    // ignore result since receiver might have been already dropped
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
    mcu_id: McuId,
    media_session_key: MediaSessionKey,
    redis: RedisConnection,
    destroy: oneshot::Sender<()>,
}

impl JanusSubscriber {
    pub async fn send_message(&self, request: Request) -> Result<Response> {
        match request {
            Request::RequestOffer { without_video } => {
                let response: janus_client::Jsep = self
                    .join_room(without_video)
                    .await
                    .context("Failed to join room")?;

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
            Request::SubscriberConfigure(configuration) => {
                self.configure_subscriber(configuration)
                    .await
                    .context("Failed to configure subscriber")?;

                Ok(Response::None)
            }
            _ => panic!("Invalid request passed to JanusSubscriber::send_message"),
        }
    }

    pub async fn destroy(mut self, broken: bool) -> Result<()> {
        let detach_result = self.handle.detach(broken).await;

        let _ = self.destroy.send(());

        self.redis
            .zincr(MCU_STATE, self.mcu_id.0.as_ref(), -1)
            .await
            .context("Failed to decrease subscriber count")?;

        detach_result.map_err(From::from)
    }

    /// Joins the room of the publisher this [JanusSubscriber](JanusSubscriber) is subscriber to
    async fn join_room(&self, without_video: bool) -> Result<janus_client::Jsep> {
        let feed = janus_client::FeedId::new(self.media_session_key.1.into());
        let join_request =
            janus_client::outgoing::VideoRoomPluginJoinSubscriber::builder(self.room_id, feed)
                .offer_video(if without_video { Some(false) } else { None })
                .build();

        let join_response = self.handle.send(join_request).await;

        match join_response {
            Ok((janus_client::incoming::VideoRoomPluginDataAttached { .. }, Some(jsep))) => {
                log::debug!("Join room got Jsep/SDP: {:?}", jsep);
                Ok(jsep)
            }
            Ok((data, None)) => bail!(
                "Got invalid response on join_room, missing jsep. Got {:?}",
                data
            ),
            Err(e) => bail!("Failed to join room, {}", e),
        }
    }

    /// Configure the subscriber
    async fn configure_subscriber(&self, configuration: SubscriberConfiguration) -> Result<()> {
        let configure_request = VideoRoomPluginConfigureSubscriber::builder()
            .substream(configuration.substream)
            .video(configuration.video)
            .build();

        match self.handle.send(configure_request).await {
            Ok((configured_event, Some(jsep))) => {
                log::debug!("Configure subscriber got Event: {:?}", configured_event);
                log::debug!("Configure subscriber got Jsep/SDP: {:?}", jsep);
                Ok(())
            }
            Ok((configured_event, None)) => {
                log::debug!("Configure subscriber got Event: {:?}", configured_event);
                Ok(())
            }
            Err(e) => bail!("Failed to configure subscriber, {}", e),
        }
    }

    async fn run(
        media_session_key: MediaSessionKey,
        mut stream: BroadcastStream<Arc<JanusMessage>>,
        event_sink: mpsc::Sender<(MediaSessionKey, WebRtcEvent)>,
        mut client_shutdown: broadcast::Receiver<ShutdownSignal>,
        mut destroy_sig: oneshot::Receiver<()>,
    ) {
        loop {
            tokio::select! {
                _ = &mut destroy_sig => {
                    log::debug!("Subscriber {} got destroy signal", media_session_key);
                    return;
                }
                shutdown_signal = client_shutdown.recv() => {
                    log::debug!("Subscriber {} got client shutdown signal", media_session_key);

                    if let Ok(shutdown_signal) = shutdown_signal {
                        match shutdown_signal {
                            ShutdownSignal::Graceful => {
                                let _ = event_sink.send((media_session_key, WebRtcEvent::WebRtcDown)).await;
                            }
                            ShutdownSignal::AlreadyDisconnected => {
                                let _ = event_sink.send((media_session_key, WebRtcEvent::AssociatedMcuDied)).await;
                            }
                        }
                    } else {
                        let _ = event_sink.send((media_session_key, WebRtcEvent::AssociatedMcuDied)).await;
                    }
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

impl TryFrom<janus_client::incoming::TrickleMessage> for TrickleMessage {
    type Error = anyhow::Error;

    fn try_from(value: janus_client::incoming::TrickleMessage) -> Result<Self> {
        match value.candidate {
            janus_client::incoming::TrickleInnerMessage::Completed { completed } => {
                if completed {
                    Ok(Self::Completed)
                } else {
                    bail!("invalid trickle message. Recieved completed == false")
                }
            }
            janus_client::incoming::TrickleInnerMessage::Candidate(candidate) => {
                Ok(Self::Candidate(TrickleCandidate {
                    sdp_m_line_index: candidate.sdp_m_line_index,
                    candidate: candidate.candidate,
                }))
            }
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
                    janus_client::incoming::VideoRoomPluginData::SlowLink(_) => {
                        log::trace!(
                            "Participant {}: Got a slow link event for its room",
                            media_session_key
                        );
                    }
                    janus_client::incoming::VideoRoomPluginData::Event(_) => {
                        log::trace!(
                            "Participant {}: Got a plugin event for its room",
                            media_session_key
                        );
                    }
                    janus_client::incoming::VideoRoomPluginData::Talking(_) => {
                        event_sink
                            .send((media_session_key, WebRtcEvent::StartedTalking))
                            .await?;
                        return Ok(());
                    }
                    janus_client::incoming::VideoRoomPluginData::StoppedTalking(_) => {
                        event_sink
                            .send((media_session_key, WebRtcEvent::StoppedTalking))
                            .await?;
                        return Ok(());
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
                    WebRtcEvent::Trickle(event.clone().try_into()?),
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
        .send_with_jsep(VideoRoomPluginConfigurePublisher::new(), offer.into())
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
