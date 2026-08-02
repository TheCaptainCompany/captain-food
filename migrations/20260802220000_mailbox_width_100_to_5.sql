-- Mailbox keyspace width 100 -> 5 (ADR-20260802-220000, amends PROP-20260728-152752 §2).
--
-- WHY: every owned lane costs one SELECT per worker pass even when empty, and the workers are
-- un-gated -- 16 actor types x 100 lanes = ~1,600 idle queries per 10s heartbeat (~580k/hour),
-- 8x the domain_events polling removed by
-- PR #301 "feat(#300): push the drain loops from Postgres NOTIFY instead of polling every 1.5s".
-- Width 5 keeps per-actor ordering and 5-way delivery parallelism per actor type (far beyond
-- Tours-scale V0 needs) at 1/20th the idle cost.
--
-- WHY THE REMAP IS EXACT: partition = fnv1a64(actor_id) mod width (actor_client::stable_partition,
-- frozen by golden-value test). Because 5 divides 100, (hash mod 100) mod 5 == hash mod 5 --
-- so `partition % 5` recomputes the width-5 stamp WITHOUT re-hashing in SQL. A width change to a
-- non-divisor would need the real hash and a different migration.
--
-- ORDER OF OPERATIONS: rows first, registry second. Between the two statements a width-100 drain
-- would find nothing on lanes >= 5 (harmless); the reverse order would let a drain deliver from a
-- deleted lane's rows before they are remapped. Any row inserted by a still-running width-100
-- writer AFTER this migration lands on a lane >= 5 with no registry row and is stranded until the
-- next deploy re-runs the remap -- the deploy sequence (migrate, then restart the single monolith
-- instance) makes that window the deploy overlap only, and re-running both statements is
-- idempotent.

-- All rows, not just RECEIVED/SCHEDULED: terminal rows keep their partition only as provenance,
-- but a consistent stamp keeps the retention sweep and any per-lane diagnostics honest.
UPDATE inbound_messages SET partition = partition % 5 WHERE partition >= 5;

-- Registry rows >= 5 disappear; workers stop claiming (and paying for) those lanes. Checkpoints on
-- the surviving 5 lanes are NOT recomputed: the drain filters on status = 'RECEIVED' alone (never
-- position > checkpoint), so a checkpoint lower than a remapped row's position cannot skip it.
DELETE FROM mailbox_partitions WHERE partition >= 5;
