use std::ops::Deref;
use zeroize::{Zeroize, Zeroizing};

pub struct Defer<F: FnMut()>(F);

impl<F: FnMut()> Defer<F> {
    pub fn new(func: F) -> Self {
        Self(func)
    }
}

impl<F: FnMut()> Drop for Defer<F> {
    fn drop(&mut self) {
        self.0()
    }
}

#[macro_export]
macro_rules! defer {
    ($target:expr) => {
        let _defer = $crate::cli::util::Defer::new(|| {
            let _ = $target;
        });
    };
}

pub struct BoundedIndex {
    value: usize,
    min: usize,
    max: usize,
}

impl BoundedIndex {
    pub fn new(value: usize, min: usize, max: usize) -> Self {
        Self {
            value,
            min,
            max,
        }
    }

    pub fn increment(&mut self) {
        if self.value < self.max {
            self.value += 1;
        }
    }

    pub fn decrement(&mut self) {
        if self.value > self.min {
            self.value -= 1;
        }
    }

    pub fn value(&self) -> usize {
        self.value
    }
}

/// How much of the stack `wipe_stack` wipes. Deriving a key, the deepest the program goes while
/// handling secrets, uses a small part of this.
const WIPED_STACK_LEN: usize = 256 * 1024;

/// Wipes the part of the stack that earlier calls used and left behind.
///
/// Wiping a value when it's dropped doesn't reach every copy of it: moving a value can leave its
/// bytes where it was, and the libraries used keep keys and words in local variables that they
/// don't wipe. Those copies are on the stack, below the caller's own variables, and that is where
/// the array here goes. Secrets must not be in the caller's own variables, which aren't reached.
#[inline(never)]
pub fn wipe_stack() {
    let mut left_behind = [0u8; WIPED_STACK_LEN];
    left_behind.as_mut_slice().zeroize();
}

/// Text typed by the user, wiped from memory when dropped.
///
/// A `String` that outgrows its buffer moves to a bigger one, and the old one is freed with the
/// text still in it. This wipes the old buffer when it grows.
///
/// This doesn't reach the keys as crossterm read them: its input buffer and event queue keep the
/// most recent ones until later input overwrites them.
pub struct SecretString(Zeroizing<String>);

impl SecretString {
    /// Enough for any BIP39 word, and for most passwords.
    const INITIAL_CAPACITY: usize = 32;

    pub fn new() -> Self {
        Self(Zeroizing::new(String::with_capacity(Self::INITIAL_CAPACITY)))
    }

    pub fn push(&mut self, ch: char) {
        if self.0.len() + ch.len_utf8() > self.0.capacity() {
            let mut grown = Zeroizing::new(String::with_capacity(2 * self.0.capacity()));
            grown.push_str(&self.0);
            // Drops the old buffer, which wipes it
            self.0 = grown;
        }
        self.0.push(ch);
    }

    pub fn pop(&mut self) -> Option<char> {
        self.0.pop()
    }
}

impl Deref for SecretString {
    type Target = str;

    fn deref(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_string_grows_without_moving_until_full() {
        let mut secret = SecretString::new();
        let capacity = secret.0.capacity();
        let start = secret.as_ptr();
        for _ in 0..capacity {
            secret.push('a');
        }
        assert_eq!(secret.as_ptr(), start);

        // Outgrowing the buffer keeps the text, also when a character doesn't fit in what's left
        secret.pop();
        secret.push('🔒');
        assert!(secret.0.capacity() >= 2 * capacity);
        assert_eq!(*secret, format!("{}🔒", "a".repeat(capacity - 1)));
        assert_eq!(secret.pop(), Some('🔒'));
        assert_eq!(secret.len(), capacity - 1);
    }
}
