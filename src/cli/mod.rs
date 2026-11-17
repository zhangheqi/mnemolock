pub mod decrypt;
pub mod encrypt;
pub mod util;
#[cfg(unix)]
mod unix;
#[cfg(any(windows, test))]
mod windows;

use std::io;
use crossterm::{ExecutableCommand, cursor};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::style::{self, Stylize};
use crossterm::terminal::{self, ClearType};
use crossterm::QueueableCommand;
use mnemolock::error::Error;
use zeroize::Zeroizing;
use crate::cli::util::SecretString;
use crate::defer;

pub const INPUT_MASK: &str = "● ";

pub enum MnemonicType {
    Mnemonic12,
    Mnemonic24,
}

impl MnemonicType {
    const fn word_count(&self) -> usize {
        match self {
            MnemonicType::Mnemonic12 => 12,
            MnemonicType::Mnemonic24 => 24,
        }
    }

    const fn encrypted_word_count(&self) -> usize {
        match self {
            MnemonicType::Mnemonic12 => 24,
            MnemonicType::Mnemonic24 => 36,
        }
    }
}

/// Joins mnemonic words for parsing. BIP39 words only match in lowercase, and the input is masked,
/// so someone typing with Caps Lock on couldn't see what's wrong.
pub fn join_words(words: &[SecretString]) -> Zeroizing<String> {
    fn lowercase(word: &str) -> impl Iterator<Item = char> {
        word.chars().flat_map(char::to_lowercase)
    }
    // Make room for everything first. A string that grows as it's built moves, and leaves copies
    // of the words behind in freed memory.
    let len = words.iter().map(|word| lowercase(word).map(char::len_utf8).sum::<usize>() + 1).sum();
    let mut joined = Zeroizing::new(String::with_capacity(len));
    for (i, word) in words.iter().enumerate() {
        if i > 0 {
            joined.push(' ');
        }
        joined.extend(lowercase(word));
    }
    joined
}

/// Explains why a mnemonic was rejected. The words are masked, so point at the wrong one if we can.
pub fn invalid_mnemonic_text(error: Error) -> String {
    match error {
        Error::Bip39Error(bip39::Error::UnknownWord(i)) => {
            format!("Word {} is not a valid BIP39 word.", i + 1)
        }
        Error::Bip39Error(bip39::Error::InvalidChecksum) => {
            "Checksum mismatch. Check the words and their order.".to_string()
        }
        _ => "Invalid mnemonic.".to_string(),
    }
}

pub fn print_prompt(text: &str) -> io::Result<()> {
    let mut stdout = io::stdout();
    stdout.queue(terminal::DisableLineWrap)?;
    stdout.queue(style::Print(format!("    {text}: ")))?;
    stdout.execute(terminal::EnableLineWrap)?;
    Ok(())
}

pub struct InputFrame {
    title: String,
    keymap: String,
}

impl InputFrame {
    pub fn new<S: Into<String>>(title: S, keymap: &[(&str, &str)]) -> Self {
        let keymap = keymap
            .iter()
            .map(
                |(key, func)| format!(
                    "{}{}",
                    format!(" {key} ").on_dark_grey().white(),
                    format!(" {func} ").on_grey().black(),
                )
            )
            .collect::<Vec<_>>()
            .join(" ");
        Self { title: title.into(), keymap }
    }

    pub fn init(&self) -> io::Result<()> {
        // In raw mode a newline moves down a line, but not back to the first column
        io::stdout().queue(style::Print(format!("{} {}\r\n", " * ".reverse(), self.title)))?;
        Self::print_bottom(&self.keymap)
    }

    pub fn reload_with_error(text: &str) -> io::Result<()> {
        Self::print_bottom(&format!(" {text} ").on_red().white().to_string())
    }

    pub fn reload(&self) -> io::Result<()> {
        Self::print_bottom(&self.keymap)
    }

    pub fn finish(self) -> io::Result<()> {
        let mut stdout = io::stdout();
        stdout.queue(cursor::MoveToColumn(0))?;
        stdout.execute(terminal::Clear(ClearType::FromCursorDown))?;
        Ok(())
    }

    pub fn print_bottom(text: &str) -> io::Result<()> {
        let mut stdout = io::stdout();
        stdout.queue(cursor::MoveToColumn(0))?;
        // Saving cursor position here is wrong. If the current line is the bottommost one,
        // then the next line printed will have the same line number as the current one.
        // If we try to restore cursor position after printing new line, the cursor won't
        // move a bit!
        stdout.queue(terminal::Clear(ClearType::FromCursorDown))?;
        // `cursor::MoveToNextLine(1)` is wrong, because the next line may not exist
        stdout.queue(style::Print("\n"))?;
        stdout.queue(terminal::DisableLineWrap)?;
        stdout.queue(style::Print(format!("    {text}")))?;
        stdout.queue(terminal::EnableLineWrap)?;
        stdout.execute(cursor::MoveToPreviousLine(1))?;
        Ok(())
    }
}

