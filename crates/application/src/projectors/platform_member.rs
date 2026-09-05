//! Hand-written `PlatformMemberCompute` (ADR-0040; #639 part C step 6-v, ADR-20260905-223957 §1/§2).
//!
//! Every column of `PlatformMember` is mechanical -- the generated `project_platform_member`
//! dispatch maps them straight off `PlatformAccessGranted`'s `platformMembershipId`/`authSubject`.
//! This impl is deliberately empty, the `MemberProjector` precedent exactly: there is no computed,
//! cross-stream or accumulated column to own. It exists because the generated dispatch is generic
//! over the trait.

use crate::projections::PlatformMemberCompute;

pub struct PlatformMemberProjector;

impl PlatformMemberCompute for PlatformMemberProjector {}
