# ADR-20260722-101500 — Per-surface translation sidecars (refines ADR-0033 single catalog)

## Status

Accepted (product-owner directive, 2026-07-22). Refines **ADR-0033** (single `translations.yaml` catalog);
builds on **ADR-20260722-091500** (SDUI screens taxonomy by audience).

## Context

ADR-0033 put **every** UI string in one `translations.yaml`. As SDUI surfaces multiply (restaurant front
office, Captain marketplace, restaurant back office, rider, system), a single file couples all surfaces'
strings together and is awkward to hand to a translator per surface. We also want a clear home for
shared strings and for future **backend/server-rendered** text (SMS/email/push templates) that is
distinct from screen UI text — while keeping `errors.yaml` as the backend anticipated-error catalog.

Naming note (from the discussion): the spec folder stays **`specs/screens/`** — it names the *spec*
(what the screens are); **"frontend"** denotes the *implementation* (`crates/web`, the Leptos renderer).

## Decision

1. **Shared strings stay in `specs/translations.yaml`** — `common.*` today, plus future
   backend/server-rendered i18n. This is the shared/backend catalog.
2. **Surface-specific strings move to a co-located sidecar** `specs/screens/<surface>.translations.yaml`
   — first **`restaurant_frontoffice.translations.yaml`**. **Sidecar, not inline**: the screen file stays
   layout-only (runtime-editable via Supabase per ADR-0033), and a translator gets one file per surface.
3. **Screens `$ref` the file that actually holds the key** — surface strings →
   `restaurant_frontoffice.translations.yaml#/<key>`, shared strings → `translations.yaml#/<key>`. Keys
   are **globally namespaced and unique across all files** (validator-enforced, new
   `translation-duplicate-key` rule).
4. **Codegen**: `is_source_file` recognizes `*.translations.yaml`; `load_model` globs
   `specs/screens/*.translations.yaml` (keyed BARE so §11 does not treat them as screen specs); the i18n
   validator (§10), the `translations.generated.json` emitter, and the docs translations table all
   **merge** `translations.yaml` + every sidecar via a shared `translation_entries()` helper. The
   generated bundle stays a **single flat catalog** (`{ "<key>": { en, fr } }`), so `leptos_i18n` is
   unaffected.
5. **Screen ref scope is now validated** — every `$ref` in a screen that is NOT an API binding
   (top-level `resolvers`/`actions`, or a screen's realtime `subscription`) is a content/text slot and
   MUST be a translation ref that resolves to a real entry (a key carrying `messages`). Two new rules:
   `screen-translation-ref-unresolved` (dangling/renamed key) and `screen-ref-out-of-scope` (a text slot
   pointing at the wrong file/scope, e.g. an api.yaml or scalar ref). This closes the previously
   unchecked gap where a screen could reference a non-existent or mis-scoped string.
6. **`errors.yaml` unchanged** — backend anticipated-error messages.

## Consequences

- `translations.generated.json` is **byte-identical** (same 149 keys, just sourced from two files) — **no
  runtime change**.
- Adding a surface = add `<surface>.yaml` + `<surface>.translations.yaml`; both are auto-discovered by the
  generic loader, no codegen edit.
- **Follow-ups** (out of scope here): per-surface `leptos_i18n` namespaces + lazy-load; moving the
  marketplace strings (`home.*`, `search.*`, `partner.*`) from `restaurant_frontoffice.translations.yaml`
  into `captain_frontoffice.translations.yaml` alongside the marketplace content-split (ADR-20260722-091500).
  **→ The string move is done by ADR-20260722-160000 (#75).**
