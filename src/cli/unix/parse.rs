//! Turns the bytes a terminal sends into key presses.
//!
//! Only the keys the program uses are recognized: text, Enter, Backspace, Esc, the arrow keys,
//! and Ctrl+letter, which is reported as the letter so that Ctrl+C can be told apart. Anything
//! else is skipped.
//!
//! The rules are those of crossterm 0.29, which read the keyboard before this did, down to what
//! it made of bytes that no keyboard sends. A password is the bytes of the characters typed, so a
//! key that came out as another character here, or as none, would leave someone unable to decrypt
//! what they encrypted earlier.

use std::str;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub const ESC: u8 = 0x1B;

/// Ends text that the terminal marked as pasted, which starts with `ESC [ 200 ~`.
const PASTE_END: &[u8] = b"\x1b[201~";

/// What some bytes from the terminal amount to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Input {
    Key(KeyEvent),
    /// The terminal's answer when asked where the cursor is: column and row, counted from 0
    CursorPosition(u16, u16),
    /// A key the program has no use for, or bytes that make no sense
    Ignored,
}

/// Reads one key press, or whatever else the terminal sent, from the start of `bytes`. Returns it
/// with the number of bytes it takes up, or `None` if it takes more bytes to tell what this is.
///
/// A lone `ESC` is the Esc key, unless the rest of an escape sequence is still on its way. The
/// bytes can't settle that, so it gives `None` and is left to the caller, who can wait and see.
pub fn parse(bytes: &[u8]) -> Option<(usize, Input)> {
    match *bytes.first()? {
        ESC => parse_escape(bytes),
        _ => parse_key(bytes, KeyModifiers::NONE),
    }
}

fn key(code: KeyCode, modifiers: KeyModifiers) -> Input {
    Input::Key(KeyEvent::new(code, modifiers))
}

fn arrow(letter: u8) -> Input {
    let code = match letter {
        b'A' => KeyCode::Up,
        b'B' => KeyCode::Down,
        b'C' => KeyCode::Right,
        b'D' => KeyCode::Left,
        _ => return Input::Ignored,
    };
    key(code, KeyModifiers::NONE)
}

/// A key that is sent as one character.
fn parse_key(bytes: &[u8], modifiers: KeyModifiers) -> Option<(usize, Input)> {
    let first = *bytes.first()?;
    let ctrl = modifiers | KeyModifiers::CONTROL;
    let input = match first {
        b'\r' => key(KeyCode::Enter, modifiers),
        b'\t' => Input::Ignored,
        0x7F => key(KeyCode::Backspace, modifiers),
        // Ctrl+letter sends the letter's place in the alphabet. That makes a line feed Ctrl+J:
        // in raw mode, Enter sends a carriage return.
        0x01..=0x1A => key(KeyCode::Char(char::from(first - 0x01 + b'a')), ctrl),
        0x1C..=0x1F => key(KeyCode::Char(char::from(first - 0x1C + b'4')), ctrl),
        0x00 => key(KeyCode::Char(' '), ctrl),
        _ => {
            let (len, ch) = parse_char(bytes)?;
            let input = ch.map_or(Input::Ignored, |ch| key(KeyCode::Char(ch), modifiers));
            return Some((len, input));
        }
    };
    Some((1, input))
}

/// A character, in UTF-8. Bytes that don't make one are dropped, together with the byte that
/// showed they don't.
fn parse_char(bytes: &[u8]) -> Option<(usize, Option<char>)> {
    let len = match *bytes.first()? {
        0x00..=0x7F => 1,
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        0xF0..=0xF7 => 4,
        // The middle of a character without its start, or a byte that UTF-8 doesn't use
        _ => return Some((1, None)),
    };
    for i in 1..len {
        if *bytes.get(i)? & 0b1100_0000 != 0b1000_0000 {
            return Some((i + 1, None));
        }
    }
    // The right number of bytes can still be wrong, as when they spell a character the long way
    let ch = str::from_utf8(&bytes[..len]).ok().and_then(|text| text.chars().next());
    Some((len, ch))
}

