//! A catalog of the fields present in the sample: every nested/array path
//! (`level`, `fields.status`, `spans.0.method`) plus a few sample values per
//! field. Powers the builder's Tab-completion of fields, operators, and values.

use std::collections::HashMap;

use serde_json::Value;

/// Filter/comparison operators, in the order suggested after a field.
pub const OPS: [&str; 8] = ["=", "!=", ">", ">=", "<", "<=", "~", "!~"];

/// How many distinct sample values to remember per field.
const MAX_VALUES: usize = 12;

pub struct Catalog {
    /// Field paths, most frequent first (ties broken by shorter then alpha).
    pub paths: Vec<String>,
    values: HashMap<String, Vec<String>>,
}

impl Catalog {
    /// Walk up to `max_records` sample lines and collect every field path and a
    /// handful of its values.
    pub fn from_sample(sample: &str, max_records: usize) -> Self {
        let mut counts: HashMap<String, usize> = HashMap::new();
        let mut values: HashMap<String, Vec<String>> = HashMap::new();
        for line in sample
            .lines()
            .filter(|l| !l.trim().is_empty())
            .take(max_records)
        {
            if let Ok(v) = serde_json::from_str::<Value>(line) {
                walk("", &v, &mut counts, &mut values);
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

    /// Sample values recorded for a field path.
    pub fn values(&self, path: &str) -> &[String] {
        self.values.get(path).map(Vec::as_slice).unwrap_or(&[])
    }
}

fn walk(
    path: &str,
    v: &Value,
    counts: &mut HashMap<String, usize>,
    values: &mut HashMap<String, Vec<String>>,
) {
    if !path.is_empty() {
        *counts.entry(path.to_owned()).or_default() += 1;
    }
    match v {
        Value::Object(map) => {
            for (k, val) in map {
                let child = if path.is_empty() {
                    k.clone()
                } else {
                    format!("{path}.{k}")
                };
                walk(&child, val, counts, values);
            }
        }
        // Descend the first element so `spans.0.method` completes; the index is a
        // valid accessor and covers arrays-of-objects (spans, etc.).
        Value::Array(arr) => {
            if let Some(first) = arr.first() {
                walk(&format!("{path}.0"), first, counts, values);
            }
        }
        Value::String(_) | Value::Number(_) | Value::Bool(_) if !path.is_empty() => {
            let s = match v {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            let entry = values.entry(path.to_owned()).or_default();
            if entry.len() < MAX_VALUES && !entry.contains(&s) {
                entry.push(s);
            }
        }
        _ => {}
    }
}

/// Field paths matching `token` — prefix matches first, then substring, both
/// case-insensitive. `paths` is already ranked, so order is preserved. Empty
/// `token` returns the top paths.
pub fn match_paths(paths: &[String], token: &str, cap: usize) -> Vec<String> {
    matching(paths, token, cap)
}

/// Like [`match_paths`] but over a field's sample values.
pub fn match_values(values: &[String], token: &str, cap: usize) -> Vec<String> {
    matching(values, token, cap)
}

/// Field paths the user has typed (or is typing) in `buf`, for highlighting them
/// in the sample record. A token highlights every path it equals, prefixes, or
/// is contained in. Single characters are ignored to avoid lighting up the whole
/// record.
pub fn active_fields(buf: &str, paths: &[String]) -> std::collections::HashSet<String> {
    let mut hl = std::collections::HashSet::new();
    let is_sep = |c: char| c.is_whitespace() || c == ',' || "=!<>~|".contains(c);
    for tok in buf.split(is_sep) {
        let t = tok.trim();
        if t.len() < 2 {
            continue;
        }
        let tl = t.to_lowercase();
        for p in paths {
            let pl = p.to_lowercase();
            if pl == tl || pl.starts_with(&tl) || pl.contains(&tl) {
                hl.insert(p.clone());
            }
        }
    }
    hl
}

fn matching(items: &[String], token: &str, cap: usize) -> Vec<String> {
    if token.is_empty() {
        return items.iter().take(cap).cloned().collect();
    }
    let t = token.to_lowercase();
    let mut prefix = Vec::new();
    let mut substr = Vec::new();
    let mut fuzzy: Vec<(usize, String)> = Vec::new();
    for it in items {
        let low = it.to_lowercase();
        if low == t {
            continue; // already fully typed — nothing to add
        }
        if low.starts_with(&t) {
            prefix.push(it.clone());
        } else if low.contains(&t) {
            substr.push(it.clone());
        } else if let Some(span) = subsequence_span(&low, &t) {
            // e.g. `spamess` matches `span.message` across the `.` separator.
            fuzzy.push((span, it.clone()));
        }
    }
    // Tighter subsequence matches (fewer skipped chars) rank first.
    fuzzy.sort_by_key(|(span, _)| *span);
    prefix
        .into_iter()
        .chain(substr)
        .chain(fuzzy.into_iter().map(|(_, it)| it))
        .take(cap)
        .collect()
}

/// If every char of `needle` appears in `hay` in order, returns the span from
/// the first to the last matched char (a tightness score, smaller is better);
/// otherwise `None`. Used as the fuzzy fallback so a token can jump separators.
fn subsequence_span(hay: &str, needle: &str) -> Option<usize> {
    let mut chars = hay.char_indices();
    let mut first = None;
    let mut last = 0;
    for nc in needle.chars() {
        let (i, _) = chars.by_ref().find(|&(_, c)| c == nc)?;
        first.get_or_insert(i);
        last = i;
    }
    Some(last - first.unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = concat!(
        r#"{"level":"INFO","fields":{"status":200,"message":"ok"},"spans":[{"method":"GET"}]}"#,
        "\n",
        r#"{"level":"ERROR","fields":{"status":500,"message":"boom"}}"#,
        "\n",
    );

    #[test]
    fn collects_nested_and_array_paths() {
        let c = Catalog::from_sample(SAMPLE, 100);
        for p in ["level", "fields", "fields.status", "fields.message", "spans", "spans.0.method"] {
            assert!(c.paths.contains(&p.to_string()), "missing {p}: {:?}", c.paths);
        }
    }

    #[test]
    fn ranks_more_common_fields_first() {
        let c = Catalog::from_sample(SAMPLE, 100);
        // `level` appears in both records; `spans.0.method` only in one.
        let li = c.paths.iter().position(|p| p == "level").unwrap();
        let si = c.paths.iter().position(|p| p == "spans.0.method").unwrap();
        assert!(li < si);
    }

    #[test]
    fn records_sample_values() {
        let c = Catalog::from_sample(SAMPLE, 100);
        assert!(c.values("level").contains(&"INFO".to_string()));
        assert!(c.values("level").contains(&"ERROR".to_string()));
        assert!(c.values("fields.status").contains(&"200".to_string()));
    }

    #[test]
    fn match_paths_prefers_prefix_then_substring() {
        let paths = vec!["fields.status".to_string(), "status_code".to_string()];
        let m = match_paths(&paths, "status", 8);
        assert_eq!(m, vec!["status_code", "fields.status"]); // prefix before substring
    }

    #[test]
    fn match_paths_is_case_insensitive_substring() {
        let paths = vec!["fields.message".to_string()];
        // `mess` is a direct substring; `MSG` now matches fuzzily (m-s-g in order).
        assert_eq!(match_paths(&paths, "mess", 8), vec!["fields.message"]);
        assert_eq!(match_paths(&paths, "MSG", 8), vec!["fields.message"]);
    }

    #[test]
    fn match_paths_fuzzy_jumps_separators() {
        let paths = vec![
            "span.message".to_string(),
            "span.method".to_string(),
            "level".to_string(),
        ];
        // `spamess` is a subsequence of `span.message` but of neither other path.
        assert_eq!(match_paths(&paths, "spamess", 8), vec!["span.message"]);
        // Prefix/substring still win over fuzzy: `span` prefixes both spans.
        let m = match_paths(&paths, "span", 8);
        assert!(m.contains(&"span.message".to_string()) && m.contains(&"span.method".to_string()));
    }
}
