//! The trigger ENVELOPE, alone in its own module — which is the point (#597).
//!
//! Rust field privacy is a MODULE SUBTREE. `TriggerEnvelope` used to be declared in
//! `process_managers/mod.rs`, so making `lanes` private stopped every OTHER CRATE from writing
//! `lanes: Some(..)` and stopped nobody in `process_managers/**`: every orchestrator there is a
//! descendant of the declaring module and could still write the field directly. That is the same
//! hierarchical-privacy trap this repo documents at PROP-20260802-130500 §1, and the first cut of
//! #597 walked into it while documenting it.
//!
//! Declared HERE, with nothing else in the module, the guarantee is unconditional: no code anywhere
//! — this crate included — can attach a lane sink except through [`TriggerEnvelope::laned`], and
//! `laned` has only AUDITED call sites — the `trigger_envelope_laned_call_sites_are_audited` guard
//! holds it to an allowlist of (file, EXPECTED COUNT, the sentence saying WHICH transaction that
//! file's caller flushes into). The count is not bookkeeping: a SECOND call inside an
//! already-listed file is exactly the edit that puts an enqueue in `prepare` — which
//! `actor_runtime::completion` re-runs with no transaction open — so a name-only allowlist would
//! wave through the one mistake ADR-20260816-040239 constraint 1 names. (#595 added the second
//! file; its review round 1 added the counts, after the first cut shipped file-granular.)

/// The trigger's ENVELOPE bits an event leg may reference (`from_envelope`, ADR-0041): the
/// `domain_events` row's id (dedup keys, `cause_id`), its correlation and its occurrence time.
/// Infrastructure metadata — never business payload.
#[derive(Debug, Clone)]
pub struct TriggerEnvelope {
    /// `domain_events.id` of the trigger — `from_envelope: event_id`; also the `cause_id` stamped on
    /// everything the reaction delivers/sends.
    pub event_id: uuid::Uuid,
    /// `domain_events.correlation_id` of the trigger, propagated onto the reaction's appends.
    pub correlation_id: uuid::Uuid,
    /// `domain_events.occurred_at` of the trigger — `from_envelope: occurred_at`.
    pub occurred_at: chrono::DateTime<chrono::Utc>,
    /// Where a ROUTED `deliver:` step stages its lane enqueue (ADR-20260816-040239). It lives on
    /// the envelope rather than in a leg parameter because it is a property of the INVOCATION
    /// ROUTE, not of the saga: a mailbox delivery owns a fenced transaction to stage into, and so
    /// — since #595 — does the polling `ProcessManagerRunner`, whose `commit_leg` pairs the flush
    /// with the checkpoint advance. A route that owns NO transaction (a unit test, `prepare`) still
    /// cannot build a laned envelope, which is the whole point of the private field.
    ///
    /// `None` — the DEFAULT, and the only value on any route that cannot stage — means NO routed
    /// step on this delivery may stage, whatever the gates say: every one falls back to the legacy
    /// foreign-stream append. It answers *can we stage at all*, and **only that**. Which of the
    /// declared routes is actually ON is `gates` below, read per route by
    /// [`Self::lane_sink_for`] — the two were fused until #797, and the fusion is what made a
    /// rollback of one route a rollback of every route sharing the caller.
    ///
    /// **PRIVATE on purpose (#597)** — ADR-20260816-040239's constraint 1 ("the enqueue is never in
    /// `prepare`") used to hold by structural reading alone, guarded by nothing: any construction
    /// site could write `lanes: Some(...)`, including one in `prepare`, which
    /// `actor_runtime::completion` re-runs with NO transaction open — a birth message stranded
    /// outside the delivery's commit. There is now no field write to make: [`Self::unlaned`] and
    /// [`Self::laned`] are the only ways to build an envelope, and attaching a sink means NAMING
    /// `laned` at a single greppable, documented seam. Compiler first, a check is the fallback
    /// (ADR-20260803-234035).
    lanes: Option<std::sync::Arc<dyn crate::lanes::LaneSink>>,
    /// Where each declared route's gate STANDS on this process (#797) — one field per
    /// [`Route`](crate::generated::process_managers::Route), generated from the DSL's
    /// `route_gate:` declarations.
    ///
    /// It sits next to the sink rather than replacing it because the two answer different
    /// questions and BOTH must be yes: `lanes` answers *can this route stage at all* (does the
    /// caller own a fenced transaction), `gates` answers *should THIS route stage* (is its own
    /// configuration key on). Before #797 only the first was asked, so route selection was
    /// `sink.is_some()` and every route hosted by one runner shared that runner's single boolean —
    /// meaning rolling one route back rolled the others back with it.
    gates: crate::generated::process_managers::RouteGates,
}

