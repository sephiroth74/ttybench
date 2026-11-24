//! The GPU's work: each process's GPU time, of the GPU's clients that
//! IOKit lists, which takes no privilege; and the system's GPU, its power,
//! its frequency and the share of the time that it works, of
//! `powermetrics`, which takes `sudo`.
//!
//! WindowServer's GPU time counts apart: it composes the windows on the
//! display, their translucency, blur and layers, a terminal's frames among
//! them, and its work is billed to it.

use std::collections::BTreeMap;
use std::io::{BufReader, Read};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;

use serde::Deserialize;

use crate::clock::now_ns;
use crate::process;

/// A client of the GPU, as IOKit lists it: the process that made it and
/// the GPU time that it used, by API.
#[derive(Debug, Deserialize)]
struct Client {
    #[serde(rename = "IOUserClientCreator")]
    creator: Option<String>,
    #[serde(rename = "AppUsage", default)]
    usage: Vec<Usage>,
}

/// A client's GPU time of an API, in nanoseconds.
#[derive(Debug, Deserialize)]
struct Usage {
    #[serde(rename = "accumulatedGPUTime", default)]
    accumulated: u64,
}

/// The process of a client's creator, `pid 429, WindowServer`.
fn creator_pid(creator: &str) -> Option<libc::pid_t> {
    creator
        .strip_prefix("pid ")?
        .split(',')
        .next()?
        .trim()
        .parse()
        .ok()
}

/// The GPU time of each process of `clients`, a plist of IOKit's GPU
/// clients, in nanoseconds since each client was made.
fn times_of(clients: &[u8]) -> Result<BTreeMap<libc::pid_t, u64>, String> {
    let clients: Vec<Client> =
        plist::from_bytes(clients).map_err(|error| format!("the GPU's clients: {error}"))?;
    let mut times = BTreeMap::new();
    for client in clients {
        let Some(pid) = client.creator.as_deref().and_then(creator_pid) else {
            continue;
        };
        let used: u64 = client.usage.iter().map(|usage| usage.accumulated).sum();
        *times.entry(pid).or_insert(0) += used;
    }
    Ok(times)
}

/// The GPU time of each process that holds a client of the GPU now, in
/// nanoseconds, as IOKit lists the clients of Apple's GPUs.
pub fn process_times() -> Result<BTreeMap<libc::pid_t, u64>, String> {
    let output = Command::new("/usr/sbin/ioreg")
        .args(["-r", "-c", "AGXDeviceUserClient", "-a", "-w0"])
        .output()
        .map_err(|error| format!("ioreg: {error}"))?;
    if !output.status.success() {
        return Err(format!("ioreg failed: {}", output.status));
    }
    if output.stdout.iter().all(u8::is_ascii_whitespace) {
        return Ok(BTreeMap::new());
    }
    times_of(&output.stdout)
}

/// The GPU time that the processes `pids` used between `before` and
/// `after`, in nanoseconds: of the clients that each held at both.
pub fn used_between(
    before: &BTreeMap<libc::pid_t, u64>,
    after: &BTreeMap<libc::pid_t, u64>,
    pids: &[libc::pid_t],
) -> u64 {
    pids.iter()
        .filter_map(|pid| {
            Some(
                after
                    .get(pid)?
                    .saturating_sub(*before.get(pid).unwrap_or(&0)),
            )
        })
        .sum()
}

/// WindowServer's process, if one runs.
pub fn window_server() -> Option<libc::pid_t> {
    process::all_pids()
        .ok()?
        .into_iter()
        .find(|&pid| process::name(pid).is_ok_and(|name| name == "WindowServer"))
}

/// A sample of the system's GPU, as `powermetrics` gives it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    /// Its end, when the tool took it, in nanoseconds of the shared clock.
    pub end_ns: u64,
    /// Its length, in nanoseconds.
    pub elapsed_ns: u64,
    /// The GPU's frequency over it, in megahertz, of the time it worked.
    pub freq_mhz: f64,
    /// The share of it that the GPU worked, from 0 to 1.
    pub active: f64,
    /// The GPU's energy over it, in millijoules.
    pub energy_mj: f64,
}

/// A sample as `powermetrics -f plist` writes it.
#[derive(Debug, Deserialize)]
struct RawSample {
    elapsed_ns: u64,
    gpu: RawGpu,
}

/// A sample's GPU.
#[derive(Debug, Deserialize)]
struct RawGpu {
    freq_hz: f64,
    idle_ratio: f64,
    gpu_energy: f64,
}

