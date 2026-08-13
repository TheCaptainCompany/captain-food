//! §15 — Bin topology ↔ c4-l2 containers (ADR-20260807-183024 step 3, #382).
//!
//! The bin emitter derives each family from its own source of truth (actors.yaml for
//! `actor-*`/`pm-*`, the scope folders for `projector-*`/`graphql-*`, the UserType role paths
//! for `gateway-*`) and `architecture/c4-l2.yaml` declares the deploy topology humans read.
//! These rules keep the two from drifting — in BOTH directions, so a new aggregate/scope/role
//! cannot silently miss its container, and a container cannot name a deployable nothing
//! generates:
//!
//! 1. `c4-bin-name-mismatch`: a container realizing actor A must be NAMED `actor_bin_name(A)` —
//!    one actor, one bin, ONE spelling; the crate-graph key, the `crates/bins/` directory and
//!    the container id are the same string or the #349 image/Deployment chain would fork names.
//! 2. `c4-bin-missing`: every derived bin has its c4-l2 container (the deploy topology is
//!    complete — a bin with no container would build an image that deploys nowhere).
//! 3. `c4-bin-unknown`: every `projector-*` / `graphql-*` / `gateway-*` / `adapter-*` container
//!    matches a derived bin (no phantom scope/role/partner; `projector-common` is unspellable —
//!    the kernel owns no View_*; an `adapter-*` container without its `crates/adapters/` crate
//!    names a partner ACL that does not exist — ADR-20260808-062432).
//!
//! Surfaces (`fo-*`/`bo-*`) and the worker family (`bam` + `worker-*`, ADR-20260808-062933) are
//! exempt from 2–3 by construction: the container list IS their source (a deploy-topology
//! decision, not derivable elsewhere). The `adapter-*` family is NOT exempt: its source is the
//! adapter-crate list, so BOTH directions are checked — a crate without its container (2) and a
//! container without its crate (3).
//!
//! 4. `c4-worker-no-schedule` / `c4-worker-bad-schedule` / `c4-schedule-on-non-worker`: shape
//!    follows cadence (ADR-20260808-062933) — every periodic `worker-*` container declares a
//!    5-field cron `schedule:` (the emitter renders its CronJob from it), and cadence fields on
//!    any other container are refused rather than silently ignored. (A `worker-*` container
//!    whose name has no wiring arm fails GENERATION loudly — `cron_worker_pass`'s unreachable —
//!    same posture as an adapter crate without its composition arm.)

use crate::*;

