//! THE EXECUTABLE DEPENDENCY RULE (ADR-20260730-234918): `actor_runtime` is extraction-ready —
//! ZERO Captain.Food crates in its dependency list, so lifting it into another product is a
//! `git mv`, not a rewrite. Prose in a Cargo.toml comment can be ignored; this test cannot.

#[test]
fn actor_runtime_depends_on_no_captain_food_crate() {
    let manifest = include_str!("../Cargo.toml");
    let deps = manifest
        .split("[dependencies]")
        .nth(1)
        .expect("Cargo.toml has a [dependencies] section");
    for forbidden in ["domain", "application", "infrastructure", "shared_types", "telemetry"] {
        let needle = format!("\n{forbidden} ");
        let needle_eq = format!("\n{forbidden}=");
        assert!(
            !deps.contains(&needle) && !deps.contains(&needle_eq),
            "actor_runtime must stay extraction-ready: found workspace crate `{forbidden}` in its \
             dependencies (ADR-20260730-234918)"
        );
    }
    assert!(!manifest.contains("path = \"../"), "no path dependencies into the workspace at all");
}
