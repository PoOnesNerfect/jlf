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

/// Reorder a *record* preview so the interesting lines aren't buried under
/// repetitive spam: collapse near-duplicate records (same level/target and
/// message shape — e.g. a run of "Skipping migration Vn…") to their first
/// occurrence, then float higher-severity records to the top (stable within a
/// severity, so order is otherwise preserved). Returns the selected raw lines
/// joined by '\n', capped at `max`.
///
/// Used only for View/Export previews; summaries run on the full sample so
/// their aggregates (counts, stats) stay accurate.
pub fn curate(sample: &str, max: usize) -> String {
    let mut seen = HashSet::new();
    let mut picked: Vec<(u8, &str)> = Vec::new();
    for line in sample.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        match serde_json::from_str::<Value>(line) {
            Ok(v) => {
                if seen.insert(signature(&v)) {
                    picked.push((severity_rank(&v), line));
                }
            }
            // Keep unparseable lines, deduped by their exact text.
            Err(_) if seen.insert(t.to_owned()) => picked.push((0, line)),
            Err(_) => {}
        }
    }
    // Stable sort keeps first-seen order within a severity; higher first.
    picked.sort_by_key(|&(rank, _)| std::cmp::Reverse(rank));
    picked
        .iter()
        .take(max)
        .map(|(_, l)| *l)
        .collect::<Vec<_>>()
        .join("\n")
}

/// Severity ordering used to float the most interesting records up.
fn severity_rank(v: &Value) -> u8 {
    match str_field(v, &["level", "lvl", "severity"])
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "error" | "err" | "fatal" | "critical" | "crit" => 4,
        "warn" | "warning" => 3,
        "info" => 2,
        "debug" => 1,
        _ => 0,
    }
}

/// A shape key that collapses records differing only in variable data (numbers,
/// filenames): level + target + the message with digit-bearing tokens masked.
/// When there's no message, the top-level key set stands in so structurally
/// distinct records aren't merged.
fn signature(v: &Value) -> String {
    let level = str_field(v, &["level", "lvl", "severity"]).unwrap_or_default();
    let target = str_field(v, &["target", "logger", "module", "module_path"])
        .unwrap_or_default();
    let msg = str_field(v, &["message", "msg", "body"])
        .or_else(|| v.get("fields").and_then(|f| f.get("message")?.as_str()))
        .unwrap_or_default();
    let msg = normalize_msg(msg);
    let shape = if msg.is_empty() { key_shape(v) } else { String::new() };
    format!("{level}\u{1}{target}\u{1}{msg}\u{1}{shape}")
}

/// First string value among `keys` at the object's top level.
fn str_field<'a>(v: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter().find_map(|k| v.get(*k)?.as_str())
}

/// Lowercase the message and mask any token carrying a digit to `#`, so
/// `V1__a.sql` and `V2__b.sql` share a signature. First 8 tokens are enough to
/// tell messages apart without over-splitting.
fn normalize_msg(s: &str) -> String {
    s.split_whitespace()
        .map(|tok| {
            let tok = tok.trim_matches(|c: char| !c.is_alphanumeric());
            if tok.chars().any(|c| c.is_ascii_digit()) {
                "#".to_owned()
            } else {
                tok.to_ascii_lowercase()
            }
        })
        .take(8)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Sorted top-level key names, as a fallback signature for messageless records.
fn key_shape(v: &Value) -> String {
    match v {
        Value::Object(m) => {
            let mut ks: Vec<&str> = m.keys().map(String::as_str).collect();
            ks.sort_unstable();
            ks.join(",")
        }
        _ => String::new(),
    }
}

/// Pretty-print `v` with syntax colors. Keys/values whose dotted path is in
/// `highlight` get a highlighted background.
pub fn render(v: &Value, highlight: &HashSet<String>) -> String {
    let mut out = String::new();
    go(v, "", 0, highlight, &mut out);
    out
}

fn dim(s: &str) -> String { style(s).dim().to_string() }

fn go(
    v: &Value,
    path: &str,
    indent: usize,
    hl: &HashSet<String>,
    out: &mut String,
) {
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
    fn curate_collapses_spam_and_floats_errors() {
        let sample = concat!(
            r#"{"level":"INFO","message":"Skipping migration V1__a.sql"}"#,
            "\n",
            r#"{"level":"INFO","message":"Skipping migration V2__b.sql"}"#,
            "\n",
            r#"{"level":"INFO","message":"Skipping migration V3__c.sql"}"#,
            "\n",
            r#"{"level":"INFO","message":"Server started"}"#,
            "\n",
            r#"{"level":"ERROR","message":"connection refused"}"#,
            "\n",
        );
        let out = curate(sample, 80);
        let lines: Vec<&str> = out.lines().collect();
        // The three near-identical migration lines collapse to one.
        assert_eq!(lines.len(), 3);
        // The ERROR floats to the top.
        assert!(lines[0].contains("connection refused"));
        // One representative migration line survives.
        assert_eq!(out.matches("Skipping migration").count(), 1);
    }

    #[test]
    fn render_highlights_the_matched_path() {
        console::set_colors_enabled(true);
        let v: Value =
            serde_json::from_str(r#"{"fields":{"status":200}}"#).unwrap();
        let hl: HashSet<String> =
            ["fields.status".to_string()].into_iter().collect();
        let out = render(&v, &hl);
        // the highlighted key uses a background (reverse/black-on-yellow)
        // escape
        assert!(out.contains("status"));
        assert!(out.contains("\u{1b}[")); // has ANSI styling
    }
}
