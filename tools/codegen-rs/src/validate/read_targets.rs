//! §5c-bis — the read-target OWNERSHIP wall (ADR-20260812-214500).
//!
//! WHY this is a rule family of its own rather than three lines inside §5c: retiring
//! `command_journal` (#500) cost 110 files and ~3,400 deleted lines, because the table had leaked out
//! of its encapsulation into GraphQL resolver bodies. The founder's verdict: *"it should never be used
//! directly because we have to pass through the actor clients that encapsulate the insert"* and
//! *"it's unacceptable"*. Deleting an implementation detail must be a small change, and it is only
//! small if nothing outside infrastructure can NAME the table.
//!
//! Two holes let the leak happen, and each has its own rule here:
//!
//! 1. **The declaration site.** `reference: true` was an unguarded opt-in — the one word that admits a
//!    table as a GraphQL `reads:` target. Every one of the eight live declarations sits in
//!    `database/tables/referential.yaml`, but nothing said it had to: the reason lived in a HEADER
//!    COMMENT (`integration_staging.yaml` literally reads *"NOT a GraphQL `reads` target (no
//!    `reference: true`)"*). A comment is not a gate — one word on `hubrise_connections` and the wall
//!    opened, with a leaked OAuth token as the production symptom.
//!    → `reference-flag-not-a-read-target`, which fires at the DECLARATION, before any `reads:` exists,
//!    so the wall cannot be widened in one PR and walked through in the next.
//!
//! 2. **Transience by omission**, the one that actually let `command_journal` through. A type was
//!    treated as transient simply because it had no `reads:` (`validate::core`'s `transient_types`), so
//!    a query escaped every read-side rule by *leaving a line out* and writing "NON-PROJECTED
//!    (transient)" in prose. The journal resolvers declared no `reads:` at all — the reads-side rule,
//!    however strict, would never have looked at them.
//!    → `transient-type-undeclared-infrastructure`: a query/subscription return type declares either a
//!    read model (`reads:`) or the write-path table it is served from (`readsInfrastructure:`), as a
//!    `$ref` the loader resolves. The next retirement is then a resolved reference, not a repo grep.
//!
//! The DISCRIMINATOR is [`refs::classify`] — the same classifier the §1b ref-kind contract uses — and
//! never a name pattern: `external_%` matches one of the seven infrastructure categories and misses
//! `auth_sessions`, `hubrise_connections`, `inbound_messages`, `mailbox_partitions`,
//! `payment_process_manager`, `slug_reservations` and `domain_events`. It is also never the author's own
//! `staging: true`, which is forgeable by omission, the very defect above.
//!
//! **How far the fail-closed property reaches, exactly.** [`refs::read_target_kind`]'s match is
//! exhaustive over `Kind`, so a new **`Kind`** does not compile until it has been classified — that
//! part is a compile-time guarantee. A new **catalog FILE** is weaker: [`refs::classify`]'s `_ => None`
//! arm accepts an unknown `database/tables/*.yaml` with no code change, and it then fails closed at
//! VALIDATE (`ref-kind-unknown` on any `$ref` into it, plus `reads-unknown-view`) rather than at
//! compile. Both directions are closed; only one of them is the compiler. Do not let this get restated
//! as "a new category does not compile" — `reservations.yaml` had no arm for months and built fine.
//!
//! Out of scope on purpose: `architecture/c4-l3.yaml`'s `components.*.reads`, which is the CORRECT home
//! for infrastructure readers (the mailbox worker, the ACLs, the process managers) — banning it there
//! would delete the only place a mailbox reader can be declared.

use crate::*;

