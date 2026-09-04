//! #639 part C step 4-i (ADR-20260904-081527 §9): the `rider-restriction` contract's counter —
//! `rider_restricted_denied_total{operation}` — proved FIRING through the real `StandingGuard`
//! (`crates/server/src/graphql/acl.rs`), never assumed from the constant in `contract.rs`. NO
//! `rider_id` label (the counter's own contract note): the correlating identity travels on the
//! sibling INFO trace event instead (the #748 skip-trace pattern).
//!
//! Its OWN test binary on purpose, same reason as `checkout_degraded_metric.rs`/
//! `orders_placed_metric.rs`: `telemetry::meters` binds `opentelemetry::global::meter` once per
//! process (`OnceLock`), so the spy provider must be installed before the process's first metric
//! call — a guarantee the parallel in-crate harness cannot give.

use application::queries::ReadScope;
use domain::generated::scalars::{RiderId, RiderStanding};
use opentelemetry_sdk::metrics::data::{AggregatedMetrics, MetricData};
use opentelemetry_sdk::metrics::{InMemoryMetricExporter, SdkMeterProvider};
use server::graphql_acl::RequestRole;
use server::graphql_schema::build_schema;

fn acting(role: RequestRole) -> server::ActingRole {
    server::Principal::role_binding(role, "test-subject".to_string(), Some(uuid::Uuid::from_u128(0x6394_3)))
        .acting_role(role)
}

/// Every `rider_restricted_denied_total` data point the spy collected, as `(operation, count)`.
fn denied_points(exporter: &InMemoryMetricExporter) -> Vec<(String, u64)> {
    let mut out = Vec::new();
    for rm in exporter.get_finished_metrics().expect("finished metrics") {
        for scope in rm.scope_metrics() {
            for metric in scope.metrics() {
                if metric.name() != "rider_restricted_denied_total" {
                    continue;
                }
                let AggregatedMetrics::U64(MetricData::Sum(sum)) = metric.data() else {
                    panic!("a defect counter aggregates as a u64 Sum: {:?}", metric.data());
                };
                for dp in sum.data_points() {
                    // The contract's own note, pinned: NO `rider_id` attribute on this counter.
                    assert!(
                        dp.attributes().all(|kv| kv.key.as_str() != "rider_id"),
                        "rider_restricted_denied_total must carry no rider_id label (the #748 pattern \
                         puts identity on the sibling trace event instead)"
                    );
                    let operation = dp
                        .attributes()
                        .find(|kv| kv.key.as_str() == "operation")
                        .map(|kv| kv.value.to_string())
                        .unwrap_or_default();
                    out.push((operation, dp.value()));
                }
            }
        }
    }
    out.sort();
    out
}

/// The gateway boundary's denial and its GATE, both directions: exactly one count, on the
/// operation actually refused — and NOTHING from an ACTIVE rider on the same operation, nor from a
/// RESTRICTED rider on an operation IN the `whileRestricted` carve (`myStanding`).
#[tokio::test]
async fn a_restricted_riders_denied_door_emits_the_counter_with_its_operation_and_no_rider_id() {
    // The spy provider FIRST — before any guard check can bind the process-wide meter.
    let exporter = InMemoryMetricExporter::default();
    let provider = SdkMeterProvider::builder().with_periodic_exporter(exporter.clone()).build();
    opentelemetry::global::set_meter_provider(provider.clone());

    // No ReadDeps/WriteDeps: the StandingGuard runs BEFORE any resolver body touches `ctx.data()`
    // for a repository, so a denial fires (and a pass reaches, then fails on the absent wiring --
    // a DIFFERENT, unrelated error) without a database, same posture as `checkout_degraded_metric.rs`.
    let schema = build_schema(None, None, None);
    let rider_id = RiderId(uuid::Uuid::from_u128(0x6394_3A));
    let restricted = ReadScope::Rider { id: rider_id, standing: RiderStanding::RESTRICTED };
    let active = ReadScope::Rider { id: rider_id, standing: RiderStanding::ACTIVE };

    // The one legitimate denial: a RESTRICTED rider on acceptDelivery (not in the carve).
    let accept = format!(
        r#"mutation {{ acceptDelivery(input: {{ deliveryJobId: "{}" }}) {{ messageId }} }}"#,
        uuid::Uuid::new_v4()
    );
    let resp = schema
        .execute(async_graphql::Request::new(accept.clone()).data(acting(RequestRole::Rider)).data(restricted.clone()))
        .await;
    assert_eq!(resp.errors.len(), 1, "expected the synchronous FORBIDDEN: {:?}", resp.errors);

    // Must NOT count: an ACTIVE rider on the SAME operation (the guard admits it; whatever fails
    // past that point is unrelated to standing).
    let _ = schema
        .execute(async_graphql::Request::new(accept).data(acting(RequestRole::Rider)).data(active))
        .await;

    // Must NOT count: a RESTRICTED rider on `myStanding` -- IN the carve, so the guard admits it too.
    let _ = schema
        .execute(
            async_graphql::Request::new("query { myStanding { standing } }")
                .data(acting(RequestRole::Rider))
                .data(restricted),
        )
        .await;

    provider.force_flush().expect("flush the spy reader");
    assert_eq!(
        denied_points(&exporter),
        vec![("acceptDelivery".to_string(), 1)],
        "exactly ONE denial -- the RESTRICTED rider refused on the un-carved operation"
    );
}
