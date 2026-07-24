# PROP-20260724-133700 — Runtime SDUI screen AND translation delivery (ADR-0033 deferred contract)
- **Status**: Proposed
- **Date**: 2026-07-24
- **Issue**: #96 "Runtime SDUI screen delivery: serve screen trees as data (ADR-0033 deferred contract)"
- **Realized by**: (pending)

> **Status:** Proposed — plan-mode proposal (wire format + storage + delivery + safety). **No
> `specs/**` or code changed yet.** On approval it becomes an ADR that lands with the implementation.
>
> **Revision 3 (2026-07-24, product-owner direction):** **translations join the runtime path** — the
> UI string catalog must be editable in production through the repo exactly like the runtime
> screens (same storage, same gate, same phases). See §1b/§2/§3/§4 additions.
>
> **Revision 2 (2026-07-24, product-owner direction):** storage moves from a Postgres table to **the
> repo itself** — runtime screen specs are committed YAML under `specs/screens/runtime_screens/`,
> gated by the same `make validate` at build time. The database variant is kept as a considered
> alternative (§8). (Authored by the @claude GitHub app on branch `claude/issue-96-20260724-1337`,
> landed here under the PROP convention of ADR-20260724-135945.)
>
> **Owns:** closing ADR-0033's original *"runtime-editable screens"* promise, deferred a second time by
> ADR-20260723-172013 (which realized `specs/screens/*.yaml` as build-time codegen instead). This is
> **residual 5** of that ADR, tracked as **#96**.
>
> **Refs:** ADR-0033 (Spec-Driven SDUI), ADR-20260723-172013 (generated screen trees + BFF web serving),
> #87 (frontend split 4/4), ADR-20260722-091500 (screens audience taxonomy), ADR-0037 (system
> surface / impersonation model).

---

## TL;DR

The DSL (`specs/screens/*.yaml`) stays the source of truth for the **default** tree — build-time
codegen (ADR-20260723-172013) doesn't go away. This proposal adds a **second, optional path**: a
per-`(surface[, slug])` **override**, stored **in this repo** as more DSL —
`specs/screens/runtime_screens/<surface>[_<slug>].yaml` — that the BFF serves **instead of** the
generated default when one exists.

Storing overrides as committed spec files (not database rows) buys four guarantees the DB variant
could only approximate:

1. **Compatibility is known at BUILD time, and fixed in the repo.** Runtime screens run through the
   exact same `make validate` gate as every other spec file. A deploy that renames a component,
   resolver, action or translation key a runtime screen depends on **fails CI before it ships** — the
   blackout scenario is caught where it is cheapest to fix, in the same commit that caused it, not
   discovered at serve time in production.