/// Every table declared under `database/tables/*.yaml`, mapped to `(file, kind)` by the §1b classifier.
/// Shared with §18 (validate/databases.rs) and the databases inventory emitter — one classification,
/// every consumer.
pub(crate) fn table_kinds(model: &Model) -> BTreeMap<String, (String, refs::Kind)> {
    let handled = BTreeSet::new();
    let mut out = BTreeMap::new();
    for (file, val) in model.defs.iter().filter(|(k, _)| k.starts_with("database/tables/")) {
        if let Value::Mapping(m) = val {
            for (tk, tv) in m {
                if let Some(n) = tk.as_str() {
                    if let Some(k) = refs::classify(file, &[n.to_string()], tv, &handled) {
                        out.insert(n.to_string(), (file.clone(), k));
                    }
                }
            }
        }
    }
    out
}

/// Returns the read-model names some api type actually BOUND — what §5c's `view-unread` reader check
/// consumes downstream. Everything else this function decides, it decides by pushing an issue.
pub(crate) fn validate_read_targets(
    model: &Model,
    api: &Api,
    views: &[SqlView],
    cov: &mut Coverage,
    issues: &mut Vec<Issue>,
) -> BTreeSet<String> {
    let kinds = table_kinds(model);

    // ── 1. the declaration site: `reference: true` is the READ side's opt-in, nobody else's ──────
    let mut legal: BTreeSet<String> = views.iter().map(|v| v.name.clone()).collect();
    for (name, (file, kind)) in &kinds {
        let opted_in = model
            .defs
            .get(file)
            .and_then(|v| v.get(name))
            .and_then(|tv| tv.get("reference"))
            .and_then(|b| b.as_bool())
            == Some(true);
        if !opted_in {
            continue;
        }
        match refs::read_target_kind(*kind) {
            Some(true) => {
                legal.insert(name.clone());
            }
            _ => issues.push(err(
                "reference-flag-not-a-read-target",
                format!("{}/{}", file, name),
                format!(
                    "'{}' is a {} — infrastructure/adapter-owned write-path state — so `reference: true` is not \
                     available to it. That flag is the SEEDED REFERENCE tables' opt-in \
                     (database/tables/referential.yaml) and its only effect is to admit a table as a GraphQL \
                     `reads:` target; on this table it would expose write-path state (a credential, a mailbox \
                     row, a saga row) to the API. A query genuinely served from here is declared with \
                     `readsInfrastructure:` on its transient output type instead (ADR-20260812-214500).",
                    name,
                    kind.name()
                ),
            )),
        }
    }

    // ── 2. the binding site ──────────────────────────────────────────────────────────────────────
    let mut bound: BTreeSet<String> = BTreeSet::new();
    for t in &api.types {
        for v in &t.reads {
            cov.reads_links += 1;
            bound.insert(v.clone());
            // An infrastructure table named as a read model gets its OWN diagnosis rather than
            // "unknown view": the name exists, and the reason it is refused is ownership, not a typo.
            // Independent of `reference: true` — this fires whether or not the flag was planted.
            if let Some((file, kind)) = kinds.get(v.as_str()) {
                if refs::read_target_kind(*kind) == Some(false) {
                    issues.push(err(
                        "reads-infrastructure-owned",
                        format!("api.yaml/types.{}", t.name),
                        format!(
                            "reads '{}' — a {} declared in {} — which is infrastructure/adapter-owned write-path \
                             state, never a business read model. A GraphQL type binds to a projection view, a \
                             projection table, or a `reference: true` referential table, so that retiring an \
                             infrastructure table stays an implementation detail. If this query really is served \
                             from the write path, make it a transient type with `readsInfrastructure:` \
                             (ADR-20260812-214500).",
                            v,
                            kind.name(),
                            file
                        ),
                    ));
                    continue;
                }
            }
            if !legal.contains(v.as_str()) {
                issues.push(err(
                    "reads-unknown-view",
                    format!("api.yaml/types.{}", t.name),
                    format!("reads references unknown view '{}'.", v),
                ));
            }
        }
        // `readsInfrastructure:` is the transient declaration. §1b's contract row proves each entry is
        // an infrastructure KIND (a projection there is an error — it belongs under `reads:`); the one
        // remaining misuse is declaring both, which would be an undeclared join across the CQRS seam.
        if !t.reads_infrastructure.is_empty() && !t.reads.is_empty() {
            issues.push(err(
                "reads-infrastructure-with-read-model",
                format!("api.yaml/types.{}", t.name),
                format!(
                    "declares BOTH `reads:` and `readsInfrastructure: [{}]` — a type is either a read-model \
                     projection or a transient write-path view, never both.",
                    t.reads_infrastructure.join(", ")
                ),
            ));
        }
    }

    // ── 3. transience is a DECLARATION, never an omission ────────────────────────────────────────
    // Nested output types reached through a parent's `reads:` legitimately declare neither; the
    // obligation lands on the type a QUERY or SUBSCRIPTION returns, which is where a store is touched.
    //
    // MUTATION return types are therefore EXEMPT, and that is deliberate rather than an oversight.
    // Under acceptance-first (ADR-20260720-015500) every mutation returns the one shared
    // `MutationAcceptance`, which is built in memory from the enqueue result — it is the echo of a
    // WRITE, not a view of a store, and the only table behind it (`inbound_messages`) is reached
    // through `actor_client`'s `Mailbox` door, whose `MailboxAccess(pub(crate) ())` witness the
    // compiler already enforces. Demanding a `readsInfrastructure:` there would be false: it would
    // declare a read that does not happen, and it would name a table the resolver cannot touch
    // directly anyway. If a mutation ever returns a store-backed type, that is a new decision (it
    // breaks "results ARE reads") and it gets this rule extended in the same change.
    {
        let returned: BTreeSet<&str> = api
            .queries
            .iter()
            .chain(api.subscriptions.iter())
            .map(|q| q.returns_type.as_str())
            .collect();
        for t in &api.types {
            if !t.reads.is_empty() || !t.reads_infrastructure.is_empty() || !returned.contains(t.name.as_str()) {
                continue;
            }
            issues.push(err(
                "transient-type-undeclared-infrastructure",
                format!("api.yaml/types.{}", t.name),
                format!(
                    "'{}' is returned by a query/subscription but declares neither `reads:` (a projection view, a \
                     projection table, or a `reference: true` table) nor `readsInfrastructure:` (the write-path \
                     table a TRANSIENT type is served from). Omitting both is not a declaration of transience — it \
                     silently exempts the type from every read-target ownership rule, which is how the journal \
                     leaked into resolver bodies and made its retirement a 110-file change (ADR-20260812-214500).",
                    t.name
                ),
            ));
        }
    }

    // ── 4. every entry is a `$ref` (ADR-20260811-014129 D2) ──────────────────────────────────────
    // A bare string is collected by `api::name_list` but INVISIBLE to the §1b refs walker, which walks
    // `$ref` nodes only — the #413 defect class ("silently invisible everywhere"). Here it would be the
    // bypass around every rule above, so it is refused at the syntax level.
    if let Some(m) = model.defs.get("api.yaml").and_then(|v| v.get("types")).and_then(|v| v.as_mapping()) {
        for (tk, tv) in m {
            let tname = tk.as_str().unwrap_or("?");
            for key in ["reads", "readsInfrastructure"] {
                for entry in tv.get(key).and_then(|v| v.as_sequence()).into_iter().flatten() {
                    if entry.get("$ref").and_then(|r| r.as_str()).is_some() {
                        continue;
                    }
                    issues.push(err(
                        "reads-not-a-ref",
                        format!("api.yaml/types.{}", tname),
                        format!(
                            "`{}` entry '{}' is a bare name; it must be a $ref into \
                             database/projection_views.yaml or database/tables/*.yaml. A bare name is invisible to \
                             the ref-kind contract (§1b collects $ref nodes only), so it would walk straight past \
                             every read-target ownership check.",
                            key,
                            entry.as_str().unwrap_or("?")
                        ),
                    ));
                }
            }
        }
    }

    bound
}
