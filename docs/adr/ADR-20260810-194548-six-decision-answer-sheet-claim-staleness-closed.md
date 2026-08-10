# ADR-20260810-194548 — The six-decision answer sheet: claim staleness closed, `currency_mismatch` approved, the local test gate gets an owner

- **Status**: accepted (product owner, interactive decision artifact, 2026-08-10)
- **Context**: the six open rows carried by [docs/proposals/DECISIONS.md](../proposals/DECISIONS.md) §22/§25 after the [#451 "cart.current returns the session's priced cart"](https://github.com/TheCaptainCompany/captain-food/issues/451) keystone merged as [PR #460](https://github.com/TheCaptainCompany/captain-food/pull/460)
- **Method**: the interactive decision artifact (DECISIONS.md "How to decide" §4) — the documented return path is: record here and in the register with VERBATIM quotes, run every "Let's discuss" item through the standing specialist lenses, and close in the same session

## The four approved as recommended

Verbatim answer on each of the four cards: **"Approve as recommended"**.

1. **451-B — `currency_mismatch` joins the `cart-price` canonical reason set.** A currency clash is
   folded into `PriceUnresolvable` at `crates/application/src/pricing.rs:44` and then labelled
   `offer_gone` at `crates/server/src/graphql/cart_read.rs:136`, sending an on-call responder to the
   catalog for a monetary defect. **Approved ⇒ the spec window is open**: the change is one line in
   `specs/observability.yaml` (the reason set at `:271-273` gains
   `currency_mismatch — the line's offer resolves but its currency does not match the cart's`), plus
   the label at `cart_read.rs:136` selecting it. Lands under this recorded approval exactly as #451
   Phase 1 did — no further arbitration.
2. **451-C — #451 is retitled.** Already executed; it now reads *"cart.current returns the session's
   priced cart: the read seam + money-free Cart fold (#429 keystone; authenticated leg deferred to
   #469)"*. The title now describes what merged: the session leg and the seam, with the
   authenticated leg on [#469](https://github.com/TheCaptainCompany/captain-food/issues/469).
3. **The `from:` naming collision** — `from:` is about to mean the screens input-source key (§1 F)
   and api.yaml scope-binding. Recommendation (c) — **the team picks and records it** — approved, so
   the team now owes the pick. It must land **before both DSLs ship the key**; after that it stops
   being a rename and becomes a migration.
4. **Geocoding vs postal-code zones** — recommendation (c), **team first, bring a proposal**,
   approved. The row is no longer unowned: the team owns the analysis and returns with a proposal,
   rather than the question sitting in the register waiting for a product-owner answer it never
   needed.

## Claim staleness (§6.4) — CLOSED, and a process lesson first

Product owner, verbatim:

> "The legal should have an answer and the business expert should know what competitors is doing so
> I'm surprised there no recommendation"

**They were right, and the process failure is recorded before the answer.** The card was escalated
with no recommendation on a question two standing lenses could answer from published sources. The
rule this earns: **consult the standing lenses before escalating a card** — a question a lens can
answer is not a decision, which is the same discipline
[ADR-20260808-144738](ADR-20260808-144738-product-ownership-lives-in-the-team-no-pm-agent.md)
already states for evidence. Escalating a lens-answerable question spends the scarcest thing in the
project on something the team owned.

Both lenses then converged.

**Legal — none of (a)/(b)/(c); take (d).** TTL is not the legal object; a **provably-exercised
revocation path** is. GDPR is silent on token lifetime. The duty lands on **Art. 32(1)(d)** (a
process for regularly testing the effectiveness of measures) plus **Art. 5(2)** accountability, with
**Art. 12(3)**'s one-month outer bound on responding to an erasure request. A live access token over
erased data is defensible **inside that month** provided refresh tokens are revoked and
`app_metadata` is scrubbed *together*. Counsel would defend ≤1h access tokens; would not defend days.
**Riders are a different regime**: under the **Platform Work Directive (EU) 2024/2831 Arts. 7–11**
(transposition ~Dec 2026 — **VERIFY-FIRST**, this date must be re-checked before it is relied on),
revoking a rider's claim restricts access to work and requires a stated reason, a log, and human
review — so riders get **explicit revocation with a reason code**, never silent TTL drift. Access-log
retention **6–12 months** per **CNIL délibération 2021-122**; those logs are themselves personal data
and owe an entry in the **Art. 30** register.

**Business — (b), role-differentiated**, and the decisive fact is that the window exists whether or
not we decide: **Supabase already mints 1h access tokens with claims stamped at mint**. Option (a)
pays to shorten what the IdP gives for free; option (c) lands on one hour by accident. The real split
is **device vs person**, not role vs role — the restaurant tablet's actual risk is a departed staff
member, which no TTL fixes. Peak cost of re-derivation is currently theoretical (~80 orders/hour,
under 1 req/s). **Churn asymmetry decides it**: a forced re-auth on the acceptance terminal at 19:45
on a Friday blocks the only surface that accepts orders.

**Decision.** Keep the **~1h Supabase default**; make **revocation explicit and immediate** for rider
deactivation and staff removal; and make [#194](https://github.com/TheCaptainCompany/captain-food/issues/194)
erasure scrub `app_metadata` **and** revoke refresh tokens in the same act. The register row is
**closed**, not deferred.

Carried out of this decision: legal's four counsel questions join the counsel packet, and legal's
**unrelated blocker stands** — **no DPIA, no privacy notice and no terms of service exist**, which is
a launch precondition and not a backlog item.

## #474 — the local test gate

Product owner, verbatim:

> "It's a big issue. Does we have at least unit tests by crates? Do we cut the system in crates to
> allow us to test part of the system without building everything?"

**Both answers are yes, measured**: 990 tests across 182 test binaries, 34s warm for the full
workspace; `cargo test -p application` is **324 tests in 0.04s, linking 9 crates**. The crate split
already delivers exactly the per-part testability the question asks about, and CI already runs both
the workspace pass and the Postgres pass. **The hole is local-only**: `make rust` runs
`cargo test --manifest-path tools/codegen-rs/Cargo.toml` — the codegen crate alone — so a green local
gate proves the validator, the emitters and drift, and nothing about `crates/**`.

**The design (beck), which supersedes the (a)/(b) framing the card carried**:

- `make rust` stays the **fast spec gate**; `make test-crates` = `DB_TESTS_REQUIRED=1 cargo test
  --workspace`, **invoked from `.claude/hooks/stop-gate.sh`** rather than merely documented, whenever
  the diff touches `migrations/**`, `crates/**` or the emitters. In this repo the contributor is an
  agent and the Stop hook IS its muscle memory; documentation is not a gate. **Diff-scoping decides
  whether the DB half is mandatory — never whether it silently vanishes.**
- **Invert the skip polarity**: DB tests required by default, with an explicit opt-out that prints a
  summary line naming what was skipped.
- **Two tests that do not exist**, in priority order: (1) **the checkpoint must not advance on a fold
  error** — seed an event whose upsert violates the constraint, drain once, assert the checkpoint did
  not move and the drain returned `Err`, distinguishing DB errors (poison/halt) from payload-shape
  skips (which legitimately advance); (2) a **no-DB writer/schema agreement check inside `make
  validate`** — every `NOT NULL` column without a `DEFAULT` must appear in its writer's insert list —
  which catches the exact #474 shape in under a second, in the gate that already runs.
- **The evidence bar, binding**: plant the #474 migration itself and watch each new gate go **red**
  before it lands. Otherwise #474 ships the same unverified provenance claim this branch was already
  corrected for twice.

## Consequences

- The register drops from ten open rows to **five**: four answered here, claim staleness closed, and
  451-A closed by the #460 merge. The `from:` and geocoding rows change owner (team) rather than
  closing.
- `specs/observability.yaml` has one approved, unstarted change (451-B) — the next session may land
  it without re-asking.
- Two obligations leave this ADR and must not be lost: **DPIA / privacy notice / terms do not exist**
  (legal blocker), and the **Platform Work Directive transposition date is VERIFY-FIRST**.
