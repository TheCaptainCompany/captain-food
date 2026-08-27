//! `queries/pendingRefunds` bound to the caller's `ReadScope` (#618 chunk 1, the money-path read
//! surface of DECISIONS §39 IDOR-1). Executed against the schema directly with a
//! `RefundReadRepository` injected as request data — no database anywhere, the shape
//! `graphql_payment_status.rs` uses.
//!
//! **The rule under test is INTERSECTION, never substitution.** `restaurantId` is a selector
//! WITHIN the caller's scope, never a grant: no argument returns the caller's own queue, the
//! caller's own id returns the same queue, and any OTHER restaurant's id returns an EMPTY list
//! plus a mismatch count. A bound caller asking for someone else must never be silently served
//! its own rows — that would make the schema lie (HTTP 200, wrong rows, no error, no way to tell).
//!
//! **The antecedent for the hardcoded `ReadScope::Public` on the unbound arm** is an assertion
//! that already exists: `crates/server/src/auth.rs:1910` —
//! `assert_eq!(read_scope(&principal(RequestRole::Restaurant, "s", None)), ReadScope::Public)`.
//! That line carries a message naming this suite as its dependent. `read_scope` cannot be called
//! from here (`mod auth` is private and every `Identity` constructor is `pub(crate)`), so the
//! constant is written — but the day a claim path changes that arm, `auth.rs:1910` goes red FIRST
//! and this probe never gets the chance to stay green over a reopened hole.
//!
//! **The fake repository is deliberately `ReadScope`-blind.** The nearest in-repo precedent,
//! `crates/server/tests/graphql_subscriptions.rs:50`, has the stand-in enforce the policy itself —
//! copy that here and the whole suite passes over an unchanged resolver. `RefundReadRepository`'s
//! only input is `RefundFilter`, so the fake cannot see a scope even by accident, and it CAPTURES
//! every call it receives: the filter is the seam, so asserting the captured calls alongside the
//! rows makes a wrong filter visible even when the row set happens to look right.
//!
//! **No bare zero.** Every empty result is asserted JOINTLY with a non-empty one from the same
//! fixture, the same schema and the same execution. A lone `assert!(rows.is_empty())` is green
//! when the fixture was never populated, when the query name was misspelled, when the guard
//! rejected the request and when the resolver errored — four ways to be green over nothing.

use std::sync::{Arc, Mutex};

use application::queries::{RefundFilter, RefundReadRepository, RefundRow};
use async_graphql::Request;
use async_trait::async_trait;
use domain::generated::scalars as ds;
use domain::shared::errors::DomainError;
use server::graphql_acl::RequestRole;
use server::graphql_schema::build_schema;

// ---------------------------------------------------------------------------------------------
// The fixture — CARD-DEFINED, not measured. Two restaurants of UNEQUAL size so a count-only
// assertion cannot pass by luck, and ids distinguishable so "returned the other tenant's rows" is
// visible rather than merely equinumerous.
//
//   O1, O2       -> R1
//   O3, O4, O5   -> R2
// ---------------------------------------------------------------------------------------------

/// The five fixture orders, in fixture order.
const ORDERS: [(&str, u128); 5] = [("O1", 1), ("O2", 2), ("O3", 3), ("O4", 4), ("O5", 5)];

fn order_id(label: &str) -> ds::OrderId {
    let (_, n) = ORDERS.iter().find(|(l, _)| *l == label).expect("a fixture order label");
    ds::OrderId(uuid::Uuid::from_u128(*n))
}

/// The label of a returned `orderId`, so assertions read as the card's `O1..O5` rather than as
/// UUIDs. Anything the fixture did not put in is a hard failure, never a silent skip.
fn label_of(order_id: &str) -> &'static str {
    let uuid: uuid::Uuid = order_id.parse().expect("orderId is a UUID");
    ORDERS
        .iter()
        .find(|(_, n)| uuid::Uuid::from_u128(*n) == uuid)
        .map(|(l, _)| *l)
        .unwrap_or_else(|| panic!("a row the fixture never contained: {order_id}"))
}

