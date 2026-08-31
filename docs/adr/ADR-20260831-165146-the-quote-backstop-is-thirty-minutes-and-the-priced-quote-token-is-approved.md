# ADR-20260831-165146 — The quote's backstop is 30 minutes, and the priced quote token is approved

## Status

Accepted

Closes register row `QUOTE-STALENESS` (decided 2026-08-31) and approves
[PROP-20260831-134539 "The priced quote token: display and charge agree by construction"](../proposals/PROP-20260831-134539-priced-quote-token.md).
Both are founder decisions of 2026-08-31.

## Enforced by

n/a — no behavioral guarantee lands in this record; it is docs-only. The guarantees it *decides* are
owed by the build and are already located: `specs/ordering/rules.yaml#/ServerPriceAuthority` (whose
enforcement clause is rewritten in the same change as PROP §11 slice 4) and
`#/CheckoutPricesCartCreatesPaymentIntent` (extended by slice 1). The 30-minute backstop becomes a
rule with a test at slice 5, per ADR-0032.

## Context

[ADR-20260831-121957](ADR-20260831-121957-the-pm-read-step-is-retired-source-fixed-the-physics-and-left-the-ownership.md)
§4d recorded the founder's choice of the **mechanism** — build the priced quote token (row
`QUOTE-TOKEN`, decided 2026-08-31) — and deliberately deferred the design and one sub-question:

- the design landed as **PROP-20260831-134539** (2026-08-31), `Status: Proposed`, tracking
  [#816 "Display/charge divergence is undetected: the expectedTotal equality check never runs in production"](https://github.com/TheCaptainCompany/captain-food/issues/816);
- the sub-question stayed open as row `QUOTE-STALENESS`, which the team **priced rather than
  re-asked** (§7 of that proposal) because the row's own instruction said to.

Two things were therefore outstanding and neither could be closed by the team: **the number**, which
is a product decision wearing a number and both of whose directions cost real money, and **the
approval** of the design that carries it. Both were put to the founder on 2026-08-31 through
`AskUserQuestion`, each with a register-check trail, the option space, the trade-offs and a
recommendation. This record is those two answers.

## Decision

### 1. `QUOTE-STALENESS` — N = 30 minutes, as a backstop only. M is dropped.

Founder's answer, verbatim option label: **"30 minutes (recommended)"**. He took the `business`
lens's figure with its stated derivation.

**What the number is derived from** — and it is stated first, because a bare 30 is
unfalsifiable: the **p99 of the cart-to-pay leg with the mandatory SCA/3DS bank-app detour in it**.
The tail of that distribution is a customer bounced into a banking app, not a customer deliberating.
It is **not a risk setting**, and it is not sized from risk appetite: it is a backstop that
essentially never fires on a live session.

**Why an N exists at all** — the load-bearing context that was put to him and would be missing from
any later reading of the number: **carts never expire in this model**
(`specs/ordering/actors.yaml:15`, *"carts never expire, so there is no abandonment state"* —
re-verified at `9cd15c75` for this record; the dispatch that commissioned it carried the citation as
an `UNVERIFIED input` and it checks out), so **N is the only clock on the whole cart**. There is no
other bound anywhere on how stale a checkout may be.

**M (a catalog version count) is dropped.** The catalog stream also carries `OfferStockUpdated` from
POS callbacks (`specs/catalog/events.yaml:198`), so at a busy service it advances constantly for
reasons unrelated to price: any small M is a **100%-fire timer wearing a correctness costume**. The
version stays as the as-of anchor and for audit; the gate is on Δtotal.

**What it is not.** N is not what protects the price. Divergence is (PROP §3 F4 — charge the quoted
amount or refuse, never more; §3 F6 — fire on a non-zero delta, never on expiry). N only bounds how
old a quote may be before the write side insists on re-pricing.

### 2. `PROP-20260831-134539` — Approved, build it, slice 1 first

Founder's answer, verbatim option label: **"Approve — build it, slice 1 first"**. The proposal moves
to **Approved (2026-08-31)**. Slice 1 (HEAD orderability at checkout — the oversell guard that is
never called on the `PlaceOrder` path) was dispatched in parallel with this record.

### 3. The three surviving `Concerns` are re-expressed, not deleted

An unchecked `Concerns` entry mechanically blocks `Approved` (validator rule
`proposal-approved-unresolved-concern`), and the rule's own message says to resolve an entry **by
checking it with a one-line resolution, never by deleting it**. Two of the three could not be
discharged as written — one was a statement of fact that cannot become false before the code exists,
and one is a build-time gate that was never an approval-time gate. They are **relocated to the slice
they actually bind**, which is the honest form:

| Entry | As written | Where it now binds |
|---|---|---|
| `QUOTE-STALENESS` N undecided | blocks approval | **Genuinely discharged** by decision 1 above; the row is `decided`. The *caveat* survives inside that row: 30 is evidence-deferred and is re-derived after the first peak |
| `PlaceOrder` input change is non-additive | blocks approval | **Slice 4's gate.** It is `HOLD: human` and gets the team's independent reviewer pass; per [ADR-20260815-134655](ADR-20260815-134655-the-team-merges-its-own-work-no-pr-waits-on-founder-review.md) no PR waits on founder review, so this was never an approval gate. Written into §11 slice 4 as a blocking condition on that PR |
| the as-of fold's peak cost is a projection, not a measurement | blocks approval **forever** | **Slice 2's acceptance criterion.** The measurement is owed *when the code exists*; a fact about the absence of code cannot be discharged before the code is written. Written into §11 slice 2 as a Done-when |

Nothing was weakened and nothing was removed: each entry is now a condition on a **named PR** rather
than on the approval, where it can actually be observed to hold.

### 4. The reversal stays flagged

PROP-20260831-134539 reverses
[ADR-20260810-112836 "Cart priced LIVE on read"](ADR-20260810-112836-cart-priced-live-on-read.md)
**§2** in part, in two clauses: the **freeze locus** moves from commitment to quote time (materially
that record's rejected Alternative A), and the **enforcement clause** naming the `expectedTotal`
equality check is replaced outright, because that check never runs in production
(#816, `crates/application/src/commands.rs:2615`). Both clauses are in that record's §2 — a reading
that puts the second in §4 is wrong (its §4 is the `cart(id)` IDOR retirement).

That reversal went **unflagged in two records** until 2026-08-31. It is named here as well as in the
proposal's header (`Reverses in part`) and §2.4, deliberately: an approval is exactly the moment a
reversal gets re-buried, because the reader's attention is on the new design.

## Alternatives considered

**For N** (each was put to the founder with its cost):

- **Shorter, ~5 minutes or less** — fires on ordinary Friday-night sessions, and every firing is a
  reprice interstitial shown to a customer who did nothing wrong. Conversion is paid on **correct**
  sessions. Rejected.
- **30 minutes** — **CHOSEN.** Sized from the cart-to-pay p99 with SCA in it; essentially never
  fires on a live session.
- **Longer** — a quote outlives the service state it was priced in, which is the guarantee the token
  exists to protect. Rejected.
- **Divergence-gated with no backstop at all** — honest about the missing instrument (there is no
  quote-age measurement to derive N from), but with carts that never expire it leaves an
  **unbounded-age quote honourable**. Rejected.

**For the proposal**: approve and build (chosen, slice 1 first) · approve but re-sequence · send it
back for redesign · defer pending counsel. The last was already answered by the `legal` lens's own
return: the design is deliberately built **past** the open counsel questions (charge the quoted
amount or refuse, never more), which is what lets the epic ship without waiting on counsel that is
not engaged.

## Consequences

### Positive

- The build is unblocked, and #816 — a live money-path defect on a legally-constrained surface — has
  an approved design and a sequenced plan.
- The number now carries its derivation and its expiry condition in the register, so the next reader
  meets *"p99 of cart-to-pay with SCA, re-derive after the first peak"* rather than a bare 30.
- Two concerns that would have blocked the proposal permanently are now conditions on the PRs that
  can satisfy them, which is where a gate belongs.

### Negative

- **N is a judgement, and the record says so.** The instrument that would let it be derived —
  `quote_age_seconds` at PlaceOrder, PROP §9 C1 — does not exist
  (`grep -rn "quote|reprice" specs/observability.yaml` → 0 hits, 2026-08-31). Under
  [ADR-20260808-144738](ADR-20260808-144738-product-ownership-lives-in-the-team-no-pm-agent.md)
  decision 5 (*evidence displaces proxy judgment*) this is an **evidence-deferred** decision, and the
  risk is the ordinary one: an evidence-deferred number that nobody returns to becomes a constant by
  neglect.
- **Approving the design does not resolve what it depends on.** The restaurant-facing half (a
  restaurant held to a withdrawn price for N minutes) stays unbuildable until the funds posture
  resolves — `QT-8`/`QT-9` in
  [BRIEF-20260831](../legal/BRIEF-20260831-repricing-and-price-quote-counsel-packet.md), absorbed
  into BRIEF-20260818 §3(c) Q10. The customer-facing half is safe under both postures.
- Approval closes a door named in the proposal's own §12: after this the checkout no longer reads
  the catalog projection for price, so `evans`'s proposed `authority:` kind may ship with zero users.
  That was recorded as an input row `PMW-4`'s decider must weigh, and it is repeated rather than
  quietly inherited.

### Follow-up actions

- **Slice 1** (HEAD orderability at checkout) — dispatched 2026-08-31, in parallel with this record.
- **Slice 2** carries the as-of fold's peak-cost **measurement** as a Done-when (§3 above).
- **Slice 4** carries the non-additive `PlaceOrder` change under `HOLD: human`, with the
  client-before-server rollout order reviewed before it lands (§3 above).
- **Contract C1** (`quote_age_seconds`) ships with the mechanism; after the first peak, re-derive N
  from the observed p99. Re-deriving needs no new founder question unless the direction changes.
- `specs/ordering/rules.yaml:61-65` stops claiming the `expectedTotal` check is the enforcement, in
  the same change as slice 4 — leaving that sentence after the check is deleted would be worse than
  the original defect.

## Consulted (ADR-20260812-143619 — one line per lens)

**Source discipline, and it is load-bearing here.** This record was written by an executor from a
dispatch, after the two answers. The lines below are **positions recorded in the repository** where a
citation is given, and **positions relayed by the coordinator in the dispatch that commissioned this
record** where one is not — marked as such. No lens was quoted from memory; writing invented lens
quotes into a record whose subject is a decision's provenance would be the failure the register-check
discipline exists to stop (the same reason
[ADR-20260831-141500](ADR-20260831-141500-the-coordinator-gets-the-register-check-gate-on-its-committing-surface.md)
gives for its own Consulted block).

- **business** — **recorded**, PROP-20260831-134539 §7 and §6 D5/D6: N's derivation from the
  cart-to-pay p99 with SCA rather than from risk appetite; dropping the M version-count axis on
  `OfferStockUpdated`; the reframing that the thing to gate on is **divergence, not staleness**;
  direction asymmetry (up and down are not symmetric); and the restaurant-borne absorb band.
  Decision 1 is its figure, taken unchanged.
- **legal** — **recorded**,
  [BRIEF-20260831 "Repricing and the priced quote token: the obligation map"](../legal/BRIEF-20260831-repricing-and-price-quote-counsel-packet.md):
  the obligation map, blockers **B1–B5**, counsel questions **QT-1…QT-10**, and §10's row-by-row
  reconciliation with the proposal's own `L1–L7`. Its load-bearing conclusion — the binding price is
  the one displayed at the confirming click — is why F4 exists and why approval does not wait on
  counsel. **No counsel is engaged; no lens output and no aggregation of lenses is legal advice or
  clearance**, and every article number in that brief is VERIFY-FIRST.
- **young** — **recorded**, ADR-20260831-121957 §7 line 1 (*"a fold gives freshness, not
  atomicity"*) and §4d's interim account, quoted in row `QUOTE-TOKEN`: *"display/charge coherence
  currently rests on a rebuildable artifact and on two reads at different times; it does not survive
  a catalog rebuild and does not survive a slow customer."* **Relayed** in the dispatch, and not
  otherwise persisted: that resting the guarantee on a rebuildable artifact is a **coherence argument
  dressed as a correctness one**.
- **architect** — **relayed** in the dispatch, not otherwise persisted: the sequencing, and that
  #816 was not dispatchable without this proposal. Consistent with the recorded position in
  ADR-20260831-121957's Consulted block (the antecedent discipline of ADR-20260817-105845 applied to
  every number).
- **holub** — **relayed** in the dispatch, not otherwise persisted: that the governance thread was
  displacing product, and that #816 is the shortest path to a user. Its nearest recorded position is
  the same shape — *"the shortest slice that removes the defect"* (ADR-20260831-141500's Consulted
  block).
- **beck** — **relayed** in the dispatch, not otherwise persisted: that the search-adequacy diagnosis
  behind the original framing was wrong. `grep -rn "search-adequacy\|search adequacy" docs/` returns
  **0 hits**, so this position exists nowhere in the repository but here; it is recorded as relayed
  rather than dropped, and rather than dressed up as a citation.
- **vernon / evans / dba / graphql-architect / ux-designer / observability / farley** — not
  separately consulted for this record and nothing is claimed on their behalf. Their standing
  positions on this subject are already composed into the proposal being approved (evans's Published
  Language reading of `specs/ordering/processmanager.yaml:63-68`, observability's four owed contracts
  in §9), which is the artifact the founder approved.

## Refs

- Rows: `docs/decisions/QUOTE-STALENESS.yaml` (decided by this record) · `docs/decisions/QUOTE-TOKEN.yaml`
  (decided 2026-08-31, the parent) · `docs/decisions/CAPTAINNET-ZERO.yaml` (open — the absorb
  *alternative*'s funding, per PROP §13 item 2)
- [PROP-20260831-134539](../proposals/PROP-20260831-134539-priced-quote-token.md) — the approved design
- [ADR-20260831-121957](ADR-20260831-121957-the-pm-read-step-is-retired-source-fixed-the-physics-and-left-the-ownership.md) §4d — the mechanism decision this completes
- [ADR-20260810-112836](ADR-20260810-112836-cart-priced-live-on-read.md) §2 — reversed in part (§4 above)
- [ADR-20260815-134655](ADR-20260815-134655-the-team-merges-its-own-work-no-pr-waits-on-founder-review.md) — why the `HOLD: human` concern is a slice gate, not an approval gate
- [ADR-20260808-144738](ADR-20260808-144738-product-ownership-lives-in-the-team-no-pm-agent.md) decision 5 — evidence-deferred
- [ADR-20260817-105845](ADR-20260817-105845-a-dispatch-card-may-not-state-a-derived-number-without-its-antecedents.md) — why every number above carries its antecedent
- [#816 "Display/charge divergence is undetected: the expectedTotal equality check never runs in production"](https://github.com/TheCaptainCompany/captain-food/issues/816)
