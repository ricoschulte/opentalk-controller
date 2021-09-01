//! This crate wraps the Janus WebSocket asynchronous API to provide a more or less idiomatic Rust API.
//!
//! For this the client internally resolves futures based on the incoming responses and their respective transaction identifier.
//! This is hidden to provide an API where you can simply call a function and .await the response.
//! Thus this creates needs to be run in a async/.await runtime. Currently we only support the tokio runtime.
//!
//! # Examples
//!```should_panic
//! # use janus_client::types::outgoing;
//! # use janus_client::{JanusPlugin, Client, RabbitMqConfig, ClientId};
//! # use tokio::sync::{broadcast,mpsc};
//! # use std::sync::Arc;
//! # tokio_test::block_on(async {
//! let (sink, _) = mpsc::channel(1);
//! let connection = lapin::Connection::connect("amqp://janus-backend:5672", lapin::ConnectionProperties::default()).await.unwrap();
//! let channel = connection.create_channel().await.unwrap();
//! let config = RabbitMqConfig::new_from_channel(channel, "janus-gateway".into(), "to-janus".into(), "from-janus".into(), "k3k-signaling".into());
//! let client = Client::new(config, ClientId(Arc::from("")), sink).await.unwrap();
//! let session = client.create_session().await.unwrap();
//! let echo_handle = session
//!     .attach_to_plugin(JanusPlugin::Echotest)
//!     .await
//!     .unwrap();
//!
//! let echo = echo_handle
//!     .send(outgoing::EchoPluginUnnamed {
//!             audio: Some(true),
//!             ..Default::default()
//!     })
//!     .await.unwrap();
//! println!("Echo {:?}, JSEP: {:?}", &echo.0, &echo.1);
//! # });
//! ```
//!
//! Furtermore you can wrap the API and build upon that similar to spreed
//! ```should_panic
//! # use janus_client::{Client, Handle, JanusPlugin, RabbitMqConfig};
//! # use janus_client::types::{TrickleCandidate, RoomId};
//! # use janus_client::types::outgoing::{TrickleMessage, PluginBody, VideoRoomPluginJoin, VideoRoomPluginJoinSubscriber};
//! # use tokio::sync::{broadcast, mpsc};
//! # use janus_client::ClientId;
//! # use std::sync::Arc;
//! pub struct SubscriberClient(Handle);
//! impl SubscriberClient {
//!     /// Joins a Room
//!     pub async fn join_room(&self, candidate: String ) {
//!         let room_id = 2.into();
//!         let request =
//!           VideoRoomPluginJoinSubscriber{
//!             room: room_id,
//!             feed: 1.into(),
//! #           audio: None, video:None, data: None, close_pc: None, private_id: None, offer_audio: None, offer_video: None, offer_data: None, spatial_layer: None, temporal_layer: None, substream: None, temporal: None
//!           };
//!         self.0.send(request).await;
//!     }
//! }
//!
//! pub struct PublisherClient(Handle);
//! impl PublisherClient {
//!     /// Sends the candidate SDP string to Janus
//!     pub async fn send_candidates(&self, candidate: String ) {
//!         self.0.trickle(TrickleMessage::Candidate(TrickleCandidate{
//!             candidate: "candidate:..".to_owned(),
//!             sdp_m_id: "audio".to_owned(),
//!             sdp_m_line_index: 1
//!         })).await;
//!     }
//! }
//! # fn main() {
//! tokio_test::block_on(async {
//! let (sink, _) = mpsc::channel(1);
//! let connection = lapin::Connection::connect("amqp://janus-backend:5672", lapin::ConnectionProperties::default()).await.unwrap();
//! let channel = connection.create_channel().await.unwrap();
//! let config = RabbitMqConfig::new_from_channel(channel, "janus-gateway".into(), "to-janus".into(), "from-janus".into(), "k3k-signaling".into());
//! let client = janus_client::Client::new(config, ClientId(Arc::from("")), sink).await.unwrap();
//! let session = client.create_session().await.unwrap();
//!
//! let echo_handle = session
//!     .attach_to_plugin(JanusPlugin::VideoRoom)
//!     .await
//!     .unwrap();
//! let publisher = PublisherClient(echo_handle);
//!
//! let echo_handle = session
//!     .attach_to_plugin(JanusPlugin::VideoRoom)
//!     .await
//!     .unwrap();
//! let subscriber = SubscriberClient(echo_handle);
//! });
//! }
//!```
//!
//! # Features
//! Specific plugins are hidden behind feature flags.
//! Supported Janus plugins can be enabled with the following cargo features
//! - `echotest` for the EchoTest Janus plugin
//! - `videoroom` for the VideoRoom Janus plugin
//!
//! By default `echotest` and `videoroom` are enabled.

