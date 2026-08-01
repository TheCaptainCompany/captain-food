//! GraphQL BFF (ADR-0006 "role = path"). The SDL is generated from `api.yaml`; here we host it with
//! async-graphql. Stage 1a: the generated type layer (`generated/` — wrapper scalars, output/input
//! types, QueryRoot) backs the schema; the real read resolvers land next.

pub mod acl;
pub mod generated;
pub mod routes;
pub mod schema;
pub mod session;

/// The Runtime D1 flip gate (`PM_MAILBOX_DELIVERY`, #272 / ADR-20260801-023000), injected as
/// schema data: the generated process-manager resolvers (placeOrder / approveRefund / denyRefund)
/// carry BOTH arms and read this at request time — mailbox delivery through the prepare phase
/// when true, the legacy journal+spawn path when false (gate-then-stabilize; the default flip is
/// its own recorded decision).
#[derive(Clone, Copy, Debug)]
pub struct PmMailboxDelivery(pub bool);
