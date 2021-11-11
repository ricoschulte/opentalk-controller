use actix_web::dev::{ServiceRequest, ServiceResponse};
use actix_web::http::header::USER_AGENT;
use actix_web::Error;
use anyhow::Result;
use controller_shared::settings::Logging;
use opentelemetry::global;
use tracing::Span;
use tracing_actix_web::RootSpanBuilder;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Registry};

pub fn init(settings: &Logging) -> Result<()> {
    // Layer which acts as filter of traces and spans.
    // The filter is created from enviroment (RUST_LOG) and config file
    let mut filter = EnvFilter::from_default_env();

    for directive in &settings.default_directives {
        filter = filter.add_directive(directive.parse()?);
    }

    // FMT layer prints the trace events into stdout
    let fmt = tracing_subscriber::fmt::Layer::default();

    // Create registry which contains all layers
    let registry = Registry::default().with(filter).with(fmt);

    // If opentelemetry is enabled install that layer
    if settings.enable_opentelemetry {
        global::set_text_map_propagator(opentelemetry_jaeger::Propagator::new());

        let tracer = opentelemetry_jaeger::new_pipeline()
            .with_service_name(settings.service_name.clone())
            // We encountered a size limitation for our spans. Setting this to 1.000.000 for now
            .with_max_packet_size(1_000_000)
            .install_batch(opentelemetry::runtime::TokioCurrentThread)?;

        let telemetry = tracing_opentelemetry::layer().with_tracer(tracer);

        // Initialize the global logging with telemetry
        registry.with(telemetry).init();
    } else {
        // Install the global logger
        registry.init();
    }

    Ok(())
}

/// Flush remaining spans and traces
pub async fn destroy() {
    let handle = tokio::runtime::Handle::current();

    if handle
        .spawn_blocking(global::shutdown_tracer_provider)
        .await
        .is_err()
    {
        eprintln!(
            "Failed to shutdown opentelemetry tracer provider, some information might be missing"
        );
    }
}

pub struct ReducedSpanBuilder;

impl RootSpanBuilder for ReducedSpanBuilder {
    fn on_request_start(request: &ServiceRequest) -> Span {
        create_span(request)
    }

    fn on_request_end<B>(span: Span, outcome: &Result<ServiceResponse<B>, Error>) {
        match &outcome {
            Ok(response) => {
                if let Some(error) = response.response().error() {
                    handle_error(span, error)
                } else {
                    span.record("http.status_code", &response.response().status().as_u16());
                    span.record("otel.status_code", &"OK");
                }
            }
            Err(error) => handle_error(span, error),
        };
    }
}

fn handle_error(span: Span, error: &Error) {
    let response_error = error.as_response_error();
    span.record(
        "exception.message",
        &tracing::field::display(response_error),
    );
    span.record("exception.details", &tracing::field::debug(response_error));
    let status_code = response_error.status_code();
    span.record("http.status_code", &status_code.as_u16());

    if status_code.is_client_error() {
        span.record("otel.status_code", &"OK");
    } else {
        span.record("otel.status_code", &"ERROR");
    }
}

fn create_span(request: &ServiceRequest) -> Span {
    let user_agent = request
        .headers()
        .get(USER_AGENT)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");

    let http_route: std::borrow::Cow<'static, str> = request
        .match_pattern()
        .map(Into::into)
        .unwrap_or_else(|| "default".into());

    let connection_info = request.connection_info();
    let span = tracing::info_span!(
        "HTTP request",
        http.method = %request.method().as_str(),
        http.route = %http_route,
        http.flavor = ?request.version(),
        http.scheme = %connection_info.scheme(),
        http.host = %connection_info.host(),
        http.user_agent = %user_agent,
        http.target = %request.uri(),
        http.status_code = tracing::field::Empty,
        otel.kind = "server",
        otel.status_code = tracing::field::Empty,
        trace_id = tracing::field::Empty,
        exception.message = tracing::field::Empty,
        // Not proper OpenTelemetry, but their terminology is fairly exception-centric
        exception.details = tracing::field::Empty,
    );

    span
}
