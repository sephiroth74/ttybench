//! The keys that a tool types into a terminal, its clicks and its scrolls,
//! as a hand would: posted to
//! the terminal's process alone, so that none reaches another app,
//! whichever is in front, or, as a keyboard's, to the window server, which
//! sends them to the app in front. Posting them takes the Accessibility
//! permission of the app that runs the tool, as macOS grants it to the
//! terminal or the editor that runs a command-line tool.

use std::thread;
use std::time::Duration;

use objc2_core_foundation::CGPoint;
use objc2_core_graphics::{
    CGEvent, CGEventField, CGEventFlags, CGEventTapLocation, CGEventType, CGMouseButton,
    CGPreflightPostEventAccess, CGScrollEventUnit,
};

use crate::config::Key;

/// Whether the tool may post events: of the Accessibility permission of
/// the app that runs it, without which macOS drops them, telling nothing;
/// why not otherwise.
pub fn may_post() -> Result<(), String> {
    if CGPreflightPostEventAccess() {
        Ok(())
    } else {
        Err(
            "posting keys, clicks and scrolls takes the Accessibility permission of the app \
             that runs the tool, in System Settings › Privacy & Security › Accessibility"
                .into(),
        )
    }
}

/// The pause after each key's press and release, as a hand's, for the
/// terminal to take it.
pub const PAUSE: Duration = Duration::from_millis(30);

/// The virtual key codes of the left modifiers, as `kVK_` names them.
const COMMAND: u16 = 0x37;
const SHIFT: u16 = 0x38;
const OPTION: u16 = 0x3A;
const CONTROL: u16 = 0x3B;

/// The virtual key code of `key`, a character of the US layout or a name
/// of [`crate::config::NAMED_KEYS`], and whether the US layout types it
/// with Shift, as `+` of `=`; none of a key of no code.
pub fn code(key: &str) -> Option<(u16, bool)> {
    let named = match key {
        "enter" => Some(0x24),
        "tab" => Some(0x30),
        "space" => Some(0x31),
        "escape" => Some(0x35),
        "backspace" => Some(0x33),
        "delete" => Some(0x75),
        "up" => Some(0x7E),
        "down" => Some(0x7D),
        "left" => Some(0x7B),
        "right" => Some(0x7C),
        "home" => Some(0x73),
        "end" => Some(0x77),
        "pageup" => Some(0x74),
        "pagedown" => Some(0x79),
        "f1" => Some(0x7A),
        "f2" => Some(0x78),
        "f3" => Some(0x63),
        "f4" => Some(0x76),
        "f5" => Some(0x60),
        "f6" => Some(0x61),
        "f7" => Some(0x62),
        "f8" => Some(0x64),
        _ => None,
    };
    if let Some(code) = named {
        return Some((code, false));
    }
    let mut chars = key.chars();
    let (Some(character), None) = (chars.next(), chars.next()) else {
        return None;
    };
    // The characters that the US layout types with Shift, by the key's.
    let (base, shift) = match character {
        '!' => ('1', true),
        '@' => ('2', true),
        '#' => ('3', true),
        '$' => ('4', true),
        '%' => ('5', true),
        '^' => ('6', true),
        '&' => ('7', true),
        '*' => ('8', true),
        '(' => ('9', true),
        ')' => ('0', true),
        '_' => ('-', true),
        '+' => ('=', true),
        '{' => ('[', true),
        '}' => (']', true),
        '|' => ('\\', true),
        ':' => (';', true),
        '"' => ('\'', true),
        '<' => (',', true),
        '>' => ('.', true),
        '?' => ('/', true),
        '~' => ('`', true),
        other if other.is_ascii_uppercase() => (other.to_ascii_lowercase(), true),
        other => (other, false),
    };
    let code = match base {
        'a' => 0x00,
        's' => 0x01,
        'd' => 0x02,
        'f' => 0x03,
        'h' => 0x04,
        'g' => 0x05,
        'z' => 0x06,
        'x' => 0x07,
        'c' => 0x08,
        'v' => 0x09,
        'b' => 0x0B,
        'q' => 0x0C,
        'w' => 0x0D,
        'e' => 0x0E,
        'r' => 0x0F,
        'y' => 0x10,
        't' => 0x11,
        '1' => 0x12,
        '2' => 0x13,
        '3' => 0x14,
        '4' => 0x15,
        '6' => 0x16,
        '5' => 0x17,
        '=' => 0x18,
        '9' => 0x19,
        '7' => 0x1A,
        '-' => 0x1B,
        '8' => 0x1C,
        '0' => 0x1D,
        ']' => 0x1E,
        'o' => 0x1F,
        'u' => 0x20,
        '[' => 0x21,
        'i' => 0x22,
        'p' => 0x23,
        'l' => 0x25,
        'j' => 0x26,
        '\'' => 0x27,
        'k' => 0x28,
        ';' => 0x29,
        '\\' => 0x2A,
        ',' => 0x2B,
        '/' => 0x2C,
        'n' => 0x2D,
        'm' => 0x2E,
        '.' => 0x2F,
        '`' => 0x32,
        _ => return None,
    };
    Some((code, shift))
}

