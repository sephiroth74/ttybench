//! The windows on screen, as the window server lists them.

use objc2_core_foundation::{CFDictionary, CFNumber, CFRetained, CFString, CFType};
use objc2_core_graphics::{
    CGWindowListCopyWindowInfo, CGWindowListOption, kCGNullWindowID, kCGWindowBounds,
    kCGWindowLayer, kCGWindowNumber, kCGWindowOwnerPID,
};
use serde::Serialize;

/// A window's description, as the window server gives it.
type Description = CFDictionary<CFString, CFType>;

/// The size of a window, in points.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Size {
    /// Its width.
    pub width: f64,
    /// Its height.
    pub height: f64,
}

/// A window's frame, in points, from the top left of the main display, as
/// the window server lists it, and as the mouse's events are placed.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Frame {
    /// Its left edge.
    pub x: f64,
    /// Its top edge.
    pub y: f64,
    /// Its width.
    pub width: f64,
    /// Its height.
    pub height: f64,
}

/// A window on screen at the normal level: its number, which the window
/// server and the screen's capture know it by, its process and its frame.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Window {
    /// Its number.
    pub id: u32,
    /// Its process.
    pub pid: libc::pid_t,
    /// Its frame.
    pub frame: Frame,
}

/// The largest window that one of the processes `pids` has on screen at the
/// normal level, as a terminal's of its tree of processes.
pub fn window_of(pids: &[libc::pid_t]) -> Option<Window> {
    // SAFETY: reading a constant that CoreGraphics defines.
    let number = unsafe { kCGWindowNumber };
    pids.iter()
        .flat_map(|&pid| {
            normal_windows(pid).into_iter().filter_map(move |window| {
                Some(Window {
                    id: u32::try_from(integer(&window, number)?).ok()?,
                    pid,
                    frame: frame(&window)?,
                })
            })
        })
        .max_by(|a, b| {
            (a.frame.width * a.frame.height).total_cmp(&(b.frame.width * b.frame.height))
        })
}

/// The frames of the windows that the process `pid` has on screen at the
/// normal level, the largest first.
pub fn frames(pid: libc::pid_t) -> Vec<Frame> {
    let mut frames: Vec<Frame> = normal_windows(pid)
        .iter()
        .filter_map(|window| frame(window))
        .collect();
    frames.sort_by(|a, b| (b.width * b.height).total_cmp(&(a.width * a.height)));
    frames
}

/// Whether the process `pid` has a window on screen at the normal level: the
/// level of document windows, below menus, panels and the menu bar.
pub fn has_window(pid: libc::pid_t) -> bool {
    !normal_windows(pid).is_empty()
}

/// The sizes of the windows that the process `pid` has on screen at the
/// normal level.
pub fn sizes(pid: libc::pid_t) -> Vec<Size> {
    normal_windows(pid)
        .iter()
        .filter_map(|window| size(window))
        .collect()
}

/// The windows of `pid` on screen at the normal level. A window's owner,
/// level and bounds can be read without the Screen Recording permission.
fn normal_windows(pid: libc::pid_t) -> Vec<CFRetained<Description>> {
    let options =
        CGWindowListOption::OptionOnScreenOnly | CGWindowListOption::ExcludeDesktopElements;
    let Some(windows) = CGWindowListCopyWindowInfo(options, kCGNullWindowID) else {
        return Vec::new();
    };
    // SAFETY: the window server describes each window with a dictionary
    // whose keys are strings.
    let windows = unsafe { windows.cast_unchecked::<Description>() };
    // SAFETY: reading constants that CoreGraphics defines.
    let (owner, layer) = unsafe { (kCGWindowOwnerPID, kCGWindowLayer) };
    windows
        .iter()
        .filter(|window| integer(window, owner) == Some(pid) && integer(window, layer) == Some(0))
        .collect()
}

/// The integer at `key` in a window's description.
fn integer(window: &Description, key: &CFString) -> Option<i32> {
    window.get(key)?.downcast::<CFNumber>().ok()?.as_i32()
}

/// The frame of a window's bounds, a dictionary with `X`, `Y`, `Width`
/// and `Height`.
fn frame(window: &Description) -> Option<Frame> {
    // SAFETY: reading a constant that CoreGraphics defines.
    let key = unsafe { kCGWindowBounds };
    let bounds = window.get(key)?.downcast::<CFDictionary>().ok()?;
    // SAFETY: the bounds are a dictionary of numbers keyed by strings.
    let bounds = unsafe { bounds.cast_unchecked::<CFString, CFNumber>() };
    let dimension = |name: &'static str| bounds.get(&CFString::from_static_str(name))?.as_f64();
    Some(Frame {
        x: dimension("X")?,
        y: dimension("Y")?,
        width: dimension("Width")?,
        height: dimension("Height")?,
    })
}

/// The size in a window's bounds, a dictionary with `Width` and `Height`.
fn size(window: &Description) -> Option<Size> {
    // SAFETY: reading a constant that CoreGraphics defines.
    let key = unsafe { kCGWindowBounds };
    let bounds = window.get(key)?.downcast::<CFDictionary>().ok()?;
    // SAFETY: the bounds are a dictionary of numbers keyed by strings.
    let bounds = unsafe { bounds.cast_unchecked::<CFString, CFNumber>() };
    let dimension = |name: &'static str| bounds.get(&CFString::from_static_str(name))?.as_f64();
    Some(Size {
        width: dimension("Width")?,
        height: dimension("Height")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_test_process_has_no_window() {
        let pid = libc::pid_t::try_from(std::process::id()).unwrap();
        assert!(!has_window(pid));
        assert!(sizes(pid).is_empty());
        assert_eq!(window_of(&[pid]), None);
    }
}
