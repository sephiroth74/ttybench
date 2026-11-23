//! A metric's samples summed up: their median, percentiles, mean and
//! spread, and a bootstrap interval of the median, so that two terminals,
//! or two copies of one, compare beyond their noise.

use serde::{Deserialize, Serialize};

/// The resamples of a bootstrap interval.
pub const RESAMPLES: usize = 2000;

/// A metric's samples summed up.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Summary {
    /// How many samples.
    pub count: usize,
    /// The least.
    pub min: f64,
    /// The median.
    pub median: f64,
    /// The 95th percentile.
    pub p95: f64,
    /// The 99th percentile.
    pub p99: f64,
    /// The greatest.
    pub max: f64,
    /// The mean.
    pub mean: f64,
    /// The standard deviation, of the samples as a sample.
    pub stddev: f64,
    /// The 95 % bootstrap interval of the median, its low and high ends.
    pub median_interval: (f64, f64),
}

/// The summary of `samples`, of the finite ones; none of none.
pub fn summary(samples: &[f64]) -> Option<Summary> {
    let mut sorted: Vec<f64> = samples
        .iter()
        .copied()
        .filter(|value| value.is_finite())
        .collect();
    if sorted.is_empty() {
        return None;
    }
    sorted.sort_by(f64::total_cmp);
    let count = sorted.len();
    let mean = sorted.iter().sum::<f64>() / count as f64;
    let variance = if count > 1 {
        sorted
            .iter()
            .map(|value| (value - mean).powi(2))
            .sum::<f64>()
            / (count - 1) as f64
    } else {
        0.0
    };
    Some(Summary {
        count,
        min: sorted[0],
        median: percentile(&sorted, 50.0),
        p95: percentile(&sorted, 95.0),
        p99: percentile(&sorted, 99.0),
        max: sorted[count - 1],
        mean,
        stddev: variance.sqrt(),
        median_interval: median_interval(&sorted),
    })
}

/// The `p`th percentile of `sorted`, ascending and not empty, of a linear
/// interpolation between its two nearest ranks, as R's and NumPy's default.
pub fn percentile(sorted: &[f64], p: f64) -> f64 {
    let rank = (p / 100.0).clamp(0.0, 1.0) * (sorted.len() - 1) as f64;
    let (low, high) = (rank.floor() as usize, rank.ceil() as usize);
    sorted[low] + (sorted[high] - sorted[low]) * (rank - low as f64)
}

/// The 95 % bootstrap interval of the median of `sorted`, of [`RESAMPLES`]
/// resamples of a seed of its own, the same for the same samples.
fn median_interval(sorted: &[f64]) -> (f64, f64) {
    if sorted.len() < 2 {
        return (sorted[0], sorted[0]);
    }
    let mut rng = XorShift(0x9E37_79B9_7F4A_7C15 ^ sorted.len() as u64);
    let mut medians = Vec::with_capacity(RESAMPLES);
    let mut resample = vec![0.0; sorted.len()];
    for _ in 0..RESAMPLES {
        for slot in &mut resample {
            *slot = sorted[rng.below(sorted.len())];
        }
        resample.sort_by(f64::total_cmp);
        medians.push(percentile(&resample, 50.0));
    }
    medians.sort_by(f64::total_cmp);
    (percentile(&medians, 2.5), percentile(&medians, 97.5))
}

/// A small generator of pseudo-random numbers, xorshift64*, enough for a
/// bootstrap's resamples, of no crate.
struct XorShift(u64);

impl XorShift {
    /// A number below `bound`.
    fn below(&mut self, bound: usize) -> usize {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        let value = self.0.wrapping_mul(0x2545_F491_4F6C_DD1D);
        (value % bound as u64) as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_summary_of_known_samples() {
        let samples: Vec<f64> = (1..=100).map(f64::from).collect();
        let summary = summary(&samples).unwrap();
        assert_eq!(summary.count, 100);
        assert_eq!((summary.min, summary.max), (1.0, 100.0));
        assert!((summary.median - 50.5).abs() < 1e-9);
        assert!((summary.p95 - 95.05).abs() < 1e-9);
        assert!((summary.p99 - 99.01).abs() < 1e-9);
        assert!((summary.mean - 50.5).abs() < 1e-9);
        assert!((summary.stddev - 29.011_491_975_882_016).abs() < 1e-9);
        let (low, high) = summary.median_interval;
        assert!(
            low <= summary.median && summary.median <= high,
            "{low} {high}"
        );
        assert!(high - low < 25.0, "{low} {high}");
        // The same samples, the same interval.
        assert_eq!(
            super::summary(&samples).unwrap().median_interval,
            (low, high)
        );
    }

    #[test]
    fn a_lone_sample_and_none() {
        let one = summary(&[3.0]).unwrap();
        assert_eq!((one.median, one.p99, one.stddev), (3.0, 3.0, 0.0));
        assert_eq!(one.median_interval, (3.0, 3.0));
        assert_eq!(summary(&[]), None);
        assert_eq!(summary(&[f64::NAN]), None);
    }
}
