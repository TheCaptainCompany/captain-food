# ADR-20260808-060309 — The marketplace owns the bare apex; adapters own the hooks host

- **Status**: Accepted (settled in [#385 "Bin runtime wiring"](https://github.com/TheCaptainCompany/captain-food/issues/385), realized by [PR #389](https://github.com/TheCaptainCompany/captain-food/pull/389))
- **Context**: Until #389 the bare `captain.food` apex was deliberately unrouted — the screens
  specs disagreed about which surface serves it — and the partner webhook endpoints rode the
  marketplace host. The ingress emitter needed both settled to derive complete host rules.

## Decision

- **`captain.food` (bare apex) → `fo-marketplace`.** Spec home: `additional_hosts:
  [captain.food]` in `specs/screens/captain_frontoffice.yaml` — the host list is spec data the
  emitter derives from, not an emitter-side hand list. This matches `web::router`'s existing
  host → surface classification (an apex visitor sees the marketplace, tenant storefronts stay
  on `{slug}.captain.food`).
- **`hooks.captain.food` → `adapters`.** Spec home: `adapters.ingress_host` in
  `specs/architecture/c4-l2.yaml`. Marketplace-host webhook paths remain as a transition alias
  until partner dashboards are re-registered (recorded on #385).

## Consequences

- The wildcard `*.captain.food` certificate does NOT cover the apex — the #358 bootstrap
  Certificate must carry the apex SAN (recorded cutover precondition on #385).
- `integration_scopes` scope-name validation is not yet a validator rule (comment corrected in
  the same change; tracked on #385) — a typo'd scope would silently drop the adapters pod's
  webhook secrets today.
