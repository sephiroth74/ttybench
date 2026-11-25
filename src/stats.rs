//! A metric's samples summed up: their median, percentiles, mean and
//! spread, and a bootstrap interval of the median, and of a statistic's
//! change between two sets of samples, so that two terminals, or two
//! copies of one, compare beyond their noise.

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
    let mut resample = vec![0.0; sorted.len()];
    let mut medians: Vec<f64> = (0..RESAMPLES)
        .map(|_| Stat::Median.of_sorted(rng.resample(sorted, &mut resample)))
        .collect();
    medians.sort_by(f64::total_cmp);
    (percentile(&medians, 2.5), percentile(&medians, 97.5))
}

/// A statistic of a metric's samples, as a budget names it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Stat {
    /// The median.
    #[default]
    Median,
    /// The mean.
    Mean,
    /// The 95th percentile.
    P95,
    /// The 99th percentile.
    P99,
    /// The least.
    Min,
    /// The greatest.
    Max,
}

impl Stat {
    /// Its name, as a session's file writes it.
    pub fn name(self) -> &'static str {
        match self {
            Self::Median => "median",
            Self::Mean => "mean",
            Self::P95 => "p95",
            Self::P99 => "p99",
            Self::Min => "min",
            Self::Max => "max",
        }
    }

    /// Its value of `sorted`, ascending and not empty.
    pub fn of_sorted(self, sorted: &[f64]) -> f64 {
        match self {
            Self::Median => percentile(sorted, 50.0),
            Self::Mean => sorted.iter().sum::<f64>() / sorted.len() as f64,
            Self::P95 => percentile(sorted, 95.0),
            Self::P99 => percentile(sorted, 99.0),
            Self::Min => sorted[0],
            Self::Max => sorted[sorted.len() - 1],
        }
    }

    /// Its value of `summary`.
    pub fn of(self, summary: &Summary) -> f64 {
        match self {
            Self::Median => summary.median,
            Self::Mean => summary.mean,
            Self::P95 => summary.p95,
            Self::P99 => summary.p99,
            Self::Min => summary.min,
            Self::Max => summary.max,
        }
    }
}

/// The 95 % bootstrap interval of `stat` of `b` less `stat` of `a`, of
/// their finite samples, each resampled apart, [`RESAMPLES`] times of a
/// seed of their own, the same for the same samples; none of either
/// empty.
pub fn change_interval(a: &[f64], b: &[f64], stat: Stat) -> Option<(f64, f64)> {
    let sorted = |samples: &[f64]| {
        let mut sorted: Vec<f64> = samples
            .iter()
            .copied()
            .filter(|value| value.is_finite())
            .collect();
        sorted.sort_by(f64::total_cmp);
        sorted
    };
    let (a, b) = (sorted(a), sorted(b));
    if a.is_empty() || b.is_empty() {
        return None;
    }
    let mut rng = XorShift(0x9E37_79B9_7F4A_7C15 ^ ((a.len() as u64) << 32 | b.len() as u64));
    let (mut resample_a, mut resample_b) = (vec![0.0; a.len()], vec![0.0; b.len()]);
    let mut changes: Vec<f64> = (0..RESAMPLES)
        .map(|_| {
            let of_a = stat.of_sorted(rng.resample(&a, &mut resample_a));
            stat.of_sorted(rng.resample(&b, &mut resample_b)) - of_a
        })
        .collect();
    changes.sort_by(f64::total_cmp);
    Some((percentile(&changes, 2.5), percentile(&changes, 97.5)))
}

/// The finite samples of each of `clusters`, sorted, of the clusters that
/// hold any.
fn sorted_clusters(clusters: &[Vec<f64>]) -> Vec<Vec<f64>> {
    clusters
        .iter()
        .map(|cluster| {
            let mut sorted: Vec<f64> = cluster
                .iter()
                .copied()
                .filter(|value| value.is_finite())
                .collect();
            sorted.sort_by(f64::total_cmp);
            sorted
        })
        .filter(|cluster| !cluster.is_empty())
        .collect()
}

/// A resample of `clusters`: as many clusters drawn with replacement, then
/// each one's samples drawn with replacement, pooled and sorted into
/// `into`.
fn resample_clusters<'a>(
    rng: &mut XorShift,
    clusters: &[Vec<f64>],
    into: &'a mut Vec<f64>,
) -> &'a [f64] {
    into.clear();
    for _ in 0..clusters.len() {
        let cluster = &clusters[rng.below(clusters.len())];
        for _ in 0..cluster.len() {
            into.push(cluster[rng.below(cluster.len())]);
        }
    }
    into.sort_by(f64::total_cmp);
    into
}

/// The seed of a bootstrap of `clusters`, of their sizes, the same for the
/// same samples.
fn seed_of(clusters: &[Vec<f64>]) -> u64 {
    clusters
        .iter()
        .fold(0x9E37_79B9_7F4A_7C15, |seed, cluster| {
            seed.rotate_left(7) ^ cluster.len() as u64
        })
}

