//! ACTIVATIONS — the held-state cache (PROP-20260728-152752 §3.5): an actor, once rehydrated,
//! stays in memory and subsequent deliveries reuse it instead of re-folding from the log.
//! Fold-on-every-message becomes fold-on-first-message.
//!
//! This is the runtime's GENERIC half: an LRU-bounded, idle-expiring dictionary keyed by the
//! host's own stream/state key, holding an opaque value. The host owns what is cached (its fold),
//! WHEN an entry is promoted (apply-after-commit — the §3.5 invariant: *the state the next
//! message sees must equal the fold of COMMITTED events*, so staged work never touches the cache
//! until the fenced transaction commits), and the version discipline that makes promotion safe.
//!
//! **Correctness never depends on this cache.** Every eviction rule exists to keep it USEFUL,
//! not correct — a stale entry's flush loses the optimistic-concurrency race
//! (`UNIQUE(stream_name, version)`), the delivery aborts, the host invalidates and the retry
//! refolds. The rules (§3.5 "cache discipline"):
//!
//! - **version conflict on append** → the host invalidates (the one signal that is never wrong);
//! - **lane lost** (steal, re-claim, shutdown release) → [`ActivationCache::drop_partition`],
//!   wired through the worker's [`crate::worker::LaneEvents`] hook — a lane that changed hands
//!   may be written by its new owner, so every held state under it is suspect;
//! - **idle timeout** → passivation (the Proto.Actor `ReceiveTimeout` pattern): an entry
//!   untouched for its idle window is dropped on the next access or [`ActivationCache::sweep`];
//! - **memory bound** → least-recently-used eviction whenever the byte total crosses the cap.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::{Duration, Instant};

struct Entry<V> {
    value: Arc<V>,
    /// The owning lane, for [`ActivationCache::drop_partition`].
    actor_type: String,
    partition: i16,
    /// Host-estimated size (feeds the LRU byte bound).
    bytes: usize,
    /// Sliding idle window: refreshed on every touch; past it the entry is passivated.
    idle: Duration,
    expires_at: Instant,
    /// LRU ordinal (a monotonic tick, not a clock — immune to timer granularity).
    last_used: u64,
}

struct Inner<V> {
    entries: HashMap<String, Entry<V>>,
    total_bytes: usize,
    tick: u64,
    /// Bumped on every INVALIDATION signal (`invalidate`, `drop_partition`) — the fence for
    /// fill-vs-invalidate races: a fill that started before an invalidation must not install
    /// its (possibly pre-invalidation) snapshot after it. Idle/LRU evictions do NOT bump it —
    /// they signal memory pressure, not staleness.
    epoch: u64,
}

/// The activation dictionary. One instance is shared by every worker of a process (entries are
/// tagged with their lane, so a lane-scoped drop never touches a peer's holdings).
pub struct ActivationCache<V> {
    inner: Mutex<Inner<V>>,
    max_bytes: usize,
}

impl<V> ActivationCache<V> {
    /// `max_bytes` bounds the byte total (host-estimated per entry); crossing it evicts
    /// least-recently-used entries until back under.
    pub fn new(max_bytes: usize) -> Self {
        Self {
            inner: Mutex::new(Inner { entries: HashMap::new(), total_bytes: 0, tick: 0, epoch: 0 }),
            max_bytes,
        }
    }

    /// The current invalidation epoch — capture BEFORE reading the backing store, hand back to
    /// [`Self::put_at_epoch`] so a fill cannot re-install a snapshot past an invalidation that
    /// raced it (the TOCTOU fence).
    pub fn epoch(&self) -> u64 {
        self.inner.lock().unwrap().epoch
    }

    /// Look up a held state. An expired entry is passivated on the spot (miss); a live one is
    /// touched (LRU + sliding idle refresh).
    pub fn get(&self, key: &str) -> Option<Arc<V>> {
        let mut inner = self.inner.lock().unwrap();
        inner.tick += 1;
        let tick = inner.tick;
        let now = Instant::now();
        let expired = match inner.entries.get_mut(key) {
            None => return None,
            Some(e) if e.expires_at <= now => true,
            Some(e) => {
                e.last_used = tick;
                e.expires_at = now + e.idle;
                return Some(e.value.clone());
            }
        };
        if expired {
            if let Some(e) = inner.entries.remove(key) {
                inner.total_bytes -= e.bytes;
            }
        }
        None
    }

