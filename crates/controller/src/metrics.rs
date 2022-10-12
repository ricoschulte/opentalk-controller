use crate::api::signaling::metrics::SignalingMetrics;
use crate::redis_wrapper::RedisMetrics;
use crate::settings::SharedSettingsActix;
use actix_http::body::BoxBody;
use actix_http::StatusCode;
use actix_web::dev::PeerAddr;
use actix_web::web::Data;
use actix_web::{get, HttpResponse};
use database::DatabaseMetrics;
use kustos::metrics::KustosMetrics;
use mail_worker_proto::MailTask;
use opentelemetry::metrics::{Counter, Descriptor, MetricsError, Unit, ValueRecorder};
use opentelemetry::sdk::export::metrics::{Aggregator, AggregatorSelector};
use opentelemetry::sdk::metrics::selectors::simple::Selector;
use opentelemetry::{global, Key};
use opentelemetry_prometheus::PrometheusExporter;
use prometheus::{Encoder, TextEncoder};
use std::sync::Arc;

const MAIL_TASK_KIND: Key = Key::from_static_str("mail_task_kind");

pub struct EndpointMetrics {
    pub(crate) request_durations: ValueRecorder<f64>,
    pub(crate) response_sizes: ValueRecorder<u64>,
    pub(crate) issued_email_tasks_count: Counter<u64>,
}

impl EndpointMetrics {
    pub fn increment_issued_email_tasks_count(&self, mail_task: &MailTask) {
        self.issued_email_tasks_count
            .add(1, &[MAIL_TASK_KIND.string(mail_task.as_kind_str())]);
    }
}

pub struct CombinedMetrics {
    exporter: PrometheusExporter,
    pub(super) endpoint: Arc<EndpointMetrics>,
    pub(super) signaling: Arc<SignalingMetrics>,
    pub(super) database: Arc<DatabaseMetrics>,
    pub(super) kustos: Arc<KustosMetrics>,
    pub(super) redis: Arc<RedisMetrics>,
}

/// Overrides the default OTel aggregation method
///
/// This is needed as currently it is not possible to say which aggregator is used for which meter.
/// The current implementation uses fixed boundaries for histograms
// Fixme when https://github.com/open-telemetry/opentelemetry-rust/issues/673 is resolved
#[derive(Debug)]
pub struct OverrideAggregatorSelector {
    fallback: Selector,
}

impl Default for OverrideAggregatorSelector {
    fn default() -> Self {
        Self {
            fallback: Selector::Histogram(vec![0.5, 0.9, 0.99]),
        }
    }
}

impl AggregatorSelector for OverrideAggregatorSelector {
    fn aggregator_for(&self, descriptor: &Descriptor) -> Option<Arc<dyn Aggregator + Send + Sync>> {
        use opentelemetry::sdk::metrics::aggregators;

        match descriptor.name() {
            "web.request_duration_seconds" => Some(Arc::new(aggregators::histogram(
                descriptor,
                &[0.005, 0.01, 0.25, 0.5, 1.0, 2.0],
            ))),
            "web.response_sizes_bytes" => Some(Arc::new(aggregators::histogram(
                descriptor,
                &[100.0, 1_000.0, 10_000.0, 100_000.0],
            ))),
            "signaling.runner_startup_time_seconds" | "signaling.runner_destroy_time_seconds" => {
                Some(Arc::new(aggregators::histogram(
                    descriptor,
                    &[0.01, 0.25, 0.5, 1.0, 2.0, 5.0],
                )))
            }
            "sql.execution_time_seconds" => Some(Arc::new(aggregators::histogram(
                descriptor,
                &[0.01, 0.05, 0.1, 0.25, 0.5],
            ))),
            "sql.dbpool_connections" => Some(Arc::new(aggregators::last_value())),
            "sql.dbpool_connections_idle" => Some(Arc::new(aggregators::last_value())),
            "kustos.enforce_execution_time_seconds" => Some(Arc::new(aggregators::histogram(
                descriptor,
                &[0.01, 0.05, 0.1, 0.25, 0.5],
            ))),
            "kustos.load_policy_execution_time_seconds" => Some(Arc::new(aggregators::histogram(
                descriptor,
                &[0.01, 0.05, 0.1, 0.25, 0.5],
            ))),
            "redis.command_execution_time_seconds" => Some(Arc::new(aggregators::histogram(
                descriptor,
                &[0.001, 0.0025, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5],
            ))),
            _ => self.fallback.aggregator_for(descriptor),
        }
    }
}

