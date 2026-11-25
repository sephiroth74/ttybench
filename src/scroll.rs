//! A scroll of a terminal's window as a hand does it, the same each run:
//! as a trackpad scrolls, of precise pixels, a swipe of its fingers' phases
//! and its momentum slowing after, up then down; or as a mouse wheel turns,
//! a step at a time. Its events go through the window server to the window
//! under the pointer, on time as a trackpad's come.

use std::thread;
use std::time::{Duration, Instant};

use crate::clock::now_ns;
use crate::keys::{self, Scroll};

/// The workload of the payload that a scroll scrolls through: lines
/// enough for a long scrollback, numbered, each unlike its neighbours.
pub const PAYLOAD: [&str; 5] = ["lines", "--count", "50000", "--width", "60"];

/// The period of the scroll's pattern, up then down, in seconds.
pub const PERIOD: f64 = 2.0;

/// How long before an event the scroll's thread stops sleeping and spins,
/// past a sleep's usual lateness.
const SPIN: Duration = Duration::from_micros(1500);

/// How the scroll comes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum By {
    /// A trackpad's precise pixels, of phases and momentum.
    Pixels,
    /// A wheel's lines, a step at a time.
    Lines,
}

impl By {
    /// Its events a second by default: a trackpad's 120, a wheel's 20.
    pub fn rate(self) -> f64 {
        match self {
            By::Pixels => 120.0,
            By::Lines => 20.0,
        }
    }

    /// Its name, as `--by` takes it.
    pub fn name(self) -> &'static str {
        match self {
            By::Pixels => "pixels",
            By::Lines => "lines",
        }
    }
}

/// A scroll's pattern.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pattern {
    /// How it comes.
    pub by: By,
    /// How long it lasts, in seconds, whole periods.
    pub seconds: f64,
    /// Its events a second.
    pub rate: f64,
    /// The points that a swipe goes, of a trackpad's.
    pub amplitude: f64,
}

impl Pattern {
    /// The time between two events.
    pub fn tick(&self) -> Duration {
        Duration::from_secs_f64(1.0 / self.rate)
    }
}

/// Scrolls at `x`, `y`, in points from the top left of the main display,
/// away from the scrollback's end, a fast swipe up, so that a scroll down
/// meets no end; its momentum left to settle for a second.
pub fn away(x: f64, y: f64) {
    keys::point(x, y);
    thread::sleep(Duration::from_millis(100));
    for step in 0..12 {
        keys::scroll(
            x,
            y,
            Scroll::Pixels {
                pixels: 170,
                phase: if step == 0 { 1 } else { 2 },
                momentum: 0,
            },
        );
        thread::sleep(Duration::from_millis(8));
    }
    keys::scroll(
        x,
        y,
        Scroll::Pixels {
            pixels: 0,
            phase: 4,
            momentum: 0,
        },
    );
    thread::sleep(Duration::from_secs(1));
}

/// The scroll's events, in their order, `rate` a second, as `settings`
/// says: by pixels, each period swipes up `amplitude` points over a
/// quarter of it, then lets its momentum slow over the next quarter to a
/// pixel an event, then the same down, so that the view moves throughout
/// and comes back where it was; by lines, each period turns the wheel up a
/// step at a time for half of it, then down.
pub fn plan(settings: Pattern) -> Vec<Scroll> {
    let periods = (settings.seconds / PERIOD).round().max(1.0) as usize;
    let mut events = Vec::new();
    match settings.by {
        By::Pixels => {
            let per = (PERIOD / 4.0 * settings.rate).round().max(2.0) as usize;
            let fingers = settings.amplitude / per as f64;
            for _ in 0..periods {
                for sign in [1.0, -1.0] {
                    for step in 0..per {
                        events.push(Scroll::Pixels {
                            pixels: (sign * fingers).round() as i32,
                            phase: if step == 0 { 1 } else { 2 },
                            momentum: 0,
                        });
                    }
                    events.push(Scroll::Pixels {
                        pixels: 0,
                        phase: 4,
                        momentum: 0,
                    });
                    for step in 0..per {
                        let left = 1.0 - step as f64 / per as f64;
                        // Down to a pixel an event, never none, so that the
                        // view moves throughout.
                        let pixels = (sign * (fingers * left * left).round().max(1.0)) as i32;
                        let momentum = match step {
                            0 => 1,
                            _ if step + 1 == per => 3,
                            _ => 2,
                        };
                        events.push(Scroll::Pixels {
                            pixels,
                            phase: 0,
                            momentum,
                        });
                    }
                }
            }
        }
        By::Lines => {
            let per = (PERIOD / 2.0 * settings.rate).round().max(1.0) as usize;
            for _ in 0..periods {
                for sign in [1, -1] {
                    events.extend(std::iter::repeat_n(Scroll::Lines(sign), per));
                }
            }
        }
    }
    events
}

/// Posts `events` at `x`, `y`, `tick` apart, on time as a trackpad's come:
/// the thread of the interactive class, which the system's timers do not
/// coalesce, asleep until a moment before each event, then spinning to it.
/// The span of the scroll, from its first event to a tick past its last,
/// in nanoseconds of the shared clock.
pub fn post(x: f64, y: f64, events: &[Scroll], tick: Duration) -> (u64, u64) {
    // SAFETY: a call on the calling thread, of a class of the system's.
    unsafe {
        libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_USER_INTERACTIVE, 0);
    }
    let origin = Instant::now();
    let start = now_ns();
    for (at, &event) in events.iter().enumerate() {
        let due = origin + tick.mul_f64(at as f64);
        if let Some(wait) = due
            .checked_duration_since(Instant::now())
            .and_then(|wait| wait.checked_sub(SPIN))
        {
            thread::sleep(wait);
        }
        while Instant::now() < due {
            std::hint::spin_loop();
        }
        keys::scroll(x, y, event);
    }
    let end = start + u64::try_from(tick.mul_f64(events.len() as f64).as_nanos()).unwrap_or(0);
    (start, end)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pattern(by: By) -> Pattern {
        Pattern {
            by,
            seconds: 4.0,
            rate: 120.0,
            amplitude: 200.0,
        }
    }

    #[test]
    fn a_trackpads_scroll_swipes_lets_its_momentum_slow_and_comes_back() {
        let events = plan(pattern(By::Pixels));
        // Two periods, each of two swipes of 60 events and their end, and
        // two momentums of 60.
        assert_eq!(events.len(), 2 * 2 * (60 + 1 + 60));
        let sum: i32 = events
            .iter()
            .map(|event| match event {
                Scroll::Pixels { pixels, .. } => *pixels,
                Scroll::Lines(_) => 0,
            })
            .sum();
        assert_eq!(sum, 0);
        assert_eq!(
            events[0],
            Scroll::Pixels {
                pixels: 3,
                phase: 1,
                momentum: 0
            }
        );
        assert_eq!(
            events[60],
            Scroll::Pixels {
                pixels: 0,
                phase: 4,
                momentum: 0
            }
        );
        assert!(matches!(events[61], Scroll::Pixels { momentum: 1, .. }));
        // The momentum slows to a pixel, never none.
        assert!(matches!(
            events[120],
            Scroll::Pixels {
                pixels: 1,
                momentum: 3,
                ..
            }
        ));
    }

    #[test]
    fn a_wheels_scroll_steps_up_then_down() {
        let events = plan(Pattern {
            rate: 20.0,
            ..pattern(By::Lines)
        });
        assert_eq!(events.len(), 2 * 2 * 20);
        assert_eq!(events[19], Scroll::Lines(1));
        assert_eq!(events[20], Scroll::Lines(-1));
    }
}
