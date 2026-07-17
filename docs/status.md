# Session status — infra, DNS, routing & API auth

_Working log for resume continuity. Not generated. Newest section first. All work pushed to `main`._

Last updated: 2026-07-17.

## Current focus — API authentication (ADR-0047) ✅ first cut shipped
Verify **Supabase JWTs via JWKS** at the `/{role}/graphql` boundary; the signing secret never touches the
server (public keys from `SUPABASE_JWKS_URL`).

- **Done:** `crates/server/src/auth.rs` — `AuthContext` (JWKS fetch + cache + rotation), JWT verify
  (asymmetric-only, `alg`-confusion-safe), `app_metadata.captain_role` → role, strict path/role gate,
  `Principal` injected into the GraphQL context. Wired in `lib.rs` (Extension) + `graphql/routes.rs`
  (gate). `/public` open; other paths need a matching token (401/403); fail-closed if JWKS down. 10
  server unit tests green. ADR-0047 written + indexed.
- **Open / next:**
  - **Per-field `@auth` guard** over the injected `Principal` (completes ADR-0006 — currently the *path*
    is gated, not each field).
  - **EXTERNAL service tokens** (Stripe/HubRise/Avelo37 callers) — user JWTs don't fit; needs a story.
  - **Login flows** (ADR-0015): wrap Supabase passwordless (phone OTP / email magic link / passkey)
    behind our GraphQL auth mutations; build the client login screens. Not started.
  - Admin bootstrap: set `app_metadata.captain_role` via SQL (`update auth.users set raw_app_meta_data …`).

## Done earlier this session
- **Licensing** — Captain.Food **Coopyleft** (`LICENSE.md` + `LICENSES/AGPL-3.0.txt`), ADR-0044.
- **Hosting / deploy (ADR-0042)** — service driven by `render.yaml` via a linked **Blueprint** (IaC source
  of truth): **Docker runtime** (cargo-chef cached build + slim image), **`autoDeployTrigger: checksPass`**
  (deploy only after `codegen-consistency` passes). First Docker deploy verified live; `/health` green.
- **Build tuning** — `[profile.release]` (thin LTO, `codegen-units=1`, strip). Open: scope the Dockerfile
  `cargo chef cook` to `-p server` (cooks the whole workspace today).
- **DNS & custom domains (Dynadot → Render)** — apex + `www` **301 → `join.captain.food`**; `join` →
  GitHub Pages (marketing, off-Render); **`*.captain.food` CNAME → Render**, **wildcard TLS issued**
  (`_acme-challenge`); onrender.com subdomain disabled (serve only via custom domains). `join`/`www`
  explicit records override the wildcard. _Not yet recorded in ADRs — TODO: ADR-0036 amendment + ADR-0042
  DNS note._
- **Host routing (ADR-0036)** — `crates/server/src/hosts.rs`: one server answers every `*.captain.food`
  host, dispatches by `Host` to per-audience placeholders (`live`/`restos`/`riders`/`system`) and
  `{slug}` tenants; `api` → GraphQL; reserved/off-Render/malformed handled. Router **fallback** so
  `/health`/`/ping`/`/{role}/graphql` keep precedence. Placeholders only — real apps later.

## Blocked / needs attention
- **SIRENE sync** — the interim direct-write `sirene_sync` GitHub Action **fails**: the workflow received
  an **empty `INSEE_API_TOKEN`** (secret not reaching CI → 401 from INSEE). Verify it's a **repository
  Actions secret** named exactly `INSEE_API_TOKEN` with the real INSEE key. The redesign (staging table +
  worker, **ADR-0045**) is decided but **not implemented**.

## Deferred decisions recorded (not yet implemented)
- **ADR-0045** — SIRENE staging-table + on-app worker (kills CI↔prod version skew); deletion via existing
  `MarkRestaurantClosed`/`RestaurantMarkedClosed`.
- **ADR-0047** — per-field `@auth` guard; EXTERNAL service tokens.
- **Observability** — external uptime monitor now; Grafana Cloud (OTLP) when the server is instrumented
  (promotes P-01). Not started.