fn row(label: &str, restaurant: ds::RestaurantId) -> RefundRow {
    RefundRow {
        order_id: order_id(label),
        restaurant_id: restaurant,
        status: ds::RefundStatus::REQUESTED,
        amount_cents: ds::MoneyCents(1_000),
        currency: ds::CurrencyCode("EUR".into()),
        approved_amount_cents: None,
        reason: None,
        refund_id: None,
        requested_at: chrono::Utc::now(),
        decided_at: None,
    }
}

// ---------------------------------------------------------------------------------------------
// The `ReadScope`-BLIND, call-capturing fake.
// ---------------------------------------------------------------------------------------------

/// The refund queue stand-in. It takes, sees and is constructed with NO `ReadScope` — its only
/// input is `RefundFilter`, exactly like the real port — and it reproduces the real SQL's
/// two-clause `WHERE` (`crates/infrastructure/src/persistence/refund_queue.rs:63-71`). Every call
/// it receives is captured, so a probe can assert the filter the resolver actually built.
#[derive(Clone, Default)]
struct FakeRefundQueue {
    rows: Arc<Vec<RefundRow>>,
    calls: Arc<Mutex<Vec<RefundFilter>>>,
}

impl FakeRefundQueue {
    fn with_fixture() -> Self {
        Self {
            rows: Arc::new(vec![
                row("O1", restaurant(1)),
                row("O2", restaurant(1)),
                row("O3", restaurant(2)),
                row("O4", restaurant(2)),
                row("O5", restaurant(2)),
            ]),
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// The captured calls, as `Option<restaurant label>` — the only part any probe asserts on.
    fn captured(&self) -> Vec<Option<&'static str>> {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .map(|f| f.restaurant_id.map(|r| restaurant_label(r)))
            .collect()
    }
}

#[async_trait]
impl RefundReadRepository for FakeRefundQueue {
    async fn list(&self, filter: RefundFilter) -> Result<Vec<RefundRow>, DomainError> {
        self.calls.lock().unwrap().push(filter.clone());
        Ok(self
            .rows
            .iter()
            .filter(|r| filter.restaurant_id.is_none_or(|want| want == r.restaurant_id))
            .filter(|r| filter.status.is_none_or(|want| want == r.status))
            .cloned()
            .collect())
    }
}

fn restaurant(n: u128) -> ds::RestaurantId {
    ds::RestaurantId(uuid::Uuid::from_u128(0x1000 + n))
}

fn restaurant_label(id: ds::RestaurantId) -> &'static str {
    match id {
        x if x == restaurant(1) => "R1",
        x if x == restaurant(2) => "R2",
        _ => "R?",
    }
}

// ---------------------------------------------------------------------------------------------
// One execution.
// ---------------------------------------------------------------------------------------------

use server::graphql_read_binding::ReadScopeBindingMode as Mode;

/// A caller: its request role, the scope its credential resolves to, the `restaurantId` it asked
/// for (if any) and the binding mode in force. `mode: None` is the ABSENT case — the schema
/// executed with no mode datum at all, which must fail closed.
#[derive(Clone)]
struct Caller {
    role: RequestRole,
    scope: application::queries::ReadScope,
    filter: Option<ds::RestaurantId>,
    mode: Option<Mode>,
}

fn admin(mode: Option<Mode>) -> Caller {
    Caller {
        role: RequestRole::Admin,
        scope: application::queries::ReadScope::Admin,
        filter: None,
        mode,
    }
}

/// A RESTAURANT credential whose claim resolves to a restaurant — the BOUND case, which is
/// unreachable in production today and is exactly why it needs a probe before it is reachable.
fn bound(n: u128, mode: Option<Mode>) -> Caller {
    Caller {
        role: RequestRole::Restaurant,
        scope: application::queries::ReadScope::Restaurant(restaurant(n)),
        filter: None,
        mode,
    }
}

/// A RESTAURANT credential with no restaurant binding — `Identity::Unbound` ⇒ `ReadScope::Public`,
/// the only RESTAURANT credential that can exist today (`crates/server/src/auth.rs:1910`).
fn unbound(mode: Option<Mode>) -> Caller {
    Caller {
        role: RequestRole::Restaurant,
        scope: application::queries::ReadScope::Public,
        filter: None,
        mode,
    }
}

impl Caller {
    fn asking_for(mut self, n: u128) -> Self {
        self.filter = Some(restaurant(n));
        self
    }
}