impl CombinedMetrics {
    pub fn init() -> Self {
        let exporter = opentelemetry_prometheus::exporter()
            .with_aggregator_selector(OverrideAggregatorSelector::default())
            .init();

        let meter = global::meter("ot-controller");

        let endpoint = Arc::new(EndpointMetrics {
            request_durations: meter
                .f64_value_recorder("web.request_duration_seconds")
                .with_description("HTTP response time measured in actix-web middleware")
                .with_unit(Unit::new("seconds"))
                .init(),
            response_sizes: meter
                .u64_value_recorder("web.response_sizes_bytes")
                .with_description(
                    "HTTP response size for sized responses measured in actix-web middleware",
                )
                .with_unit(Unit::new("bytes"))
                .init(),
            issued_email_tasks_count: meter
                .u64_counter("web.issued_email_tasks_count")
                .with_description("Number of issued email tasks")
                .init(),
        });

        let signaling = Arc::new(SignalingMetrics {
            runner_startup_time: meter
                .f64_value_recorder("signaling.runner_startup_time_seconds")
                .with_description("Time the runner takes to initialize")
                .with_unit(Unit::new("seconds"))
                .init(),
            runner_destroy_time: meter
                .f64_value_recorder("signaling.runner_destroy_time_seconds")
                .with_description("Time the runner takes to stop")
                .with_unit(Unit::new("seconds"))
                .init(),
            created_rooms_count: meter
                .u64_counter("signaling.created_rooms_count")
                .with_description("Number of created rooms")
                .init(),
            destroyed_rooms_count: meter
                .u64_counter("signaling.destroyed_rooms_count")
                .with_description("Number of destroyed rooms")
                .init(),
            participants_count: meter
                .i64_up_down_counter("signaling.participants_count")
                .with_description("Number of participants")
                .init(),
            participants_with_audio_count: meter
                .i64_up_down_counter("signaling.participants_with_audio_count")
                .with_description("Number of participants with audio unmuted")
                .init(),
            participants_with_video_count: meter
                .i64_up_down_counter("signaling.participants_with_video_count")
                .with_description("Number of participants with video unmuted")
                .init(),
        });

        let database = Arc::new(DatabaseMetrics {
            sql_execution_time: meter
                .f64_value_recorder("sql.execution_time_seconds")
                .with_description("SQL execution time for a single diesel query")
                .with_unit(Unit::new("seconds"))
                .init(),
            sql_error: meter
                .u64_counter("sql.errors_total")
                .with_description("Counter for total SQL query errors")
                .init(),
            dbpool_connections: meter
                .u64_value_recorder("sql.dbpool_connections")
                .with_description("Number of currently non-idling db connections")
                .init(),
            dbpool_connections_idle: meter
                .u64_value_recorder("sql.dbpool_connections_idle")
                .with_description("Number of currently idling db connections")
                .init(),
        });

        let kustos = Arc::new(KustosMetrics {
            enforce_execution_time: meter
                .f64_value_recorder("kustos.enforce_execution_time_seconds")
                .with_description("Execution time of kustos enforce")
                .with_unit(Unit::new("seconds"))
                .init(),
            load_policy_execution_time: meter
                .f64_value_recorder("kustos.load_policy_execution_time_seconds")
                .with_description("Execution time of kustos load_policy")
                .with_unit(Unit::new("seconds"))
                .init(),
        });

        let redis = Arc::new(RedisMetrics {
            command_execution_time: meter
                .f64_value_recorder("redis.command_execution_time_seconds")
                .with_description("Execution time of redis commands in seconds")
                .with_unit(Unit::new("seconds"))
                .init(),
        });

        Self {
            exporter,
            endpoint,
            signaling,
            database,
            kustos,
            redis,
        }
    }
}

#[get("/metrics")]
pub async fn metrics(
    settings: SharedSettingsActix,
    PeerAddr(peer_addr): PeerAddr,
    metrics: Data<CombinedMetrics>,
) -> HttpResponse {
    let settings = settings.load_full();

    let allowed = &settings
        .metrics
        .allowlist
        .iter()
        .any(|allowed_net| allowed_net.contains(&peer_addr.ip()));

    if !allowed {
        return HttpResponse::new(StatusCode::FORBIDDEN);
    }

    let encoder = TextEncoder::new();
    let metric_families = metrics.exporter.registry().gather();
    let mut buf = Vec::new();
    if let Err(err) = encoder.encode(&metric_families[..], &mut buf) {
        global::handle_error(MetricsError::Other(err.to_string()));
        return HttpResponse::new(StatusCode::INTERNAL_SERVER_ERROR);
    }

    let response = String::from_utf8(buf).unwrap_or_default();

    HttpResponse::with_body(StatusCode::OK, BoxBody::new(response))
}
