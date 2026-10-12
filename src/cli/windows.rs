//! Keyboard input on Windows, read straight from the console.
//!
//! crossterm reports both the press and the release of every key on Windows, but not which
//! physical key an event came from, so duplicate releases can't be told apart from the release
//! that carries an Alt code's character. The console's key records say whether a key went down
//! and which key it was, so key presses are taken from those, and releases are ignored except
//! for Alt codes.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

// Virtual-key codes, from the Windows API
const VK_BACK: u16 = 0x08;
const VK_RETURN: u16 = 0x0D;
const VK_MENU: u16 = 0x12; // Alt
const VK_ESCAPE: u16 = 0x1B;
const VK_LEFT: u16 = 0x25;
const VK_UP: u16 = 0x26;
const VK_RIGHT: u16 = 0x27;
const VK_DOWN: u16 = 0x28;
const VK_A: u16 = 0x41;
const VK_Z: u16 = 0x5A;

/// The parts of a console key record (`KEY_EVENT_RECORD`) used here.
struct KeyRecord {
    key_down: bool,
    virtual_key_code: u16,
    /// UTF-16 code unit of the character typed, or 0
    u_char: u16,
    modifiers: KeyModifiers,
}

/// Returns the key press a record represents, if any. `surrogate` holds the first half of a
/// character outside the Basic Multilingual Plane, such as an emoji, until the second half
/// arrives in a later record.
fn translate(record: &KeyRecord, surrogate: &mut Option<u16>) -> Option<KeyEvent> {
    if !record.key_down {
        // An Alt code, such as Alt+0163 for `£`, delivers its character with the release of Alt
        if record.virtual_key_code == VK_MENU {
            return text(record.u_char, KeyModifiers::NONE, surrogate);
        }
        return None;
    }
    let modifiers = record.modifiers;
    let code = match record.virtual_key_code {
        VK_BACK => KeyCode::Backspace,
        VK_RETURN => KeyCode::Enter,
        VK_ESCAPE => KeyCode::Esc,
        // With Num Lock off, some digits of an Alt code arrive as arrow keys
        VK_LEFT..=VK_DOWN if modifiers.contains(KeyModifiers::ALT) => return None,
        VK_LEFT => KeyCode::Left,
        VK_UP => KeyCode::Up,
        VK_RIGHT => KeyCode::Right,
        VK_DOWN => KeyCode::Down,
        // Ctrl+letter types a control character. Report the letter, so Ctrl+C can be recognized.
        vk @ VK_A..=VK_Z if record.u_char == vk - VK_A + 1 => {
            KeyCode::Char(char::from(vk as u8).to_ascii_lowercase())
        }
        _ => return text(record.u_char, modifiers, surrogate),
    };
    Some(KeyEvent::new(code, modifiers))
}

fn text(unit: u16, modifiers: KeyModifiers, surrogate: &mut Option<u16>) -> Option<KeyEvent> {
    let ch = match unit {
        // No character, as for Shift or a dead key, or a control character such as Tab
        0x00..=0x1F | 0x7F => return None,
        0xD800..=0xDBFF => {
            *surrogate = Some(unit);
            return None;
        }
        0xDC00..=0xDFFF => char::decode_utf16([surrogate.take()?, unit]).next()?.ok()?,
        _ => char::from_u32(unit.into())?,
    };
    Some(KeyEvent::new(KeyCode::Char(ch), modifiers))
}

