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

enum EditPwd {
    Pwd(String),
    Reload,
    Exit,
}

fn edit_pwd(mask: &str) -> io::Result<EditPwd> {
    let mut stdout = io::stdout();

    terminal::enable_raw_mode()?;
    defer!(terminal::disable_raw_mode());

    stdout.execute(terminal::DisableLineWrap)?;
    defer!(io::stdout().execute(terminal::EnableLineWrap));

    let mut cursor_checkpoints = Vec::new();
    let mut buf = String::new();

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
                KeyCode::Enter => return Ok(EditPwd::Pwd(buf)),
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

struct WordNo(usize);

impl WordNo {
    const MIN: usize = 1;
    const MID: usize = 12;
    const MAX: usize = 24;

    fn new() -> Self {
        Self(Self::MIN)
    }

    fn print_prompt(&self) {
        let left_arrow = if self.0 <= WordNo::MIN { " " } else { "◀︎" };
        let right_arrow = if self.0 >= WordNo::MAX { " " } else { "▶︎" };
        print_prompt(&format!("Word {} {} {}", left_arrow, self.0, right_arrow));
    }

    fn increment(&mut self) {
        if self.0 < Self::MAX {
            self.0 += 1;
        }
    }

    fn decrement(&mut self) {
        if self.0 > Self::MIN {
            self.0 -= 1;
        }
    }

    fn value(&self) -> usize {
        self.0
    }
}

enum MnemonicType {
    Mnemonic24,
    Mnemonic36,
}

fn print_title(text: &str) {
    println!("{} {}", " * ".reverse(), text);
}

fn print_prompt(text: &str) {
    print!(" ({text}) ");
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
        Self::print_bottom(&format!(" {text} ").white().on_red().to_string())
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
        stdout.queue(style::Print(text))?;
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
        let mut words: [String; WordNo::MAX] = Default::default();
        let mut word_no = WordNo::new();
        frame.init()?;
        loop {
            word_no.print_prompt();
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
                    let mut mnemonic_type = Some(MnemonicType::Mnemonic36);
                    for i in 0..WordNo::MAX {
                        if words[i].is_empty() {
                            match i.cmp(&WordNo::MID) {
                                Ordering::Less => mnemonic_type = None,
                                Ordering::Equal => {
                                    if words[WordNo::MID + 1..WordNo::MAX].iter().all(String::is_empty) {
                                        mnemonic_type = Some(MnemonicType::Mnemonic24);
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
                        Some(MnemonicType::Mnemonic24) => {
                            if let Ok(mnemonic) = Mnemonic::parse_normalized(
                                &words[..WordNo::MID].join(" ")
                            )
                            {
                                frame.finish()?;
                                break (mnemonic, MnemonicType::Mnemonic24);
                            } else {
                                InputFrame::reload_with_error("Invalid mnemonic.")?;
                            }
                        }
                        Some(MnemonicType::Mnemonic36) => {
                            if let Ok(mnemonic) = Mnemonic::parse_normalized(
                                &words.join(" ")
                            )
                            {
                                frame.finish()?;
                                break (mnemonic, MnemonicType::Mnemonic36);
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
            let (pwd, words) = loop {
                print_prompt("Enter Password");
                match edit_pwd(mask)? {
                    EditPwd::Pwd(pwd) => {
                        let result = match mnemonic_type {
                            MnemonicType::Mnemonic24 => EncryptedMnemonic24::new(&mnemonic, pwd.as_bytes())
                                .map(|x| x.words().to_vec()),
                            MnemonicType::Mnemonic36 => EncryptedMnemonic36::new(&mnemonic, pwd.as_bytes())
                                .map(|x| x.words().to_vec()),
                        };
                        match result {
                            Ok(words) => {
                                frame.reload()?;
                                break (pwd, words);
                            }
                            Err(_) => InputFrame::reload_with_error("Please choose another password.")?,
                        }
                    }
                    EditPwd::Reload => frame.reload()?,
                    EditPwd::Exit => InputFrame::exit(),
                }
            };
            let repeat_pwd = loop {
                print_prompt("Repeat Password");
                match edit_pwd(mask)? {
                    EditPwd::Pwd(pwd) => {
                        frame.reload()?;
                        break pwd;
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
    print_title("An encrypted version of your mnemonic has been successfully created:");
    println!("{}", words.join(" "));
    Ok(())
}
