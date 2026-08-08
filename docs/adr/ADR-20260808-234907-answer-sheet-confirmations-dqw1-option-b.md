# ADR-20260808-234907 — Brief answer sheet: cards 1–7 confirmed, D-QW1 decided as option (b) — orderId joins the four delivery payloads

**Status**: Accepted · **Date**: 2026-08-08 (night) · **Decider**: the customer (product owner),
via the interactive decision brief's answer sheet (pasted into the session — the recorded
decision-form return path) · **Tracking**:
[#348 "Epic: the rider/delivery write surface does not exist (24 of main's 32 validator warnings)"](https://github.com/TheCaptainCompany/captain-food/issues/348)

## The answer sheet (verbatim substance)

Cards 1–5 (the #348 batch previously answered inline, re-presented with full per-lens content):
**all "Confirmed — approval stands"** — slice 1 retirement (applied), D6 `sends:` (pending its
validator mechanism), quick wins pulled forward, slices 3–8 value order, apply-now vehicle.
Cards 6–7 (ensemble-consent decisions under the veto window — #388 remedy + flake policy, #335
consolidation scope): **both "Consent stands"** — no veto.

Card 8 (new): **"Different choice — option b or amend", note: "B"** — on decision D-QW1 in
[PROP-20260808-233000 "Customer-anxiety quick wins: the exact spec diff"](../proposals/PROP-20260808-233000-customer-anxiety-quick-wins-spec-diff.md)
§2.4, the customer chooses **option (b): add `orderId` to the four delivery event payloads**
(`DeliveryAcceptedByRider`, `DeliveryPickedUp`, `DeliveryCompleted`, `DeliveryStatusUpdated`)
so the projection worker keys delivery events to their OrderTracking row from the payload itself —
self-contained events, the `PaymentRefunded` house precedent — instead of the recommended
worker-side `View_DeliveryJob` lookup.

## Consequences

- PROP-20260808-233000 is **rewritten for option (b)** (living document, ADR-20260801-020000)
  before any application: the QW1 diff grows to the four event payloads plus every fixture,
  behaviour test, command handler and ACL mapping that builds them; the worker keying stays
  mechanical (no read dependency inside the fold path). QW2 (checkout FAILED state) is unchanged.
- The rewritten exact text returns to the customer for approval (brief card), like slice 1 did.
  The document's Status stays `Proposed` until then.
- Still legal precisely because this is pre-production: stored-event payload shapes are immutable
  contracts once real events exist — this decision rides the same closing window as the
  vocabulary retirement.
- Cards 1–7 need no action: they confirm the already-recorded state
  (ADR-20260808-230800, ADR-20260808-224500).
