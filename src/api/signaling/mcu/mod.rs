use crate::api::signaling::ParticipantId;
use crate::settings;
use anyhow::{bail, Context, Result};
use futures::StreamExt;
use janus_client::types::{SdpAnswer, SdpOffer};
use janus_client::{JanusMessage, JsepType, RoomId as JanusRoomId, TrickleCandidate};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::convert::TryInto;
use std::time::Duration;
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio::time::interval;
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
    session: janus_client::Session,

    _gw: janus_client::Client,

    /// Shutdown signal issued by the MCU for the janus-client receive task, since that is
    /// needed for teardown AFTER receiving the app shutdown signal.
    mcu_shutdown: broadcast::Sender<()>,

    /// Stores the room ids?
    publisher_room_ids: Mutex<HashMap<MediaSessionKey, JanusRoomId>>,
}

impl JanusMcu {
    /// Connects to a Janus MediaServer
    pub async fn connect(
        config: settings::JanusMcuConfig,
        channel: lapin::Channel,
    ) -> Result<Self> {
        let (mcu_shutdown, _) = broadcast::channel(1);

        let rabbit_mq_config = janus_client::RabbitMqConfig::new_from_channel(
            channel,
            config.connection.to_janus_queue,
            config.connection.to_janus_routing_key,
            config.connection.janus_exchange,
            config.connection.from_janus_routing_key,
            // TODO dynamically create this TAG from controller- and janus-id
            TAG.to_owned(),
        );

        let (gw, janus_stream) = janus_client::Client::new(rabbit_mq_config, mcu_shutdown.clone())
            .await
            .context("Failed to create janus_client")?;

        let session = gw
            .create_session()
            .await
            .context("Failed to create session")?;

        let mut shutdown_sig = mcu_shutdown.subscribe();

        let keep_alive_session = session.clone();

        tokio::spawn(async move {
            let mut stream = ReceiverStream::new(janus_stream);
            let mut interval = interval(Duration::from_secs(30));

            loop {
                tokio::select! {
                    Some(msg) = stream.next() => {
                        log::warn!("Unhandled janus message {:?}", msg);
                        // TODO Find out what we want to with these messages
                        // most of them are events which are not interesting to us
                        // and others expose where we ignore responses from janus
                    }
                    _ = interval.tick() => {
                        keep_alive_session.keep_alive().await;
                    }
                    _ = shutdown_sig.recv() => {
                        log::debug!("receive/keepalive task got shutdown signal, exiting");
                        return;
                    }
                };
            }
        });

        Ok(Self {
            max_stream_bitrate: config.max_video_bitrate,
            max_screen_bitrate: config.max_screen_bitrate,
            session,
            _gw: gw,
            mcu_shutdown,
            publisher_room_ids: Mutex::new(HashMap::new()),
        })
    }

    pub async fn destroy(&mut self) -> Result<()> {
        let _ = self.mcu_shutdown.send(());

        self.session.destroy().await?;

        Ok(())
    }

    pub async fn new_publisher(
        &self,
        event_sink: mpsc::Sender<(MediaSessionKey, WebRtcEvent)>,
        participant_id: ParticipantId,
        media_session_type: MediaSessionType,
        bitrate: u64,
    ) -> Result<JanusPublisher> {
        let (handle, room_id) = self
            .get_or_create_publisher_handle(participant_id, media_session_type, bitrate)
            .await
            .context("Failed to get or create publisher handle")?;

        let (destroy, destroy_sig) = oneshot::channel();

        let publisher = JanusPublisher {
            handle: handle.clone(),
            room_id,
            destroy,
        };

        self.publisher_room_ids
            .lock()
            .insert(MediaSessionKey(participant_id, media_session_type), room_id);

        tokio::spawn(JanusPublisher::run(
            participant_id,
            media_session_type,
            handle,
            event_sink,
            destroy_sig,
        ));

        Ok(publisher)
    }

    pub async fn new_subscriber(
        &self,
        event_sink: mpsc::Sender<(MediaSessionKey, WebRtcEvent)>,
        publisher: ParticipantId,
        media_session_type: MediaSessionType,
    ) -> Result<JanusSubscriber> {
        let (handle, room_id) = self
            .get_or_create_subscriber_handle(publisher, media_session_type)
            .await
            .context("Failed to get or create subscriber handle")?;

        let (destroy, destroy_sig) = oneshot::channel();

        let subscriber = JanusSubscriber {
            handle: handle.clone(),
            room_id,
            media_session_type,
            destroy,
        };

        tokio::spawn(JanusSubscriber::run(
            publisher,
            media_session_type,
            handle,
            event_sink,
            destroy_sig,
        ));

        Ok(subscriber)
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
    handle: janus_client::Handle,
    room_id: JanusRoomId,
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

    pub async fn destroy(self) -> Result<()> {
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

        Ok(())
    }

    /// Event handler for a Publisher
    ///
    /// Stops when all Senders of the handle [Receiver](tokio::sync::broadcast::Receiver) are dropped.
    async fn run(
        participant_id: ParticipantId,
        media_session_type: MediaSessionType,
        handle: janus_client::Handle,
        event_sink: mpsc::Sender<(MediaSessionKey, WebRtcEvent)>,
        mut destroy_sig: oneshot::Receiver<()>,
    ) {
        let mut stream = BroadcastStream::new(handle.subscribe());

        let media_session_key = MediaSessionKey(participant_id, media_session_type);

        loop {
            tokio::select! {
                _ = &mut destroy_sig => {
                    return;
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
    media_session_type: MediaSessionType,
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

    pub async fn destroy(self) -> Result<()> {
        let _ = self.destroy.send(());

        self.handle.detach().await?;

        Ok(())
    }

    /// Joins the room of the publisher this [JanusSubscriber](JanusSubscriber) is subscriber to
    async fn join_room(&self) -> Result<janus_client::Jsep> {
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
                Ok(jsep)
            }
            Ok((_, None)) => bail!("Got invalid response on join_room, missing jsep"),
            Err(e) => bail!("Failed to join room, {}", e),
        }
    }

    async fn run(
        publisher: ParticipantId,
        media_session_type: MediaSessionType,
        handle: janus_client::Handle,
        event_sink: mpsc::Sender<(MediaSessionKey, WebRtcEvent)>,
        mut destroy_sig: oneshot::Receiver<()>,
    ) {
        let mut stream = BroadcastStream::new(handle.subscribe());

        let media_session_key = MediaSessionKey(publisher, media_session_type);

        loop {
            tokio::select! {
                _ = &mut destroy_sig => {
                    return;
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
