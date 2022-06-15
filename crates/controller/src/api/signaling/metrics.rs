use opentelemetry::metrics::ValueRecorder;
use opentelemetry::Key;

const STARTUP_SUCCESSFUL: Key = Key::from_static_str("successful");
const DESTROY_SUCCESSFUL: Key = Key::from_static_str("successful");

pub struct SignalingMetrics {
    pub(crate) runner_startup_time: ValueRecorder<f64>,
    pub(crate) runner_destroy_time: ValueRecorder<f64>,
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
}