/// Hand-written because the lane sink is a trait object with no meaningful identity: two
/// envelopes are the same TRIGGER when their ids and instant match. The sink is delivery-route
/// plumbing, not part of what the envelope IS.
impl PartialEq for TriggerEnvelope {
    fn eq(&self, other: &Self) -> bool {
        self.event_id == other.event_id
            && self.correlation_id == other.correlation_id
            && self.occurred_at == other.occurred_at
    }
}

impl TriggerEnvelope {
    /// The envelope of a trigger delivered on a route with NO lane sink (a gated-OFF route, unit
    /// tests): routed `deliver:`/`sends:` steps fall back to the legacy append.
    ///
    /// The DEFAULT shape, and the only one a route that owns no transaction can build.
    pub fn unlaned(
        event_id: uuid::Uuid,
        correlation_id: uuid::Uuid,
        occurred_at: chrono::DateTime<chrono::Utc>,
    ) -> Self {
        Self {
            event_id,
            correlation_id,
            occurred_at,
            lanes: None,
            gates: crate::generated::process_managers::RouteGates::NONE,
        }
    }

    /// The envelope of a trigger delivered on a route that OWNS THE TRANSACTION the sink's staged
    /// enqueues will be flushed into (`infrastructure::mailbox::handler::handle_pm_fact`, and since
    /// #595 `infrastructure::process_manager::runner`'s `commit_leg`).
    ///
    /// Calling this is a claim about the CALLER, and the claim is exactly ADR-20260816-040239's
    /// constraint 1: *I hold a fenced transaction, and whatever this sink buffers I will flush into
    /// it before I commit.* A phase that cannot make that claim — `prepare`, which
    /// `actor_runtime::completion` re-runs with no transaction and throws away — must call
    /// [`Self::unlaned`]; there is no third option, because the field is private (#597).
    ///
    /// What the compiler carries here is that a sink cannot be attached by an anonymous field
    /// write, so a lane enqueue can only appear on a route that names this constructor. What it
    /// cannot carry is the claim itself: `application` cannot name a Postgres transaction without
    /// inverting the dependency rule, so there is no value for `laned` to demand as proof. The
    /// audited call-site list is the review surface, and this doc is its contract.
    pub fn laned(
        event_id: uuid::Uuid,
        correlation_id: uuid::Uuid,
        occurred_at: chrono::DateTime<chrono::Utc>,
        lanes: std::sync::Arc<dyn crate::lanes::LaneSink>,
        gates: crate::generated::process_managers::RouteGates,
    ) -> Self {
        Self { event_id, correlation_id, occurred_at, lanes: Some(lanes), gates }
    }

    /// The lane sink THIS route stages into, or `None` when the route must take the legacy
    /// append — because the caller owns no transaction to stage into, or because this route's own
    /// gate is off.
    ///
    /// **There is deliberately no accessor that ignores the route** (#797). A bare
    /// `lane_sink()` made the route-selection predicate `sink.is_some()`: every routed step on a
    /// given delivery route read the same boolean, so adding a second route to a runner silently
    /// bound it to the first route's flag, and turning one off turned the other off too. Rolling
    /// back a route you did not intend to change is not a rollback (farley), and the gate names
    /// WHICH AGGREGATE BOUNDARY is being closed, so sharing one boolean shares a fence between
    /// unrelated aggregates (vernon). Taking a [`Route`](crate::generated::process_managers::Route)
    /// makes staging-without-naming-your-route unspellable rather than merely discouraged
    /// (compiler first, ADR-20260803-234035).
    ///
    /// Crate-internal: the generated step pipeline and the hand-written wrapper seams are the only
    /// readers, and the only writers are the two constructors above.
    pub(crate) fn lane_sink_for(
        &self,
        route: crate::generated::process_managers::Route,
    ) -> Option<&std::sync::Arc<dyn crate::lanes::LaneSink>> {
        self.lanes.as_ref().filter(|_| self.gates.enabled(route))
    }
}
