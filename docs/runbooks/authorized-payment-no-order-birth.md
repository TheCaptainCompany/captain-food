# Runbook — an authorized payment with no order behind it

**Alert**: `payment_authorized_no_order_birth_age_seconds{reason}` above its declared threshold
(`specs/observability.yaml`, the `place-order` contract). Related:
[#608](https://github.com/TheCaptainCompany/captain-food/issues/608) ·
[ADR-20260816-213000](../adr/ADR-20260816-213000-the-birth-gap-detector-reads-the-saga-run-not-an-anti-join.md).

**What it means, in one sentence.** A customer's card is authorized — the money is held — and no
order exists for it. This is the worst failure mode this product has, with the money already moved:
the kitchen will never see the order, the customer believes they have bought food, and the hold
expires silently in about seven days.

**Page on the FIRST one.** There is no tolerable rate. At V0 the base rate is ~0/day, so "noise" is
not a reason to wait for a second occurrence.

---

## 0. Who to call

**There is no alert route wired anywhere in this repository, and no artifact names a human or a
rota.** This is a recorded gap, not an oversight of this page: the same gap blocks the
`ROUTE_ORDER_BIRTH_THROUGH_LANE` flip's "the rollback trigger has no observer" obligation, which is
founder-gated. Until a route exists, this page is what a person reads once they have noticed the
series by hand.

## 1. Read the `reason` label first — it decides the diagnosis, not the remedy

| `reason` | What the system believes | Where to look |
|---|---|---|
| `retry_pending` | The `PaymentAuthorized` hop is on the `PlaceOrderProcess` lane and still deliverable, but the run has been `AWAITING_PAYMENT_RESULT` longer than the whole retry schedule allows. | `inbound_messages` where `actor_type = 'PlaceOrderProcess'` and `status = 'RECEIVED'` — is a worker claiming that lane at all? |
| `delivery_exhausted` | The hop reached a terminal status and the run never resolved. Nothing will redeliver it. Time cannot help. | the same row's `error` column, and `mailbox_poison_failed_total{actor_type}` |
| `no_run` | A `PaymentAuthorized` fact exists in `domain_events` with **no `payment_process_manager` row for its intent**. The crash window between the Stripe intent-create and the run-row upsert (`crates/application/src/commands.rs`, `PlaceOrder`). | `domain_events` where `event_type = 'PaymentAuthorized'`, anti-joined to `payment_process_manager` |

The remedy below is the same for all three. The label tells you what to fix so it does not recur.

## 2. The response, in order

1. **Find the PaymentIntent.** The gauge reports an age, not an identity — that is deliberate (a
   payment intent id is not a metric label). Get the identities from the database:

   ```sql
   -- retry_pending / delivery_exhausted: the saga's own run rows
   SELECT p.payment_intent_id, p.cart_id, p.order_id, p.customer_id,
          m.status, m.attempts, m.received_at, m.error
   FROM inbound_messages m
   JOIN payment_process_manager p ON p.payment_intent_id = m.payload->'payload'->>'paymentIntentId'
   WHERE m.actor_type = 'PlaceOrderProcess'
     AND m.message_type = 'PaymentAuthorized'
     AND p.process_status = 'AWAITING_PAYMENT_RESULT'
   ORDER BY m.received_at;

   -- no_run: the log residue, invisible to the run state by construction
   SELECT e.payload->>'paymentIntentId' AS payment_intent_id, e.occurred_at, e.correlation_id
   FROM domain_events e
   LEFT JOIN payment_process_manager p ON p.payment_intent_id = e.payload->>'paymentIntentId'
   WHERE e.event_type = 'PaymentAuthorized' AND p.cart_id IS NULL
   ORDER BY e.occurred_at;
   ```

2. **Check the Stripe dashboard** for that PaymentIntent. Confirm it is genuinely
   `requires_capture` (authorized, uncaptured) and that the amount matches the cart. If Stripe says
   it was already captured or canceled, the detector is reporting stale state — record that, it is
   a defect in this signal and not a customer incident.

3. **Cancel the PaymentIntent** (Stripe dashboard → the intent → Cancel). This releases the hold.
   **By hand, in the dashboard.** There is deliberately no automated void: voiding a hold is money
   movement, it is out of scope for the detector, and an automated remedy for a signal that has
   just proved the system was wrong about this order is the wrong order of operations.

4. **Contact the customer.** They believe they ordered food. Tell them the order did not go
   through, that the hold is released, and roughly when their bank will show it (holds can take a
   few days to disappear even after a void).

5. **Note the reclamation.** Record the incident against the customer so the next person sees it —
   the intent id, what the `reason` label said, what Stripe showed, and that the hold was voided.

## 3. What NOT to do

- **Do not re-drive the saga to "just create the order"** without step 2. If Stripe already
  captured, replaying the birth produces an order nobody is preparing while the money HAS moved —
  the same failure with the sign flipped.
- **Do not delete the `inbound_messages` row** to clear the alert. The row is the evidence and the
  gauge is derived from it; deleting it makes the incident invisible instead of resolved.
- **Do not raise the threshold** because the alert fired. The thresholds are lane-derived (the
  exponential retry schedule, from the declared `MAILBOX_MAX_DELIVERY_ATTEMPTS` and
  `MAILBOX_HEARTBEAT_SECONDS`); raising one silently buys latency for a held card.

## 4. Verifying the detector itself

If the series is **absent** rather than zero, the sweep is not running — that is what
`payment_birth_gap_sweep_heartbeat_total` is for. Alert on the absence of an increment there, never
on a threshold: on a healthy system every gauge here reads 0, and 0 is indistinguishable from a
dead sweep without the heartbeat.
