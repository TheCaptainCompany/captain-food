# ADR-20260829-145848 — The founder's answer sheet of 2026-08-29 (nine decisions)

**Status**: Accepted · **Date**: 2026-08-29 ·
**Decider**: the **FOUNDER / Tech CEO**, answers verbatim below ·
**Register rows**: [ERASURE-LAUNCH-GATE](../decisions/ERASURE-LAUNCH-GATE.yaml) ·
[V0-PROMO-AND-MINIMUM](../decisions/V0-PROMO-AND-MINIMUM.yaml) ·
[IDENT1-OUTAGE-EXPERIENCE](../decisions/IDENT1-OUTAGE-EXPERIENCE.yaml) ·
[LOSS-1](../decisions/LOSS-1.yaml) (stays open — shape only) ·
**Session**: https://claude.ai/code/session_01Dbhq2Y7U5NcnqhByscaB4v

## Status

Accepted — the register rows, STATUS/journal entries and GitHub bookkeeping land in the same
change. The 13-lens consult ran in-session before the answers were relayed
(ADR-20260812-143619); the Consulted block is below.

## The answers, verbatim

| Question | Founder answer (verbatim) | Note (verbatim) |
|---|---|---|
| ERASURE-LAUNCH-GATE | *"A — Erasure ships first"* | — |
| ETA-V0 ([#733](https://github.com/TheCaptainCompany/captain-food/issues/733)) | *"C — Team drafts the proposal with both options"* | — |
| OVH-SMS-FACTS | (facts, not a choice) | *"500 / 500"* |
| V0-PROMO-AND-MINIMUM | *"B — Minimum order only"* | *"15"* |
| IDENT1-OUTAGE-EXPERIENCE | *"B — Visible couldn't-load-your-account-retry"* | — |
| LOSS-1 | *"A — Bounded write-off"* | (empty) |
| STOREFRONT-DEGRADED-ORDERING ([#743](https://github.com/TheCaptainCompany/captain-food/issues/743)) | *"A — Keep it orderable"* | — |
| UX-PROPOSALS ([#734](https://github.com/TheCaptainCompany/captain-food/issues/734) / [#741](https://github.com/TheCaptainCompany/captain-food/issues/741)) | *"A — Draft both"* | — |
| MONEY-STACK | *"A — Yes, prepare the money-stack session"* | — |

## What each answer decides

1. **ERASURE-LAUNCH-GATE → decided (A).**
   [#708](https://github.com/TheCaptainCompany/captain-food/issues/708) becomes
   **launch-gating** — a tier-1 legal precondition (Art. 17 attaches at the first real customer
   order). The first chunk is a **full-mob, `HOLD: human` PROPOSAL** (stored-event-shape class).
   Farley's finding stands recorded: the deletion engine already exists **gated OFF**
   (ADR-20260731-214500) — Customer lacks only its `deletion:` block, identity unlinking,
   crypto-shred for `legalRetention` events, the SNAP-1 snapshot answer, and an executed drill.
2. **V0-PROMO-AND-MINIMUM → decided (B), note "15".** The unit derivation **"15 EUR" is
   derived, not asked**: the venue is Tours/France and `Money = {amountCents, currency}` kills
   unit ambiguity — 1500 cents EUR as the platform default in ordering configuration; a
   per-restaurant override is later work, not a reversal. **Promo codes are post-V0** — this
   retires [#468](https://github.com/TheCaptainCompany/captain-food/issues/468)'s
   `cart.discount` gap permanently. Open sub-questions for the minimum-order slice, named now:
   delivery-only vs also collection (a business flag — the founder decides at the slice), and
   pre-order disclosure placement (legal: the storefront/menu surface BEFORE checkout,
   Code de la consommation L112-1/L221-5 lineage).
3. **IDENT1-OUTAGE-EXPERIENCE → decided (B).** Amends the silent fail-closed **EXPERIENCE** of
   ADR-20260818-004646 — the fail-closed authorization **posture STANDS**. Implementation notes
   from the consult: a typed `ViewerResolution` variant, not a null (graphql); per-binding
   [#742](https://github.com/TheCaptainCompany/captain-food/issues/742) error vocabulary, not a
   global banner (ux); ephemeral UI state never on `domain_events` (young). Ships with the
   IDENT1-RESOLUTION-ACTIVATION flip.
4. **LOSS-1 → stays OPEN.** The founder chose the **shape** — "A — Bounded write-off"
   (2026-08-29) — but the threshold value is UNSET and folds into the money-stack session. The
   business lens recommendation is banked for that session: a 30 EUR per-order cap, a reserve
   priced at 0.3–0.5% GMV until a capture-failure-rate signal exists (that missing
   observability contract is named).
5. **ETA-V0 (#733)**: the team drafts the proposal with **both options** (Declared/Computed
   vocabulary, ux journey scope from today's consult).
6. **OVH-SMS facts**: a 500-SMS pack with **500 remaining** (founder console, 2026-08-29) — the
   pack is untouched, so there is no real OTP traffic yet; the
   [#699](https://github.com/TheCaptainCompany/captain-food/issues/699) credit gauge is
   **pre-launch** work (at the 200/day worst case the pack drains in 2.5 days). Alert shape
   banked: warn ≤300, page ≤100, a staleness dead-man (observability lens).
7. **STOREFRONT-DEGRADED-ORDERING (#743)**: current behaviour stands — the storefront stays
   orderable in the degraded state; checkout is the money gate. #743 closes.
8. **UX-PROPOSALS (#734/#741)**: draft both — sequenced as ONE combined session once the
   erasure proposal is in review (holub's WIP discipline; the founder's draft-both answer
   standing).
9. **MONEY-STACK**: prepare the money-stack session (LOSS-1's threshold, the reserve, and the
   named observability contract fold into it).

## Alternatives considered

Each question was put with its full option set on the answer sheet (the rejected options: an
accepted erasure exposure window; promo codes at V0 / both / neither; keeping the silent
fail-closed experience; unbounded or zero write-off; blocking degraded-storefront ordering;
drafting one or neither UX proposal; deferring the money-stack session). The founder's picks are
the verbatim table above; no option was re-opened.

## Consequences

Positive: launch scope is now explicit (#708 gates it); the #468 gap class closes; the IDENT1
outage experience is decided before the flip that makes it visible. Negative: #708 moves ahead
of feature work by design. Follow-up: the #708 full-mob `HOLD: human` proposal dispatches next;
the ETA proposal (#733) is commissioned; the combined #734+#741 session waits for the erasure
proposal to be in review; the money-stack session page is prepared.

## Consulted

Thirteen-lens consult, run in-session on the answer sheet before relay (2026-08-29):

- **architect** — sequencing: the #708 full-mob `HOLD: human` proposal dispatches next; the
  combined imagery+tiles session waits until the erasure proposal is in review; the money-stack
  session gets its own page.
- **beck** — the erasure drill must be executable, not documentary: a drill in CI, and the #699
  smoke asserting the credit gauge once emitted.
- **business** — promo codes post-V0 (fair fees, not coupon wars); minimum order is a launch
  need priced from rider economics; LOSS-1: 30 EUR per-order cap, reserve 0.3–0.5% GMV until a
  capture-failure-rate signal exists.
- **dba** — erasure storage legs: crypto-shred for `legalRetention` events, the SNAP-1 snapshot
  answer, backup posture W.
- **evans** — two-term language kept distinct: *erasure* (data destroyed) vs *unlinking*
  (identity mapping severed); the vocabulary goes into the #708 proposal.
- **farley** — the deletion engine exists gated OFF (ADR-20260731-214500): the gap is the
  Customer `deletion:` block + unlinking + crypto-shred + SNAP-1 + an executed drill, not a
  from-scratch build; separately, the nightly smoke vehicle is itself red 19 runs —
  fix-vehicle-first before asserting anything through it.
- **graphql** — IDENT1 outage state is a typed `ViewerResolution` variant, never a null; the
  erasure API is a two-mutation shape.
- **holub** — WIP discipline: #734 and #741 become ONE combined session, sequenced after the
  erasure proposal is in review.
- **legal** — the erasure gate is a tier-1 legal precondition; minimum-order pre-order
  disclosure belongs on the storefront/menu surface BEFORE checkout (Code de la consommation
  L112-1/L221-5 lineage); no lens output is legal advice or clearance.
- **observability** — #699 alert shape: warn ≤300, page ≤100, staleness dead-man; LOSS-1's
  missing capture-failure-rate contract named for the money-stack session.
- **ux** — IDENT1: per-binding #742 error vocabulary, not a global banner; #743 must not stay
  open once decided (an open issue would misstate its state); the minimum-order rule needs a
  pre-checkout surface.
- **vernon** — erasure is a command, not an event; the flow is a process manager with per-leg
  Tells.
- **young** — ephemeral UI state (the retry affordance) never lands on `domain_events`; the
  erasure fold/upcasting questions ride the `HOLD: human` proposal.