/// Execute `pendingRefunds` once and return the ORDER LABELS it produced, sorted. A GraphQL error
/// is a hard failure: a probe that reads an empty list out of a REJECTED request proves nothing,
/// and that is one of the four ways a lone `is_empty()` assertion goes green over nothing.
async fn pending_refunds(repo: &FakeRefundQueue, caller: Caller) -> Vec<&'static str> {
    let schema = build_schema(None, None, None);
    let args = match caller.filter {
        Some(r) => format!("(input: {{ restaurantId: \"{}\" }})", r.0),
        None => String::new(),
    };
    let port: Arc<dyn RefundReadRepository> = Arc::new(repo.clone());
    let mut request = Request::new(format!("{{ pendingRefunds{args} {{ orderId }} }}"))
        .data(caller.role)
        .data(caller.scope)
        .data(port);
    // P8's arm: NO mode datum at all, which the resolver must read as ENFORCE.
    if let Some(mode) = caller.mode {
        request = request.data(mode);
    }
    let resp = schema.execute(request).await;
    assert!(resp.errors.is_empty(), "pendingRefunds errored: {:?}", resp.errors);
    let json = resp.data.into_json().expect("json");
    let mut out: Vec<&'static str> = json["pendingRefunds"]
        .as_array()
        .expect("pendingRefunds returns a list")
        .iter()
        .map(|r| label_of(r["orderId"].as_str().expect("orderId is a string")))
        .collect();
    out.sort_unstable();
    out
}

/// Every expectation below is a HARDCODED LITERAL, never derived from the fixture by a helper.
/// That is what makes the fixture's own mutation (swap R1 and R2 in `with_fixture`) able to fail:
/// derive the expectations and swapping moves both sides, the suite stays green, and the homework
/// grades itself.
fn all_five() -> Vec<&'static str> {
    vec!["O1", "O2", "O3", "O4", "O5"]
}

fn none() -> Vec<&'static str> {
    Vec::new()
}

// ---------------------------------------------------------------------------------------------
// P1 — the FIRST probe, and the only arm production can actually reach today.
// ---------------------------------------------------------------------------------------------

/// **P1.** `unbound ⇒ denied` outranks `other-restaurant ⇒ denied` (ADR-20260818-101500): without
/// it, `domain_id: None` gets coded as "unknown ⇒ allow", the cross-tenant probe passes, and the
/// hole is untouched. It is also the ONLY arm reachable in production today — nothing mints a
/// restaurant claim (`stamp_put_body` writes `{role, customer_id}` only,
/// `crates/infrastructure/src/integrations/supabase_auth.rs:424-433`), so every RESTAURANT
/// credential that can exist resolves to `Identity::Unbound` ⇒ `ReadScope::Public`.
///
/// Asserted as a PAIR against ADMIN on the same fixture, same schema, same execution: the admin
/// arm is what makes the empty arm an observation rather than a green over nothing.
#[tokio::test(flavor = "multi_thread")]
async fn p1_an_unbound_restaurant_reads_nothing_while_admin_reads_everything() {
    let repo = FakeRefundQueue::with_fixture();

    let admin_order_ids = pending_refunds(&repo, admin(Some(Mode::Observe))).await;
    let unbound_order_ids = pending_refunds(&repo, unbound(Some(Mode::Observe))).await;

    assert_eq!(
        (admin_order_ids, unbound_order_ids),
        (all_five(), none()),
        "the only RESTAURANT credential that can exist today is Identity::Unbound, and it reads \
         the whole platform's refund queue"
    );
}

// ---------------------------------------------------------------------------------------------
// P2–P4 — the binding itself.
// ---------------------------------------------------------------------------------------------

/// **P2.** A BOUND restaurant asking for nothing gets its OWN queue, not the platform's. The
/// captured filter is asserted beside the rows: the filter is the seam, so a resolver that returned
/// the right rows by luck (or by the fake filtering for it) is still caught.
#[tokio::test(flavor = "multi_thread")]
async fn p2_a_bound_restaurant_with_no_filter_reads_its_own_queue() {
    let repo = FakeRefundQueue::with_fixture();

    let own = pending_refunds(&repo, bound(1, Some(Mode::Enforce))).await;
    let everything = pending_refunds(&repo, admin(Some(Mode::Enforce))).await;

    assert_eq!(
        (own, everything),
        (vec!["O1", "O2"], all_five()),
        "no filter means the CALLER'S OWN queue -- the bound scope is applied, not skipped"
    );
    assert_eq!(
        repo.captured(),
        vec![Some("R1"), None],
        "the resolver must push its own bound restaurant into the filter it hands the repository; \
         the admin call carries none"
    );
}

