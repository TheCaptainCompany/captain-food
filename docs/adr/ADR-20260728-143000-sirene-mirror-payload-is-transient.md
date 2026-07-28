# ADR-20260728-143000 — The SIRENE mirror's payload is transient; the hash is what persists

- **Status**: Accepted
- **Date**: 2026-07-28
- **Issue**: [#231 "The SIRENE mirror stores verbatim INSEE payloads (~1.8 kB/row) to read 5 fields — it is 77% of the database and blocks national coverage"](https://github.com/TheCaptainCompany/captain-food/issues/231)
- **Proposal**: [PROP-20260728-120931](../proposals/PROP-20260728-120931-sirene-mirror-payload-is-transient.md)
- **Refines**: [ADR-0045](0045-sirene-staging-table-and-split-sync.md) (staging-table retention), [ADR-20260728-011344](ADR-20260728-011344-slug-lifecycle-and-sirene-inbound-events.md) (`payload_hash`)

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
2. The worker NULLs the payload in the **same statement** that advances the `processed_at` checkpoint,
   so a row can never be marked processed while still holding one, or stripped without being marked.
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
| `SYNCED` | translated into a domain fact | dropped — spent |
| `UNMAPPABLE` | ACL-rejected or unparsable | kept — it is the evidence |
| `FAILED` | last attempt failed; row stays pending and retries | present |

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

### What is unaffected

Detect-by-absence (`last_seen_at`, `etat`), department partitioning and sweep ordering (#218,
`department`/`last_seen_at`), the pending predicate (`processed_at`/`last_seen_at`) and change detection
(`payload_hash`) all read dedicated columns. Nothing but the ACL reads the payload, which is what makes
this safe.

`integration_staging.yaml`'s "NO RETENTION" note is now scoped explicitly to **rows**: detect-by-absence
still needs the complete row set, and `sweep_retention()` still never touches this table. Payloads have
the lifetime above. Conflating the two is the mistake a future reader is most likely to make.
