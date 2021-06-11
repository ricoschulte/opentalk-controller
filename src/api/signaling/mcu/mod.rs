use super::ParticipantId;
use crate::settings;
use anyhow::{bail, Context, Result};
use futures::StreamExt;
use janus_client::error::JanusInternalError;
use janus_client::types::{SdpAnswer, SdpOffer};
use janus_client::{JsepType, RoomId as JanusRoomId, TrickleCandidate};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::convert::TryInto;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::sync::{broadcast, mpsc};
use tokio::time::{interval, sleep};
use tokio_stream::wrappers::errors::BroadcastStreamRecvError;
use tokio_stream::wrappers::{BroadcastStream, ReceiverStream};

mod types;

pub use types::*;

// Todo use ID of this controller.
static TAG: &str = "k3k-signaling";

/// This is a per Room/per Janus manager of Subscribers and Publishers
pub struct JanusMcu {
    max_stream_bitrate: u64,
    max_screen_bitrate: u64,

    // TODO v2 Handle disconnection and state recovery
    _gw: janus_client::Client,
    session: janus_client::Session,

    shutdown: broadcast::Sender<()>,

    /// Stores the room ids?
    publisher_room_ids: Mutex<HashMap<MediaSessionKey, JanusRoomId>>,

    next_client_id: AtomicU64,
}

impl JanusMcu {
    /// Connects to a Janus MediaServer
    pub async fn connect(
        config: settings::JanusMcuConfig,
        channel: lapin::Channel,
    ) -> Result<Self> {
        let rabbit_mq_config = janus_client::RabbitMqConfig::new_from_channel(
            channel,
            config.connection.to_janus_queue,
            config.connection.to_janus_routing_key,
            config.connection.janus_exchange,
            config.connection.from_janus_routing_key,
            TAG.to_owned(),
        );
        let (gw, janus_stream) = janus_client::Client::new(rabbit_mq_config)
            .await
            .context("Failed to create janus_client")?;

        let session = gw
            .create_session()
            .await
            .context("Failed to create session")?;

        let (shutdown, mut shutdown_sig) = broadcast::channel(1);

        tokio::spawn(async move {
            let mut stream = ReceiverStream::new(janus_stream);

            loop {
                tokio::select! {
                    Some(msg) = stream.next() => {
                        log::warn!("Unhandled janus message {:?}", msg);
                    }
                    _ = shutdown_sig.recv() => {
                        return;
                    }
                };
            }
        });

        Ok(Self {
            max_stream_bitrate: config.max_video_bitrate,
            max_screen_bitrate: config.max_screen_bitrate,
            _gw: gw,
            session,
            shutdown,
            publisher_room_ids: Mutex::new(HashMap::new()),
            next_client_id: AtomicU64::new(0),
        })
    }

