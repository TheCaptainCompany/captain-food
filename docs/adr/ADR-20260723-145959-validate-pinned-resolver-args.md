# ADR-20260723-145959 — Pinned SDUI resolver args are validated against the bound query

- **Status**: Accepted
- **Date**: 2026-07-23
- **Refines**: ADR-0033 (Spec-Driven SDUI), ADR-20260722-152201 (the ref-KIND contract)
- **Realizes**: #82 "SDUI resolver pins a non-existent arg: restaurants.featured uses listKey,
  RestaurantsQueryInput has list"

## Context

A `screens/*.yaml` resolver may pin **static arguments** on its query binding:

```yaml
restaurants.featured: { query: { $ref: 'api.yaml#/queries/restaurants' }, args: { list: RECOMMENDED } }
```

Both customer front offices carried this pin with the key spelled **`listKey`**, but
`api.yaml#/queries/restaurants` declares the argument as **`list`**. The home screen's featured rail
would therefore have sent `input: { listKey: "RECOMMENDED" }` — an unknown input field, rejected by the
server, so the rail would fail rather than show the RECOMMENDED shelf.

The validator did not catch it. §1 proves a `$ref` **resolves**; §1b (ADR-20260722-152201) proves *what
kind* of thing it resolves to — here, that `resolvers.<key>.query` really is a `Kind::Query`. Neither
looks **inside** the `args:` map. This was the only `args:` pin in the whole spec, which is why the typo
survived from the screens DSL's introduction until #80 "Frontend split 2/4" became the first code to
actually consume a pin.

This is the same lesson as the ref-KIND contract, one level deeper: **a binding being valid says nothing
about the arguments carried on it.**

## Decision

The pin is corrected (`listKey` → `list` on both surfaces), and — per ADR-0032's completeness principle,
where the fix for a *class* of error is a new check rather than a one-line correction — the gate hole is
closed by `validate_resolver_args` in `tools/codegen-rs/src/main.rs`, called from validator §11 on the
branch where the query binding has just been proven valid. Two fail-closed rules:

- **`resolver-unknown-arg`** — the pinned key is not an argument of the bound query. The message lists
  the query's actual argument names (or states that it declares none), so the error alone carries the
  fix. A query that declares no `args:` at all rejects every pin; it does not skip the check.
- **`resolver-invalid-arg-value`** — the argument *is* declared and its `$ref` target carries an
  `enum:`, but the pinned literal is not a member. `array: true` pins validate each item. This mirrors
  the enum-literal check inside `check_shape` (rule `test-invalid-enum-value`, added by #24); it is
  implemented inline rather than by calling `check_shape`, because that function resolves refs against
  `tests.yaml` and would misresolve an api.yaml-local `#/types/...` ref.

Both are **errors**, not warnings: a pin that cannot be honoured is a broken screen, and `make validate`
is the single blocking gate.

## Explicit non-goals

- **Required-arg coverage is NOT checked.** A pin is a static *default*; the remaining arguments are
  supplied by the caller at runtime (`crates/web/src/graphql.rs#execute_resolver` merges caller
  variables **over** the pins, caller winning). An unpinned required argument is normal, not an error.
- **Scope is `resolvers` only.** The `actions:` block has no `args:` pin — not in the DSL, not in the
  emitter's `ActionDef`, and no surface declares one. If action pins are ever introduced, they need the
  same check against the bound mutation's payload.
- **No `REF_CONTRACT` entry.** Pinned argument values are plain scalars, not `$ref`s, so §1b is
  unaffected.

## Consequences

- A typo in a pinned resolver argument is now caught by `make validate`, at spec-authoring time, on
  every surface — instead of at runtime, in the browser, after the screen ships.
- The rule is proven against the live bug: with the check in place and the pin uncorrected, `make
  validate` reported `resolver-unknown-arg` on **both** `captain_frontoffice.yaml` and
  `restaurant_frontoffice.yaml`.
- `crates/web/src/generated/data_layer.rs` regenerates (`("listKey", …)` → `("list", …)`); the
  hand-written `crates/web/src/graphql.rs` doc comment and its two pin-merge tests move to `list`
  (the override test's value `NEARBY` — never a `RestaurantListKey` member — becomes `TOP_DEALS`).
- No `specs/rules.yaml` / `tests.yaml` entry: this is a codegen-validator gate, not a business
  guarantee — the same treatment as `test-invalid-enum-value` and `ref-site-undeclared`. Its coverage
  lives in the codegen unit tests (4 new, incl. the #82 regression).
