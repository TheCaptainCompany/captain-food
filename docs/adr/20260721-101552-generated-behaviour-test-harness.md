# ADR-20260721-101552 — Generated behaviour-test harness from tests.yaml

## Status

Accepted

## Context

`specs/tests.yaml` is validator-enforced as complete (every command/event/error exercised, every
rule asserted — ADR-0032), yet every Given/When/Then case was hand-mirrored into
`crates/application/tests/*_behaviour.rs`: a pure translation step paid on every new case, and a
standing drift risk the validator could not see — a hand-mirrored test can silently assert less
than its spec (and several did). Codegen-roadmap item 2 / issue #24; #23's generated handlers and
#25's generated orchestrator legs want a generated harness to prove themselves against.

## Decision

1. **Emit the suite from the spec** (`emit_behaviour_tests`, tools/codegen-rs): one
   `#[tokio::test]` per tests.yaml case, generated into
   `crates/application/src/generated/behaviour_tests.rs` (`#[cfg(test)]` in the generated module
   index). GIVEN seeds each fixture onto its aggregate's stream; WHEN dispatches the
   command/event through the real write path; THEN asserts the appended facts across ALL streams
   equal the spec payloads (strict per-stream diff — `then: []` asserts a strict no-op);
   `thrown` asserts the typed rejection code is one the spec lists AND that nothing was appended.
   New tests.yaml cases now cost zero Rust.
2. **Emitter-owned dispatch tables**: command → handler signature (ports per handler),
   (process-manager, event) → wrapper leg, and aggregate ← event → record path. The emitter also
   owns the aggregate table (stream category, id property, uuid-vs-string key) and derives
   event→stream routing from actors.yaml, with per-test id context (an event that does not carry
   its aggregate's id — e.g. `RefundApproved` — keys off the test's given, else the fixture
   pool's unique id).
3. **Hand-written harness runtime** `application::behaviour_support` (`#[cfg(test)]`): the
   in-memory event store, read-model/service doubles (fed by the seeded given facts + a canned
   OrderTracking baseline from the fixture pool), PM-run-row seeding mirroring the legs'
   state-sets, and the deterministic spec-id → UUIDv5 mapping (delivery-job ids mirror the
   dispatch PM's own `delivery_job_id_for` derivation). Playbook rule unchanged: a failing
   behaviour test means fixing this runtime or the emitter — never the spec's intent or the
   generated file.
4. **Spec data must be executable**: a new validator rule `test-invalid-enum-value` rejects
   sample values not in their enum (it caught `serviceType: "PICKUP"`). Executing the spec also
   exposed inconsistencies that were corrected in tests.yaml (see the issue-#24 PR for the full
   list): givens missing the aggregate's birth fact (catalog cases) or required cross-aggregate
   facts (active restaurant for checkout, READY order for the dispatch close leg), `then`
   fixtures that could not equal the handler's output (per-leg RefundOpened variants, catalog
   line items lacking `productId`/`offerName`, the listing-claim proof), refund legs that DO open
   a refund while the spec said `then: []`, and the money chain normalized to the V0 pricing
   policy (total = articles, zero fee/split legs — pricing.rs; the ADR-0016/0017 fee policy will
   update spec and code together when it lands).
5. **Runtime fixes surfaced by the executable spec** (fix-the-runtime, never the test):
   `RegisterRestaurant` now enforces `RestaurantAccountNotFound` by folding the account stream
   (was a TODO the hand suite silently skipped), and `ReportDeliveryIssue`/`ResolveDeliveryIssue`
   no longer stamp wall-clock time into business payloads (`reported_at`/`resolved_at` = None;
   the envelope's `occurred_at` owns time — ADR-0041).
6. **Hand-mirrored suite deleted**: the ten `crates/application/tests/*_behaviour.rs` files are
   removed (migration gate: the generated suite covers all 161 spec cases — the hand suite
   mirrored 118). The in-src PM tests and `pm_state_mem.rs` stay (they assert run-row/hook
   internals the spec deliberately does not).

## Alternatives considered

- Field-subset assertions (mirroring the hand suite's weaker asserts) — rejected: the issue's
  point is payload-level equality against the spec; weak asserts are the drift risk.
- Auto-inserting missing birth facts / read-model rows in the harness — rejected: it would mask
  spec incompleteness instead of surfacing it.
- A runtime interpreter reading tests.yaml at test time — rejected: generation keeps the suite
  reviewable, breakpointable and byte-diffed by the CI drift gate like every other artifact.

## Consequences

### Positive
- The spec IS the executable suite; spec↔test drift is structurally impossible.
- Coverage rose from 118 hand-mirrored cases to all 161; each case asserts full payload equality
  across every touched stream, plus strict no-op and no-side-effect guarantees.
- #23/#25 can prove generated handlers/legs against a generated harness.

### Negative
- A few hand-written extra arms (tenant-scoping probes, TODO-invariant placeholders) had no spec
  case and were lost with the hand files — re-adding one is now a tests.yaml entry, not Rust.
- `behaviour_support` mirrors leg state-sets and projection effects; when those change, the
  runtime doubles must follow (surfaced immediately by the failing generated suite).
- The spec's sample data is now load-bearing: inconsistent sample values fail the gate instead of
  rotting silently (this is the point, but it makes spec edits stricter).
