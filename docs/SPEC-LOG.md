# Spec log — what the DSL now says differently

**One row per landed `specs/**` change, newest first.** This page exists to satisfy one standing
product-owner obligation, verbatim: *"Just keep me informed"*
([ADR-20260810-221840](adr/ADR-20260810-221840-specs-are-the-teams-work-the-freeze-is-lifted.md)).

It is a **pull surface, not a report**. There is no cadence, no weekly digest and nothing to send.
Read it when you want to know what the product now promises that it did not before — the same way you
read [`docs/proposals/DECISIONS.md`](proposals/DECISIONS.md). It is kept current by a gate, not by a
habit.

> **Not a changelog of files.** `git log -- specs/` already says which files changed and that is
> exactly the thing nobody can read. The **"What the product now promises differently"** column is
> the whole point of this page: one sentence, in product language, that someone who has never opened
> the repo can evaluate. A row that says *"added field `x` to `Y`"* has failed and should be rewritten.

---

## How to write a row

Write it **in the same commit as the spec change**. Cost: one sentence. If the sentence is hard to
write, that is signal — it usually means the change is a Tier 2 that has not noticed yet.

### The tier is the boundary, and writing it is where the boundary is enforced

| Tier | Test | What it means |
|---|---|---|
| **0 — free** | Nothing emitted, nothing stored, no client shipped, no recorded decision touched | Land it. One row here, and that is the whole obligation. |
| **1 — migration** | The shape already exists in `domain_events`, in a shipped client, in an alert route, in a partner contract (HubRise/Stripe/Uber), or in a legal artifact (allergens, VAT, receipts, retention) | **Not an edit — a migration.** The versioning story is recorded *before* it lands. Stored events are immutable contracts: upcasting, never mutation (Greg Young, *Versioning in an Event Sourced System*). The GDPR tombstone path ([ADR-20260731-160000](adr/ADR-20260731-160000-order-erasure-tombstone-then-stream-deletion.md)) is the one recorded exception and is not a precedent. |
| **2 — decision** | It contradicts, or creates, a recorded decision in [`DECISIONS.md`](proposals/DECISIONS.md) or `docs/adr/` | **Stop.** This is a decision reversal wearing a spec edit's clothes, and it is not the team's to make regardless of how small the diff is. File a register row instead. **A Tier 2 row should never appear on this page** — if you are writing one, the change should not be landing. |

Structure is not a tier. A structural change with a real option space is routed by CLAUDE.md's
**proportionality** rule to a proposal + tracking issue + a register row — which *is* the discussion
the product owner offered. A structural change with no option space (a file move that rewrites no
`$ref`s, because refs are kind-logical) is a Tier 0 like any other.

> **Known blind spot: the tier test is applied to the `specs/**` diff, so a stored-shape change
> arriving through CODE is invisible to it** (found in review of
> [#469](https://github.com/TheCaptainCompany/captain-food/issues/469), 2026-08-11). That change
> widened what is written to `domain_events` envelopes — every open-path command now stamps a
> `user_id`/`user_type` it did not carry before — while its `specs/**` diff is two descriptions and
> a metric, i.e. a correct **Tier 0** on both its rows. Tier 1 asks "is the shape already emitted or
> stored?", but it only ever gets asked of the YAML. Until a gate closes this, ask the tier question
> of the **change**, not of the diff: if the code half alters what lands in `domain_events`, in a
> shipped client, in an alert route or in a legal artifact, it is a Tier 1 conversation whatever the
> YAML says.

---

## Rows

| Date | What the product now promises differently | Tier | Change | `make validate` |
|---|---|---|---|---|
| 2026-08-11 | **Nothing new — the spec promises one thing LESS, and that thing was never true.** `c4-l2` described the actor pods as scope-isolated: *"drains ONLY its own mailbox lanes — the scoping is the linker"*. Read literally that says an `actor-cart` pod could not reach another aggregate's code even if something told it to. It can: every actor image links every domain crate behind `bin_runtime`, so a lane list ROUTES work, it does not restrict reach. The comment now says which of the two it is, so nobody builds a blast-radius or least-privilege argument on an isolation the build never provided. Nothing emitted changes — it is a YAML comment, and `make generate` produces a byte-identical tree. | 0 | [#475 "Per-bin scope isolation is nominal: every actor/pm/projector bin transitively links all 8 domain scopes…"](https://github.com/TheCaptainCompany/captain-food/issues/475) · [PR #489 "fix(475): the bin manifest scope header states what the build enforces, not what it wishes"](https://github.com/TheCaptainCompany/captain-food/pull/489) | 0 errors, 37 warnings (unchanged) |
| 2026-08-11 | A customer signed in **before** we started stamping their domain id on their token is served the anonymous view on a storefront rather than a half-identity, and that window is now counted as its own thing. It is expected, it lasts one token lifetime after a release, and it must not be read as customers being denied anything — so it no longer bumps the counter that means "someone's account is not properly provisioned". | 0 | [#469](https://github.com/TheCaptainCompany/captain-food/issues/469) — `specs/observability.yaml` read-authorization: `public_credential_degraded_total` gains `reason=claim_absent`, with the contract text saying why it is not `bridge_unresolved` | 0 errors |
| 2026-08-11 | **`current` is now the cart AT THIS STOREFRONT.** A customer with open carts at two restaurants is served — and priced for — the one belonging to the restaurant whose address they are on; the tenant comes from the web address, never from anything the client can assert, and a page that names no restaurant (the marketplace, an unknown address) shows no cart rather than the most recent one from somewhere else. Carts at other restaurants remain readable through `carts`, which is what that query is for. | 0 | [#469](https://github.com/TheCaptainCompany/captain-food/issues/469) — `specs/ordering/api.yaml` `current`: description now states the Host-derived tenant bound on both legs | 0 errors |
| 2026-08-11 | A customer who is signed in on a restaurant's storefront is now recognised there — the anonymous web path READS the credential the browser already sends, so their cart is theirs rather than a stranger's empty one; when that credential cannot be honoured (expired cookie, identity provider down, or a staff token, which stays anonymous on purpose) they are served the anonymous view rather than an error, and **we now count every one of those silent degrades** — so "identified customers are being served anonymous" can no longer look like "customers stopped having carts". | 0 | [#469](https://github.com/TheCaptainCompany/captain-food/issues/469) — `specs/observability.yaml` read-authorization: `public_credential_degraded_total{reason}` | 0 errors |

---

## Status of the gate

The gate that keeps this page current — *if a commit range touches `specs/**` and this file is
unchanged, fail* — is **not yet built**. Its shape is one open decision with four options and a
recommendation, filed as [`DECISIONS.md`](proposals/DECISIONS.md) §26 row **SPEC-1**.

Until it lands, this page is a prose obligation and is therefore exactly as reliable as CLAUDE.md
says prose obligations are. That is a knowingly accepted, short-lived gap, not an oversight: the page
had to exist the day the freeze lifted, and it must not wait on the gate that protects it.
