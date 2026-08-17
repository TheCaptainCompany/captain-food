//! Validator sections, one module per rule family (#277 split); the run order lives in
//! validate() (validate::core).

pub(crate) mod answers; // §2g actor `answers:` blocks (PROP-20260815-142349, #582)
pub(crate) mod bins; // §15 bin topology ↔ c4-l2 containers (#382)
pub(crate) mod business_metrics; // §19 business-metric catalog (ADR-20260811-014129, #484)
pub(crate) mod core; // §1 validate() orchestrator + resolver-args checks
pub(crate) mod databases; // §18 database catalog + per-kind placement (#494 slice 1)
pub(crate) mod lifecycles; // §2c aggregate lifecycles (parse + validate)
pub(crate) mod mailbox; // §2d actor-mailbox addressing + §2e declared state/requires
pub(crate) mod metric_emitters; // §20 declared-but-silent metrics (observability.yaml vs crates/**, #608)
pub(crate) mod process_managers; // §2b typed-step process managers
pub(crate) mod proposals; // §13 docs/proposals hygiene
pub(crate) mod read_targets; // §5c-bis read-target ownership (reads/readsInfrastructure, ADR-20260812-214500)
pub(crate) mod schema_writers; // §16 writer/schema agreement (migrations vs *_store.rs, #474)
pub(crate) mod scopes; // §14 per-scope spec folders (placement, DAG, kernel purity, api nesting)
pub(crate) mod reminders; // §2f reminders/schedules/deletion DSL
pub(crate) mod services; // §2d service catalog
pub(crate) mod shape; // api-shape helpers (roles, inline types, data shapes)
pub(crate) mod span_error_status; // §21 technical_error rules that cannot fire (observability.yaml vs spans.rs, #623/#624)
pub(crate) mod translations; // §10 translation hygiene
pub(crate) mod warning_baseline; // §17 warning ratchet (tools/codegen-rs/warning-baseline.json)

pub(crate) use answers::*;
pub(crate) use bins::*;
pub(crate) use business_metrics::*;
pub(crate) use core::*;
pub(crate) use databases::*;
pub(crate) use lifecycles::*;
pub(crate) use mailbox::*;
pub(crate) use metric_emitters::*;
pub(crate) use process_managers::*;
pub(crate) use proposals::*;
pub(crate) use read_targets::*;
pub(crate) use reminders::*;
pub(crate) use services::*;
pub(crate) use schema_writers::*;
pub(crate) use scopes::*;
pub(crate) use shape::*;
pub(crate) use span_error_status::*;
pub(crate) use translations::*;
pub(crate) use warning_baseline::*;
