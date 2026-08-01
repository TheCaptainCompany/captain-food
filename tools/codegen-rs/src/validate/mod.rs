//! Validator sections, one module per rule family (#277 split); the run order lives in validate() (core).

pub(crate) mod proposals;

pub(crate) use proposals::*;
