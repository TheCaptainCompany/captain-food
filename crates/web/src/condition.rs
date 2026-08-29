//! The corpus-exact SDUI condition evaluator (#472).
//!
//! The screens DSL declares presentation conditions in three prop spellings —
//! `visible_when`, `disabled_when` and `conditional_section`'s `condition:` — and until #472 the
//! renderer consumed NONE of them: every declared condition rendered as if true (a dead control
//! stayed live). This module is the ONE evaluator all three spellings route through.
//!
//! **Corpus-exact, deliberately NOT a general expression language.** The grammar is exactly the
//! shapes the checked-in specs use (verified against `generated/screens.rs` at the #472 briefing,
//! plus the two shapes the corpus audit added — `!= null` and the `.length` pseudo-segment):
//!
//!   * bare-path truthiness              — `passkey_available`, `reclamation.overdue`
//!   * negation                          — `!resend_available`
//!   * integer compare (`>`, `==`, `!=`) — `cart_item_count > 0`, `cart.lines.length == 0`
//!   * string equality vs a QUOTED literal — `order.serviceType == 'DELIVERY'`,
//!     `search_input.value == ""`
//!   * null comparison                   — `item.errorCode != null`
//!   * `in`-list of quoted literals      — `resolution.value in ['PARTIAL_REFUND','GOODWILL_CREDIT']`
//!
//! The single-quoted token form is the same one `tools/codegen-rs/src/validate/core.rs`
//! (`collect_status_tokens`) tokenises — the renderer's grammar and the validator's must agree.
//!
//! **An unknown construct fails LOUDLY — never silently-true** (`a >= b` is a [`ParseError`], and
//! the renderer pairs the fail-closed non-render with an auditable DOM marker). **A condition over
//! MISSING data is NOT an unknown construct** (briefing decision): [`Condition::eval`] returns
//! `None` (unevaluatable) and the caller fails CLOSED — `visible_when` hides, `disabled_when`
//! disables. Never default-visible/enabled. The one deliberate exception: `== null` / `!= null`
//! treat a missing path as null — "no value" is exactly what those expressions ask about.
//!
//! Compiler-first (ADR-20260803-234035): `Condition`'s internals are private and `parse` is its
//! only constructor, so there is no way to obtain an answer from a condition expression that did
//! not go through this grammar.
//!
//! `variant_when` (specs/screens/system.yaml) is OUT of this module's scope — stated at the #472
//! briefing (evans), left for its own chunk.

use serde_json::Value;

use crate::generated::screens::{Node, PropValue};

/// A condition expression outside the corpus grammar. Loud by contract: the renderer never maps
/// this to "true", and the corpus gate (`condition_defects` + its test) keeps checked-in specs out
/// of this branch entirely.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    /// What the parser objected to — diagnostics only, never rendered to a customer.
    pub what: &'static str,
}

