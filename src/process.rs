//! What the kernel tells about a process: its name, its executable, when it
//! started, its memory and how often it wakes up. A process of the same user
//! needs no special privileges.

use std::ffi::{OsString, c_int};
use std::io;
use std::mem::{self, MaybeUninit};
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;

/// Memory statistics of a process, in bytes.
#[derive(Clone, Copy, Debug)]
pub struct Memory {
    /// `phys_footprint`: the figure Activity Monitor shows as "Memory".
    pub footprint: u64,
    /// The highest footprint over the life of the process. The kernel updates
    /// it lazily, so it can lag behind `footprint` for a moment.
    pub lifetime_peak: u64,
}

/// How many times a process has woken up since it started.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Wakeups {
    /// Wakeups that brought a CPU package out of idle: what `top` shows as
    /// IDLEW.
    pub package_idle: u64,
    /// Wakeups caused by interrupts, such as timers.
    pub interrupt: u64,
}

/// Reads the memory statistics of `pid`.
pub fn memory(pid: libc::pid_t) -> io::Result<Memory> {
    let info = rusage(pid)?;
    Ok(Memory {
        footprint: info.ri_phys_footprint,
        lifetime_peak: info.ri_lifetime_max_phys_footprint,
    })
}

/// Reads the wakeup counters of `pid`.
pub fn wakeups(pid: libc::pid_t) -> io::Result<Wakeups> {
    let info = rusage(pid)?;
    Ok(Wakeups {
        package_idle: info.ri_pkg_idle_wkups,
        interrupt: info.ri_interrupt_wkups,
    })
}

/// The name of the process `pid`: its whole name, or, of a process of
/// another user's, whose name the kernel refuses, as WindowServer's, the
/// first 16 bytes of it of the short record.
pub fn name(pid: libc::pid_t) -> io::Result<String> {
    let mut buffer = [0_u8; 256];
    let size = u32::try_from(buffer.len()).expect("the buffer is small");
    // SAFETY: `buffer` is writable for `size` bytes, and `proc_name` writes
    // at most that many.
    let length = unsafe { libc::proc_name(pid, buffer.as_mut_ptr().cast(), size) };
    let length = usize::try_from(length).unwrap_or(0);
    if length > 0 {
        return Ok(String::from_utf8_lossy(&buffer[..length]).into_owned());
    }
    let info = bsdinfo(pid)?;
    let command: Vec<u8> = info
        .pbsi_comm
        .iter()
        .take_while(|&&byte| byte != 0)
        .map(|&byte| byte as u8)
        .collect();
    if command.is_empty() {
        return Err(io::Error::other("a process of no name"));
    }
    Ok(String::from_utf8_lossy(&command).into_owned())
}

/// The processes that run the executable at `executable`.
pub fn pids_of(executable: &Path) -> io::Result<Vec<libc::pid_t>> {
    let wanted = executable.canonicalize()?;
    Ok(all_pids()?
        .into_iter()
        .filter(|&pid| path(pid).is_ok_and(|path| path == wanted))
        .collect())
}

/// When the process `pid` started, by the wall clock, to the microsecond.
pub fn start_time(pid: libc::pid_t) -> io::Result<SystemTime> {
    let mut info = MaybeUninit::<libc::proc_bsdinfo>::zeroed();
    let size = c_int::try_from(mem::size_of::<libc::proc_bsdinfo>()).expect("the struct is small");
    // SAFETY: `info` is writable and sized for `proc_bsdinfo`, the struct
    // that `PROC_PIDTBSDINFO` asks the kernel to fill in. `zeroed` already
    // made every field valid, so the struct is initialized whether the call
    // succeeds or not.
    let (written, info) = unsafe {
        let written = libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            info.as_mut_ptr().cast(),
            size,
        );
        (written, info.assume_init())
    };
    if written <= 0 {
        return Err(io::Error::last_os_error());
    }
    if written != size {
        return Err(io::Error::other(
            "the kernel returned a short process record",
        ));
    }
    Ok(UNIX_EPOCH
        + Duration::from_secs(info.pbi_start_tvsec)
        + Duration::from_micros(info.pbi_start_tvusec))
}

/// What a process has used since it started: its CPU time, its wakeups,
/// its energy, and its memory's footprint now and at its peak.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Usage {
    /// CPU time, of the user and the system, in nanoseconds.
    pub cpu_ns: u64,
    /// Wakeups that brought a CPU package out of idle.
    pub package_idle: u64,
    /// Wakeups of interrupts, as timers.
    pub interrupt: u64,
    /// Energy, in nanojoules, as the kernel bills it.
    pub energy_nj: u64,
    /// Memory's footprint now, in bytes.
    pub footprint: u64,
    /// Memory's footprint at its highest, in bytes.
    pub peak: u64,
}

