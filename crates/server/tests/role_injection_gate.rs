//! **A gate at the one seam the compiler cannot reach** (ADR-20260803-234035: compiler first, a
//! check is the fallback) — #639 part B.
//!
//! The role guard reads a `crate::auth::ActingRole` from the GraphQL context, and that type cannot
//! be forged: it comes from `Principal::acting_role` or it does not exist. What the type system
//! CANNOT see is the injection, because `async_graphql::Data::insert` is keyed by `TypeId` over
//! `Any` — putting a bare `RequestRole` in the bag is not a compile error, not a warning, and not a
//! runtime failure. It reads as `data_opt::<ActingRole>() == None`, which fails closed to PUBLIC.
//!
//! **Failing closed is the right default and the wrong thing for a test.** A refusal test whose
//! role never arrives still passes — PUBLIC is refused too — so it goes green while asserting
//! nothing about the role it names. That is not hypothetical: the #639 sweep converted 45
//! literal `.data(RequestRole::X)` call sites and missed three variable-bound `.data(role)` ones
//! inside `for role in …` loops, and all three suites stayed green with every iteration collapsed
//! to PUBLIC. `mailbox_lanes.rs` records the same loop being caught covering 4 of 6 roles on #536;
//! the miss had taken it to 0 of 6.
//!
//! So this asserts the property a grep would have to be run by hand to check: **no test in this
//! crate hands the schema a role that is not the witness.** Two spellings are refused by name
//! because both were written here and one shipped. It cannot catch every alias — a `.data(x)` where
//! `x: RequestRole` under some other name still slips — which is why the guard names the
//! recurrence class rather than claiming completeness.
//!
//! Seen RED before it was trusted, by restoring `.data(role)` at `mailbox_lanes.rs:302`; the
//! recursive walk seen RED by planting `.data(RequestRole::Admin)` in a `tests/<sub>/` file the
//! top-level walk could not see.

use std::path::{Path, PathBuf};

/// The two spellings that put a bare `RequestRole` into the execution context. `acting(role)` and
/// `acting(RequestRole::X)` are the correct forms and contain neither.
const FORBIDDEN: [&str; 2] = [".data(role)", ".data(RequestRole::"];

/// Every `.rs` under `dir`, RECURSIVELY, in a stable order. The first cut walked `read_dir`
/// once and stopped at the top level (#639 part C step 2b found it): a suite in `tests/<sub>/`
/// — a shared `common/` module, or a file someone tidied into a folder — was simply not scanned,
/// and because the `scanned >= 8` floor was met by the top level alone the gate stayed GREEN
/// while blind to it. A gate that only looks where the offence used to be is a gate that
/// documents the last incident rather than preventing the next one.
fn rust_sources_under(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("{} is readable: {e}", dir.display()))
        .filter_map(Result::ok)
        .map(|e| e.path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            rust_sources_under(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn no_test_in_this_crate_injects_a_bare_request_role_into_the_schema() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let mut offences: Vec<String> = Vec::new();
    let mut scanned = 0usize;

    let mut entries: Vec<PathBuf> = Vec::new();
    rust_sources_under(&dir, &mut entries);

    for path in &entries {
        // This file names the forbidden spellings in order to forbid them.
        if path.file_name().is_some_and(|n| n == "role_injection_gate.rs") {
            continue;
        }
        let src = std::fs::read_to_string(path).expect("test source is readable");
        scanned += 1;
        let shown = path.strip_prefix(&dir).unwrap_or(path).display();
        for (n, line) in src.lines().enumerate() {
            for needle in FORBIDDEN {
                if line.contains(needle) {
                    offences.push(format!("{shown}:{}: {}", n + 1, line.trim()));
                }
            }
        }
    }

    // A gate that scans nothing passes forever. Assert it found the suite it is guarding.
    assert!(
        scanned >= 8,
        "expected to scan the server integration suites, scanned only {scanned} file(s) in {} — \
         if the tests moved, move this gate with them rather than letting it pass on an empty set",
        dir.display()
    );

    assert!(
        offences.is_empty(),
        "a bare `RequestRole` in the GraphQL context is INVISIBLE to the role guard, which reads \
         `ActingRole` and fails closed to PUBLIC — so the test below is green while asserting \
         nothing about the role it names.\n\nUse the suite's `acting(..)` helper, which mints the \
         witness from a `Principal` bound to that role:\n\n  {}\n",
        offences.join("\n  ")
    );
}
