# ADR-20260817-105844 — The walk goes first, on ONE database, and production stays suspended on purpose

**Status**: Accepted · **Date**: 2026-08-17 ·
**Decider**: the **FOUNDER / Tech CEO**, answering two rows of the 2026-08-17 decision queue ·
**Amends**: [ADR-20260813-191111 "The acceptance criterion: six clauses walked with the front door unlocked from inside"](ADR-20260813-191111-the-acceptance-criterion-six-clauses-walked-with-the-front-door-unlocked-from-inside.md)
— specifically the **gating** half of its "Scope clarification (founder directive, 2026-08-14)"
section, and only that half ·
**Register**: [DECISIONS §45](../proposals/DECISIONS.md) rows **PROD-1** and **SEQ-1** ·
**Session**: https://claude.ai/code/session_01SDJjYQsfwaa4DVyNfFepbA

## Status

Accepted.

## Enforced by

n/a — no behavioral guarantee

(The one executable residue this decision creates is named in *Follow-up actions*: the nightly
`prod-smoke` schedule now points at a target the team has decided not to have.)

## Context

Two questions went to the founder together, and he answered them together, because they are one
question about **where the first end-to-end reading happens**.

### The recorded contradiction this closes

Two records disagreed about the same sequence, and both were live:

- **2026-08-13** ([ADR-20260813-191111](ADR-20260813-191111-the-acceptance-criterion-six-clauses-walked-with-the-front-door-unlocked-from-inside.md)
  §5, re-sequenced 2026-08-14 by the architect under that section's own grant): the local acceptance
  harness ([#556](https://github.com/TheCaptainCompany/captain-food/issues/556)) and smoke L5 run on
  a **single-database** monolith stack; the eleven-database stack is a precondition of the **final**
  acceptance walk only.
- **2026-08-14** (founder directive, *"The acceptance include the full enforcement and full split"*,
  recorded as that ADR's "Scope clarification"): the harness and the walk target the
  **physically-split, least-privilege, write-authorization-enforced eleven-database stack from the
  start**, because under
  [ADR-20260808-235113](ADR-20260808-235113-final-vision-first-no-intermediate-steps.md) a single-DB
  stack that must then be rebuilt is the throwaway intermediate that directive forbids.

`STATUS.md` had been reconciled earlier on 2026-08-17 by marking the 2026-08-14 entry **the live
sequence** and the older one **superseded in place**. That reconciliation stated which reading won;
it could not state which reading was *right*, because that was a founder call. This record makes it
one reading.

### What the split band is actually waiting on

The 2026-08-14 tail sequences the physical split (#513 → #514 → #509) **before** anything is walked.
Measured against the register on the day of this decision, that band cannot start:

- **[STO-7](../proposals/DECISIONS.md)** ⚠️ OPEN — blocks the physical split of
  `read_order`/`read_catalog` (the cart READ path *and* the checkout WRITE path, both fail-closed
  post-split as mapped).
- **STO-8** ⚠️ OPEN — blocks the physical split of `read_common` (the `verify_phone` login-path read
  among them).
- **STO-9** ⚠️ OPEN — blocks the physical split of `read_order` **independently of STO-7**, and says
  so in its own binding constraint: settlement's pre-capture read still fail-closes even if STO-7
  and STO-8 are both answered well.
- **STO-10** ⚠️ AMBER/founder-owned — reopens ADP-1, and carries the standing instruction that #513
  must not emit the CONNECT that would decide it by default. **Parked by the same answer sheet**
  (§45 **STO-10-PARK**).
- **RDR-1** 🟡 TEAM-OWNED, open since 2026-08-15 — its own recommendation is to decide it **before**
  #513/PR2's emitter, since the emitter encodes an assumption about `model:` either way and
  reversing it afterwards means re-deriving grants that are by then in generated manifests.

Four of these five are open decisions, not open work. The fifth is team-owned and unresolved.

### The production question, and the defect underneath it

`captain-food.onrender.com` has been suspended for billing since ~2026-08-04 12:26 UTC. The team's
recommendation was to restore it with signup closed at the auth provider, on the corrected premise
that customer signup is **self-service** (`requestPhoneVerification` / `verifyPhone` are
`roles: [PUBLIC, CUSTOMER]`, and a first verified phone CREATES the Customer), so restoring with
signup open *is* issuing credentials outside the team while the cross-tenant IDOR is open
([DECISIONS §39](../proposals/DECISIONS.md)).

**The founder declined, and the harder finding is not the 503.** Verified against the GitHub Actions
API on 2026-08-17:

| Fact | Value |
|---|---|
| Last GREEN scheduled `prod-smoke` | **2026-07-29** |
| Consecutive RED scheduled runs since | **19** (2026-07-30 → 2026-08-17, no gaps) |
| Of those, explainable by the billing suspension | **13** (2026-08-05 → 2026-08-17) |
| The other **6** (2026-07-28 one-off, 2026-07-30 → 2026-08-04) | a **different, unrecorded** cause |

A nightly gate had been red for **five nights before the suspension existed** and for fourteen after
it, and no record treated it as a broken gate. That is the defect class
[ADR-20260810-231300](ADR-20260810-231300-no-polling-only-pushing-polling-as-graceful-fallback.md)
names for monitoring, arrived at from the other side: a dead-man's switch that has already died
reports nothing new when the thing it watches dies again.

## Decision

### 1. Production stays suspended — as a DELIBERATE, RECORDED state

Production is **not** restored. This is now a decided posture with a named owner and a named exit,
not an open incident waiting on an account action:

- **The walk does not need production.** Everything the first end-to-end reading requires runs
  locally, including the money path: `stripe listen --forward-to` is **outbound-only**, which is
  exactly what makes it work — the CLI opens the tunnel *from* the local stack and its own signing
  secret satisfies the fail-closed `STRIPE_WEBHOOK_SECRET` boot gate
  ([ADR-20260813-004634](ADR-20260813-004634-supabase-auth-is-retained-for-v0-and-the-window-closes-at-the-first-real-order.md),
  [DECISIONS §36 IDP-1](../proposals/DECISIONS.md)). No cluster ingress, no founder action.
- **The signup decision recorded on 2026-08-17 is answered by circumstance, not by rule.** §39 left
  *"keep signup closed at the auth provider while production runs, or close #618 first"* as a
  decision owed. With production down, neither arm is exercised. **It is not closed** — it becomes
  owed again, unchanged, the moment restoration is on the table. Recorded here so a future session
  does not read the silence as an answer.
- **What does not change**: local remains **demo, never evidence**
  ([DECISIONS §35 INV-1](../proposals/DECISIONS.md)); no recovery claim may cite a local walk, and
  [#429 "Production with test data"](https://github.com/TheCaptainCompany/captain-food/issues/429)
  still closes only on the provisioned cluster.

### 2. The walk goes first, on ONE database

- **The acceptance criterion is UNCHANGED as what CERTIFIES.** The six clauses in the founder's
  order, the deliberately-unlocked-from-inside auth posture (§3), the D2 capture semantics (§4), the
  honesty sentence (§6), the eleven-database least-privilege stack, and full enforcement including
  the write-authorization fix — all of it still stands as the definition of *accepted*.
- **What changes is that the split band stops GATING the first end-to-end reading.** The harness
  ([#556](https://github.com/TheCaptainCompany/captain-food/issues/556)), L5's lifecycle legs and
  the browser walls are built and run on **one database**, now, ahead of #513/#514/#509.
- **The target is the stack that already ran.** On **2026-08-11**, on single-node k3s, watched: the
  CNPG 1.27.4 operator → `captain-db` Cluster healthy with `initdb` complete → the full migration
  chain applied to an empty database → the generated monolith overlay applied verbatim → the pod
  `1/1 Running` → `/health` 200 with a matching `requiredSchemaVersion` → `/ping` = `pong` →
  `prod-smoke.sh` **L1 and L2 PASS**. The schema line was re-verified on 2026-08-13 after #500's
  migration landed. That stack is the walk's target; it is not a thing to be designed.
- **A reading is not a certificate — and the label is mandatory.** Any artifact produced by the
  one-database walk is labelled a **reading**: what the machine did end to end on one database. The
  word *accepted* is reserved for the eleven-database, fully-enforced walk. The same discipline
  ADR-20260813-191111 §4 already imposes on an interim capture walk applies here, for the same
  reason: a labelled interim result is useful, an unlabelled one silently redefines the criterion.

### 3. Why this does NOT overturn final-vision-first

[ADR-20260808-235113](ADR-20260808-235113-final-vision-first-no-intermediate-steps.md) forbids an
intermediate step **where the final step can be built**. Here it cannot: the eleven-database stack
is decision-blocked on STO-7, STO-8 and STO-9 (each independently), with STO-10 parked and RDR-1
open upstream of the grant emitter. "Build the final step first" would mean **no end-to-end reading
exists until four open decisions close** — which is not the final step arriving earlier, it is the
first reading arriving never.

The directive's own carve-out applies verbatim: *where staging is externally forced, the
intermediate ships only with the final step already designed and recorded.* It is: the eleven-DB
stack is fully specified (`specs/database/databases.yaml`, the §18 placement gate, the generated
inventory), sequenced (#513 → #514 → #509), and remains the certification target of this ADR's §1.
Nothing about it is being deferred except **when it is walked on**.

## Alternatives considered

- **(a) Restore production and walk there** — the team's recommendation, **declined by the
  founder**. It costs money for a target the walk does not need, and on the corrected public-repo
  premise it would have to run with signup closed at the auth provider (an unbuilt control) while
  §39's self-service-signup hole is open. Its one genuine advantage — a walk on the real hosting —
  is precisely what INV-1 already says a local walk cannot claim, so restoring buys the claim, not
  the reading.
- **(b) Hold the walk until the split band clears** — the 2026-08-14 reading. Rejected: the band is
  blocked on four open decisions, so this is a schedule with no start date, and the first
  end-to-end reading is the artifact the spend gate itself is waiting on (INV-1).
- **(c) Walk on one database now, certify on eleven later** — **chosen**. It separates *reading*
  from *certificate*, which is the distinction the two contradicting records were conflating.

## Consequences

### Positive

- **The contradiction is gone.** One sequence: harness + L5 + browser walls on one database now;
  the split band, the write-auth fix and the `inbound_messages` hardening before the walk that
  *certifies*. `STATUS.md` stops carrying two readings with a tie-break rule.
- **The first end-to-end reading is unblocked today**, against a stack that has already stood up
  once, with no dependency on any open register row.
- **The suspension stops being an incident nobody owns.** A permanently-red nightly is now either a
  known-red gate with a recorded reason or a re-pointed one — not a green-looking pipeline with a
  red job in it that everyone has learned to skip.

### Negative

- **A one-database walk will pass things the split stack will fail.** STO-7's cart read, STO-8's
  `verify_phone` and STO-9's pre-capture settlement read all work today precisely because the wall
  is not physical. The reading therefore proves the order machine, **not** the boundary design — and
  the split band will find defects the reading cannot. That is a known cost of (c), not a surprise
  to be discovered later.
- **Production stays dark**, so nothing exercises the deploy path, TLS, DNS or the real image; every
  gap the 2026-08-11 rehearsal listed as unproven stays unproven.
- **The `HOLD: human` reversibility axis is unaffected** — none of this licenses a Tours-facing
  change; the walk uses synthetic identities (ADR-20260813-191111 §6).

### Follow-up actions

- **The nightly `prod-smoke` schedule must stop lying.** It has been red for 19 consecutive
  scheduled runs and is now pointed at a target the team has decided not to have. Either it is
  re-pointed at the local walk target, or its schedule is disabled with the reason recorded in the
  workflow itself. A scheduled gate whose red is expected is worse than no gate: it trains the team
  to ignore the one signal that would tell them something new. **Owed as an issue, and deliberately
  NOT filed by the records run that landed this ADR** — it was dispatched to file exactly one issue
  and did not widen its own scope; the drafted text is in its run report and the `architect` owns
  filing it (§45 **PROD-1**).
- The **6 pre-suspension red nights** (2026-07-28, 2026-07-30 → 2026-08-04) have an unrecorded
  cause. Whoever re-points the nightly reads one of those runs first; a cause that predates the
  suspension will survive it.
- `docs/STATUS.md`'s 2026-08-14 "live sequence" marker and its superseded-in-place counterpart are
  reconciled in the same change as this ADR.

## Consulted (ADR-20260812-143619 — one line per lens)

The consultation on the sequencing question was run by the **coordinator** before the founder
answered; this record was written by the **executor** from the register and the repository. Each
line below therefore names the lens's position **as it stands on the record**, with its provenance,
and says so where the dispatch relayed no fresh text. A lens never asked must not be
indistinguishable from a lens with nothing to say — and neither must a lens whose words were not
carried into the record.

- **architect** — owns the re-cutting grant this decision exercises (ADR-20260813-191111 §5:
  *"the architect owns re-cutting this if reality disagrees"*), and produced the read-only
  `file:line` verification that the physical split is today a **declared-and-gated MAP** only (one
  CNPG cluster, one database `app`, zero per-database grants), which is the fact that makes the
  band's blockage checkable rather than asserted.
- **farley** — the browser leg needs the **wasm bundle actually served**, not just the API up
  (recorded position, ADR-20260813-191111); it is a deployment property of the walk target and is
  satisfied by the monolith overlay the 2026-08-11 rehearsal applied.
- **holub** — authored the honesty sentence (§6) and the rule that *anything not on the six-clause
  path is out of the acceptance claim, whatever else it improves*. That rule is what forces the
  reading/certificate labelling in Decision §2 rather than letting a one-database walk drift into
  being called acceptance.
- **beck** — L5 is three-plus **seen-red** legs, and nothing green today can assert
  capture-after-delivery; the harness must be written against D2 semantics or it certifies the
  wrong machine (recorded position). The one-database target does not weaken this: RED-first is a
  method, not a topology.
- **dba** — *"eleven databases are free locally, the plumbing is not"*: the chains (#514), the
  Database CRs and the local overlay belong in ONE slice, and the delivered leg forces the
  `View_DeliveryJob` → table conversion there (recorded position). This is the sharpest argument
  **for** the 2026-08-14 reading, and it is why the eleven-DB stack stays the certification target
  rather than being dropped.
- **young** — a read model is a disposable, rebuildable fold; the one-database walk exercises the
  same folds the split stack will, because the split changes *where* they live, not *what* they
  compute. No position was relayed to this record beyond that standing doctrine.
- **vernon** — one aggregate per transaction and the one-writer rule are unaffected by database
  topology; the settlement read STO-9 is about is a **source-of-truth mismatch independent of any
  wall** (his recorded position in STO-9), which is exactly why the wall's absence in the reading
  does not make the reading dishonest about that defect. No fresh text relayed.
- **evans** — no position relayed to this record. The bounded-context question this touches
  (`boundedContexts:` vs `specs/{scope}/`, §31 BND-1) is not moved by where the walk runs.

**Not asked** (named, per the rule): `graphql-architect`, `legal-specialist`, `business-specialist`,
`observability`, `reviewer`, `ux-designer`, `generator`. The founder's parallel authorization
questions were consulted with a different roster (see
[ADR-20260817-105845](ADR-20260817-105845-a-dispatch-card-may-not-state-a-derived-number-without-its-antecedents.md)
and §45). **Worth naming plainly**: `legal-specialist` holds the standing condition that a public
dated record is an asset only while its date holds, and `observability` is the lens with standing on
a nightly gate that has been red for nineteen runs — neither was asked about the production
decision, and both would have had something to say about the follow-up actions above.