/// **P3.** INTERSECTION, and the finding that nearly made this chunk a shim: a caller bound to R1
/// that asks for R2 gets EMPTY — never R1's rows. Substitution would make the schema lie (the
/// client asked for R2, got HTTP 200 with R1's rows, no error, no way to tell).
///
/// Asserted as a TRIPLE: empty rows, the repository NOT CALLED AT ALL, and the same caller with no
/// filter still seeing its own two rows. The middle assertion is stronger than an empty row set and
/// free — the resolver returns before it ever touches the port.
#[tokio::test(flavor = "multi_thread")]
async fn p3_a_conflicting_filter_intersects_to_empty_without_touching_the_repository() {
    let repo = FakeRefundQueue::with_fixture();
    let conflicting = pending_refunds(&repo, bound(1, Some(Mode::Enforce)).asking_for(2)).await;
    let captured_during_conflict = repo.captured();

    let same_caller_no_filter = pending_refunds(&repo, bound(1, Some(Mode::Enforce))).await;

    assert_eq!(
        (conflicting, captured_during_conflict, same_caller_no_filter),
        (none(), Vec::new(), vec!["O1", "O2"]),
        "bound scope INTERSECT requested filter: an argument may narrow WITHIN the caller's scope \
         and can never move it -- and the empty intersection short-circuits before the read"
    );
}

/// **P4.** The ADMIN arm is untouched in every mode: an admin arbitrates ACROSS restaurants
/// (`stories.yaml`, `ArbitrateRefunds` — "the cross-restaurant refund queue"), so the argument stays
/// a plain selector for them. Asserted as a pair so "admin sees everything" cannot be satisfied by
/// an admin who simply ignores the filter.
#[tokio::test(flavor = "multi_thread")]
async fn p4_admin_arbitrates_across_restaurants_and_its_filter_still_selects() {
    let repo = FakeRefundQueue::with_fixture();

    let unfiltered = pending_refunds(&repo, admin(Some(Mode::Enforce))).await;
    let filtered = pending_refunds(&repo, admin(Some(Mode::Enforce)).asking_for(2)).await;

    assert_eq!(
        (unfiltered, filtered),
        (all_five(), vec!["O3", "O4", "O5"]),
        "the admin arm is unchanged by the binding -- and its restaurantId still selects"
    );
}

// ---------------------------------------------------------------------------------------------
// P5, P6, P8 — the three modes.
// ---------------------------------------------------------------------------------------------

/// **P5.** The two shipped modes must DIFFER on one arm and AGREE on the other; both halves are
/// asserted as triples with a non-empty member from the same fixture, so neither can anchor on
/// `[] == []` and go green over an unpopulated fixture or a misspelled query name.
///
/// They differ on the BOUND caller with a conflicting filter — `Observe` still serves what was
/// asked for and only counts, `Enforce` serves nothing. They AGREE on the unbound caller, because
/// that arm is not a binding at all: it is the absence of one, and the system's recorded answer is
/// that a missing claim fails closed.
#[tokio::test(flavor = "multi_thread")]
async fn p5_the_modes_differ_on_the_bound_arm_and_agree_on_the_unbound_one() {
    let repo = FakeRefundQueue::with_fixture();

    let observe_conflicting =
        pending_refunds(&repo, bound(1, Some(Mode::Observe)).asking_for(2)).await;
    let enforce_conflicting =
        pending_refunds(&repo, bound(1, Some(Mode::Enforce)).asking_for(2)).await;
    let admin_no_filter = pending_refunds(&repo, admin(Some(Mode::Observe))).await;
    assert_eq!(
        (observe_conflicting, enforce_conflicting, admin_no_filter),
        (vec!["O3", "O4", "O5"], none(), all_five()),
        "OBSERVE serves the conflicting request and only counts it; ENFORCE serves nothing"
    );

    let admin_observe = pending_refunds(&repo, admin(Some(Mode::Observe))).await;
    let unbound_observe = pending_refunds(&repo, unbound(Some(Mode::Observe))).await;
    let unbound_enforce = pending_refunds(&repo, unbound(Some(Mode::Enforce))).await;
    assert_eq!(
        (admin_observe, unbound_observe, unbound_enforce),
        (all_five(), none(), none()),
        "the unbound arm is NOT gated between OBSERVE and ENFORCE -- it is the absence of a \
         binding, not a binding"
    );
}

