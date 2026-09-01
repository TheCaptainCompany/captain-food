# ADR-20260728-143000 — The SIRENE mirror's payload is transient; the hash is what persists

- **Status**: Accepted
- **Date**: 2026-07-28
- **Issue**: [#231 "The SIRENE mirror stores verbatim INSEE payloads (~1.8 kB/row) to read 5 fields — it is 77% of the database and blocks national coverage"](https://github.com/TheCaptainCompany/captain-food/issues/231)
- **Proposal**: [PROP-20260728-120931](../proposals/PROP-20260728-120931-sirene-mirror-payload-is-transient.md)
- **Refines**: [ADR-0045](0045-sirene-sync-staging-table-and-worker.md) (staging-table retention), [ADR-20260728-011344](ADR-20260728-011344-slug-lifecycle-and-sirene-inbound-events.md) (`payload_hash`)

## Context

`external_sirene_restaurants` kept the verbatim INSEE établissement record **forever** in order to read
five fields out of it (SIRET, name, address, NAF, état). Measured on production 2026-07-28:

| table | rows | size | share |
|---|---:|---:|---:|
| `external_sirene_restaurants` | 339,077 | **655 MB** | **77%** |
| `domain_events` | 116,276 | 128 MB | 15% |

That is at department **37 of 101**, on a **2 GB disk with ~580 MB free**, on the Supabase Free plan,
with the project already flagged *exceeding usage limits*. Full France projects to ~2 GB for this one
table. [#218](https://github.com/TheCaptainCompany/captain-food/issues/218) made the sweep stalest-first
and budgeted, so it is now *capable* of national coverage — but pacing does not create disk, so storage
is what actually gates it.

The WAL was investigated and rejected as the lever: no replication slots pin it, no archiver failures,
it sits inside the configured `max_wal_size`, both shrink levers (`CHECKPOINT`, `ALTER SYSTEM`) are
permission-denied on this plan, and shrinking it would force more frequent checkpoints — trading disk
for the IO budget that triggered [#220](https://github.com/TheCaptainCompany/captain-food/issues/220).

## Decision

**The payload and the hash have different lifetimes.**

- The **payload** is an *input to translation*: needed from the moment INSEE reports a change until the
  worker has turned it into a domain fact, and never again.
- The **hash** is the *change-detection key*: needed forever, because every future sweep compares
  against it.

So the payload is present exactly while a row is **pending**. Concretely:

1. The ingestion writes a payload only when the row will pend — an unchanged, already-processed record
   keeps whatever the row holds (`NULL`, once compacted).
2. The worker NULLs the payload in the **same statement that records the sync as CERTAIN** — never
   earlier. See "Evidence before removal" below; this is the correction that matters most.
3. A record the ACL could not **map** (or could not parse) **keeps** its payload — it is the only
   evidence of why INSEE's record was unusable.
4. A one-shot compaction pass (`sirene_ingest --compact`) applies this to rows already in the table.
5. A new **`status`** column records what the worker decided — `PENDING` / `SYNCED` / `UNMAPPABLE` /
   `FAILED` — so the table answers "has this row been synced, or not?" directly.

Steady state goes from ~1.8 kB/row to ~200 B/row: ~655 MB → ~90 MB today, ~250 MB at full France.

### Why "store only the hash" cannot be literal

The on-app worker reads `payload` to run the ACL — with no payload there is nothing to translate. The CI
ingest crate cannot translate instead: ADR-0045 deliberately keeps domain logic out of it so the
version-sensitive ACL runs only on the deployed server. That was the fix for the retired direct-write
binary's version-skew hazard and must not be undone to save disk. Hence *transient*, not *absent*.

### The product-owner decisions (PROP-20260728-120931)

| # | Question | Answer |
|---|---|---|
| D1 | What the mirror retains | ✅ Payload transient, hash permanent |
| D2 | Hash algorithm + encoding | ✅ Keep SHA-256, store as `bytea`; column stays named `payload_hash` — **sequenced after compaction**, see below |
| D3 | Unmappable / failed rows | ✅ Keep their payload |
| D4 | Migration strategy | ✅ Batched `UPDATE` with `VACUUM` interleaved |
| D5 | Replay/backfill posture | ✅ Accept re-fetch from INSEE — with a sharper reason than the proposal gave |
| — | Where compaction runs | **CI `sirene_ingest` job**, *against* the recommendation of server-side |

## Consequences

### D5 costs less than the proposal claimed

The proposal presented "an ACL that learns to read a new INSEE field cannot re-translate from the
mirror" as a real loss requiring a ~4h special re-fetch. The product owner's answer — a re-fetch is
absorbed by the hash comparison — is right, and the mechanism is stronger than that.

`payload_hash` covers the **typed projection**, a canonical re-serialization of
`sirene_ingest::wire::Etablissement`. So the day the ACL learns to read a new field, **adding it to the
wire types changes the digest of every record**. The next ordinary paced sweep then re-pends and
re-translates the whole mirror by itself. The backfill is not an operation anyone has to build; it is
the normal sweep noticing that everything changed. What it costs is INSEE quota, which a sweep spends
anyway.

The same property is why the hash fails safe generally: a field we start parsing is automatically
covered, so a real change can never slip through as "unchanged".

### D2 is sequenced after compaction, not shipped with it

`ALTER TABLE … ALTER COLUMN payload_hash TYPE bytea USING decode(payload_hash,'hex')` **rewrites the
whole table** and needs free space equal to its current size — ~655 MB against ~580 MB free. It would
fail exactly as the earlier `VACUUM FULL` did. Once compaction has run and live data is ~90 MB, the same
statement is cheap. The decision stands; only its order changes. Migration `20260728050000` documents
this at the point where a future reader will look for it.

### Choosing CI over the server costs part of D3, for historical rows only

`sirene_ingest` contains no domain code (ADR-0045), so the compaction runs the wire types but **not the
ACL**. It therefore cannot distinguish a row that parses fine yet is ACL-unmappable (no usable name, no
postal code) from one that translated successfully. The compaction:

- **keeps** the payload of any row whose JSON fails to *parse* — a wire-level judgement the crate
  legitimately owns, and the strongest diagnostic category;
- **drops** the payload of every row that parses, including ACL-unmappable ones (~5.4k historically).

D3 holds **going forward** — the on-app worker has the ACL and keeps those payloads. It is only this
one-shot historical pass that cannot. Moving the pass server-side would close the gap and is a
contained change if that tail turns out to matter.

*Superseded by "Evidence before removal" below: the pass no longer classifies rows at all — both of
its arms only transcribe verdicts recorded elsewhere — so the gap this section weighed is gone.*

### Disk is reclaimed in two steps, and the first one does not shrink the file

A plain `VACUUM` makes freed space **reusable by this table**; it does not return it to the OS, so
`pg_total_relation_size` stays ~655 MB after compaction. That is still the win that matters —
departments 38–101 fill that free space instead of extending the file, so full France fits in what
departments 1–37 already cost. Actually returning the disk needs a `VACUUM FULL`, which becomes
affordable only **after** this pass, because it rewrites into free space equal to the *live* data
(~90 MB, against the ~620 MB that made the earlier attempt fail with `No space left on device`).

Stating this explicitly because "we compacted and the database is still 655 MB" is otherwise going to
read as a failed change.

### `status` is a consequence of the payload becoming transient, not a decoration

Product-owner addition during implementation. Making the payload transient would otherwise have made the
table **ambiguous**: once `payload` is nullable, a row that HAS one is either still awaiting translation
or was kept as evidence of an unmappable record — and nothing distinguished the two. Before this change
the question did not arise, because every row had a payload.

So `status` is what keeps the table readable, and it answers the operator's actual question directly
rather than by inference from `processed_at >= last_seen_at`:

| value | meaning | payload |
|---|---|---|
| `PENDING` | ingested/refreshed, not yet translated | present — the worker needs it |
| `STAGED` | handed to `inbound_events`; the aggregate has not decided | kept — the inbound row holds only the TRANSLATED copy (see "Evidence before removal") |
| `SYNCED` | reached the domain (verdict resolved, or a synchronous closure) | dropped |
| `UNMAPPABLE` | ACL-rejected or unparsable | kept — it is the evidence |
| `FAILED` | last attempt failed; row stays pending and retries | present |
| `POISON` | quarantined after 10 consecutive failures; the drain skips it | kept — needed to diagnose it |

#### `STAGED` exists because the worker does not know the aggregate's verdict

Raised by the product owner during implementation, and it is a correctness issue rather than a
refinement. Since ADR-20260728-011344 the register path **stages an inbound fact** instead of issuing a
command — `InboundEventsDrainWorker` delivers it later and the *aggregate* decides. So at hand-over the
SIRENE worker genuinely does not know whether the record was accepted. Marking the row `SYNCED` there
would have made the mirror assert a success nobody observed, and a later delivery failure would never
correct it.

The fix needs no new bookkeeping, because the link already exists: the ACL writes
`inbound_events.external_id = '{siret}:{payload_hash}'`, and both halves are columns on the staging row.
Each drain pass therefore resolves its `STAGED` rows with a single joined `UPDATE`:

- `DELIVERED` / `IGNORED` / `DUPLICATE` → **`SYNCED`**. All three mean the record reached the domain and
  the domain is now correct about it. A no-change verdict is a real answer, not a failure — conflating
  the two is what once made a sweep unable to tell 200,000 registrations from 200,000 no-ops.
- `FAILED` → `FAILED`.
- `RECEIVED` → still in flight, left alone.

This necessarily **lags by at least one drain**, because the verdict does not exist at hand-over time.
`synced_at` is stamped from the inbound row's `delivered_at`, so it dates the moment the domain actually
accepted the record rather than the moment we handed it over.

#### `POISON` stops the retry, rather than just labelling it

A failed sync deliberately leaves the row pending **with** its payload — the retry needs something to
translate — so nothing in the pending predicate ever excludes a permanently-broken row. It would be
re-attempted every pass forever, burning the sweep's budget and emitting an error nobody acts on. That
is not hypothetical: the 605-row `SlugAlreadyTaken` log storm was exactly this shape.

So `attempt_sync_retry_count` counts **consecutive** failures (resetting on any checkpointed outcome,
which is what makes it answer "is this stuck *now*?"), and at 10 the row becomes `POISON` and the drain
filters it out. Ten is generous enough that no transient outage can quarantine a healthy row, small
enough that a broken one stops costing anything within a sweep or two.

Recovery is automatic: a **changed** record from INSEE re-pends the row through the ordinary conflict
arm, which writes `PENDING` and so releases the quarantine. Quarantine therefore holds a row exactly as
long as it keeps arriving unchanged and broken — no operator step, and no permanent leak.

#### `synced_at` and `last_attempt_sync_at` are not `processed_at`

`processed_at` is a **checkpoint**, not a wall clock: the worker sets it to the `last_seen_at` it *read*
so a concurrent ingestion bump re-pends the row rather than being swallowed, and the ingestion then
advances it to `now()` on every unchanged row it re-sees. So it moves for rows nothing happened to.
`synced_at` moves only when a fact actually reached the domain and survives a re-pend ("last synced 3
weeks ago, PENDING again since yesterday"); `last_attempt_sync_at` moves on every attempt, successful or
not.

`GROUP BY status` is the per-sweep report. It does NOT replace `processed_at`: that remains the
concurrency-safe checkpoint (marking a row at the `last_seen_at` that was READ is what makes a
concurrent ingestion bump re-pend it rather than be lost), while `status` is the outcome.

**It is TEXT, not a scalar enum `$ref`.** Enums persist as declaration-order ints (ADR-0037/0041), and
the CI `sirene_ingest` crate — which writes `PENDING` — cannot see domain types at all (ADR-0045 keeps
domain crates out of that build). A shared enum would therefore mean hardcoded ordinals in a crate that
cannot verify them, so a future reordering would silently reinterpret every row. That is precisely the
hazard `InboundEventStatus` carries a warning comment about. Text has no such failure mode and matches
the sibling columns (`etat`, `naf`) on this adapter-owned table.

`FAILED` is written on the retry paths (staging or close failure) WITHOUT advancing the checkpoint, so a
row that keeps failing is visible instead of being indistinguishable from one not yet reached. The write
is best-effort: it runs on a path that is already failing, and turning a retryable error into a hard one
to record an annotation would be strictly worse.

### Evidence before removal (correction, product owner)

> *"Before removing the existing payload we need to know if the sync has been done successfully. Once you
> have indicated the column `synced_at` and the status you can remove the payload without doubt."*

The first implementation of this ADR removed payloads on an **inference**, and the inference was wrong.
Two places:

- the compaction pass read `processed_at >= last_seen_at` as "already translated", wrote `SYNCED` on the
  strength of that reading, and then dropped the payload — deciding the outcome itself and then trusting
  its own decision;
- the worker dropped the payload at **hand-over** (`STAGED`), before the aggregate had decided anything.

`processed_at` is a **checkpoint, not a verdict**. The worker advances it for an unmappable row, for one
whose write failed, and for one merely handed to the inbox; the ingestion advances it again on every
unchanged row it re-sees. It never carried the information that was being read out of it. Deleting the
only original record on that basis is irreversible without a ~4h re-fetch from INSEE.

**The rule is now: a payload is removed only from a row that positively records having reached the
domain** — `status = 'SYNCED'` **and** `synced_at IS NOT NULL`, two independent witnesses of the same
fact, both written by the code path that observed it. Concretely:

- the register path drops the payload in `reconcile_staged`, the same statement that writes the
  aggregate's verdict back;
- the explicit-closure path drops it at mark time, because `MarkRestaurantClosed` has actually executed
  by then;
- `STAGED`, `FAILED`, `POISON`, `UNMAPPABLE` and pre-`status` rows all keep theirs.

Note what "the inbound row has its own copy" does not buy: that copy is the **translated** form, which is
exactly what is in question if the ACL mistranslated the record. The raw payload is the only original.

#### The pre-#227 verdicts were recorded all along — in the command journal (correction, product owner)

> *"Before we were using a command `RegisterRestaurant`, which means the data of the existing sync are
> in the `command_journal`."*

The first version of this correction concluded that pre-`status` rows carry **no** evidence of their
outcome, so the historical 655 MB could only come back by re-syncing through a resumed sweep. That
overlooked where the old write path put its evidence. Until ADR-20260728-011344 every sync was a
`RegisterRestaurant` / `MarkRestaurantClosed` **command** sent through the journaling dispatch (#15),
and `command_journal` therefore holds, per submission:

- a **deterministic `message_id`** — UUIDv5 over (command type, SIRET, the staged version's
  `last_seen_at` as read) — which pins the verdict to the **exact version the row still carries**
  (a later refresh would have re-pended it and changed `last_seen_at`);
- a **verdict** (`SUCCEEDED` / `REJECTED` / `FAILED`) plus `completed_at`, written by the dispatch when
  the handler returned — a recorded observation, the same standard `status`/`synced_at` meet.

So the compaction gained a **journal arm**: for a checkpointed pre-`status` row it re-derives the
expected `message_id` and, on a `SUCCEEDED` verdict, transcribes it — `status = 'SYNCED'`,
`synced_at = journal.completed_at`, payload dropped, one statement. `REJECTED`/`FAILED` verdicts, missing
journal rows (drained before #15's journal existed, or an `etat=F` signal that had nothing to close) and
non-checkpointed rows are left untouched and fall back to reclamation-by-re-sync. The derivation is
duplicated into `sirene_ingest` (which cannot see the domain layers, ADR-0045) and pinned to the worker's
by a parity test, `journal_message_id_parity_with_the_compaction`.

Two bounds, stated so nobody discovers them operationally:

- **The evidence expires.** `sweep_retention()` deletes terminal journal rows 90 days after
  `completed_at`, and journaling itself only began 2026-07-20 — run the compaction before the verdicts
  age out; afterwards those rows need the sweep again.
- **`SUCCEEDED` certifies the submission, not every field.** The pre-#227 register path absorbed changed
  records as idempotent no-op replays, so a row's latest INSEE data may not all be in the domain. The
  sync still happened and the payload is still spent — INSEE is the system of record, the hash sentinel
  re-pends every historical row once on the next sweep regardless, and the aggregate then decides on
  fresh data.

Reclaiming the historical 655 MB therefore no longer waits on resuming SIRENE for journal-covered rows;
what the journal cannot confirm is reported as `left_unconfirmed` — a run that reclaimed nothing because
nothing was confirmed must not be mistaken for one that found nothing to do. Those need opposite
responses.

One thing the correction keeps removed: compaction still classifies nothing, so **the ACL gap from
running it in CI stays gone**. Both arms only read verdicts someone else recorded.

### What is unaffected

Detect-by-absence (`last_seen_at`, `etat`), department partitioning and sweep ordering (#218,
`department`/`last_seen_at`), the pending predicate (`processed_at`/`last_seen_at`) and change detection
(`payload_hash`) all read dedicated columns. Nothing but the ACL reads the payload, which is what makes
this safe.

`integration_staging.yaml`'s "NO RETENTION" note is now scoped explicitly to **rows**: detect-by-absence
still needs the complete row set, and `sweep_retention()` still never touches this table. Payloads have
the lifetime above. Conflating the two is the mistake a future reader is most likely to make.