/// What the process `pid` has used since it started.
pub fn usage(pid: libc::pid_t) -> io::Result<Usage> {
    let info = rusage(pid)?;
    Ok(Usage {
        cpu_ns: crate::clock::mach_to_ns(info.ri_user_time.saturating_add(info.ri_system_time)),
        package_idle: info.ri_pkg_idle_wkups,
        interrupt: info.ri_interrupt_wkups,
        energy_nj: energy(pid).unwrap_or(0),
        footprint: info.ri_phys_footprint,
        peak: info.ri_lifetime_max_phys_footprint,
    })
}

/// What each process of `pids` has used since it started, of those that
/// run still.
pub fn usages(pids: &[libc::pid_t]) -> Vec<(libc::pid_t, Usage)> {
    pids.iter()
        .filter_map(|&pid| Some((pid, usage(pid).ok()?)))
        .collect()
}

/// What the processes of `after` used since `before`, summed: each process
/// of both its difference, one that started since all of its own, one
/// that ended since none; the memory's footprint and peak of `after`'s.
pub fn used_between(before: &[(libc::pid_t, Usage)], after: &[(libc::pid_t, Usage)]) -> Usage {
    let mut sum = Usage::default();
    for (pid, now) in after {
        let then = before
            .iter()
            .find(|(of, _)| of == pid)
            .map_or_else(Usage::default, |&(_, usage)| usage);
        sum.cpu_ns += now.cpu_ns.saturating_sub(then.cpu_ns);
        sum.package_idle += now.package_idle.saturating_sub(then.package_idle);
        sum.interrupt += now.interrupt.saturating_sub(then.interrupt);
        sum.energy_nj += now.energy_nj.saturating_sub(then.energy_nj);
        sum.footprint += now.footprint;
        sum.peak += now.peak;
    }
    sum
}

/// The parent of the process `pid`.
pub fn parent(pid: libc::pid_t) -> io::Result<libc::pid_t> {
    let info = bsdinfo(pid)?;
    libc::pid_t::try_from(info.pbsi_ppid).map_err(io::Error::other)
}

/// The process `root` and every process under it, its children's children
/// too, of one look at every process's parent: `root` first.
pub fn tree(root: libc::pid_t) -> Vec<libc::pid_t> {
    let parents: Vec<(libc::pid_t, libc::pid_t)> = all_pids()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|pid| Some((pid, parent(pid).ok()?)))
        .collect();
    let mut found = vec![root];
    let mut at = 0;
    while let Some(&pid) = found.get(at) {
        for &(child, of) in &parents {
            if of == pid && child != pid && !found.contains(&child) {
                found.push(child);
            }
        }
        at += 1;
    }
    found
}

