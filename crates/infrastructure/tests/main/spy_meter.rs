//! The spy meter — the metric-ASSERTION harness (#589).
//!
//! Until this file existed, `infrastructure` could prove an emit fired exactly once
//! (`tests/orders_placed_metric.rs`, #456) and nowhere else. Every OTHER monitor was tested by
//! calling it and asserting it returned `Ok` — which is the defect #589 names: a dead-man's
//! switch whose only test never looks at a series cannot tell "emitted zero" from "emitted
//! nothing", so the mutation that silences it stays green. This harness is the thing that looks.
//!
//! **What it is**: an in-memory `SdkMeterProvider` installed as the PROCESS-GLOBAL provider, plus
//! a drain that flattens the exporter's `ResourceMetrics` into `(attributes, value)` points an
//! assertion can compare BY EQUALITY. It is shared by `#[path]` include, never copied — a second
//! hand-rolled drain is how two harnesses start disagreeing about what "a point" is.
//!
//! **Two constraints it inherits from the SDK, both load-bearing:**
//!
//! 1. `telemetry::meters` binds `opentelemetry::global::meter` once per process (`OnceLock`), so
//!    the provider must be installed before the process's FIRST metric call. A binary that hosts
//!    ~30 suites cannot promise that — hence one standalone binary per spy, and ONE
//!    `#[tokio::test]` in it (a second would race the same process-global binding, and with
//!    cumulative state, each other's points).
//! 2. **Delta temporality on purpose.** Under the default cumulative temporality every flush
//!    re-reports every point ever recorded, so "the point set of ONE tick" is inexpressible and
//!    an equality assertion degrades into `contains`. Delta makes [`SpyMeter::drain`] mean
//!    exactly "what was emitted since the previous drain" — which is what a per-tick liveness
//!    contract is about.
//!
//! A histogram point's value is its delta SUM and its `records` is its delta COUNT; for a monitor
//! that records once per tick per lane those are "the value it recorded" and "it recorded once",
//! which is precisely what a liveness assertion needs to state. A gauge point is its last value,
//! with `records` 1.

use std::collections::BTreeMap;

use opentelemetry::KeyValue;
use opentelemetry_sdk::metrics::data::{AggregatedMetrics, Metric, MetricData};
use opentelemetry_sdk::metrics::{
    InMemoryMetricExporter, InMemoryMetricExporterBuilder, SdkMeterProvider, Temporality,
};

/// One flattened data point: the attribute set that identifies it, and the number the assertion
/// is about. `BTreeMap` so two points compare and sort deterministically whatever order the SDK
/// hands their attributes back in.
pub(crate) type Point = (BTreeMap<String, String>, f64);

/// The installed spy. Holds the provider alive for the life of the test — dropping it would stop
/// the reader the drain flushes.
pub(crate) struct SpyMeter {
    exporter: InMemoryMetricExporter,
    provider: SdkMeterProvider,
}

impl SpyMeter {
    /// Install the spy as the process-global meter provider. **Call this before anything that can
    /// touch a meter** (including before opening the database, since a migration or a pool can
    /// bind one): the binding is a `OnceLock` and the loser is a silent no-op provider, i.e. a
    /// harness that observes nothing and passes.
    pub(crate) fn install() -> SpyMeter {
        let exporter =
            InMemoryMetricExporterBuilder::new().with_temporality(Temporality::Delta).build();
        let provider =
            SdkMeterProvider::builder().with_periodic_exporter(exporter.clone()).build();
        opentelemetry::global::set_meter_provider(provider.clone());
        SpyMeter { exporter, provider }
    }

    /// Flush the reader and TAKE everything emitted since the previous drain.
    ///
    /// Taking (rather than reading) is the whole ergonomics of the harness: one drain per tick
    /// gives each assertion a point set that belongs to that tick alone, and no caller can forget
    /// a `force_flush` and read a vacuous empty set as a pass.
    pub(crate) fn drain(&self) -> Series {
        self.provider.force_flush().expect("flush the spy reader");
        let mut series = Series::default();
        for rm in self.exporter.get_finished_metrics().expect("finished metrics") {
            for scope in rm.scope_metrics() {
                for metric in scope.metrics() {
                    series.absorb(metric);
                }
            }
        }
        self.exporter.reset();
        series
    }
}

