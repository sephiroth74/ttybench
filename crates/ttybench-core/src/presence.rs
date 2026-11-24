//! A person's presence at the Mac while a tool measures: the tool's keys,
//! clicks and scrolls are synthetic events, which macOS does not count as
//! the user's activity, so that a Mac left alone to a measure idles as if
//! nobody used it, its display going to sleep and its processors to their
//! lower states, and a terminal answers later than for a person typing. The
//! presence declares the user active every second, as a person's input
//! does, while it lives.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use objc2_core_foundation::CFString;

/// How often the presence declares the user active.
const EVERY: Duration = Duration::from_secs(1);

#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    /// `<IOKit/pwr_mgt/IOPMLib.h>`: declares the user active, as an input
    /// of the user's does.
    fn IOPMAssertionDeclareUserActivity(
        name: &CFString,
        user_type: u32,
        assertion: *mut u32,
    ) -> i32;
}

/// `kIOPMUserActiveLocal`: the user at the Mac's own keyboard and screen.
const USER_ACTIVE_LOCAL: u32 = 0;

/// The user declared active while it lives.
#[derive(Debug)]
pub struct Presence {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Presence {
    /// Declares the user active, every second, until the presence drops.
    pub fn start() -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = Arc::clone(&stop);
        let thread = thread::spawn(move || {
            let name = CFString::from_static_str("ttybench measures as a person types");
            let mut assertion = 0_u32;
            while !stopping.load(Ordering::Relaxed) {
                // SAFETY: a name that lives through the call, and a
                // writable number of the assertion, which the call renews.
                unsafe {
                    IOPMAssertionDeclareUserActivity(&name, USER_ACTIVE_LOCAL, &raw mut assertion);
                }
                thread::park_timeout(EVERY);
            }
        });
        Self {
            stop,
            thread: Some(thread),
        }
    }
}

impl Drop for Presence {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            thread.thread().unpark();
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_presence_starts_and_stops() {
        let presence = Presence::start();
        thread::sleep(Duration::from_millis(50));
        drop(presence);
    }
}
