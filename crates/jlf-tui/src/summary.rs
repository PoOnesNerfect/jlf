use std::collections::HashMap;

use jlf_core::Json;

use crate::field::{path, resolve, scalar};

/// A computed summary panel: a title plus pre-formatted rows.
pub struct Summary {
    pub title: String,
    pub rows: Vec<String>,
}

fn parsed(line: &str) -> Option<Json<'_>> {
    let mut j = Json::Null;
    j.parse_replace(line).ok()?;
    Some(j)
}

/// `count [field]`: total, or a frequency breakdown of a field's values.
pub fn count(lines: &[&str], field: Option<&str>) -> Summary {
    let Some(field) = field else {
        return Summary {
            title: "count".into(),
            rows: vec![format!("{}", lines.len())],
        };
    };
    let p = path(field);
    let mut by: HashMap<String, u64> = HashMap::new();
    let mut total = 0u64;
    for line in lines {
        if let Some(j) = parsed(line) {
            total += 1;
            let key = scalar(resolve(&j, &p)).unwrap_or("∅").to_owned();
            *by.entry(key).or_insert(0) += 1;
        }
    }
    let mut rows: Vec<_> = by.into_iter().collect();
    rows.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let mut out = vec![format!("{:>8}  value", "count")];
    out.extend(rows.iter().map(|(k, n)| format!("{n:>8}  {k}")));
    out.push(format!("{total:>8}  total"));
    Summary {
        title: format!("count {field}"),
        rows: out,
    }
}

/// `uniq field`: number of distinct values.
pub fn uniq(lines: &[&str], field: &str) -> Summary {
    let p = path(field);
    let mut seen = std::collections::HashSet::new();
    let mut total = 0u64;
    for line in lines {
        if let Some(j) = parsed(line) {
            total += 1;
            if let Some(v) = scalar(resolve(&j, &p)) {
                seen.insert(v.to_owned());
            }
        }
    }
    Summary {
        title: format!("uniq {field}"),
        rows: vec![format!("{} distinct (of {total} values)", seen.len())],
    }
}

/// `top field [n]`: most frequent values with their share.
pub fn top(lines: &[&str], field: &str, n: usize) -> Summary {
    let p = path(field);
    let mut by: HashMap<String, u64> = HashMap::new();
    let mut total = 0u64;
    for line in lines {
        if let Some(j) = parsed(line) {
            if let Some(v) = scalar(resolve(&j, &p)) {
                total += 1;
                *by.entry(v.to_owned()).or_insert(0) += 1;
            }
        }
    }
    let distinct = by.len();
    let mut rows: Vec<_> = by.into_iter().collect();
    rows.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    rows.truncate(n);
    let mut out = vec![format!("{:>8}   share  value", "count")];
    out.extend(rows.iter().map(|(k, c)| {
        let pct = if total > 0 { *c as f64 / total as f64 * 100.0 } else { 0.0 };
        format!("{c:>8}  {pct:>5.1}%  {k}")
    }));
    out.push(format!("top {} of {distinct} distinct ({total} values)", rows.len()));
    Summary {
        title: format!("top {field}"),
        rows: out,
    }
}

/// `stats field`: count/min/max/mean/p50/p90/p99 over numeric values.
pub fn stats(lines: &[&str], field: &str) -> Summary {
    let p = path(field);
    let mut d = jlf_core::Digest::new();
    for line in lines {
        if let Some(j) = parsed(line) {
            if let Some(n) = scalar(resolve(&j, &p)).and_then(|s| s.parse::<f64>().ok()) {
                d.add(n);
            }
        }
    }
    let rows = if d.count() == 0 {
        vec!["count 0".into(), "(no numeric values)".into()]
    } else {
        vec![
            format!("count {}", d.count()),
            format!("min   {:.2}", d.min()),
            format!("max   {:.2}", d.max()),
            format!("mean  {:.2}", d.mean()),
            format!("p50   {:.2}", d.quantile(0.5)),
            format!("p90   {:.2}", d.quantile(0.9)),
            format!("p99   {:.2}", d.quantile(0.99)),
        ]
    };
    Summary {
        title: format!("stats {field}"),
        rows,
    }
}
