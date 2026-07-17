//! GraphQL BFF (ADR-0006 "role = path"). The SDL is generated from `api.yaml`; here we host it with
//! async-graphql. Stage 1a: the generated type layer (`generated/` — wrapper scalars, output/input
//! types, QueryRoot) backs the schema; the real read resolvers land next.

pub mod acl;
pub mod generated;
pub mod routes;
pub mod schema;
