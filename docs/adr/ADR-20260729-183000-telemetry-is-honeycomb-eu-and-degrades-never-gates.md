# ADR-20260729-183000 — Telemetry is OTLP to Honeycomb, pinned to the EU region, and it degrades rather than gates

- **Status**: Accepted
- **Date**: 2026-07-29
- **Issue**: [#191 "Observability contracts are 100% unimplemented: no OpenTelemetry dependency, no tracing subscriber, 69 println! calls"](https://github.com/TheCaptainCompany/captain-food/issues/191)
- **Proposal**: [PROP-20260726-170500 — Runtime observability and scale readiness](../proposals/PROP-20260726-170500-runtime-observability-and-scale.md) (**D1** and **D2** answered here)
- **Completes**: [#16 "Observability: `surface: graphql` binding kind + generic command-acceptance contract"](https://github.com/TheCaptainCompany/captain-food/issues/16), which closed honestly deferring emission
- **Refines**: [ADR-0042](0042-hosting-and-region.md) (Frankfurt for compute *and* data), [ADR-0035](0035-project-structure-clean-architecture.md) (the crate layout this adds a leaf to)

## Context

`specs/observability.yaml` had grown to 898 lines of contracts — required spans, run identities,
attributes, metrics, status rules and SLOs across eleven critical workflows. **None of it was emitted.**
There was no `opentelemetry` or `tracing` dependency in the workspace, no subscriber anywhere in
`crates/`, and logging was 69 `println!`/`eprintln!` calls across the server, infrastructure and the
five partner adapters.

The practical consequence was not "we lack dashboards". `correlation_id` and `trace_id` are marked
**required** in every contract's `run_identity`, and neither existed at runtime. The write path is
acceptance-first (ADR-20260720-015500): a mutation journals synchronously, answers `PENDING`, and the
handler runs on a spawned task. So the interesting half of every command — the handler, the event
append, the Stripe call, the projection — happened with nothing tying it back to the request that
caused it. When a Friday-night order went wrong, the investigation was grepping unstructured stdout.

PROP-170500 named the systemic cause, and it is worth repeating rather than paraphrasing: **the
operating model rewards work that is spec-able.** Aggregates, events, rules and tests fit the DSL and
are excellent. Telemetry wiring does not fit it, and was correspondingly the weakest area. That is a
property of the machine, not a lapse of attention.

Two decisions were open in the proposal and blocked the work: **D1 the telemetry backend** (with a
recommendation of "hosted OTLP, EU region pinned" and an explicit note that the vendor choice was
unmade) and **D2 sampling**.

## Decision

### 1. The backend is Honeycomb, over OTLP/HTTP-protobuf — D1 answered

The product owner chose Honeycomb and provisioned `HONEYCOMB_API_KEY` as a repo Actions secret. This
records that choice. OTLP keeps it swappable: nothing above the exporter knows the vendor's name, so a
move to Grafana Cloud or Axiom is an endpoint and a header.

### 2. The region is `eu1`, and that is a GDPR constraint, not a default

Traces here carry `customerId` and `orderId`. Those are personal data, and ADR-0042 pinned compute *and*
data to Frankfurt precisely for GDPR. Exporting to `api.honeycomb.io` (US) would move personal data out
of the EU as an incidental side effect of a telemetry setting — the kind of transfer nobody decides and
everybody later has to explain.

So `HONEYCOMB_API_ENDPOINT` defaults to and is baked as `https://api.eu1.honeycomb.io`, and a unit test
asserts the declared default is the EU host. That test exists because the failure mode is a
well-intentioned edit: "use the documented default" is exactly how the US endpoint would get restored.

### 3. Telemetry DEGRADES; it never gates the boot

No telemetry key is `required:` in `specs/configuration.yaml`. With no ingest key the exporter is not
constructed and the process keeps emitting structured JSON logs; if the exporter fails to *build*, that
is logged loudly and the app continues the same way.

This is deliberately the opposite of how money and identity secrets behave. A missing
`STRIPE_WEBHOOK_SECRET` must stop the boot, because a captured payment that never reaches the domain is
the worst failure this product has. A missing telemetry key must not, because refusing to take orders in
order to protest that the system cannot describe itself is a self-inflicted outage.

The corollary is that "not exporting" must be **visible**. `Emission` is a three-state value —
`exporting` / `logs-only` / `exporter-unavailable` — reported at boot. An operator who believes traces
are flowing when they are not loses the first ten minutes of an incident inside the Honeycomb UI looking
for data that was never sent.

### 4. Sampling is parent-based head sampling at 1.0 — D2 answered, and narrowed

PROP-170500 D2 recommended **tail-based** sampling: keep 100% of errors and rejections, sample
successes. We are not implementing that, and the reason should be on the record rather than discovered
later from the code.

True tail sampling requires a collector that can buffer a whole trace before deciding — for Honeycomb
that is **Refinery**, a service to deploy, size and pay for. That is squarely against ADR-0042's
minimal-ops-pre-PMF posture, and it would be infrastructure added to support a decision whose own
justification says the volume is not there yet: *"volume at V0 is low enough that head-sampling would
mostly discard the interesting traces."*

So: `Sampler::ParentBased(TraceIdRatioBased(ratio))` with `OTEL_TRACES_SAMPLE_RATIO` baked at `1.0`.
Everything is kept. The ratio is the dial to turn down *if* ingest cost becomes real, and turning it
down before there is volume buys nothing while losing the traces that explain the incidents. Tail
sampling is revisited when the cost is measurable, and it is a separate decision with a separate cost.

`ParentBased` matters on its own: once a trace is sampled at its root every child follows. Sampling
children independently produces a trace with holes, which reads as a *missing required span* — a
contract violation — rather than as a sampling decision.

### 5. Instrumentation lives only at framework boundaries, and that is now enforced

`specs/architecture/c4-l3.yaml` is the authority through its `instrumented` flags, and
`command-handlers` is `instrumented: false`. Three layers, three different permissions:

| Layer | `opentelemetry*` / `crates/telemetry` | `tracing` facade |
|---|---|---|
| `domain` | no | **no** |
| `application` | no | **yes** |
| `infrastructure`, `server`, `crates/adapters/*` | yes | yes |

The line is: **`application` may say things; only boundaries may measure them.** Process managers and
handlers emit levelled, structured events — which inherit the ambient span opened by the saga runner or
the GraphQL dispatch, and therefore arrive already carrying `correlation_id` — but never open a span,
record a metric, or know that OpenTelemetry exists.

The alternative was leaving those call sites as `eprintln!`. That would have preserved exactly what
#191 exists to end: a saga leg skipping a Stripe refund, printed to stdout, with nothing connecting it
to the order it belongs to.

A dependency test enforces both halves. It is a test rather than a review rule because the failure is
silent: adding `telemetry` to `application` compiles, passes everything else, and quietly relocates
instrumentation into the layer the architecture exists to keep clean.

### 6. Contract conformance is a test, not a claim

A second test reads `specs/observability.yaml` and asserts that every **required** span, required
attribute and named metric of the `command-acceptance` and `place-order` contracts is actually
constructed in `crates/telemetry`. Scoped to those two because they are the ones #191's Definition of
Done names; the other nine contracts are not yet emitted, and asserting them would fail for work this
change does not claim to have done.

Both guards were validated by deliberately breaking them, which caught two vacuous passes worth
recording as a general lesson:

- The span-name check first searched the whole file, and **passed** when `command.journal` was renamed
  to `command.journalx` — because a `#[cfg(test)]` assertion still contained the old literal. A guard
  that a test can satisfy by asserting the very thing the guard verifies is worse than no guard.
- The attribute check first used a bare substring, and **passed** when `business.dispatch_outcome` was
  renamed to `business.dispatch_outcomeX`. Prefix matching is not name matching.

**A guard is not finished when it passes. It is finished when it has been seen to fail.**

## Consequences

**Good**

- The contracts are real for the two workflows that matter most, and a trace now spans the
  acceptance-first async boundary: `command.receive` → `command.journal` → `command.dispatch` → the
  spawned handler → `event.store.append` → `event.publish` → `event.consume.projection`.
- Logging is structured, levelled and correlated, and the worker lifecycle lines carry
  `worker`/`toggle` fields — the shape of output that would have made issue #220's four silent hours a
  single query.
- `checkout_payment_failures_total` answers "can customers pay right now" without reading a trace.
- The vendor is one endpoint and one header away from replaceable.

**Costs and things deliberately left open**

- **Nine contracts remain unemitted** (`refund`, `customer-identification`, `prospection`, the four
  webhook-ingestion contracts, `delivery-dispatch-strategy`, `reclamation-sla`, `sirene-sync`). The
  conformance test is scoped so this is explicit rather than implied.
- **`payment.intent.create` records `created`, not the contract's `captured`.** Capture arrives later as
  an inbound Stripe webhook fact. Conflating the two would make a created-but-never-captured payment
  look successful, which is the exact shape of "a paid order nobody was told about", so the span tells
  the narrower truth and the contract's success condition is not yet fully satisfied by this path.
- **Traces carrying `customerId`/`orderId` are personal data in a third-party system.** EU residency is
  handled; retention, and whether a GDPR erasure request must reach Honeycomb, are not. That belongs
  with the erasure work in PROP-170000 and is not resolved here.
- **`tracing-opentelemetry` adds a second span-context mechanism.** Spans are `tracing` spans bridged to
  OTel, so code that reaches for `opentelemetry::Context` directly will not see them.
- **One reqwest, by hand.** None of `opentelemetry-otlp`'s client features were usable:
  `reqwest-client` declares reqwest 0.13 with default features off, so it arrives with no TLS backend at
  all; `reqwest-rustls` pulls `ring`, which this workspace avoids on purpose; the crate default
  `reqwest-blocking-client` would block a Tokio worker on export. `HoneycombHttpClient` implements
  `opentelemetry_http::HttpClient` over the house reqwest 0.12 + native-tls client instead — ~20 lines,
  and it keeps one reqwest version in the tree rather than compiling two.
- **A programmatic OTLP endpoint is used verbatim** by the exporter; only the env-var form appends the
  signal path. `/v1/traces` is therefore appended explicitly. Worth writing down because the failure is
  a silent 404 with no spans and nothing pointing at the cause.

## Alternatives considered

| Option | Why not |
|---|---|
| Self-hosted collector + storage | EU residency by construction and full control, but another service to run and pay for — against ADR-0042's minimal-ops-pre-PMF stance |
| Structured JSON logs only, no traces | Cheapest, and a large improvement over `println!`. But no spans means no causality across the acceptance-first async boundary, which is exactly where the hard bugs live |
| US region | Simpler default, and wrong: it moves personal data out of the EU as a side effect of a telemetry setting |
| Refinery for true tail sampling | The correct end state; premature at zero volume, and it is infrastructure |
| Instrument aggregates directly | Contradicts `c4-l3.yaml`; and an aggregate that needs a subscriber to run cannot be unit-tested |
| Hand-write the acceptance spans into `mutation.rs` | It is generated. Hand edits are lost on the next `make generate`, and inlining the field list into ~100 resolvers lets a contract change land in some and not others |
