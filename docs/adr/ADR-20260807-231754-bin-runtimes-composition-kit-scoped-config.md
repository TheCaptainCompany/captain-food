# ADR-20260807-231754 — Bin runtimes: shared composition kit, scope-filtered per-bin Config, ports-derived adapter links

- **Status**: Accepted (realization of [#385 "Bin runtime wiring: business runtimes inside the 49
  shells"](https://github.com/TheCaptainCompany/captain-food/issues/385), the carried-forward
  remainder of ADR-20260807-183024 step (4), sitting between steps (5) and (6))
- **Tracking issue**: [#385 "Bin runtime wiring: business runtimes inside the 49 shells"](https://github.com/TheCaptainCompany/captain-food/issues/385)
- **Extends**: ADR-20260807-183024 (one decomposition axis) — this ADR decides HOW the emitted
  shells become business runtimes; no new option space large enough for a proposal (the
  architecture was decided there; these are its realization decisions, recorded inline).

## Decisions

1. **One hand-written composition kit, generated parameter lists** (`crates/bin_runtime`). A wired
   bin's `main.rs` stays GENERATED and trivial: config gate → telemetry → declared-size pool →
   one family spawn call → probe server. All machinery — telemetry init, pg pool, the probe
   server, the fleet/runner/projector spawns — lives in `bin_runtime`, which itself only assembles
   existing `application`/`infrastructure` code (the actor/pm fleets ride the SAME
   `infrastructure::mailbox::standalone` runtime the adapter binaries use; the flip-time backfill
   sequencing mirrors the monolith's `router()`). Options considered: emitting full monolith-style
   composition per bin (27 divergent copies to review — rejected), or a runtime crate per family
   (three crates sharing 90% — rejected). One kit, three spawn helpers, won.

2. **Per-bin Config = the same generated reader over the bin's scope-filtered key subset**
   (closes [#374](https://github.com/TheCaptainCompany/captain-food/issues/374) Q4). Each wired
   bin gets a generated `src/config.rs` — the identical typed reader the monolith uses
   (fail-fast by profile, every problem reported at once, scalar-validated values, secrets never
   printed) — emitted over the keys whose ORIGIN SCOPE is in the bin's linked scopes + owning
   scope + `common`: exactly the set the deploy emitter routes to its pod as env, so reader and
   pod env derive from one routing. A bin structurally cannot read another scope's key: the field
   does not exist on its Config. Reminder-window keys outside the bin's scopes are FILTERED (the
   standalone fleet falls back to the spec default, loudly) where the monolith ASSERTS — it hosts
   every lane, a bin hosts a slice. Rejected: one shared full-config crate (re-opens D5's
   "every pod reads everything"), hand-written per-bin readers (drift by construction).

3. **sqlx pool sizes are DECLARED**: `DATABASE_POOL_MAX_CONNECTIONS` (common scope, default 5) —
   read by the monolith's pool and every wired bin's. 49 pods × this ceiling is the CNPG
   connection budget, so the number is a one-place spec review, with env override per pod for
   incidents (env > baked > default).

4. **Integration-adapter links derive from spec `ports:`**. A bin links the Stripe adapter iff
   its actor/PM declares the `payment` port; the delivery-partner composite (avelo37, coopcycle,
   uber_direct + the independent-pool no-op) iff it declares `delivery`. No hand-curated money-bin
   list. This surfaced a REAL spec omission: `ReclamationProcess` drives Stripe refunds
   (its #207 refund arm calls `approve_refund` → the payment service) but declared no `payment`
   port — undetectable while the monolith hosted every PM. The port is now declared; the bin
   derivation is what made the hole visible.

5. **Runtime registries carry scope labels tied to spec placement.** The projection worker's
   hand-written registry now labels each group with its owning scope (the `projector-{scope}`
   slice, D4) and the saga runner accepts a single-PM restriction (`pm-{name}` bins); both filters
   run on the SHARED per-group checkpoints, so monolith ⇄ per-scope handover needs no
   re-projection and loses no position. The labels are tied to the generated `ACTOR_SCOPES`
   table (emitted from `specs/{scope}/` folder placement) by unit test — a spec move that
   re-homes an aggregate turns a stale label into a red test, not a mis-scoped projector.

## Recorded costs (accepted, with their exits)

- **Wired bins couple to the full `domain` facade through `infrastructure`** — the build/deploy
  blast radius of a domain-scope change is now every wired bin (the determinator property tests
  encode this honestly). The VOCABULARY containment holds — `domain` is a transitive dep, so
  `actor-order` still cannot spell a catalog type — but the re-sharpened build closure waits on
  splitting `infrastructure` per scope (follow-up recorded on #385).
- **Cross-process push gap**: the operation-status bus and the GraphQL-subscription event bus are
  in-process, so a delivery completed in a bin reaches pollers but not another process's push
  subscribers — the same documented trade-off the standalone adapter fleets carry
  (LISTEN/NOTIFY fan-out is the recorded follow-up).
- **`delivery` scope owns no projection group today** (its read models are saga state tables):
  `projector-delivery` idles caught-up and says so — a viable, honest pod, not a crash loop.
- **The money posture is read twice in a mailboxed PM bin** (runner sequencing + fleet's own
  read). Both read the same row; a mid-boot flip is absorbed by the next restart, and the fleet's
  refusal semantics are unchanged.
- **Connection arithmetic is a CUTOVER PRECONDITION, not solved here.** At the declared default
  (5 max / 1 min per pool) the 27 wired bins alone can demand 27×5 = 135 pooled connections plus
  ~30 dedicated LISTEN connections (event push for pm/projector, mailbox push for actor/pm) —
  against CNPG's single ~1 Gi instance (ADR-20260807-114122), whose Postgres default ceiling is
  100. Nothing applies the manifests yet (no Argo, step 6), so nothing is live-overcommitted
  today; but flipping steps (6)–(7) REQUIRES either lowering the per-bin ceiling (env override
  per pod on `DATABASE_POOL_MAX_CONNECTIONS` — min_connections is 1, so idle bins hold 1+LISTEN),
  raising `max_connections`, or a pooler (which then conflicts with LISTEN — the RUN_*_PUSH
  escape hatches exist for exactly that). Recorded on
  [#385](https://github.com/TheCaptainCompany/captain-food/issues/385)'s remainder; the
  arithmetic re-runs whenever the bin count or the default changes.
