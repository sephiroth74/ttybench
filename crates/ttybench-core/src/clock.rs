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

/// `ticks` of the Mach clock in nanoseconds of the shared clock, of the
/// clock's timebase: the kernel counts a process's CPU time in those
/// ticks, and the window server times its frames in them.
pub fn mach_to_ns(ticks: u64) -> u64 {
    /// `<mach/mach_time.h>`'s `mach_timebase_info_data_t`.
    #[repr(C)]
    struct Timebase {
        numer: u32,
        denom: u32,
    }
    unsafe extern "C" {
        fn mach_timebase_info(info: *mut Timebase) -> libc::c_int;
    }
    let mut timebase = Timebase { numer: 0, denom: 0 };
    // SAFETY: `timebase` is a writable struct of the layout that the call
    // fills in.
    let status = unsafe { mach_timebase_info(&raw mut timebase) };
    if status != 0 || timebase.denom == 0 {
        return ticks;
    }
    u64::try_from(u128::from(ticks) * u128::from(timebase.numer) / u128::from(timebase.denom))
        .unwrap_or(u64::MAX)
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

    #[test]
    fn the_mach_clock_is_the_shared_clock() {
        unsafe extern "C" {
            fn mach_absolute_time() -> u64;
        }
        let before = now_ns();
        // SAFETY: a call of no argument that reads the clock.
        let ticks = unsafe { mach_absolute_time() };
        let after = now_ns();
        let at = mach_to_ns(ticks);
        assert!(before <= at && at <= after, "{before} {at} {after}");
    }
}
