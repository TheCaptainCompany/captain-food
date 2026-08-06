# ADR-20260806-151122 — The hosting destination is Clever Cloud, not OVH

- **Status**: Accepted (product owner, 2026-08-06, in-session: *"Instead of OVH"* — after Clever Cloud
  was surfaced and its trade-offs put on the table)
- **Tracking issue**: [#271 "Migrate hosting to OVH: app compute + PostgreSQL leave Render/Supabase; Supabase retained for identity only"](https://github.com/TheCaptainCompany/captain-food/issues/271) (title now stale — the migration it tracks is unchanged in purpose, changed in destination)
- **Supersedes, in part**: [ADR-20260731-061609](ADR-20260731-061609-hosting-migrates-to-ovh-supabase-identity-only.md)
  — **only its point 1** (the destination). Points 2, 3 and 4 survive verbatim; see Decision below.
- **Migration plan**: [PROP-20260731-061609](../proposals/PROP-20260731-061609-ovh-migration.md) (D1 carries
  the Clever Cloud row and its open question)

## Context

ADR-20260731-061609 chose OVH on 2026-07-31, correctly, for the reasons recorded there: Render and
Supabase ceilings were exhausted and the next tier up did not match the project. That reasoning is
unchanged and is **not** what this ADR revisits.

What changed is the shape of the destination, and it surfaced by working the consequences. Choosing
an OVH instance means owning a host OS for the first time, which generated a whole proposal
([PROP-20260805-181926](../proposals/PROP-20260805-181926-host-provisioning-and-configuration-ownership.md))
about who configures it — and, through a session that evaluated SaltStack, Ansible, NixOS, cloud-init
and OpenTofu, a growing tail of work that produces **no customer value**: a WireGuard overlay (because
OVH VPS cannot join a vRack), block volumes for a database disk, an upgrade path that is a one-way
ratchet, and WAL archiving to object storage — mandatory, because self-hosted PostgreSQL has no
backups unless we build them.

Clever Cloud is a French PaaS (Paris region) that removes that tail: managed PostgreSQL with daily
backups at 7-day retention on paid plans and PITR via pgBackRest on request, Docker-image deploys, and
no operating system of ours anywhere. The decisive argument is capacity, not technology: **a team of
one product owner plus agents should not be operating a PostgreSQL server.** Every hour spent on
tunnels and WAL shipping is an hour not spent on the ETA, the acceptance timeout, or allergen
declaration — and this project's own lens says the ETA is the product.

Sovereignty moves the right way too: data in France under European jurisdiction and explicitly outside
the Cloud Act, against a Supabase that is US-owned. Given CLAUDE.md treats French legal preconditions
as launch blockers rather than backlog items, that is a substantive gain, not a nicety.

## Decision

1. **App compute and the domain PostgreSQL go to Clever Cloud** (Paris region), not OVH. The database
   is a **paid** managed plan — the free `DEV` plan has carried no backups since 2025-10-01 and is a
   trap for the same reason the Supabase free tier is.
2. **ADR-20260731-061609 points 2–4 survive unchanged**: Supabase is retained for **identity only**
   (phone OTP, magic links, JWKS, opaque `auth_ref`); **the build side does not move** (GitHub Actions
   builds, GHCR hosts images, the isolated build → manual deploy → migrate pipeline keeps its shape,
   only the deploy target changes); and **the cutover still uses the existing outage**.
3. **OVH is not abandoned as a vendor** — the SMS hook (ADR-20260722-174500, phone OTP delivery)
   stays on OVHcloud. This ADR changes where the app and database run, nothing else.
4. **Spending is gated on one unanswered question** (see Follow-up): whether Clever Cloud meters
   egress the way Render did. Render's outbound-bandwidth exhaustion is one of the incidents that
   started this migration, and repeating it on a new PaaS is the single way this decision fails.

## Alternatives considered

- **OVH Public Cloud, 2 x d2-2 with self-hosted PostgreSQL** (~EUR 11.42/month) — the cheapest option
  in euros and the most expensive in the product owner's time. Lost on operational burden: WAL
  archiving, patching, monitoring and a block-volume data layout all become ours, and none of it is
  differentiating work. Kept in PROP-20260731-061609 D1/D2 as the fallback if the egress question
  disqualifies Clever Cloud.
- **OVH + managed PostgreSQL** — the original recommendation, and the right architecture. Ruled out by
  the product owner on cost, 2026-08-05.
- **Two OVH VPS** — ruled out on a hard technical fact: VPS cannot join a vRack, so the database would
  sit on a public IP or behind a self-run WireGuard tunnel.
- **Staying on Supabase free** — 500 MB of database, 500 MB shared RAM, no backups, no PITR, and a
  project that pauses after 7 days idle. Not a tier a production ordering system can run on.

## Consequences

### Positive

- **[PROP-20260805-181926](../proposals/PROP-20260805-181926-host-provisioning-and-configuration-ownership.md)
  largely dissolves**: D1–D6 (provisioning IaC, host configuration, rebuild-vs-converge posture,
  OpenTofu state, sequencing) have no subject without a host we own. Only **D7** survives, reduced:
  deriving deployment artifacts from the specs that already exist.
- The backup gap closes **without us building it**. Measured against the real baseline — Supabase free,
  which has no backups at all — this is the single biggest reliability gain available, and the event
  log is what it protects.
- Digest-pinned Docker deploys carry over, so ADR-20260721-175411's build half and
  ADR-20260730-051500's pipeline isolation both survive with a changed target.

### Negative

- **Monthly cost is above the OVH self-hosted option** and was NOT verified at decision time —
  secondary sources disagreed and this proposal has already been bitten once by a third-party spec
  table (the VPS-2 figures, corrected 2026-08-05). Prices come from the vendor's estimator only.
- **We are moving to a PaaS having just been failed by one.** The Render failure modes were egress,
  build caps and disk. Build is ours (GHCR) and disk is the managed plan's problem, which leaves egress
  as the exposure — hence the gate in Decision 4.
- Less control than an instance. That is the point of the choice and also its risk: a ceiling we
  cannot engineer around is a ceiling we must migrate away from, again.

### Follow-up

- [ ] **Settle the egress question before any spend** — how Clever Cloud meters and prices outbound
      bandwidth, checked against what the WASM bundle plus GraphQL traffic realistically costs at
      peak. This is a blocking precondition of the cutover, not a post-migration discovery.
- [x] Price the app instance and a **paid** PostgreSQL plan on the vendor's estimator, Paris region.
      **Done 2026-08-06: Rust `pico` EUR 4.50 + PostgreSQL `XXS Small Space` EUR 5.25 = EUR 9.75 HT/30
      days** — below the OVH 2 x d2-2 option (EUR 11.42) *and* managed. **But that exact selection is
      UNDER-SPECCED and must not be the one we buy**: `XXS Small Space` is **1 GB max database size
      with 512 MB memory**, which against the Supabase free tier we are escaping (500 MB / 500 MB
      shared) is 2x the storage and **parity on RAM**. The repo's own history is the argument: the
      SIRENE mirror measured **655 MB for 339k rows — 77% of the whole database — at department 37
      alone** before [#231](https://github.com/TheCaptainCompany/captain-food/issues/231) reclaimed it
      to ~4 MB steady state, and `sirene_ingest` is designed **France-wide by department** (~101 of
      them). Add an append-only event log that never shrinks, plus projections and indexes, and 1 GB
      has no headroom at all.
- [ ] **Re-size before buying.** Storage scales independently of compute on this platform — the plan
      ladder runs `XXS Small/Medium/Big Space` (Medium = 2 GiB), then `XS Tiny/Small/Medium/Big`, then
      `S Small/Medium/Big/Huge` — so move the **Space** dimension well past 1 GB and the instance
      dimension past 512 MB of memory. For the app, `pico`'s exact shape was NOT verified (`nano` is
      1 vCPU / 512 MB, `XS` is 1 vCPU / 1 GB): size it remembering the server does **not** just serve
      requests — the projector runs **in-process** (ADR-0040/0043), alongside the SIRENE sync worker
      and the actor-mailbox workers. Budget for a real total above EUR 9.75, which still compares well.
- [ ] Confirm PITR (pgBackRest) availability and how it is requested, since it is not on by default.
- [ ] Retarget `deploy.yml` at Clever Cloud, and retire `render-config-sync.yml` as already planned.
- [ ] Rename/re-scope [#271](https://github.com/TheCaptainCompany/captain-food/issues/271) — its title
      still says OVH, and #242 slice 3's prod-gate becomes "Clever Cloud cutover complete".
- [ ] Re-home the future Redis (D7 placement cache, [#267](https://github.com/TheCaptainCompany/captain-food/issues/267)
      `ScopeMembership`): Clever Cloud offers a Redis add-on, so the one-provider-one-jurisdiction
      argument in ADR-20260731-061609 still holds — it just points somewhere else.