/// **P6.** `OFF` restores today's behaviour EXACTLY. This is the probe most likely to be skipped
/// and the one that matters most operationally: an untested rollback rung is a rollback that fails
/// at 20:00 on a Friday with the operator holding the flag. The RLS card learned the same lesson —
/// everyone remembers to test enforcing; the untested clause is the one the mitigation rests on.
#[tokio::test(flavor = "multi_thread")]
async fn p6_off_restores_todays_behaviour_exactly() {
    let repo = FakeRefundQueue::with_fixture();

    let off_unbound = pending_refunds(&repo, unbound(Some(Mode::Off))).await;
    let enforce_unbound = pending_refunds(&repo, unbound(Some(Mode::Enforce))).await;

    assert_eq!(
        (off_unbound, enforce_unbound),
        (all_five(), none()),
        "OFF is the incident rung: it re-opens the only hole production can reach today, which is \
         exactly what it is for -- and it must be provably reachable"
    );
}

/// **P8.** A request context with NO mode datum is treated as `Enforce` — fail closed, the same
/// posture as `ctx.data_opt::<ReadScope>().unwrap_or(Public)` on every scoped resolver. Asserted as
/// a triple against a non-empty arm so it cannot be green over nothing.
#[tokio::test(flavor = "multi_thread")]
async fn p8_a_missing_mode_is_treated_as_enforce() {
    let repo = FakeRefundQueue::with_fixture();

    let absent = pending_refunds(&repo, unbound(None)).await;
    let enforce = pending_refunds(&repo, unbound(Some(Mode::Enforce))).await;
    let off = pending_refunds(&repo, unbound(Some(Mode::Off))).await;

    assert_eq!(
        (absent, enforce, off),
        (none(), none(), all_five()),
        "\"forgot to wire the mode\" must never be indistinguishable from \"chose the permissive \
         value\""
    );
}

// ---------------------------------------------------------------------------------------------
// P9 — the mode is legible at boot.
// ---------------------------------------------------------------------------------------------

/// **P9.** Nothing else in the system distinguishes a pod running `OFF` from one running `ENFORCE`:
/// the only signal carrying `mode` fires on MISMATCH, and mismatch traffic is zero today. That is
/// CLAUDE.md's named defect class — a monitoring path that can only fire when a signal arrives.
///
/// The boot report is the one artifact that already answers it, for free: it prints every declared
/// non-secret key's resolved value, so an operator can read which mode the pod is actually running
/// instead of inferring it from the absence of a counter. Also `render-config-sync.yml` explicitly
/// does not push non-secret values, so an operator-set override is otherwise untracked config —
/// the `RUN_SIRENE_WORKER` failure that workflow's own header narrates.
///
/// Asserted against the RESOLVED value rather than a literal, so the probe holds however the
/// environment running it is configured.
#[test]
fn p9_the_resolved_binding_mode_is_legible_in_the_boot_report() {
    let (config, _) = server::generated::config::Config::resolve();
    let report = config.boot_report();
    let line = report
        .lines()
        .find(|l| l.trim_start().starts_with("READ_SCOPE_BINDING_MODE"))
        .unwrap_or_else(|| {
            panic!("no READ_SCOPE_BINDING_MODE line in the boot report:\n{report}")
        });
    assert_eq!(
        (
            line.split('=').nth(1).map(str::trim),
            Some(config.read_scope_binding_mode.as_str())
        ),
        (
            Some(config.read_scope_binding_mode.as_str()),
            Some(config.read_scope_binding_mode.as_str())
        ),
        "the boot report must state the mode the process actually resolved -- it is the only \
         artifact that distinguishes a pod running OFF from one running ENFORCE"
    );
}
