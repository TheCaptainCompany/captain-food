//! The OTLP transport, backed by the workspace's own `reqwest` 0.12 + `native-tls` client.
//!
//! `opentelemetry-otlp` can bring its own client, but every option it offers is wrong here:
//!
//! - `reqwest-client` declares `reqwest ^0.13` with `default-features = false`, so it arrives with **no
//!   TLS backend at all** — every export to an `https://` endpoint fails, and it also puts a second
//!   reqwest major version in the tree.
//! - `reqwest-rustls` fixes TLS by pulling `ring`, which the root `Cargo.toml` avoids on purpose:
//!   native-tls maps to SChannel on Windows dev boxes and OpenSSL on Render's build image, neither
//!   needing a C toolchain.
//! - `reqwest-blocking-client` (the crate default) would block a Tokio worker thread on export.
//!
//! Implementing the trait over the house client is ~20 lines and sidesteps all three.

use async_trait::async_trait;
use bytes::Bytes;
use http::{Request, Response};
use opentelemetry_http::{HttpClient, HttpError};

/// An `HttpClient` over `reqwest::Client`. Cheap to clone (reqwest clones share the connection pool),
/// so the trace and metric exporters share one pool rather than opening two.
#[derive(Debug, Clone)]
pub struct HoneycombHttpClient {
    inner: reqwest::Client,
}

impl HoneycombHttpClient {
    pub fn new() -> Result<Self, reqwest::Error> {
        let inner = reqwest::Client::builder()
            // Export must never outlive the batch interval it belongs to: a hung POST would otherwise
            // pin a batch and let the queue grow behind it, turning a telemetry outage into memory
            // pressure on a 512Mi instance that has already been OOM-killed four times (#105/#106/
            // #107/#123). The exporter applies its own timeout too; this is the transport-level floor.
            .timeout(std::time::Duration::from_secs(10))
            .connect_timeout(std::time::Duration::from_secs(5))
            .build()?;
        Ok(Self { inner })
    }
}

#[async_trait]
impl HttpClient for HoneycombHttpClient {
    async fn send_bytes(&self, request: Request<Bytes>) -> Result<Response<Bytes>, HttpError> {
        let request = reqwest::Request::try_from(request)?;
        let mut response = self.inner.execute(request).await?.error_for_status()?;
        let headers = std::mem::take(response.headers_mut());
        let mut out = Response::builder().status(response.status()).body(response.bytes().await?)?;
        *out.headers_mut() = headers;
        Ok(out)
    }
}
