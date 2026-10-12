pub mod error;

use aes_gcm_siv::{Aes256GcmSiv, Key, KeyInit, Nonce, aead::Aead};
use argon2::{Algorithm, Argon2, Params, Version};
use bip39::Mnemonic;
use error::{Error, Result};
use std::ops::{Index, RangeFrom, RangeFull, RangeTo};

const SALT: &[u8; 22] = b"Uom1dTQVPOMypdOKji8axA";
const NONCE: &[u8; 12] = b"j+d5//euNzoM";

// Argon2id parameters. Like the salt and nonce, these are part of the encrypted format: changing
// any of them makes existing encrypted mnemonics undecryptable. They are spelled out rather than
// taken from `Argon2::default()`, whose values could change in a future release of the crate.
const ARGON2_M_COST: u32 = 19 * 1024; // KiB
const ARGON2_T_COST: u32 = 2;
const ARGON2_P_COST: u32 = 1;
const KEY_LEN: usize = 32;

fn cipher(pwd: &[u8]) -> Result<Aes256GcmSiv> {
    let params = Params::new(ARGON2_M_COST, ARGON2_T_COST, ARGON2_P_COST, Some(KEY_LEN))?;
    let mut key = [0u8; KEY_LEN];
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params).hash_password_into(pwd, SALT, &mut key)?;
    Ok(Aes256GcmSiv::new(Key::<Aes256GcmSiv>::from_slice(&key)))
}

pub trait Entropy
where
    Self: TryFrom<Vec<u8>>
        + Index<RangeFull, Output = [u8]>
        + Index<RangeFrom<usize>, Output = [u8]>
        + Index<RangeTo<usize>, Output = [u8]>
        + sealed::Sealed,
{}

impl Entropy for [u8; 32] {}
impl Entropy for [u8; 48] {}

mod sealed {
    pub trait Sealed {}
    impl Sealed for [u8; 32] {}
    impl Sealed for [u8; 48] {}
}

pub enum EncryptedMnemonic {
    EncryptedMnemonic24(EncryptedMnemonic24),
    EncryptedMnemonic36(EncryptedMnemonic36),
}

impl EncryptedMnemonic {
    pub fn decrypt(&self, pwd: &[u8]) -> Result<Mnemonic> {
        match self {
            EncryptedMnemonic::EncryptedMnemonic24(mnemonic) => mnemonic.decrypt(pwd),
            EncryptedMnemonic::EncryptedMnemonic36(mnemonic) => mnemonic.decrypt(pwd),
        }
    }
}

pub type EncryptedMnemonic24 = _EncryptedMnemonic<[u8; 32]>;
pub type EncryptedMnemonic36 = _EncryptedMnemonic<[u8; 48]>;

pub struct _EncryptedMnemonic<E: Entropy> {
    entropy: E,
}

impl<E: Entropy> _EncryptedMnemonic<E> {
    pub fn new(mnemonic: &Mnemonic, pwd: &[u8]) -> Result<Self> {
        let nonce = Nonce::from_slice(NONCE);
        let ciphertext = cipher(pwd)?.encrypt(nonce, mnemonic.to_entropy().as_ref()).unwrap();
        Ok(
            Self {
                entropy: ciphertext.try_into().map_err(|_| Error::BadWordCount)?,
            }
        )
    }

    pub fn decrypt(&self, pwd: &[u8]) -> Result<Mnemonic> {
        let nonce = Nonce::from_slice(NONCE);
        Ok(Mnemonic::from_entropy(cipher(pwd)?.decrypt(nonce, &self.entropy[..])?.as_ref()).unwrap())
    }
}

impl EncryptedMnemonic24 {
    pub fn from_words(words: &str) -> Result<Self> {
        Ok(
            Self {
                entropy: Mnemonic::parse_normalized(words)?
                    .to_entropy()
                    .try_into()
                    .map_err(|_| Error::BadWordCount)?,
            }
        )
    }

    pub fn words(&self) -> [&'static str; 24] {
        let mnemonic = Mnemonic::from_entropy(&self.entropy[..]).unwrap();
        let mut words = [""; 24];
        for (i, word) in mnemonic.words().enumerate() {
            words[i] = word;
        }
        words
    }

    pub fn into_enum(self) -> EncryptedMnemonic {
        EncryptedMnemonic::EncryptedMnemonic24(self)
    }
}

impl EncryptedMnemonic36 {
    pub fn from_words(words: &str) -> Result<Self> {
        let words = words.split_whitespace().collect::<Vec<_>>();
        if words.len() != 36 {
            return Err(Error::BadWordCount);
        }
        let mut entropy = Mnemonic::parse_normalized(&words[..24].join(" "))?.to_entropy();
        entropy.append(
            &mut Mnemonic::parse_normalized(&words[24..].join(" "))
                // Report the unknown word's position in all 36 words, not in the last 12
                .map_err(|error| match error {
                    bip39::Error::UnknownWord(i) => bip39::Error::UnknownWord(24 + i),
                    error => error,
                })?
                .to_entropy(),
        );
        Ok(
            Self {
                entropy: entropy.try_into().unwrap(),
            }
        )
    }

