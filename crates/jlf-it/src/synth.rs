//! Fabricate a record that satisfies the current filters, so the preview shows
//! something even when nothing in the sample matches. We mutate the first real
//! sample record (keeping its realistic shape) rather than build one from
//! scratch. Best-effort: negations (`!=`, `!~`) can't be synthesized and are
//! skipped, so a filter made only of negations yields `None`.

use serde_json::{Map, Number, Value};

/// Build a JSON line satisfying `filters` by editing `first_line`, or `None`
/// when there's nothing to change (no filters, or only unsatisfiable ones).
pub fn synthesize(first_line: &str, filters: &[String]) -> Option<String> {
    if filters.is_empty() {
        return None;
    }
    let mut base = serde_json::from_str::<Value>(first_line)
        .ok()
        .filter(Value::is_object)
        .unwrap_or_else(|| Value::Object(Map::new()));

    let mut changed = false;
    for f in filters {
        changed |= apply(&mut base, f);
    }
    changed.then(|| base.to_string())
}

/// Set the field(s) referenced by one `key OP value` filter so it matches.
/// Returns whether anything was set.
fn apply(base: &mut Value, token: &str) -> bool {
    let Some((lhs, op, rhs)) = split_op(token) else {
        return false;
    };
    // A negation is satisfied by *absence* of a value, which we can't usefully
    // show, so leave those alone.
    if op == "!=" || op == "!~" {
        return false;
    }
    // A fallback chain `a|b|c` matches when the first present field matches, so
    // set the first. An OR list `x,y` matches any, so pick the first.
    let key = lhs.split('|').next().unwrap_or(lhs).trim();
    let raw = rhs
        .split(',')
        .next()
        .unwrap_or(rhs)
        .trim()
        .trim_matches('"');
    if key.is_empty() {
        return false;
    }
    set_path(base, key, satisfying(op, raw));
    true
}

/// A value that makes `op raw` true.
fn satisfying(op: &str, raw: &str) -> Value {
    if let Ok(n) = raw.parse::<f64>() {
        let v = match op {
            ">" => n + 1.0,
            "<" => n - 1.0,
            _ => n, // = ~ >= <= are satisfied by the value itself
        };
        return number(v);
    }
    Value::String(raw.to_owned())
}

fn number(v: f64) -> Value {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        Value::Number((v as i64).into())
    } else {
        Number::from_f64(v)
            .map(Value::Number)
            .unwrap_or(Value::Null)
    }
}

/// Insert `val` at a dotted `path`, creating intermediate objects as needed.
fn set_path(base: &mut Value, path: &str, val: Value) {
    let parts: Vec<&str> = path.split('.').collect();
    let mut cur = base;
    for part in &parts[..parts.len() - 1] {
        if !cur.is_object() {
            *cur = Value::Object(Map::new());
        }
        let next = cur
            .as_object_mut()
            .unwrap()
            .entry(part.to_string())
            .or_insert_with(|| Value::Object(Map::new()));
        cur = next;
    }
    if !cur.is_object() {
        *cur = Value::Object(Map::new());
    }
    cur.as_object_mut()
        .unwrap()
        .insert(parts[parts.len() - 1].to_string(), val);
}

/// Split a filter token into `(field, operator, value)`, preferring the
/// two-character operators. Mirrors the operators the `jlf` filter accepts.
fn split_op(token: &str) -> Option<(&str, &str, &str)> {
    const OPS2: [&str; 4] = ["!=", ">=", "<=", "!~"];
    const OPS1: [&str; 4] = ["~", "=", ">", "<"];
    for i in 0..token.len() {
        if !token.is_char_boundary(i) {
            continue;
        }
        let rest = &token[i..];
        for op in OPS2 {
            if let Some(v) = rest.strip_prefix(op) {
                return Some((&token[..i], op, v));
            }
        }
        for op in OPS1 {
            if let Some(v) = rest.strip_prefix(op) {
                return Some((&token[..i], op, v));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synth_val(line: &str, filters: &[&str]) -> Value {
        let filters: Vec<String> =
            filters.iter().map(|s| s.to_string()).collect();
        let s = synthesize(line, &filters).expect("should synthesize");
        serde_json::from_str(&s).unwrap()
    }

    #[test]
    fn equality_sets_the_field() {
        let v = synth_val("{}", &["level=error"]);
        assert_eq!(v["level"], Value::String("error".into()));
    }

    #[test]
    fn comparison_sets_a_satisfying_number() {
        assert_eq!(synth_val("{}", &["status>=500"])["status"], 500);
        assert_eq!(synth_val("{}", &["status>500"])["status"], 501);
        assert_eq!(synth_val("{}", &["status<500"])["status"], 499);
    }

    #[test]
    fn nested_path_creates_objects() {
        let v = synth_val("{}", &["fields.latency>500"]);
        assert_eq!(v["fields"]["latency"], 501);
    }

    #[test]
    fn keeps_the_base_records_other_fields() {
        let v = synth_val(r#"{"timestamp":"t","msg":"hi"}"#, &["level=warn"]);
        assert_eq!(v["timestamp"], Value::String("t".into()));
        assert_eq!(v["level"], Value::String("warn".into()));
    }

    #[test]
    fn fallback_and_or_pick_the_first() {
        let v = synth_val("{}", &["lvl|level=error,fatal"]);
        assert_eq!(v["lvl"], Value::String("error".into()));
    }

    #[test]
    fn substring_sets_the_value() {
        let v = synth_val("{}", &["msg~timeout"]);
        assert_eq!(v["msg"], Value::String("timeout".into()));
    }

    #[test]
    fn only_negations_yields_none() {
        let filters = vec!["level!=info".to_string()];
        assert_eq!(synthesize("{}", &filters), None);
    }

    #[test]
    fn no_filters_yields_none() {
        assert_eq!(synthesize("{}", &[]), None);
    }
}