/// The sample of one plist of `powermetrics`, taken at `end_ns`.
fn sample_of(plist: &[u8], end_ns: u64) -> Result<Sample, String> {
    let raw: RawSample =
        plist::from_bytes(plist).map_err(|error| format!("a sample of powermetrics: {error}"))?;
    Ok(Sample {
        end_ns,
        elapsed_ns: raw.elapsed_ns,
        freq_mhz: raw.gpu.freq_hz,
        active: (1.0 - raw.gpu.idle_ratio).clamp(0.0, 1.0),
        energy_mj: raw.gpu.gpu_energy,
    })
}

/// The system's GPU over the samples `samples` that lie within `start` to
/// `end`, each of the share of it within: its power, in milliwatts; the
/// share of the time that it worked, in percent; and its frequency while it
/// worked, in megahertz; none of no sample within.
pub fn system_over(samples: &[Sample], start: u64, end: u64) -> Option<(f64, f64, f64)> {
    let (mut seconds, mut energy, mut active, mut cycles) = (0.0, 0.0, 0.0, 0.0);
    for sample in samples {
        let begin = sample.end_ns.saturating_sub(sample.elapsed_ns);
        let within = sample.end_ns.min(end).saturating_sub(begin.max(start));
        if within == 0 || sample.elapsed_ns == 0 {
            continue;
        }
        let share = within as f64 / sample.elapsed_ns as f64;
        let length = within as f64 / 1e9;
        seconds += length;
        energy += sample.energy_mj * share;
        active += sample.active * length;
        cycles += sample.freq_mhz * sample.active * length;
    }
    (seconds > 0.0).then(|| {
        (
            energy / seconds,
            active / seconds * 100.0,
            if active > 0.0 { cycles / active } else { 0.0 },
        )
    })
}

/// `powermetrics` sampling the system's GPU under way, through `sudo -n`,
/// of credentials already given, as the coordinator's `sudo -v`.
#[derive(Debug)]
pub struct Powermetrics {
    child: Child,
    samples: Receiver<Result<Sample, String>>,
}

impl Powermetrics {
    /// Starts sampling the GPU every `interval`; why not otherwise, as of no
    /// credentials of `sudo`.
    pub fn start(interval: Duration) -> Result<Self, String> {
        let ready = Command::new("/usr/bin/sudo")
            .args(["-n", "true"])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|error| format!("sudo: {error}"))?;
        if !ready.success() {
            return Err(
                "the system's GPU takes powermetrics, which takes sudo: run `sudo -v` \
                        first, or a session of `sudo = true`"
                    .into(),
            );
        }
        let millis = interval.as_millis().max(100).to_string();
        let mut child = Command::new("/usr/bin/sudo")
            .args([
                "-n",
                "/usr/bin/powermetrics",
                "-f",
                "plist",
                "--samplers",
                "gpu_power",
                "-i",
                &millis,
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| format!("powermetrics: {error}"))?;
        let stdout = child.stdout.take().ok_or("powermetrics gave no output")?;
        let (sender, samples) = mpsc::channel();
        thread::spawn(move || {
            // Each sample a plist, ended by a NUL.
            let mut reader = BufReader::new(stdout);
            let mut buffer = Vec::new();
            let mut byte = [0_u8; 4096];
            while let Ok(read) = reader.read(&mut byte) {
                if read == 0 {
                    break;
                }
                buffer.extend_from_slice(&byte[..read]);
                while let Some(end) = buffer.iter().position(|&b| b == 0) {
                    let plist: Vec<u8> = buffer.drain(..=end).collect();
                    let plist = &plist[..plist.len() - 1];
                    if plist.iter().all(u8::is_ascii_whitespace) {
                        continue;
                    }
                    if sender.send(sample_of(plist, now_ns())).is_err() {
                        return;
                    }
                }
            }
        });
        Ok(Self { child, samples })
    }

