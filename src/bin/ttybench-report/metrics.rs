//! The metrics of ttybench's own tools: a title of each, and which way is
//! better, where one is; someone else's metric goes by its name alone.

use serde::Serialize;

/// Which way of a metric is better.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Better {
    /// The lower, as a time.
    Lower,
    /// The higher, as a rate.
    Higher,
}

/// The tools' metrics: the tool, the metric, its title, and which way is
/// better, of none where it depends on the workload, as the frames a
/// second of a scroll or a rest.
const KNOWN: &[(&str, &str, &str, Option<Better>)] = &[
    (
        "startup",
        "window_ms",
        "From the launch to its window",
        Some(Better::Lower),
    ),
    (
        "startup",
        "ready_ms",
        "From the launch to running the payload",
        Some(Better::Lower),
    ),
    ("idle", "cpu_percent", "Its CPU", Some(Better::Lower)),
    (
        "idle",
        "package_idle_wakeups_per_s",
        "Package idle wakeups a second",
        Some(Better::Lower),
    ),
    (
        "idle",
        "interrupt_wakeups_per_s",
        "Interrupt wakeups a second",
        Some(Better::Lower),
    ),
    (
        "idle",
        "power_mw",
        "The power that the kernel bills it",
        Some(Better::Lower),
    ),
    (
        "memory",
        "footprint_mib",
        "Its memory's footprint",
        Some(Better::Lower),
    ),
    (
        "memory",
        "peak_mib",
        "Its memory's peak footprint",
        Some(Better::Lower),
    ),
    (
        "throughput",
        "written_mib_s",
        "Its rate to the last write",
        Some(Better::Higher),
    ),
    (
        "throughput",
        "quiet_mib_s",
        "Its rate to the quiet",
        Some(Better::Higher),
    ),
    (
        "latency",
        "latency_ms",
        "A key to its echo",
        Some(Better::Lower),
    ),
    (
        "latency",
        "missed_keys",
        "Keys of no echo seen",
        Some(Better::Lower),
    ),
    (
        "scroll",
        "frames_per_s",
        "Frames a second that moved the text",
        Some(Better::Higher),
    ),
    (
        "scroll",
        "frame_gap_ms",
        "The gap between two frames",
        Some(Better::Lower),
    ),
    (
        "gpu",
        "terminal_gpu_ms_per_s",
        "Its GPU time a second",
        Some(Better::Lower),
    ),
    (
        "gpu",
        "window_server_gpu_ms_per_s",
        "WindowServer's GPU time a second",
        Some(Better::Lower),
    ),
    (
        "gpu",
        "gpu_power_mw",
        "The GPU's power",
        Some(Better::Lower),
    ),
    (
        "gpu",
        "gpu_active_percent",
        "The GPU's share of the time at work",
        Some(Better::Lower),
    ),
    ("gpu", "gpu_freq_mhz", "The GPU's frequency at work", None),
    (
        "frames",
        "frame_gpu_ms",
        "A frame's GPU time",
        Some(Better::Lower),
    ),
    ("frames", "frames_per_s", "Frames a second on the GPU", None),
    ("frames", "frame_gap_ms", "The gap between two frames", None),
    (
        "frames",
        "refreshes_with_frame",
        "Refreshes of a new frame",
        None,
    ),
    (
        "frames",
        "frames_shown_per_s",
        "Frames shown a second",
        None,
    ),
    (
        "frames",
        "frames_dropped",
        "Frames dropped",
        Some(Better::Lower),
    ),
    (
        "motion",
        "frames_per_s",
        "Frames a second of the motion",
        Some(Better::Higher),
    ),
    (
        "motion",
        "missed_frames",
        "Frames missed",
        Some(Better::Lower),
    ),
    (
        "motion",
        "compositor_delay_ms",
        "The compositor's delay",
        Some(Better::Lower),
    ),
    (
        "motion",
        "terminal_cpu_percent",
        "Its CPU",
        Some(Better::Lower),
    ),
    (
        "motion",
        "window_server_cpu_percent",
        "WindowServer's CPU",
        Some(Better::Lower),
    ),
];

/// The title of `tool`'s `metric`, its own name if it is not one of the
/// tools', and which way is better, if one.
pub fn of(tool: &str, metric: &str) -> (String, Option<Better>) {
    KNOWN
        .iter()
        .find(|known| known.0 == tool && known.1 == metric)
        .map_or_else(
            || (metric.to_owned(), None),
            |known| (known.2.to_owned(), known.3),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_known_metric_and_another() {
        assert_eq!(
            of("startup", "ready_ms"),
            (
                "From the launch to running the payload".to_owned(),
                Some(Better::Lower)
            )
        );
        assert_eq!(of("mine", "ready_ms"), ("ready_ms".to_owned(), None));
        // Each pair once.
        for (at, known) in KNOWN.iter().enumerate() {
            assert!(
                !KNOWN[at + 1..]
                    .iter()
                    .any(|other| (other.0, other.1) == (known.0, known.1)),
                "{known:?}"
            );
        }
    }
}
