# PROP-20260724-144500 — App flag `ADMIN`: display translation keys on screen
- **Status**: Proposed
- **Date**: 2026-07-24
- **Tracking issue**: [#111 "App flag ADMIN: display translation keys instead of strings (bridge to on-screen live translation)"](https://github.com/TheCaptainCompany/captain-food/issues/111)
- **Realized by**: (pending)

> **Related — deliberately separate** (product-owner direction: "I don't want to mix the things"):
> [PROP-20260724-133700 "Live spec editing + per-tenant customizations"](PROP-20260724-133700-runtime-screen-and-translation-delivery.md)
> owns the *publishing* pipe (branch → preview → gated merge) and the translation hygiene gates
> ([#110](https://github.com/TheCaptainCompany/captain-food/issues/110)); THIS proposal owns only the
> *display* mode. They meet at one seam (§4) and nowhere else.

## Why

Finding which translation key produces which on-screen string currently means grepping the catalog
and guessing at context. With live copy editing coming (#96) and the hygiene gates making keys a
first-class concern (#110), the team needs to SEE keys where they live: on the real screen.

## 1. The `flags` concept (first flag: `ADMIN`)

A minimal app-level flag mechanism — not a feature-flag platform:

- **Activation**: `?flags=ADMIN` on any URL → persisted in a `captain.flags` cookie → honored by
  BOTH the SSR path (the server reads the cookie when rendering) and the hydrated client. Removal:
  `?flags=` clears it.
- **Authorization (honest V0 stance)**: the flag only *reveals translation keys* — which are public
  in this repo — and changes nothing about data access (the role-pathed GraphQL ACL is untouched).
  So V0 ships it cookie-gated only; when client-side auth lands (the #93/#94 auth-token residual),
  activation tightens to require the ADMIN JWT, matching the flag's name.
- The mechanism is deliberately generic (`flags` = a set of tokens) so later flags (e.g. a debug
  grid, slow-network simulation) reuse it — but THIS proposal ships only `ADMIN`.

## 2. Key-display mode

- `crates/web`'s `i18n::resolve(key, locale)` gains a mode: when the `ADMIN` flag is active it
  returns the KEY itself (rendered distinctly, e.g. `⟦checkout.title⟧`) instead of the message.
  One switch, in the one function every string already flows through — SDUI screens, the
  hand-written checkout/tracking pages, toasts, sheets: everything shows keys, because everything
  resolves through it.
- Locale fallback/`[key]`-missing behavior is unchanged when the flag is off.

## 3. `data-i18n-key` attributes (flag-independent)

Every rendered string additionally carries its key as a DOM attribute
(`<span data-i18n-key="checkout.title">…`), whether the flag is on or not. This is the durable
half: tooling (browser devtools today, click-to-translate later) can always map screen → key
without flipping display modes.

## 4. The bridge to on-screen live translation (the ONE seam with #96)

If/when we are "good enough" (product-owner phrasing): clicking a key while the `ADMIN` flag is
active opens an edit box whose SUBMIT feeds the publishing flow of
[PROP-20260724-133700 §3](PROP-20260724-133700-runtime-screen-and-translation-delivery.md) — a
branch editing the per-language catalog file, preview, gated merge. This proposal only reserves the
seam (`data-i18n-key` is the addressing scheme); the editor UI and its service identity belong to
the other proposal's implementation and are OUT OF SCOPE here.

## 5. Verification

- SSR test: rendering with the flag cookie shows keys (`⟦…⟧` markers present, translated strings
  absent); without it, translations render — both over the same screen.
- The `data-i18n-key` attribute is present in both modes.
- `make rust` green; no spec change needed (this is renderer behavior, not DSL).

## Considered alternatives

- **A separate "translator preview" deployment** rather than a flag: heavier (another environment
  to keep current), and useless for the "which key is THIS string on production" question.
- **Browser extension / devtools-only** (`data-i18n-key` alone, no display mode): keeps the DOM
  half, but hunting keys in an inspector is exactly the friction this exists to remove.