#[derive(Debug, Clone, PartialEq)]
enum Rhs {
    Null,
    Str(String),
    Int(i64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Eq,
    Ne,
    Gt,
}

#[derive(Debug, Clone, PartialEq)]
enum Expr {
    /// `path` / `!path` — truthiness of the resolved value.
    Truthy { path: String, negated: bool },
    /// `path <op> <rhs>` — comparison against a literal.
    Compare { path: String, op: Op, rhs: Rhs },
    /// `path in ['A','B']` — membership in a closed literal list.
    InList { path: String, items: Vec<String> },
}

/// One parsed condition. `parse` is the only constructor (private field) — the render path cannot
/// consult an expression the grammar did not accept.
#[derive(Debug, Clone, PartialEq)]
pub struct Condition {
    expr: Expr,
}

/// A dotted path of plain identifier segments — the only shape the refs/data walkers can see.
fn parse_path(s: &str) -> Result<String, ParseError> {
    let s = s.trim();
    let ok = !s.is_empty()
        && !s.starts_with('.')
        && !s.ends_with('.')
        && !s.contains("..")
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.');
    if ok {
        Ok(s.to_string())
    } else {
        Err(ParseError { what: "not a dotted identifier path" })
    }
}

/// A quoted string literal (single or double quotes — both occur in the corpus).
fn parse_quoted(s: &str) -> Option<String> {
    let s = s.trim();
    for q in ['\'', '"'] {
        if s.len() >= 2 && s.starts_with(q) && s.ends_with(q) {
            let inner = &s[1..s.len() - 1];
            if !inner.contains(q) {
                return Some(inner.to_string());
            }
        }
    }
    None
}

fn parse_rhs(s: &str) -> Result<Rhs, ParseError> {
    let s = s.trim();
    if s == "null" {
        return Ok(Rhs::Null);
    }
    if let Some(lit) = parse_quoted(s) {
        return Ok(Rhs::Str(lit));
    }
    if let Ok(n) = s.parse::<i64>() {
        return Ok(Rhs::Int(n));
    }
    // A bare identifier RHS is deliberately NOT grammar: `a == b` where `b` is a path is
    // inexpressible, and a bare enum token must be quoted (the spec's `selected_delivery_mode ==
    // 'delivery'` form). Loud, so the corpus gate catches the unquoted spelling.
    Err(ParseError { what: "right-hand side is not null, a quoted literal or an integer" })
}

impl Condition {
    /// Parse one corpus-grammar expression. Anything else — `>=`, `<`, `&&`, a bare-identifier
    /// right-hand side, an unquoted list item — is a loud [`ParseError`].
    pub fn parse(raw: &str) -> Result<Condition, ParseError> {
        let s = raw.trim();
        if s.is_empty() {
            return Err(ParseError { what: "empty expression" });
        }
        if let Some(rest) = s.strip_prefix('!') {
            return Ok(Condition { expr: Expr::Truthy { path: parse_path(rest)?, negated: true } });
        }
        if let Some((lhs, rest)) = s.split_once(" in ") {
            let rest = rest.trim();
            let inner = rest
                .strip_prefix('[')
                .and_then(|r| r.strip_suffix(']'))
                .ok_or(ParseError { what: "`in` needs a [...] list" })?;
            let mut items = Vec::new();
            for item in inner.split(',') {
                items.push(
                    parse_quoted(item)
                        .ok_or(ParseError { what: "`in` list items must be quoted literals" })?,
                );
            }
            if items.is_empty() {
                return Err(ParseError { what: "`in` list is empty" });
            }
            return Ok(Condition { expr: Expr::InList { path: parse_path(lhs)?, items } });
        }
        for (token, op) in [("==", Op::Eq), ("!=", Op::Ne), (">", Op::Gt)] {
            if let Some((lhs, rhs)) = s.split_once(token) {
                let rhs = parse_rhs(rhs)?;
                if op == Op::Gt && !matches!(rhs, Rhs::Int(_)) {
                    return Err(ParseError { what: "`>` compares integers only" });
                }
                return Ok(Condition { expr: Expr::Compare { path: parse_path(lhs)?, op, rhs } });
            }
        }
        Ok(Condition { expr: Expr::Truthy { path: parse_path(s)?, negated: false } })
    }

    /// Evaluate over resolved data. `lookup` resolves a dotted path to its JSON value (`None` =
    /// unresolved/missing). Returns `None` when the expression is UNEVALUATABLE over the data at
    /// hand — the caller fails CLOSED (hidden / disabled), per the #472 briefing decision.
    pub fn eval(&self, lookup: &dyn Fn(&str) -> Option<Value>) -> Option<bool> {
        match &self.expr {
            Expr::Truthy { path, negated } => {
                let b = truthiness(&resolve_path(path, lookup)?)?;
                Some(if *negated { !b } else { b })
            }
            Expr::Compare { path, op, rhs: Rhs::Null } => {
                // Missing IS null here: "no value" is what a null comparison asks about.
                let is_null = resolve_path(path, lookup).map_or(true, |v| v.is_null());
                match op {
                    Op::Eq => Some(is_null),
                    Op::Ne => Some(!is_null),
                    Op::Gt => None, // unreachable by parse (`>` is integer-only)
                }
            }
            Expr::Compare { path, op, rhs: Rhs::Str(lit) } => {
                let v = resolve_path(path, lookup)?;
                let s = v.as_str()?;
                match op {
                    Op::Eq => Some(s == lit),
                    Op::Ne => Some(s != lit),
                    Op::Gt => None, // unreachable by parse
                }
            }
            Expr::Compare { path, op, rhs: Rhs::Int(rhs) } => {
                let v = resolve_path(path, lookup)?;
                let n = v.as_i64()?;
                match op {
                    Op::Eq => Some(n == *rhs),
                    Op::Ne => Some(n != *rhs),
                    Op::Gt => Some(n > *rhs),
                }
            }
            Expr::InList { path, items } => {
                let v = resolve_path(path, lookup)?;
                let s = v.as_str()?;
                Some(items.iter().any(|i| i == s))
            }
        }
    }
}

/// Resolve a path, honouring the `.length` pseudo-segment: when the FULL path does not resolve
/// but its parent is an array or string, `.length` reads that length (the corpus's
/// `cart.lines.length`, `recent_searches.length`).
fn resolve_path(path: &str, lookup: &dyn Fn(&str) -> Option<Value>) -> Option<Value> {
    if let Some(v) = lookup(path) {
        return Some(v);
    }
    let parent = path.strip_suffix(".length")?;
    match lookup(parent)? {
        Value::Array(a) => Some(Value::from(a.len() as i64)),
        Value::String(s) => Some(Value::from(s.chars().count() as i64)),
        _ => None,
    }
}

/// JSON truthiness for bare-path conditions. `null` is falsey (an ANSWERED "nothing"); a MISSING
/// path never reaches here (unevaluatable — the caller fails closed).
fn truthiness(v: &Value) -> Option<bool> {
    match v {
        Value::Null => Some(false),
        Value::Bool(b) => Some(*b),
        Value::Number(n) => Some(n.as_f64().map(|f| f != 0.0).unwrap_or(false)),
        Value::String(s) => Some(!s.is_empty()),
        Value::Array(a) => Some(!a.is_empty()),
        Value::Object(_) => Some(true),
    }
}

/// Whether a flattened prop KEY declares a condition — exact spelling or a dotted suffix
/// (`rows.1.visible_when`, `item_badge.visible_when`, `if_true.0.visible_when`, …).
pub fn is_condition_prop(key: &str) -> bool {
    matches!(key, "visible_when" | "disabled_when" | "condition")
        || key.ends_with(".visible_when")
        || key.ends_with(".disabled_when")
        || key.ends_with(".condition")
}

/// One condition prop the grammar rejects — the corpus gate's finding, and the router's
/// `condition_unparseable` degradation (static per screen: parseability never depends on data).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConditionDefect {
    /// The component kind's `data-c` type string.
    pub component: &'static str,
    /// The offending prop key.
    pub prop: &'static str,
    /// The expression text (empty for a non-literal prop value).
    pub expr: &'static str,
    pub what: &'static str,
}

