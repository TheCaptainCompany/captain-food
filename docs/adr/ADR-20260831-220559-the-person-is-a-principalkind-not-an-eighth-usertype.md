# ADR-20260831-220559 — The person is a `PrincipalKind`, not an eighth `UserType`

<!-- Filename: docs/adr/ADR-20260831-220559-the-person-is-a-principalkind-not-an-eighth-usertype.md -->

## Status

Accepted — realized in the same change (`specs/common/scalars.yaml`, the retyped `authRef` sites).

Closes [PRINCIPALS-MEMBER](../decisions/PRINCIPALS-MEMBER.yaml) (opened 2026-08-30, owner `team`),
which gated step 1 of [PROP-20260831-180622](../proposals/PROP-20260831-180622-staff-authentication-the-roster-the-invitation-and-the-door.md).
It does **not** discharge any of that proposal's four Concerns, and it does not move it to `Approved`.

## Enforced by

n/a — no behavioral guarantee. This ADR records a **vocabulary and migration posture**: it adds
three kernel scalars and retypes seven `authRef` sites. No rule changes, no runtime branch is added
or removed, and the emitted SQL and the stored JSON are byte-identical before and after (see
Consequences). The one executable consequence is negative and already enforced by `rustc`: an
`AuthSubject` and an `ExternalReference` are now distinct newtypes, so the two can no longer be
substituted for one another anywhere in the workspace.

## Context