fn parse_escape(bytes: &[u8]) -> Option<(usize, Input)> {
    match *bytes.get(1)? {
        // The Esc key, then something else that starts with `ESC`
        ESC => Some((1, key(KeyCode::Esc, KeyModifiers::NONE))),
        b'[' => parse_csi(bytes),
        // What arrow keys send in the mode `vi` and others put the terminal in
        b'O' => Some((3, arrow(*bytes.get(2)?))),
        // Alt+key sends `ESC`, then the key
        _ => {
            let (len, input) = parse_key(&bytes[1..], KeyModifiers::ALT)?;
            Some((1 + len, input))
        }
    }
}

/// A sequence that starts with `ESC [`.
fn parse_csi(bytes: &[u8]) -> Option<(usize, Input)> {
    let len = match *bytes.get(2)? {
        letter @ b'A'..=b'D' => return Some((3, arrow(letter))),
        b'0'..=b'9' => return parse_csi_with_parameters(bytes),
        // F1 to F5 on the Linux console: one more byte
        b'[' => 4,
        // A mouse click, if the terminal reports those: three more bytes
        b'M' => 6,
        // The same in a newer format, which ends with `M` or `m`
        b'<' => 4 + bytes[3..].iter().position(|byte| matches!(byte, b'M' | b'm'))?,
        // The terminal describing itself, which ends with `c`, or with `u` after something else
        b'?' => 1 + (3..bytes.len()).find(|&i| bytes[i] == b'c' || (bytes[i] == b'u' && i > 3))?,
        _ => 3,
    };
    (bytes.len() >= len).then_some((len, Input::Ignored))
}

/// A sequence that starts with `ESC [` and a digit. Its parameters run up to the first byte from
/// `@` to `~`, which says what the sequence is.
fn parse_csi_with_parameters(bytes: &[u8]) -> Option<(usize, Input)> {
    let end = (3..bytes.len()).find(|&i| (b'@'..=b'~').contains(&bytes[i]))?;
    let (parameters, last) = (&bytes[2..end], bytes[end]);
    if parameters == b"200" && last == b'~' {
        // Pasted text, if the terminal marks it. crossterm reported it as a paste, not as keys,
        // and the program ignored that.
        let pasted = &bytes[end + 1..];
        let len = pasted.windows(PASTE_END.len()).position(|bytes| bytes == PASTE_END)?;
        return Some((end + 1 + len + PASTE_END.len(), Input::Ignored));
    }
    let input = str::from_utf8(parameters).map_or(Input::Ignored, |parameters| match last {
        b'R' => cursor_position(parameters),
        b'u' => numbered_key(parameters),
        b'A'..=b'D' if has_modifier(parameters) => arrow(last),
        _ => Input::Ignored,
    });
    Some((end + 1, input))
}

/// The parameters of `ESC [ row ; column R`, both counted from 1.
fn cursor_position(parameters: &str) -> Input {
    let mut numbers = parameters.split(';').map(str::parse::<u16>);
    match (numbers.next(), numbers.next()) {
        (Some(Ok(row)), Some(Ok(column))) => {
            Input::CursorPosition(column.saturating_sub(1), row.saturating_sub(1))
        }
        _ => Input::Ignored,
    }
}

/// Whether the parameters of an arrow key say which modifier keys are held, as in
/// `ESC [ 1 ; 5 A` for Ctrl+Up, or just `ESC [ 5 A`. An arrow key without that is ignored.
fn has_modifier(parameters: &str) -> bool {
    let modifier = parameters.split(';').nth(1).and_then(|part| part.split(':').next());
    modifier.is_some_and(|modifier| modifier.parse::<u8>().is_ok())
        || parameters.ends_with(|ch: char| ch.is_ascii_digit())
}

