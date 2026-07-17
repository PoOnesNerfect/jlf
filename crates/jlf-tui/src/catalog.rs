//! A catalog of the fields present in the loaded records — every nested/array
//! path plus a few sample values — used to autocomplete the search filter.

use std::collections::HashMap;

use jlf_core::Json;

/// Filter/comparison operators, in the order suggested after a field.
pub const OPS: [&str; 8] = ["=", "!=", ">", ">=", "<", "<=", "~", "!~"];

const MAX_VALUES: usize = 12;

#[derive(Default)]
pub struct Catalog {
    /// Field paths, most frequent first (ties broken by shorter then alpha).
    pub paths: Vec<String>,
    values: HashMap<String, Vec<String>>,
}

impl Catalog {
    /// Walk up to `max_records` of the most recent records and collect their
    /// field paths and sample values.
    pub fn from_lines<S: AsRef<str>>(lines: &[S], max_records: usize) -> Self {
        let mut counts: HashMap<String, usize> = HashMap::new();
        let mut values: HashMap<String, Vec<String>> = HashMap::new();
        for line in lines.iter().rev().take(max_records) {
            let mut j = Json::Null;
            if j.parse_replace(line.as_ref()).is_ok() {
                walk("", &j, &mut counts, &mut values);
            }
        }
        let mut paths: Vec<String> = counts.keys().cloned().collect();
        paths.sort_by(|a, b| {
            counts[b]
                .cmp(&counts[a])
                .then(a.len().cmp(&b.len()))
                .then(a.cmp(b))
        });
        Catalog { paths, values }
    }

    pub fn values(&self, path: &str) -> &[String] {
        self.values.get(path).map(Vec::as_slice).unwrap_or(&[])
    }
}

fn walk(
    path: &str,
    j: &Json,
    counts: &mut HashMap<String, usize>,
    values: &mut HashMap<String, Vec<String>>,
) {
    if !path.is_empty() {
        *counts.entry(path.to_owned()).or_default() += 1;
    }
    if let Some(obj) = j.as_object() {
        for pair in obj.iter() {
            let child = if path.is_empty() {
                pair.0.to_owned()
            } else {
                format!("{path}.{}", pair.0)
            };
            walk(&child, &pair.1, counts, values);
        }
    } else if j.is_array() {
        let first = j.get_i(0);
        if !first.is_null() {
            walk(&format!("{path}.0"), first, counts, values);
        }
    } else if !path.is_empty() {
        if let Some(s) = j.as_str().or_else(|| j.as_value()) {
            let entry = values.entry(path.to_owned()).or_default();
            let s = s.to_owned();
            if entry.len() < MAX_VALUES && !entry.contains(&s) {
                entry.push(s);
            }
        }
    }
}

/// Completions for the filter `input` with the cursor at the end. Returns the
/// byte offset where the token being completed starts and the ranked
/// candidates: field paths, then operators once a field is exact, then that
/// field's values after an operator.
pub fn suggest(input: &str, cat: &Catalog) -> (usize, Vec<String>) {
    let ws = input.rfind(char::is_whitespace).map(|i| i + 1).unwrap_or(0);
    let word = &input[ws..];
    match find_op(word) {
        Some((op_at, op_len)) => {
            let field = word[..op_at].split('|').next().unwrap_or("");
            let val_start = ws + op_at + op_len;
            let token = &input[val_start..];
            (val_start, matching(cat.values(field), token, 8))
        }
        None => {
            if !word.is_empty() && cat.paths.iter().any(|p| p == word) {
                (input.len(), OPS.iter().map(|s| s.to_string()).collect())
            } else {
                (ws, matching(&cat.paths, word, 8))
            }
        }
    }
}

/// Completions for a `:` command `input`. The first word completes against the
/// known command `verbs`; later words complete field paths (arguments), split
/// on whitespace or commas so `csv level,st` completes the `st` column.
pub fn command_suggest(
    input: &str,
    cat: &Catalog,
    verbs: &[&str],
) -> (usize, Vec<String>) {
    let start = input
        .rfind(|c: char| c.is_whitespace() || c == ',')
        .map(|i| i + 1)
        .unwrap_or(0);
    let token = &input[start..];
    if input[..start].trim().is_empty() {
        let verbs: Vec<String> = verbs.iter().map(|s| s.to_string()).collect();
        (start, matching(&verbs, token, 12))
    } else {
        (start, matching(&cat.paths, token, 8))
    }
}

/// The first filter operator in `word` as `(byte index, length)`.
fn find_op(word: &str) -> Option<(usize, usize)> {
    const OPS2: [&str; 4] = ["!=", ">=", "<=", "!~"];
    const OPS1: [&str; 4] = ["~", "=", ">", "<"];
    for i in 0..word.len() {
        if !word.is_char_boundary(i) {
            continue;
        }
        let rest = &word[i..];
        for op in OPS2 {
            if rest.starts_with(op) {
                return Some((i, op.len()));
            }
        }
        for op in OPS1 {
            if rest.starts_with(op) {
                return Some((i, op.len()));
            }
        }
    }
    None
}

/// Items matching `token`: prefix matches first, then substring, both
/// case-insensitive; exact matches are dropped (nothing to complete).
fn matching(items: &[String], token: &str, cap: usize) -> Vec<String> {
    if token.is_empty() {
        return items.iter().take(cap).cloned().collect();
    }
    let t = token.to_lowercase();
    let mut prefix = Vec::new();
    let mut substr = Vec::new();
    for it in items {
        let low = it.to_lowercase();
        if low == t {
            continue;
        }
        if low.starts_with(&t) {
            prefix.push(it.clone());
        } else if low.contains(&t) {
            substr.push(it.clone());
        }
    }
    prefix.into_iter().chain(substr).take(cap).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cat() -> Catalog {
        Catalog::from_lines(
            &[
                r#"{"level":"INFO","fields":{"status":200,"message":"ok"},"spans":[{"method":"GET"}]}"#
                    .to_string(),
                r#"{"level":"ERROR","fields":{"status":500}}"#.to_string(),
            ],
            100,
        )
    }

    #[test]
    fn collects_nested_and_array_paths() {
        let c = cat();
        for p in ["level", "fields.status", "fields.message", "spans.0.method"]
        {
            assert!(
                c.paths.contains(&p.to_string()),
                "missing {p}: {:?}",
                c.paths
            );
        }
    }

    #[test]
    fn suggests_fields_then_ops_then_values() {
        let c = cat();
        // partial field -> field paths
        let (_, s) = suggest("stat", &c);
        assert!(s.contains(&"fields.status".to_string()));
        // exact field -> operators
        let (_, s) = suggest("level", &c);
        assert_eq!(s, OPS.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        // after operator -> that field's values
        let (start, s) = suggest("level=", &c);
        assert_eq!(start, "level=".len());
        assert!(s.contains(&"INFO".to_string()));
    }

    #[test]
    fn suggest_respects_earlier_tokens() {
        let c = cat();
        let (start, s) = suggest("level=INFO stat", &c);
        assert_eq!(&"level=INFO stat"[start..], "stat");
        assert!(s.contains(&"fields.status".to_string()));
    }
}