/// Everything one drain collected, by metric name.
#[derive(Default)]
pub(crate) struct Series {
    by_name: BTreeMap<String, Vec<(BTreeMap<String, String>, f64, u64)>>,
}

impl Series {
    /// Every point emitted for `name`, sorted — the vector an equality assertion compares
    /// against. An empty vector means the series was NOT emitted, which is the distinction the
    /// whole harness exists to make.
    pub(crate) fn points(&self, name: &str) -> Vec<Point> {
        let mut out: Vec<Point> = self
            .by_name
            .get(name)
            .map(|ps| ps.iter().map(|(attrs, value, _)| (attrs.clone(), *value)).collect())
            .unwrap_or_default();
        out.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.total_cmp(&b.1)));
        out
    }

    /// How many MEASUREMENTS each point aggregates (histogram count; 1 for a gauge). One tick that
    /// records a lane twice is a different fact from one tick that records it once, and a sum
    /// alone cannot tell them apart.
    pub(crate) fn records(&self, name: &str) -> Vec<(BTreeMap<String, String>, u64)> {
        let mut out: Vec<(BTreeMap<String, String>, u64)> = self
            .by_name
            .get(name)
            .map(|ps| ps.iter().map(|(attrs, _, records)| (attrs.clone(), *records)).collect())
            .unwrap_or_default();
        out.sort();
        out
    }

    /// Flatten one metric, or PANIC. An aggregation this drain cannot express is a new kind of
    /// signal arriving, and the honest answer is a loud failure naming it — silently skipping it
    /// would turn "the harness cannot see your metric" into "your metric was not emitted".
    fn absorb(&mut self, metric: &Metric) {
        let mut points: Vec<(BTreeMap<String, String>, f64, u64)> = Vec::new();
        match metric.data() {
            AggregatedMetrics::F64(MetricData::Histogram(h)) => {
                for dp in h.data_points() {
                    points.push((attribute_set(dp.attributes()), dp.sum(), dp.count()));
                }
            }
            AggregatedMetrics::U64(MetricData::Histogram(h)) => {
                for dp in h.data_points() {
                    points.push((attribute_set(dp.attributes()), dp.sum() as f64, dp.count()));
                }
            }
            AggregatedMetrics::I64(MetricData::Histogram(h)) => {
                for dp in h.data_points() {
                    points.push((attribute_set(dp.attributes()), dp.sum() as f64, dp.count()));
                }
            }
            AggregatedMetrics::I64(MetricData::Gauge(g)) => {
                for dp in g.data_points() {
                    points.push((attribute_set(dp.attributes()), dp.value() as f64, 1));
                }
            }
            AggregatedMetrics::F64(MetricData::Gauge(g)) => {
                for dp in g.data_points() {
                    points.push((attribute_set(dp.attributes()), dp.value(), 1));
                }
            }
            AggregatedMetrics::U64(MetricData::Gauge(g)) => {
                for dp in g.data_points() {
                    points.push((attribute_set(dp.attributes()), dp.value() as f64, 1));
                }
            }
            AggregatedMetrics::U64(MetricData::Sum(s)) => {
                for dp in s.data_points() {
                    points.push((attribute_set(dp.attributes()), dp.value() as f64, 1));
                }
            }
            AggregatedMetrics::I64(MetricData::Sum(s)) => {
                for dp in s.data_points() {
                    points.push((attribute_set(dp.attributes()), dp.value() as f64, 1));
                }
            }
            AggregatedMetrics::F64(MetricData::Sum(s)) => {
                for dp in s.data_points() {
                    points.push((attribute_set(dp.attributes()), dp.value(), 1));
                }
            }
            other => panic!(
                "spy_meter: `{}` aggregates as a shape this drain cannot flatten: {other:?} -- \
                 extend the drain, never weaken the assertion",
                metric.name()
            ),
        }
        self.by_name.entry(metric.name().to_string()).or_default().extend(points);
    }
}

fn attribute_set<'a>(kvs: impl Iterator<Item = &'a KeyValue>) -> BTreeMap<String, String> {
    kvs.map(|kv| (kv.key.to_string(), kv.value.to_string())).collect()
}
