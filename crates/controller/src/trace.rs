use crate::settings::Logging;
use anyhow::Result;
use opentelemetry::global;
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
