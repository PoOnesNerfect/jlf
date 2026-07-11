use std::collections::{HashMap, HashSet};
#[cfg(test)]
use std::rc::Rc;

use jlf_core::{Digest, Json};

use crate::field::{path, resolve, scalar};

/// A computed summary panel: a title plus pre-formatted rows.
pub struct Summary {
    pub title: String,
    pub rows: Vec<String>,
}

/// Which summary an [`Agg`] computes.
#[derive(Clone)]
enum Kind {
    Count,
    Uniq,
    Top(usize),
    Stats,
}

/// An incremental summary aggregator. Records are `feed`-d one at a time (so a
/// summary can stream over a huge store across frames and keep updating as new
/// records arrive) and `render`-ed on demand. `reset` clears the accumulators so
/// a filter change can recompute from scratch.
pub struct Agg {
    kind: Kind,
    /// The field to aggregate (`None` = plain `count` of records).
    field: Option<String>,
    path: Vec<String>,
    by: HashMap<String, u64>,
    seen: HashSet<String>,
    digest: Digest,
    /// Records contributing a value (or all records, for plain `count`).
    total: u64,
}

impl Agg {
    /// Build an aggregator for a `:` command verb, or `Err(message)` when the
    /// verb needs a field it wasn't given.
    pub fn new(verb: &str, field: Option<&str>, n: usize) -> Result<Agg, String> {
        let kind = match verb {
            "count" => Kind::Count,
            "uniq" => Kind::Uniq,
            "top" => Kind::Top(n),
            "stats" => Kind::Stats,
            _ => return Err(format!("unknown summary '{verb}'")),
        };
        // Only plain `count` may omit the field.
        if field.is_none() && !matches!(kind, Kind::Count) {
            return Err(format!("{verb} needs a field"));
        }
        Ok(Agg {
            kind,
            field: field.map(str::to_owned),
            path: field.map(path).unwrap_or_default(),
            by: HashMap::new(),
            seen: HashSet::new(),
            digest: Digest::new(),
            total: 0,
        })
    }

    /// Clear the accumulators (for recompute after a filter change).
    pub fn reset(&mut self) {
        self.by.clear();
        self.seen.clear();
        self.digest = Digest::new();
        self.total = 0;
    }

    /// Fold one record into the running aggregate.
    pub fn feed(&mut self, line: &str) {
        // Plain count needs no parse — every record counts.
        if self.field.is_none() {
            self.total += 1;
            return;
        }
        let mut j = Json::Null;
        if j.parse_replace(line).is_err() {
            return;
        }
        let value = scalar(resolve(&j, &self.path));
        match self.kind {
            Kind::Count => {
                self.total += 1;
                let key = value.unwrap_or("∅").to_owned();
                *self.by.entry(key).or_insert(0) += 1;
            }
            Kind::Uniq => {
                self.total += 1;
                if let Some(v) = value {
                    self.seen.insert(v.to_owned());
                }
            }
            Kind::Top(_) => {
                if let Some(v) = value {
                    self.total += 1;
                    *self.by.entry(v.to_owned()).or_insert(0) += 1;
                }
            }
            Kind::Stats => {
                if let Some(x) = value.and_then(|s| s.parse::<f64>().ok()) {
                    self.digest.add(x);
                }
            }
        }
    }

    /// Render the current aggregate. `processed`/`of` drive a progress note shown
    /// until the whole view has been folded in.
    pub fn render(&self, processed: usize, of: usize) -> Summary {
        let mut rows = match &self.kind {
            Kind::Count => self.render_count(),
            Kind::Uniq => self.render_uniq(),
            Kind::Top(n) => self.render_top(*n),
            Kind::Stats => self.render_stats(),
        };
        if processed < of {
            let pct = processed.checked_mul(100).and_then(|p| p.checked_div(of)).unwrap_or(100);
            rows.push(format!("… computing {pct}% ({processed} of {of})"));
        }
        Summary { title: self.title(), rows }
    }

    fn title(&self) -> String {
        match (&self.kind, &self.field) {
            (Kind::Count, None) => "count".into(),
            (Kind::Count, Some(f)) => format!("count {f}"),
            (Kind::Uniq, Some(f)) => format!("uniq {f}"),
            (Kind::Top(_), Some(f)) => format!("top {f}"),
            (Kind::Stats, Some(f)) => format!("stats {f}"),
            _ => "summary".into(),
        }
    }

    fn sorted(&self) -> Vec<(&String, &u64)> {
        let mut rows: Vec<_> = self.by.iter().collect();
        rows.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
        rows
    }

    fn render_count(&self) -> Vec<String> {
        if self.field.is_none() {
            return vec![format!("{}", self.total)];
        }
        let mut out = vec![format!("{:>10}  value", "count")];
        out.extend(self.sorted().iter().map(|(k, n)| format!("{n:>10}  {k}")));
        out.push(format!("{:>10}  total", self.total));
        out
    }

    fn render_uniq(&self) -> Vec<String> {
        vec![format!("{} distinct (of {} values)", self.seen.len(), self.total)]
    }

    fn render_top(&self, n: usize) -> Vec<String> {
        let distinct = self.by.len();
        let rows = self.sorted();
        let shown = rows.len().min(n);
        let mut out = vec![format!("{:>10}   share  value", "count")];
        out.extend(rows.iter().take(n).map(|(k, c)| {
            let pct = if self.total > 0 { **c as f64 / self.total as f64 * 100.0 } else { 0.0 };
            format!("{c:>10}  {pct:>5.1}%  {k}")
        }));
        out.push(format!("top {shown} of {distinct} distinct ({} values)", self.total));
        out
    }

    fn render_stats(&self) -> Vec<String> {
        // `quantile` sorts in place, so work on a clone to keep `render` `&self`.
        let mut d = self.digest.clone();
        if d.count() == 0 {
            return vec!["count 0".into(), "(no numeric values)".into()];
        }
        vec![
            format!("count {}", d.count()),
            format!("min   {:.2}", d.min()),
            format!("max   {:.2}", d.max()),
            format!("mean  {:.2}", d.mean()),
            format!("p50   {:.2}", d.quantile(0.5)),
            format!("p90   {:.2}", d.quantile(0.9)),
            format!("p99   {:.2}", d.quantile(0.99)),
        ]
    }
}

/// Convenience for tests: fold `records` fully, then render.
#[cfg(test)]
pub fn compute(
    verb: &str,
    field: Option<&str>,
    n: usize,
    records: impl Iterator<Item = Rc<str>>,
) -> Summary {
    let mut agg = Agg::new(verb, field, n).unwrap();
    let mut total = 0;
    for line in records {
        agg.feed(&line);
        total += 1;
    }
    agg.render(total, total)
}
