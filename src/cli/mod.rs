pub mod encrypt;
pub mod util;

use std::{io, process};
use crossterm::{ExecutableCommand, cursor};
use crossterm::style::{self, Stylize};
use crossterm::terminal::{self, ClearType};
use crossterm::QueueableCommand;

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