Part C of [#639](https://github.com/TheCaptainCompany/captain-food/issues/639) needs a restaurant
**member** — a natural person acting within a scope — and the model has no person concept.
`RESTAURANT` is a *place* standing in for whoever holds the tablet. The register row asked one
question with four parts: what is the change to `actors.yaml` `principals`, `ScopeType`, `UserType`
and `requires.acting`, and **what is the versioning story**, given that `UserType` is stored on
every `domain_events` row ([ADR-0041](0041-acting-user-is-envelope-not-payload.md)) and `ScopeType` is stored in
`ScopeMembership`.

Two things were verified at `83b8154` before answering, because the row's own note marked the
cheapest reading `UNVERIFIED input`:

1. **`UserType` is a URL path, mechanically.** `tools/codegen-rs/src/emit/bins.rs`'s
   `user_type_roles` reads *this enum* and emits one gateway bin and one `/{path}/graphql` route per
   member; `crates/server/src/graphql/acl.rs` carries the matching seven-arm `RequestRole` table
   (`from_segment` / `segment` / `api_name`). So widening `UserType` with a person would mint an
   eighth role path, an eighth gateway and an eighth arm — as a **side effect of naming a person**.
2. **`principals` has exactly one consumer and it is a comment.** `grep -n '^\s*requires:'` over
   every `specs/*/api.yaml` returns nothing: zero `requires:` blocks exist. The only hit is prose in
   `specs/common/api.yaml:52`, about the unrelated `@auth(requires: […])` directive. The kernel
   change is therefore far cheaper than the row implied, and `requires.acting`'s semantics are
   entirely open — the emitter that will consume them belongs to
   [#636](https://github.com/TheCaptainCompany/captain-food/issues/636), not here.

A third fact was found while implementing, and it is the reason this ADR also covers a retype the
row never mentioned: **seven `authRef` sites were typed `ExternalReference`** — the scalar the kernel
declares as the HubRise `ref`, with examples `'MARGHERITA'` and `'CAT-PIZZAS'`. A catalog import key
and a human being's credential were the same type, in `specs/common/`, against CLAUDE.md's *one name
= one dedicated scalar*. Because both are `type: string`, nothing could see it: the two were
interchangeable at every boundary and the compiler had no grounds to object.

The count matters, and it is **seven, not the four** PROP-20260831-180622 §2.1 enumerates
(`customer/events.yaml` ×2, `delivery/events.yaml`, `delivery/commands.yaml`). The other three are in
`specs/services.yaml` — `verify_phone_otp`'s output, `verify_email_token`'s output and
`stamp_customer_claim`'s input — and they are **not optional extras**: the identity service's output
flows directly into the retyped event fields at `crates/application/src/commands.rs:3432-3518`, so
retyping the four without the three does not type-check. The four-site reading is a derived number
that was never verified against the corpus ([ADR-20260817-105845](ADR-20260817-105845-a-dispatch-card-may-not-state-a-derived-number-without-its-antecedents.md)).

## Decision

**1. `MEMBER` goes on a NEW `PrincipalKind` scalar, and `UserType` does not widen.**
`PrincipalKind` = `CUSTOMER | RESTAURANT | RESTAURANT_ACCOUNT | RIDER | MEMBER` — one member per
entry in `actors.yaml`'s `principals` map, plus the person. It is the vocabulary of **who acts**;
`UserType` stays the vocabulary of **which door they came through**. `PUBLIC`, `ADMIN` and
`EXTERNAL` are absent for the same reason they are absent from `principals`: no resolved domain
identity, so they can never be a member of anything.

**2. The versioning story is that there is nothing to version.** `PrincipalKind` is a brand-new
scalar with **no stored history** — nothing has ever been written with this type — so `MEMBER` costs
no upcaster, no re-attribution and no migration. This is the whole reason the vocabulary is added
here rather than onto `UserType`.

**3. `RESTAURANT` stays a legal `UserType` value forever.** The log is immutable (ADR-0041) and
historical rows mean what they meant. Correct vocabulary is added **alongside**; `RESTAURANT`
becomes a value no *new* event carries, and it is never renamed. The row's "cost window" — the sole
claim writer hardcodes `CUSTOMER`, so no `domain_events` row was ever authored by a `RESTAURANT`
principal, making a rename cheap *today* — is therefore **moot rather than exploited**: we decline
the rename on principle, so no production `SELECT user_type, count(*)` is needed to license this
decision, and the `UNVERIFIED input` marker on that figure is discharged by not depending on it.

**4. `ScopeType` is untouched.** It names the kind of protected **instance** one belongs to
(`ORDER`, `RESTAURANT`); this names the kind of party doing the belonging. A member is not a thing
others are members of.

**5. `requires.acting` stops being a scalar equality and becomes a membership question.** Once the
PERSON is the principal, "is `actor.id` equal to the caller's domain id?" is the wrong question: a
member acts **within** a scope by grant, so the predicate is *is this principal a member of
(scopeType, scopeId)?* — the question `ScopeMembership` already exists to answer. This ADR states
those semantics and **builds no emitter**: with zero `requires:` blocks in the corpus, there is
nothing to migrate and #636 owns the consumer.

**6. `AuthSubject` and `MemberId` are minted in `specs/common/`**, and the seven `authRef` sites move
off `ExternalReference` onto `AuthSubject`. `MemberId` is deliberately **not** `RestaurantMemberId`:
a person is not restaurant-shaped, scope is an axis on the *membership*, and naming the person for
one scope width would bake back in the collapse this vocabulary exists to undo. `AuthSubject` is an
**authentication** fact and never an authorization one — `ScopeMembership.member_id` holds the domain
id, never this, because the sub→domain bridge happens once per request at the edge
([ADR-20260818-004646](ADR-20260818-004646-no-business-identifier-lives-in-the-identity-provider.md)).

## Alternatives considered

- **Widen `UserType` with `MEMBER`.** Rejected on verified mechanics, not taste: `user_type_roles`
  would emit a `/member/graphql` path and gateway bin, and `RequestRole` would need an eighth arm —
  a URL surface conjured by naming a person. It is also the enum stored on every `domain_events`
  row, so it is the one we can least afford to reshape. `young`'s deploy-order cost (readers before
  writers, because `from_text` is strict while the writer is stringly typed) applies to any widening
  and is avoided entirely by not widening.
- **Rename the stored `RESTAURANT` token while the cost window is open.** Rejected. The window is
  real — no event was ever authored by a `RESTAURANT` principal — but a legal value of an immutable
  log is not renamed because renaming happens to be cheap this week. Adding alongside costs nothing
  and stays correct after the window closes.
- **Put `MEMBER` on `ScopeType`.** Rejected as a category error: `ScopeType` names protected
  instances, and nobody is a member of a member.
- **Leave `authRef` on `ExternalReference`** and mint only `PrincipalKind`/`MemberId`. Rejected: the
  kernel would keep declaring that a person's credential and a HubRise catalog ref are one type, and
  the retype is free — same string, same TEXT column, same JSON (proven below).
- **Also retype the `by_auth_ref` read port to `AuthSubject`** in the same change. Deferred, not
  rejected — see Follow-up. It reaches the codegen emitter (`server_graphql.rs:703` hardcodes
  `ExternalReference` in the emitted `me` resolver) and a file fenced to a concurrent session.

## Consequences

### Positive

- A person's credential and a catalog import key are **no longer the same type**. The confusion was
  unspellable-in-review and is now unspellable-in-`rustc`, which is the compiler-first ordering
  CLAUDE.md asks for (a check would have been the fallback; here the type system reaches).
- **Nothing stored changes.** Regenerating produced a **zero-byte diff** in
  `specs/generated/schema.generated.sql`, `views.generated.sql`, `security.generated.sql`,
  `security.permissive.generated.sql` and `databases.generated.json`: `type: string` emits `TEXT`
  irrespective of the scalar's name or `maxLength` (`tools/codegen-rs/src/emit/sql.rs`). No DDL, no
  migration, no backfill, no replay.
- The GraphQL change is **purely additive**: three new type declarations (`scalar AuthSubject`,
  `scalar MemberId`, `enum PrincipalKind`) and **not one field retyped**, because no API field is
  bound to `authRef` today. This is not a non-additive schema change.
- Part C step 1 is unblocked without spending the proposal's remaining decisions.

### Negative

- **`MemberId` and `PrincipalKind` have no consumer yet.** They are vocabulary minted ahead of the
  aggregates that use them (`RestaurantInvitation`, `RestaurantMembership`, later steps). This is
  deliberate — the register row's answer *is* the vocabulary — but a scalar with no reference is a
  promise until something references it. **And nothing enforces this bullet**: the validator has
  `translation-key-unused` and `view-fedby-unused` but no `scalar-unused` rule, so the promise is
  kept by prose alone. Filed as part of [#836](https://github.com/TheCaptainCompany/captain-food/issues/836)
  (a warning, not an error — minting vocabulary one step ahead is deliberate here).
- **The `by_auth_ref` port still takes an `ExternalReference`** ([#836](https://github.com/TheCaptainCompany/captain-food/issues/836)),
  so the identity bridge itself is the one place the old confusion survives. It is now *visible* rather than invisible: the fake
  repository and the projection test each name both scalars, with a comment saying why.
- The same **three** generated GraphQL type declarations named in Positive (`scalar AuthSubject`,
  `scalar MemberId`, `enum PrincipalKind`) are added to a schema no client reads yet — a small,
  reversible surface increase. (This bullet read "Two generated GraphQL scalars", contradicting
  Positive's three by dropping the enum; corrected in the same PR.)

### Follow-up actions

- **Step 1b — retype the `by_auth_ref` read port to `AuthSubject`** ([#836](https://github.com/TheCaptainCompany/captain-food/issues/836)).
  **Ten edit sites**, enumerated from `grep -rn "by_auth_ref" crates/ tools/` at `3587e32`, because
  retyping a trait parameter forces every `impl` signature (Rust requires an exact match) plus every
  hand-written caller: the **trait decl** (`crates/application/src/queries.rs:345`); **five impls**
  (`crates/infrastructure/src/persistence/customer.rs:54`,
  `crates/application/src/behaviour_support.rs:904`, and the three `Empty` fakes at
  `crates/server/tests/graphql_subscriptions.rs:196`,
  `crates/server/tests/storefront_menu_paint.rs:238`, `crates/server/tests/graphql_cart_read.rs:356`);
  the **`me` resolver emitter** (`tools/codegen-rs/src/emit/server_graphql.rs:703`, whose output
  `crates/server/src/graphql/generated/query.rs:136` regenerates and is never hand-edited); and
  **three hand-written callers** (`crates/infrastructure/src/mailbox/handler.rs:400`,
  `crates/server/src/auth.rs:2181`, `crates/infrastructure/tests/main/customer_projection.rs:147`).
  Kept out of this change because `handler.rs` was fenced to a concurrent session and the emitter is
  a codegen change. **`crates/server/src/auth.rs:2181` is the substantive one**: the gated
  `RESOLVE_CUSTOMER_IDENTITY_FROM_POSTGRES` sub→domain resolver — the bridge-at-the-edge that
  `AuthSubject`'s own docstring cites [ADR-20260818-004646](ADR-20260818-004646-no-business-identifier-lives-in-the-identity-provider.md)
  for, the one site where the type and the doctrine most conspicuously still disagree, and neither
  fenced nor generated. **This entry read "Three edits" when first written** — a derived number
  stated without its antecedents, the exact defect [ADR-20260817-105845](ADR-20260817-105845-a-dispatch-card-may-not-state-a-derived-number-without-its-antecedents.md)
  names; caught by the independent reviewer pass on PR #835 and corrected in the same PR.
- Steps 2–7 of PROP-20260831-180622 proceed on this vocabulary. Its four Concerns remain unchecked.
- When `requires.acting` gains its first real block (#636), the membership predicate decided in §5
  is what it must compile to — not an id comparison.
