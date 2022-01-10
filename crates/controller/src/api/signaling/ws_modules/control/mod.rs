//! Control Module Stub
//!
//! Actual control 'module' code can be found inside `crate::api::signaling::ws::runner`
use crate::Timestamp;
use serde::{Deserialize, Serialize};

pub mod incoming;
pub mod outgoing;
pub mod rabbitmq;
pub mod storage;

pub const NAMESPACE: &str = "control";

/// Control module's FrontendData
#[derive(Debug, Serialize)]
pub struct ControlData {
    pub display_name: String,
    pub participation_kind: ParticipationKind,
    pub hand_is_up: bool,
    pub joined_at: Timestamp,
    pub left_at: Option<Timestamp>,
    pub hand_updated_at: Timestamp,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParticipationKind {
    User,
    Guest,
    Sip,
}

impl_to_redis_args_se!(ParticipationKind);
impl_from_redis_value_de!(ParticipationKind);