/// Runs the program's prompts with the terminal in raw mode from start to finish.
///
/// Outside raw mode the terminal echoes whatever is typed. Leaving it between prompts, or while a
/// key is derived, would show in the clear any part of a password or mnemonic typed in that gap.
pub fn session(run: impl FnOnce() -> io::Result<()>) -> io::Result<()> {
    #[cfg(unix)]
    {
        unix::protect_memory();
        unix::watch_signals()?;
    }
    let result = {
        terminal::enable_raw_mode()?;
        defer!(end_session());
        run()
    };
    #[cfg(unix)]
    unix::die_if_signaled()?;
    result
}

/// Clears the prompt the session ended on, however it ended, and restores the terminal.
fn end_session() {
    let mut stdout = io::stdout();
    let _ = stdout.queue(cursor::MoveToColumn(0));
    let _ = stdout.execute(terminal::Clear(ClearType::FromCursorDown));
    let _ = terminal::disable_raw_mode();
    #[cfg(unix)]
    unix::wipe_input();
}

/// Reads the next event. Keys are reported once, when pressed. The keyboard isn't read through
/// crossterm: on Windows it also reports key releases, and on Unix it keeps what was typed in
/// memory. On Unix, this fails once the program is told to terminate.
fn read_event() -> io::Result<Event> {
    #[cfg(windows)]
    return windows::read_event();
    #[cfg(unix)]
    unix::read_event()
}

/// Throws away keys that were typed but not read yet.
pub fn discard_pending_input() -> io::Result<()> {
    #[cfg(windows)]
    return windows::discard_pending_input();
    #[cfg(unix)]
    unix::discard_pending_input()
}

/// Asks the terminal where the cursor is: column and row, counted from 0. On Unix, the answer
/// comes in with the keys, so it is read the same way they are.
fn cursor_position() -> io::Result<(u16, u16)> {
    #[cfg(windows)]
    return cursor::position();
    #[cfg(unix)]
    unix::cursor_position()
}

/// Raw mode turns off the terminal's signal keys, so Ctrl+C arrives as a key event instead of
/// interrupting the program.
fn is_ctrl_c(event: &KeyEvent) -> bool {
    matches!(event.code, KeyCode::Char('c' | 'C'))
        && event.modifiers.contains(KeyModifiers::CONTROL)
        && !event.modifiers.contains(KeyModifiers::ALT)
}

/// Whether a character key types text. Ctrl+key and Alt+key are shortcuts, not text, but Windows
/// reports AltGr as Ctrl+Alt, and AltGr types characters such as `@` on many keyboard layouts.
fn is_text(event: &KeyEvent) -> bool {
    event.modifiers.contains(KeyModifiers::CONTROL) == event.modifiers.contains(KeyModifiers::ALT)
}

pub enum ViewWord {
    Prev,
    Next,
    Done,
    Reload,
    Exit,
}

pub fn view_word(word: &str) -> io::Result<ViewWord> {
    // Told to terminate, perhaps while the key was derived: don't show the word even briefly
    #[cfg(unix)]
    unix::check_signal()?;

    let mut stdout = io::stdout();

    stdout.execute(terminal::DisableLineWrap)?;
    defer!(io::stdout().execute(terminal::EnableLineWrap));

    stdout.execute(cursor::Hide)?;
    defer!(io::stdout().execute(cursor::Show));

    stdout.execute(style::Print(word))?;

    loop {
        match read_event()? {
            Event::Key(event) if is_ctrl_c(&event) => return Ok(ViewWord::Exit),
            Event::Key(event) => match event.code {
                KeyCode::Enter => return Ok(ViewWord::Done),
                KeyCode::Left | KeyCode::Up => return Ok(ViewWord::Prev),
                KeyCode::Right | KeyCode::Down => return Ok(ViewWord::Next),
                KeyCode::Esc => return Ok(ViewWord::Exit),
                _ => (),
            }
            Event::Resize(..) => return Ok(ViewWord::Reload),
            _ => (),
        }
    }
}

pub enum EditPwd {
    Submit,
    Reload,
    Exit,
}

