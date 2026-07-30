# ADR-20260730-135741 — `live.captain.food` serves `204 No Content` until the bandwidth posture is fixed

## Status

Accepted (product-owner directive, 2026-07-30). **Temporary** — the removal condition is explicit:
delete the `HostRoute::Live` arm in `crates/server/src/hosts.rs` (and its behaviour test) once the
hosting/egress decision (Render pause + Supabase egress, see the follow-up below) is resolved.

## Context

Render is paused — outbound bandwidth exhausted (ADR-20260730-051500). Two traffic sinks fed that:

- `live.captain.food` (the marketplace, `captain_frontoffice`) renders its discovery screens with
  **data-full SSR**: every request runs the cross-restaurant reads against Supabase and ships the
  resulting HTML. A monitor pointed at that host therefore burned Render bandwidth (the HTML) *and*
  Supabase egress (the database reads) on every probe, around the clock, for a surface no customer
  is using yet.
- The keep-warm/uptime pinger. It is documented to target `/ping` (ADR-0043) — a 4-byte liveness
  answer — but was in practice pointed at the marketplace host.

The product owner switched the external monitor from GET to HEAD. **That does not stop the server
cost**: axum serves HEAD with the same handler as GET and strips only the response body, so a HEAD
of `live.captain.food` still executes the full SSR data resolution — the Supabase egress and render
CPU are spent; only the Render→client HTML bytes are saved. The fix must live server-side.

An audit of the repository confirms **no code monitors `live.captain.food`**: `prod-smoke` targets
`api.captain.food` and the `smoke-test` tenant host, `render-status` reads the Render API, and no
workflow or crate issues requests to the marketplace host. The only monitor is the external
UptimeRobot check, which is user-managed.

## Decision

1. `HostRoute::Live` answers **`204 No Content` on every path**, short-circuiting **before** SSR
   and data resolution (`crates/server/src/hosts.rs`). Explicit routes (`/health`, `/ping`,
   `/{role}/graphql`) still win over the fallback, so probes and the API are unaffected.
2. Monitoring/keep-warm targets **`/ping`** (any host), per ADR-0043 — never an SSR surface. The
   HEAD-vs-GET distinction is irrelevant to server cost and must not be relied on.
3. The other data-full SSR surfaces (`restos`, `riders`, tenant storefronts) are unchanged — they
   are the product. The marketplace spec (`specs/screens/captain_frontoffice.yaml`) is untouched:
   this is a runtime mitigation, not a product change.

## Alternatives considered

- **HEAD-only monitoring** (what was tried first) — saves only the response-body bytes; axum still
  runs the handler, so the dominant cost (DB reads/egress per probe) remains. Insufficient alone.
- **Cache/CDN the marketplace SSR output** — the right shape later (the discovery page is highly
  cacheable), but it is real work and still serves bytes; the immediate need is to stop the bleed
  and make remaining outbound usage attributable.
- **404/robots/rate-limit the host** — a 404 on a real product host is misleading state, and none
  of these stop a monitor's request from executing the handler.

## Consequences

### Positive
- Zero bytes and zero database reads for any request to the marketplace host, whatever the verb —
  remaining bandwidth/egress usage now points at other sources (the stated diagnostic goal).
- A behaviour test pins the mitigation (`the_live_host_answers_no_content_on_every_path`), so it
  cannot silently regress while it is policy — and its deletion marks the deliberate restore.

### Negative
- The marketplace surface is dark: any visitor to `live.captain.food` gets an empty 204. Acceptable
  while the surface has no traffic and Render is paused anyway.
- An uptime monitor pointed at the host will report "up" (2xx) without proving SSR works — which is
  exactly the point, but worth remembering when reading monitor history.

### Follow-up
- The hosting/egress decision (leave Render free tier? colocate app + Postgres so DB traffic stops
  being egress? keep Supabase for identity only) is being researched and belongs to the product
  owner; restoring the marketplace rides on it.
