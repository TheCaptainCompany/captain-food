//! Generated-artifact emitters, one module per artifact family (#277 split).

pub(crate) mod actor_clients; // actor_client crate: typed per-actor mailbox clients (#284) + addressing tables (#290)
pub(crate) mod app_index; // specs/generated/apps.generated.md: the per-app index (#491, PROP-20260811-141654 slice A1)
pub(crate) mod behaviour_tests; // application behaviour_tests.rs from tests.yaml (§7 corpus)
pub(crate) mod bins; // per-deployable bin crates under crates/bins/ (#382, ADR-20260807-183024 step 3)
pub(crate) mod databases; // specs/generated/databases.generated.{md,json}: the placement inventory (#494 slice 1)
pub(crate) mod deploy; // deploy/generated/: K8s manifests, Dockerfile.bin, images/secret contracts, pin ledger (#349, step 4)
pub(crate) mod docs; // documentation.generated.{md,html} + context map + story parsing
pub(crate) mod domain_scopes; // per-scope domain crates + kernel + crate graph (#373)
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
pub(crate) use app_index::*;
pub(crate) use behaviour_tests::*;
pub(crate) use bins::*;
pub(crate) use databases::*;
pub(crate) use deploy::*;
pub(crate) use docs::*;
pub(crate) use domain_scopes::*;
pub(crate) use pm_orchestrators::*;
pub(crate) use pm_state::*;
pub(crate) use projectors::*;
pub(crate) use rust_domain::*;
pub(crate) use server_graphql::*;
pub(crate) use services::*;
pub(crate) use sql::*;
pub(crate) use translations::*;
pub(crate) use web::*;
