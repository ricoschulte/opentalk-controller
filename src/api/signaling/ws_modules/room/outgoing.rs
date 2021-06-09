use crate::api::signaling::local::Participant;
use crate::api::signaling::mcu::MediaSessionType;
use crate::api::signaling::ParticipantId;
use janus_client::TrickleCandidate;
use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(tag = "message")]
pub enum Message {
    #[serde(rename = "hello")]
    JoinSuccess(JoinSuccess),

    /// State change of this participant
    #[serde(rename = "update")]
    Update(Participant),
    /// A participant that joined the room
    #[serde(rename = "joined")]
    Joined(Participant),
    /// This participant left the room
    #[serde(rename = "left")]
    Left(AssociatedParticipant),

    /// SDP Offer, starts publishing
    #[serde(rename = "offer")]
    Offer(Sdp<String>),
    /// SDP Answer, starts subscribing
    #[serde(rename = "answer")]
    Answer(Sdp<String>),
    /// SDP Candidate, used for ICE negotiation
    #[serde(rename = "candidate")]
    Candidate(Sdp<TrickleCandidate>),

    #[serde(rename = "error")]
    Error { text: &'static str },
}

#[derive(Debug, Serialize)]
pub struct JoinSuccess {
    pub id: ParticipantId,
    pub participants: Vec<Participant>,
}

#[derive(Debug, Serialize)]
pub struct AssociatedParticipant {
    pub id: ParticipantId,
}

#[derive(Debug, Serialize)]
pub struct Sdp<P> {
    /// The payload of the sdp message
    pub payload: P,

    /// The source of this SDP message.
    pub source: ParticipantId,

    /// The type of stream
    pub media_session_type: MediaSessionType,
}