use crate::client::{InnerClient, InnerHandle, InnerSession};
use crate::outgoing::TrickleMessage;
use std::sync::{Arc, Weak};
use std::time::Duration;
use tokio::sync::{broadcast, mpsc};
use tokio::time::sleep;

mod client;
pub mod error;
pub mod rabbitmq;
pub mod types;

pub use crate::rabbitmq::RabbitMqConfig;
pub use crate::types::incoming::JanusMessage;
pub use crate::types::*;

#[derive(Debug, Clone)]
pub struct ClientId(pub Arc<str>);

/// Janus API Client
#[derive(Debug, Clone)]
pub struct Client {
    inner: Arc<InnerClient>,
}

impl Client {
    /// Creates a new [`Client`](Client)
    ///
    /// Returns the client itself and a broadcast receiver for messages from Janus that are not a response
    #[tracing::instrument(name = "client_new", skip(sink, config))]
    pub async fn new(
        config: RabbitMqConfig,
        id: ClientId,
        sink: mpsc::Sender<(ClientId, Arc<JanusMessage>)>,
    ) -> Result<Self, error::Error> {
        let inner_client = InnerClient::new(config, id, sink).await?;

        let client = Self {
            inner: Arc::new(inner_client),
        };

        Ok(client)
    }

    pub async fn destroy(&mut self) {
        self.inner.destroy().await
    }

    /// Creates a Session
    ///
    /// Returns a [`Session`](Session) or [`Error`](error::Error) if something went wrong
    pub async fn create_session(&self) -> Result<Session, error::Error> {
        let session_id = self.inner.create_session().await?;

        let session = Arc::new(InnerSession::new(Arc::downgrade(&self.inner), session_id));

        self.inner
            .sessions
            .lock()
            .insert(session_id, Arc::downgrade(&session));

        Ok(Session { inner: session })
    }
}

/// Janus API Session
///
/// Allows to receive events from Janus and to create a [`Handle`](Handle) for a specific janus plugin (e.g. videoroom)
// todo expose a broadcast::Receiver as well for a Session as there might be Janus events that have a session but no sender
#[derive(Clone, Debug)]
pub struct Session {
    inner: Arc<InnerSession>,
}

impl Session {
    // Returns the SessionId
    pub fn id(&self) -> SessionId {
        self.inner.id
    }

    /// Returns the [`Handle`](Handle) with the given `HandleId`
    pub fn find_handle(&self, id: &HandleId) -> Result<Handle, error::Error> {
        Ok(Handle {
            inner: self.inner.find_handle(id)?,
        })
    }

    /// Attaches to the given plugin
    ///
    /// Returns a [`Handle`](Handle) or [`Error`](error::Error) if something went wrong
    pub async fn attach_to_plugin(&self, plugin: JanusPlugin) -> Result<Handle, error::Error> {
        Ok(Handle {
            inner: self.inner.attach_to_plugin(plugin).await?,
        })
    }

    /// Send Keep Alive
    pub async fn keep_alive(&self) -> Result<(), error::Error> {
        self.inner.keep_alive().await
    }

