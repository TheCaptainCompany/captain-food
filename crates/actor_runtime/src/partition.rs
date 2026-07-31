//! The FROZEN partition routing function (PROP-20260728-152752 §2, ADR-20260730-234918 D2.1:
//! "the routing function is a frozen contract"). `partition = FNV-1a-64(actor_id bytes) mod width`,
//! stamped on every insert and relied on by every drain — CHANGING THIS STRANDS IN-FLIGHT ROWS
//! (they would sit on lanes no consumer maps to the actor anymore). Never replace it with a
//! std/default hasher: Rust's SipHash is randomly keyed per process, which is exactly the
//! non-determinism this function exists to exclude. The golden-value test below is the freeze.

/// FNV-1a 64-bit over the uuid's 16 big-endian bytes, reduced mod `width`.
pub fn stable_partition(actor_id: &uuid::Uuid, width: u16) -> i16 {
    const OFFSET: u64 = 0xcbf29ce484222325;
    const PRIME: u64 = 0x00000100000001b3;
    let mut hash = OFFSET;
    for byte in actor_id.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(PRIME);
    }
    (hash % u64::from(width.max(1))) as i16
}

#[cfg(test)]
mod tests {
    use super::stable_partition;

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
