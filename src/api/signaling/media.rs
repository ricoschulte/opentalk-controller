use crate::api::signaling::local::MediaSessionState;
use crate::api::signaling::mcu::{
    JanusMcu, JanusPublisher, JanusSubscriber, MediaSessionKey, MediaSessionType, TrickleMessage,
};
use crate::api::signaling::ParticipantId;
use anyhow::{ensure, Result};
use std::collections::HashMap;
use tokio::sync::mpsc;

pub struct Media {
    id: ParticipantId,

    /// All publishers that belong to the participant
    publishers: HashMap<MediaSessionType, JanusPublisher>,

    /// All subscribers that belong to the participant
    subscribers: HashMap<MediaSessionKey, JanusSubscriber>,

    /// The send part of the event channel used when creating new publishers/subscribers
    sender: mpsc::Sender<(MediaSessionKey, TrickleMessage)>,
}

impl Media {
    pub fn new(id: ParticipantId, sender: mpsc::Sender<(MediaSessionKey, TrickleMessage)>) -> Self {
        Self {
            id,
            publishers: Default::default(),
            subscribers: Default::default(),
            sender,
        }
    }

    /// Creates a new [McuPublisher] for this stream
    ///
    /// The created [McuPublisher] is stored and a reference is returned.
    pub async fn create_publisher(
        &mut self,
        mcu_client: &JanusMcu,
        media_session_type: MediaSessionType,
    ) -> Result<&JanusPublisher> {
        ensure!(
            !self.publishers.contains_key(&media_session_type),
            "There can only be one publisher per media_session_type"
        );

        let publisher = mcu_client
            .new_publisher(Some(self.sender.clone()), self.id, media_session_type, 0)
            .await?;

        self.publishers.insert(media_session_type, publisher);

        Ok(self
            .publishers
            .get(&media_session_type)
            .expect("Insert failed"))
    }

    /// Returns [McuPublisher](McuPublisher) for the given stream if present, else None
    pub fn get_publisher(&self, media_session_type: MediaSessionType) -> Option<&JanusPublisher> {
        self.publishers.get(&media_session_type)
    }

    /// Creates a new [McuSubscriber] for this stream
    ///
    /// The created [McuPublisher] is stored in this [ClientSessions] map, and a reference is returned.
    pub async fn create_subscriber(
        &mut self,
        mcu_client: &JanusMcu,
        participant: ParticipantId,
        media_session_type: MediaSessionType,
    ) -> Result<&JanusSubscriber> {
        let key = MediaSessionKey(participant, media_session_type);
        ensure!(
            !self.subscribers.contains_key(&key),
            "There should only be one subscriber per media session key"
        );

        let subscriber = mcu_client
            .new_subscriber(Some(self.sender.clone()), participant, media_session_type)
            .await?;

        // Todo v4+ Can we make the StreamKey copy? Can we guarantee that this holds in the future?
        self.subscribers.insert(key.clone(), subscriber);

        Ok(self.subscribers.get(&key).expect("Insert failed"))
    }

    /// Returns [McuSubscriber] for the given stream if present, else None
    pub fn get_subscriber(
        &mut self,
        participant: ParticipantId,
        media_session_type: MediaSessionType,
    ) -> Option<&JanusSubscriber> {
        self.subscribers
            .get(&MediaSessionKey(participant, media_session_type))
    }

    /// Removes the [McuPublisher] for the given StreamType
    pub fn remove_publisher(&mut self, media_session_type: MediaSessionType) {
        self.publishers.remove(&media_session_type);
    }

    /// When receiving an update message for a specific participant one must check if the updated
    /// participant has dropped any media sessions.
    ///
    /// To remove any subscribers that might be subscribed to a room that has no publisher or has
    /// been removed this function must be called with the updated participant's `publishing` field
    pub fn remove_dangling_subscribers(
        &mut self,
        participant: ParticipantId,
        lookup: &HashMap<MediaSessionType, MediaSessionState>,
    ) {
        self.subscribers.retain(|stream_key, _| {
            if stream_key.0 == participant {
                lookup.contains_key(&stream_key.1)
            } else {
                true
            }
        });
    }

    /// Remove all subscribers to all streams published by the given participant
    pub fn remove_subscribers(&mut self, participant: ParticipantId) {
        self.subscribers
            .retain(|stream_key, _| stream_key.0 != participant);
    }
}