/// Where a key goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum To {
    /// The process of this number, alone.
    Process(libc::pid_t),
    /// The window server, as a keyboard's key, which goes to the app in
    /// front.
    System,
}

/// Strikes the key `code`, of no modifier, to `to`: pressed, then released
/// after [`PAUSE`], as a hand strikes it; the time of its press, in
/// nanoseconds of the shared clock, as close before it was posted as can
/// be.
pub fn strike(to: To, code: u16) -> u64 {
    let events = [true, false].map(|down| {
        let event = CGEvent::new_keyboard_event(None, code, down);
        CGEvent::set_flags(event.as_deref(), CGEventFlags::empty());
        event
    });
    let mut pressed = 0;
    for (at, event) in events.iter().enumerate() {
        if at == 0 {
            pressed = crate::clock::now_ns();
        }
        match to {
            To::Process(pid) => CGEvent::post_to_pid(pid, event.as_deref()),
            To::System => CGEvent::post(CGEventTapLocation::HIDEventTap, event.as_deref()),
        }
        thread::sleep(PAUSE);
    }
    pressed
}

/// Clicks at `x`, `y`, in points from the top left of the main display, as
/// a window's frame is placed: the left button pressed and released, of
/// the window server, which sends it to the window there.
pub fn click(x: f64, y: f64) {
    let at = CGPoint { x, y };
    for kind in [CGEventType::LeftMouseDown, CGEventType::LeftMouseUp] {
        let event = CGEvent::new_mouse_event(None, kind, at, CGMouseButton::Left);
        CGEvent::post(CGEventTapLocation::HIDEventTap, event.as_deref());
        thread::sleep(PAUSE);
    }
}

/// Moves the pointer to `x`, `y`, in points from the top left of the main
/// display, through the window server.
pub fn point(x: f64, y: f64) {
    let event = CGEvent::new_mouse_event(
        None,
        CGEventType::MouseMoved,
        CGPoint { x, y },
        CGMouseButton::Left,
    );
    CGEvent::post(CGEventTapLocation::HIDEventTap, event.as_deref());
}

/// A scroll's event: a trackpad's, of its precise pixels and its phases,
/// or a mouse wheel's, of lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scroll {
    /// `pixels` up, down where negative, of the fingers' `phase` and the
    /// momentum's `momentum`, as macOS numbers them: began 1, changed 2
    /// and ended 4 of the fingers', begin 1, continue 2 and end 3 of the
    /// momentum's, 0 of none.
    Pixels {
        /// The pixels.
        pixels: i32,
        /// The fingers' phase.
        phase: i64,
        /// The momentum's phase.
        momentum: i64,
    },
    /// A wheel's step of these lines up, down where negative.
    Lines(i32),
}

/// Posts `scroll` at `x`, `y`, in points from the top left of the main
/// display, through the window server, to the window there.
pub fn scroll(x: f64, y: f64, scroll: Scroll) {
    let (unit, delta) = match scroll {
        Scroll::Pixels { pixels, .. } => (CGScrollEventUnit::Pixel, pixels),
        Scroll::Lines(lines) => (CGScrollEventUnit::Line, lines),
    };
    let Some(event) = CGEvent::new_scroll_wheel_event2(None, unit, 1, delta, 0, 0) else {
        return;
    };
    let event = Some(&*event);
    CGEvent::set_location(event, CGPoint { x, y });
    if let Scroll::Pixels {
        phase, momentum, ..
    } = scroll
    {
        CGEvent::set_integer_value_field(event, CGEventField::ScrollWheelEventIsContinuous, 1);
        CGEvent::set_integer_value_field(event, CGEventField::ScrollWheelEventScrollPhase, phase);
        CGEvent::set_integer_value_field(
            event,
            CGEventField::ScrollWheelEventMomentumPhase,
            momentum,
        );
    }
    CGEvent::post(CGEventTapLocation::HIDEventTap, event);
}

