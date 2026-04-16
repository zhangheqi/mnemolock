mod util;

use std::cmp::Ordering;
use std::{io, process};
use bip39::Mnemonic;
use crossterm::{ExecutableCommand, cursor};
use crossterm::event::{self, Event, KeyCode};
use crossterm::style::{self, Stylize};
use crossterm::terminal::{self, ClearType};
use crossterm::QueueableCommand;
use mnemolock::{EncryptedMnemonic24, EncryptedMnemonic36};

use crate::util::BoundedIndex;

enum ViewWord {
    Prev,
    Next,
    Done,
    Reload,
    Exit,
}

fn view_word(word: &str) -> io::Result<ViewWord> {
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

enum EditPwd {
    Submit,
    Reload,
    Exit,
}

fn edit_pwd(buf: &mut String, mask: &str) -> io::Result<EditPwd> {
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

enum EditWord {
    Prev,
    Next,
    Submit,
    Reload,
    Exit,
}

fn edit_word(buf: &mut String, mask: &str) -> io::Result<EditWord> {
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

enum MnemonicType {
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

fn print_title(text: &str) {
    println!("{} {}", " * ".reverse(), text);
}

fn print_prompt(text: &str) -> io::Result<()> {
    let mut stdout = io::stdout();
    stdout.queue(terminal::DisableLineWrap)?;
    stdout.queue(style::Print(format!("    {text}: ")))?;
    stdout.execute(terminal::EnableLineWrap)?;
    Ok(())
}

struct InputFrame {
    title: String,
    keymap: String,
}

impl InputFrame {
    fn new<S: Into<String>>(title: S, keymap: &[(&str, &str)]) -> Self {
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

    fn init(&self) -> io::Result<()> {
        print_title(&self.title);
        Self::print_bottom(&self.keymap)
    }

    fn reload_with_error(text: &str) -> io::Result<()> {
        Self::print_bottom(&format!(" {text} ").on_red().white().to_string())
    }

    fn reload(&self) -> io::Result<()> {
        Self::print_bottom(&self.keymap)
    }

    fn finish(self) -> io::Result<()> {
        let mut stdout = io::stdout();
        stdout.queue(cursor::MoveToColumn(0))?;
        stdout.execute(terminal::Clear(ClearType::FromCursorDown))?;
        Ok(())
    }

    fn exit() -> ! {
        let mut stdout = io::stdout();
        let _ = stdout.queue(cursor::MoveToColumn(0));
        let _ = stdout.execute(terminal::Clear(ClearType::FromCursorDown));
        process::exit(0);
    }

    fn print_bottom(text: &str) -> io::Result<()> {
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

fn main() -> io::Result<()> {
    let mask = "● ";
    let (mnemonic, mnemonic_type) = {
        let frame = InputFrame::new(
            "Enter your mnemonic.",
            &[
                ("Arrows", "Navigate"),
                ("Space", "Next"),
                ("Enter", "Submit"),
                ("Esc", "Exit"),
            ],
        );
        let mut words: [String; MnemonicType::Mnemonic24.word_count()] = Default::default();
        let mut word_no = BoundedIndex::new(
            1,
            1,
            MnemonicType::Mnemonic24.word_count(),
        );
        frame.init()?;
        loop {
            print_prompt(&format!("Word {:#04x}", word_no.value()))?;
            match edit_word(&mut words[word_no.value() - 1], mask)? {
                EditWord::Prev => {
                    word_no.decrement();
                    frame.reload()?;
                }
                EditWord::Next => {
                    word_no.increment();
                    frame.reload()?;
                }
                EditWord::Submit => {
                    let mut mnemonic_type = Some(MnemonicType::Mnemonic24);
                    for i in 0..MnemonicType::Mnemonic24.word_count() {
                        if words[i].is_empty() {
                            match i.cmp(&MnemonicType::Mnemonic12.word_count()) {
                                Ordering::Less => mnemonic_type = None,
                                Ordering::Equal => {
                                    if words[MnemonicType::Mnemonic12.word_count() + 1..MnemonicType::Mnemonic24.word_count()]
                                        .iter()
                                        .all(String::is_empty)
                                    {
                                        mnemonic_type = Some(MnemonicType::Mnemonic12);
                                    } else {
                                        mnemonic_type = None;
                                    }
                                }
                                Ordering::Greater => mnemonic_type = None,
                            }
                            break;
                        }
                    }
                    match mnemonic_type {
                        Some(MnemonicType::Mnemonic12) => {
                            if let Ok(mnemonic) = Mnemonic::parse_normalized(
                                &words[..MnemonicType::Mnemonic12.word_count()].join(" ")
                            )
                            {
                                frame.finish()?;
                                break (mnemonic, MnemonicType::Mnemonic12);
                            } else {
                                InputFrame::reload_with_error("Invalid mnemonic.")?;
                            }
                        }
                        Some(MnemonicType::Mnemonic24) => {
                            if let Ok(mnemonic) = Mnemonic::parse_normalized(
                                &words.join(" ")
                            )
                            {
                                frame.finish()?;
                                break (mnemonic, MnemonicType::Mnemonic24);
                            } else {
                                InputFrame::reload_with_error("Invalid mnemonic.")?;
                            }
                        }
                        None => InputFrame::reload_with_error("Please fill in all the words.")?,
                    }
                }
                EditWord::Reload => frame.reload()?,
                EditWord::Exit => InputFrame::exit(),
            }
        }
    };
    let words = {
        let frame = InputFrame::new(
            "Choose a password to protect your mnemonic.",
            &[
                ("Enter", "Submit"),
                ("Esc", "Exit"),
            ],
        );
        frame.init()?;
        loop {
            let mut pwd = String::new();
            let words = loop {
                print_prompt("Enter Password")?;
                match edit_pwd(&mut pwd, mask)? {
                    EditPwd::Submit => {
                        let result = match mnemonic_type {
                            MnemonicType::Mnemonic12 => EncryptedMnemonic24::new(&mnemonic, pwd.as_bytes())
                                .map(|x| x.words().to_vec()),
                            MnemonicType::Mnemonic24 => EncryptedMnemonic36::new(&mnemonic, pwd.as_bytes())
                                .map(|x| x.words().to_vec()),
                        };
                        match result {
                            Ok(words) => {
                                frame.reload()?;
                                break words;
                            }
                            Err(_) => InputFrame::reload_with_error("Please choose another password.")?,
                        }
                    }
                    EditPwd::Reload => frame.reload()?,
                    EditPwd::Exit => InputFrame::exit(),
                }
            };
            let mut repeat_pwd = String::new();
            loop {
                print_prompt("Repeat Password")?;
                match edit_pwd(&mut repeat_pwd, mask)? {
                    EditPwd::Submit => {
                        frame.reload()?;
                        break;
                    }
                    EditPwd::Reload => frame.reload()?,
                    EditPwd::Exit => InputFrame::exit(),
                }
            };
            if pwd == repeat_pwd {
                frame.finish()?;
                break words;
            }
            InputFrame::reload_with_error("Password does not match.")?;
        }
    };
    {
        let frame = InputFrame::new(
            format!(
                "Encryption successful. View your encrypted mnemonic ({} words) below.",
                words.len(),
            ),
            &[
                ("Arrows", "Navigate"),
                ("Enter", "Done"),
                ("Esc", "Exit"),
            ],
        );
        let mut word_no = BoundedIndex::new(
            1,
            1,
            words.len(),
        );
        frame.init()?;
        loop {
            print_prompt(&format!("Word {:#04x}", word_no.value()))?;
            match view_word(words[word_no.value() - 1])? {
                ViewWord::Prev => {
                    word_no.decrement();
                    frame.reload()?;
                }
                ViewWord::Next => {
                    word_no.increment();
                    frame.reload()?;
                }
                ViewWord::Done => {
                    frame.finish()?;
                    break;
                }
                ViewWord::Reload => frame.reload()?,
                ViewWord::Exit => InputFrame::exit(),
            }
        }
    }
    Ok(())
}
