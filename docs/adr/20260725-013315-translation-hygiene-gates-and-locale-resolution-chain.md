# ADR-20260725-013315 — Translation hygiene gates + the runtime locale-resolution chain

## Status

Accepted (realizes [PROP-20260724-133700](../proposals/PROP-20260724-133700-runtime-screen-and-translation-delivery.md) §1c; tracking issue [#110](https://github.com/TheCaptainCompany/captain-food/issues/110)).

## Context

Live copy editing (the #96 proposal) multiplies translation churn, and the catalog had no gate
against rot: nothing forced full locale coverage as a *named* rule, nothing caught a key that no
screen used, and nothing caught a hand-written-Rust key that referenced a catalog entry that did not
exist. Separately, the runtime hard-coded the locale to `fr` at every SSR call site, so
`Accept-Language`, a user's choice, and `Customer.locale` were all ignored — and the catalog is keyed
by bare tags (`fr`) while `Customer.locale`/`Accept-Language` speak full tags (`fr-FR`).

## Decision

**Two blocking `make validate` rules + a code-reference escape hatch, and a request-scoped SSR locale
chain.**

1. **`translation-locale-missing`** — every catalog key must carry a message in every
   `SUPPORTED_LOCALES` (one centralized list in the codegen, replacing three hard-coded `["en","fr"]`
   sites). A new `en` string without its `fr` cannot ship.
2. **`translation-key-unused`** — a key referenced by no screen `$ref` **and** no `code_refs` entry is
   a hard error (delete it, or declare it). `translation-code-ref-unknown` catches a manifest entry
   that matches no catalog key.
3. **`specs/translations.code_refs.yaml`** — the declared list of keys consumed by hand-written Rust
   (not on any SDUI screen), e.g. `order.status.*` via `tracking.rs`. A companion codegen test greps
   `crates/**/*.rs` for each entry so a *stale* manifest entry (matching no code) is itself caught.
   The rules live in an extracted `validate_translations` fn (mirrors `validate_ref_kinds`) so tests
   exercise them on minimal fixtures.
4. **Runtime chain** — `resolve_locale(Customer.locale → cookie → Accept-Language/device → default fr)`
   with `normalize_locale` reducing any tag (`fr-FR`, `EN`, `en_US`) to a bare SUPPORTED locale; SSR
   (`hosts.rs`) reads the `captain_locale` cookie + `Accept-Language` and threads the resolved locale
   through every render call site (no more hard-coded `fr`). `<html lang>` carries the resolved
   locale, and **hydrate reads it back from the DOM** so the client re-render cannot disagree with the
   server shell (SSR is the source of truth, no flash). `resolve()` normalizes its locale so a full
   tag still hits the bare-keyed catalog.

## Alternatives considered

- **Per-key `used_by: code` annotation** instead of a separate `code_refs` manifest — rejected: a
  single file is easier to audit and the grep-staleness test needs one place to read.
- **A per-request JWT→`Customer.locale` read in SSR** for the top rung of the chain — deferred: it
  needs auth-context extraction in the fallback handler. Instead `Customer.locale` reaches SSR via the
  `captain_locale` cookie (the language switch sets both; `changeLanguage` already persists
  `Customer.locale`). Noted as a follow-up.
- **Reordering the catalog to one-file-per-language** (proposal §1b) — out of scope here; it is a #96
  concern for conflict-free live editing. The coverage rule works on the current `messages:{en,fr}`
  shape.

## Consequences

### Positive
- The catalog cannot rot: missing locales, dead keys, and stale code-refs all fail CI. The gates
  already caught two real drifts on landing (`order.not_found` missing from the catalog though
  `tracking.rs` referenced it; `order.tracking_title` over-declared in `code_refs` when it is
  screen-`$ref`'d).
- Pages render in the visitor's language (cookie/`Accept-Language`), full tags resolve, and hydrate
  matches the server locale.

### Negative
- Adding a locale is now a deliberate act: it must be added to `SUPPORTED_LOCALES` **and** every key
  must gain that message before anything ships (by design).

### Follow-up actions
- A visible language-switcher (a screen action calling `changeLanguage` + writing `captain_locale`) —
  `changeLanguage` is unreferenced by any screen today; the cookie contract + SSR read are in place.
- A per-request `Customer.locale` SSR read (top rung) once auth context is available in the fallback.
- One-file-per-language catalog restructuring rides #96.