    pub fn start(&self) -> Result<()> {
        let session = self.session.clone();
        let mut shutdown_sig = self.shutdown.subscribe();

        // spawn a task that sends a keep-alive message to keep the session alive
        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(30));

            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        session.keep_alive().await;
                    }
                    _ = shutdown_sig.recv() => {
                        return;
                    }
                }
            }
        });

        Ok(())
    }

    pub fn stop(&self) {
        self.shutdown
            .send(())
            .expect("Failed to shut down mcu tasks");

        todo!("Shut down all media sessions and remove all rooms")
    }

    pub async fn new_publisher(
        &self,
        listener: Option<mpsc::Sender<(MediaSessionKey, TrickleMessage)>>,
        participant_id: ParticipantId,
        media_session_type: MediaSessionType,
        bitrate: u64,
    ) -> Result<JanusPublisher> {
        let (handle, room_id) = self
            .get_or_create_publisher_handle(participant_id, media_session_type, bitrate)
            .await
            .context("Failed to get or create publisher handle")?;

        let client_id = self.next_client_id();

        let publisher = JanusPublisher {
            session: self.session.clone(),
            handle: handle.clone(),
            room_id,
            media_session_type,
            participant_id,
        };

        self.publisher_room_ids
            .lock()
            .insert(MediaSessionKey(participant_id, media_session_type), room_id);

        tokio::spawn(async move {
            JanusPublisher::run(
                client_id,
                participant_id,
                media_session_type,
                handle,
                listener,
            )
            .await
        });

        Ok(publisher)
    }

    pub async fn new_subscriber(
        &self,
        listener: Option<mpsc::Sender<(MediaSessionKey, TrickleMessage)>>,
        publisher: ParticipantId,
        media_session_type: MediaSessionType,
    ) -> Result<JanusSubscriber> {
        let (handle, room_id) = self
            .get_or_create_subscriber_handle(publisher, media_session_type)
            .await
            .context("Failed to get or create subscriber handle")?;

        let client_id = self.next_client_id();

        let subscriber = JanusSubscriber {
            session: self.session.clone(),
            handle: handle.clone(),
            room_id,
            media_session_type,
            publisher,
        };

        tokio::spawn(async move {
            JanusSubscriber::run(client_id, publisher, media_session_type, handle, listener).await
        });

        Ok(subscriber)
    }

    fn next_client_id(&self) -> ClientId {
        ClientId(self.next_client_id.fetch_add(1, Ordering::Relaxed))
    }

    /// Checks if we already track a room for the publisher media_session_type combination
    ///
    /// As we publish each stream in its own Janus room, we need to keep track of the janus room id from our publisher + stream
    /// Returns the RoomId or None
    fn search_publisher_room(
        &self,
        publisher: ParticipantId,
        media_session_type: MediaSessionType,
    ) -> Option<JanusRoomId> {
        let room_ids = &mut *self.publisher_room_ids.lock();
        room_ids
            .get(&MediaSessionKey(publisher, media_session_type))
            .copied()
    }

    /// Gets or creates a publisher handle
    ///
    /// If no (ParticipantID, StreamType) key is found in our local publisherRoomIds map,
    /// we create a new Janus room (we use one room per publisher per stream type).
    /// Returns a new Janus videoroom handle, ~~the returned Sessionid~~ and the room id.
    async fn get_or_create_publisher_handle(
        &self,
        publisher: ParticipantId,
        media_session_type: MediaSessionType,
        bitrate: u64,
    ) -> Result<(janus_client::Handle, JanusRoomId)> {
        let handle = self
            .session
            .attach_to_plugin(janus_client::JanusPlugin::VideoRoom)
            .await
            .context("Failed to attach session to videoroom plugin")?;

        let room_id = self.search_publisher_room(publisher, media_session_type);

        let room_id = if let Some(room_id) = room_id {
            log::trace!(
                "Found room for publisher `{:?}` with media_session_type `{:?}`",
                &publisher,
                &media_session_type
            );

            room_id
        } else {
            log::trace!(
                "No room for publisher `{:?}` with media_session_type `{:?}`, creating new room",
                &publisher,
                &media_session_type
            );

            let max_bitrate = match media_session_type {
                MediaSessionType::Video => self.max_stream_bitrate,
                MediaSessionType::Screen => self.max_screen_bitrate,
            };

            let bitrate = if bitrate == 0 {
                max_bitrate
            } else {
                bitrate.min(max_bitrate)
            };

            let request = janus_client::outgoing::VideoRoomPluginCreate {
                description: MediaSessionKey(publisher, media_session_type).to_string(),
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
            match response {
                janus_client::incoming::VideoRoomPluginDataCreated::Ok { room, .. } => room,
                janus_client::incoming::VideoRoomPluginDataCreated::Err(e) => {
                    bail!("Failed to create videoroom, got error response: {}", e);
                }
            }
        };

        log::trace!(
            "Using Janus Room {} for publisher {} with media_session_type {}",
            room_id,
            publisher,
            media_session_type
        );

        let join_request = janus_client::outgoing::VideoRoomPluginJoinPublisher {
            room: room_id,
            id: Some(media_session_type.into()),
            display: None,
            token: None,
        };

        let (response, _) = handle.send(join_request).await?;

        match response {
            janus_client::incoming::VideoRoomPluginDataJoined::Ok { .. } => {
                log::trace!(
                    "Publisher {} joined room {} sucessfully",
                    publisher,
                    room_id
                );

                Ok((handle, room_id))
            }
            janus_client::incoming::VideoRoomPluginDataJoined::Err(e) => {
                bail!("Failed to join videoroom, got error response: {}", e);
            }
        }
    }

    /// Gets or creates a subscriber handle
    ///
    /// Gets the room_id from the passed publisher and stream and returns a new janus videoroom handle
    async fn get_or_create_subscriber_handle(
        &self,
        publisher: ParticipantId,
        media_session_type: MediaSessionType,
    ) -> Result<(janus_client::Handle, JanusRoomId)> {
        log::trace!(
            "Looking for janus room_id for {}",
            MediaSessionKey(publisher, media_session_type)
        );

        let room_id = self
            .search_publisher_room(publisher, media_session_type)
            .context("Failed to subscribe to room, it does not exist")?;

        log::trace!(
            "Got room_id {} for {}",
            room_id,
            MediaSessionKey(publisher, media_session_type)
        );

        let handle = self
            .session
            .attach_to_plugin(janus_client::JanusPlugin::VideoRoom)
            .await
            .context("Failed to attach to videoroom plugin")?;

        Ok((handle, room_id))
    }
}

