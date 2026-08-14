# ADR-20260808-195315 — The customer answers the decision brief: money posture, entity path, radical transparency

**Status**: Accepted · **Date**: 2026-08-08 · **Deciders**: the customer (product owner), via the
interactive decision form · **Records**: seven of the ten decisions in
[BRIEF-20260808-customer-decisions.md](../proposals/BRIEF-20260808-customer-decisions.md); the
remaining three (tips, erasure, admin-on-behalf) moved to **discussion** — see "Still open" below.

The customer answered the ten-decision brief through the decision form. Answers are recorded
verbatim; where the answer diverges from the recommendation, the customer's choice stands and the
team's follow-up notes are advisory only, never re-litigation.

## Decided

### 1.1 Payout posture (PROP-165000 D1) — **as recommended**

**Stripe Connect, separate charges & transfers.** Restaurants are sellers of record; a regulated
institution holds customer funds; Captain invoices only its service fee. Unblocks
[#173 "Stripe Connect onboarding"](https://github.com/TheCaptainCompany/captain-food/issues/173)-family
work *once the entity below exists* (the Connect platform account belongs to the SASU, see 4).

### 1.2 Capture timing (PROP-165000 D2) — **different choice, the customer's**

> "Authorise on checkout. Capture on delivered / picked up / paid in advance for at-table service."

Capture is **per service type**: DELIVERY captures on `delivered`, PICKUP/collection on
`picked up`, at-table service captures **in advance** (at checkout). This is later than the
recommended capture-on-acceptance. Team notes carried, not blocking:

> **Refined 2026-08-14 — see
> [ADR-20260814-141350](ADR-20260814-141350-collection-captures-at-ready-not-at-pickup.md).** The
> founder refined the COLLECTION leg: capture at **prepared / ready** (`OrderMarkedReady`), one step
> before pickup — READY is collection's last controlled moment (collection is the customer's action,
> not a platform step), so this protects against cook-then-no-show. DELIVERY (on `delivered`) and
> at-table (advance) are unchanged. This annotation is a forward pointer, not a rewrite of the
> decision above.


- A card authorization is valid ~7 days — same-day orders are safely inside it; the scheduled-order
  window (PROP-164500 D6) stays bounded by authorization life, as already recorded.
- Capture now happens **after fulfilment cost is sunk** (food cooked, ride done). Capture of a
  confirmed authorization rarely fails, but the failure path (capture declined post-delivery) needs
  an anticipated error + operator surface when the payments slice is realized.
- Tips at delivery time interact with capture ≤ authorized amount — folded into the tips
  discussion (1.4, still open).

### 1.3 Acceptance timeout (PROP-164500 D1+D2) — **resolved as a consequence of 1.2**

> "No need to refund because no capture."

Under capture-on-delivery, the acceptance-timeout path **releases the authorization** — no refund
machinery on this path, the customer was never charged. Auto-cancel on timeout stands; the refund
leg of the recommendation is void by construction. The TTL value (5 min default, per-restaurant
override) is reversible gated config — the team owns tuning it (ADR-20260808-144738).

### 1.5 External orders (PROP-032306 D4) — **as recommended**

Distinct `ExternalOrderReceived` event; provenance visible; `OrderPlaced`'s payment fields stay
non-nullable.

### 4. Operating entity (PROP-032306 D7 / brief ch. 4) — **different choice, the customer's**

> "The initiative is on Caring Hope association for now and will be carried by a SASU for now,
> I'm currently working on the brand name. Then once we have enough restaurants and riders we
> will create a SCIC per area like CoopCycle did, with a federation that will maintain the
> product for all SCIC."

Recorded path: **association (now) → SASU (operations, brand pending) → SCIC per area + federation
(at scale)**. Consequences:

- The **Stripe Connect platform account is opened by the SASU** — Connect onboarding waits for the
  SASU to exist; do not onboard under the association.
- The **Uber Direct agreement** currently names Caring Hope Foundation; the counsel packet's
  entity questions now become *transfer/novation to the SASU* questions, plus the future
  SASU→SCIC transition (asset/contract transfer, licence follow-through).
- The CoopCycle-style federation model is a recorded strategic intent, not a V0 work item.

### 5. Transparency levels (§19 D1 / brief ch. 5) — **different choice, the customer's**

> "Radical transparency. Accounting will be publicly visible on Open Collective. Kubernetes pods
> and technical usage will be publicly visible. Incident and post-incident will be published in
> GitHub and we will have a status page with useful info in it for the public."

All levels of PROP-20260807-190936 are ON: public accounting (Open Collective), public
infrastructure/technical usage, public incident reports and postmortems (GitHub), public status
page. The two ensemble-decided guardrails **stand and compose** (they constrain *how*, not
*whether*): transparency exposes INFORMATION never CONTROL, and published metrics are
platform-wide aggregates only — per-restaurant/per-rider dimensions never without consent, k ≥ 10
when slicing starts (§19 D2). Incident postmortems are scrubbed of personal data before
publication.

### 6. Promo funding (PROP-165500 D5 / brief ch. 6) — **as recommended**

Restaurant-funded promo codes first; platform-funded codes deferred until a funding source exists;
loyalty next, reusing
[#158](https://github.com/TheCaptainCompany/captain-food/issues/158)'s balance.

## Still open — moved to discussion (customer chose "Let's discuss")

- **1.4 Tips (PROP-165000 D5)** — the customer widened the question: tip *timing* (at checkout vs
  at delivery) × tip *recipient* (rider, restaurant, platform). Needs a small option grid
  (business + ux + Stripe mechanics) before deciding.
- **2. Account-level erasure (§1 C remainder)** — the customer's direction: *"Same behavior as
  Facebook, people can recover their account anytime."* Legal must reconcile
  recoverable-deactivation with GDPR Art. 17 (a recoverable account is not erasure; Facebook
  itself ships BOTH deactivate and delete). Discussion prepared by the legal lens.
- **3. Admin-on-behalf (PROP-171500 D4)** — the customer's note (*"even with impersonation, …
  the event envelope will know it"*) is likely already satisfied by ADR-0041 envelope attribution;
  needs one clarifying exchange to close.

## Register effect

Open decisions: 15 → **8** (three brief items in discussion + the five §22 sweep rows).
