use crate::api::signaling::mcu::{
    JanusPublisher, JanusSubscriber, McuPool, MediaSessionKey, MediaSessionType, WebRtcEvent,
};
use crate::api::signaling::ws_modules::media::MediaSessionState;
use crate::api::signaling::ParticipantId;
use anyhow::{ensure, Result};
use std::collections::HashMap;
use tokio::sync::mpsc;

pub struct MediaSessions {
    id: ParticipantId,

    // All publishers that belong to the participant
    publishers: HashMap<MediaSessionType, JanusPublisher>,

    // All subscribers that belong to the participant
    subscribers: HashMap<MediaSessionKey, JanusSubscriber>,

    // The send part of the event channel used when creating new publishers/subscribers
    sender: mpsc::Sender<(MediaSessionKey, WebRtcEvent)>,
}

impl MediaSessions {
    pub fn new(id: ParticipantId, sender: mpsc::Sender<(MediaSessionKey, WebRtcEvent)>) -> Self {
        Self {
            id,
            publishers: Default::default(),
            subscribers: Default::default(),
            sender,
        }
    }

    /// Creates a new [JanusPublisher] for this stream
    ///
    /// The created [JanusPublisher] is stored and a reference is returned.
    pub async fn create_publisher(
        &mut self,
        mcu_client: &McuPool,
        media_session_type: MediaSessionType,
    ) -> Result<&JanusPublisher> {
        ensure!(
            !self.publishers.contains_key(&media_session_type),
            "There can only be one publisher per media_session_type"
        );

        let publisher = mcu_client
            .new_publisher(
                self.sender.clone(),
                MediaSessionKey(self.id, media_session_type),
            )
            .await?;

        self.publishers.insert(media_session_type, publisher);

        Ok(self
            .publishers
            .get(&media_session_type)
            .expect("Insert failed"))
    }

    /// Returns [JanusSubscriber] for the given stream if present, else None
    pub fn get_publisher(&self, media_session_type: MediaSessionType) -> Option<&JanusPublisher> {
        self.publishers.get(&media_session_type)
    }

    /// Creates a new [JanusSubscriber] for this stream
    ///
    /// The created [JanusPublisher] is stored in this [MediaSessions] map, and a reference is returned.
    pub async fn create_subscriber(
        &mut self,
        mcu_client: &McuPool,
        participant: ParticipantId,
        media_session_type: MediaSessionType,
    ) -> Result<&JanusSubscriber> {
        let key = MediaSessionKey(participant, media_session_type);
        ensure!(
            !self.subscribers.contains_key(&key),
            "There should only be one subscriber per media session key"
        );

        let subscriber = mcu_client
            .new_subscriber(
                self.sender.clone(),
                MediaSessionKey(participant, media_session_type),
            )
            .await?;

        self.subscribers.insert(key, subscriber);

        Ok(self.subscribers.get(&key).expect("Insert failed"))
    }

    /// Returns [JanusSubscriber] for the given stream if present, else None
    pub fn get_subscriber(
        &mut self,
        participant: ParticipantId,
        media_session_type: MediaSessionType,
    ) -> Option<&JanusSubscriber> {
        self.subscribers
            .get(&MediaSessionKey(participant, media_session_type))
    }

    /// Removes the [JanusPublisher] for the given StreamType
    pub async fn remove_publisher(&mut self, media_session_type: MediaSessionType) {
        if let Some(publisher) = self.publishers.remove(&media_session_type) {
            if let Err(e) = publisher.destroy().await {
                log::error!("Failed to destroy publisher, {}", e);
            }
        }
    }

    /// When receiving an update message for a specific participant one must check if the updated
    /// participant has dropped any media sessions.
    ///
    /// To remove any subscribers that might be subscribed to a room that has no publisher or has
    /// been removed this function must be called with the updated participant's `publishing` field
    pub async fn remove_dangling_subscriber(
        &mut self,
        participant: ParticipantId,
        lookup: &HashMap<MediaSessionType, MediaSessionState>,
    ) {
        while let Some(key) = self
            .subscribers
            .keys()
            .find(|key| key.0 == participant)
            .copied()
        {
            if lookup.contains_key(&key.1) {
                continue;
            }

            // Safe unwrap since key was taken from hashmap
            let subscriber = self.subscribers.remove(&key).unwrap();

            if let Err(e) = subscriber.destroy().await {
                log::error!("Failed to destroy subscriber, {}", e);
            }
        }
    }

    /// Remove a specific subscriber
    pub async fn remove_subscriber(&mut self, media_session_key: &MediaSessionKey) {
        if let Some(subscriber) = self.subscribers.remove(&media_session_key) {
            if let Err(e) = subscriber.destroy().await {
                log::error!("Failed to destroy subscriber, {}", e);
            }
        } else {
            log::error!(
                "Failed to destroy subscriber, unable to find subscriber by media_session_key {}",
                media_session_key
            )
        }
    }

    /// Remove all subscribers to all streams published by the given participant
    pub async fn remove_subscribers(&mut self, participant: ParticipantId) {
        while let Some(key) = self
            .subscribers
            .keys()
            .find(|key| key.0 == participant)
            .copied()
        {
            // Safe unwrap since key was taken from hashmap
            self.remove_subscriber(&key).await;
        }
    }

    /// Destroy all sessions
    pub async fn destroy(mut self) {
        for (_, subscriber) in self.subscribers.drain() {
            log::debug!("Destroy subscriber {}", self.id);
            if let Err(e) = subscriber.destroy().await {
                log::error!("Failed to destroy subscriber, {}", e);
            }
        }

        for (_, publisher) in self.publishers.drain() {
            log::debug!("Destroy publisher {}", self.id);
            if let Err(e) = publisher.destroy().await {
                log::error!("Failed to destroy publisher, {}", e);
            }
        }

        log::debug!("Destroyed all sessions");
    }
}
