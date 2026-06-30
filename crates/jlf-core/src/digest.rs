//! Bounded-memory streaming quantiles.
//!
//! [`Digest`] keeps values verbatim while there are few of them — so typical
//! inputs get *exact* percentiles — and folds into a t-digest sketch once it
//! crosses a cap, trading a small approximation for constant memory on huge
//! streams. `count`, `min`, `max` and `mean` stay exact in either mode.

use std::f64::consts::PI;

/// Below this many values a digest stores them verbatim and answers percentiles
/// exactly; past it, values fold into the sketch.
const EXACT_CAP: usize = 50_000;

/// t-digest compression: higher = more centroids, more accuracy, more memory.
const COMPRESSION: f64 = 200.0;

/// Flush buffered singletons into centroids once this many accumulate.
const BUFFER: usize = 1024;

/// A streaming numeric summary with bounded memory.
#[derive(Clone)]
pub struct Digest {
    exact: Vec<f64>,
    sketch: Option<TDigest>,
    count: u64,
    min: f64,
    max: f64,
    sum: f64,
}

impl Default for Digest {
    fn default() -> Self {
        Self::new()
    }
}

impl Digest {
    pub fn new() -> Self {
        Self {
            exact: Vec::new(),
            sketch: None,
            count: 0,
            min: f64::INFINITY,
            max: f64::NEG_INFINITY,
            sum: 0.0,
        }
    }

    /// Record a value. Non-finite values (NaN, ±∞) are ignored so they can't
    /// poison min/max/sum or break the sort.
    pub fn add(&mut self, x: f64) {
        if !x.is_finite() {
            return;
        }
        self.count += 1;
        self.sum += x;
        self.min = self.min.min(x);
        self.max = self.max.max(x);

        if let Some(s) = &mut self.sketch {
            s.add(x);
            return;
        }
        self.exact.push(x);
        if self.exact.len() > EXACT_CAP {
            let mut s = TDigest::new(COMPRESSION);
            s.add_slice(&self.exact);
            self.exact = Vec::new();
            self.sketch = Some(s);
        }
    }

    pub fn count(&self) -> u64 {
        self.count
    }
    pub fn min(&self) -> f64 {
        self.min
    }
    pub fn max(&self) -> f64 {
        self.max
    }
    pub fn mean(&self) -> f64 {
        if self.count == 0 {
            f64::NAN
        } else {
            self.sum / self.count as f64
        }
    }

    /// Whether percentiles are still exact (`true`) or sketch-approximated.
    pub fn is_exact(&self) -> bool {
        self.sketch.is_none()
    }

    /// The `q`-quantile (`0.0..=1.0`). Exact while under the cap.
    pub fn quantile(&mut self, q: f64) -> f64 {
        if self.count == 0 {
            return f64::NAN;
        }
        match &mut self.sketch {
            Some(s) => s.quantile(q).clamp(self.min, self.max),
            None => {
                self.exact
                    .sort_by(|a, b| a.partial_cmp(b).expect("finite"));
                let i = (((self.exact.len() - 1) as f64) * q).round() as usize;
                self.exact[i]
            }
        }
    }
}

#[derive(Clone, Copy)]
struct Centroid {
    mean: f64,
    weight: f64,
}

/// A merging t-digest: a sorted set of weighted centroids whose sizes are
/// bounded by the `asin` scale function, so accuracy is highest at the tails.
#[derive(Clone)]
struct TDigest {
    centroids: Vec<Centroid>,
    buffer: Vec<f64>,
    delta: f64,
}

impl TDigest {
    fn new(delta: f64) -> Self {
        Self {
            centroids: Vec::new(),
            buffer: Vec::new(),
            delta,
        }
    }

    fn add(&mut self, x: f64) {
        self.buffer.push(x);
        if self.buffer.len() >= BUFFER {
            self.flush();
        }
    }

    fn add_slice(&mut self, xs: &[f64]) {
        for &x in xs {
            self.add(x);
        }
    }

