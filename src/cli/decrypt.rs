use std::cmp::Ordering;
use std::{array, io};
use mnemolock::{EncryptedMnemonic24, EncryptedMnemonic36};
use crate::cli::util::BoundedIndex;
use crate::cli::{self, EditPwd, EditWord, INPUT_MASK, InputFrame, MnemonicType, ViewWord};

pub fn decrypt() -> io::Result<()> {
    let mnemonic = {
        let frame = InputFrame::new(
            "Enter your encrypted mnemonic.",
            &[
                ("Arrows", "Navigate"),
                ("Space", "Next"),
                ("Enter", "Submit"),
                ("Esc", "Exit"),
            ],
        );
        let mut words: [String; MnemonicType::Mnemonic24.encrypted_word_count()] = array::repeat(String::new());
        let mut word_no = BoundedIndex::new(
            1,
            1,
            MnemonicType::Mnemonic24.encrypted_word_count(),
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
                    for i in 0..MnemonicType::Mnemonic24.encrypted_word_count() {
                        if words[i].is_empty() {
                            match i.cmp(&MnemonicType::Mnemonic12.encrypted_word_count()) {
                                Ordering::Less => mnemonic_type = None,
                                Ordering::Equal => {
                                    if words[MnemonicType::Mnemonic12.encrypted_word_count() + 1..MnemonicType::Mnemonic24.encrypted_word_count()]
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
                            if let Ok(mnemonic) = EncryptedMnemonic24::from_words(
                                &words[..MnemonicType::Mnemonic12.encrypted_word_count()].join(" ")
                            )
                            {
                                frame.finish()?;
                                break mnemonic.into_enum();
                            } else {
                                InputFrame::reload_with_error("Invalid mnemonic.")?;
                            }
                        }
                        Some(MnemonicType::Mnemonic24) => {
                            if let Ok(mnemonic) = EncryptedMnemonic36::from_words(
                                &words.join(" ")
                            )
                            {
                                frame.finish()?;
                                break mnemonic.into_enum();
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
            "Enter the password used to encrypt your mnemonic.",
            &[
                ("Enter", "Submit"),
                ("Esc", "Exit"),
            ],
        );
        let mut pwd = String::new();
        frame.init()?;
        loop {
            cli::print_prompt("Enter Password")?;
            match cli::edit_pwd(&mut pwd, INPUT_MASK)? {
                EditPwd::Submit => {
                    match mnemonic.decrypt(pwd.as_bytes()) {
                        Ok(decrypted) => {
                            frame.reload()?;
                            break decrypted.words().collect::<Vec<_>>();
                        }
                        Err(_) => InputFrame::reload_with_error("Wrong password.")?,
                    }
                }
                EditPwd::Reload => frame.reload()?,
                EditPwd::Exit => InputFrame::exit(),
            }
        }
    };
    {
        let frame = InputFrame::new(
            format!(
                "Decryption successful. View your original mnemonic ({} words) below.",
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
