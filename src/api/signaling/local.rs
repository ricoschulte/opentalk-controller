use super::ParticipantId;
use crate::api::signaling::mcu::MediaSessionType;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::sync::mpsc;

/// All Room specific events
#[derive(Debug)]
pub enum RoomEvent {
    /// This participant joined the room.
    ///
    /// Contains the participant state
    Joined(Participant),

    /// A room member with the given participant id left / has been removed
    Left(ParticipantId),

    /// A room member with the given participant id has updated its state
    Update(ParticipantId),
}

pub enum MemberKind {
    User(i64),
    Guest,
}

/// A collection of room members without additional state
pub struct Room {
    pub members: Vec<RoomMember>,
}

/// A Member inside a room, holds state information and handle to notify
/// associated tasks of room events
pub struct RoomMember {
    /// The participant state
    pub participant: Participant,

    pub kind: MemberKind,

    /// The sender that is linked to the WS Endpoint to receive room specific events
    pub sender: mpsc::Sender<RoomEvent>,
}

impl RoomMember {
    /// Send a room event to the associated task which handles interfacing with the frontend
    pub async fn send(&self, event: RoomEvent) {
        if let Err(e) = self.sender.send(event).await {
            log::error!(
                "Failed to send room event to RoomMember({}): {}",
                self.participant.id,
                e
            );
        }
    }
}

impl Room {
    /// Add a member to the room and notify all other members
    pub async fn add_member(
        &mut self,
        id: ParticipantId,
        name: String,
        kind: MemberKind,
        sender: mpsc::Sender<RoomEvent>,
    ) {
        if self.members.iter().any(|m| m.participant.id == id) {
            log::warn!("Participant {} tried to join more than once", id);
            return;
        }

        let participant = Participant::new(id, name);

        for member in &self.members {
            member.send(RoomEvent::Joined(participant.clone())).await;
        }

        self.members.push(RoomMember {
            participant,
            kind,
            sender,
        });
    }

    /// Remove a member from the room and notify everyone
    pub async fn remove_member(&mut self, id: ParticipantId) {
        let index = self
            .members
            .iter()
            .position(|member| member.participant.id == id);

        if let Some(index) = index {
            self.members.remove(index);

            for member in &self.members {
                member.send(RoomEvent::Left(id)).await;
            }
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Participant {
    pub id: ParticipantId,
    pub display_name: String,

    /// TODO v2: Do we want this data structure?
    /// {
    ///     publishing: {
    ///         "video": {
    ///             "video": true,
    ///             "audio": true,
    ///         },
    /// or unmuted streams
    ///         "screen": ["video"],
    ///     }
    /// }
    // TODO v4: Other kinds of streams like Audio only or Screen without audio.
    pub publishing: HashMap<MediaSessionType, MediaSessionState>,
}

/// State of streams of a single SDP session/WebRTCStream
#[derive(Default, Debug, Copy, Clone, Deserialize, Serialize)]
pub struct MediaSessionState {
    /// Audio is enabled (unmuted)
    pub audio: bool,
    /// Video is enabled (unmuted)
    pub video: bool,
}

impl Participant {
    pub fn new(id: ParticipantId, name: String) -> Self {
        Self {
            id,
            display_name: name,
            publishing: Default::default(),
        }
    }
}
