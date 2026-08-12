//! The write-path journal's shared payload identity (ADR-20260731-122500 "the mailbox is the only
//! door"). Since #242 Runtime D there is exactly ONE journal — `inbound_messages` — and its store
//! trait lives with the actor client, not here: what remains in this module is the one function
//! every acceptance door must agree on, [`payload_hash`].
//!
//! What used to live here: the `CommandJournal` port + `CommandJournalEntry`/`Row`/
//! `JournalInsertOutcome` over the `command_journal` table, and the `inbound_events` staging port.
//! Both tables are dropped; keeping their ports would keep a second door spellable.

/// Canonical payload hash: sha256 hex over the serde_json serialization. The SAME function must be
/// used by every acceptance door (the GraphQL resolvers, the typed actor clients, the adapter ACL
/// enqueues) so duplicate detection never depends on key ordering differences (serde_json preserves
/// struct-declaration order for our generated commands).
pub fn payload_hash(payload: &serde_json::Value) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(payload.to_string().as_bytes());
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn payload_hash_discriminates_content_not_instance() {
        let a = payload_hash(&json!({ "x": 1 }));
        let b = payload_hash(&json!({ "x": 1 }));
        let c = payload_hash(&json!({ "x": 2 }));
        assert_eq!(a, b);
        assert_ne!(a, c);
    }
}