/// Reads the next event. Keys are reported once, when pressed.
#[cfg(windows)]
pub fn read_event() -> std::io::Result<crossterm::event::Event> {
    use crossterm::event::Event;
    use crossterm_winapi::{Console, ControlKeyState, Handle, InputRecord};

    // Control key states, from the Windows API
    const RIGHT_ALT_PRESSED: u32 = 0x0001;
    const LEFT_ALT_PRESSED: u32 = 0x0002;
    const RIGHT_CTRL_PRESSED: u32 = 0x0004;
    const LEFT_CTRL_PRESSED: u32 = 0x0008;
    const SHIFT_PRESSED: u32 = 0x0010;

    fn modifiers(state: &ControlKeyState) -> KeyModifiers {
        let mut modifiers = KeyModifiers::NONE;
        if state.has_state(SHIFT_PRESSED) {
            modifiers |= KeyModifiers::SHIFT;
        }
        if state.has_state(LEFT_CTRL_PRESSED | RIGHT_CTRL_PRESSED) {
            modifiers |= KeyModifiers::CONTROL;
        }
        if state.has_state(LEFT_ALT_PRESSED | RIGHT_ALT_PRESSED) {
            modifiers |= KeyModifiers::ALT;
        }
        modifiers
    }

    let console = Console::from(Handle::current_in_handle()?);
    let mut surrogate = None;
    loop {
        match console.read_single_input_event()? {
            InputRecord::KeyEvent(record) => {
                let record = KeyRecord {
                    key_down: record.key_down,
                    virtual_key_code: record.virtual_key_code,
                    u_char: record.u_char,
                    modifiers: modifiers(&record.control_key_state),
                };
                if let Some(event) = translate(&record, &mut surrogate) {
                    return Ok(Event::Key(event));
                }
            }
            // Windows counts rows and columns from 0
            InputRecord::WindowBufferSizeEvent(record) => {
                return Ok(Event::Resize(
                    (i32::from(record.size.x) + 1) as u16,
                    (i32::from(record.size.y) + 1) as u16,
                ));
            }
            _ => (),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{is_ctrl_c, is_text};

    const VK_NUMPAD1: u16 = 0x61;
    const VK_PACKET: u16 = 0xE7;

    fn down(virtual_key_code: u16, ch: char, modifiers: KeyModifiers) -> KeyRecord {
        KeyRecord { key_down: true, virtual_key_code, u_char: ch as u16, modifiers }
    }

    fn up(virtual_key_code: u16, ch: char, modifiers: KeyModifiers) -> KeyRecord {
        KeyRecord { key_down: false, ..down(virtual_key_code, ch, modifiers) }
    }

    fn char_key(ch: char, modifiers: KeyModifiers) -> Option<KeyEvent> {
        Some(KeyEvent::new(KeyCode::Char(ch), modifiers))
    }

    fn translate_all(records: &[KeyRecord]) -> Vec<KeyEvent> {
        let mut surrogate = None;
        records.iter().filter_map(|record| translate(record, &mut surrogate)).collect()
    }

    #[test]
    fn keys_are_reported_when_pressed_only() {
        let none = KeyModifiers::NONE;
        let events = translate_all(&[
            down(0x41, 'a', none),
            up(0x41, 'a', none),
            down(0x20, ' ', none),
            up(0x20, ' ', none),
            down(VK_BACK, '\x08', none),
            up(VK_BACK, '\x08', none),
            down(VK_RETURN, '\r', none),
            up(VK_RETURN, '\r', none),
            down(VK_ESCAPE, '\x1b', none),
            down(VK_LEFT, '\0', none),
            down(VK_DOWN, '\0', none),
        ]);
        let codes = events.iter().map(|event| event.code).collect::<Vec<_>>();
        assert_eq!(
            codes,
            [
                KeyCode::Char('a'),
                KeyCode::Char(' '),
                KeyCode::Backspace,
                KeyCode::Enter,
                KeyCode::Esc,
                KeyCode::Left,
                KeyCode::Down,
            ],
        );
    }

    #[test]
    fn alt_code_is_typed_after_held_key_repeats_with_shift() {
        let events = translate_all(&[
            down(0x31, '1', KeyModifiers::NONE),
            down(0x31, '1', KeyModifiers::NONE),
            down(0x31, '!', KeyModifiers::SHIFT),
            up(0x31, '!', KeyModifiers::SHIFT),
            down(VK_MENU, '\0', KeyModifiers::ALT),
            down(VK_NUMPAD1, '\0', KeyModifiers::ALT),
            up(VK_NUMPAD1, '\0', KeyModifiers::ALT),
            up(VK_MENU, '£', KeyModifiers::NONE),
        ]);
        assert_eq!(
            events,
            [
                KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE),
                KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE),
                KeyEvent::new(KeyCode::Char('!'), KeyModifiers::SHIFT),
                KeyEvent::new(KeyCode::Char('£'), KeyModifiers::NONE),
            ],
        );
    }

    #[test]
    fn alt_code_is_text_even_if_alt_is_still_reported() {
        let event = translate(&up(VK_MENU, '£', KeyModifiers::ALT), &mut None).unwrap();
        assert_eq!(event, KeyEvent::new(KeyCode::Char('£'), KeyModifiers::NONE));
        assert!(is_text(&event));
    }

    #[test]
    fn alt_code_digits_are_not_typed() {
        // Num Lock on: a digit with Alt held, which isn't text
        let digit = translate(&down(VK_NUMPAD1, '1', KeyModifiers::ALT), &mut None);
        assert!(!digit.is_some_and(|event| is_text(&event)));
        // Num Lock off: an arrow key with Alt held, which mustn't move between words
        assert_eq!(translate(&down(VK_RIGHT, '\0', KeyModifiers::ALT), &mut None), None);
        // Releasing Alt without typing a code
        assert_eq!(translate(&up(VK_MENU, '\0', KeyModifiers::NONE), &mut None), None);
    }

    #[test]
    fn ctrl_c_is_reported_as_the_letter() {
        let event = translate(&down(0x43, '\x03', KeyModifiers::CONTROL), &mut None).unwrap();
        assert!(is_ctrl_c(&event));
        let event = translate(&down(0x44, '\x04', KeyModifiers::CONTROL), &mut None).unwrap();
        assert!(!is_text(&event));
    }

    #[test]
    fn altgr_characters_are_typed() {
        let ctrl_alt = KeyModifiers::CONTROL | KeyModifiers::ALT;
        let event = translate(&down(0x51, '@', ctrl_alt), &mut None);
        assert_eq!(event, char_key('@', ctrl_alt));
        assert!(is_text(&event.unwrap()));
        // AltGr with a key that types nothing on the layout
        assert_eq!(translate(&down(0x41, '\0', ctrl_alt), &mut None), None);
    }

    #[test]
    fn keys_without_a_character_are_ignored() {
        // Shift, a dead key (the accent arrives with the next key), Tab
        assert_eq!(translate(&down(0x10, '\0', KeyModifiers::SHIFT), &mut None), None);
        assert_eq!(translate(&down(0xDE, '\0', KeyModifiers::NONE), &mut None), None);
        assert_eq!(translate(&down(0x09, '\t', KeyModifiers::NONE), &mut None), None);
    }

    #[test]
    fn surrogate_pairs_are_combined() {
        let [high, low] = [0xD83D, 0xDD12]; // 🔒
        let record = |key_down, u_char| KeyRecord {
            key_down,
            virtual_key_code: VK_PACKET,
            u_char,
            modifiers: KeyModifiers::NONE,
        };
        let events = translate_all(&[
            record(true, high),
            record(false, high),
            record(true, low),
            record(false, low),
        ]);
        assert_eq!(events, [KeyEvent::new(KeyCode::Char('🔒'), KeyModifiers::NONE)]);
    }
}
