//! OpenTelemetry emission + structured logging for Captain.Food (issue #191).
//!
//! `specs/observability.yaml` declares required spans, run identities, attributes, metrics and SLOs for
//! eleven critical workflows. Before this crate existed **none of it was emitted**: there was no OTel
//! dependency, no subscriber, and logging was 69 `println!` calls — so `correlation_id` and `trace_id`,
//! mandatory in every contract's `run_identity`, were unobtainable at runtime. This crate is the layer
//! that makes those contracts true.
//!
//! ## Where instrumentation is allowed to live
//!
//! Only at FRAMEWORK boundaries — the command bus, event store, publisher, consumers, projections, saga
//! runner, GraphQL gateway and middleware. `specs/architecture/c4-l3.yaml` is the authority via its
//! `instrumented` flags, and `command-handlers` is explicitly `instrumented: false`. Consequently
//! neither `domain` nor `application` depends on this crate, and a workspace test enforces that rather
//! than trusting review to notice. The practical reason is not purity: an aggregate that imports a
//! telemetry SDK becomes untestable without a subscriber, and its decisions start being shaped by what
//! is convenient to trace.
//!
//! ## Backend
//!
//! Honeycomb over OTLP/HTTP-protobuf, pinned to the **EU (`eu1`)** region. That pin is a GDPR
//! requirement, not a default: ADR-0042 chose Frankfurt for compute and data, and these spans carry
//! `customerId` / `orderId`, which are personal data. The endpoint is supplied programmatically from the
//! declared configuration, never read from `OTEL_EXPORTER_OTLP_*` — configuration reaches this app
//! through the generated reader (`specs/configuration.yaml`) or not at all.
//!
//! ## Degradation
//!
//! No telemetry setting can stop the app. With no ingest key the exporter is not constructed and the
//! process keeps emitting structured JSON logs to stdout; if the exporter *fails to build*, that is
//! logged and the app continues the same way. Refusing to serve orders because the system cannot
//! describe itself would be a self-inflicted outage — whereas a missing payment secret genuinely must
//! stop the boot, which is why that one is `required:` in the spec and these are not.

use std::collections::HashMap;
use std::time::Duration;

use opentelemetry::trace::TracerProvider as _;
use opentelemetry::KeyValue;
use opentelemetry_sdk::metrics::SdkMeterProvider;
use opentelemetry_sdk::trace::{Sampler, SdkTracerProvider};
use opentelemetry_sdk::Resource;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

pub mod contract;
mod http_client;
pub mod meters;
pub mod spans;

pub use contract::{attr, dispatch_outcome, journal_status, metric, span};

/// The `business.channel` value for the GraphQL dispatch surface — `scalars.yaml`
/// `InboundMessageChannel`.
/// Named here so the generated resolvers do not each carry a bare string literal.
pub const CHANNEL_GRAPHQL: &str = "GRAPHQL";
/// `business.channel` for on-app workers enqueueing through a typed actor client.
pub const CHANNEL_WORKER: &str = "WORKER";

/// Everything this crate needs, read from the generated configuration reader by the composition root.
///
/// Deliberately plain owned data rather than a borrow of the server's `Config`: this crate sits below
/// `server` in the dependency graph and must not know that type exists.
#[derive(Debug, Clone)]
pub struct TelemetryConfig {
    /// Honeycomb INGEST key (`x-honeycomb-team`). `None` disables export — logs still flow.
    pub api_key: Option<String>,
    /// OTLP base endpoint, e.g. `https://api.eu1.honeycomb.io`. The signal path is appended here.
    pub endpoint: String,
    /// Honeycomb dataset, also reported as the `service.name` resource attribute.
    pub dataset: String,
    /// Head-sampling probability in [0, 1]; parent-respecting.
    pub sample_ratio: f64,
    /// Minimum log severity (`trace|debug|info|warn|error`).
    pub log_level: String,
    /// Build identity (short git SHA) → `service.version`, so a span can be tied to a deploy.
    pub service_version: String,
    /// `development` | `staging` | `production` → `deployment.environment.name`.
    pub profile: String,
}

/// Holds the providers alive and flushes them on drop.
///
/// This matters more than it looks. Span batches are buffered, so a process that exits without a
/// flush loses precisely the spans describing the shutdown — which is the window a crash or a failed
/// deploy lives in. The composition root must keep this guard for the lifetime of the process.
#[must_use = "dropping the guard immediately shuts telemetry down again; bind it for the process lifetime"]
pub struct TelemetryGuard {
    tracer_provider: Option<SdkTracerProvider>,
    meter_provider: Option<SdkMeterProvider>,
}

impl TelemetryGuard {
    /// Flush and shut down. Called by `Drop`, exposed so a graceful SIGTERM path can flush BEFORE the
    /// runtime is torn down (on Render, SIGTERM → drain → exit is the normal deploy path).
    pub fn shutdown(&mut self) {
        if let Some(p) = self.tracer_provider.take() {
            if let Err(e) = p.shutdown() {
                // Deliberately `eprintln!`: the subscriber is being dismantled, so `tracing` may no
                // longer have anywhere to go. This is the one place a raw write is the correct tool.
                eprintln!("telemetry: tracer shutdown failed: {e}");
            }
        }
        if let Some(p) = self.meter_provider.take() {
            if let Err(e) = p.shutdown() {
                eprintln!("telemetry: meter shutdown failed: {e}");
            }
        }
    }
}

