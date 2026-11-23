//! The keys that a tool types into a terminal, as a hand would: posted to
//! the terminal's process alone, so that none reaches another app,
//! whichever is in front. Posting them takes the Accessibility permission
//! of the app that runs the tool, as macOS grants it to the terminal or
//! the editor that runs a command-line tool.

use std::thread;
use std::time::Duration;

use objc2_core_graphics::{CGEvent, CGEventFlags};

use crate::config::Key;

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

/// Posts the key `code`, down or up, of `flags`, to the process `pid`.
fn post(pid: libc::pid_t, code: u16, down: bool, flags: CGEventFlags) {
    if let Some(event) = CGEvent::new_keyboard_event(None, code, down) {
        CGEvent::set_flags(Some(&event), flags);
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
    let mut flags = CGEventFlags::empty();
    for &(modifier, flag) in &held {
        flags |= flag;
        post(pid, modifier, true, flags);
    }
    post(pid, code, true, flags);
    post(pid, code, false, flags);
    for &(modifier, flag) in held.iter().rev() {
        flags &= !flag;
        post(pid, modifier, false, flags);
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
