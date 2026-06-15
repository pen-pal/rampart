//! Optional self-observability: Rampart exports its OWN traces + logs via
//! OTLP/HTTP when `RAMPART_OTLP_ENDPOINT` is set. In the full-stack example it
//! points at Rampart itself, so the dashboard shows real, live data from the
//! running app instead of fabricated samples.
//!
//! Feedback control: a telemetry system ingesting its own telemetry can loop
//! (every export POST is an inbound request → more spans/logs → more exports).
//! Two guards break it: the HTTP `make_span` skips the ingest routes
//! (`/otlp`, `/rum`, `/api`) so their spans are never created, and the
//! export layers carry a filter that drops the HTTP-plumbing targets
//! (`hyper`, `reqwest`, `h2`, `tower_http`) and OpenTelemetry's own logs.

use opentelemetry::trace::TracerProvider as _;
use opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge;
use opentelemetry_otlp::{LogExporter, SpanExporter, WithExportConfig};
use opentelemetry_sdk::logs::SdkLoggerProvider;
use opentelemetry_sdk::trace::SdkTracerProvider;
use opentelemetry_sdk::Resource;
use tracing_subscriber::{filter::EnvFilter, Layer};

/// Held by `main` for the process lifetime; flushes on shutdown.
pub struct Guards {
    tracer: SdkTracerProvider,
    logger: SdkLoggerProvider,
}

impl Guards {
    /// Flush + stop the batch exporters. Best-effort.
    pub fn shutdown(self) {
        let _ = self.tracer.shutdown();
        let _ = self.logger.shutdown();
    }
}

/// Targets that are pure export plumbing — exporting them would feed the loop
/// and bury the app's own signal. App logs (`rampart*`) still flow.
fn export_filter() -> EnvFilter {
    EnvFilter::new("info,hyper=off,h2=off,reqwest=off,tower_http=warn,opentelemetry=off")
}

/// Build OTLP/HTTP trace + log exporters pointed at `endpoint` (a base URL like
/// `http://rampart:3000`). Returns the boxed tracing layers to add to the
/// subscriber, plus the guards to keep alive.
type BoxLayer<S> = Box<dyn Layer<S> + Send + Sync>;

pub fn build<S>(endpoint: &str) -> anyhow::Result<(Vec<BoxLayer<S>>, Guards)>
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a> + Send + Sync,
{
    let base = endpoint.trim_end_matches('/');
    let resource = Resource::builder().with_service_name("rampart").build();

    // with_endpoint is used verbatim by the HTTP exporter (no signal-path
    // append), so pass the full ingest URLs.
    let span_exporter = SpanExporter::builder()
        .with_http()
        .with_endpoint(format!("{base}/otlp/v1/traces"))
        .build()?;
    let tracer_provider = SdkTracerProvider::builder()
        .with_batch_exporter(span_exporter)
        .with_resource(resource.clone())
        .build();

    let log_exporter = LogExporter::builder()
        .with_http()
        .with_endpoint(format!("{base}/otlp/v1/logs"))
        .build()?;
    let logger_provider = SdkLoggerProvider::builder()
        .with_batch_exporter(log_exporter)
        .with_resource(resource)
        .build();

    let trace_layer = tracing_opentelemetry::layer()
        .with_tracer(tracer_provider.tracer("rampart"))
        .with_filter(export_filter())
        .boxed();
    let log_layer = OpenTelemetryTracingBridge::new(&logger_provider)
        .with_filter(export_filter())
        .boxed();

    Ok((
        vec![trace_layer, log_layer],
        Guards {
            tracer: tracer_provider,
            logger: logger_provider,
        },
    ))
}