impl Drop for TelemetryGuard {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Whether export is active, for the boot report. The report must distinguish "exporting" from
/// "logging only" — an operator who believes traces are flowing when they are not will waste the first
/// ten minutes of an incident in the Honeycomb UI looking for data that was never sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Emission {
    /// Spans and metrics are being exported to Honeycomb.
    Exporting,
    /// No ingest key: structured logs only, spans built and dropped.
    LogsOnly,
    /// A key was present but the exporter could not be built (bad endpoint, TLS, etc).
    ExporterUnavailable,
}

impl Emission {
    pub fn as_str(self) -> &'static str {
        match self {
            Emission::Exporting => "exporting",
            Emission::LogsOnly => "logs-only",
            Emission::ExporterUnavailable => "exporter-unavailable",
        }
    }
}

/// Install the global subscriber: structured JSON logs always, OTLP export when a key is configured.
///
/// Returns the guard plus what actually happened, so the caller can say so in the boot report instead of
/// letting the operator assume.
pub fn init(cfg: &TelemetryConfig) -> (TelemetryGuard, Emission) {
    // `RUST_LOG` still wins when explicitly set — an incident sometimes needs a filter far more precise
    // than a level (`LOG_LEVEL` is a closed set on purpose). Otherwise the declared level applies, with
    // the noisy dependencies pinned down so `debug` on our own code stays readable.
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        let lvl = cfg.log_level.to_ascii_lowercase();
        EnvFilter::new(format!(
            "{lvl},hyper=warn,h2=warn,tower=warn,sqlx=warn,reqwest=warn,rustls=warn,opentelemetry=warn"
        ))
    });

    // JSON to stdout: Render captures stdout, and a machine-parseable line is the difference between
    // grepping and querying. `flatten_event` keeps fields at the top level so `correlation_id` is a
    // first-class field rather than nested under `fields`.
    let fmt_layer = tracing_subscriber::fmt::layer()
        .json()
        .flatten_event(true)
        .with_current_span(true)
        .with_span_list(false)
        .with_target(true);

    let resource = Resource::builder()
        // service.name is what Honeycomb's Environments & Services model keys on.
        .with_service_name(cfg.dataset.clone())
        .with_attributes([
            KeyValue::new("service.version", cfg.service_version.clone()),
            KeyValue::new("deployment.environment.name", cfg.profile.clone()),
        ])
        .build();

    let Some(key) = cfg.api_key.clone().filter(|k| !k.is_empty()) else {
        tracing_subscriber::registry().with(filter).with(fmt_layer).init();
        tracing::info!(
            emission = Emission::LogsOnly.as_str(),
            "telemetry: no HONEYCOMB_API_KEY -- structured logs only, no OTLP export"
        );
        return (TelemetryGuard { tracer_provider: None, meter_provider: None }, Emission::LogsOnly);
    };

    match build_providers(cfg, &key, resource) {
        Ok((tracer_provider, meter_provider)) => {
            let tracer = tracer_provider.tracer("captain-food");
            let otel_layer = tracing_opentelemetry::layer().with_tracer(tracer);
            let metrics_layer =
                tracing_opentelemetry::MetricsLayer::new(meter_provider.clone());

            // Set the globals so `opentelemetry::global::meter(...)` works from the instrumented
            // boundaries without threading a provider handle through every constructor.
            opentelemetry::global::set_tracer_provider(tracer_provider.clone());
            opentelemetry::global::set_meter_provider(meter_provider.clone());

            tracing_subscriber::registry()
                .with(filter)
                .with(fmt_layer)
                .with(otel_layer)
                .with(metrics_layer)
                .init();

            tracing::info!(
                emission = Emission::Exporting.as_str(),
                endpoint = %cfg.endpoint,
                dataset = %cfg.dataset,
                sample_ratio = cfg.sample_ratio,
                "telemetry: exporting OTLP to Honeycomb"
            );
            (
                TelemetryGuard {
                    tracer_provider: Some(tracer_provider),
                    meter_provider: Some(meter_provider),
                },
                Emission::Exporting,
            )
        }
        Err(e) => {
            // A telemetry failure must not be a boot failure — but it must be LOUD, because the
            // alternative is an operator who thinks the absence of traces means the absence of traffic.
            tracing_subscriber::registry().with(filter).with(fmt_layer).init();
            tracing::error!(
                error = %e,
                emission = Emission::ExporterUnavailable.as_str(),
                endpoint = %cfg.endpoint,
                "telemetry: OTLP exporter could not be built -- continuing with logs only"
            );
            (
                TelemetryGuard { tracer_provider: None, meter_provider: None },
                Emission::ExporterUnavailable,
            )
        }
    }
}

