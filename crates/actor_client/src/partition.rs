//! The FROZEN partition routing function (PROP-20260728-152752 §2, ADR-20260730-234918 D2.1:
//! "the routing function is a frozen contract"). `partition = FNV-1a-64(actor_id bytes) mod width`,
//! stamped on every insert and relied on by every drain — CHANGING THIS STRANDS IN-FLIGHT ROWS
//! (they would sit on lanes no consumer maps to the actor anymore). Never replace it with a
//! std/default hasher: Rust's SipHash is randomly keyed per process, which is exactly the
//! non-determinism this function exists to exclude. The golden-value test below is the freeze.

/// FNV-1a 64-bit over the uuid's 16 big-endian bytes, reduced mod `width`. The result feeds a
/// SMALLINT column, so `width` is clamped to `1..=i16::MAX` — an unclamped `width > 32767` would
/// silently wrap negative in the cast and strand the row on a partition no lane maps to.
pub fn stable_partition(actor_id: &uuid::Uuid, width: u16) -> i16 {
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
/// It exists to make a whole class of defect unspellable rather than merely discouraged
/// (ADR-20260803-234035, compiler-first). Callers pass no `width`, so no call site can decide
/// where a width comes from — and the wrong source was a real, shipped, money-path defect:
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

    #[test]
    fn stays_in_range() {
        for n in 0..64u128 {
            let id = uuid::Uuid::from_u128(n * 7919);
            let p = stable_partition(&id, 100);
            assert!((0..100).contains(&p));
        }
    }
}
