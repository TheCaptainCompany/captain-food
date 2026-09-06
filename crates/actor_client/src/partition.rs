//! The FROZEN partition routing function (PROP-20260728-152752 §2, ADR-20260730-234918 D2.1:
//! "the routing function is a frozen contract"). `partition = FNV-1a-64(actor_id bytes) mod width`,
//! stamped on every insert and relied on by every drain — CHANGING THIS STRANDS IN-FLIGHT ROWS
//! (they would sit on lanes no consumer maps to the actor anymore). Never replace it with a
//! std/default hasher: Rust's SipHash is randomly keyed per process, which is exactly the
//! non-determinism this function exists to exclude. The golden-value test below is the freeze.

/// FNV-1a 64-bit over the uuid's 16 big-endian bytes, reduced mod `width`. The result feeds a
/// SMALLINT column, so `width` is clamped to `1..=i16::MAX` — an unclamped `width > 32767` would
/// silently wrap negative in the cast and strand the row on a partition no lane maps to.
///
/// **PRIVATE to this module since #609**, and that is the whole point: [`declared_lane`] is the
/// only way to obtain a lane, so no caller anywhere — production or test — can supply a `width`
/// and hold a second opinion about the keyspace. Not a convention, not a gate: there is no path.
fn stable_partition(actor_id: &uuid::Uuid, width: u16) -> i16 {
    const OFFSET: u64 = 0xcbf29ce484222325;
    const PRIME: u64 = 0x00000100000001b3;
    let mut hash = OFFSET;
    for byte in actor_id.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(PRIME);
    }
    (hash % u64::from(width.clamp(1, i16::MAX as u16))) as i16
}

/// **The one accessor every routing site uses** (#596): the DECLARED lane for an aggregate.
/// `None` when the actor type declares no mailbox at all — a wiring bug, never a business outcome,
/// and each caller names it in its own terms.
///
/// It exists to take a decision away from call sites (ADR-20260803-234035, compiler-first):
/// callers pass no `width`, so no routing site can decide where a width comes from. Since #596's
/// review that is true of every routing site — the typed door, the entry constructors, the
/// reminder scheduler and the generated clients all lost their `width` argument.
///
/// **Since #609 the possibility is gone too, not only the parameter.** `stable_partition` is
/// private to this module, so `stable_partition(id, some_width)` is not spellable anywhere outside
/// it — no `pub`, no feature gate, no export, and therefore nothing to enforce. #596's review
/// caught the earlier version of this doc over-claiming exactly that, which is why the claim is
/// stated flatly only now that the compiler carries it: the tests that used to compute an expected
/// lane from a hand-copied width call [`declared_lane`] instead, so a fixture and a producer cannot
/// disagree about the keyspace even in test code.
///
/// The wrong source was a real, shipped, money-path defect:
/// `chain_pm_copy_in_tx` derived the keyspace from `SELECT count(*) FROM mailbox_partitions`, a
/// RUNTIME artifact written only by the workers at startup, while every other producer used the
/// DECLARED contract below.
///
/// **Why the declaration is the only admissible source.** The keyspace width is not configuration,
/// it is an ADDRESSING contract, and it is the sole thing that maps an aggregate to exactly ONE
/// lane. The lease and the completion fence are both keyed by *lane*, so two producers disagreeing
/// on the width put one aggregate in two lanes, each with a live lease, each passing its own
/// fence: one-writer is broken at the addressing function, upstream of anything a fence can
/// observe. A source that can be empty, stale or partial cannot carry that.
pub fn declared_lane(actor_type: &str, actor_id: &uuid::Uuid) -> Option<i16> {
    let (_, width) = crate::generated::addresses::ACTOR_MAILBOXES
        .iter()
        .find(|(a, _)| *a == actor_type)?;
    Some(stable_partition(actor_id, *width))
}

#[cfg(test)]
mod tests {
    use super::{declared_lane, stable_partition};

    /// The accessor and the frozen function agree for a declared actor, and an undeclared actor
    /// is `None` rather than a silent default lane 0 — a "wiring bug" that routed everything to
    /// one lane would look like a working system under any test with a single aggregate.
    ///
    /// `MailboxSupervision` is the load-bearing case: it is the ONLY actor whose declared width is
    /// not 5, so it is the only thing in the repository that can distinguish "reads the
    /// declaration" from "happens to say 5" (mutation 3 of #596's kill set).
    #[test]
    fn declared_lane_reads_the_declaration_and_refuses_the_undeclared() {
        let id = uuid::Uuid::from_u128(0x0AD1);
        assert_eq!(declared_lane("PlaceOrderProcess", &id), Some(stable_partition(&id, 5)));
        assert_eq!(declared_lane("MailboxSupervision", &id), Some(0), "width 1 => one lane");
        assert_eq!(declared_lane("NotAnActor", &id), None);
    }