/// The 95 % bootstrap interval of `stat` of samples taken in clusters, as
/// a session's runs, each of its samples: the clusters resampled, then
/// each one's samples, so that a run that moved as a whole, as of the
/// machine's work beside it, widens the interval, as a resample of the
/// samples alone would not. None of no sample.
pub fn clustered_interval(clusters: &[Vec<f64>], stat: Stat) -> Option<(f64, f64)> {
    let clusters = sorted_clusters(clusters);
    if clusters.is_empty() {
        return None;
    }
    let mut rng = XorShift(seed_of(&clusters));
    let mut resample = Vec::new();
    let mut values: Vec<f64> = (0..RESAMPLES)
        .map(|_| stat.of_sorted(resample_clusters(&mut rng, &clusters, &mut resample)))
        .collect();
    values.sort_by(f64::total_cmp);
    Some((percentile(&values, 2.5), percentile(&values, 97.5)))
}

/// The 95 % bootstrap interval of `stat` of `b` less `stat` of `a`, each
/// of samples taken in clusters, as runs, each side resampled apart as
/// [`clustered_interval`] resamples it. None of either of no sample.
pub fn clustered_change_interval(a: &[Vec<f64>], b: &[Vec<f64>], stat: Stat) -> Option<(f64, f64)> {
    let (a, b) = (sorted_clusters(a), sorted_clusters(b));
    if a.is_empty() || b.is_empty() {
        return None;
    }
    let mut rng = XorShift(seed_of(&a) ^ seed_of(&b).rotate_left(32));
    let (mut resample_a, mut resample_b) = (Vec::new(), Vec::new());
    let mut changes: Vec<f64> = (0..RESAMPLES)
        .map(|_| {
            let of_a = stat.of_sorted(resample_clusters(&mut rng, &a, &mut resample_a));
            stat.of_sorted(resample_clusters(&mut rng, &b, &mut resample_b)) - of_a
        })
        .collect();
    changes.sort_by(f64::total_cmp);
    Some((percentile(&changes, 2.5), percentile(&changes, 97.5)))
}

/// A small generator of pseudo-random numbers, xorshift64*, enough for a
/// bootstrap's resamples, of no crate.
struct XorShift(u64);

impl XorShift {
    /// `into` filled of `sorted`'s values drawn with replacement, sorted.
    fn resample<'a>(&mut self, sorted: &[f64], into: &'a mut [f64]) -> &'a [f64] {
        for slot in into.iter_mut() {
            *slot = sorted[self.below(sorted.len())];
        }
        into.sort_by(f64::total_cmp);
        into
    }

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
    fn a_change_beyond_the_noise_and_none() {
        let a: Vec<f64> = (0..50).map(|at| 100.0 + f64::from(at % 5)).collect();
        let b: Vec<f64> = a.iter().map(|value| value + 10.0).collect();
        let (low, high) = change_interval(&a, &b, Stat::Median).unwrap();
        assert!(low > 5.0 && high < 15.0, "{low} {high}");
        let (low, high) = change_interval(&a, &a, Stat::Median).unwrap();
        assert!(low <= 0.0 && 0.0 <= high, "{low} {high}");
        // The same samples, the same interval.
        assert_eq!(
            change_interval(&a, &b, Stat::P95),
            change_interval(&a, &b, Stat::P95)
        );
        assert_eq!(change_interval(&a, &[], Stat::Median), None);
        assert_eq!(change_interval(&[f64::NAN], &a, Stat::Median), None);
    }

    #[test]
    fn runs_that_moved_as_a_whole_widen_the_interval() {
        // Two runs each, of 200 samples about 12 or 16: A's two runs one
        // of each, B's both of 16. The samples alone tell B slower beyond
        // their noise; the runs, resampled, tell too few runs to say so.
        let run = |centre: f64| -> Vec<f64> {
            (0..200)
                .map(|at| centre + f64::from(at % 5) * 0.1)
                .collect()
        };
        let a = vec![run(12.0), run(16.0)];
        let b = vec![run(16.0), run(16.0)];
        let flat_a: Vec<f64> = a.concat();
        let flat_b: Vec<f64> = b.concat();
        let (low, _) = change_interval(&flat_a, &flat_b, Stat::Median).unwrap();
        assert!(low > 0.0, "{low}");
        let (low, high) = clustered_change_interval(&a, &b, Stat::Median).unwrap();
        assert!(low <= 0.0 && 0.0 < high, "{low} {high}");
        // Runs alike, a change beyond them told.
        let slower: Vec<Vec<f64>> = (0..4).map(|_| run(16.0)).collect();
        let quicker: Vec<Vec<f64>> = (0..4).map(|_| run(12.0)).collect();
        let (low, high) = clustered_change_interval(&quicker, &slower, Stat::Median).unwrap();
        assert!(low > 3.0 && high < 5.0, "{low} {high}");
        let (low, high) = clustered_interval(&a, Stat::Median).unwrap();
        assert!(low < 13.0 && high > 15.0, "{low} {high}");
        // The same samples, the same interval; no sample, none.
        assert_eq!(
            clustered_interval(&a, Stat::Median),
            clustered_interval(&a, Stat::Median)
        );
        assert_eq!(
            clustered_interval(&[vec![], vec![f64::NAN]], Stat::Median),
            None
        );
        assert_eq!(clustered_change_interval(&a, &[], Stat::Median), None);
    }

    #[test]
    fn each_statistic_of_its_samples() {
        let sorted: Vec<f64> = (1..=100).map(f64::from).collect();
        let summary = summary(&sorted).unwrap();
        for stat in [
            Stat::Median,
            Stat::Mean,
            Stat::P95,
            Stat::P99,
            Stat::Min,
            Stat::Max,
        ] {
            assert!(
                (stat.of_sorted(&sorted) - stat.of(&summary)).abs() < 1e-9,
                "{}",
                stat.name()
            );
        }
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
