use std::collections::VecDeque;

/// Fixed-size ring buffer of samples for sparklines. A gap (no data, e.g.
/// USB unplugged) is stored as NaN so charts can break the line there.
pub struct History {
    cap: usize,
    data: VecDeque<f32>,
}

impl History {
    pub fn new(cap: usize) -> Self {
        Self {
            cap,
            data: VecDeque::with_capacity(cap),
        }
    }

    pub fn push(&mut self, v: f32) {
        if self.data.len() == self.cap {
            self.data.pop_front();
        }
        self.data.push_back(v);
    }

    /// Record one sample slot with no data.
    pub fn push_gap(&mut self) {
        self.push(f32::NAN);
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    pub fn capacity(&self) -> usize {
        self.cap
    }

    /// All slots, gaps included (as NaN).
    pub fn iter(&self) -> impl Iterator<Item = f32> + '_ {
        self.data.iter().copied()
    }

    /// Real samples only.
    pub fn values(&self) -> impl Iterator<Item = f32> + '_ {
        self.iter().filter(|v| v.is_finite())
    }

    pub fn mean(&self) -> Option<f32> {
        let (sum, n) = self.values().fold((0.0, 0usize), |(s, n), v| (s + v, n + 1));
        (n > 0).then(|| sum / n as f32)
    }

    pub fn min_max(&self) -> Option<(f32, f32)> {
        let mut it = self.values();
        let first = it.next()?;
        Some(it.fold((first, first), |(lo, hi), v| (lo.min(v), hi.max(v))))
    }

    /// Points normalized to 0..1 on both axes (x by capacity, right-aligned;
    /// y within `[lo, hi]`, 1 = top). Gap slots have y = NaN.
    pub fn normalized(&self, lo: f32, hi: f32) -> Vec<(f32, f32)> {
        let n = self.data.len();
        if n == 0 {
            return Vec::new();
        }
        let span = (hi - lo).max(f32::EPSILON);
        let denom = (self.cap.max(2) - 1) as f32;
        let offset = self.cap - n;
        self.iter()
            .enumerate()
            .map(|(i, v)| {
                let y = if v.is_finite() {
                    ((v - lo) / span).clamp(0.0, 1.0)
                } else {
                    f32::NAN
                };
                ((offset + i) as f32 / denom, y)
            })
            .collect()
    }
}

/// Split points into continuous runs, dropping gap (NaN) points.
pub fn segments(points: &[(f32, f32)]) -> Vec<&[(f32, f32)]> {
    points.split(|p| !p.1.is_finite()).filter(|s| !s.is_empty()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gaps_are_skipped_in_stats_and_split_lines() {
        let mut h = History::new(10);
        for v in [10.0, 20.0] {
            h.push(v);
        }
        h.push_gap();
        h.push_gap();
        h.push(30.0);
        assert_eq!(h.len(), 5);
        assert_eq!(h.min_max(), Some((10.0, 30.0)));
        assert_eq!(h.mean(), Some(20.0));
        let pts = h.normalized(0.0, 40.0);
        let segs = segments(&pts);
        assert_eq!(segs.len(), 2);
        assert_eq!(segs[0].len(), 2);
        assert_eq!(segs[1].len(), 1);
    }

    #[test]
    fn only_gaps_has_no_stats() {
        let mut h = History::new(4);
        h.push_gap();
        assert_eq!(h.min_max(), None);
        assert_eq!(h.mean(), None);
        assert!(segments(&h.normalized(0.0, 1.0)).is_empty());
    }
}
