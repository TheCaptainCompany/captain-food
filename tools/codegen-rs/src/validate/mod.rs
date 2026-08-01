//! Validator sections, one module per rule family (#277 split); the run order lives in
//! validate() (validate::core).

pub(crate) mod core; // §1 validate() orchestrator + resolver-args checks
pub(crate) mod lifecycles; // §2c aggregate lifecycles (parse + validate)
pub(crate) mod mailbox; // §2d actor-mailbox addressing + §2e declared state/requires
pub(crate) mod process_managers; // §2b typed-step process managers
pub(crate) mod proposals; // §13 docs/proposals hygiene
pub(crate) mod reminders; // §2f reminders/schedules/deletion DSL
pub(crate) mod services; // §2d service catalog
pub(crate) mod shape; // api-shape helpers (roles, inline types, data shapes)
pub(crate) mod translations; // §10 translation hygiene

pub(crate) use core::*;
pub(crate) use lifecycles::*;
pub(crate) use mailbox::*;
pub(crate) use process_managers::*;
pub(crate) use proposals::*;
pub(crate) use reminders::*;
pub(crate) use services::*;
pub(crate) use shape::*;
pub(crate) use translations::*;