    /// THE FREEZE: these values may never change. A failure here means the routing function
    /// drifted — which strands every in-flight mailbox row. Fix the code, never the constants.
    #[test]
    fn golden_values_are_frozen() {
        let nil = uuid::Uuid::nil();
        let a = uuid::Uuid::from_u128(1);
        let b = uuid::Uuid::parse_str("5b1e0000-0000-4000-8000-00000000c421").unwrap();
        assert_eq!(stable_partition(&nil, 100), 21);
        assert_eq!(stable_partition(&a, 100), 10);
        assert_eq!(stable_partition(&b, 100), 63);
        assert_eq!(stable_partition(&nil, 1), 0);
    }

    /// **A declared width is a MIGRATION, not a setting** — ADR-20260802-220402 had to remap every
    /// non-terminal in-flight row when the width went 100 -> 5, and only because 5 divides 100.
    /// Narrowing or widening one re-lanes every live aggregate of that actor.
    ///
    /// Until #609 that contract was pinned only INCIDENTALLY, by four integration assertions that
    /// compared a production stamp against a hand-copied literal 5 (`graphql_typed_send` for Cart,
    /// `drift_guard` for Order, two in `pm_prepare_delivery`). Converting those to
    /// [`declared_lane`] makes both sides read the declaration, so they can no longer notice the
    /// declaration moving — and three of the four only ran with a Postgres attached. The pin moves
    /// HERE, where it is deliberate, names the reason, and fires on every `cargo test`.
    ///
    /// It pins the SHAPE, not the roster: a new actor taking the standard width adds nothing to
    /// maintain, while changing an existing width — or introducing a second exception — stops the
    /// build and asks for the migration story.
    ///
    /// **Anti-blindness, named specifically** (same idiom as
    /// `test_fixtures_feature_never_reaches_a_release_artifact` and the DOORS guard's AST seed in
    /// `tools/codegen-rs/src/tests.rs`). A loop over a generated slice has two ways to pass by
    /// scanning nothing, and both are reachable from an ordinary `specs/*/actors.yaml` edit:
    /// an EMPTY slice iterates zero times, and losing the one non-5 actor turns the whole body
    /// into `5 == 5` sixteen times over. The two assertions before the loop are the floor and the
    /// seed; neither costs anything when a new standard-width actor is declared.
    #[test]
    fn every_declared_width_is_the_standard_one_because_changing_one_is_a_migration() {
        let declared = crate::generated::addresses::ACTOR_MAILBOXES;

        // FLOOR, not equality: a new actor is free, an actor DISAPPEARING is not. An empty or
        // truncated slice makes the loop below a silent no-op wearing a loop.
        assert!(
            declared.len() >= 17,
            "ACTOR_MAILBOXES is down to {} entries from 17. Removing a mailbox actor is a real \
             decision (its in-flight rows have nowhere to drain), so raise this floor deliberately \
             in the same change — a shrinking slice must never quietly shrink what this test reads",
            declared.len()
        );

        // SEED: the ONE non-5 width in the repository, and therefore the only case in it that can
        // distinguish "reads the declaration" from "happens to say 5" (mutation 3 of #596's kill
        // set). Without this the loop still passes over sixteen identical fives and the load-
        // bearing case is gone with no test noticing.
        assert!(
            declared.iter().any(|(a, w)| *a == "MailboxSupervision" && *w == 1),
            "no actor named 'MailboxSupervision' declares width 1 any more. If it was RENAMED that \
             is fine — update this seed to the new name. If its width became 5, that is a lane \
             MIGRATION and belongs above. If the actor was REMOVED, the repository has lost its \
             only non-standard width and the loop below can no longer tell a declaration read from \
             a hard-coded 5: replace this seed with another exception, do not delete the test"
        );

        for (actor_type, width) in declared {
            let expected: u16 = match *actor_type {
                // Operator actions are rare and human-paced (specs/common/actors.yaml).
                "MailboxSupervision" => 1,
                // Platform grants are rare and human-paced too, population 1-3 admins (#639 part
                // C step 6-v, ADR-20260905-223957 §1, specs/common/actors.yaml).
                "PlatformMembership" => 1,
                _ => 5,
            };
            assert_eq!(
                *width, expected,
                "'{actor_type}' declares width {width}, not {expected}: changing a declared width \
                 re-lanes every in-flight row of that actor, so it needs a migration \
                 (ADR-20260802-220402) — not just an edit to actors.yaml and this line"
            );
        }
    }

    #[test]
    fn stays_in_range() {
        for n in 0..64u128 {
            let id = uuid::Uuid::from_u128(n * 7919);
            let p = stable_partition(&id, 100);
            assert!((0..100).contains(&p));
        }
    }
}