    /// [`Self::put`] fenced against invalidations: installs only if no `invalidate`/
    /// `drop_partition` happened since `at_epoch` was captured (see [`Self::epoch`]). Returns
    /// whether the value was installed. Fills MUST use this — a plain `put` of data read before
    /// a racing invalidation would resurrect exactly the staleness the invalidation announced.
    pub fn put_at_epoch(
        &self,
        key: &str,
        actor_type: &str,
        partition: i16,
        idle: Duration,
        bytes: usize,
        value: Arc<V>,
        at_epoch: u64,
    ) -> bool {
        let mut inner = self.inner.lock().unwrap();
        if inner.epoch != at_epoch {
            return false;
        }
        Self::put_locked(&mut inner, self.max_bytes, key, actor_type, partition, idle, bytes, value);
        true
    }

    /// Insert or replace a held state (the host calls this on fill AND on apply-after-commit
    /// promotion — promotion is a replace). Evicts LRU entries if the byte bound is crossed;
    /// a single value larger than the whole bound is inserted and immediately evicted, i.e.
    /// effectively not cached.
    pub fn put(
        &self,
        key: &str,
        actor_type: &str,
        partition: i16,
        idle: Duration,
        bytes: usize,
        value: Arc<V>,
    ) {
        let mut inner = self.inner.lock().unwrap();
        Self::put_locked(&mut inner, self.max_bytes, key, actor_type, partition, idle, bytes, value);
    }

    #[allow(clippy::too_many_arguments)]
    fn put_locked(
        inner: &mut Inner<V>,
        max_bytes: usize,
        key: &str,
        actor_type: &str,
        partition: i16,
        idle: Duration,
        bytes: usize,
        value: Arc<V>,
    ) {
        inner.tick += 1;
        let tick = inner.tick;
        if let Some(old) = inner.entries.remove(key) {
            inner.total_bytes -= old.bytes;
        }
        inner.total_bytes += bytes;
        inner.entries.insert(
            key.to_owned(),
            Entry {
                value,
                actor_type: actor_type.to_owned(),
                partition,
                bytes,
                idle,
                expires_at: Instant::now() + idle,
                last_used: tick,
            },
        );
        while inner.total_bytes > max_bytes && !inner.entries.is_empty() {
            let lru_key = inner
                .entries
                .iter()
                .min_by_key(|(_, e)| e.last_used)
                .map(|(k, _)| k.clone())
                .expect("non-empty");
            if let Some(e) = inner.entries.remove(&lru_key) {
                inner.total_bytes -= e.bytes;
            }
        }
    }

    /// Drop one key — the version-conflict reaction (the host saw its flush lose the race, so
    /// the held state is provably stale), and the cross-writer reaction (a delivery committed
    /// appends to a stream some OTHER lane may be holding).
    pub fn invalidate(&self, key: &str) {
        let mut inner = self.inner.lock().unwrap();
        inner.epoch += 1;
        if let Some(e) = inner.entries.remove(key) {
            inner.total_bytes -= e.bytes;
        }
    }

    /// Drop every entry held under one lane — the lane-lost reaction (steal, re-claim, shutdown
    /// release): the new owner may write those actors, so nothing held under the lane can be
    /// trusted to stay equal to the committed fold.
    pub fn drop_partition(&self, actor_type: &str, partition: i16) {
        let mut inner = self.inner.lock().unwrap();
        inner.epoch += 1;
        let doomed: Vec<String> = inner
            .entries
            .iter()
            .filter(|(_, e)| e.actor_type == actor_type && e.partition == partition)
            .map(|(k, _)| k.clone())
            .collect();
        for k in doomed {
            if let Some(e) = inner.entries.remove(&k) {
                inner.total_bytes -= e.bytes;
            }
        }
    }

    /// Passivate every expired entry (idle actors must leave memory ON SCHEDULE, not only when
    /// touched — the host runs this on a timer). Returns how many were dropped.
    pub fn sweep(&self) -> usize {
        let mut inner = self.inner.lock().unwrap();
        let now = Instant::now();
        let doomed: Vec<String> = inner
            .entries
            .iter()
            .filter(|(_, e)| e.expires_at <= now)
            .map(|(k, _)| k.clone())
            .collect();
        let n = doomed.len();
        for k in doomed {
            if let Some(e) = inner.entries.remove(&k) {
                inner.total_bytes -= e.bytes;
            }
        }
        n
    }

