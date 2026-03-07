#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Error {
    Argon2Error(argon2::Error),
    AeadError(aes_gcm_siv::Error),
    BadWordCount,
    Bip39Error(bip39::Error),
}

pub type Result<T> = core::result::Result<T, Error>;

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Argon2Error(error) => error.fmt(f),
            Error::AeadError(error) => error.fmt(f),
            Error::BadWordCount => write!(f, "bad word count"),
            Error::Bip39Error(error) => error.fmt(f),
        }
    }
}

impl From<argon2::Error> for Error {
    fn from(value: argon2::Error) -> Self {
        Self::Argon2Error(value)
    }
}

impl From<aes_gcm_siv::Error> for Error {
    fn from(value: aes_gcm_siv::Error) -> Self {
        Self::AeadError(value)
    }
}

impl From<bip39::Error> for Error {
    fn from(value: bip39::Error) -> Self {
        Self::Bip39Error(value)
    }
}
