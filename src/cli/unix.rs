//! Signals on Unix, and keeping memory private. Keyboard input is in `input`.
//!
//! Left alone, a signal such as SIGTERM kills the program on the spot: the terminal stays in raw
//! mode, and a decrypted word that was being shown stays on screen. So the signals are caught
//! and only noted. Reading input fails once one is noted, which ends the session the same way an
//! I/O error would, with the screen cleared and the terminal restored. Then the program dies by
//! the signal after all, so whoever sent it sees it take effect.

mod input;
mod parse;

use std::io;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock};
use std::time::Duration;
use rustix::process::{self, Resource, Rlimit};
use signal_hook::consts::{SIGHUP, SIGINT, SIGQUIT, SIGTERM, SIGWINCH};

pub use input::{cursor_position, discard_pending_input, read_event, wipe as wipe_input};

/// How long a signal can go unnoticed while waiting for input, if it arrives just before the
/// wait starts. One that arrives during the wait ends it.
const CHECK_INTERVAL: Duration = Duration::from_millis(50);

/// The termination signal received, or 0.
static RECEIVED: LazyLock<Arc<AtomicUsize>> = LazyLock::new(Arc::default);

/// Whether the terminal's window has changed size since `take_resize` last asked.
static RESIZED: LazyLock<Arc<AtomicBool>> = LazyLock::new(Arc::default);

/// Keeps the program's memory, and the secrets in it, out of core dumps. On Linux, this also
/// keeps other programs of the same user from reading it, as a debugger would.
///
/// This is best effort: failing here is no reason to keep someone from their mnemonic.
pub fn protect_memory() {
    let no_core_dump = Rlimit { current: Some(0), maximum: Some(0) };
    let _ = process::setrlimit(Resource::Core, no_core_dump);
    #[cfg(any(target_os = "linux", target_os = "android"))]
    let _ = process::set_dumpable_behavior(process::DumpableBehavior::NotDumpable);
}

/// Starts noting termination signals instead of dying by them, and noting when the terminal's
/// window changes size.
pub fn watch_signals() -> io::Result<()> {
    for signal in [SIGHUP, SIGINT, SIGQUIT, SIGTERM] {
        signal_hook::flag::register_usize(signal, Arc::clone(&RECEIVED), signal as usize)?;
    }
    signal_hook::flag::register(SIGWINCH, Arc::clone(&RESIZED))?;
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

/// Whether the terminal's window has changed size since this was last called.
fn take_resize() -> bool {
    RESIZED.swap(false, Ordering::Relaxed)
}
