// ─── pm-sends-human-only-command (#639 part C step 4-i, ADR-20260904-081527 §6/§8) ───────────────
//
// A THIRD layer of "a human decides", beside the door's `roles: [ADMIN]` (§27/api.yaml) and the
// aggregate's own `requires: acting` declaration: no `processmanager.yaml` `sends:` may name a
// command whose actors.yaml `receives:` entry declares `requires: acting` with NO `EXTERNAL` key —
// that is exactly the shape RestrictRider/ReinstateRider declare (`{ ADMIN: any }`), and a
// process-manager `sends:` reaching it would let a saga impersonate the human the door exists to
// require. Before this rule, a `sends: RestrictRider` planted in any processmanager.yaml validated
// clean — the mutant this rule exists to kill (M7 on the #639 4-i card).
//
//   ERROR `pm-sends-human-only-command` — a processmanager.yaml `sends:` names a command whose
//   `requires: acting` (actors.yaml) carries no `EXTERNAL` key.

use crate::*;

/// Every command name whose actors.yaml `receives:` entry declares `requires: acting` with NO
/// `EXTERNAL` key — the closed "human-only" set, derived from the spec, never hand-listed.
fn human_only_commands(model: &Model) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let Some(Value::Mapping(actors)) = model.defs.get("actors.yaml") else { return out };
    for (k, node) in actors {
        if k.as_str() == Some("principals") {
            continue;
        }
        if node.get("type").and_then(|x| x.as_str()) != Some("aggregate") {
            continue;
        }
        for entry in node.get("receives").and_then(|r| r.as_sequence()).into_iter().flatten() {
            let Some(acting) = entry.get("requires").and_then(|r| r.get("acting")).and_then(|a| a.as_mapping())
            else {
                continue;
            };
            let has_external = acting.keys().any(|k| k.as_str() == Some("EXTERNAL"));
            if has_external {
                continue;
            }
            if let Some(cmd) = entry
                .get("message")
                .and_then(|m| m.get("$ref"))
                .and_then(|r| r.as_str())
                .and_then(ref_name)
            {
                out.insert(cmd);
            }
        }
    }
    out
}

pub(crate) fn check_pm_sends_human_only_command(model: &Model, issues: &mut Vec<Issue>) {
    let human_only = human_only_commands(model);
    if human_only.is_empty() {
        return;
    }
    let Some(Value::Mapping(pms)) = model.defs.get("processmanager.yaml") else { return };
    for (pk, pm) in pms {
        let Some(pname) = pk.as_str() else { continue };
        for (i, leg) in pm.get("receives").and_then(|r| r.as_sequence()).into_iter().flatten().enumerate() {
            let site_base = format!("processmanager.yaml/{}.receives[{}]", pname, i);
            for (j, s) in leg.get("sends").and_then(|x| x.as_sequence()).into_iter().flatten().enumerate() {
                let Some(cmd) = s
                    .get("command")
                    .and_then(|x| x.get("$ref"))
                    .and_then(|x| x.as_str())
                    .and_then(ref_name)
                else {
                    continue;
                };
                if human_only.contains(&cmd) {
                    issues.push(err(
                        "pm-sends-human-only-command",
                        format!("{}.sends[{}]", site_base, j),
                        format!(
                            "process manager '{}' sends '{}', whose actors.yaml `requires: acting` carries no \
                             EXTERNAL key -- a human-only door (#639 part C step 4-i, ADR-20260904-081527 §6/§8). \
                             No saga may impersonate the human this door requires; route the decision to an \
                             admin surface instead.",
                            pname, cmd
                        ),
                    ));
                }
            }
        }
    }
}
