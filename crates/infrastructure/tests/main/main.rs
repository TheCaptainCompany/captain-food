//! The ONE integration-test binary for `infrastructure` (#335, ADR-20260808-224500 item 5).
//!
//! Why one binary: 27 separate integration files each linked the full infrastructure graph —
//! ~1.4 GB of link products per build state and 27 sequential link steps per test run. The merge
//! keeps every suite as its own module below, so `cargo test -p infrastructure --test main
//! <module>::` still runs one suite.
//!
//! Why it stays correct: cargo used to run the 27 binaries one at a time, which was the ONLY
//! cross-suite isolation — a mechanical merge would have deleted exactly that (dba veto in the
//! ADR). Serialization is instead COMPILER-ENFORCED here: the only way any test gets a database
//! pool is [`common::TestDb::acquire`], whose constructor holds a binary-wide async mutex for the
//! life of the returned witness and resets the schema from the REAL migration chain
//! (`migrations/*.sql` via `include_str!`, one shared `reset_schema` replacing the ~20 divergent
//! hand-copied DDL blocks the old files carried). An unlocked DB test is unspellable: there is no
//! other pool constructor in this binary — do not add one. The witness serializes test BODIES on
//! its own, but `--test-threads=1` stays LOAD-BEARING locally too: tests that `tokio::spawn`
//! long-running workers (mailbox_wake, mailbox_requeue, standalone_workers) only cancel them when
//! the per-test runtime drops AFTER the gate is released, so at higher parallelism a prior test's
//! worker can touch the database in that tail window while the next test resets the schema.

mod common;

mod actors_projector_batching;
mod cart_projection;
mod catalog_projection;
mod customer_projection;
mod deletion_engine;
mod delivery_read_model;
mod event_wake;
mod mailbox_activations;
mod mailbox_delivery;
mod mailbox_requeue;
mod mailbox_retention;
mod mailbox_schedule_pg;
mod mailbox_wake;
mod order_projection;
mod pending_refund_read_model;
mod pm_prepare_delivery;
mod projection_batching;
mod projection_checkpoint_halt;
mod prospection_projection;
mod referential_policies;
mod restaurant_locations_by_account;
mod restaurant_projection;
mod restaurant_write_path;
mod retention_sweep;
mod runtime_posture;
mod sms_send_quota;
mod scope_membership;
mod sirene_registration;
mod standalone_workers;
mod sync_sirene_worker;