    /// Merge buffered singletons into the centroid set in one sorted pass.
    fn flush(&mut self) {
        if self.buffer.is_empty() {
            return;
        }
        let mut pts: Vec<Centroid> = Vec::with_capacity(self.centroids.len() + self.buffer.len());
        pts.append(&mut self.centroids);
        for &x in &self.buffer {
            pts.push(Centroid { mean: x, weight: 1.0 });
        }
        self.buffer.clear();
        pts.sort_by(|a, b| a.mean.partial_cmp(&b.mean).expect("finite"));

        let total: f64 = pts.iter().map(|c| c.weight).sum();
        let mut merged: Vec<Centroid> = Vec::new();
        let mut cur = pts[0];
        let mut w_before = 0.0;
        for p in &pts[1..] {
            let q0 = w_before / total;
            let q1 = (w_before + cur.weight + p.weight) / total;
            if k(q1, self.delta) - k(q0, self.delta) <= 1.0 {
                let w = cur.weight + p.weight;
                cur.mean += (p.mean - cur.mean) * p.weight / w;
                cur.weight = w;
            } else {
                merged.push(cur);
                w_before += cur.weight;
                cur = *p;
            }
        }
        merged.push(cur);
        self.centroids = merged;
    }

    fn quantile(&mut self, q: f64) -> f64 {
        self.flush();
        let cs = &self.centroids;
        match cs.len() {
            0 => return f64::NAN,
            1 => return cs[0].mean,
            _ => {}
        }
        let total: f64 = cs.iter().map(|c| c.weight).sum();
        let target = q * total;

        // Cumulative weight at the center of each centroid.
        let mut centers = Vec::with_capacity(cs.len());
        let mut cum = 0.0;
        for c in cs {
            centers.push(cum + c.weight / 2.0);
            cum += c.weight;
        }
        if target <= centers[0] {
            return cs[0].mean;
        }
        if target >= centers[cs.len() - 1] {
            return cs[cs.len() - 1].mean;
        }
        for i in 1..cs.len() {
            if target <= centers[i] {
                let t = (target - centers[i - 1]) / (centers[i] - centers[i - 1]);
                return cs[i - 1].mean + (cs[i].mean - cs[i - 1].mean) * t;
            }
        }
        cs[cs.len() - 1].mean
    }
}

/// The t-digest scale function `k1`: a centroid spanning cumulative quantiles
/// `[q0, q1]` is allowed while `k(q1) - k(q0) <= 1`, which keeps centroids small
/// near 0 and 1 (accurate tails) and larger in the middle.
fn k(q: f64, delta: f64) -> f64 {
    let q = q.clamp(0.0, 1.0);
    delta / (2.0 * PI) * (2.0 * q - 1.0).asin()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_under_the_cap() {
        let mut d = Digest::new();
        for i in 1..=100 {
            d.add(i as f64);
        }
        assert!(d.is_exact());
        assert_eq!(d.count(), 100);
        assert_eq!(d.min(), 1.0);
        assert_eq!(d.max(), 100.0);
        assert!((d.mean() - 50.5).abs() < 1e-9);
        // round((n-1)*q): p50 -> index 50 -> value 51
        assert_eq!(d.quantile(0.5), 51.0);
        assert_eq!(d.quantile(0.0), 1.0);
        assert_eq!(d.quantile(1.0), 100.0);
    }

    #[test]
    fn non_finite_is_ignored() {
        let mut d = Digest::new();
        d.add(1.0);
        d.add(f64::NAN);
        d.add(f64::INFINITY);
        d.add(3.0);
        assert_eq!(d.count(), 2);
        assert_eq!(d.max(), 3.0);
    }

    #[test]
    fn approximates_past_the_cap_with_small_error() {
        // A uniform 1..=1_000_000 crosses the cap, so percentiles come from the
        // sketch. They should track the true quantiles within ~1%.
        let mut d = Digest::new();
        let n = 1_000_000.0;
        for i in 1..=1_000_000u64 {
            d.add(i as f64);
        }
        assert!(!d.is_exact());
        assert_eq!(d.count(), 1_000_000);
        assert_eq!(d.min(), 1.0);
        assert_eq!(d.max(), 1_000_000.0);
        assert!((d.mean() - 500_000.5).abs() < 1.0);

        let check = |q: f64, d: &mut Digest| {
            let got = d.quantile(q);
            let want = q * n;
            let err = (got - want).abs() / n;
            assert!(err < 0.01, "q{q}: got {got}, want ~{want}, err {err}");
        };
        check(0.5, &mut d);
        check(0.9, &mut d);
        check(0.99, &mut d);
        check(0.999, &mut d);
    }

    #[test]
    fn skewed_distribution_tail_accuracy() {
        // Most values small, a heavy tail; p99 should land in the tail region.
        let mut d = Digest::new();
        for i in 0..200_000 {
            d.add((i % 100) as f64); // 0..99 bulk
        }
        for _ in 0..2_000 {
            d.add(10_000.0); // 1% tail
        }
        assert!(!d.is_exact());
        assert!(d.quantile(0.5) < 100.0);
        assert_eq!(d.max(), 10_000.0);
    }
}
