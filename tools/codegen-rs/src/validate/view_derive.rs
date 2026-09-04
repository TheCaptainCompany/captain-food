// ─── `derive:` arm values are FOUR forms (#639 part C step 3-i/3-ii, ADR-20260904-015903 §3) ─────
//
// A `derive:` map (status-from-event-type columns) maps an event type to the column's value: a
// literal (`PENDING`), a payload extraction (`{ from: status }`), an explicit `null` that RESETS
// the column (3-i), or — new with 3-ii, "the grammar extension of A.3, mirrored" — a value→value MAP
// keyed off a DIFFERENT field of the same event (`{ from: foodLocation, map: { RETURNED_TO_RESTAURANT:
// PENDING, WITH_RIDER: FAILED, … } }`): the column's value depends on that OTHER field's value, not a
// straight copy (a custody-keyed status arm cannot be a `Payload` — the field's own scalar and the
// column's are different enums). `emit/sql.rs#parse_col` used to `continue` past anything else, so a
// value the grammar did not know (a YAML null included) vanished from the CASE with the gate green,
// and a "clearing" event left the last value standing. The parser stays total (a skip); these rules
// make the skip (and a malformed map) an ERROR, so no validated corpus ever reaches it:
//
//   ERROR `view-derive-value-unknown`  — an arm value that is neither a string, `{ from: <prop> }`,
//                                         `null` nor `{ from: <prop>, map: {...} }`; names the view,
//                                         the column and the event.
//   ERROR `view-derive-null-not-nullable` — a `null` arm on a column not declared `nullable: true`
//                                         (a NOT NULL column cannot be reset; the emitters would
//                                         panic at generation, which is later than here).
//   ERROR `view-derive-map-field-not-enum` — a `Mapped` arm's `from` field does not exist on the
//                                         event's payload, or does not `$ref` a scalars.yaml enum —
//                                         there is nothing to key the map's values off.
//   ERROR `view-derive-map-key-unknown` — a `Mapped` arm's map key is not a member of the referenced
//                                         field's own enum — it could never match at runtime.
//   ERROR `view-derive-map-value-unknown` — a `Mapped` arm's map VALUE is not a member of the
//                                         COLUMN's own enum (mirrors the `Lit` check).
//   ERROR `view-derive-map-not-exhaustive` — a `Mapped` arm's map does not cover every member of the
//                                         referenced field's enum: the SQL CASE would silently fall
//                                         through to NULL for the missing value(s), and the sibling
//                                         Rust match (a materialized table's projector) would not
//                                         compile at all — the DSL must be exhaustive, not the arm.

use crate::*;

pub(crate) fn check_view_derive_values(model: &Model, issues: &mut Vec<Issue>) {
    for (file, label) in [
        ("database/projection_views.yaml", "projection_views.yaml"),
        ("database/tables/projection_tables.yaml", "projection_tables.yaml"),
    ] {
        let Some(Value::Mapping(views)) = model.defs.get(file) else { continue };
        for (vname, view) in views {
            let Some(vname) = vname.as_str() else { continue };
            let Some(cols) = view.get("columns").and_then(|c| c.as_mapping()) else { continue };
            for (cname, col) in cols {
                let Some(cname) = cname.as_str() else { continue };
                let Some(dm) = col.get("derive").and_then(|d| d.as_mapping()) else { continue };
                let nullable = col.get("nullable").and_then(|x| x.as_bool()) == Some(true);
                // The column's own enum (for `Lit`/`Mapped` value checks) — explicit `type:` or, if
                // absent, whatever `derive_type` would infer from `from:` (mirrors emit/sql.rs).
                let col_ty = if let Some(t) = col.get("type") {
                    column_type_explicit(t)
                } else {
                    let from: Vec<String> = col
                        .get("from")
                        .and_then(|f| f.as_sequence())
                        .map(|s| {
                            s.iter()
                                .filter_map(|it| it.get("$ref").and_then(|r| r.as_str()).map(|x| x.to_string()))
                                .collect()
                        })
                        .unwrap_or_default();
                    derive_type(&from, model.defs.get("events.yaml").unwrap_or(&Value::Null))
                };
                let col_enum = enum_values(model, &col_ty);
                for (evt, dv) in dm {
                    let evt = evt.as_str().unwrap_or("?");
                    let at = format!("{label}/{vname}/columns/{cname}/derive/{evt}");
                    match derive_val(dv) {
                        None => issues.push(err(
                            "view-derive-value-unknown",
                            at,
                            format!(
                                "derive arm '{evt}' of column '{cname}' on {vname} is neither a literal value, \
                                 `{{ from: <property> }}`, `null` nor `{{ from: <property>, map: {{...}} }}` — it \
                                 would be silently dropped from the fold."
                            ),
                        )),
                        Some(DeriveVal::Null) if !nullable => issues.push(err(
                            "view-derive-null-not-nullable",
                            at,
                            format!(
                                "derive arm '{evt}' resets column '{cname}' on {vname} to null, but the column is not \
                                 `nullable: true`."
                            ),
                        )),
                        Some(DeriveVal::Mapped(field, pairs)) => {
                            let field_scalar = payload_field_scalar(model, evt, &field);
                            let field_enum = field_scalar.as_deref().and_then(|s| enum_values(model, s));
                            match &field_enum {
                                None => issues.push(err(
                                    "view-derive-map-field-not-enum",
                                    at.clone(),
                                    format!(
                                        "derive arm '{evt}' of column '{cname}' on {vname} maps `from: {field}`, which does not \
                                         exist on events.yaml#/{evt}'s payload or does not $ref a scalars.yaml enum."
                                    ),
                                )),
                                Some(vals) => {
                                    for (k, v) in &pairs {
                                        if !vals.iter().any(|x| x == k) {
                                            issues.push(err(
                                                "view-derive-map-key-unknown",
                                                at.clone(),
                                                format!(
                                                    "derive arm '{evt}' of column '{cname}' on {vname}: map key '{k}' is not a \
                                                     member of {} ({}).",
                                                    field_scalar.clone().unwrap_or_default(),
                                                    vals.join(", ")
                                                ),
                                            ));
                                        }
                                        if let Some(cvals) = &col_enum {
                                            if !cvals.iter().any(|x| x == v) {
                                                issues.push(err(
                                                    "view-derive-map-value-unknown",
                                                    at.clone(),
                                                    format!(
                                                        "derive arm '{evt}' of column '{cname}' on {vname}: map value '{v}' (for key \
                                                         '{k}') is not a member of {col_ty} ({}).",
                                                        cvals.join(", ")
                                                    ),
                                                ));
                                            }
                                        }
                                    }
                                    let covered: BTreeSet<&str> = pairs.iter().map(|(k, _)| k.as_str()).collect();
                                    let missing: Vec<&String> = vals.iter().filter(|v| !covered.contains(v.as_str())).collect();
                                    if !missing.is_empty() {
                                        issues.push(err(
                                            "view-derive-map-not-exhaustive",
                                            at,
                                            format!(
                                                "derive arm '{evt}' of column '{cname}' on {vname} does not map every member of {} \
                                                 — missing: {}.",
                                                field_scalar.unwrap_or_default(),
                                                missing.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
                                            ),
                                        ));
                                    }
                                }
                            }
                        }
                        Some(_) => {}
                    }
                }
            }
        }
    }
}