/// Walk a generated node tree and report every condition prop that does not parse. Empty on every
/// checked-in screen — the corpus-walk test in this module is the gate that keeps it so.
pub fn condition_defects(nodes: &[Node]) -> Vec<ConditionDefect> {
    let mut out = Vec::new();
    collect_defects(nodes, &mut out);
    out
}

fn collect_defects(nodes: &[Node], out: &mut Vec<ConditionDefect>) {
    for node in nodes {
        for (key, value) in node.props {
            if !is_condition_prop(key) {
                continue;
            }
            match value {
                PropValue::Text(expr) => {
                    if let Err(e) = Condition::parse(expr) {
                        out.push(ConditionDefect {
                            component: node.kind.as_str(),
                            prop: key,
                            expr,
                            what: e.what,
                        });
                    }
                }
                // A condition must be a literal expression: a `{{ binding }}` or i18n key here is
                // invisible to this grammar and to the validator's tokeniser alike.
                PropValue::Binding(_) | PropValue::I18n(_) => out.push(ConditionDefect {
                    component: node.kind.as_str(),
                    prop: key,
                    expr: "",
                    what: "condition prop must be a literal expression",
                }),
            }
        }
        collect_defects(node.children, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generated::registry::ComponentKind;
    use serde_json::json;

    fn data(pairs: &[(&str, Value)]) -> impl Fn(&str) -> Option<Value> {
        let map: Vec<(String, Value)> =
            pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
        move |path: &str| {
            map.iter().find(|(k, _)| k == path).map(|(_, v)| v.clone()).or_else(|| {
                // dotted walk over object roots, mirroring RenderContext::lookup
                let mut segs = path.split('.');
                let root = segs.next()?;
                let mut cur = map.iter().find(|(k, _)| k == root).map(|(_, v)| v.clone())?;
                for seg in segs {
                    cur = cur.get(seg)?.clone();
                }
                Some(cur)
            })
        }
    }

    #[test]
    fn the_corpus_shapes_parse_and_evaluate() {
        let lookup = data(&[
            ("resend_available", json!(false)),
            ("passkey_available", json!(true)),
            ("cart_item_count", json!(2)),
            ("cart", json!({ "lines": ["a"] })),
            ("recent_searches", json!([])),
            ("order", json!({ "serviceType": "DELIVERY", "status": "DELIVERED" })),
            ("item", json!({ "errorCode": null, "oldestPendingAt": "2026-08-29T01:00:00Z" })),
            ("resolution", json!({ "value": "PARTIAL_REFUND" })),
            ("search_input", json!({ "value": "" })),
            ("restaurant", json!({ "rating": 4.5 })),
            ("selected_delivery_mode", json!("delivery")),
        ]);
        for (expr, want) in [
            ("passkey_available", true),
            ("resend_available", false),
            ("!resend_available", true),
            ("cart_item_count > 0", true),
            ("cart.lines.length == 0", false),
            ("recent_searches.length > 0", false),
            ("order.serviceType == 'DELIVERY'", true),
            ("order.status in ['DELIVERED','CANCELLED_BY_CUSTOMER']", true),
            ("resolution.value in ['PARTIAL_REFUND','GOODWILL_CREDIT']", true),
            ("item.errorCode != null", false),
            ("item.oldestPendingAt != null", true),
            ("search_input.value == \"\"", true),
            ("restaurant.rating", true),
            ("selected_delivery_mode == 'delivery'", true),
            ("stripe_publishable_key == null", true), // missing IS null for null comparisons
        ] {
            let c = Condition::parse(expr).unwrap_or_else(|e| panic!("{expr}: {e:?}"));
            assert_eq!(c.eval(&lookup), Some(want), "{expr}");
        }
    }

    #[test]
    fn missing_data_is_unevaluatable_not_false_and_not_loud() {
        let lookup = data(&[]);
        for expr in [
            "passkey_available",
            "!resend_available",
            "cart_item_count > 0",
            "cart.lines.length == 0",
            "order.serviceType == 'DELIVERY'",
            "resolution.value in ['PARTIAL_REFUND']",
        ] {
            let c = Condition::parse(expr).expect(expr);
            assert_eq!(c.eval(&lookup), None, "{expr}: missing data must be unevaluatable");
        }
    }

    #[test]
    fn unknown_constructs_are_parse_errors() {
        for expr in [
            "a >= b",
            "a <= 1",
            "a < 1",
            "a == b",                          // bare-identifier RHS is not grammar — quote it
            "a && b",
            "a > 'x'",                         // `>` is integer-only
            "x in [PARTIAL_REFUND]",           // unquoted list item
            "x in []",
            "",
            "a == 'unterminated",
        ] {
            assert!(Condition::parse(expr).is_err(), "{expr}: must be a loud parse error");
        }
    }

    /// The corpus-walk gate (#472): EVERY condition prop of EVERY generated screen tree and
    /// bottom sheet, across all five surfaces, parses. Seen red during development by planting
    /// `selected_delivery_mode == delivery` (the unquoted spelling this branch fixed in the spec);
    /// the planted-defect test below keeps the red path proven permanently.
    #[test]
    fn every_generated_condition_parses() {
        use crate::generated::screens as s;
        let mut walked = 0usize;
        for (surface, screens, sheets) in [
            ("captain_frontoffice", s::captain_frontoffice::SCREENS, s::captain_frontoffice::SHEETS),
            ("restaurant_frontoffice", s::restaurant_frontoffice::SCREENS, s::restaurant_frontoffice::SHEETS),
            ("restaurant_backoffice", s::restaurant_backoffice::SCREENS, s::restaurant_backoffice::SHEETS),
            ("rider", s::rider::SCREENS, s::rider::SHEETS),
            ("system", s::system::SCREENS, s::system::SHEETS),
        ] {
            for screen in screens {
                let defects = condition_defects(screen.tree);
                assert!(
                    defects.is_empty(),
                    "{surface}/{}: unparseable condition(s): {defects:?}",
                    screen.id
                );
                walked += count_conditions(screen.tree);
            }
            for sheet in sheets {
                let tree = [sheet.node];
                let defects = condition_defects(&tree);
                assert!(
                    defects.is_empty(),
                    "{surface}/sheet {}: unparseable condition(s): {defects:?}",
                    sheet.id
                );
                walked += count_conditions(&tree);
            }
        }
        // The corpus is non-trivial: if this drops to zero the walk itself broke.
        assert!(walked >= 20, "expected a real condition corpus, walked only {walked}");
    }

    fn count_conditions(nodes: &[Node]) -> usize {
        nodes
            .iter()
            .map(|n| {
                n.props.iter().filter(|(k, _)| is_condition_prop(k)).count()
                    + count_conditions(n.children)
            })
            .sum()
    }

    /// The gate's own red path, kept permanently: a planted out-of-grammar expression IS reported.
    #[test]
    fn the_corpus_gate_flags_a_planted_defect() {
        let planted = Node {
            kind: ComponentKind::Text,
            props: &[
                ("visible_when", PropValue::Text("selected_delivery_mode == delivery")),
                ("rows.1.visible_when", PropValue::Text("a >= b")),
            ],
            children: &[],
        };
        let defects = condition_defects(&[planted]);
        assert_eq!(defects.len(), 2, "{defects:?}");
    }
}
