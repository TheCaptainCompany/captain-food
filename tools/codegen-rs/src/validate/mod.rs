//! Validator sections, one module per rule family (#277 split); the run order lives in
//! validate() (validate::core).

pub(crate) mod bins; // §15 bin topology ↔ c4-l2 containers (#382)
pub(crate) mod core; // §1 validate() orchestrator + resolver-args checks
pub(crate) mod lifecycles; // §2c aggregate lifecycles (parse + validate)
pub(crate) mod mailbox; // §2d actor-mailbox addressing + §2e declared state/requires
pub(crate) mod process_managers; // §2b typed-step process managers
pub(crate) mod proposals; // §13 docs/proposals hygiene
pub(crate) mod schema_writers; // §16 writer/schema agreement (migrations vs *_store.rs, #474)
pub(crate) mod scopes; // §14 per-scope spec folders (placement, DAG, kernel purity, api nesting)
pub(crate) mod reminders; // §2f reminders/schedules/deletion DSL
pub(crate) mod services; // §2d service catalog
pub(crate) mod shape; // api-shape helpers (roles, inline types, data shapes)
pub(crate) mod translations; // §10 translation hygiene

pub(crate) use bins::*;
pub(crate) use core::*;
pub(crate) use lifecycles::*;
pub(crate) use mailbox::*;
pub(crate) use process_managers::*;
pub(crate) use proposals::*;
pub(crate) use reminders::*;
pub(crate) use services::*;
pub(crate) use schema_writers::*;
pub(crate) use scopes::*;
pub(crate) use shape::*;
pub(crate) use translations::*;
