use serde::Deserialize;

use super::RecordingId;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case", tag = "action")]
pub enum Message {
    Start,
    Stop(Stop),
    SetConsent(SetConsent),
}

#[derive(Debug, Deserialize)]
pub struct Stop {
    pub recording_id: RecordingId,
}

#[derive(Debug, Deserialize)]
pub struct SetConsent {
    pub consent: bool,
}