/// Whether the process `pid` runs still.
pub fn exists(pid: libc::pid_t) -> bool {
    signal(pid, 0).is_ok() || io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// The kernel's short BSD record of the process `pid`, which it gives of
/// every user's processes, as `/usr/bin/login`'s, root's, between a
/// terminal and its shell, when the full record is refused.
fn bsdinfo(pid: libc::pid_t) -> io::Result<libc::proc_bsdshortinfo> {
    let mut info = MaybeUninit::<libc::proc_bsdshortinfo>::zeroed();
    let size =
        c_int::try_from(mem::size_of::<libc::proc_bsdshortinfo>()).expect("the struct is small");
    // SAFETY: `info` is writable and sized for `proc_bsdshortinfo`, the
    // struct that `PROC_PIDT_SHORTBSDINFO` asks the kernel to fill in;
    // `zeroed` made every field valid, whether the call succeeds or not.
    let (written, info) = unsafe {
        let written = libc::proc_pidinfo(
            pid,
            libc::PROC_PIDT_SHORTBSDINFO,
            0,
            info.as_mut_ptr().cast(),
            size,
        );
        (written, info.assume_init())
    };
    if written != size {
        return Err(io::Error::last_os_error());
    }
    Ok(info)
}

/// Asks the process `pid` to terminate, as `kill` does by default.
pub fn terminate(pid: libc::pid_t) -> io::Result<()> {
    signal(pid, libc::SIGTERM)
}

/// Interrupts the process `pid`, as Control-C does.
pub fn interrupt(pid: libc::pid_t) -> io::Result<()> {
    signal(pid, libc::SIGINT)
}

/// Terminates the process `pid` without giving it a chance to refuse.
pub fn kill(pid: libc::pid_t) -> io::Result<()> {
    signal(pid, libc::SIGKILL)
}

/// Sends `signal` to the process `pid`. Signal 0 only checks that the
/// process exists.
fn signal(pid: libc::pid_t, signal: c_int) -> io::Result<()> {
    // SAFETY: `kill` takes no pointers: it only asks the kernel to deliver a
    // signal.
    let status = unsafe { libc::kill(pid, signal) };
    if status != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// The IDs of every process in the system.
pub fn all_pids() -> io::Result<Vec<libc::pid_t>> {
    let mut pids: Vec<libc::pid_t> = vec![0; 1024];
    loop {
        let size = c_int::try_from(pids.len() * mem::size_of::<libc::pid_t>())
            .map_err(io::Error::other)?;
        // SAFETY: `pids` is writable for `size` bytes, and `proc_listallpids`
        // writes at most that many.
        let count = unsafe { libc::proc_listallpids(pids.as_mut_ptr().cast(), size) };
        let Ok(count) = usize::try_from(count) else {
            return Err(io::Error::last_os_error());
        };
        // A full buffer may have cut the list short: try again with more room.
        if count < pids.len() {
            pids.truncate(count);
            return Ok(pids);
        }
        pids.resize(pids.len() * 2, 0);
    }
}

/// The path of the executable that the process `pid` runs.
pub fn path(pid: libc::pid_t) -> io::Result<PathBuf> {
    let mut buffer = vec![0_u8; usize::try_from(libc::PROC_PIDPATHINFO_MAXSIZE).expect("positive")];
    let size = u32::try_from(buffer.len()).expect("the buffer is small");
    // SAFETY: `buffer` is writable for `size` bytes, and `proc_pidpath`
    // writes at most that many.
    let length = unsafe { libc::proc_pidpath(pid, buffer.as_mut_ptr().cast(), size) };
    let length = usize::try_from(length).unwrap_or(0);
    if length == 0 {
        return Err(io::Error::last_os_error());
    }
    buffer.truncate(length);
    Ok(PathBuf::from(OsString::from_vec(buffer)))
}

/// The kernel's record of version 6, as `<sys/resource.h>` has
/// `rusage_info_v6`, which the `libc` crate lacks: its UUID, then 56
/// counters of 64 bits.
#[repr(C)]
struct RusageV6 {
    uuid: [u8; 16],
    counters: [u64; 56],
}

/// Where `ri_energy_nj` is among the record's counters, after `ri_pcycles`.
const ENERGY_NJ: usize = 40;

/// The energy that `pid` has spent since it started, in nanojoules, as the
/// kernel bills it to the process, `ri_energy_nj`.
pub fn energy(pid: libc::pid_t) -> io::Result<u64> {
    let mut info = MaybeUninit::<RusageV6>::zeroed();
    // SAFETY: `info` is writable and sized for `rusage_info_v6`, the struct
    // that `RUSAGE_INFO_V6` asks the kernel to fill in, whose layout
    // `RusageV6` has. `zeroed` already made every field a valid integer.
    let (status, info) = unsafe {
        let status = libc::proc_pid_rusage(pid, 6, info.as_mut_ptr().cast());
        (status, info.assume_init())
    };
    if status != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(info.counters[ENERGY_NJ])
}

/// The kernel's resource usage record for `pid`.
fn rusage(pid: libc::pid_t) -> io::Result<libc::rusage_info_v4> {
    let mut info = MaybeUninit::<libc::rusage_info_v4>::zeroed();
    // SAFETY: `info` is writable and sized for `rusage_info_v4`, the struct
    // that `RUSAGE_INFO_V4` asks the kernel to fill in. `zeroed` already made
    // every field a valid integer, so the struct is initialized whether the
    // call succeeds or not.
    let (status, info) = unsafe {
        let status = libc::proc_pid_rusage(pid, libc::RUSAGE_INFO_V4, info.as_mut_ptr().cast());
        (status, info.assume_init())
    };
    if status != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(info)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn own_pid() -> libc::pid_t {
        libc::pid_t::try_from(std::process::id()).expect("PIDs fit in a pid_t")
    }

    #[test]
    fn its_own_energy_grows_with_its_work() {
        let before = energy(own_pid()).unwrap();
        let mut sum = 0_u64;
        for n in 0..50_000_000_u64 {
            sum = sum.wrapping_add(std::hint::black_box(n));
        }
        std::hint::black_box(sum);
        let after = energy(own_pid()).unwrap();
        // A virtual machine, as CI's, may keep no energy: the counter reads
        // nothing there.
        if after == 0 {
            return;
        }
        assert!(after > before);
    }

    #[test]
    fn the_record_has_the_layout_of_rusage_info_v6() {
        // `<sys/resource.h>`'s `rusage_info_v6`: 464 bytes, `ri_energy_nj`
        // at the 336th.
        assert_eq!(std::mem::size_of::<RusageV6>(), 464);
        assert_eq!(16 + ENERGY_NJ * 8, 336);
    }

    #[test]
    fn reads_its_own_memory() {
        let memory = memory(own_pid()).expect("a process can read its own statistics");
        assert!(memory.footprint > 0);
        assert!(memory.lifetime_peak > 0);
    }

    #[test]
    fn wakeup_counters_only_grow() {
        let before = wakeups(own_pid()).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        let after = wakeups(own_pid()).unwrap();
        assert!(after.package_idle >= before.package_idle);
        assert!(after.interrupt >= before.interrupt);
    }

    #[test]
    fn reads_its_own_name() {
        // Test binaries are named after the crate, with a hash appended.
        let name = name(own_pid()).unwrap();
        assert!(name.starts_with("ttybench"), "{name}");
    }

    #[test]
    fn finds_its_own_executable() {
        let executable = std::env::current_exe().unwrap();
        assert!(pids_of(&executable).unwrap().contains(&own_pid()));
    }

    #[test]
    fn its_tree_holds_the_children_that_it_started() {
        let mut child = std::process::Command::new("/bin/sleep")
            .arg("5")
            .spawn()
            .unwrap();
        let pid = libc::pid_t::try_from(child.id()).unwrap();
        assert_eq!(parent(pid).unwrap(), own_pid());
        let tree = tree(own_pid());
        assert_eq!(tree[0], own_pid());
        assert!(tree.contains(&pid), "{tree:?}");
        assert!(exists(pid));
        child.kill().unwrap();
        child.wait().unwrap();
        assert!(!exists(pid));
    }

    #[test]
    fn a_process_of_root_tells_its_parent_and_its_name() {
        // launchd, root's, whose full record the kernel refuses to others.
        assert_eq!(parent(1).unwrap(), 0);
        assert_eq!(name(1).unwrap(), "launchd");
    }

    #[test]
    fn its_usage_grows_with_its_work_and_sums_between_two_reads() {
        let before = usages(&[own_pid()]);
        let mut sum = 0_u64;
        for n in 0..30_000_000_u64 {
            sum = sum.wrapping_add(std::hint::black_box(n));
        }
        std::hint::black_box(sum);
        let after = usages(&[own_pid()]);
        let used = used_between(&before, &after);
        // Some milliseconds of CPU, not hours: the ticks made nanoseconds.
        assert!(
            (1_000_000..60_000_000_000).contains(&used.cpu_ns),
            "{used:?}"
        );
        assert!(
            used.footprint > 0 && used.peak >= used.footprint,
            "{used:?}"
        );
        // A process that started since counts whole; one that ended, none.
        let other = (
            1,
            Usage {
                cpu_ns: 5,
                ..Usage::default()
            },
        );
        assert_eq!(used_between(&[], &[other]).cpu_ns, 5);
        assert_eq!(used_between(&[other], &[]).cpu_ns, 0);
    }

    #[test]
    fn knows_when_it_started() {
        let started = start_time(own_pid()).unwrap();
        let age = SystemTime::now().duration_since(started).unwrap();
        assert!(age < Duration::from_secs(600), "{age:?}");
    }

    #[test]
    fn signal_zero_checks_that_a_process_exists() {
        signal(own_pid(), 0).unwrap();
        let error = signal(libc::pid_t::MAX, 0).unwrap_err();
        assert_eq!(error.raw_os_error(), Some(libc::ESRCH));
    }

    #[test]
    fn reports_a_missing_process() {
        // PIDs never reach `pid_t::MAX`: the kernel caps them far lower.
        let error = memory(libc::pid_t::MAX).expect_err("no such process");
        assert_eq!(error.raw_os_error(), Some(libc::ESRCH));
        assert!(name(libc::pid_t::MAX).is_err());
    }
}
