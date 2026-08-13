# ADR-20260813-004634 — Supabase Auth is retained for V0 ("free and easier"), and the window to own identity closes at the first real order

- **Status**: Accepted (founder directive, 2026-08-13)
- **Date**: 2026-08-13
- **Governed by**: [ADR-20260812-143619](ADR-20260812-143619-the-founder-is-the-founder-and-every-founder-message-goes-to-the-whole-team.md)
  (every founder message goes to the whole team before any answer; a record created from a founder
  directive carries a `Consulted:` block)
- **Register row**: [DECISIONS §36 IDP-1](../proposals/DECISIONS.md)
- **Relates**: [ADR-0015](0015-wrap-supabase-auth-behind-graphql.md) (Supabase Auth is wrapped behind
  our GraphQL, ACL'd, SDK never in the domain) · [ADR-20260807-002705](ADR-20260807-002705-hosting-ovh-mks-cnpg-gitops.md)
  (self-hosted Postgres — CloudNativePG in-cluster on OVH MKS) · [ADR-20260812-214021](ADR-20260812-214021-the-founder-answer-sheet-of-2026-08-12.md)
  (Q-L3 = no real phone-verified end user; INV-1's spend gate) · [ADR-20260722-174500](20260722-174500-identity-federation-cross-tenant-personalization.md)
  (SMS goes out on **our own** OVHcloud account, not the provider's) · [ADR-0041](0041-acting-user-is-envelope-not-payload.md)
  (the acting user is envelope metadata on `domain_events.user_id`)

## Status

Accepted.

## Context

### The correction that occasioned this record

The question was opened by a founder message the previous day: *"Don't care about the Supabase keys
because we going to use our own Postgres hosted on Kubernetes"*. **The premise was already true and
the inference was not.**

- The database has been **self-hosted CloudNativePG in-cluster** since
  [ADR-20260807-002705](ADR-20260807-002705-hosting-ovh-mks-cnpg-gitops.md). Nothing about Supabase
  changes that, because Supabase never held it.
- Supabase is the **identity provider only** and holds **no business data** — `specs/services.yaml`
  declares the `identity` capability (`send_phone_otp` / `verify_phone_otp`, wrapped by the
  `supabase-acl` per ADR-0015); it is a local, unexposed capability, not a datastore.
- `SUPABASE_SECRET_KEY` gates **authentication**, not storage. Its declaration in
  `specs/common/configuration.yaml` authorizes exactly one server-side operation: the admin
  `app_metadata` stamp writing `captain_customer_id` + `captain_role` at phone verification (#437).
  Unset, it fails **closed**: verification stands, the session is never rotated or parked, and login
  degrades to a fresh-OTP retry. So the key is a **login blocker**, not a data dependency — which is
  why "we host our own Postgres" does not retire it.

### The answer

Founder / Tech CEO, 2026-08-13, verbatim:

> **"For the auth/identify we will use Supabase because it's free and easier"**

## Decision

**Supabase Auth is retained as the identity provider for V0.** Self-hosting identity (GoTrue or any
replacement issuer) is **not** done now. The reasons are the founder's own — *free* and *easier* —
and both survive checking:

- **Free, at V0 volumes.** The provider's free tier (published as ~50k monthly active users — a
  vendor figure, not verifiable in this repo) sits against a Tours pilot of well under 500 users.
  The relevant half **is** repo-verifiable: against the unpaid €26.60/mo entry rung
  ([ADR-20260807-114122](ADR-20260807-114122-mks-starts-at-one-node.md), still unpaid under INV-1's
  spend gate), the identity delta is **zero**.
- **The SMS bill is identical either way**, because the SMS already goes out on **our own provider
  account**: the OTP is delivered by **OVHcloud SMS** through the Supabase *Send-SMS hook*
  (`crates/infrastructure/src/integrations/ovh_sms.rs`, our application/consumer keys and service
  name; [ADR-20260722-174500](20260722-174500-identity-federation-cross-tenant-personalization.md)
  chose OVH over Twilio for FR price + EU residency). Self-hosting the issuer would move **no**
  per-message cost, because the per-message cost was never the provider's.
  ⚠️ **Stale spec wording, uncorrected here**: `specs/architecture/c4-l3.yaml` (and therefore the
  generated `specs/generated/c4.generated.dsl` and `documentation.generated.md`) still describes the
  `supabase-acl` as sending OTP via *"Twilio; mock in dev"*. `specs/services.yaml` is correct
  (OVHcloud). The wording is a leftover of the Twilio→OVH correction, not a second provider.
- **Easier, and the mob was unanimous.** All ten lenses recommended not self-hosting now.

### The one technical reason that must not be lost, because it is a security regression

`crates/server/src/auth.rs` accepts **asymmetric JWKS algorithms only, deliberately**:

```rust
/// Resolve the algorithm from the matched JWK (falling back to the header only for asymmetric families).
/// Restricting to asymmetric algorithms defeats `alg`-confusion (no HS* downgrade against a public key).
fn asymmetric_alg(jwk: &Jwk, header_alg: Algorithm) -> Option<Algorithm> {
```

`is_asymmetric` admits RS*/ES*/PS*/EdDSA and nothing else, so an `HS256` token signed with a *public*
key — the classic algorithm-confusion forgery — cannot be minted into a session. Self-hosted GoTrue
defaults to a **symmetric** signing algorithm. "Swap the provider" therefore means **editing the token
verification path** — the one file whose bugs are silent authentication bypasses — days before a demo.
That is the difference between "a config change" and "a security-critical change under time pressure",
and it is why the unanimity was not merely about effort.

## THE DEADLINE — this is a V0 decision, not a permanent one

The window to reverse this cheaply has a **hard edge**, and both halves of it close at the **same
event**.

1. **Technical: `domain_events.user_id` holds the provider's subject id.** The column is declared
   `UserId` (uuid) and documented as *"the acting user — envelope metadata, ADR-0041 (not in
   payloads)"*; the value written is the **auth subject** — `Principal::user_id()` returns *"the
   Supabase `sub` — the AUTH subject, never a domain identity (#433)"*, and
   `request_envelope()` parses it straight into the envelope. A different issuer mints a **different
   subject** for the same human. On an **immutable log** that is an **upcasting migration**, not a
   spec edit — stored events are never mutated. Today the log is empty, so the migration is free.
2. **Legal: there is no real phone-verified end user** — the founder's own Q-L3 answer, **dated
   2026-08-12** ([ADR-20260812-214021](ADR-20260812-214021-the-founder-answer-sheet-of-2026-08-12.md)
   Decision 7). While that holds, switching issuers is **not a personal-data migration** and triggers
   **no processor-exit obligations** (no export, no documented deletion at the outgoing processor, no
   Art. 28(3)(g) return-or-delete exercise). The day one real user exists, that stops being true.

**Both windows close at the first real order.** Recorded plainly, because this is the sentence that
must survive the decision:

> **If we ever intend to own identity, it lands before the first real order — or it becomes
> materially more expensive.**

The same event is already the trigger for the Art. 35 DPIA deadline, the Art. 17 erasure trigger and
the médiation-de-la-consommation registration deadline (ADR-20260812-214021 Decision 7). This adds a
**fourth** thing that changes character at that instant.

## What stays true regardless of this decision

None of the following is retired by choosing Supabase; each is listed so it is not lost with the
question that surfaced it.

- **The claim contract is an invariant of *any* issuer.** Every `/{role}/graphql` boundary trusts
  `app_metadata.captain_customer_id` + `captain_role` (`crates/server/src/auth.rs`: the path role must
  equal the token's `captain_role`, and the domain binding comes from `captain_customer_id` — an
  authenticated caller with no binding is `Unbound` and denied). A different issuer must emit
  **exactly those claims**, or the **authorization boundary moves even though the GraphQL schema does
  not** — the most dangerous shape of change, because nothing generated would differ.
- **The SMS provider is our own processor either way.** OVHcloud SMS is a **processor** under Art. 28
  — contract, EU routing, purpose limitation — whoever issues the tokens. This decision does not
  remove that obligation, it merely leaves it where it already was.
- **SMS-pumping fraud is an unguarded money risk, today.** An unprotected OTP endpoint sending on our
  own OVH account can be driven to premium-rate ranges for hundreds of euros in a night. The guards
  named by the mob are a **+33-only allowlist** and **per-number / per-IP caps**. **Neither exists**:
  there is no rate-limit or allowlist code anywhere in `crates/server` or `crates/application`.
  Unchanged by this decision, and still owed.
- **The auth-session handoff has no observability contract at all.** `specs/observability.yaml`'s
  `customer-identification` contract covers the *command* path (`otp.verify`, `claims.stamp`,
  `customer_claim_stamp_failed_total`) and stops there: the session **park** and **pickup** emit
  nothing, and `crates/server/src/auth_routes.rs` collapses `absent | expired | wrong_owner` into one
  `404` (correctly — *"no existence oracle"*), `decrypt_failed` into a `500`, and `not_configured`
  into a `503`, **discarding the cause**. Refusing to tell the *caller* which it was is a security
  property; discarding it *server-side* means an operator cannot distinguish a misconfigured
  `AUTH_SESSION_KEY` from a user who waited too long. Unchanged by this decision, still owed.
- **The OTP rejection message has no translation key.** `specs/screens/captain_frontoffice.yaml:166`
  (and `restaurant_frontoffice.yaml:213`) render
  `{ type: inline_error, visible_when: otp_error, message: "{{ otp_error_message }}" }` — a **runtime
  binding**, not a `$ref` into the translations catalog, unlike every neighbouring label on the same
  sheet. The text a customer sees when their code is wrong is therefore un-localized prose. Verified
  in the generated renderer (`crates/web/src/generated/screens.rs:149,615` → `PropValue::Binding`).

## What this un-blocks, and how

For the next session working the local-rehearsal / demo path:

- **The identity leg needs TWO things, not one**: the existing `SUPABASE_SECRET_KEY` **and pod egress
  to the provider's JWKS endpoint** (`SUPABASE_JWKS_URL`). A key present in the cluster with egress
  blocked fails exactly like a key that is absent — every token unverifiable, every request anonymous.
  **Egress is checkable in minutes** and should be checked before the key is blamed.
- **The payment leg needs no ingress.** `stripe listen --forward-to` reaches the local stack *outbound*
  through the hosts entry the rehearsal runbook already writes, and the CLI prints its **own webhook
  signing secret**, which satisfies the fail-closed `STRIPE_WEBHOOK_SECRET` boot gate
  (`crates/server/src/lib.rs`). That is **real Stripe, a real signature and a real verification path**
  — not a shim, and not the webhook-ingress gap §35 INV-1 recorded against L4.

## Alternatives considered

- **(A) Retain Supabase Auth for V0 — CHOSEN.** €0 delta, no per-SMS change, no edit to the token
  verification path, no issuer migration on the event log. Cost: a vendor dependency on the
  authentication path, and a reversal price that grows at the first real order.
- **(B) Self-host GoTrue now.** Owns the issuer before any user exists — the cheapest moment there
  will ever be. Rejected on timing, not on merit: it requires touching `auth.rs`'s asymmetric-only
  verification (GoTrue's default is symmetric) days before a demo, and buys nothing at V0 volumes
  where the provider is free and the SMS bill is already ours.
- **(C) Build our own issuer.** Rejected outright: minting and rotating signing keys for a payment-
  bearing session is not V0 work, and every argument against (B) applies with more force.

## Consequences

### Positive
- Zero cost and zero code change for V0 identity; the demo path keeps one unknown (egress) instead of
  a rewrite.
- The security posture of the verification path is preserved by *not touching it*.
- The reversal price is now **written down with its trigger**, so a later switch is a decision with a
  known cost rather than a discovery.

### Negative
- A vendor sits on the authentication path, and its subject ids are being written into an **immutable**
  log. Every event appended from now on raises the price of (B).
- The three findings above (OTP rate limiting, auth-session observability, the untranslated OTP error)
  remain open and are **not** consequences of this decision — they are pre-existing and merely
  surfaced by the review it triggered.

### Follow-up actions
- **Before the first real order**, decide explicitly whether identity is owned. Silence past that
  point is itself the decision to stay, at the higher price.
- Three issues are **proposed to the coordinator, not filed here** (the executor has no GitHub
  access): (1) OTP rate-limit + `+33`-only allowlist; (2) an observability contract for the
  auth-session park/pickup with a typed `reason`; (3) a translation key for the OTP rejection message.
- Correct the stale *"Twilio"* wording in `specs/architecture/c4-l3.yaml` (a `specs/**` change, owed a
  SPEC-LOG row, deliberately **out of scope** for this records-only change).

## Consulted (ADR-20260812-143619)

All ten lenses were asked on the founder's previous message and **all ten recommended not
self-hosting now** — the unanimity is the finding. Where the founder's own reasoning quotes a lens,
it is attributed as such; the remaining findings are recorded against the lens whose remit they fall
in. **Nothing here is legal advice or clearance**, and agreement between lenses does not upgrade a
hedged finding to a settled one.

- **architect** — retaining costs no structure: ADR-0015 already puts the provider behind an ACL with
  the SDK out of the domain, so the dependency is contained by construction, not by discipline.
- **holub** — the **claim contract**, not the vendor, is the boundary: `captain_customer_id` +
  `captain_role` is the invariant any issuer must satisfy, and it is the thing a swap would silently
  move.
- **dba** — the store was never Supabase's; the only real coupling is `domain_events.user_id` on an
  **immutable** log, which makes a later switch an upcasting migration.
- **farley** — a provider swap is an edit to the token verification path days before a demo; that is
  a change you make when you can observe the result, and today you cannot.
- **beck** — nothing in the test surface asserts the issuer; there is no cheap experiment that makes
  self-hosting safer this week.
- **graphql-architect** — `asymmetric_alg` accepts asymmetric JWKS **only**, deliberately, to kill
  `alg`-confusion forgery at the `/{role}/graphql` door; GoTrue's symmetric default would regress it.
- **business-specialist** (quoted in the decision) — **€0 at V0 volumes** (free tier ≈50k MAU against
  <500 users in Tours) and the **SMS cost is identical either way**, since OTP already goes out on our
  own provider account. Delta against the unpaid €26.60 base: zero.
- **legal-specialist** — with **Q-L3 = no** (2026-08-12) a switch today is not a personal-data
  migration and triggers no processor-exit obligations; the SMS provider stays our Art. 28 processor
  regardless; both change at the first real order. *A grade, not clearance.*
- **ux-designer** — the OTP rejection message is a runtime binding with **no translation key**, so the
  worst moment of the login flow is the one moment that is un-localized.
- **observability** — the auth-session **park/pickup emits nothing**, and the route collapses five
  distinct causes into status codes; an operator cannot tell a misconfigured key from an expired
  session.