pub struct JanusPublisher {
    session: janus_client::Session,
    handle: janus_client::Handle,
    room_id: JanusRoomId,
    media_session_type: MediaSessionType,

    participant_id: ParticipantId,
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

    /// Event handler for a Publisher
    ///
    /// Stops when all Senders of the handle [Receiver](tokio::sync::broadcast::Receiver) are dropped.
    async fn run(
        id: ClientId,
        participant_id: ParticipantId,
        media_session_type: MediaSessionType,
        handle: janus_client::Handle,
        listener: Option<mpsc::Sender<(MediaSessionKey, TrickleMessage)>>,
    ) {
        let mut stream = BroadcastStream::new(handle.subscribe());
        while let Some(data) = stream.next().await {
            log::debug!("Subscriber got msg:{:?}", data);
            match data {
                Ok(e) => {
                    match e.as_ref() {
                        janus_client::JanusMessage::Event(event) => {
                            let janus_client::incoming::Event { plugindata, .. } = event;
                            if let janus_client::PluginData::VideoRoom(plugindata) = plugindata {
                                match plugindata {
                                    janus_client::incoming::VideoRoomPluginData::Destroyed(_) => {
                                        log::info!("Publisher {}: The room of this publisher got destroyed. Closing this publisher", id)
                                        // todo send close over shutdown channel
                                    }
                                    _ => log::warn!(
                                        "Invalid handle event for publisher {}: {:?}",
                                        id,
                                        event
                                    ),
                                }
                            }
                        }
                        janus_client::JanusMessage::Hangup(event) => {
                            log::info!(
                                "Publisher {}: Received HangUp: {}. Shutting down.",
                                id,
                                event.reason
                            );
                            // todo send close over shutdown channel
                        }
                        janus_client::JanusMessage::Detached => {
                            log::info!("Publisher {}: Received Detached", id);
                            // todo send close over shutdown channel
                        }
                        janus_client::JanusMessage::Media(_) => {
                            log::debug!("Received Media Event. This is unsupported.")
                        }
                        janus_client::JanusMessage::WebRtcUpdate(_) => {
                            log::debug!("Publisher {}: Received connected", id)
                        }
                        janus_client::JanusMessage::SlowLink(event) => {
                            if event.uplink {
                                log::info!("Publisher {}: Received SlowLink (Janus -> Client)", id);
                            } else {
                                log::info!("Publisher {}: Received SlowLink (Client -> Janus)", id);
                            }
                        }
                        janus_client::JanusMessage::Trickle(event) => {
                            let msg: TrickleMessage = event.clone().into();
                            if let Some(listener) = &listener {
                                if let Err(e) = listener
                                    .send((
                                        MediaSessionKey(participant_id, media_session_type),
                                        msg,
                                    ))
                                    .await
                                {
                                    log::error!("Failed to send ICE msg to ICE listener {}", e);
                                }
                            }
                        }
                        _ => {
                            log::debug!("Received unwelcomed Event for handle: {}", handle.id());
                        }
                    }
                }
                Err(BroadcastStreamRecvError::Lagged(_)) => {
                    panic!("Receiver lagged behind. Increase channel size")
                }
            }
        }
        log::trace!("Broadcast Stream for this JanusPublisher stopped. Exiting run method");
    }
}

impl Drop for JanusPublisher {
    fn drop(&mut self) {
        let rt = if let Ok(rt) = tokio::runtime::Handle::try_current() {
            rt
        } else {
            log::error!(
                "Could not clean up resources for Handle({}|{}) (runtime stopped)",
                self.participant_id,
                self.media_session_type
            );

            return;
        };

        let handle = self.handle.clone();
        let room_id = self.room_id;
        let session = self.session.clone();

        rt.spawn(async move {
            handle
                .send(janus_client::types::outgoing::VideoRoomPluginDestroy {
                    room: room_id,
                    secret: None,
                    permanent: None,
                    token: None,
                })
                .await
                .expect("Failed to send VideoRoomPluginDestroy on drop");

            session.detach_handle(&handle.id())
        });
    }
}

pub struct JanusSubscriber {
    session: janus_client::Session,
    handle: janus_client::Handle,
    room_id: JanusRoomId,
    media_session_type: MediaSessionType,
    publisher: ParticipantId,
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

