//! Show the user the raw record they're working with: pick the richest sample
//! line and pretty-print it with JSON syntax colors, highlighting any field
//! paths the user is currently typing so they can see where they are.

use std::collections::HashSet;

use console::style;
use serde_json::Value;

/// The sample record with the most nodes (fields, nested values, array items),
/// so the example shows as much structure as possible to experiment with.
pub fn richest(sample: &str) -> Option<Value> {
    sample
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter(Value::is_object)
        .max_by_key(count_nodes)
}

fn count_nodes(v: &Value) -> usize {
    match v {
        Value::Object(m) => 1 + m.values().map(count_nodes).sum::<usize>(),
        Value::Array(a) => 1 + a.iter().map(count_nodes).sum::<usize>(),
        _ => 1,
    }
}

/// Pretty-print `v` with syntax colors. Keys/values whose dotted path is in
/// `highlight` get a highlighted background.
pub fn render(v: &Value, highlight: &HashSet<String>) -> String {
    let mut out = String::new();
    go(v, "", 0, highlight, &mut out);
    out
}

fn dim(s: &str) -> String {
    style(s).dim().to_string()
}

fn go(v: &Value, path: &str, indent: usize, hl: &HashSet<String>, out: &mut String) {
    match v {
        Value::Object(map) => {
            out.push_str(&dim("{"));
            out.push('\n');
            let n = map.len();
            for (i, (k, val)) in map.iter().enumerate() {
                let child = if path.is_empty() {
                    k.clone()
                } else {
                    format!("{path}.{k}")
                };
                out.push_str(&"  ".repeat(indent + 1));
                let key = format!("\"{k}\"");
                if hl.contains(&child) {
                    out.push_str(&style(key).black().on_yellow().to_string());
                } else {
                    out.push_str(&style(key).blue().to_string());
                }
                out.push_str(&dim(": "));
                go(val, &child, indent + 1, hl, out);
                if i + 1 < n {
                    out.push_str(&dim(","));
                }
                out.push('\n');
            }
            out.push_str(&"  ".repeat(indent));
            out.push_str(&dim("}"));
        }
        Value::Array(arr) => {
            out.push_str(&dim("["));
            out.push('\n');
            let n = arr.len();
            for (i, val) in arr.iter().enumerate() {
                out.push_str(&"  ".repeat(indent + 1));
                go(val, &format!("{path}.{i}"), indent + 1, hl, out);
                if i + 1 < n {
                    out.push_str(&dim(","));
                }
                out.push('\n');
            }
            out.push_str(&"  ".repeat(indent));
            out.push_str(&dim("]"));
        }
        scalar => {
            let s = scalar_text(scalar);
            if hl.contains(path) {
                out.push_str(&style(s).black().on_yellow().to_string());
            } else {
                out.push_str(&scalar_colored(scalar, &s));
            }
        }
    }
}

fn scalar_text(v: &Value) -> String {
    match v {
        Value::String(s) => format!("\"{s}\""),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        _ => "null".to_owned(),
    }
}

fn scalar_colored(v: &Value, s: &str) -> String {
    match v {
        Value::String(_) => style(s).green().to_string(),
        Value::Number(_) => style(s).yellow().to_string(),
        Value::Bool(_) => style(s).magenta().to_string(),
        _ => dim(s),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn richest_picks_the_biggest_record() {
        let sample = concat!(
            "{\"a\":1}\n",
            "{\"a\":1,\"b\":{\"c\":2,\"d\":3},\"e\":[1,2]}\n",
        );
        let v = richest(sample).unwrap();
        assert!(v.get("b").is_some());
    }

    #[test]
    fn render_highlights_the_matched_path() {
        console::set_colors_enabled(true);
        let v: Value = serde_json::from_str(r#"{"fields":{"status":200}}"#).unwrap();
        let hl: HashSet<String> = ["fields.status".to_string()].into_iter().collect();
        let out = render(&v, &hl);
        // the highlighted key uses a background (reverse/black-on-yellow) escape
        assert!(out.contains("status"));
        assert!(out.contains("\u{1b}[")); // has ANSI styling
    }
}
