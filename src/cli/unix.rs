//! Termination signals on Unix.
//!
//! Left alone, a signal such as SIGTERM kills the program on the spot: the terminal stays in raw
//! mode, and a decrypted word that was being shown stays on screen. So the signals are caught
//! and only noted. Reading input fails once one is noted, which ends the session the same way an
//! I/O error would, with the screen cleared and the terminal restored. Then the program dies by
//! the signal after all, so whoever sent it sees it take effect.

use std::io;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock};
use std::time::Duration;
use crossterm::event::{self, Event};
use signal_hook::consts::{SIGHUP, SIGINT, SIGQUIT, SIGTERM};

/// How long a signal can go unnoticed while waiting for input.
const CHECK_INTERVAL: Duration = Duration::from_millis(50);

/// The signal received, or 0.
static RECEIVED: LazyLock<Arc<AtomicUsize>> = LazyLock::new(Arc::default);

/// Starts noting termination signals instead of dying by them.
pub fn watch_signals() -> io::Result<()> {
    for signal in [SIGHUP, SIGINT, SIGQUIT, SIGTERM] {
        signal_hook::flag::register_usize(signal, Arc::clone(&RECEIVED), signal as usize)?;
    }
    Ok(())
}

/// Fails if a termination signal has been received.
pub fn check_signal() -> io::Result<()> {
    match RECEIVED.load(Ordering::Relaxed) {
        0 => Ok(()),
        _ => Err(io::Error::new(io::ErrorKind::Interrupted, "terminated by a signal")),
    }
}

/// Dies by the termination signal received, if any, as if it had never been caught.
pub fn die_if_signaled() -> io::Result<()> {
    match RECEIVED.load(Ordering::Relaxed) {
        0 => Ok(()),
        signal => signal_hook::low_level::emulate_default_handler(signal as i32),
    }
}

/// Reads the next event, or fails once a termination signal is received.
pub fn read_event() -> io::Result<Event> {
    loop {
        check_signal()?;
        if event::poll(CHECK_INTERVAL)? {
            return event::read();
        }
    }
}