    /// Joins the room of the publisher this [JanusSubscriber](JanusSubscriber) is subscriber to
    async fn join_room(&self) -> Result<janus_client::Jsep> {
        let mut retry_counter = 0;
        loop {
            // Todo do we want to do this?
            if retry_counter >= 5 {
                log::warn!(
                    "Giving up on joining publisher {} to room {}.",
                    self.publisher,
                    self.room_id
                );
                break;
            }
            retry_counter += 1;

            let join_response = self
                .handle
                .send(janus_client::outgoing::VideoRoomPluginJoinSubscriber {
                    room: self.room_id,
                    feed: janus_client::FeedId::new(self.media_session_type.into()),
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
                    return Ok(jsep);
                }
                Ok((_, None)) => bail!("Got invalid response on join_room, missing jsep"),
                Err(janus_client::error::Error::JanusPluginError(e)) => {
                    match e.error_code() {
                        JanusInternalError::VideoroomErrorAlreadyJoined => {
                            // todo handle this case
                            // We need to generate a new handle. Can we use std::mem::replace? We would need to get self.handle mutually.
                            log::error!(
                                "Subscriber {} already subscribed to this room",
                                self.publisher
                            );

                            todo!()
                        }
                        JanusInternalError::VideoroomErrorNoSuchRoom
                        | JanusInternalError::VideoroomErrorNoSuchFeed => {
                            log::warn!(
                                "Publisher {} not ready for room {}. Waiting 200ms before retrying",
                                self.publisher,
                                self.room_id
                            );

                            sleep(Duration::from_millis(200)).await;
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }

        bail!(
            "Failed to join room: {}, see logs for info",
            MediaSessionKey(self.publisher, self.media_session_type)
        )
    }

    async fn run(
        id: ClientId,
        publisher: ParticipantId,
        media_session_type: MediaSessionType,
        handle: janus_client::Handle,
        listener: Option<mpsc::Sender<(MediaSessionKey, TrickleMessage)>>,
    ) {
        let mut stream = BroadcastStream::new(handle.subscribe());
        while let Some(data) = stream.next().await {
            match data {
                Ok(e) => {
                    match e.as_ref() {
                        janus_client::JanusMessage::Event(event) => {
                            let janus_client::incoming::Event { plugindata, .. } = event;
                            if let janus_client::PluginData::VideoRoom(plugindata) = plugindata {
                                match plugindata {
                                    janus_client::incoming::VideoRoomPluginData::Destroyed(_) => {
                                        log::info!("Subscriber {}: The room of this Subscriber got destroyed. Closing this Subscriber", id)
                                        // todo send close over shutdown channel
                                    }
                                    _ => log::warn!(
                                        "Invalid handle event for Subscriber {}: {:?}",
                                        id,
                                        event
                                    ),
                                }
                            }
                        }
                        janus_client::JanusMessage::Hangup(event) => {
                            log::info!(
                                "Subscriber {}: Received HangUp: {}. Shutting down.",
                                id,
                                event.reason
                            );
                            // todo send close over shutdown channel
                        }
                        janus_client::JanusMessage::Detached => {
                            log::info!("Subscriber {}: Received Detached", id);
                            // todo send close over shutdown channel
                        }
                        janus_client::JanusMessage::Media(_) => {
                            log::debug!("Received Media Event. This is unsupported.")
                        }
                        janus_client::JanusMessage::WebRtcUpdate(_) => {
                            log::debug!("Subscriber {}: Received connected", id)
                        }
                        janus_client::JanusMessage::SlowLink(event) => {
                            if event.uplink {
                                log::info!(
                                    "Subscriber {}: Received SlowLink (Janus -> Client)",
                                    id
                                );
                            } else {
                                log::info!(
                                    "Subscriber {}: Received SlowLink (Client -> Janus)",
                                    id
                                );
                            }
                        }
                        janus_client::JanusMessage::Trickle(event) => {
                            let msg: TrickleMessage = event.clone().into();
                            if let Some(listener) = &listener {
                                if let Err(e) = listener
                                    .send((MediaSessionKey(publisher, media_session_type), msg))
                                    .await
                                {
                                    log::error!("Failed to send ICE msg to ICE listener {}", e);
                                }
                            }
                        }
                        _ => {
                            log::debug!("Received unwelcomed Event for handle: {}", handle.id());
                        }
                    }
                }
                Err(BroadcastStreamRecvError::Lagged(_)) => {
                    panic!("Receiver lagged behind. Increase channel size")
                }
            }
        }
        log::trace!("Broadcast Stream for this JanusSubscriber stopped. Exiting run method");
    }
}

impl Drop for JanusSubscriber {
    fn drop(&mut self) {
        self.session.detach_handle(&self.handle.id())
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
