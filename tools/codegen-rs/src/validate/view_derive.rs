// ─── `derive:` arm values are one of three forms (#639 part C step 3-i, ADR-20260904-015903 §3) ──
//
// A `derive:` map (status-from-event-type columns) maps an event type to the column's value: a
// literal (`PENDING`), a payload extraction (`{ from: status }`) or — new with 3-i — an explicit
// `null` that RESETS the column. `emit/sql.rs#parse_col` used to `continue` past anything else, so
// a value the grammar did not know (a YAML null included) vanished from the CASE with the gate
// green, and a "clearing" event left the last value standing. The parser stays total (a skip);
// this rule makes the skip an ERROR, so no validated corpus ever reaches it:
//
//   ERROR `view-derive-value-unknown`  — an arm value that is neither a string, `{ from: <prop> }`
//                                         nor `null`; names the view, the column and the event.
//   ERROR `view-derive-null-not-nullable` — a `null` arm on a column not declared `nullable: true`
//                                         (a NOT NULL column cannot be reset; the emitters would
//                                         panic at generation, which is later than here).

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
                for (evt, dv) in dm {
                    let evt = evt.as_str().unwrap_or("?");
                    let at = format!("{label}/{vname}/columns/{cname}/derive/{evt}");
                    match derive_val(dv) {
                        None => issues.push(err(
                            "view-derive-value-unknown",
                            at,
                            format!(
                                "derive arm '{evt}' of column '{cname}' on {vname} is neither a literal value, \
                                 `{{ from: <property> }}` nor `null` — it would be silently dropped from the fold."
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
                        Some(_) => {}
                    }
                }
            }
        }
    }
}