    /// Destroys the session.
    ///
    /// # Danger:
    ///
    /// Assumes that all other occurrences of the same Session will be dropped.
    /// Waits for the strong reference count to reach zero and sends a Destroy request.
    #[tracing::instrument(
        name = "session_destroy",
        level = "debug",
        skip(self),
        fields(session = %self.inner.id),
    )]
    pub async fn destroy(&mut self, broken: bool) -> Result<(), error::Error> {
        let client = self
            .inner
            .client
            .upgrade()
            .expect("Failed Weak::upgrade. Expected the client reference to be still valid");

        client.sessions.lock().remove(&self.inner.id);

        loop {
            let strong_count = Arc::strong_count(&self.inner);

            log::debug!(
                "Destroying Session({}), waiting strong_count to reach 1 (is {})",
                self.inner.id,
                strong_count
            );

            if strong_count == 1 {
                let inner = Arc::get_mut(&mut self.inner).expect("already checked strong_count");

                return if broken {
                    inner.assume_destroyed();
                    Ok(())
                } else {
                    inner.destroy(client).await
                };
            } else {
                sleep(Duration::from_millis(100)).await;
            }
        }
    }
}

/// Janus API Plugin sessionhandle
///
/// Allows to talk to a plugin and receive messages from the plugin
/// You can [`Self::subscribe()`](Self::subscribe()) to the sink to receive messages that are not the response to a sent request
#[derive(Clone, Debug)]
pub struct Handle {
    inner: Arc<InnerHandle>,
}

impl Handle {
    /// Returns the HandleId
    pub fn id(&self) -> HandleId {
        self.inner.id
    }

    /// Returns the HandleId
    pub fn session_id(&self) -> SessionId {
        self.inner.session_id
    }

    /// Subscribe to messages that are not the response to a sent request
    pub fn subscribe(&self) -> broadcast::Receiver<Arc<incoming::JanusMessage>> {
        self.inner.subscribe()
    }

    /// Sends data to the attached plugin
    ///
    /// Checks if the returned result is of the correct type.
    pub async fn send<R: PluginRequest>(
        &self,
        request: R,
    ) -> Result<(R::PluginResponse, Option<Jsep>), error::Error> {
        self.inner.send(request).await
    }

    /// Sends data to the attached plugin together with jsep
    ///
    /// Checks if the returned result is of the correct type.
    pub async fn send_with_jsep<R: PluginRequest>(
        &self,
        request: R,
        jsep: Jsep,
    ) -> Result<(R::PluginResponse, Option<Jsep>), error::Error> {
        self.inner.send_with_jsep(request, jsep).await
    }

    /// Sends the candidate SDP string to Janus
    pub async fn trickle(&self, msg: TrickleMessage) -> Result<(), error::Error> {
        self.inner.trickle(msg).await
    }

    /// Detaches this handle
    ///
    /// # Danger:
    ///
    /// Assumes that all other occurrences of the same Handle will be dropped.
    /// Waits for the strong reference count to reach zero and sends a Detach request.
    #[tracing::instrument(
        name = "handle_detach",
        level = "trace",
        skip(self),
        fields(handle = %self.inner.id),
    )]
    pub async fn detach(mut self, broken: bool) -> Result<(), error::Error> {
        log::trace!("Detaching handle {}", self.id());
        let client = self
            .inner
            .client
            .upgrade()
            .ok_or(error::Error::NotConnected)?;

        let sessions = client.sessions.lock();

        if let Some(session) = sessions.get(&self.inner.session_id).and_then(Weak::upgrade) {
            session.handles.lock().remove(&self.inner.id);
        } else {
            log::trace!(
                "Failed to detach handle {}, session no longer exists",
                self.id()
            );
            return Ok(());
        }

        drop(sessions);

        loop {
            if let Some(inner) = Arc::get_mut(&mut self.inner) {
                // skip the detach process if the client is broken
                return if broken {
                    inner.assume_detached();
                    Ok(())
                } else {
                    inner.detach(client).await
                };
            } else {
                log::debug!(
                    "Detaching Handle({}), waiting refcount to reach 1",
                    self.inner.id,
                );

                sleep(Duration::from_millis(100)).await;
            }
        }
    }
}