    pub fn words(&self) -> [&'static str; 36] {
        let mnemonic_1 = Mnemonic::from_entropy(&self.entropy[..32]).unwrap();
        let mnemonic_2 = Mnemonic::from_entropy(&self.entropy[32..]).unwrap();
        let mut words = [""; 36];
        for (i, word) in mnemonic_1.words().chain(mnemonic_2.words()).enumerate() {
            words[i] = word;
        }
        words
    }

    pub fn into_enum(self) -> EncryptedMnemonic {
        EncryptedMnemonic::EncryptedMnemonic36(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MNEMONIC_12: &str = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
    const MNEMONIC_24: &str = "legal winner thank year wave sausage worth useful legal winner thank year wave sausage worth useful legal winner thank year wave sausage worth title";

    // Known-answer vectors. If these change, previously encrypted mnemonics can no longer be
    // decrypted: check for a dependency upgrade or a change to the constants above.
    const PWD_12: &str = "correct horse battery staple";
    const ENCRYPTED_12: &str = "feature chapter top slight shrimp ranch combine tomato bike canvas volume vendor upper rich very rhythm aspect leaf fame trouble salad bench slow peasant";
    const PWD_24: &str = "pässwörd 🔒";
    const ENCRYPTED_24: &str = "inquiry because trip cabin lottery fire syrup auction sample crazy eye cave cabbage mixture wrist embody talk airport cart reflect fluid ecology amused embark nose test axis scene enlist useless seven alpha gift bulk ivory tribe";

    fn mnemonic(words: &str) -> Mnemonic {
        Mnemonic::parse_normalized(words).unwrap()
    }

    #[test]
    fn encrypt_12_matches_known_answer() {
        let encrypted = EncryptedMnemonic24::new(&mnemonic(MNEMONIC_12), PWD_12.as_bytes()).unwrap();
        assert_eq!(encrypted.words().join(" "), ENCRYPTED_12);
    }

    #[test]
    fn encrypt_24_matches_known_answer() {
        let encrypted = EncryptedMnemonic36::new(&mnemonic(MNEMONIC_24), PWD_24.as_bytes()).unwrap();
        assert_eq!(encrypted.words().join(" "), ENCRYPTED_24);
    }

    #[test]
    fn decrypt_12_known_answer() {
        let encrypted = EncryptedMnemonic24::from_words(ENCRYPTED_12).unwrap().into_enum();
        assert_eq!(encrypted.decrypt(PWD_12.as_bytes()).unwrap(), mnemonic(MNEMONIC_12));
    }

    #[test]
    fn decrypt_24_known_answer() {
        let encrypted = EncryptedMnemonic36::from_words(ENCRYPTED_24).unwrap().into_enum();
        assert_eq!(encrypted.decrypt(PWD_24.as_bytes()).unwrap(), mnemonic(MNEMONIC_24));
    }

    #[test]
    fn decrypt_with_wrong_password_fails() {
        let encrypted = EncryptedMnemonic24::from_words(ENCRYPTED_12).unwrap();
        assert!(matches!(encrypted.decrypt(b"wrong password"), Err(Error::AeadError(_))));
        let encrypted = EncryptedMnemonic36::from_words(ENCRYPTED_24).unwrap();
        assert!(matches!(encrypted.decrypt(b"wrong password"), Err(Error::AeadError(_))));
    }

    #[test]
    fn round_trip_through_words() {
        let original = Mnemonic::from_entropy(&[0x5a; 16]).unwrap();
        let words = EncryptedMnemonic24::new(&original, b"pwd").unwrap().words().join(" ");
        let decrypted = EncryptedMnemonic24::from_words(&words).unwrap().decrypt(b"pwd").unwrap();
        assert_eq!(decrypted, original);

        let original = Mnemonic::from_entropy(&[0xa5; 32]).unwrap();
        let words = EncryptedMnemonic36::new(&original, b"pwd").unwrap().words().join(" ");
        let decrypted = EncryptedMnemonic36::from_words(&words).unwrap().decrypt(b"pwd").unwrap();
        assert_eq!(decrypted, original);
    }

    #[test]
    fn encrypting_with_mismatched_length_fails() {
        assert_eq!(
            EncryptedMnemonic24::new(&mnemonic(MNEMONIC_24), b"pwd").err(),
            Some(Error::BadWordCount),
        );
        assert_eq!(
            EncryptedMnemonic36::new(&mnemonic(MNEMONIC_12), b"pwd").err(),
            Some(Error::BadWordCount),
        );
    }

    #[test]
    fn from_words_reports_unknown_word_position() {
        let mut words = ENCRYPTED_24.split(' ').collect::<Vec<_>>();
        words[3] = "notaword";
        assert_eq!(
            EncryptedMnemonic36::from_words(&words.join(" ")).err(),
            Some(Error::Bip39Error(bip39::Error::UnknownWord(3))),
        );

        let mut words = ENCRYPTED_24.split(' ').collect::<Vec<_>>();
        words[30] = "notaword";
        assert_eq!(
            EncryptedMnemonic36::from_words(&words.join(" ")).err(),
            Some(Error::Bip39Error(bip39::Error::UnknownWord(30))),
        );
    }

    #[test]
    fn from_words_rejects_wrong_word_count() {
        assert_eq!(EncryptedMnemonic24::from_words(MNEMONIC_12).err(), Some(Error::BadWordCount));
        assert_eq!(EncryptedMnemonic36::from_words(MNEMONIC_24).err(), Some(Error::BadWordCount));
    }
}
