// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use crate::mcu::{
    JanusPublisher, JanusSubscriber, McuPool, MediaSessionKey, MediaSessionType, WebRtcEvent,
};
use crate::MediaSessionState;
use anyhow::{ensure, Result};
use controller::prelude::*;
use std::collections::HashMap;
use std::future::Future;
use tokio::sync::mpsc;
use types::core::ParticipantId;

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

    /// Removes a broken [JanusPublisher] for the given StreamType
    pub async fn remove_broken_publisher(&mut self, media_session_type: MediaSessionType) {
        if let Some(publisher) = self.publishers.remove(&media_session_type) {
            if let Err(e) = publisher.destroy_broken().await {
                log::error!("Failed to remove broken publisher, {}", e);
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
        new_media_state: &HashMap<MediaSessionType, MediaSessionState>,
    ) {
        let to_remove = self
            .subscribers
            .keys()
            .filter(|key| key.0 == participant && !new_media_state.contains_key(&key.1))
            .copied()
            .collect::<Vec<MediaSessionKey>>();

        for key in to_remove {
            // Safe unwrap since key was taken from hashmap
            let subscriber = self.subscribers.remove(&key).unwrap();

            if let Err(e) = subscriber.destroy(false).await {
                log::error!("Failed to destroy subscriber, {}", e);
            }
        }
    }

    /// Remove a specific subscriber
    pub async fn remove_subscriber(&mut self, media_session_key: &MediaSessionKey) {
        if let Some(subscriber) = self.subscribers.remove(media_session_key) {
            if let Err(e) = subscriber.destroy(false).await {
                log::error!("Failed to destroy subscriber, {}", e);
            }
        } else {
            log::error!(
                "Failed to destroy subscriber, unable to find subscriber by media_session_key {}",
                media_session_key
            )
        }
    }
    /// Remove a specific broken subscriber
    pub async fn remove_broken_subscriber(&mut self, media_session_key: &MediaSessionKey) {
        if let Some(subscriber) = self.subscribers.remove(media_session_key) {
            if let Err(e) = subscriber.destroy(true).await {
                log::error!("Failed to remove broken subscriber, {}", e);
            }
        } else {
            log::error!(
                "Failed to destroy broken subscriber, unable to find subscriber by media_session_key {}",
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
    pub fn destroy(&mut self) -> impl Future<Output = ()> + 'static {
        let id = self.id;
        let subscribers: Vec<_> = self.subscribers.drain().collect();
        let publishers: Vec<_> = self.publishers.drain().collect();

        async move {
            for (_, subscriber) in subscribers {
                log::debug!("Destroy subscriber {}", id);
                if let Err(e) = subscriber.destroy(false).await {
                    log::error!("Failed to destroy subscriber, {}", e);
                }
            }

            for (_, publisher) in publishers {
                log::debug!("Destroy publisher {}", id);
                if let Err(e) = publisher.destroy().await {
                    log::error!("Failed to destroy publisher, {}", e);
                }
            }

            log::debug!("Destroyed all sessions");
        }
    }
}