/// Posts the key `code`, down or up, of `flags`, to the process `pid`, of
/// the text `text` if one: the character that the key types on the US
/// layout, which an app that matches its shortcuts by their characters,
/// as a menu does, then reads whatever the keyboard's layout.
fn post(pid: libc::pid_t, code: u16, down: bool, flags: CGEventFlags, text: Option<char>) {
    if let Some(event) = CGEvent::new_keyboard_event(None, code, down) {
        CGEvent::set_flags(Some(&event), flags);
        if let Some(character) = text {
            let mut units = [0u16; 2];
            let units = character.encode_utf16(&mut units);
            // SAFETY: the units live through the call, of the length given.
            unsafe {
                CGEvent::keyboard_set_unicode_string(
                    Some(&event),
                    units.len() as _,
                    units.as_ptr(),
                );
            }
        }
        CGEvent::post_to_pid(pid, Some(&event));
    }
    thread::sleep(PAUSE);
}

/// Presses and releases `key` in the process `pid`, each modifier pressed
/// before it and released after, as a keyboard sends them; whether the key
/// has a code to press.
pub fn press(pid: libc::pid_t, key: &Key) -> bool {
    let Some((code, shifted)) = code(&key.key) else {
        return false;
    };
    let mut held = Vec::new();
    if key.control {
        held.push((CONTROL, CGEventFlags::MaskControl));
    }
    if key.option {
        held.push((OPTION, CGEventFlags::MaskAlternate));
    }
    if key.shift || shifted {
        held.push((SHIFT, CGEventFlags::MaskShift));
    }
    if key.command {
        held.push((COMMAND, CGEventFlags::MaskCommand));
    }
    // The character of a key of one, as the US layout types it.
    let mut chars = key.key.chars();
    let text = match (chars.next(), chars.next()) {
        (Some(character), None) if key.shift && character.is_ascii_lowercase() => {
            Some(character.to_ascii_uppercase())
        }
        (Some(character), None) => Some(character),
        _ => None,
    };
    let mut flags = CGEventFlags::empty();
    for &(modifier, flag) in &held {
        flags |= flag;
        post(pid, modifier, true, flags, None);
    }
    post(pid, code, true, flags, text);
    post(pid, code, false, flags, text);
    for &(modifier, flag) in held.iter().rev() {
        flags &= !flag;
        post(pid, modifier, false, flags, None);
    }
    true
}

/// Types `text` in the process `pid`, a character at a time, each of its
/// text alone, whatever the keyboard's layout, as Typometer types.
pub fn type_text(pid: libc::pid_t, text: &str) {
    for character in text.chars() {
        let mut units = [0u16; 2];
        let units = character.encode_utf16(&mut units);
        for down in [true, false] {
            if let Some(event) = CGEvent::new_keyboard_event(None, 0, down) {
                CGEvent::set_flags(Some(&event), CGEventFlags::empty());
                // SAFETY: the units live through the call, of the length
                // given.
                unsafe {
                    CGEvent::keyboard_set_unicode_string(
                        Some(&event),
                        units.len() as _,
                        units.as_ptr(),
                    );
                }
                CGEvent::post_to_pid(pid, Some(&event));
            }
            thread::sleep(PAUSE);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_key_of_the_config_has_its_code() {
        for name in crate::config::NAMED_KEYS {
            assert!(code(name).is_some(), "{name}");
        }
        for character in (b'!'..=b'~').map(char::from) {
            assert!(code(&character.to_string()).is_some(), "{character}");
        }
        assert_eq!(code("d"), Some((0x02, false)));
        assert_eq!(code("+"), Some((0x18, true)));
        assert_eq!(code("D"), Some((0x02, true)));
        assert_eq!(code("é"), None);
        assert_eq!(code("f12"), None);
    }
}
