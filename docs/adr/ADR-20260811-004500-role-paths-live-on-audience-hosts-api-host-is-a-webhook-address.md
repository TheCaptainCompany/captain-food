# ADR-20260811-004500 — Role paths live on audience hosts; `api.captain.food` is a webhook address, not an API address

- **Status**: Accepted
- **Date**: 2026-08-11
- **Context**: [#358 "MKS bootstrap: OVH auth, cluster + vRack, ≥3-node pool, kubeconfig into CI"](https://github.com/TheCaptainCompany/captain-food/issues/358)
- **Supersedes in part**: [ADR-0036](0036-domain-topology-single-origin-identity.md)'s row naming `api.captain.food` as *the* GraphQL API host
- **Related**: [ADR-0006](0006-graphql-role-as-path-acl.md) (role = path) · [ADR-20260807-183024](ADR-20260807-183024-one-decomposition-axis.md) (the bin topology) · [ADR-20260808-062432](ADR-20260808-062432-one-bin-per-adapter.md) (`hooks.captain.food`)

## Context

Two descriptions of where the GraphQL role paths live had drifted apart, and nothing compared them:

- **The generated Ingress** (`deploy/generated/manifests/ingress.yaml`, derived from the screens
  specs) routes **no `api.` host at all**. Role = path *per audience host*: `/admin/graphql` on
  `system.captain.food`, `/public/graphql` and `/customer/graphql` on the marketplace host,
  `/restaurant/graphql` on `restos.`, and so on.
- **`tools/smoke/prod-smoke.sh` and `db-migrate.yml`** hard-wired `https://api.captain.food` for
  every role path and for the deploy health gate.

Both are correct about the monolith, which serves every role path on every host — its routes are
explicit and host-independent, and `crates/server/src/hosts.rs` does host routing only in the router
*fallback*. Neither noticed that only one of them survives the cutover. The first request the smoke
makes after the flip would 404, on the one script that tells us whether money still moves, at the
worst possible moment to be debugging host routing.

## Decision

1. **Role paths are addressed on their audience host.** `api.` is not the API address, and no new
   consumer may be pointed at it. The smoke and the deploy health gate now target `live.` (public,
   customer, edge) and `system.` (admin) — hosts served identically by the monolith today and by the
   surface/gateway bins after the cutover, so they are correct across the whole transition rather
   than on one side of it.

2. **`api.captain.food` is retained as a webhook address with a defined lifetime.** The registered
   Stripe endpoint is `https://api.captain.food/adapters/stripe/webhooks`. Retiring the host before
   every partner dashboard is re-registered on `hooks.captain.food` silently drops payment-capture
   callbacks — the customer is charged and the restaurant is never told, which is the worst failure
   mode this system has. It is therefore declared as the monolith's `ingress_host` in c4-l2 and
   served by the monolith overlay only. **Deleting it is a separate, recorded step, after
   re-registration is confirmed** — never a side effect of the cutover.

3. **The bins Ingress must not carry it.** That tree has no `server` Service to route it to, so the
   rule would dangle the moment it is applied. A codegen test asserts the host appears in the
   monolith overlay and is absent from the bins tree.

## Consequences

- The smoke gains `SMOKE_SCHEME` and per-audience base overrides, which is also what makes a local
  rehearsal possible at all (`docs/runbooks/cutover-local-rehearsal.md`).
- A codegen test pins the smoke's hosts against the generated Ingress, so this pair cannot drift
  apart silently again — the failure mode was invisible precisely because nothing compared them.
- ADR-0036's host table is now historical on the `api.` row; the live description is the generated
  Ingress plus this ADR.
- **Open, for the cutover session**: re-register the Stripe webhook endpoint (and any HubRise /
  Uber Direct callback) on `hooks.captain.food`, then retire `api.` in its own change.

## Options considered

- **Add `api.` to the bins Ingress and keep the smoke as it was.** Rejected: it re-introduces a
  role-multiplexed host into a topology whose entire point is one surface per audience, and it needs
  a "route everything to every gateway" rule that contradicts ADR-0006's derivation.
- **Retire `api.` now and move the smoke to audience hosts.** Rejected as one step: correct
  destination, but it drops registered partner webhooks in the same change that moves them.
  Sequencing is the whole content of the decision.
