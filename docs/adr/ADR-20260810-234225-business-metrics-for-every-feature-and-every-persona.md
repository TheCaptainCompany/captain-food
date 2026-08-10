# ADR-20260810-234225 — Business metrics for every feature and every persona, developed with the test and the code

- **Status**: Accepted
- **Date**: 2026-08-10
- **Source**: product-owner directive (verbatim below)
- **Realized by**: [PROP-20260810-234225 "Business metrics for every feature and every persona"](../proposals/PROP-20260810-234225-business-metrics-for-every-persona.md) · [#484 "26 of the 29 declared `business_metrics` emit nothing…"](https://github.com/TheCaptainCompany/captain-food/issues/484)

---

## The directive

> *"Principle: Follow Jeff Patton about the business metrics during the analysis must be developed
> with the test and the code, we must have business metrics for all features for each persona. It's
> the only way that will allow us to know the usage of the product."*
>
> *"I let you define and implement them all the business metrics for every features for every
> persona."*
>
> — product owner, 2026-08-10

## Decision

**Every feature, for every persona, carries at least one business metric, and a business metric is
not done until it is declared, emitted and asserted by a test.** The three land in the same change,
exactly as a command lands with its event, its error, its rule and its test.

Four things follow, and they are the decision:

1. **The unit is the persona ACTIVITY, not the story step.** `specs/stories.yaml` is the
   persona × activity × step map: 8 personas, **25 activities**, 144 steps. Patton's backbone is the
   activity, and "feature × persona" is the activity. A step is an *operation call*, not an
   *outcome* — `SeeCheckoutBreakdown` and `CompareWithUberEats` both `$ref`
   `api.yaml#/queries/cart` (`specs/stories.yaml:57-58`) and `PollOperationStatus` fires ~30 times
   per checkout. Measuring per step would mint a metric for a poll loop and two for one query.

2. **Declaration is enforced exactly like ADR-0032; emission is not.** See below — this is the part
   that is easy to get wrong, and getting it wrong is how the current state happened.

3. **A metric declares the QUESTION it answers.** "Know the usage of the product" is a decision
   need, not a data need. A metric that answers no decision is cardinality, cost and attention
   with no return, and it is refused at the gate.

4. **Attributes are bounded sets, never entity ids.** Ids belong on spans (which already carry
   `business.order_id`, `business.correlation_id`); a metric dimension keyed by `restaurant_id` or
   `customer_id` is a Honeycomb bill and, under GDPR, a data-minimisation question we do not need
   to have.

## Is ADR-0032 the right precedent?

**For the declaration half: yes, identically. For the emission half: no, and saying otherwise would
reproduce the exact failure this ADR exists to stop.**

ADR-0032 works because *both sides of the obligation are declarations in the DSL*. Rules ↔ tests,
commands/events/errors ↔ tests, api operations ↔ story steps — the validator owns all of it and can
compute both directions from data it fully sees. So the coverage half of this principle — every
activity is measured, every metric binds a real activity — is an ADR-0032 rule in every respect and
is built as one.

The emission half is a different kind of claim. A rule is satisfied by an *assertion*; a metric is
satisfied by a *runtime behaviour on the real path*. `make validate` cannot see a call site. If the
principle is recorded as "metrics are like rules" and stops there, the result is a fully green
validator over a catalog of metrics that fire nowhere.

**That is not a hypothesis. It is the measured state of the repository on `168fd77`:**

| | |
|---|---|
| `business_metrics` entries declared in `specs/observability.yaml` | **29** across 14 contracts (20 distinct names) |
| Entries with **zero occurrences** anywhere in `crates/`, `tools/`, `deploy/` | **26** |
| Entries emitted on a real path | **3** (`orders_placed_total`, `checkout_payment_failures_total`, `scope_membership_lag_positions`) |
| Contracts covered by the emitted-ness gate (`tools/codegen-rs/src/tests.rs:1500`) | **3 of 14**, and it asserts only that the *name exists as a string constant* |

The slot existed, was used 29 times, and is 90% fiction — and the gate that was supposed to notice
covers 2 business metrics out of 29, in the weaker direction. Declaring metrics ahead of emission has
already been tried at this scale; this ADR does not repeat it at 5× the size.

So the obligation is enforced in three layers, and the layer is chosen by what can actually reach it
(the enforcement hierarchy of PROP-20260802-130500 §1, ADR-20260803-234035 *compiler first*):

| Property | Reached by | Mechanism |
|---|---|---|
| Every activity is measured; every metric binds a real activity; every metric states its question | **Validator** | bidirectional rules, ERROR severity |
| The metric name, its attribute names, types and arity at every call site | **Compiler** | instruments **generated** from the catalog into `crates/telemetry/src/generated/`; a rename or an attribute change is a compile error everywhere |
| ≥1 real emission site, firing exactly once, never on a replay | **Behaviour test** | the `InMemoryMetricExporter` spy already proven at `crates/infrastructure/tests/orders_placed_metric.rs:129` |

**No source-text scanner is added.** The third property is behavioural: no cross-crate type-level
construct reaches "this `pub fn` is called at least once from the real path" (a `pub` item in a
library is never dead code, and a distributed-slice registration inside the generated function is
linked whether or not anyone calls it). A behavioural property is proved by a behaviour test — which
is precisely what *"developed with the test and the code"* asks for. The naming half of the existing
scanner is **deleted** by generating the instruments, per ADR-20260803-234035.

## Sequencing: structurally true now, backfilled proportionately

The principle becomes **structurally true immediately**: the catalog, the four validator rules and
the generated instruments land first, so **nothing new can land unmeasured**.

The backfill of the 25 existing activities is **value-stream ordered, not a single sweep**, and the
debt is made *countable* rather than invisible: an enumerated `unmeasured:` waiver list in the
catalog, every entry naming the issue that will remove it, monotone-shrinking and validator-enforced.
A warning would not do — this repo carries a drifting warning baseline (43 on 2026-08-08) that CLAUDE.md
explicitly says to re-measure rather than trust, so a warning is invisible by construction.

Order: `customer/OrderFood` first (the paid-order path, and `orders_placed_total` already emits) →
restaurant-manager order operations (the un-accepted-order failure mode) → `public_user/BrowseForFood`
(the conversion funnel; the ETA is the product) → rider → admin → `restaurant_sync`.

Rejected: the big-bang backfill. It is cheaper to review in one pass and it would produce consistent
naming — but there is **no production and no users**, so most declarations would be unfalsifiable for
weeks, and the first time anyone learns a metric is the wrong metric is the moment it is expensive to
change. Twenty-six dead declarations are the receipt for that method.

## Consequences

- `specs/business_metrics.yaml` becomes a first-class catalog; `specs/observability.yaml` keeps only
  the technical `metrics`. The split the file's own header already asserts ("technical vs business
  signals (kept separate — see BAM)") becomes structural instead of typographic.
- No stored shape changes: metric names are telemetry dimensions, not `domain_events` payloads. **No
  event-versioning story is owed** (Young's immutability rule does not apply here).
- The `asserted_by:` link from a metric to its emission test depends on
  [#212 "ADR-0032 completeness cannot reach projectors or read guards — the behaviour-test DSL is
  command-shaped"](https://github.com/TheCaptainCompany/captain-food/issues/212) (decided
  2026-07-28, unbuilt): `specs/tests.yaml` is actor/command-shaped and cannot yet express "this
  metric fires when this event is appended". Until it can, the emission proof is a **convention with
  two working examples**, not a gate — stated here so nobody reads slice 1 as closing the loop.
- Instrumentation stays out of the domain (ADR-0012): every emit site is a framework boundary.
- This ADR does **not** define the metrics themselves. The per-persona, per-activity grid is the
  `ux-designer` lens's deliverable; this ADR owns the principle, the declaration mechanism and the
  enforcement.

## Refs

- `specs/observability.yaml:26` — "metrics/business_metrics : technical vs business signals (kept separate — see BAM)"
- `tools/codegen-rs/src/tests.rs:1500` — the 3-of-14 feature allowlist
- `tools/codegen-rs/src/validate/core.rs:738-820` — the story-map completeness rules this mirrors
- `crates/infrastructure/tests/orders_placed_metric.rs:129` — the emission-proof pattern
- [ADR-0032](0032-business-rules-and-completeness-gates.md) · [ADR-0012](0012-domain-infra-observability-separation.md) · [ADR-20260803-234035](ADR-20260803-234035-compiler-first-a-check-is-the-fallback.md) · [ADR-20260808-235113](ADR-20260808-235113-final-vision-first-no-intermediate-steps.md)
- [ADR-20260808-144738 "Product ownership lives in the team; no product-manager agent, ever"](ADR-20260808-144738-product-ownership-lives-in-the-team-no-pm-agent.md) — evidence displaces proxy judgment; this is the machinery that produces the evidence
- [#400 "Epic: reality-sensing infrastructure — agents closer to customers, mission metrics as contracts"](https://github.com/TheCaptainCompany/captain-food/issues/400) — parent epic, scopes 1–2