2. **Branch-testable with zero infrastructure.** A layout experiment is a branch. Point a preview
   deployment (or Captain Studio's mounted checkout) at the branch and test the full runtime tree —
   no production database, no copy of it, no seed scripts.
3. **Versioning, rollback, review and audit are git.** Publish = merge; rollback = revert commit;
   history = `git log` on one file; review = a PR diff a human (or CODEOWNERS rule) can gate.
4. **No shadow spec.** The override lives next to the spec it overrides, in the same repo, validated
   by the same gate, diffable against the default at any time. Promoting an override into the
   default is moving YAML between two files in one commit — not an export from a live table.

Fail-safe stays: if an override is missing or fails validation at serve time, the BFF serves the
generated default tree — never a blank screen.

---

## 1. Storage — the repo, one file per surface (+ slug for tenant scope)

New directory: **`specs/screens/runtime_screens/`**. One file per override target:

| file | overrides | scope |
|---|---|---|
| `captain_frontoffice.yaml` | the marketplace surface | platform-wide (ADMIN-authored) |
| `restaurant_frontoffice.yaml` | the storefront surface default | platform-wide (ADMIN-authored) |
| `restaurant_frontoffice_<slug>.yaml` | the storefront **for one restaurant** | tenant-scoped — `<slug>` is the restaurant slug that fronts `{slug}.captain.food` |

The filename prefix **is the surface** (it matches the base file in `specs/screens/` exactly); the
optional `_<slug>` suffix is the tenant. Resolution order for a storefront request is most-specific
first: `restaurant_frontoffice_<slug>.yaml` → `restaurant_frontoffice.yaml` (platform override) →
the generated default from `specs/screens/restaurant_frontoffice.yaml`.

**File shape — overrides only, allowlists inherited.** A runtime file declares `screens:` entries
(keyed by the base surface's screen `id`, overriding whole screens) and NOTHING else: no
`component_registry`, no `resolvers`, no `actions`, no design tokens. The base surface's allowlists
remain the only law — a runtime screen can rearrange, restyle and re-copy, but cannot reach data or
mutations the base surface never declared. (Whether a runtime file may introduce a NEW screen id —
e.g. a seasonal promo page — or only override existing ones is left to the landing ADR, §7.)

Translation keys used by runtime screens resolve against the merged catalog INCLUDING the runtime
translation overrides of §1b — a runtime-only string ships in the same PR as the runtime screen that
uses it, enforced by the same validator.

## 1b. Runtime translations (revision 3) — same mechanism, same directory family

The string catalog gets the identical override path. New files next to the screen overrides:

| file | overrides | scope |
|---|---|---|
| `specs/screens/runtime_screens/translations.yaml` | the merged catalog (`translations.yaml` + sidecars) | platform-wide (ADMIN-authored) |
| `specs/screens/runtime_screens/translations_<slug>.yaml` | the catalog **for one restaurant's storefront** | tenant-scoped |

Rules (all `make validate`-enforced, same errors.yaml-style shape as the base catalog):

- **Overriding an existing key**: the override keeps the base key's declared `params` contract — a
  `{placeholder}` the base declares must appear in every overridden message (`translation-param-mismatch`
  already checks this shape) and no new placeholder may be invented. Copy changes, contracts don't.
- **New keys** are allowed only when referenced by a runtime SCREEN file in the same repo state —
  a runtime-only string with no consumer is a validator warning (dead key), and a runtime screen
  referencing a key that exists nowhere (base + runtime merged) stays the existing hard error.
- **Locale completeness**: every entry carries `en` + `fr`, like the base catalog — a runtime
  override cannot drop a locale the platform serves.
- Resolution order mirrors the screens: `translations_<slug>` → `translations` (runtime platform) →
  the base merged catalog. Per-key overlay, not per-file replacement — one overridden string never
  forks the whole catalog.

Client/server consumption is the same "loader, not a rewrite": `crates/web`'s `i18n::resolve`
currently reads the EMBEDDED generated catalog (`include_str!`, fr default / en fallback /
fail-visible `[key]`); it gains an OVERLAY layer consulted first, populated from the runtime
catalog artifacts of §2 — embedded catalog stays the permanent fallback, so a missing or invalid
runtime catalog can never blank a string. The SSR side loads the same artifacts server-side, so
server-rendered pages and hydrated pages always agree on copy.

## 2. Wire format

Unchanged from revision 1, now covering runtime files too. The generated `Screen` / `Node` /
`PropValue` Rust shapes (`crates/web/src/generated/screens.rs`, ADR-20260723-172013) are the
**versioned contract**:

- The codegen's `emit_web_screens` gains a sibling JSON emitter serializing the **same** trees it
  builds for Rust — for base surfaces AND every `runtime_screens/*.yaml` file — e.g.
  `specs/generated/screens/<surface>[_<slug>].json`. One source (the DSL), two encodings (Rust table
  + JSON), one emitter pass — structurally impossible to drift.
- **Translations (revision 3)**: the existing translations emitter does the same for the overrides —
  `specs/generated/runtime/translations[_<slug>].json`, each holding ONLY the overlay (per-key), in
  the same flat `key → {en, fr}` shape as `translations.generated.json`. Loaders overlay at run
  time; the base generated catalog remains the embedded fallback.
- Because runtime files are compiled by the same pass, there is **no separate schema-version
  problem**: a wire-shape evolution and every stored override move through CI together, in the same
  commit, or the build fails.

## 3. Delivery

The BFF, per `(surface, screen_id, tenant)` request, walks the resolution order of §1 and serves the
first tree that exists and validates. Two serving modes, phased:

- **Phase A — baked (ships with the emitter, nearly free):** runtime trees are compiled into the
  image like everything else. "Publish" = merge to `main` → the spec-only CI/deploy path. Layout
  iteration cost drops from "full feature deploy" to "merge a YAML PR"; latency is one CI cycle.
- **Phase B — live fetch (closes the no-redeploy promise):** the BFF fetches
  `specs/generated/screens/*.json` **and `specs/generated/runtime/translations*.json`** from the
  repo (raw content at a pinned ref/`main`, ETag-cached, short TTL) and hot-swaps trees AND string
  overlays on change — publish latency becomes "merge", with **no deploy at
  all**. The baked tree of Phase A remains loaded as the permanent fallback. Note the BFF fetches
  the **generated JSON**, never parses YAML at runtime — the codegen stays the only YAML reader.

Either way the renderer's input type doesn't change — this is still the "loader, not a rewrite"
promised by ADR-20260723-172013.

## 4. Safety

Three checkpoints now, and the important one moved left:

- **Build time (the primary gate, new in revision 2):** `make validate` extends to
  `runtime_screens/*.yaml` — screen ids resolve against the base surface, component types against
  the registry, data needs against the surface's `resolvers`, action types against its `actions`,
  strings against the translation catalog (base + runtime overlays, §1b rules incl. the params
  contract and locale completeness), filename prefix against an existing surface. This is the
  answer to *"check the stored tree is still compatible before deploying, to avoid a blackout"*:
  the check runs in CI on **every commit that could break it** (a component rename breaks the build,
  not the storefront) and the fix happens in the repo, in the same PR, before anything ships.
- **Serve time (Phase B only):** structural re-validation of the fetched JSON (registry +
  allowlists + key existence) before hot-swapping; a fetch failure or invalid tree keeps the last
  good tree, falling back to the baked default. This guards the one gap build time cannot see —
  skew between the deployed image and a newer `main` — and it is deliberately the same shared check,
  not a re-implementation.
- **Observability contract:** any serve-time fallback or fetch failure feeds a contract in
  `specs/observability.yaml` (this repo's rule: every critical workflow has one). A fallback is an
  operator signal ("runtime screen for `<slug>` is stale/invalid — fix the file"), never a silent
  degradation.

Because base specs and runtime overrides live in one repo and one gate, the "same validation, twice,
without drift" problem of revision 1 (write-time vs read-time vs codegen re-implementations)
collapses: there is **one validator** (the codegen), and serve-time re-checks reuse the same
generated allowlist tables (`ResolverKey` / `ActionKey` / registry) the renderer already carries.

## 5. Who may edit — authoring is Captain Studio, authorization is git

Editing a runtime screen is **editing a spec file**, so the authoring surface already exists:
**Captain Studio** (spec workbench) — typed component-tree forms, translation-resolved preview,
registry/allowlist checks, diff/revert, and the product gate — pointed at a branch of this repo.
The flow: Studio edits `runtime_screens/<surface>_<slug>.yaml` on a branch → preview → PR → merge
publishes. No hand-written `Node` JSON anywhere; humans never touch the wire format.

Authorization maps to git, not to a new mutation set:

- **ADMIN** (platform-wide files): repo write access + PR review; optionally CODEOWNERS on
  `specs/screens/runtime_screens/`.
- **RESTAURANT self-service** (their `_<slug>` file): Studio, authenticated as the tenant, commits
  through a service identity that is **structurally scoped** — it may only create/modify
  `restaurant_frontoffice_<their-slug>.yaml`. The filename IS the scope boundary, checked by the
  committing service and re-checked by the validator. Whether self-service ships with ADMIN editing
  or is phased in later stays a product decision (§7).

This refines ADR-0037's impersonation-only stance without inventing a new admin surface for layout
editing: git is the write path, the BFF stays read-only over generated artifacts.

## 6. Versioning / rollback

Git history on one file per target. Rollback = revert commit (Phase A: one spec-only deploy;
Phase B: live on merge). Retention = full history, free. "What was live on date X" =
`git log`/`git show` — plus, in Phase B, the pinned ref the BFF was serving, which the observability
contract records on every hot-swap.

## 7. What this proposal deliberately leaves open

- May a runtime file introduce **new** screen ids (seasonal pages), or only override existing ones?
- Phase B's fetch source and cadence (raw `main` vs a release ref; TTL; webhook-triggered refresh).
- Slug existence: the repo cannot know the live restaurant table, so an override for an unknown slug
  is validator-warnable at best (dead file) — decide whether to warn, ignore, or reconcile via a
  committed slug seed list.
- RESTAURANT self-service phasing and the exact service identity/CODEOWNERS setup (§5).
- Whether Phase A alone is enough for Tours PMF (merge + spec-only deploy may already be "content
  speed") — Phase B can wait for evidence.

## 8. Considered alternative — the database (revision 1)

Revision 1 of this proposal stored overrides in a Postgres table (`screen_overrides`: jsonb tree,
append-only versions, write-time + read-time validation, DB survives deploys). It remains the right
shape **if** publish latency must ever drop to seconds (live A/B at request time, per-session
targeting) — git's merge latency is the price of build-time safety. Its costs were real: a second
validation implementation to keep honest, a schema-version/migration story for stored trees, a
tenant column and scope enforcement in application code, prod-DB (or copies) needed just to test a
layout, and an override history invisible to the repo. Revision 2 trades seconds-latency publishing
for compile-time certainty and zero-infrastructure branch testing; the DB path can still be layered
on later behind the same wire format if the product ever needs it.
