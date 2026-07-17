# ADR-0047 — API authentication & role authorization (verify Supabase JWT via JWKS at the role-path boundary)

## Status
Accepted — realizes the **deferred runtime guard** of ADR-0006 and extends ADR-0015. First-cut middleware
ships with this ADR (gates the *path*; per-field `@auth` is the next step).

## Context
ADR-0006 serves one master GraphQL schema per role under `/{role}/graphql`, filtered by `@auth`/`@public`.
The runtime guard was **deferred**: today the role is parsed from the path and **trusted** — no token, no
verification (`acl.rs`: "every role path serves the same schema"). So `/admin`, `/customer`, … are
effectively **open**, mitigated only by the resolvers being thin read-models. ADR-0015 wraps Supabase Auth
behind our GraphQL; Supabase issues **JWT** access tokens and the identity bridges to `Customer` via
`View_Customer.auth_ref`.

We need to actually authenticate callers and enforce that a caller may use a given role path — **without
spreading a signing secret** across services.

## Decision
Authenticate at the `/{role}/graphql` boundary by verifying a Supabase-issued **JWT** presented as
`Authorization: Bearer <token>`:

- **Asymmetric verification via JWKS.** Fetch the project's **public** keys from `SUPABASE_JWKS_URL`, cache
  them with periodic refresh + refetch-on-unknown-`kid` (key rotation). The **signing secret never resides
  on our server** — this is the "don't share the secret everywhere" property. We do **NOT** enable Supabase's
  *OAuth Server* (that makes Supabase an IdP for third-party apps — wrong direction, unrelated).
- **Validate** signature (by `kid`/`alg` from the JWKS), `exp`, and `aud = "authenticated"` (optionally
  `iss` derived from `SUPABASE_URL`).
- **Business role** is carried in the token as **`app_metadata.captain_role`** — server-controlled, not
  user-editable, and included in the Supabase JWT by default. Absent claim ⇒ `CUSTOMER` (a plain
  authenticated user).
- **Authorization per path:**
  - `/public` → open (no token; `@public`).
  - `/customer` → any valid token (`captain_role` defaults to CUSTOMER).
  - `/admin` → `captain_role == ADMIN`.
  - `/restaurant`, `/restaurant-account`, `/rider` → matching `captain_role`.
  - `/external` → `captain_role == EXTERNAL` (machine/third-party tokens; may get a dedicated mechanism later).
  - Missing/invalid token on a non-public path → **401**; valid token, wrong role → **403**.
- The verified **`Principal { user_id (sub), role }`** is injected into the GraphQL context (replacing the
  self-asserted path role), so the deferred per-field `@auth` guard (ADR-0006) can build on a trustworthy
  identity.
- **Fail closed:** if JWKS is unavailable, non-public paths reject rather than allow.
- Admin/privileged users are provisioned in Supabase with `app_metadata.captain_role` (SQL or Admin API),
  consistent with ADR-0036 admin-via-impersonation.

## Alternatives considered
- **Legacy HS256 shared JWT secret** — rejected: spreads the secret to every verifier; asymmetric JWKS avoids it.
- **Supabase OAuth Server (IdP for third parties)** — rejected: wrong direction, extra surface, unrelated to
  protecting our own API.
- **Roles purely in our read model (no claim)** — deferred: fine for CUSTOMER (the `authRef` bridge), but
  privileged roles need an out-of-band grant; `app_metadata` is the simplest trustworthy carrier for V0. A
  later hardening can resolve roles fully server-side by `sub`.

## Consequences
### Positive
- Real authn/authz at the boundary; **no shared signing secret**; identity available to resolvers/`@auth`;
  admin bootstrap is a Supabase metadata edit.
### Negative / caveats
- JWKS fetch/caching is a runtime dependency (must handle refresh + rotation, and fail closed).
- This gates the **path**, not yet each **field** — the per-field `@auth` guard is still to come.
- **EXTERNAL/machine tokens** (Stripe/HubRise/Avelo37 callers) need a longer-term service-token story.
- `app_metadata.captain_role` is **coarse** (single role); multi-role / tenant-scoped grants are future work.

## Follow-up actions
- Implement the middleware in `crates/server` (`jsonwebtoken` + JWKS cache), gate `/{role}/graphql`, inject
  `Principal`. *(Ships with this ADR as a first cut.)*
- Wire the per-field `@auth` guard over the injected `Principal` (completes ADR-0006).
- Define EXTERNAL service-token handling.
- Consider a **Custom Access Token Hook** for a top-level `captain_role` claim (vs reading `app_metadata`).
- Document admin provisioning (`app_metadata.captain_role` via SQL / Admin API).

## References
ADR-0006 (role-as-path ACL), ADR-0015 (Supabase Auth wrapped behind GraphQL), ADR-0036 (single-origin
identity, admin impersonation), `specs/integrations/supabase.md`, `scalars.yaml#/UserType`.
