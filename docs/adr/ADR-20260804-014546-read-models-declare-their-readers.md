# ADR-20260804-014546 — Every read model declares its reader, on the component that consumes it

- **Status**: Accepted
- **Date**: 2026-08-04
- **Extends**: [ADR-20260802-170059](ADR-20260802-170059-client-surface-is-spec-gated.md)
  ("the declaration is the permission") from the write side to the read side
- **Governed by**: [ADR-20260803-234035](ADR-20260803-234035-compiler-first-a-check-is-the-fallback.md)
  ("compiler first; a check is the fallback") — see *Why this is a validator rule* below
- **Realized by**: [#305 "View_* read declarations: no spec says which surface reads which view"](https://github.com/TheCaptainCompany/captain-food/issues/305),
  a named hole in [PROP-20260802-130500](../proposals/PROP-20260802-130500-isolation-by-construction.md) §5

## Decision

`components.*.reads[*]` in `specs/architecture/c4-l3.yaml` declares which component consumes which
read model. It takes the same `$ref` targets as the existing `updates[*]` (`ProjectionView` /
`ProjectionTable`), so the whole declaration is **one row in `refs.rs`** — no new file, no new
vocabulary, no new grammar.

`read-model-no-reader` (**error**) replaces `view-no-query` (warning). A read model passes iff:

1. an `api.yaml` output type binds it (`reads:`) — the GraphQL chain, unchanged; **or**
2. a c4-l3 component declares it in `reads:` — the readers no GraphQL type can speak for; **or**
3. it is `internal: true` — the existing opt-out, unchanged.

(1) and (2) are **declarations**; (3) is the pre-existing **exemption**, carried forward unchanged and
used by exactly one model. No NEW escape hatch was added, deliberately: a required declaration shows
up in a diff, and a warning does not.

**A read model reached from GraphQL is declared by its api.yaml type binding and must NOT be
re-listed on `graphql-gateway`** — enforced by `gateway-declares-reads`, not left to prose, because
otherwise one blanket declaration there would satisfy the reader gate for every model at once. Any
OTHER component declares what it actually consumes, whether or not GraphQL also reads it: several
declared models are legitimately api-bound as well, and that is not duplication — a command handler
reading `Customer` to enforce an invariant is a different consumer from the `CustomerProfile` type.

## Why this is a validator rule and not the compiler

ADR-20260803-234035 requires asking whether the type system can make the mistake unspellable before
writing any gate. Here it cannot, for a reason that is structural rather than a matter of effort:
the property is *"this read model has a declared reader in `specs/**`"* — **a fact about YAML**.
There is no type system for YAML, and rustc cannot read `api.yaml`. That directive names this exact
territory ("spec↔generation drift, non-Rust artifacts") as where a gate is the right instrument, and
`op-uncovered-by-story` is the same shape and has never been read as a level-3 dodge.

This is **not** [#329](https://github.com/TheCaptainCompany/captain-food/issues/329) repeating. That
was a syntactic scanner over *Rust source*, re-deriving a boundary the compiler already enforced —
seven review rounds and ~191 lines to approximate what rustc did exactly. Nothing here scans Rust.

The compiler answer does exist for the Rust half, and this ADR is its **prerequisite**, not its
alternative: generating a read-port bundle so an undeclared `(component, read model)` pair has no
field — `E0609` — needs a declaration to generate *from*, and none existed. That is the successor
below, and the pairs this ADR introduces are exactly its input.

## Bounded claim — what this does NOT prove

**Nothing here proves the declaration matches the Rust — in either direction.**

- *False negatives*: a component that reads a model and forgets to declare it is invisible to this
  gate. The rule proves every read model has *a* declared reader; it does not prove every actual
  reader is declared.
- *False positives*: **a declaration may simply be a lie.** Nothing checks that a component named in
  `reads:` performs that read, so a declaration added to silence the gate satisfies it just as well as
  a true one. Today's eight declarations were verified call site by call site, and an independent
  review re-verified them — but that is a fact about this change, not a property of the gate.

Both directions close the same way, and only the same way: the generated port bundle, where the
declaration *is* the accessor.

The temptation is to close the gap with a source scan that walks `queries.rs` call sites and compares
them to the spec. **Do not.** That is #329 verbatim — a syntactic approximation of a semantic
property, maintained by review rounds, over a boundary a generated bundle would hold exactly. The
honest state is: this is a **completeness gate over the spec**, and the Rust side stays undeclared
until successor B lands.

Two further limits, recorded rather than fixed:

- **Referential tables are never checked.** `reference: true` tables (`City`, `RuntimePosture`,
  `PhoneCountry`, …) are valid `reads:` *targets* but are not in the `views` population, so no reader
  is required of them. Pre-existing asymmetry, deliberately left: including them would force
  declarations against config paths nobody has audited.
- **`View_RestaurantAccount` has zero readers, declared or actual, and its comment says otherwise.**
  `specs/database/projection_views.yaml:44` claims it is "consulted by the RegisterRestaurant
  handler". It is not: that handler does an **event-store fold** (`store.load("RestaurantAccount-…")`
  → `domain::restaurant_account::fold`, `crates/application/src/commands.rs:369-375`), never a
  read-model read — and there is no `RestaurantAccountReadRepository` in
  `crates/application/src/queries.rs` nor any such field on `CommandDeps`. So this model passes the
  gate purely by exemption, and the projection is maintained for nobody. Surfaced rather than
  silently "corrected", because the fix is a design call: delete the projection, or give the
  existence check a read port instead of a fold.

## Consequences

- **The gate is satisfiable on the committed spec with four declarations.** 13 of 15 read models were
  already bound by an api.yaml type. `SlugAlias` was the single `view-no-query` warning on `main` and
  is read perfectly legitimately — by the tenant host router's 301, which never touches GraphQL; it
  now has a `tenant-host-router` component. `View_RestaurantAccount` keeps `internal: true`.
  **`main`'s warning baseline drops 33 → 32.**
- **`c4-l3.yaml` gained a component** for `crates/server/src/hosts.rs`, which had no C4 representation
  at all despite being a live entry point.
- **C4 renders `reads` beside `updates`** in both generated doc surfaces. L3 previously showed
  projection *writes* and hid every read, which read as "nothing consumes these".
- **Declared readers are the ones the code actually has**, verified call site by call site rather
  than from the component descriptions: host router (`Restaurant` + `SlugAlias`), command handlers
  using a read as a write-side invariant (`Restaurant`, `Catalog`, `Customer`, `ProspectionPipeline`),
  process managers (`Cart`, `OrderTracking`), HubRise ACL (`Restaurant`). `prospection-acl`'s
  description claims it reads `ProspectionPipeline`, but that read lives in the command handlers
  today — so it is declared where the code is, not where the prose is.
- **`phoneCountries` was deleted** (product-owner call): the only V0 query reached by no screen and
  the only one of 32 with no wired resolver body — it advertised a `reads:` binding while returning
  `Err("not implemented")`. The `PhoneCountry` reference table stays.

## Successors

- **[#336 "Every V0 query must be reached by a screen resolver (query-unreached-by-screen)"](https://github.com/TheCaptainCompany/captain-food/issues/336)** — the backward mirror of `screen-unknown-resolver`:
  every V0 query must be reached by ≥1 screen resolver. Six queries remain unreached, all `slice: V1`,
  so V1 is exempt by construction and the rule is satisfiable today. Needs product decisions about the
  six, so it is a proposal rather than a mechanism.
- **[#337 "Generated ReadPorts bundle: make an undeclared read not compile"](https://github.com/TheCaptainCompany/captain-food/issues/337)** — the level-4 close. Modelled on `CommandDeps` /
  `emit_infra_command_router` and the conditional emission in `emit/actor_clients.rs`: one accessor per
  declared `(component, read model)` pair, undeclared → `E0609`. Two costs to name up front:
  `crates/application/src/queries.rs` stays **hand-written** (do not try to generate 709 lines of
  filters, doc comments and provided default bodies), and each read model needs a port name in the
  spec — the `View_X → XReadRepository` convention holds for 15 of 17 but breaks on
  `MailboxLaneRepository` and `RefundReadRepository ↔ View_PendingRefunds`.