/// The parameters of `ESC [ key ; modifiers u`, which names a key by the number of the character
/// it types. A terminal sends these if it was set up to, for keys it has no older way to report,
/// such as Shift+Space.
fn numbered_key(parameters: &str) -> Input {
    let mut parameters = parameters.split(';');
    // After a colon comes the character that the key types with Shift held
    let mut numbers = parameters.next().unwrap_or_default().split(':').map(str::parse::<u32>);
    let Some(Ok(number)) = numbers.next() else {
        return Input::Ignored;
    };
    // One more than a bit for each modifier key held. After a colon comes whether the key was
    // pressed or released, which crossterm passed on and the program never looked at.
    let held = parameters
        .next()
        .and_then(|part| part.split(':').next())
        .and_then(|held| held.parse::<u8>().ok())
        .map_or(0, |held| held.saturating_sub(1));
    let mut modifiers = KeyModifiers::NONE;
    modifiers.set(KeyModifiers::SHIFT, held & 1 != 0);
    modifiers.set(KeyModifiers::ALT, held & 2 != 0);
    modifiers.set(KeyModifiers::CONTROL, held & 4 != 0);

    let mut code = match number {
        // The numeric keypad
        57399..=57408 => Some(KeyCode::Char(char::from(b'0' + (number - 57399) as u8))),
        57409 => Some(KeyCode::Char('.')),
        57410 => Some(KeyCode::Char('/')),
        57411 => Some(KeyCode::Char('*')),
        57412 => Some(KeyCode::Char('-')),
        57413 => Some(KeyCode::Char('+')),
        57414 => Some(KeyCode::Enter),
        57415 => Some(KeyCode::Char('=')),
        57416 => Some(KeyCode::Char(',')),
        57417 => Some(KeyCode::Left),
        57418 => Some(KeyCode::Right),
        57419 => Some(KeyCode::Up),
        57420 => Some(KeyCode::Down),
        // Shift, Ctrl and Alt themselves, left and right, which count as held
        57441 | 57447 => {
            modifiers |= KeyModifiers::SHIFT;
            None
        }
        57442 | 57448 => {
            modifiers |= KeyModifiers::CONTROL;
            None
        }
        57443 | 57449 => {
            modifiers |= KeyModifiers::ALT;
            None
        }
        // Other keys that type nothing: Caps Lock, F13, Page Up, the media keys and so on
        57358..=57363 | 57376..=57398 | 57421..=57454 => None,
        _ => match char::from_u32(number) {
            Some('\x1b') => Some(KeyCode::Esc),
            Some('\r') => Some(KeyCode::Enter),
            Some('\t') => None,
            Some('\x7f') => Some(KeyCode::Backspace),
            Some(ch) => Some(KeyCode::Char(ch)),
            None => return Input::Ignored,
        },
    };
    let shifted = numbers.next().and_then(Result::ok).and_then(char::from_u32);
    if let (true, Some(shifted)) = (modifiers.contains(KeyModifiers::SHIFT), shifted) {
        code = Some(KeyCode::Char(shifted));
        modifiers.remove(KeyModifiers::SHIFT);
    }
    code.map_or(Input::Ignored, |code| key(code, modifiers))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{is_ctrl_c, is_text};

    fn plain(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn char_key(ch: char, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(ch), modifiers)
    }

    /// The key presses in `bytes`, which must not end partway through one.
    fn keys(bytes: &[u8]) -> Vec<KeyEvent> {
        keys_in_pieces(bytes, bytes.len())
    }

    /// The same, when the first `first` bytes arrive before the rest: as much as possible is read
    /// from those before the rest is added.
    fn keys_in_pieces(bytes: &[u8], first: usize) -> Vec<KeyEvent> {
        let mut keys = Vec::new();
        let mut start = 0;
        for end in [first, bytes.len()] {
            while let Some((len, input)) = parse(&bytes[start..end]) {
                start += len;
                if let Input::Key(event) = input {
                    keys.push(event);
                }
            }
        }
        assert_eq!(start, bytes.len(), "{:x?} is left over", &bytes[start..]);
        keys
    }

    fn codes(bytes: &[u8]) -> Vec<KeyCode> {
        keys(bytes).iter().map(|key| key.code).collect()
    }

    /// What typing `bytes` puts in a password.
    fn typed(bytes: &[u8]) -> String {
        let text = |event: KeyEvent| match event.code {
            KeyCode::Char(ch) if is_text(&event) => Some(ch),
            _ => None,
        };
        keys(bytes).into_iter().filter_map(text).collect()
    }

    #[test]
    fn keys_are_recognized() {
        assert_eq!(
            codes(b"a \x7f\r\x1b[D\x1b[A\x1b[C\x1b[B"),
            [
                KeyCode::Char('a'),
                KeyCode::Char(' '),
                KeyCode::Backspace,
                KeyCode::Enter,
                KeyCode::Left,
                KeyCode::Up,
                KeyCode::Right,
                KeyCode::Down,
            ],
        );
    }

    #[test]
    fn text_is_typed_as_its_utf8_bytes() {
        // The password of a test vector in the README, with characters of 1, 2 and 4 bytes
        let password = "pässwörd 🔒";
        assert_eq!(password.as_bytes(), b"p\xc3\xa4ssw\xc3\xb6rd \xf0\x9f\x94\x92");
        assert_eq!(typed(password.as_bytes()), password);

        // Everything a US keyboard types
        let ascii = (b' '..=b'~').collect::<Vec<_>>();
        assert_eq!(typed(&ascii).as_bytes(), ascii);
        // The first and the last character of 2, 3 and 4 bytes. And no normalization: `é` as
        // one character and as `e` with a combining accent stay as they are.
        let text = "\u{80}\u{7ff}\u{800}\u{ffff}\u{10000}\u{10ffff} \u{e9} e\u{301} 日本語";
        assert_eq!(typed(text.as_bytes()), text);
    }

    #[test]
    fn characters_are_kept_whole_when_they_arrive_in_pieces() {
        let lock = "🔒".as_bytes();
        for len in 0..lock.len() {
            assert_eq!(parse(&lock[..len]), None);
        }
        assert_eq!(parse(lock), Some((4, Input::Key(plain(KeyCode::Char('🔒'))))));
        // Only the first character is read, however much follows
        assert_eq!(parse("éa".as_bytes()), Some((2, Input::Key(plain(KeyCode::Char('é'))))));

        // Wherever the input is cut in two, the keys are the same
        let input = "pässwörd 🔒\x1b[1;5C€\x1bOA\x7f\x1bä\x1b[32;2u\r".as_bytes();
        for first in 0..=input.len() {
            assert_eq!(keys_in_pieces(input, first), keys(input), "cut after {first} bytes");
        }
    }

    #[test]
    fn bytes_that_are_not_utf8_are_dropped() {
        // The middle of a character without its start, and bytes that UTF-8 doesn't use
        assert_eq!(typed(b"a\x80b\xbfc\xf8d\xffe"), "abcde");
        // A character spelled the long way, half of a surrogate pair, and one past the last
        assert_eq!(typed(b"a\xc0\xafb\xe0\x80\xafc\xed\xa0\x80d\xf4\x90\x80\x80e"), "abcde");
        // A character that is cut short takes the byte that cuts it short with it
        assert_eq!(typed(b"\xc3ab"), "b");
        assert_eq!(typed(b"\xf0\x9f\x94ab"), "b");
        assert_eq!(keys(b"\xe2\x82\rb"), [plain(KeyCode::Char('b'))]);
    }

    #[test]
    fn lone_esc_waits_for_what_follows() {
        assert_eq!(parse(b""), None);
        assert_eq!(parse(b"\x1b"), None);
        assert_eq!(parse(b"\x1b["), None);
        assert_eq!(parse(b"\x1bO"), None);
        assert_eq!(parse(b"\x1b[1;5"), None);
        // Another `ESC` starts something else, so the first was the Esc key
        assert_eq!(parse(b"\x1b\x1b"), Some((1, Input::Key(plain(KeyCode::Esc)))));
        assert_eq!(parse(b"\x1b\x1b[A"), Some((1, Input::Key(plain(KeyCode::Esc)))));
    }

    #[test]
    fn arrow_keys_are_recognized_in_every_form() {
        let arrows = [KeyCode::Up, KeyCode::Down, KeyCode::Right, KeyCode::Left].map(plain);
        assert_eq!(keys(b"\x1b[A\x1b[B\x1b[C\x1b[D"), arrows);
        assert_eq!(keys(b"\x1bOA\x1bOB\x1bOC\x1bOD"), arrows);
        // With Shift, Alt, Ctrl, and Ctrl+Shift held
        assert_eq!(keys(b"\x1b[1;2A\x1b[1;3B\x1b[1;5C\x1b[1;6D"), arrows);
        // Shorter forms of the same, and one that says the key was pressed, not released
        assert_eq!(keys(b"\x1b[2A\x1b[5B\x1b[1C\x1b[1;1:1D"), arrows);
        // Malformed: the modifier is missing
        assert_eq!(keys(b"\x1b[1;A\x1b[1; B"), []);
    }

    #[test]
    fn ctrl_c_is_reported_as_the_letter() {
        let [event] = keys(b"\x03")[..] else { panic!() };
        assert!(is_ctrl_c(&event));
        let [event] = keys(b"\x04")[..] else { panic!() };
        assert_eq!(event, char_key('d', KeyModifiers::CONTROL));
        assert!(!is_ctrl_c(&event) && !is_text(&event));
        // Alt+Ctrl+C is something else
        let [event] = keys(b"\x1b\x03")[..] else { panic!() };
        assert!(!is_ctrl_c(&event));
    }

    #[test]
    fn ctrl_and_alt_keys_are_not_text() {
        // Ctrl+A to Ctrl+Z, apart from Enter, and the rest of the control characters. A line
        // feed is among them, as Ctrl+J: it isn't Enter in raw mode.
        let control = (0x00..0x20).filter(|byte| ![b'\r', ESC].contains(byte)).collect::<Vec<_>>();
        assert_eq!(typed(&control), "");
        assert_eq!(keys(b"\n"), [char_key('j', KeyModifiers::CONTROL)]);
        assert_eq!(keys(b"\0"), [char_key(' ', KeyModifiers::CONTROL)]);

        // Alt+key
        assert_eq!(keys(b"\x1bx"), [char_key('x', KeyModifiers::ALT)]);
        assert_eq!(keys("\x1bä".as_bytes()), [char_key('ä', KeyModifiers::ALT)]);
        assert_eq!(typed("a\x1bxb\x1bäc\x1b d".as_bytes()), "abcd");
        // Alt doesn't stop Enter and Backspace from working
        assert_eq!(codes(b"\x1b\r\x1b\x7f"), [KeyCode::Enter, KeyCode::Backspace]);
    }

    #[test]
    fn ctrl_alt_keys_type_what_they_did_with_crossterm() {
        // Ctrl+Alt+A arrives as Alt, then Ctrl+A. `is_text` takes Ctrl+Alt for AltGr, which it is
        // on Windows, so this types `a`. It did the same when crossterm read the keyboard, and
        // a password may have been chosen that way.
        assert_eq!(keys(b"\x1b\x01"), [char_key('a', KeyModifiers::CONTROL | KeyModifiers::ALT)]);
        assert_eq!(typed(b"\x1b\x01\x1b\x1a\x1b\n\x1b\0\x1b\x1c"), "azj 4");
    }

    #[test]
    fn numbered_keys_are_recognized() {
        // Shift+Space and Shift+Enter, from a terminal set up to tell them from Space and Enter
        assert_eq!(keys(b"\x1b[32;2u"), [char_key(' ', KeyModifiers::SHIFT)]);
        assert_eq!(typed(b"a\x1b[32;2ub"), "a b");
        assert_eq!(codes(b"\x1b[13;2u"), [KeyCode::Enter]);
        // Esc, Enter and Backspace, from a terminal that reports every key this way
        assert_eq!(
            codes(b"\x1b[27u\x1b[13u\x1b[127u"),
            [KeyCode::Esc, KeyCode::Enter, KeyCode::Backspace],
        );
        assert_eq!(typed("\x1b[97u\x1b[228u\x1b[128274u\x1b[97;1:1u".as_bytes()), "aä🔒a");
        // With the character that Shift makes of the key, when the terminal gives it
        assert_eq!(typed(b"\x1b[97;2u\x1b[97:65;2u"), "aA");

        // Ctrl+C, Ctrl+A, Alt+A
        let [ctrl_c, ctrl_a, alt_a] = keys(b"\x1b[99;5u\x1b[97;5u\x1b[97;3u")[..] else { panic!() };
        assert!(is_ctrl_c(&ctrl_c));
        assert!(!is_text(&ctrl_a) && !is_text(&alt_a));

        // The numeric keypad: 1, Enter, and its arrow keys
        assert_eq!(typed(b"\x1b[57400u\x1b[57409u"), "1.");
        assert_eq!(
            codes(b"\x1b[57414u\x1b[57417u\x1b[57420u"),
            [KeyCode::Enter, KeyCode::Left, KeyCode::Down],
        );

        // Tab, Caps Lock, F13, left Shift, and numbers that aren't characters
        assert_eq!(keys(b"\x1b[9u\x1b[57358u\x1b[57376u\x1b[57441;2u\x1b[55296u\x1b[1114112u"), []);
    }

    #[test]
    fn keys_the_program_has_no_use_for_are_skipped() {
        let skipped: [&[u8]; 16] = [
            b"\t",
            b"\x1b\t",
            b"\x1b[Z",           // Shift+Tab
            b"\x1bOP",           // F1
            b"\x1b[[A",          // F1 on the Linux console
            b"\x1b[15~",         // F5
            b"\x1b[H",           // Home
            b"\x1bOF",           // End
            b"\x1b[3~",          // Delete
            b"\x1b[5;5~",        // Ctrl+Page Up
            b"\x1b[27;2;13~",    // Shift+Enter, the way xterm can be set up to report it
            b"\x1b[I",           // The terminal's window was switched to
            b"\x1b[M !!",        // A mouse click
            b"\x1b[<0;10;20M",   // The same, in the newer format
            b"\x1b[?64;1;2c",    // The terminal describing itself
            b"\x1b[?1u",
        ];
        for bytes in skipped {
            assert_eq!(parse(bytes), Some((bytes.len(), Input::Ignored)), "{bytes:x?}");
            // No more than the key is skipped, and no less
            assert_eq!(parse(&bytes[..bytes.len() - 1]), None, "{bytes:x?}");
            assert_eq!(typed(&[b"a", bytes, b"b"].concat()), "ab", "{bytes:x?}");
        }
        // Sequences that mean nothing end where crossterm took them to end
        assert_eq!(typed(b"\x1b[xa\x1bOxb\x1b[;c"), "abc");
    }

    #[test]
    fn text_marked_as_pasted_is_skipped() {
        let paste = b"\x1b[200~correct horse\r\x1b[A \x1b[201~";
        assert_eq!(parse(paste), Some((paste.len(), Input::Ignored)));
        assert_eq!(parse(&paste[..paste.len() - 1]), None);
        assert_eq!(typed(&[b"a", &paste[..], b"b"].concat()), "ab");
    }

    #[test]
    fn cursor_position_is_recognized() {
        // Row 24, column 80
        assert_eq!(parse(b"\x1b[24;80R"), Some((8, Input::CursorPosition(79, 23))));
        assert_eq!(parse(b"\x1b[1;1Rabc"), Some((6, Input::CursorPosition(0, 0))));
        assert_eq!(parse(b"\x1b[24;80"), None);
        assert_eq!(parse(b"\x1b[24R"), Some((5, Input::Ignored)));
        assert_eq!(parse(b"\x1b[24; R"), Some((7, Input::Ignored)));
    }
}
