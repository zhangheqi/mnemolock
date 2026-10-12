use std::cmp::Ordering;
use std::io;
use bip39::Mnemonic;
use mnemolock::{EncryptedMnemonic24, EncryptedMnemonic36};
use crate::cli::util::BoundedIndex;
use crate::cli::{self, EditPwd, EditWord, INPUT_MASK, InputFrame, MnemonicType, ViewWord};

/// Passwords shorter than this get a warning, since a weak password is all that protects the
/// encrypted mnemonic from anyone who finds it.
const MIN_RECOMMENDED_PWD_LEN: usize = 8;

pub fn encrypt() -> io::Result<()> {
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
            match cli::edit_word(&mut words[word_no.value() - 1], INPUT_MASK)? {
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
                            match Mnemonic::parse_normalized(
                                &cli::join_words(&words[..MnemonicType::Mnemonic12.word_count()])
                            ) {
                                Ok(mnemonic) => {
                                    frame.finish()?;
                                    break (mnemonic, MnemonicType::Mnemonic12);
                                }
                                Err(error) => InputFrame::reload_with_error(
                                    &cli::invalid_mnemonic_text(error.into())
                                )?,
                            }
                        }
                        Some(MnemonicType::Mnemonic24) => {
                            match Mnemonic::parse_normalized(&cli::join_words(&words)) {
                                Ok(mnemonic) => {
                                    frame.finish()?;
                                    break (mnemonic, MnemonicType::Mnemonic24);
                                }
                                Err(error) => InputFrame::reload_with_error(
                                    &cli::invalid_mnemonic_text(error.into())
                                )?,
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
                match cli::edit_pwd(&mut pwd, INPUT_MASK)? {
                    EditPwd::Submit if pwd.is_empty() => {
                        InputFrame::reload_with_error("Password cannot be empty.")?
                    }
                    EditPwd::Submit => {
                        let result = match mnemonic_type {
                            MnemonicType::Mnemonic12 => EncryptedMnemonic24::new(&mnemonic, pwd.as_bytes())
                                .map(|x| x.words().to_vec()),
                            MnemonicType::Mnemonic24 => EncryptedMnemonic36::new(&mnemonic, pwd.as_bytes())
                                .map(|x| x.words().to_vec()),
                        };
                        match result {
                            Ok(words) => {
                                if pwd.chars().count() < MIN_RECOMMENDED_PWD_LEN {
                                    InputFrame::reload_with_error(
                                        "Warning: short passwords are easy to guess.",
                                    )?;
                                } else {
                                    frame.reload()?;
                                }
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
                match cli::edit_pwd(&mut repeat_pwd, INPUT_MASK)? {
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
            match cli::view_word(words[word_no.value() - 1])? {
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