pub(crate) fn validate_bin_topology(model: &Model, issues: &mut Vec<Issue>) {
    // Skip on models without a c4-l2 container split or without scope folders (test fixtures,
    // transitional layouts): both sides of the comparison must exist for drift to be meaningful.
    let has_containers = model
        .defs
        .get("architecture/c4-l2.yaml")
        .and_then(|v| v.get("containers"))
        .and_then(|v| v.as_mapping())
        .is_some_and(|m| !m.is_empty());
    let topology = bin_topology(model);
    if !has_containers || topology.is_empty() {
        return;
    }
    let c4 = read_c4(model);
    let container_ids: BTreeSet<&str> = c4.containers.iter().map(|c| c.id.as_str()).collect();
    let is_pm_by_actor: BTreeMap<String, bool> =
        actor_scope_links(model).into_iter().map(|(n, pm, _)| (n, pm)).collect();

    // 1. realizes container id == the derived bin name.
    for c in &c4.containers {
        for actor in &c.realizes {
            let Some(is_pm) = is_pm_by_actor.get(actor) else { continue };
            let expected = actor_bin_name(actor, *is_pm);
            if c.id != expected {
                issues.push(err(
                    "c4-bin-name-mismatch",
                    format!("architecture/c4-l2.yaml/containers.{}", c.id),
                    format!(
                        "container '{}' realizes '{}' but the derived bin name is '{}' — one actor, one bin, one spelling (the crate-graph key, crates/bins/ directory and container id must be the same string).",
                        c.id, actor, expected
                    ),
                ));
            }
        }
    }

    // 2. every derived bin (families with an independent source) has its container.
    for b in &topology {
        if matches!(b.family, "surface" | "worker") {
            continue; // derived FROM the container list — cannot be missing from it
        }
        if !container_ids.contains(b.name.as_str()) {
            issues.push(err(
                "c4-bin-missing",
                "architecture/c4-l2.yaml".into(),
                format!(
                    "derived bin '{}' ({}) has no c4-l2 container — it would build an image that deploys nowhere; add the container (ADR-20260807-183024 deploy topology).",
                    b.name, b.family
                ),
            ));
        }
    }

    // 4. worker cadence (ADR-20260808-062933 "shape follows cadence"): every `worker-*`
    //    container declares a 5-field cron `schedule:` (UTC) — absent, the emitter would render
    //    an always-on Deployment for a pass-shaped main that exits immediately (a crash-loop);
    //    and `schedule:`/`suspended:` on any NON-worker container is refused — on other
    //    families they would be silently ignored, which is how a "scheduled" adapter would lie.
    for c in &c4.containers {
        let is_periodic_worker = c.id.starts_with("worker-");
        if is_periodic_worker {
            match c.schedule.as_deref() {
                None => issues.push(err(
                    "c4-worker-no-schedule",
                    format!("architecture/c4-l2.yaml/containers.{}", c.id),
                    format!(
                        "worker container '{}' declares no `schedule:` — a periodic worker's cadence is spec-declared (5-field cron, UTC; ADR-20260808-062933).",
                        c.id
                    ),
                )),
                Some(s) if s.split_whitespace().count() != 5 => issues.push(err(
                    "c4-worker-bad-schedule",
                    format!("architecture/c4-l2.yaml/containers.{}", c.id),
                    format!(
                        "worker container '{}' schedule '{}' is not a 5-field cron expression (minute hour day-of-month month day-of-week).",
                        c.id, s
                    ),
                )),
                Some(_) => {}
            }
        } else if c.schedule.is_some() || c.suspended {
            issues.push(err(
                "c4-schedule-on-non-worker",
                format!("architecture/c4-l2.yaml/containers.{}", c.id),
                format!(
                    "container '{}' declares `schedule:`/`suspended:` but is not a `worker-*` container — cadence shapes only the periodic worker family (ADR-20260808-062933).",
                    c.id
                ),
            ));
        }
    }

    // 5. `deploy_tree:` (#358) — the field that decides WHICH generated tree a container is
    //    emitted into, so a mistake in it is a container that deploys nowhere or twice:
    //    - an unrecognised value is refused rather than defaulted (`DeployTree::Unknown` lands in
    //      neither tree by construction — this is the message that explains why);
    //    - at most ONE `monolith` container, because the overlay names its Deployment/Service
    //      `server` unconditionally; a second one would silently overwrite the first;
    //    - a `monolith` container is never ALSO a derived bin — the two trees must not both claim
    //      the same deployable.
    let mut monoliths: Vec<&str> = Vec::new();
    for c in &c4.containers {
        match &c.deploy_tree {
            DeployTree::Unknown(v) => issues.push(err(
                "c4-deploy-tree-unknown",
                format!("architecture/c4-l2.yaml/containers.{}", c.id),
                format!(
                    "container '{}' declares deploy_tree '{}' — the only generated trees are `bins` (default) and `monolith`; an unknown value is emitted into NEITHER tree, so the container would deploy nowhere.",
                    c.id, v
                ),
            )),
            DeployTree::Monolith => monoliths.push(c.id.as_str()),
            DeployTree::Bins => {}
        }
    }
    if monoliths.len() > 1 {
        issues.push(err(
            "c4-deploy-tree-many-monoliths",
            "architecture/c4-l2.yaml".into(),
            format!(
                "containers {:?} all declare `deploy_tree: monolith` — the overlay emits ONE Deployment/Service named `server`, so the later declaration would silently overwrite the earlier.",
                monoliths
            ),
        ));
    }

    // 3. every projector-*/graphql-*/gateway-* container matches a derived bin.
    let derived: BTreeSet<&str> = topology.iter().map(|b| b.name.as_str()).collect();
    for id in &monoliths {
        if derived.contains(id) {
            issues.push(err(
                "c4-deploy-tree-monolith-is-a-bin",
                format!("architecture/c4-l2.yaml/containers.{id}"),
                format!(
                    "container '{id}' declares `deploy_tree: monolith` but is ALSO a derived bin — both trees would emit a workload for it and GitOps would apply two Deployments with the same purpose."
                ),
            ));
        }
    }
    for c in &c4.containers {
        let generated_family = ["projector-", "graphql-", "gateway-", "adapter-"]
            .iter()
            .any(|p| c.id.starts_with(p));
        if generated_family && !derived.contains(c.id.as_str()) {
            issues.push(err(
                "c4-bin-unknown",
                format!("architecture/c4-l2.yaml/containers.{}", c.id),
                format!(
                    "container '{}' matches a generated bin family but no derived bin — its scope/role/partner does not exist (kernel projector, or an adapter with no crates/adapters/ crate).",
                    c.id
                ),
            ));
        }
    }
}
