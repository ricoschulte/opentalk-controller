use crate::api;
use opentelemetry::metrics::{Counter, UpDownCounter, ValueRecorder};
use opentelemetry::Key;

const STARTUP_SUCCESSFUL: Key = Key::from_static_str("successful");
const DESTROY_SUCCESSFUL: Key = Key::from_static_str("successful");
const PARTICIPATION_KIND: Key = Key::from_static_str("participation_kind");

pub struct SignalingMetrics {
    pub(crate) runner_startup_time: ValueRecorder<f64>,
    pub(crate) runner_destroy_time: ValueRecorder<f64>,
    pub(crate) destroyed_rooms_count: Counter<u64>,
    pub(crate) participants_count: UpDownCounter<i64>,
}

impl SignalingMetrics {
    pub fn record_startup_time(&self, secs: f64, success: bool) {
        self.runner_startup_time
            .record(secs, &[STARTUP_SUCCESSFUL.bool(success)]);
    }

    pub fn record_destroy_time(&self, secs: f64, success: bool) {
        self.runner_destroy_time
            .record(secs, &[DESTROY_SUCCESSFUL.bool(success)]);
    }

    pub fn increment_destroyed_rooms_count(&self) {
        self.destroyed_rooms_count.add(1, &[]);
    }

    pub fn increment_participants_count<U>(&self, participant: &api::Participant<U>) {
        self.participants_count
            .add(1, &[PARTICIPATION_KIND.string(participant.as_kind_str())]);
    }

    pub fn decrement_participants_count<U>(&self, participant: &api::Participant<U>) {
        self.participants_count
            .add(-1, &[PARTICIPATION_KIND.string(participant.as_kind_str())]);
    }
}
