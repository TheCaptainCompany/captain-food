# ADR-20260829-082615 — The path-addressed storefront (`/r/{slug}`) is retired: the host is the ONE address

**Status**: Accepted · **Date**: 2026-08-29 ·
**Decider**: the **FOUNDER / Tech CEO**, verbatim below ·
**Prompted by**: [#749 "the storefront MENU has never rendered from a real paint"](https://github.com/TheCaptainCompany/captain-food/issues/749) ·
**Register row**: [docs/decisions/PATH-ADDRESSED-STOREFRONT.yaml](../decisions/PATH-ADDRESSED-STOREFRONT.yaml) ·
**Session**: https://claude.ai/code/session_01Dbhq2Y7U5NcnqhByscaB4v

## Status

Accepted — realized in the same change ([PR #752](https://github.com/TheCaptainCompany/captain-food/pull/752)).

## The directive, verbatim

> *"I don't want to have /r/&lt;slug&gt; possible we already have it in the &lt;slug&gt;.captain.food"*

## Context

The storefront had two addresses: the tenant host (`{slug}.captain.food`, ADR-0036 — the tenant
selector) and a path-addressed route (`/r/:slug` in
`specs/screens/restaurant_frontoffice.yaml`), whose own code comment recorded the prior stance:
*"the `/r/:slug` path route stays for path-addressed access"* (`crates/web/src/router.rs`), and
which `specs/PRODUCT_SPEC_WEB_CLIENT.md` promised as `https://captain.food/r/{slug}`. This ADR
**reverses that recorded stance and that promise** — hence the register row.

## The decision

1. **`route: "/r/:slug"` is removed** — the restaurant screen's declared route is the tenant root
   (`route: "/"`); the router's tenant-root special case dissolves into the ordinary table match
   plus the #745 host-slug injection. At the web-router layer `/r/anything` resolves **nothing**.
2. **Old links 301, never 404** (architect + ux): `/r/{slug}` was handed out (printed menus, QR
   codes, search results), so the server fallback (`hosts::path_addressed_redirect`) answers
   `301 → https://{slug}.captain.food/` on every host — the
   [ADR-20260728-011344](ADR-20260728-011344-two-worlds-one-storefront-slug-uniqueness-and-renames.md)
   precedent (a superseded storefront address redirects to the current one). Only a well-formed
   slug label (`[a-z0-9-]`, no leading/trailing `-`, no deeper path) redirects — the Location
   header is built from the path segment, so anything else 404s rather than being reflected.
3. **Prose follows**: `specs/network/api.yaml` (`restaurant` description), the
   `PRODUCT_SPEC_WEB_CLIENT.md` promise, and the `router.rs` "stays for path-addressed access"
   comment are rewritten in the same change.

## Alternatives considered

- **404 for `/r/{slug}`** — rejected: dead-ends every bookmarked/printed URL for zero gain.
- **Keep the route as an alias** — rejected by the directive itself: two addresses for one page
  splits analytics, caching and copied links, and the host form already exists.

## Consequences

Positive: one canonical storefront address; one fewer route to secure/instrument; the router loses
a special case. Negative: any external material that printed `captain.food/r/{slug}` (none known —
the apex never served it; it 301s to marketing) relies on the redirect. Follow-up: none — the
redirect is permanent, like the superseded-slug 301.

## Consulted

Full-roster briefing on the founder's message (2026-08-29, relayed with #749's dispatch):

- **architect** — retire the route, but 301 (not 404) any handed-out `/r/` link to the canonical
  host; sequence it as its own commit with its own record.
- **beck** — the "not possible" needs its own red: a negative seen failing while the route still
  served (done — hosts.rs 301 test red through `host_root`, router-level `is_none` red).
- **business** — one address is the right call for funnel attribution; no known printed material
  uses the path form.
- **dba** — nothing in my lens; no storage shape moves.
- **evans** — the host IS the tenant selector (ADR-0036's language); a second spelling of the same
  identity was an alias inviting drift — remove it and say so where the old promise lived.
- **farley** — the redirect is behaviour at the public interface; pin it in the deployable's own
  tests, and sweep the prose so no doc re-teaches the removed form.
- **graphql** — with `/r/:slug` gone, exactly two reads are host-derivable (`restaurant.bySlug`,
  `catalog`); the schema is untouched by the removal itself.
- **holub** — keep the removal a separate, revertable commit from the menu fix; the two decisions
  have different owners (defect fix vs founder directive).
- **legal** — nothing in my lens; per-host obligations ride #400 unchanged.
- **observability** — no contract moves; the 301 is visible in edge logs like the superseded-slug
  one.
- **ux** — bookmarked/printed URLs must keep working: 301 to the storefront root, never a 404.
- **vernon** — nothing in my lens; no aggregate boundary moves.
- **young** — the removal deletes a read path, not a write path; the host-injection precedence
  rule (host wins, disagreement rejects) is decided on the #749 fix side.
