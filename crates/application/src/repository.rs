//! The write-side `Repository` — an event-sourced aggregate's journal.
//!
//! A thin layer over the low-level [`EventStore`] journal that gives every aggregate ("actor",
//! `specs/actors.yaml`) a load/save API keyed by its typed id, so command handlers and the saga runner
//! stop re-deriving stream names and calling `fold`/`append` by hand. The aggregate OWNS emission (its pure
//! `fold` + decide); the **repository owns persistence**; the `EventStore`/`PgEventStore` is the adapter
//! behind it.
//!
//! Loads are always the aggregate's OWN write-side stream (never an eventually-consistent read model), so a
//! write decision sees authoritative state **and** the stream version for its optimistic-concurrency append.

use domain::aggregate::Aggregate;
use domain::generated::events::DomainEvent;
use domain::shared::errors::DomainError;

use crate::ports::{is_version_conflict, Actor, EventStore};

/// A write-side repository over the [`EventStore`] journal. Cheap to build per unit of work
/// (`Repository::new(store)`) — it borrows the journal, adds no state.
pub struct Repository<'a> {
    journal: &'a dyn EventStore,
}

impl<'a> Repository<'a> {
    pub fn new(journal: &'a dyn EventStore) -> Self {
        Self { journal }
    }

    /// Rehydrate aggregate `A` for `id`: fold its stream into the minimal write-side state and return it
    /// with the stream's current version (the expected version for the next append). `None` = the
    /// aggregate does not exist yet.
    pub async fn load<A: Aggregate>(&self, id: A::Id) -> Result<(Option<A>, i64), DomainError> {
        let (events, version) = self.journal.load(&A::stream(id)).await?;
        Ok((A::fold(&events), version))
    }

    /// The raw event slice + version of aggregate `A`'s stream. For the few decisions that inspect the
    /// events the folded state does NOT capture — a process-manager reacting over a target stream, or the
    /// test-mode check scanning for `RestaurantRegistered { mode: TEST }`. Prefer [`Self::load`]/[`Self::require`]
    /// when the folded state suffices.
    pub async fn events<A: Aggregate>(
        &self,
        id: A::Id,
    ) -> Result<(Vec<DomainEvent>, i64), DomainError> {
        self.journal.load(&A::stream(id)).await
    }

    /// Rehydrate and require existence, or reject with the aggregate's not-found error (built by `nf`).
    pub async fn require<A: Aggregate>(
        &self,
        id: A::Id,
        nf: impl FnOnce() -> DomainError,
    ) -> Result<(A, i64), DomainError> {
        let (state, version) = self.load::<A>(id).await?;
        state.map(|s| (s, version)).ok_or_else(nf)
    }

    /// Persist the events a decision produced onto `stream` at `expected_version` (optimistic concurrency;
    /// a clash surfaces as [`crate::ports::version_conflict`]). Returns the stream's new version. Used both
    /// by aggregate command handlers (their own stream) and by the saga runner (each `StreamAppend`).
    pub async fn save(
        &self,
        stream: &str,
        expected_version: i64,
        events: &[DomainEvent],
        actor: &Actor,
    ) -> Result<i64, DomainError> {
        self.journal.append(stream, expected_version, events, actor).await
    }

    /// Birth a new aggregate stream (`expected_version = 0`), absorbing the optimistic-concurrency clash of
    /// a REPLAYED creation command as success — the aggregate already exists under this client-generated id.
    pub async fn create(
        &self,
        stream: &str,
        events: &[DomainEvent],
        actor: &Actor,
    ) -> Result<(), DomainError> {
        match self.journal.append(stream, 0, events, actor).await {
            Ok(_) => Ok(()),
            Err(e) if is_version_conflict(&e) => Ok(()),
            Err(e) => Err(e),
        }
    }
}
