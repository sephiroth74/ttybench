//! The clock that the tools and the payload share, so that a time that the
//! payload tells in the terminal compares with the tool's: the system's
//! uptime of no sleep, in nanoseconds, as `mach_absolute_time` counts it,
//! the same in every process.

/// Now, in nanoseconds of the shared clock.
pub fn now_ns() -> u64 {
    let mut time = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `time` is a writable `timespec` that `clock_gettime` fills
    // in; `CLOCK_UPTIME_RAW` is a clock that macOS has.
    let status = unsafe { libc::clock_gettime(libc::CLOCK_UPTIME_RAW, &raw mut time) };
    debug_assert_eq!(status, 0);
    u64::try_from(time.tv_sec).unwrap_or(0) * 1_000_000_000
        + u64::try_from(time.tv_nsec).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clock_goes_on() {
        let before = now_ns();
        std::thread::sleep(std::time::Duration::from_millis(5));
        let elapsed = now_ns() - before;
        assert!((5_000_000..1_000_000_000).contains(&elapsed), "{elapsed}");
    }
}
