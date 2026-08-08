# ADR-20260808-203443 — Tips, voluntary contributions, and the funding model; erasure two-path and admin act-as confirmed

**Status**: Accepted · **Date**: 2026-08-08 · **Deciders**: the customer (product owner), in
session · **Closes**: the last three open items of
[BRIEF-20260808-customer-decisions.md](../proposals/BRIEF-20260808-customer-decisions.md)
(ch. 1.4 tips · ch. 2 erasure · ch. 3 admin-on-behalf). The ten-decision brief is now fully
answered; with [ADR-20260808-195315](ADR-20260808-195315-customer-brief-answers.md) the register
drops to the five §22 sweep rows.

The customer: *"I agree with all except the tipping"* — the erasure two-path model and the
explicit act-as recommendation are **confirmed as proposed**; tips are decided with the
customer's own model, recorded verbatim below.

## 1. Tips and voluntary contributions — the customer's model (PROP-165000 D5)

> "Considering that the goal of this platform is to help restaurants and riders and help the
> platform to help them: we should propose to tip the restaurant **if the restaurant allows it**,
> and allow a **voluntary contribution to the platform** to keep the low pricing for the
> restaurants. Like **HelloAsso** we will explain and propose to the user that if they contribute
> to the platform they will help the restaurant to offer a free-commission service to them.
>
> We will show to the user that the platform is commission-free and the platform made a
> **« pari »** that the voluntary contributions will cover the cost of the platform. And in
> advance, in total transparency: if it's not the case — the cost is not covered by the voluntary
> contributions — we will apply a **monthly cascade pricing to the restaurants** based on the
> fixed cost of the platform divided by the restaurant count. So the more restaurants the platform
> has, the less expensive it is. And in advance, the pricing can be **free** if the voluntary
> contributions cover the cost of the platform.
>
> We will show the **« cagnotte »** and show the contribution amounts and names **if the
> contributor wishes it**. These voluntary contributions can be done **during the order process or
> outside the order process**."

### What this decides

1. **Three tip/contribution surfaces exist**, not one:
   - **Rider tip** — as recommended in
     [BRIEF-20260808-tips-discussion.md](../proposals/BRIEF-20260808-tips-discussion.md)
     (checkout module + one-tap rating-sheet tip; 100% pass-through, coop absorbs fees;
     `TipNeverVisibleBeforeDelivered`; gated on the transfer leg). Unchanged by this decision.
   - **Restaurant tip — per-restaurant opt-in.** The restaurant decides whether its customers see
     a tip control; a restaurant-level setting, off by default, owned by the restaurant in its
     back office. (This replaces the team's "no restaurant tip in V0": shipped, but only where
     the restaurant turned it on.)
   - **Platform voluntary contribution — the HelloAsso model.** Offered during the order process
     AND outside it (a standalone support surface). The pitch is explicit and educational:
     Captain is commission-free; contributing helps *the restaurant* keep a free-commission
     service.
2. **The funding narrative is a public bet (« pari »)**, declared in advance with total
   transparency: voluntary contributions are bet to cover platform costs. The fallback is declared
   in advance too — **monthly cascade pricing**: `fixed platform cost ÷ restaurant count`,
   applied to restaurants only if contributions fall short, decreasing as the network grows, and
   **0 €** whenever contributions cover the costs.
3. **The cagnotte is public**: running contribution total visible; individual amounts and names
   shown **only with the contributor's wish** (per-contribution consent, default anonymous).
   Composes with the radical-transparency decision (ADR-20260808-195315 ch. 5 — Open Collective
   accounting): the cagnotte and the cost side it is bet against are both public.

### Team notes carried (advisory, not re-litigation)

- The ux lens had recommended keeping platform asks out of money-moments; the customer's
  HelloAsso framing overrides this **with a reason the lens accepted for HelloAsso itself**: the
  contribution is presented as helping the *restaurant*, not as a fee — placement and copy must
  preserve that framing (educational, dark-pattern-free, "Aucun" default, never blocking the pay
  path; HelloAsso's own contribution step is the reference pattern).
- Cascade pricing needs the cost baseline public and auditable (the radical-transparency
  machinery provides it) and its contractual shape belongs in the restaurant terms — P2B
  transparency applies. → counsel packet F-series.
- VAT/tax treatment of voluntary contributions received by a commercial SASU (vs HelloAsso's
  specific posture) is a counsel question, not an assumption. → counsel packet F-series.
- The rider-tip gate is unchanged: no money control ships before its transfer leg exists.

## 2. Account-level erasure — two-path model CONFIRMED (§1 C remainder)

As mapped in
[docs/legal/BRIEF-20260808-account-erasure-two-path.md](../legal/BRIEF-20260808-account-erasure-two-path.md):
**deactivate** (recoverable anytime, data kept, disclosed as not-deletion, dormant sunset) +
**delete** (Art. 17 path: ≤30-day grace, re-login cancels, then real erasure of identity, files
and Supabase, orders via the tombstone machinery; carve-outs per the retention table). Equal
prominence between the two paths. Counsel questions E1–E8 go to the avocat.

## 3. Admin-on-behalf — explicit act-as CONFIRMED (PROP-171500 D4)

The admin acts *as admin* on a named restaurant's scope: a distinct, logged authorization path
(admin writes queryable as a class, per-capability limits possible), never assumed-identity
impersonation. Envelope attribution was never in question (ADR-0041). **This supersedes
ADR-0037's impersonation-only stance** — reversed by its own author, closing the reversal the
brief flagged.

## Consequences

- PROP-20260726-165000 D5 is decided (header updated); the tips brief becomes the realization
  reference for the rider surface; the contribution/cagnotte/cascade model is new scope —
  tracked by its own epic issue.
- The counsel packet gains the F-series (funding model) questions.
- Realization of all three lands through the normal plan-mode → spec → codegen pipeline; nothing
  in `specs/**` changes by this ADR alone.
