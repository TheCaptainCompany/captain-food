//! `RiderCompute` (#144) — the identity bridge's projector.
//!
//! Every column is mechanical (scalar-latest from the declared `from` lineage), so the generated
//! `project_rider` dispatch does all the work and this trait has no hooks. The unit struct exists to
//! satisfy the generated signature and to keep the shape uniform with every other read model, so a
//! future computed column has an obvious home.

use crate::projections::RiderCompute;

pub struct RiderProjector;

impl RiderCompute for RiderProjector {}