    /// Held entry count (supervision/tests).
    pub fn len(&self) -> usize {
        self.inner.lock().unwrap().entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Held byte total (supervision/tests).
    pub fn bytes(&self) -> usize {
        self.inner.lock().unwrap().total_bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cache(max: usize) -> ActivationCache<i64> {
        ActivationCache::new(max)
    }

    const IDLE: Duration = Duration::from_secs(60);

    #[test]
    fn put_get_promote_and_invalidate() {
        let c = cache(1024);
        c.put("Order-1", "Order", 3, IDLE, 100, Arc::new(1));
        assert_eq!(c.get("Order-1").as_deref(), Some(&1));
        // Promotion is a replace under the same key.
        c.put("Order-1", "Order", 3, IDLE, 120, Arc::new(2));
        assert_eq!(c.get("Order-1").as_deref(), Some(&2));
        assert_eq!(c.bytes(), 120);
        c.invalidate("Order-1");
        assert!(c.get("Order-1").is_none());
        assert_eq!(c.bytes(), 0);
    }

    #[test]
    fn lru_bound_evicts_least_recently_used() {
        let c = cache(250);
        c.put("a", "Order", 0, IDLE, 100, Arc::new(1));
        c.put("b", "Order", 1, IDLE, 100, Arc::new(2));
        // Touch `a` so `b` is the LRU when the bound is crossed.
        assert!(c.get("a").is_some());
        c.put("c", "Order", 2, IDLE, 100, Arc::new(3));
        assert!(c.get("a").is_some(), "recently used survives");
        assert!(c.get("b").is_none(), "LRU evicted at the byte bound");
        assert!(c.get("c").is_some());
        assert!(c.bytes() <= 250);
    }

    #[test]
    fn idle_expiry_passivates_on_get_and_sweep() {
        let c = cache(1024);
        c.put("a", "Order", 0, Duration::from_millis(0), 10, Arc::new(1));
        c.put("b", "Order", 0, Duration::from_secs(60), 10, Arc::new(2));
        std::thread::sleep(Duration::from_millis(5));
        assert!(c.get("a").is_none(), "expired entry passivated on access");
        assert_eq!(c.sweep(), 0, "already passivated");
        c.put("a", "Order", 0, Duration::from_millis(0), 10, Arc::new(1));
        std::thread::sleep(Duration::from_millis(5));
        assert_eq!(c.sweep(), 1, "sweep passivates without an access");
        assert!(c.get("b").is_some(), "live entry untouched");
    }

    /// The TOCTOU fence: a fill whose read started before a racing invalidation must not
    /// install its snapshot after it (the invalidation announced staleness the fill's data may
    /// predate). Idle/LRU evictions are memory pressure, not staleness — they don't fence.
    #[test]
    fn a_fill_never_lands_past_a_racing_invalidation() {
        let c = cache(1024);
        let before = c.epoch();
        c.invalidate("Order-1"); // the racing invalidation (entry absent — still a signal)
        assert!(
            !c.put_at_epoch("Order-1", "Order", 3, IDLE, 10, Arc::new(1), before),
            "a pre-invalidation snapshot must be refused"
        );
        assert!(c.get("Order-1").is_none());
        let now = c.epoch();
        assert!(c.put_at_epoch("Order-1", "Order", 3, IDLE, 10, Arc::new(2), now));
        assert_eq!(c.get("Order-1").as_deref(), Some(&2));
    }

    #[test]
    fn drop_partition_only_touches_the_lane() {
        let c = cache(1024);
        c.put("Order-1", "Order", 3, IDLE, 10, Arc::new(1));
        c.put("Order-2", "Order", 4, IDLE, 10, Arc::new(2));
        c.put("Cart-1", "Cart", 3, IDLE, 10, Arc::new(3));
        c.drop_partition("Order", 3);
        assert!(c.get("Order-1").is_none(), "the lane's holdings dropped");
        assert!(c.get("Order-2").is_some(), "same type, other partition survives");
        assert!(c.get("Cart-1").is_some(), "same partition ordinal, other type survives");
    }
}
