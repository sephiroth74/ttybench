//! The display kept awake while a tool measures, as `caffeinate -d` keeps
//! it: the tool's keys, clicks and scrolls are synthetic events, which
//! macOS does not count as the user's activity, so that a Mac left alone
//! to a measure would let its display sleep in the middle of it, leaving
//! nothing to capture. An assertion of IOKit's holds it awake while it
//! lives. It declares no activity of the user's, which would hold the
//! window server's compositor a refresh longer on each frame.

use objc2_core_foundation::CFString;

#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    /// `<IOKit/pwr_mgt/IOPMLib.h>`: an assertion of a type, held until it
    /// is released.
    fn IOPMAssertionCreateWithName(
        kind: &CFString,
        level: u32,
        name: &CFString,
        assertion: *mut u32,
    ) -> i32;

    /// Releases an assertion.
    fn IOPMAssertionRelease(assertion: u32) -> i32;
}

/// `kIOPMAssertionLevelOn`.
const LEVEL_ON: u32 = 255;

/// The display held awake while it lives.
#[derive(Debug)]
pub struct Awake {
    assertion: Option<u32>,
}

impl Awake {
    /// Holds the display awake, as `caffeinate -d` does.
    pub fn start() -> Self {
        let kind = CFString::from_static_str("PreventUserIdleDisplaySleep");
        let name = CFString::from_static_str("ttybench measures");
        let mut assertion = 0_u32;
        // SAFETY: the strings live through the call, and the assertion's
        // number is writable.
        let status =
            unsafe { IOPMAssertionCreateWithName(&kind, LEVEL_ON, &name, &raw mut assertion) };
        Self {
            assertion: (status == 0).then_some(assertion),
        }
    }
}

impl Drop for Awake {
    fn drop(&mut self) {
        if let Some(assertion) = self.assertion {
            // SAFETY: an assertion that this one made, released once.
            unsafe {
                IOPMAssertionRelease(assertion);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_display_is_held_awake_and_let_go() {
        let awake = Awake::start();
        assert!(awake.assertion.is_some());
        drop(awake);
    }
}
