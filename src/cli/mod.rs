pub mod decrypt;
pub mod encrypt;
pub mod util;

use std::{io, process};
use crossterm::{ExecutableCommand, cursor};
use crossterm::event::{self, Event, KeyCode};
use crossterm::style::{self, Stylize};
use crossterm::terminal::{self, ClearType};
use crossterm::QueueableCommand;
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
        println!("{} {}", " * ".reverse(), self.title);
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

    pub fn exit() -> ! {
        let mut stdout = io::stdout();
        let _ = stdout.queue(cursor::MoveToColumn(0));
        let _ = stdout.execute(terminal::Clear(ClearType::FromCursorDown));
        process::exit(0);
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

pub enum ViewWord {
    Prev,
    Next,
    Done,
    Reload,
    Exit,
}

pub fn view_word(word: &str) -> io::Result<ViewWord> {
    let mut stdout = io::stdout();

    terminal::enable_raw_mode()?;
    defer!(terminal::disable_raw_mode());

    stdout.execute(terminal::DisableLineWrap)?;
    defer!(io::stdout().execute(terminal::EnableLineWrap));

    stdout.execute(cursor::Hide)?;
    defer!(io::stdout().execute(cursor::Show));

    stdout.execute(style::Print(word))?;

    loop {
        match event::read()? {
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

pub fn edit_pwd(buf: &mut String, mask: &str) -> io::Result<EditPwd> {
    let mut stdout = io::stdout();

    terminal::enable_raw_mode()?;
    defer!(terminal::disable_raw_mode());

    stdout.execute(terminal::DisableLineWrap)?;
    defer!(io::stdout().execute(terminal::EnableLineWrap));

    let mut cursor_checkpoints = Vec::new();

    for _ in 0..buf.chars().count() {
        cursor_checkpoints.push(cursor::position()?);
        stdout.execute(style::Print(mask))?;
    }

    loop {
        match event::read()? {
            Event::Key(event) => match event.code {
                KeyCode::Char(ch) => {
                    buf.push(ch);
                    cursor_checkpoints.push(cursor::position()?);
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

pub fn edit_word(buf: &mut String, mask: &str) -> io::Result<EditWord> {
    let mut stdout = io::stdout();

    terminal::enable_raw_mode()?;
    defer!(terminal::disable_raw_mode());

    stdout.execute(terminal::DisableLineWrap)?;
    defer!(io::stdout().execute(terminal::EnableLineWrap));

    let mut cursor_checkpoints = Vec::new();

    for _ in 0..buf.chars().count() {
        cursor_checkpoints.push(cursor::position()?);
        stdout.execute(style::Print(mask))?;
    }

    loop {
        match event::read()? {
            Event::Key(event) => match event.code {
                KeyCode::Char(ch) => {
                    if ch == ' ' {
                        return Ok(EditWord::Next);
                    }
                    buf.push(ch);
                    cursor_checkpoints.push(cursor::position()?);
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
