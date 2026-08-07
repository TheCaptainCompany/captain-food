# ADR-20260807-183024 — One decomposition axis: spec folders, per-scope crates/images/graphs, core/views storage, per-scope projectors

- **Status**: Accepted (product owner, 2026-08-07: *"Approved as recommended"* — D1–D8 of
  PROP-20260807-174246, with D2 and D8 in their revised, product-owner-sharpened forms; the
  `critical-path-growth` concern explicitly accepted with the approval)
- **Proposal**: [PROP-20260807-174246](../proposals/PROP-20260807-174246-one-decomposition-axis-specs-schemas-projectors.md)
  (Approved) — option tables, the scope list, and the revision history
- **Tracking issue**: [#374](https://github.com/TheCaptainCompany/captain-food/issues/374)
- **Extends**: ADR-20260807-002705 (MKS + CNPG + GitOps) and PROP-20260806-223656's D5 addendum —
  this ADR defines what the emitters emit and what the cluster runs.

## Decision

One axis of decomposition runs the whole stack, every layer generated from the one above:

```
specs/{scope}/  →  domain-{scope} crate  →  per-scope bins/images  →  core/views storage  →  projector-{scope}
```

1. **D1 — Spec folders per scope + `specs/common/`** (screaming architecture): each scope holds its
   own `events/commands/entities/actors/errors/tests/rules/api/configuration.yaml`. Validator rules:
   placement (an item lives in its owning scope's folder), cross-scope `$ref` DAG (no cycles),
   kernel purity (`common/` references no scope). Scope list (8, from PM coupling): **ordering ·
   catalog · network · customer · delivery · payments · comms · common**.
2. **D2 — Storage splits by RESPONSIBILITY** (revised on the product owner's
   integration-database-antipattern argument): **`captain-core`** (event log + mailbox only; ALL
   backup/PITR budget, rehearsed drills) and **`captain-views`** (per-scope projection schemas +
   admin + bam; **excluded from backups — restore is replay**), both in the one CNPG cluster. No
   SQL ever crosses; per-scope lifts later are connection-string changes.
3. **D3 — The event log stays single** in `core`: global ordering, PM causality, one PITR timeline,
   the GDPR erasure path.
4. **D4 — Projectors per scope** over the single log, independent checkpoints; admin/BAM are
   consumer schemas fed by their own projectors — scope views never join across schemas.
5. **D5 — Configuration splits per scope + common**; each bin's generated `Config` reads only its
   own keys, and the drift test catches a pod reading another scope's key.
6. **D6 — Cross-scope data access is projections + GraphQL composition**; `admin_ro`/`claude_ro`
   cross-schema SQL is incident tooling, never an application path.
7. **D7 — Everything lands PRE-cutover**: start-clean makes the storage split free (schemas
   created, nothing migrated) — the window that does not recur.
8. **D8 — GraphQL per domain + a boring stitched gateway** (revised on the same responsibility
   axis): **`graphql-{scope}` services** generated from the per-scope `api.yaml` fragments — one
   domain, one graph, one GRANT — and a **thin generated gateway per role path** (no DB access, no
   logic, no state) routing top-level fields from a codegen-emitted composition table. Cheap
   because CQRS denormalization puts composition in the projector; a validator rule keeps nested
   types intra-scope so that stays a gate. Surface bins hold no broad views access.

## Consequences

- The pre-cutover program (PROP-20260806-223656 D5 addendum + this ADR), in order: **(1)** spec
  reorg — folders, api/config fragments, the new validator rules, and the `c4-l2.yaml` container
  split; **(2)** [#373](https://github.com/TheCaptainCompany/captain-food/issues/373) per-scope
  crates + kernel; **(3)** the bin crates — `fo-*`/`bo-*` surfaces, `graphql-{scope}`, per-role
  gateways, `actor-{type}`, `pm-{name}`, `projector-{scope}`, `bam`; **(4)**
  [#349](https://github.com/TheCaptainCompany/captain-food/issues/349) emitter — manifests,
  Dockerfile targets, `{digest, source_hash}` pins; **(5)**
  [#363](https://github.com/TheCaptainCompany/captain-food/issues/363) build matrix + determinator
  gate; **(6)** core/views databases + per-scope roles in the CNPG manifests
  ([#360](https://github.com/TheCaptainCompany/captain-food/issues/360)); **(7)**
  [#358](https://github.com/TheCaptainCompany/captain-food/issues/358) +
  [#361](https://github.com/TheCaptainCompany/captain-food/issues/361) — cluster + NS, product
  owner live.
- The `critical-path-growth` concern is accepted and CLOSED by this approval: production stays down
  for the duration of (1)–(7), chosen knowingly.
- The three standing reviewer agents (`architect`, `dba`, `graphql-architect`) review their layers
  of every realization PR.
- Unresolved questions (proposal §7) are copied to [#374](https://github.com/TheCaptainCompany/captain-food/issues/374)'s
  checklist: SIRENE/files/tips scope membership; per-schema migration tooling; BAM single vs
  per-scope; the per-bin generated `Config` reader shape.
