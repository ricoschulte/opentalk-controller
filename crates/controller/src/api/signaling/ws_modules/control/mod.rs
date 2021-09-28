use crate::Timestamp;
use serde::Serialize;

pub mod incoming;
pub mod outgoing;
pub mod rabbitmq;
pub mod storage;
// Control 'module' code can be found inside `crate::api::signaling::ws::runner`

/// Control module's FrontendData
#[derive(Debug, Serialize)]
pub struct ControlData {
    pub display_name: String,
    pub hand_is_up: bool,
    pub joined_at: Timestamp,
    pub left_at: Option<Timestamp>,
    pub hand_updated_at: Timestamp,
}
