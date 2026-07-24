# PROP-20260724-133700 — Live spec editing + per-tenant customizations (ADR-0033 deferred contract)
- **Status**: Proposed
- **Date**: 2026-07-24
- **Tracking issue**: [#96 "Live spec editing + per-tenant customizations (specs/customizations/) with fail-closed branch publishing"](https://github.com/TheCaptainCompany/captain-food/issues/96) (retitled to this proposal's scope on 2026-07-24 — every proposal MUST have one, ADR-20260724-143000; always the full clickable link, never a bare number)
- **Realized by**: (pending)

> **Status:** Proposed — plan-mode proposal. **No `specs/**` or code changed yet.** On approval it
> becomes an ADR that lands with the implementation.
>
> **Revision 5 (2026-07-24, product-owner direction):** live translation editing explicitly covers
> the **defaults**, not only tenant customizations — and the catalog is restructured **one file per
> language** so concurrent live edits in different languages can never merge-conflict (§1b).
>
> **Revision 4 (2026-07-24, product-owner direction):** two corrections to revision 3. (a) There is
> **no runtime override layer for the designed spec** — no `runtime_screens/` platform files and no
> runtime translations family: the designed spec (`specs/screens/*.yaml`,
> `specs/translations.yaml` + sidecars) IS the live-editable artifact, changed through the
> publishing flow of §3. The only new storage is for what is NOT designed in the spec: **per-tenant
> customizations**, in **`specs/customizations/`** (`restaurant_frontoffice_<slug>.yaml`).
> (b) The publishing flow is made explicit and FAIL-CLOSED: a live edit in production **creates a
> branch → preview → merge to `main` gated by the checks — green applies it, red changes nothing**.
>
> **Revision 3 (2026-07-24):** translations were given their own runtime override family — RETIRED
> by revision 4 (kept in §8 as a considered alternative).
>
> **Revision 2 (2026-07-24, product-owner direction):** storage moved from a Postgres table to the
> repo itself, gated by the same `make validate`. The database variant stays in §8. (Authored by the
> @claude GitHub app on branch `claude/issue-96-20260724-1337`, landed here under the PROP
> convention of ADR-20260724-135945.)
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

One model for screens AND translations alike: **the designed spec is the live artifact.** Editing
the platform experience in production = editing `specs/screens/*.yaml` /
`specs/translations.yaml` (+ sidecars) through the publishing flow below — no shadow layer, no
override files, no second mechanism. Build-time codegen (ADR-20260723-172013) doesn't go away; it
is what a merge applies.

What IS new is storage for the one thing the designed spec cannot contain: **per-tenant
customizations** — a restaurant's own storefront variant — in **`specs/customizations/`**.

Publishing (both cases) is **git, fail-closed**: a live edit made on production creates a **branch**
→ a **preview** serves that branch → **merge to `main`**; the required checks (`make validate` /
the `codegen` gate) decide — **green ⇒ applied, red ⇒ nothing changes in production**. A broken
edit cannot ship by construction.

Fail-safe stays: a tenant customization that is missing (or predates a breaking spec change caught
by CI) falls back to the designed storefront — never a blank screen.

## 1. Storage — the designed spec, plus `specs/customizations/` for tenant deltas

**Platform scope (marketplace, storefront default, back office, rider, all copy): the designed spec
files themselves.** They are already in the repo, already validated, already compiled — "runtime
editable" means the publishing flow of §3 can change them from production, not that they get a
parallel override family.

**Tenant scope: `specs/customizations/`** — one file per customized restaurant storefront:

| file | customizes | notes |
|---|---|---|
| `specs/customizations/restaurant_frontoffice_<slug>.yaml` | the storefront served at `{slug}.captain.food` | overrides whole screens of the base surface by screen `id`; allowlists (registry, resolvers, actions) are INHERITED from the base surface and cannot be extended here |
| `specs/customizations/restaurant_frontoffice_<slug>.translations.yaml` | that tenant's strings | the EXISTING sidecar convention (ADR-20260722-101500) co-located with its customization — not a new translations mechanism; keys are tenant-namespaced and referenced by the customization file |

Resolution for a storefront request: `customizations/restaurant_frontoffice_<slug>.yaml` → the
designed `specs/screens/restaurant_frontoffice.yaml`. Two levels, no platform override layer in
between.

## 1b. Translations — the defaults are live-editable, one file per language (revision 5)

**No separate runtime mechanism** (revision 4 holds): platform copy is edited by editing the
CATALOG SOURCE through §3's publishing flow — that explicitly includes the **defaults** (every
`common.*` string, every surface's screen copy), not just tenant sidecars. Changing the checkout
button label in production = a branch editing the catalog → preview → gated merge.

**Restructuring for conflict-free live edits: one file per language.** Today one file holds every
locale of a key (`messages: { en, fr }`), so two concurrent live edits — a French copy tweak and an
English one — collide in the same file/lines and can merge-conflict. Proposed layout:

| today | proposed |
|---|---|
| `specs/translations.yaml` (`key: { messages: { en, fr } }`) | `specs/translations/en.yaml` + `specs/translations/fr.yaml` (`key: <message>`; shared `params`/descriptions in `specs/translations/keys.yaml`) |
| `specs/screens/<surface>.translations.yaml` | `specs/screens/<surface>.translations.en.yaml` + `.fr.yaml` (same split) |
| `specs/customizations/…_<slug>.translations.yaml` | per-language likewise |

- The codegen merge is unchanged in OUTPUT: the same `translations.generated.json`
  (`key → {en, fr}`) — only the SOURCE layout changes, so `crates/web` and the embedded catalog are
  untouched.
- The validator's completeness check becomes CROSS-FILE: every key present in one language file must
  exist in all supported-locale files (a missing `fr` for a new `en` key is a hard error), and the
  `params` contract lives once in the shared key manifest — both languages checked against it.
- Adding a locale later = adding one file per catalog, not touching every key in every file.
- Open point for the landing ADR: whether `keys.yaml` (params + description manifest) is worth the
  third file per catalog, or params stay declared in the DEFAULT locale's file with others checked
  against it.

Tenant copy rides the co-located customization sidecars of §1 (per-language, same split).
Revision 3's runtime-overlay family stays retired (§8).

## 2. Wire format

The generated `Screen` / `Node` / `PropValue` Rust shapes (`crates/web/src/generated/screens.rs`,
ADR-20260723-172013) are the **versioned contract**:

- `emit_web_screens` extends over `specs/customizations/*.yaml` exactly as it does over surfaces —
  customization trees become generated tables/JSON in the SAME emitter pass as the designed spec.
  One source, one pass — a customization and the surface it customizes cannot drift through CI.
- For Phase B (§3) the emitter serializes the same trees as JSON artifacts
  (`specs/generated/screens/<surface>[_<slug>].json`) plus the existing
  `translations.generated.json` (unchanged in shape — §1b only restructures its SOURCE files). The BFF only ever consumes GENERATED artifacts — the codegen stays
  the only YAML reader.

## 3. Delivery & the publishing flow (revision 4 — fail-closed by construction)

**The write path from production** (Studio or any authoring surface):

1. A live edit **creates a branch** in this repo (service identity; branch naming
   `customize/<slug>/<change>` for tenant scope, `content/<change>` for platform scope).
2. A **preview** serves that branch — the full real pipeline against the branch's generated
   artifacts (a preview deployment or Studio's mounted checkout); the editor sees exactly what
   would ship.
3. **Merge to `main`** is the publish action, gated by the required checks (the `codegen` gate =
   `make validate` + drift): **green ⇒ the change is applied; red ⇒ the merge is blocked and
   production is untouched.** There is no path by which an invalid edit reaches a customer.

**How a merge becomes live**, phased as before:

- **Phase A — baked (ships first, nearly free):** the merge rides the existing spec-only
  CI → image → deploy path; publish latency = one CI cycle. For platform copy/layout this is
  already true today — Phase A's new work is only the `customizations/` compilation + serving.
- **Phase B — live fetch (closes the no-redeploy promise):** the BFF fetches the generated JSON
  artifacts from `main` (raw content, ETag-cached, short TTL) and hot-swaps on change — publish
  latency becomes "merge", with no deploy. The baked artifacts remain the permanent fallback.

Either way the renderer's input type doesn't change — still the "loader, not a rewrite" promised by
ADR-20260723-172013.

## 4. Safety

Three checkpoints now, and the important one moved left:

- **Build time (the primary gate):** `make validate` extends to `customizations/*.yaml` — screen
  ids resolve against the base surface, component types against the registry, data needs against
  the surface's `resolvers`, action types against its `actions`, strings against the merged
  catalog (incl. the co-located tenant sidecar), filename prefix against an existing surface. This
  is the
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
  operator signal ("customization for `<slug>` is stale/invalid — fix the file"), never a silent
  degradation.

Because base specs and runtime overrides live in one repo and one gate, the "same validation, twice,
without drift" problem of revision 1 (write-time vs read-time vs codegen re-implementations)
collapses: there is **one validator** (the codegen), and serve-time re-checks reuse the same
generated allowlist tables (`ResolverKey` / `ActionKey` / registry) the renderer already carries.

## 5. Who may edit — authoring is Captain Studio, authorization is git

Editing a runtime screen is **editing a spec file**, so the authoring surface already exists:
**Captain Studio** (spec workbench) — typed component-tree forms, translation-resolved preview,
registry/allowlist checks, diff/revert, and the product gate — pointed at a branch of this repo.
The flow: Studio edits the designed spec (platform scope) or `customizations/restaurant_frontoffice_<slug>.yaml` (tenant scope) on a branch → preview → PR → merge
publishes. No hand-written `Node` JSON anywhere; humans never touch the wire format.

Authorization maps to git, not to a new mutation set:

- **ADMIN** (the designed spec): repo write access + PR review; optionally CODEOWNERS on
  `specs/**`.
- **RESTAURANT self-service** (their customization): Studio, authenticated as the tenant, commits
  through a service identity that is **structurally scoped** — it may only create/modify
  `specs/customizations/restaurant_frontoffice_<their-slug>[.translations].yaml`. The filename IS
  the scope boundary, checked by the committing service and re-checked by the validator. Whether self-service ships with ADMIN editing
  or is phased in later stays a product decision (§7).

This refines ADR-0037's impersonation-only stance without inventing a new admin surface for layout
editing: git is the write path, the BFF stays read-only over generated artifacts.

## 6. Versioning / rollback

Git history on one file per target. Rollback = revert commit (Phase A: one spec-only deploy;
Phase B: live on merge). Retention = full history, free. "What was live on date X" =
`git log`/`git show` — plus, in Phase B, the pinned ref the BFF was serving, which the observability
contract records on every hot-swap.

## 7. What this proposal deliberately leaves open

- May a customization introduce **new** screen ids (seasonal pages), or only override existing ones?
- Phase B's fetch source and cadence (raw `main` vs a release ref; TTL; webhook-triggered refresh).
- Slug existence: the repo cannot know the live restaurant table, so a customization for an unknown
  slug is validator-warnable at best (dead file) — decide whether to warn, ignore, or reconcile via a
  committed slug seed list.
- RESTAURANT self-service phasing and the exact service identity/CODEOWNERS setup (§5).
- Whether Phase A alone is enough for Tours PMF (merge + spec-only deploy may already be "content
  speed") — Phase B can wait for evidence.

## 8. Considered alternatives

### A runtime override/overlay family for the designed spec (revisions 2–3, retired)

Revisions 2–3 gave the PLATFORM scope its own override files (`runtime_screens/<surface>.yaml`) and
translations an overlay family (`runtime_screens/translations[_<slug>].yaml`). Retired by revision 4
(product-owner direction): the designed spec is itself a repo file behind the same gate — a platform
"override" of it is just an EDIT of it, and a parallel layer would mean two places answering "what
does the marketplace look like", diffable drift between them, and a second translations mechanism
beside the catalog+sidecar rules. The only content that cannot live in the designed spec is a
tenant's variant — hence `specs/customizations/` holding exactly that, nothing else.

### The database (revision 1)

Revision 1 of this proposal stored overrides in a Postgres table (`screen_overrides`: jsonb tree,
append-only versions, write-time + read-time validation, DB survives deploys). It remains the right
shape **if** publish latency must ever drop to seconds (live A/B at request time, per-session
targeting) — git's merge latency is the price of build-time safety. Its costs were real: a second
validation implementation to keep honest, a schema-version/migration story for stored trees, a
tenant column and scope enforcement in application code, prod-DB (or copies) needed just to test a
layout, and an override history invisible to the repo. Revision 2 trades seconds-latency publishing
for compile-time certainty and zero-infrastructure branch testing; the DB path can still be layered
on later behind the same wire format if the product ever needs it.