    /// Stops sampling: the samples taken, in their order.
    pub fn stop(mut self) -> Result<Vec<Sample>, String> {
        // sudo passes the signal on to powermetrics, which writes its last
        // sample and ends.
        if let Ok(pid) = libc::pid_t::try_from(self.child.id()) {
            let _ = process::interrupt(pid);
        }
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            if matches!(self.child.try_wait(), Ok(Some(_))) {
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        let samples: Vec<Sample> = self.samples.try_iter().collect::<Result<_, _>>()?;
        Ok(contiguous(samples))
    }
}

/// `samples` placed end to end, back from the last one's end: a pipe may
/// hold powermetrics' output and give its samples later than it took
/// them, in bursts, but each tells its length, and they follow each other.
fn contiguous(mut samples: Vec<Sample>) -> Vec<Sample> {
    let Some(last) = samples.last() else {
        return samples;
    };
    let mut end = last.end_ns;
    for sample in samples.iter_mut().rev() {
        sample.end_ns = end;
        end = end.saturating_sub(sample.elapsed_ns);
    }
    samples
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLIENTS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><array>
<dict><key>AppUsage</key><array/><key>IOUserClientCreator</key><string>pid 429, WindowServer</string></dict>
<dict><key>AppUsage</key><array>
  <dict><key>API</key><string>Metal</string><key>accumulatedGPUTime</key><integer>4000</integer></dict>
  <dict><key>API</key><string>Metal</string><key>accumulatedGPUTime</key><integer>500</integer></dict>
</array><key>IOUserClientCreator</key><string>pid 429, WindowServer</string></dict>
<dict><key>AppUsage</key><array>
  <dict><key>accumulatedGPUTime</key><integer>70</integer></dict>
</array><key>IOUserClientCreator</key><string>pid 1234, ghostty</string></dict>
<dict><key>AppUsage</key><array/></dict>
</array></plist>"#;

    #[test]
    fn each_processs_gpu_time_sums_its_clients() {
        let times = times_of(CLIENTS.as_bytes()).unwrap();
        assert_eq!(times.get(&429), Some(&4500));
        assert_eq!(times.get(&1234), Some(&70));
        assert_eq!(times.len(), 2);
        let mut later = times.clone();
        later.insert(1234, 270);
        later.insert(429, 4600);
        assert_eq!(used_between(&times, &later, &[1234]), 200);
        assert_eq!(used_between(&times, &later, &[1234, 429, 7]), 300);
        assert_eq!(creator_pid("pid 77, kitty"), Some(77));
        assert_eq!(creator_pid("kitty"), None);
    }

    #[test]
    fn a_sample_of_powermetrics_reads_its_gpu() {
        let plist = br#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>is_delta</key><true/>
<key>elapsed_ns</key><integer>500000000</integer>
<key>gpu</key><dict>
  <key>freq_hz</key><real>338</real>
  <key>idle_ratio</key><real>0.75</real>
  <key>gpu_energy</key><integer>20</integer>
</dict>
</dict></plist>"#;
        let sample = sample_of(plist, 2_000_000_000).unwrap();
        assert_eq!(sample.elapsed_ns, 500_000_000);
        assert_eq!(sample.active, 0.25);
        assert_eq!(sample.energy_mj, 20.0);
        assert_eq!(sample.freq_mhz, 338.0);
    }

    #[test]
    fn the_system_over_a_span_weighs_its_samples() {
        // Two samples of half a second: 20 mJ a quarter active at 338 MHz,
        // then 40 mJ half active at 600 MHz.
        let samples = [
            Sample {
                end_ns: 1_500_000_000,
                elapsed_ns: 500_000_000,
                freq_mhz: 338.0,
                active: 0.25,
                energy_mj: 20.0,
            },
            Sample {
                end_ns: 2_000_000_000,
                elapsed_ns: 500_000_000,
                freq_mhz: 600.0,
                active: 0.5,
                energy_mj: 40.0,
            },
        ];
        let (power, active, freq) = system_over(&samples, 1_000_000_000, 2_000_000_000).unwrap();
        assert!((power - 60.0).abs() < 1e-9, "{power}");
        assert!((active - 37.5).abs() < 1e-9, "{active}");
        let expected = (338.0 * 0.25 + 600.0 * 0.5) / 0.75;
        assert!((freq - expected).abs() < 1e-9, "{freq}");
        // Half of the second sample alone.
        let (power, ..) = system_over(&samples, 1_750_000_000, 2_000_000_000).unwrap();
        assert!((power - 80.0).abs() < 1e-9, "{power}");
        assert_eq!(system_over(&samples, 0, 500_000_000), None);
    }

    #[test]
    fn samples_given_in_a_burst_are_placed_end_to_end() {
        let sample = |end_ns| Sample {
            end_ns,
            elapsed_ns: 250,
            freq_mhz: 0.0,
            active: 0.0,
            energy_mj: 0.0,
        };
        let placed = contiguous(vec![sample(1000), sample(1001), sample(1002)]);
        let ends: Vec<u64> = placed.iter().map(|sample| sample.end_ns).collect();
        assert_eq!(ends, [502, 752, 1002]);
    }

    #[test]
    fn the_gpus_clients_list_here() {
        let times = process_times().unwrap();
        if let Some(server) = window_server() {
            assert!(times.contains_key(&server), "{times:?}");
        }
    }
}
