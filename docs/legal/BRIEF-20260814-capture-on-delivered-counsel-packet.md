# Capture-on-delivered — counsel packet (authorize-now, capture-on-delivery posture)

**Date-stamped 2026-08-14** · **Status**: Counsel questions CAP-1–CAP-7 pending a French avocat ·
**Prepared by**: the legal-specialist lens of the five-lens review of
[PR #545 "capture on delivered"](https://github.com/TheCaptainCompany/captain-food/pull/545)
(tracking issue [#544](https://github.com/TheCaptainCompany/captain-food/issues/544)),
session https://claude.ai/code/session_01XTrJE7m5TGkKRRPKj5ZFqZ ·
**Decision context**: capture timing decided by the customer 2026-08-08
([ADR-20260808-195315](../adr/ADR-20260808-195315-customer-brief-answers.md),
[DECISIONS §1 row B](../proposals/DECISIONS.md)) — *authorize at checkout, capture on delivered /
picked up (in advance for at-table)* — under Connect separate charges & transfers
([PROP-20260726-165000 D1/D2](../proposals/PROP-20260726-165000-marketplace-economics-and-money-movement.md)).

**Grades**: (a) established obligation · (b) interpretation for counsel to confirm · (c) unknown.
**None of this is legal advice, and no lens output — nor any aggregation of lenses — is legal
clearance** (ADR-20260812-143619). It maps obligations and questions; it never substitutes for
licensed French counsel, and agreement between review lenses does not upgrade a hedged finding to a
settled one.

## The finding in one line

Moving from capture-at-checkout to **authorize-now / capture-on-delivery** changes *when money
moves, on what amount, and against what promise* — which reaches consumer-disclosure (Code de la
consommation L221-5), the payment-agent/fund-safeguarding posture, and the VAT tax-point. The change
**helps** the payment-agent posture (an authorization hold moves no money, so Captain holds nothing
during the hold) but does not close it, and it opens new disclosure and tax-point questions that
must be answered before launch.

## Counsel packet — CAP-1 … CAP-7

- **CAP-1 — agent status under manual-capture.** Does an authorize-now/capture-on-delivery flow,
  with the restaurant as merchant of record via Connect separate charges & transfers, keep Captain
  clear of the DSP2/agent-financier and fund-safeguarding perimeter during the hold window? (b) The
  hold moves no money and no funds rest in a Captain balance; confirm this holds across the whole
  authorize → capture → transfer sequence and for the rider transfer leg.
- **CAP-2 — hold-lifecycle management.** What are Captain's obligations around a card authorization
  it holds but has not captured — duration limits, the duty to release promptly on cancellation or
  rejection, and the consumer's right to a timely release rather than a refund? (b)
- **CAP-3 — L221-5 disclosure floor.** For distance selling, what must the pre-contract disclosure
  say about *authorize now, charge on delivery*: that a hold (not a charge) is placed at checkout,
  when and on what amount the actual charge occurs, and what a rejection/timeout releases? (a/b) The
  consumer must not believe they have been charged when only a hold exists.
- **CAP-4 — refund-vs-release wording.** On the rejection/acceptance-timeout path the authorization
  is *released*, not *refunded* (no money moved). What consumer-facing wording is required so a
  "release" is not mis-described as a "refund" (and vice-versa), given the two have different
  timings and consumer expectations? (b)
- **CAP-5 — VAT tax-point.** For an authorize-now/capture-on-delivery sale, is the VAT tax-point
  (fait générateur / exigibilité) at **delivery/capture on the captured amount**, not at
  order/authorization? (a/b) This is load-bearing for the unbuilt VAT-receipt engine — see the
  forward note below.
- **CAP-6 — post-void hold visibility.** After a hold is voided/released, the consumer's bank may
  still display the pending authorization for some days. What disclosure or support obligation, if
  any, attaches to that bank-side visibility lag Captain does not control? (b/c)
- **CAP-7 — recovery posture on capture-failure-after-fulfilment.** When a post-delivery capture
  ultimately fails on a *fulfilled* order (authorization dead / card permanently declined), what is
  Captain's lawful recovery posture toward a consumer who received the goods — and what disclosure
  must precede any such pursuit? (b/c) This is the legal half of the founder-owed loss-allocation
  decision [DECISIONS §38 LOSS-1](../proposals/DECISIONS.md).

## Forward notes (build constraints for unbuilt work)

- **VAT-receipt engine (unbuilt) must key to capture/delivery on the captured amount.** When the
  receipt/invoicing engine is built ([#174](https://github.com/TheCaptainCompany/captain-food/issues/174)),
  its VAT tax-point must be **capture/delivery** and the **captured amount** — never
  authorization/order — pending CAP-5 confirmation. A receipt keyed to the authorization would state
  the tax-point wrongly and on a possibly-uncaptured amount.
- **The `checkout.email "for receipt"` promise must not fire "sale complete" at authorization.** The
  checkout flow collects an email "for receipt"; under this posture it must **not** send a
  "sale complete" / purchase receipt at authorization, because no sale has completed and no charge
  has occurred — only a hold. Any at-authorization message must read as an order/hold confirmation,
  and the fiscal receipt fires at capture (see CAP-3, CAP-5).
- **Payment-agent posture is helped, not closed.** This change reduces exposure (a hold moves no
  money) but the agent/fund-safeguarding question remains counsel-gated (CAP-1); it is not resolved
  by the posture change alone.

## Triage

- **BLOCKER (pre-launch)**: L221-5 disclosure that a hold — not a charge — is placed at checkout
  (CAP-3); the receipt/email must not claim a completed sale at authorization.
- **EXPOSURE**: agent-financier perimeter across the full sequence (CAP-1); VAT tax-point wrong in
  the future receipt engine if keyed to order/authorization (CAP-5); consumer-recovery framing on a
  fulfilled order whose capture failed (CAP-7 / LOSS-1).
- **HYGIENE**: "release" vs "refund" wording (CAP-4); post-void bank-side hold visibility support
  copy (CAP-6).
