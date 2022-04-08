use opentelemetry::metrics::ValueRecorder;

pub struct KustosMetrics {
    pub enforce_execution_time: ValueRecorder<f64>,
    pub load_policy_execution_time: ValueRecorder<f64>,
}
