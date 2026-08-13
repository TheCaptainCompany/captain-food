# ADR-20260813-013211 — A token must prove the PRODUCT, not only the provider: issuer is mandatory, roles fail closed, and the product claim is a positive check

- **Status**: Accepted
- **Date**: 2026-08-13
- **Issue**: [#519 "Token audience is the Supabase constant, roles fail OPEN to Customer, and staging shares production's identity project"](https://github.com/TheCaptainCompany/captain-food/issues/519)
- **Relates**: [ADR-0047](0047-api-auth-supabase-jwt-jwks.md) (role-as-path, Supabase JWT verified via JWKS) ·
  [ADR-0015](0015-wrap-supabase-auth-behind-graphql.md) (Supabase Auth wrapped behind our GraphQL) ·
  [ADR-20260813-004634](ADR-20260813-004634-supabase-auth-is-retained-for-v0-and-the-window-closes-at-the-first-real-order.md)
  (Supabase Auth retained for V0) ·
  [ADR-20260803-234035](ADR-20260803-234035-compiler-first-a-check-is-the-fallback.md) (compiler first) ·
  [ADR-20260811-113000](ADR-20260811-113000-the-open-path-reads-credentials-and-current-is-tenant-scoped-by-host.md)
  (the open path reads credentials and degrades)

## Context

The founder is provisioning **one Supabase project to hold identity for every product of the group**.
That single fact invalidates the separators our verifier was built on, and two of the resulting holes
were already live before the shared project existed.

**What a token had to satisfy, and what each check was actually worth:**

| Check | Worth |
|---|---|
| Signature against our JWKS | The **project's** key. Every user of that project gets tokens signed by it — including a sibling product's. |
| `aud == "authenticated"` | **Nothing.** It is the Supabase constant carried by every user of every project. It proves the token is a user token, and stops there. |
| `iss == {SUPABASE_URL}/auth/v1` | Correct — but **optional**: `issuer` was an `Option<String>` derived only when `SUPABASE_URL` was non-empty, and `verify` read `None` as *skip the check*. |
| `app_metadata.captain_role` | **Fail-open**: `parse_role`'s catch-all was `_ => RequestRole::Customer`, and an absent claim took the same branch. |

Compose them. `specs/common/configuration.yaml` points **staging at production's Supabase project**,
so a staging session already verified in production; and with `SUPABASE_URL` unset the issuer check
was not merely weak, it was absent. Under the shared group project the failure generalises past our
own environments: a token from a *sibling product* has our issuer, our audience and our signature,
carries no `captain_*` claim, and therefore landed on `/customer/graphql` as an authenticated
**CUSTOMER** — the fail-open default doing exactly what its name says.

The suite could not have caught any of it. Every fixture built `issuer: None` and the JWT minter
emitted **no `iss` claim at all**, so the whole test file was issuer-blind by construction: it passed
while all four defects stood, which is what a gate never seen red is worth.

## Decision

**A verified token must prove the PRODUCT, not only the provider.** Five things, in order, each
necessary and none skippable — recorded in the `crates/server/src/auth.rs` module header:

1. **Signature**, from a JWKS key, asymmetric families only (unchanged, ADR-0047).
2. **`iss`**, equal to `{SUPABASE_URL}/auth/v1` — **mandatory in both senses**: unset configuration
   **refuses** rather than skipping, and the claim must be **present and a string** on the token, not
   merely non-contradictory (see below — the library's matcher is present-only).
3. **`aud`**, `authenticated` — kept as a shape check on the same present-and-a-string footing, and
   documented as evidence of nothing about who minted the token.
4. **`app_metadata.captain_food`**, present and carrying a role we recognise. Its **absence is a
   refusal**, not a default.
5. The granted role must **equal the path role** (unchanged).

### Matching a reserved claim is not requiring it — and the library made that the default

The first version of this change set the issuer matcher and stopped there, on a comment asserting that
`set_issuer` also makes `iss` required. **It does not.** In the pinned `jsonwebtoken 10.3.0`,
`Validation::new` seeds `required_spec_claims` with `{"exp"}` alone (`validation.rs:112-115`),
`set_issuer`/`set_audience` only assign the matcher (`:143-145`), and `validate()`'s `iss` and `aud`
arms both end in `_ => {}` (`:308-320`, `:325-349`) — so a token that simply **omits** the claim, or
carries a **non-string** one (`TryParse::FailedToParse`), falls through and passes **vacuously**. The
crate documents it: *"Validation only happens if `iss` claim is present in the token."* Found by
independent review, reproduced by a standalone probe on the same crate version, and seen red here as
`left: ["exp"]`.

That is not an outsider's attack — the token must still be signed by a key in our JWKS. It matters
because the premise of this ADR is a project whose **claim shaping is not ours alone**: an
access-token hook or custom-claim arrangement in the shared group project is exactly the actor that
can drop or retype `iss`, and the mandatory-issuer guarantee would then have been worth nothing while
reading as if it held.

The fix is **derived, not written out beside the matchers**: `Verifier::validation` computes
`required_spec_claims` from the matchers it actually set, so "matched but not required" is not a pair
that can be spelled there, and removing a matcher keeps the two in step. Because
`required_spec_claims` demands `TryParse::Parsed`, the one line covers the retyped-claim road as well
as the absent-claim one.

**The general lesson, which is not derivable from our code**: with any JWT library, *matched* and
*required* are two switches, and a claim-matching API that reads like an assertion usually is not one.
Assert the produced `Validation`, and mint a token with the claim removed.

### The issuer case is taken by the compiler, not by a guard

`Verifier { jwks_url: String, issuer: String }` is one value with one constructor. The
fail-open configuration was not a missing branch — it was a **state the type permitted**, and it is
now unspellable: there is no `None` for `verify` to interpret, `Verifier::validation` is the only
`Validation` this module builds and it always sets issuer *and* audience, and with the configuration
absent the whole verifier is absent (`AuthContext.verifier: Option<Verifier>`), so every role path
answers `503` and `/public` degrades to anonymous. Half a verifier is not a weaker verifier; it is one
that cannot tell one project's tokens from another's, so it is no verifier at all.

`parse_role` returns `Option<RequestRole>` and `AppMetadata::grant()` is the single place a verified
token becomes a role — `Principal::role_path` takes the product claim object that only `grant()`
yields, so "authenticated, role unknown, treat as customer" is not a value any call site can be
handed.

### Nesting, not renaming — because the provider's merge is shallow

Supabase merges `app_metadata` **shallowly** (`specs/services.yaml`). Under one product-owned key:
our write replaces our object wholesale and cannot touch a sibling's, theirs cannot touch ours, and
the object's presence is a claim about *us* that no other product's tooling produces by accident.
Renaming flat keys would have bought a longer prefix and none of that. The claim words are unchanged
(`role`, `customer_id`, `restaurant_id`, `restaurant_account_id`, `rider_id`); only the nesting is new.

The name has **one authority** —
`infrastructure::integrations::supabase_auth::PRODUCT_CLAIM_KEY`, on the ACL that writes it. `serde`
needs a literal for the reader's field, so the two sides are tied by a test that runs the writer's
**actual output** through the reader (`the_verifier_reads_what_the_claim_stamp_writes`): they live in
different crates with no shared type, and a transposition between them would otherwise surface first
as a production smoke timeout — customers signing in to a session that verifies as a stranger's.

### No read-side tolerance for the pre-nesting flat claims

Deliberate, and cheap to justify: **Q-L3 (2026-08-12) records no real phone-verified end user**, so
no live customer token carries the flat shape. The only producers were `tools/smoke/prod-smoke.sh`
(updated here) and the test suite. Flat keys already written to smoke users survive the shallow merge
as inert siblings — read by nothing, and explicitly *not* treated as a stamp, so a stale flat id
belonging to another customer cannot block the real stamp as a phantom conflict.

## Alternatives considered

- **Keep the issuer optional and add a boot-time guard that refuses to start when it is empty.** A
  check where the compiler reaches (ADR-20260803-234035): it leaves the fail-open state
  representable, so the next caller of `verify` can reintroduce it, and it says nothing about the
  three other paths (`routes.rs`, `bin_support.rs`, tests) that build an `AuthContext`.
- **Rely on the issuer alone and split staging from production.** Necessary and still owed, but it
  does not survive the premise: under a group-wide project, issuer no longer separates *products*,
  only *environments*.
- **Rename the flat claims (`captainfood_role`, …) rather than nest them.** Same shallow-merge
  hazard, and no single key for another product's write to be excluded from.
- **Tolerate the flat claims on read for one token lifetime.** The standard migration courtesy, and
  here it buys nothing: there is no live token to protect, and it would keep the fail-open shape
  readable for exactly as long as anyone forgot to remove it.

## Consequences

### Positive

- A sibling product's token, a staging token and a token with no product claims are all **refused**,
  by the same rule, for the same reason.
- The unvalidated-issuer state is gone from the type, not from a branch.
- Six behaviours that were previously unobserved are pinned, each seen red first — including the two
  the first round of this work asserted in prose and did not enforce.

### Negative

- `SUPABASE_URL` is now **load-bearing for authentication**, not only for the OTP flows: unset, every
  role path returns `503`. Both keys are already `required: [staging, production]`, and the fail-closed
  warning names both. A local developer running with a JWKS URL and no `SUPABASE_URL` — which used to
  authenticate with no issuer check — now gets `503`, which is the correct answer to that configuration.
- Any auth user stamped before this change must be re-stamped before its token authorizes anything.
  Idempotent and automatic (`stamp_decision` sees no nested object and PUTs), free of live impact by
  the Q-L3 finding above, and self-healing for smoke users on the next run.

### Follow-up (not done here, deliberately)

- **Staging must stop sharing production's identity project** —
  `specs/common/configuration.yaml` still points both `SUPABASE_URL`/`SUPABASE_JWKS_URL` at
  `zcshlzhiinwmpzujuiep`. That is a founder/provisioning action, not a code change; this ADR removes
  the *fail-open* half of the risk, not the shared-project half.
- **`role_not_customer` now covers two populations** on `/public` — a staff token and a token proving
  no Captain Food role at all. Splitting them is
  [#517 "auth telemetry"](https://github.com/TheCaptainCompany/captain-food/issues/517)'s subject; the
  contract text in `specs/observability.yaml` says so rather than leaving the widening silent.
- **OTP rate limiting** remains unbuilt
  ([#516](https://github.com/TheCaptainCompany/captain-food/issues/516)).