pub fn edit_pwd(buf: &mut SecretString, mask: &str) -> io::Result<EditPwd> {
    let mut stdout = io::stdout();

    stdout.execute(terminal::DisableLineWrap)?;
    defer!(io::stdout().execute(terminal::EnableLineWrap));

    let mut cursor_checkpoints = Vec::new();

    for _ in 0..buf.chars().count() {
        cursor_checkpoints.push(cursor_position()?);
        stdout.execute(style::Print(mask))?;
    }

    loop {
        match read_event()? {
            Event::Key(event) if is_ctrl_c(&event) => return Ok(EditPwd::Exit),
            Event::Key(event) => match event.code {
                KeyCode::Char(ch) if is_text(&event) => {
                    buf.push(ch);
                    cursor_checkpoints.push(cursor_position()?);
                    stdout.execute(style::Print(mask))?;
                }
                KeyCode::Backspace => {
                    let Some(pos) = cursor_checkpoints.pop() else {
                        continue;
                    };
                    buf.pop();
                    stdout.queue(cursor::MoveTo(pos.0, pos.1))?;
                    stdout.execute(terminal::Clear(ClearType::UntilNewLine))?;
                }
                KeyCode::Enter => return Ok(EditPwd::Submit),
                KeyCode::Esc => return Ok(EditPwd::Exit),
                _ => (),
            }
            Event::Resize(..) => return Ok(EditPwd::Reload),
            _ => (),
        }
    }

}

pub enum EditWord {
    Prev,
    Next,
    Submit,
    Reload,
    Exit,
}

pub fn edit_word(buf: &mut SecretString, mask: &str) -> io::Result<EditWord> {
    let mut stdout = io::stdout();

    stdout.execute(terminal::DisableLineWrap)?;
    defer!(io::stdout().execute(terminal::EnableLineWrap));

    let mut cursor_checkpoints = Vec::new();

    for _ in 0..buf.chars().count() {
        cursor_checkpoints.push(cursor_position()?);
        stdout.execute(style::Print(mask))?;
    }

    loop {
        match read_event()? {
            Event::Key(event) if is_ctrl_c(&event) => return Ok(EditWord::Exit),
            Event::Key(event) => match event.code {
                KeyCode::Char(ch) if is_text(&event) => {
                    if ch == ' ' {
                        return Ok(EditWord::Next);
                    }
                    buf.push(ch);
                    cursor_checkpoints.push(cursor_position()?);
                    stdout.execute(style::Print(mask))?;
                }
                KeyCode::Backspace => {
                    let Some(pos) = cursor_checkpoints.pop() else {
                        return Ok(EditWord::Prev);
                    };
                    buf.pop();
                    stdout.queue(cursor::MoveTo(pos.0, pos.1))?;
                    stdout.execute(terminal::Clear(ClearType::UntilNewLine))?;
                }
                KeyCode::Enter => return Ok(EditWord::Submit),
                KeyCode::Left | KeyCode::Up => return Ok(EditWord::Prev),
                KeyCode::Right | KeyCode::Down => return Ok(EditWord::Next),
                KeyCode::Esc => return Ok(EditWord::Exit),
                _ => (),
            }
            Event::Resize(..) => return Ok(EditWord::Reload),
            _ => (),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    #[test]
    fn ctrl_c_is_detected() {
        assert!(is_ctrl_c(&key(KeyCode::Char('c'), KeyModifiers::CONTROL)));
        assert!(is_ctrl_c(&key(KeyCode::Char('C'), KeyModifiers::CONTROL | KeyModifiers::SHIFT)));
        assert!(!is_ctrl_c(&key(KeyCode::Char('c'), KeyModifiers::NONE)));
        assert!(!is_ctrl_c(&key(KeyCode::Char('c'), KeyModifiers::CONTROL | KeyModifiers::ALT)));
    }

    #[test]
    fn modified_keys_are_not_text_except_altgr() {
        assert!(is_text(&key(KeyCode::Char('a'), KeyModifiers::NONE)));
        assert!(is_text(&key(KeyCode::Char('A'), KeyModifiers::SHIFT)));
        assert!(!is_text(&key(KeyCode::Char('d'), KeyModifiers::CONTROL)));
        assert!(!is_text(&key(KeyCode::Char('x'), KeyModifiers::ALT)));
        // AltGr on Windows
        assert!(is_text(&key(KeyCode::Char('@'), KeyModifiers::CONTROL | KeyModifiers::ALT)));
    }

    #[test]
    fn words_are_lowercased_and_joined() {
        let words = ["Abandon", "ABOUT", "zoo"].map(|word| {
            let mut secret = SecretString::new();
            word.chars().for_each(|ch| secret.push(ch));
            secret
        });
        let joined = join_words(&words);
        assert_eq!(*joined, "abandon about zoo");
        // Built in place, without growing
        assert!(joined.capacity() <= "abandon about zoo ".len());
    }

    #[test]
    fn invalid_mnemonic_text_points_at_the_problem() {
        assert_eq!(
            invalid_mnemonic_text(Error::Bip39Error(bip39::Error::UnknownWord(6))),
            "Word 7 is not a valid BIP39 word.",
        );
        assert_eq!(
            invalid_mnemonic_text(Error::Bip39Error(bip39::Error::InvalidChecksum)),
            "Checksum mismatch. Check the words and their order.",
        );
        assert_eq!(invalid_mnemonic_text(Error::BadWordCount), "Invalid mnemonic.");
    }
}
