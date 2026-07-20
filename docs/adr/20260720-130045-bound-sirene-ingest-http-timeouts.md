# ADR-20260720-130045 — Bound the SIRENE ingestion's outbound HTTP calls (per-request + job timeout)

## Status

Accepted

## Context

The weekly `sirene-sync` GitHub Actions job (`crates/sirene_ingest`, ADR-0045) ran for the full
6-hour GitHub job ceiling and was force-`cancelled` twice in a row (runs on 2026-07-18 and the
2026-07-20 03:00-UTC scheduled sweep). The build step finished in ~40s; the hang was entirely in the
`Run the SIRENE ingestion (--once)` step (06:22 → 12:21 UTC).

Root cause: `SireneClient` built its HTTP client with a bare `reqwest::Client::new()`, which has **no
request timeout**. If the INSEE Sirene API opens a connection but stalls on the read, `fetch_page`'s
`.send().await` blocks forever, freezing the department sweep until GitHub cancels the job. The
worker-ping path in `main.rs` already set an explicit 30s timeout; the fetch path did not. A hung run
also silently burns ~6h of CI minutes and never wakes the on-app `sync_sirene_worker`.

## Decision

Bound every outbound call so a stalled peer fails a page instead of the whole job:

1. **Per-request timeout on the client** (the real fix): construct `SireneClient`'s `reqwest::Client`
   via the builder with `timeout(60s)` + `connect_timeout(15s)`. 60s never trips a healthy slow page
   (INSEE pages return in seconds) but bounds a dead socket; the existing retry loop and per-department
   failure isolation then handle the error path (re-runs are idempotent UPSERTs).
2. **Job `timeout-minutes: 90` on the workflow** (defense in depth): a full France sweep is minutes, so
   90 never trips a healthy run, but any future unforeseen hang dies in ≤90min instead of 6h.

Both changes are code/CI only — no `specs/**` or generated artifact is touched.

## Alternatives considered

- **Only add the workflow `timeout-minutes`** — would stop the 6h burn but still fail every sweep on a
  single stalled request, and never drain staged rows. Treats the symptom, not the cause.
- **Wrap each `fetch_page` in `tokio::time::timeout`** — equivalent bound but more code and no
  connect-phase distinction; the client-level builder timeout is the idiomatic reqwest mechanism.

## Consequences

### Positive
- A stalled INSEE connection now fails a single page (retried, then the department is isolated) instead
  of hanging the entire scheduled job.
- CI can no longer burn 6 hours on a hung sweep; the job is bounded to 90 minutes.

### Negative
- A genuinely slow (>60s) legitimate page would now error; acceptable given INSEE's documented latency
  and the idempotent re-run design.

### Follow-up actions
- Watch the next scheduled `sirene-sync` run (Monday 03:00 UTC) for a clean success/exit.
