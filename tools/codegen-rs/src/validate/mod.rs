//! Validator sections, one module per rule family (#277 split); the run order lives in validate() (core).

pub(crate) mod proposals;

pub(crate) use proposals::*;
pub(crate) mod process_managers;
pub(crate) use process_managers::*;
pub(crate) mod lifecycles;
pub(crate) use lifecycles::*;
pub(crate) mod mailbox;
pub(crate) use mailbox::*;
pub(crate) mod reminders;
pub(crate) use reminders::*;
pub(crate) mod services;
pub(crate) use services::*;
