//! #745 / PR #748 checkpoint (BLOCKING, found independently by the observability and graphql
//! lenses): the skip-by-design explainability event at `hosts.rs` was `tracing::debug!` behind
//! the registry-global EnvFilter, and production pins `LOG_LEVEL=info`
//! (specs/common/configuration.yaml) — so the event reached neither the JSON logs nor OTLP under
//! deployed defaults, while the observability contract promises the skip reasons "feed the
//! render's TRACE". The exact wired-never-to-scream class.
//!
//! This test drives PRODUCTION's own render path (`host_root` → `app_page` → the checkout screen,
//! whose `paymentStatus.byOrder` is a declared §25b structural skip) under a subscriber built with
//! the SAME default filter string `telemetry::init` derives from `LOG_LEVEL=info`, and asserts
//! the skip event comes OUT. Seen RED against the `debug!` form: zero matching lines captured.
//!
//! Its own test binary (the `checkout_degraded_metric.rs` precedent): the subscriber is installed
//! per-thread (`with_default`), so it needs the render future polled on THIS thread — a manual
//! current-thread runtime inside the closure, not `#[tokio::test]`.

use std::io::Write;
use std::sync::{Arc, Mutex};

use axum::extract::Extension;
use axum::http::{header::HOST, HeaderMap, HeaderValue, Uri};
use server::graphql_schema::build_schema;
use server::{host_root, SsrExec, TenantLookup};
use tracing_subscriber::layer::SubscriberExt;

/// A `MakeWriter` capturing everything the fmt layer emits.
#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl Capture {
    fn contents(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().unwrap()).into_owned()
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Capture {
    type Writer = CaptureWriter;
    fn make_writer(&'a self) -> Self::Writer {
        CaptureWriter(self.0.clone())
    }
}

struct CaptureWriter(Arc<Mutex<Vec<u8>>>);

impl Write for CaptureWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// The skip event survives the DEPLOYED default filter. The filter string mirrors
/// `telemetry::init`'s derivation for `LOG_LEVEL=info` (the pinned production/staging value) —
/// asserting against the real gate the event must pass, not against a permissive test subscriber.
#[test]
fn the_skip_event_is_visible_under_the_deployed_info_default() {
    let capture = Capture::default();
    let filter = tracing_subscriber::EnvFilter::new(
        "info,hyper=warn,h2=warn,tower=warn,sqlx=warn,reqwest=warn,rustls=warn,opentelemetry=warn",
    );
    let subscriber = tracing_subscriber::registry().with(filter).with(
        tracing_subscriber::fmt::layer().json().flatten_event(true).with_writer(capture.clone()),
    );

    tracing::subscriber::with_default(subscriber, || {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async {
            let exec =
                SsrExec { schema: build_schema(None, None, None), stripe_publishable_key: None };
            let mut headers = HeaderMap::new();
            headers.insert(HOST, HeaderValue::from_static("chez-test.captain.food"));
            let _ = host_root(
                Extension(TenantLookup(None)),
                Extension(exec),
                headers,
                Uri::from_static("/checkout"),
            )
            .await;
        });
    });

    let out = capture.contents();
    let skip_lines: Vec<&str> =
        out.lines().filter(|l| l.contains("sdui read skipped by design")).collect();
    assert!(
        !skip_lines.is_empty(),
        "the skip-by-design event must be VISIBLE under LOG_LEVEL=info — a debug-gated event is \
         a promise the trace never keeps under deployed defaults. Captured output:\n{out}"
    );
    // The checkout paint emits one line per skip (me.profile is a role_refused skip on the
    // anonymous transport); the #745 fix's own line is the STRUCTURAL one — find it and check it
    // carries everything a trace query needs.
    let structural = skip_lines
        .iter()
        .find(|l| l.contains("structurally_unfulfillable"))
        .unwrap_or_else(|| panic!("a structurally_unfulfillable skip line must be emitted: {skip_lines:?}"));
    for needle in ["paymentStatus.byOrder", "correlation_id", "checkout"] {
        assert!(structural.contains(needle), "skip line must carry `{needle}`: {structural}");
    }
}