/// Build the trace + metric providers against Honeycomb.
fn build_providers(
    cfg: &TelemetryConfig,
    key: &str,
    resource: Resource,
) -> Result<(SdkTracerProvider, SdkMeterProvider), Box<dyn std::error::Error + Send + Sync>> {
    use opentelemetry_otlp::{WithExportConfig, WithHttpConfig};

    let mut headers = HashMap::new();
    headers.insert("x-honeycomb-team".to_string(), key.to_string());
    // Required for METRICS (Honeycomb routes metrics by dataset); ignored for traces in an
    // Environments & Services team, where `service.name` decides. Harmless to send in both.
    headers.insert("x-honeycomb-dataset".to_string(), cfg.dataset.clone());

    let client = http_client::HoneycombHttpClient::new()?;

    let span_exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_http()
        // The signal path MUST be appended by us. `resolve_http_endpoint` uses a programmatically
        // supplied endpoint VERBATIM (only the env-var form appends `/v1/traces`), so passing the bare
        // host here would POST to `/` and fail as an opaque 404 with no spans and no obvious cause.
        .with_endpoint(signal_endpoint(&cfg.endpoint, "/v1/traces"))
        .with_headers(headers.clone())
        .with_http_client(client.clone())
        .with_timeout(Duration::from_secs(10))
        .build()?;

    let tracer_provider = SdkTracerProvider::builder()
        .with_batch_exporter(span_exporter)
        .with_resource(resource.clone())
        // ParentBased: once a trace is sampled at its root, every child follows. Sampling children
        // independently produces the worst possible artifact — a trace with holes, which reads as a
        // missing span (a contract violation) rather than as a sampling decision.
        .with_sampler(Sampler::ParentBased(Box::new(Sampler::TraceIdRatioBased(
            cfg.sample_ratio,
        ))))
        .build();

    let metric_exporter = opentelemetry_otlp::MetricExporter::builder()
        .with_http()
        .with_endpoint(signal_endpoint(&cfg.endpoint, "/v1/metrics"))
        .with_headers(headers)
        .with_http_client(client)
        .with_timeout(Duration::from_secs(10))
        .build()?;

    let meter_provider = SdkMeterProvider::builder()
        .with_periodic_exporter(metric_exporter)
        .with_resource(resource)
        .build();

    Ok((tracer_provider, meter_provider))
}

/// Join a base OTLP endpoint and a signal path without doubling or dropping the separating slash.
///
/// Trailing slashes come from humans and from dashboards, and `https://api.eu1.honeycomb.io//v1/traces`
/// is not the same URL as the correct one.
fn signal_endpoint(base: &str, path: &str) -> String {
    format!("{}{}", base.trim_end_matches('/'), path)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The signal path is appended exactly once, whatever the operator pasted. A doubled slash 404s
    /// against Honeycomb, and the symptom (no traces) points nowhere near the cause.
    #[test]
    fn signal_endpoint_appends_the_path_exactly_once() {
        for base in ["https://api.eu1.honeycomb.io", "https://api.eu1.honeycomb.io/"] {
            assert_eq!(
                signal_endpoint(base, "/v1/traces"),
                "https://api.eu1.honeycomb.io/v1/traces"
            );
        }
        assert_eq!(
            signal_endpoint("https://api.eu1.honeycomb.io///", "/v1/metrics"),
            "https://api.eu1.honeycomb.io/v1/metrics"
        );
    }

    /// The default endpoint must stay in the EU. This is a GDPR guard, not a style test: these spans
    /// carry customer and order ids, ADR-0042 pinned data to Frankfurt, and a well-meaning "use the
    /// documented default" edit would move personal data to the US as a silent side effect.
    #[test]
    fn the_declared_default_endpoint_is_the_eu_region() {
        // Telemetry keys are platform keys, so they live in the KERNEL configuration fragment
        // (specs/common/configuration.yaml — ADR-20260807-183024 D5).
        let declared = include_str!("../../../specs/common/configuration.yaml");
        let block = declared
            .split("HONEYCOMB_API_ENDPOINT:")
            .nth(1)
            .expect("HONEYCOMB_API_ENDPOINT is declared in specs/configuration.yaml");
        let default_line = block
            .lines()
            .find(|l| l.trim_start().starts_with("default:"))
            .expect("it declares a default");
        assert!(
            default_line.contains("api.eu1.honeycomb.io"),
            "the OTLP default must be the EU region (ADR-0042 / GDPR), got: {default_line}"
        );
        assert!(
            !default_line.contains("//api.honeycomb.io"),
            "api.honeycomb.io is the US host -- spans carry personal data"
        );
    }

    /// `LogsOnly` is a distinct, reportable state. If it rendered the same as `Exporting`, an operator
    /// would open Honeycomb during an incident and read "no data" as "no traffic".
    #[test]
    fn emission_states_are_distinguishable_in_the_boot_report() {
        let rendered: Vec<&str> =
            [Emission::Exporting, Emission::LogsOnly, Emission::ExporterUnavailable]
                .iter()
                .map(|e| e.as_str())
                .collect();
        assert_eq!(rendered, vec!["exporting", "logs-only", "exporter-unavailable"]);
    }
}
