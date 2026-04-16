pub mod error;

use aes_gcm_siv::{Aes256GcmSiv, Key, KeyInit, Nonce, aead::Aead};
use argon2::Argon2;
use bip39::Mnemonic;
use error::{Error, Result};
use std::ops::{Index, RangeFrom, RangeFull, RangeTo};

const SALT: &[u8; 22] = b"Uom1dTQVPOMypdOKji8axA";
const NONCE: &[u8; 12] = b"j+d5//euNzoM";

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
        let mut key = [0u8; 32];
        Argon2::default().hash_password_into(pwd, SALT, &mut key)?;
        let key = Key::<Aes256GcmSiv>::from_slice(&key);
        let cipher = Aes256GcmSiv::new(key);
        let nonce = Nonce::from_slice(NONCE);
        let ciphertext = cipher.encrypt(nonce, mnemonic.to_entropy().as_ref()).unwrap();
        Ok(
            Self {
                entropy: ciphertext.try_into().map_err(|_| Error::BadWordCount)?,
            }
        )
    }

    pub fn decrypt(&self, pwd: &[u8]) -> Result<Mnemonic> {
        let mut key = [0u8; 32];
        Argon2::default().hash_password_into(pwd, SALT, &mut key)?;
        let key = Key::<Aes256GcmSiv>::from_slice(&key);
        let cipher = Aes256GcmSiv::new(key);
        let nonce = Nonce::from_slice(NONCE);
        Ok(Mnemonic::from_entropy(cipher.decrypt(nonce, &self.entropy[..])?.as_ref()).unwrap())
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
        entropy.append(&mut Mnemonic::parse_normalized(&words[24..].join(" "))?.to_entropy());
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
