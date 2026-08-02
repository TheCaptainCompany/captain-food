//! Generated-artifact emitters, one module per artifact family (#277 split).

pub(crate) mod actor_clients; // actor_client crate: typed per-actor mailbox clients (#284) + addressing tables (#290)
pub(crate) mod behaviour_tests; // application behaviour_tests.rs from tests.yaml (§7 corpus)
pub(crate) mod docs; // documentation.generated.{md,html} + context map + story parsing
pub(crate) mod pm_orchestrators; // application process_managers.rs (typed-step PM legs)
pub(crate) mod pm_state; // PM state ports (application) + Postgres stores (infrastructure)
pub(crate) mod projectors; // application rows.rs + projectors.rs from projection tables
pub(crate) mod rust_domain; // domain crate: scalars/entities/events/commands/errors/states/lifecycles + handlers
pub(crate) mod server_graphql; // server crate: async-graphql layer, command router, deletion policy, reminders
pub(crate) mod services; // service ports, HTTP clients, bindings, /services/* routes, render manifest
pub(crate) mod sql; // views/schema SQL, projection DDL, database.md §2 injection
pub(crate) mod translations; // merged translations.generated.json
pub(crate) mod web; // web crate: tokens CSS, SDUI registry, data layer, screens

pub(crate) use actor_clients::*;
pub(crate) use behaviour_tests::*;
pub(crate) use docs::*;
pub(crate) use pm_orchestrators::*;
pub(crate) use pm_state::*;
pub(crate) use projectors::*;
pub(crate) use rust_domain::*;
pub(crate) use server_graphql::*;
pub(crate) use services::*;
pub(crate) use sql::*;
pub(crate) use translations::*;
pub(crate) use web::*;
