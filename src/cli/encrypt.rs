use std::cmp::Ordering;
use std::io;
use bip39::Mnemonic;
use crossterm::{ExecutableCommand, cursor};
use crossterm::event::{self, Event, KeyCode};
use crossterm::style;
use crossterm::terminal::{self, ClearType};
use crossterm::QueueableCommand;
use mnemolock::{EncryptedMnemonic24, EncryptedMnemonic36};
use crate::cli::util::BoundedIndex;
use crate::cli::{self, InputFrame, MnemonicType};
use crate::defer;

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

pub fn encrypt(mask: &str) -> io::Result<()> {
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
            cli::print_prompt(&format!("Word {}", word_no.value()))?;
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
                cli::print_prompt("Enter Password")?;
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
                cli::print_prompt("Repeat Password")?;
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
            cli::print_prompt(&format!("Word {}", word_no.value()))?;
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
