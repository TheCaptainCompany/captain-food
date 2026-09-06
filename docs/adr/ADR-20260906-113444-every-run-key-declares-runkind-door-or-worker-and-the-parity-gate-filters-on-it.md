# ADR-20260906-113444 — Every `RUN_*` bool key declares `runKind: door | worker`, and the fleet-parity gate filters on it

<!-- Filename: docs/adr/ADR-20260906-113444-every-run-key-declares-runkind-door-or-worker-and-the-parity-gate-filters-on-it.md -->

## Status

Accepted — a **team decision by consent**, 2026-09-06, under register row `TEAM-DECIDES-OPTION-SPACES`
([ADR-20260904-013834](ADR-20260904-013834-the-team-decides-option-spaces-and-spec-diffs-external-legal-and-admin-gated-actions-stay-with-the-founder.md)),
taken inside [#917 "SIRENE gate hole"](https://github.com/TheCaptainCompany/captain-food/issues/917) round 2 and
landed by [PR #918](https://github.com/TheCaptainCompany/captain-food/pull/918) (squash `8a34af1b`). Written after the
merge because the reviewer's presentation pass found the decision citable only as prose (dsl.md, SPEC-LOG, the PR
body, the journal) — [#919 "runKind follow-ups"](https://github.com/TheCaptainCompany/captain-food/issues/919) item 5.
**Amends in place**
[ADR-20260905-223957](ADR-20260905-223957-the-admin-door-in-two-slices-a-platform-membership-binds-the-admin-seam-then-the-system-host-is-routed.md)
§5, whose gate sentence read "every `RUN_*` key is `declare_flag`'d unconditionally at both roots": the gate's
population is the **declared doors**, never every key (#919 item 6). Reversal check on the terms *runKind*,
*run_flag_parity*, *declare_flag*, *door class*, *decisionRow* across `docs/decisions/`, `docs/proposals/DECISIONS.md`
and `docs/adr/`: §5 above is the only record touched; no record forbids or narrows the attribute.

## Enforced by

`tools/codegen-rs/src/config.rs::validate_configuration` — `config-run-kind-missing` (a `RUN_*` key of `type: bool`
with no `runKind:`) and `config-run-kind-unknown` (a value outside the closed set), both **errors** on the
`make validate` path; `tools/codegen-rs/src/tests.rs` `mod run_flag_parity` (the door population is
`run_kind == Some(RunKind::Door)`; the worker population must be non-empty; the converse test
`every_declare_flagged_key_is_declared_a_door` reads the two composition roots and refuses a `declare_flag`'d key
that is not a declared door); `mod run_kind_declared` (the real-corpus proof). Grammar authority:
[docs/claude/dsl.md](../claude/dsl.md) `## runKind:`.

## Context

Round 1 of #917 bound `decisionRow: SIRENE-RESTART` to `RUN_SIRENE_WORKER` — correctly, the row exists so that a
stopped worker cannot restart on an env flip alone. That bind tripped `run_flag_parity`, the 6-v fleet-parity gate
(ADR-20260905-223957 §5), whose door population was inferred from `decision_row.is_some()`: a PROXY for "is a
per-request door" that the honest bind falsified, exactly as
[#908 "6-v follow-ups"](https://github.com/TheCaptainCompany/captain-food/issues/908) item 3 had predicted. The
executor stopped on its own finding. The option space: infer the population from the `declare_flag` sites (invisible
to codegen), keep a hand-kept name list (the gate's doc comment WAS one and had drifted twice on comments alone),
or declare the class on the key.

## Decision

1. **A declared, required, closed-set attribute** `runKind: door | worker` on every `RUN_*` key of `type: bool` in
   `configuration.yaml`. A bare token closed in the loader
   ([ADR-20260811-014129](ADR-20260811-014129-a-business-metric-is-a-projection-and-every-reference-is-a-ref.md)),
   never a `scalars.yaml` scalar. Missing or unknown is a validator **error**.
2. **Classification comes from the key's own `gates:` prose**, never from its name suffix or the section heading it
   lives under. Sixteen keys today: seven doors (`RUN_PLATFORM_ACCESS_GRANT`, `RUN_ADMIN_SIGN_IN_DOOR`,
   `RUN_RIDER_RESTRICTION_SOCKET_CLOSE`, `RUN_RIDER_RESTRICTION_DOOR`, `RUN_MEMBER_ACCESS_GRANT`,
   `RUN_MEMBER_SIGN_IN_DOOR`, `RUN_RESTAURANT_INVITATION`) and nine workers (`RUN_MAILBOX_WORKERS`, `RUN_PROJECTOR`,
   `RUN_PROCESS_MANAGERS`, `RUN_EVENT_PUSH`, `RUN_MAILBOX_PUSH`, `RUN_DELETION_ENGINE`, `RUN_RETENTION_SWEEP`,
   `RUN_DELIVERY_OFFER_TIMEOUT`, `RUN_SIRENE_WORKER`). The seven doors are byte-for-byte the keys `declare_flag`'d at
   both composition roots — derived independently from prose, then checked mechanically.
3. **`decisionRow:` and `runKind:` are orthogonal.** `decisionRow:` means exactly one thing: a release gate bound to an
   open register row (`decision-row-open-key-must-be-off`). A worker may carry a row (`RUN_SIRENE_WORKER` does).
   `run_flag_parity` filters on `runKind`, never on `decisionRow:`.
4. **Red-first, in three steps, one commit each**: the grammar landed with the corpus red on all sixteen keys; the
   annotation turned it green with a spec-only diff; the gate repoint turned round 1's red green with zero
   `crates/**` edits. Two card mutants (a worker declared `door`; a door declared `worker` with its `declare_flag`
   left in place) were planted, fired as predicted, and reverted; the reviewer re-planted four on a sandbox copy.

## Consequences

- A door added tomorrow without `runKind: door` fails `make validate` before it can drop out of the parity
  population; a typo is reported as *unknown*, not *missing*.
- **Open, on #919 (items 2 and 3) — is a door NECESSARILY row-bound?** The `door` definiens in dsl.md and the
  key-convention header says "bound to a preconditions record"; evans reads that as the proxy re-smuggled as prose and
  asks for the clause to go, farley asks for `runKind: door ⇒ decisionRow:` to be enforced so a row-less door cannot
  ship ON with no preconditions gate. Both cannot hold; the team decides by consent when #919 is picked up and this
  record is amended with the outcome.
- The converse test requires a `declare_flag` at BOTH roots; a worker declared at exactly one root is invisible to
  both directions until #919 item 1 lands (`&&` → `||`).
- The gate proves the SPEC, not the running service: Render env beats BAKED, and `sirene-sync.yml`'s resume note
  still bypasses `SIRENE-RESTART` (#919 item 7).

## Consulted (ADR-20260812-143619 — one line per lens)

Consulted at the round-2 consent (three lenses, the reversible-refactor roster of ADR-20260816-134352) and at the
presentation pass (four); **no lens output is legal advice or clearance**.

- **farley** — a declared class, required on every boolean run-key, never inferred from consumption sites and never a
  hand-kept list; fix the population, never weaken the assert. At presentation: PASS; the door population is the same
  seven keys; asks for door ⇒ row enforcement and a live-vs-BAKED drill (#919).
- **evans** — the word is `runKind: door | worker`, both already the repository's own terms; `enforcement` is taken,
  `topology`/`subsystem` are words the tree does not speak; a bare token, never a scalar. At presentation: PASS; the
  definiens drifted (the row clause, the refusal parenthetical, the "Worker toggles" header, the word's other prose
  senses) — #919.
- **beck** — three red-first shapes, all planted and confirmed (grammar red on the unannotated corpus; the converse
  test; the two mutants). At presentation: PASS; the reds are observable at their commits; the one-root worker is the
  missed third mutant (#919 item 1).
- **reviewer** (presentation) — PASS on the full diff; four mutants re-planted on a sandbox copy; the decision needed a
  register id (this record) and ADR-20260905-223957 §5 needed the banner (applied).
- **the executor's own finding** — the collision itself, round 1's STOP.
- **architect, young, vernon, dba, graphql-architect, observability-agent, legal-specialist, business-specialist,
  ux-designer, holub** — not asked: a codegen-grammar refactor with no stored shape, no API, no money and no legal
  surface (reversible class); the consult surface for write-capable dispatches was refusing `Red-first: none`
  consults at the time (#914 item 10).
