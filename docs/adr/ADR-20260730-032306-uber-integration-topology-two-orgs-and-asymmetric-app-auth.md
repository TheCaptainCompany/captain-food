# ADR-20260730-032306 — Uber integration topology: two Direct organizations, asymmetric app auth, per-surface credentials

- **Status**: Accepted (product-owner decisions, 2026-07-30 session)
- **Date**: 2026-07-30
- **Proposal**: [PROP-20260730-032306](../proposals/PROP-20260730-032306-uber-eats-marketplace-and-per-surface-direct-credentials.md)
- **Tracking issue**: [#260 "Epic: Uber Eats Marketplace integration (order centralization + menu sync) and per-surface Uber Direct credentials"](https://github.com/TheCaptainCompany/captain-food/issues/260)
- **Refines**: ADR-20260729-020000 (configuration rides the artifact, secrets ride CI) · ADR-20260720-004556 (single delivery partner in V0)

## Context

Captain now touches Uber through **two unrelated products**, and the repo had conflated them. [#57
"Uber Eats (Uber Direct) delivery-partner adapter"](https://github.com/TheCaptainCompany/captain-food/issues/57)
shipped `crates/adapters/uber_direct` — the delivery API — under a title naming Uber *Eats*. The Uber
Eats **Marketplace** API (menu/order/store sync, Captain acting as an Uber "Provider") was specified
nowhere, while `UBER_DIRECT_*` config existed for a symmetric OAuth2 flow with no Uber app registered
against it.

On 2026-07-30 the product owner registered **Captain Food Restaurant** on the Eats Marketplace suite
and accepted the API Licensing Agreement with all seven APIs. Several credential-shape questions had
to be answered before any secret could be created, and answering them per dashboard field — as the
session initially did — produced two wrong key sets and four mis-named repository secrets. Recording
the topology once is cheaper than re-deriving it from a screenshot each time.

## Decision

**1. Uber Eats and Uber Direct are separate integrations, named separately.** `UBER_EATS_*` is the
Marketplace app (order centralization + menu sync). `UBER_DIRECT_*` is delivery dispatch. Neither
prefix borrows the other's vocabulary, and the Eats *price-comparison* feature (ADR-0022/0023/0024/
0025/0030) is a third, unrelated concern that keeps its own naming.

**2. Uber app authentication is asymmetric.** Application id + key id + private key, signing a client
assertion. This retires `UBER_DIRECT_CLIENT_SECRET` and `UBER_DIRECT_SCOPE`, and the OAuth2 token
manager built around them. No shared secret exists, so nothing Uber stores about us is replayable.

**3. Private keys are stored base64-encoded**, declared with a `Base64PrivateKey` scalar. A raw
multi-line PEM is mangled inconsistently across the Render dashboard, Actions secrets, Docker `--env`
and Kubernetes, and the `\n`-vs-real-newline ambiguity fails at *first signature* — asynchronously,
during dispatch — while the boot report still reports the key as `set`. The scalar rejects a pasted
PEM or a truncated copy at validation time instead.

**4. Webhook verification accepts either of two signing keys.** We generate both (256 bits, CSPRNG,
hex), register both with Uber, and the verifier tries primary then secondary, constant-time. Accepting
either is not a weakening — it is the only way to rotate without rejecting in-flight webhooks, and a
rejected Uber webhook is an order nobody is told about, which is the worst failure mode this domain
has.

**5. Uber Direct uses two Uber organizations, split by acquisition surface** (proposal D2, option A):
restaurant-storefront dispatches billed to one, `captain.food` marketplace dispatches to the other.
Storefront first; the marketplace org is created later.

**6. The acquisition surface is a fact on the order, not a request-scoped value** (proposal D3, option
A). The write path is acceptance-first (ADR-20260720-015500): `DeliveryDispatchProcess` dispatches on a
spawned task long after the mutation answered `PENDING` and the `Host` header is gone. A surface
derived at dispatch time is therefore not derivable at all. It is recorded on the order and folded.

**7. Delivery channels are keyed by surface** — `uber_direct:restaurant`, `uber_direct:marketplace` —
not by a single `uber_direct` channel with a per-order credential lookup. An unconfigured surface is
then an *unwired channel*, and the composite gateway's existing behaviour applies: the offer times out
and the saga escalates (`crates/server/src/lib.rs`). A single channel would have to invent a
not-configured path, and its failure mode would be dispatching on the wrong organization's
credentials — billing the wrong party while looking like success.

**8. Per-tenant values are never configuration.** Uber Eats addresses locations by store id, one per
restaurant, so store ids and merchant consent live in an adapter-owned `uber_eats_connections` table,
as `hubrise_connections` already does. The rule: what scales with restaurants is a table row; what is
fixed per deployment and selects which Uber account we act as is a config key.

## Consequences

- `crates/adapters/uber_direct` needs its authentication replaced, not extended: the token manager
  goes, and the gateway gains a surface-keyed registration. The seven `UBER_DIRECT_*` keys in
  `specs/configuration.yaml:677-744` are superseded by `UBER_DIRECT_RESTAURANT_*` (+ later
  `_MARKETPLACE_*`); `CLIENT_SECRET` and `SCOPE` are removed rather than left readerless.
- `OrderPlaced` gains an acquisition surface — an event payload change, so plan mode plus ADR-0032
  completeness (behaviour test + `rules:` link).
- Two Uber organizations make Captain a principal for the storefront delivery leg, which touches the
  payment-agent posture (a French legal precondition per `CLAUDE.md`, not a backlog item). Recorded
  here because the existence of two Uber accounts would otherwise have to be reverse-engineered.
- The Order API clause makes Captain *"wholly responsible for correctly relaying all information …
  including but not limited to allergy information and special instructions."* With EU FIC 1169/2011
  that becomes a `rules.yaml` rule with a test, not a best effort: an item without an allergen
  declaration is not published, and allergen data survives translation verbatim.
- Data received under the Provider licence serves the merchant *on Uber* — it must never seed the
  Captain marketplace catalog. Enforced by direction of flow in the ACL, asserted by test.
- **Left open** (proposal D4/D5/D7): how a marketplace-originated order is represented given it
  carries no Captain PaymentIntent; menu ownership and per-channel price parity; and whether the
  Provider entity on the agreement (*Caring Hope Foundation*, RNA W372020229 — a loi-1901
  association) is the entity that will operate the platform. The last is a legal question, flagged
  rather than answered.
