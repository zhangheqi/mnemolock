//! Runs the program in a pseudo-terminal, to check what reaches the screen.

#![cfg(unix)]

use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::OwnedFd;
use std::os::unix::process::ExitStatusExt;
use std::process::{Child, Command, ExitStatus};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use rustix::fs::{self, Mode, OFlags};
use rustix::process::{self, Pid, Signal};
use rustix::pty::{self, OpenptFlags};
use rustix::termios::{self, LocalModes};

const TIMEOUT: Duration = Duration::from_secs(30);

const MNEMONIC: &str = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
/// `MNEMONIC`, encrypted with `ENCRYPTED_PASSWORD`
const ENCRYPTED: &str = "feature chapter top slight shrimp ranch combine tomato bike canvas volume vendor upper rich very rhythm aspect leaf fame trouble salad bench slow peasant";
const ENCRYPTED_PASSWORD: &str = "correct horse battery staple";
/// None of these letters appears in the program's own text, so seeing one means it was echoed.
const PASSWORD: &str = "QJZXKQJZXK";
/// A 24-word mnemonic, encrypted with `UTF8_PASSWORD`: a test vector from the README
const ENCRYPTED_WITH_UTF8_PASSWORD: &str = "inquiry because trip cabin lottery fire syrup auction sample crazy eye cave cabbage mixture wrist embody talk airport cart reflect fluid ecology amused embark nose test axis scene enlist useless seven alpha gift bulk ivory tribe";
const UTF8_PASSWORD: &str = "pässwörd 🔒";

struct Terminal {
    child: Child,
    master: File,
    slave: OwnedFd,
    /// Everything the terminal was given to show
    output: Arc<Mutex<Vec<u8>>>,
}

/// Opens a pseudo-terminal: the terminal's end of it, and the program's.
fn open_pty() -> (File, OwnedFd) {
    let master = pty::openpt(OpenptFlags::RDWR | OpenptFlags::NOCTTY).unwrap();
    // Not for the program to inherit: the terminal is closed when its end is, and that has to
    // be up to the test
    rustix::io::fcntl_setfd(&master, rustix::io::FdFlags::CLOEXEC).unwrap();
    pty::grantpt(&master).unwrap();
    pty::unlockpt(&master).unwrap();
    let name = pty::ptsname(&master, Vec::new()).unwrap();
    let slave = fs::open(name, OFlags::RDWR | OFlags::NOCTTY, Mode::empty()).unwrap();
    (File::from(master), slave)
}

/// Starts the program in the pseudo-terminal that `slave` is the program's end of.
fn spawn(command: &str, slave: &OwnedFd) -> Child {
    Command::new(env!("CARGO_BIN_EXE_mnemolock"))
        .arg(command)
        .stdin(slave.try_clone().unwrap())
        .stdout(slave.try_clone().unwrap())
        .stderr(slave.try_clone().unwrap())
        .spawn()
        .unwrap()
}

impl Terminal {
    fn run(command: &str) -> Self {
        let (master, slave) = open_pty();
        let child = spawn(command, &slave);
        let output = Arc::new(Mutex::new(Vec::new()));

        // Stands in for the terminal: records what it is given to show, and answers when the
        // program asks where the cursor is, which it does after every key. Any answer will do.
        thread::spawn({
            let mut master = master.try_clone().unwrap();
            let output = Arc::clone(&output);
            move || {
                let mut answered = 0;
                let mut buf = [0; 4096];
                while let Ok(len @ 1..) = master.read(&mut buf) {
                    let mut output = output.lock().unwrap();
                    output.extend_from_slice(&buf[..len]);
                    let asked = output.windows(4).filter(|bytes| bytes == b"\x1b[6n").count();
                    for _ in answered..asked {
                        let _ = master.write_all(b"\x1b[1;1R");
                    }
                    answered = asked;
                }
            }
        });

        Self { child, master, slave, output }
    }

    fn send(&mut self, keys: impl AsRef<[u8]>) {
        self.master.write_all(keys.as_ref()).unwrap();
    }

    /// Sends the signal that tells a program its terminal's window has changed size.
    fn resize(&mut self) {
        process::kill_process(Pid::from_child(&self.child), Signal::WINCH).unwrap();
    }

    /// The text shown so far, without the escape sequences that move the cursor, set colors, and
    /// so on.
    fn shown(&self) -> String {
        let output = self.output.lock().unwrap();
        let mut text = Vec::new();
        let mut bytes = output.iter().copied();
        while let Some(byte) = bytes.next() {
            if byte == 0x1b {
                // `ESC [`, then parameters, up to a final byte in `@` to `~`
                bytes.next();
                bytes.find(|byte| (0x40..=0x7e).contains(byte));
            } else {
                text.push(byte);
            }
        }
        String::from_utf8_lossy(&text).into_owned()
    }

    /// Everything the terminal was given to show so far, escape sequences included.
    fn output(&self) -> String {
        String::from_utf8_lossy(&self.output.lock().unwrap()).into_owned()
    }

    fn wait_until<T>(&mut self, what: &str, mut check: impl FnMut(&mut Self) -> Option<T>) -> T {
        let start = Instant::now();
        loop {
            if let Some(value) = check(self) {
                return value;
            }
            assert!(start.elapsed() < TIMEOUT, "timed out waiting for {what}: {:?}", self.shown());
            thread::sleep(Duration::from_millis(5));
        }
    }

    fn wait_for(&mut self, text: &str) {
        self.wait_for_count(text, 1)
    }

    /// Waits until `text` has been shown `count` times.
    fn wait_for_count(&mut self, text: &str, count: usize) {
        self.wait_until(text, |terminal| {
            (terminal.shown().matches(text).count() >= count).then_some(())
        })
    }

    fn wait_for_exit(&mut self) -> ExitStatus {
        self.wait_until("the program to exit", |terminal| terminal.child.try_wait().unwrap())
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn keys_typed_while_the_key_is_derived_are_not_echoed() {
    let mut terminal = Terminal::run("encrypt");
    terminal.wait_for("Enter your mnemonic");
    terminal.send(MNEMONIC);
    terminal.send("\r");
    terminal.wait_for("Choose a password");
    terminal.send(PASSWORD);
    terminal.send("\r");

    // Retype the password straight away, key by key, while the key is derived from the first one
    thread::sleep(Duration::from_millis(2));
    for key in PASSWORD.chars() {
        terminal.send(key.encode_utf8(&mut [0; 4]));
        thread::sleep(Duration::from_millis(1));
    }
    terminal.send("\r");

    // What was typed ahead is taken as the repeated password
    terminal.wait_for("Encryption successful");
    terminal.send("\x1b");
    assert!(terminal.wait_for_exit().success());

    let shown = terminal.shown();
    assert!(!shown.contains(|ch| PASSWORD.contains(ch)), "the password was echoed: {shown:?}");
}

#[test]
fn termination_clears_the_screen_and_restores_the_terminal() {
    let mut terminal = Terminal::run("decrypt");
    terminal.wait_for("Enter your encrypted mnemonic");
    terminal.send(ENCRYPTED);
    terminal.send("\r");
    terminal.wait_for("Enter the password");
    terminal.send(ENCRYPTED_PASSWORD);
    terminal.send("\r");
    terminal.wait_for("Word 1: abandon");

    process::kill_process(Pid::from_child(&terminal.child), Signal::TERM).unwrap();
    let status = terminal.wait_for_exit();

    // The signal still takes effect, once the program has cleaned up
    assert_eq!(status.signal(), Some(Signal::TERM.as_raw()));
    // The line with the word is erased: back to the first column, then clear to the end
    terminal.wait_until("the word to be erased", |terminal| {
        let output = terminal.output();
        let (_, after_word) = output.rsplit_once("abandon")?;
        after_word.contains("\x1b[1G\x1b[J").then_some(())
    });
    // The terminal is back out of raw mode
    let modes = termios::tcgetattr(&terminal.slave).unwrap().local_modes;
    assert!(modes.contains(LocalModes::ECHO | LocalModes::ICANON));
}

#[test]
fn password_is_the_utf8_bytes_of_what_is_typed() {
    let mut terminal = Terminal::run("decrypt");
    terminal.wait_for("Enter your encrypted mnemonic");
    terminal.send(ENCRYPTED_WITH_UTF8_PASSWORD);
    terminal.send("\r");
    terminal.wait_for("Enter the password");
    // Byte by byte, so that characters of more than one byte arrive in pieces. A character is
    // masked once the terminal has said where the cursor is. The next one waits for that: a
    // terminal doesn't say it halfway through a character, as the one here otherwise might.
    let masked = terminal.shown().matches('●').count();
    for (i, ch) in UTF8_PASSWORD.chars().enumerate() {
        for byte in ch.encode_utf8(&mut [0; 4]).bytes() {
            terminal.send([byte]);
            thread::sleep(Duration::from_millis(2));
        }
        terminal.wait_for_count("●", masked + i + 1);
    }
    terminal.send("\r");

    // It decrypts, so the password came out as the bytes it was encrypted with
    terminal.wait_for("Word 1: legal");
    terminal.send("\x03");
    assert!(terminal.wait_for_exit().success());
}

#[test]
fn keys_that_send_escape_sequences_are_not_typed() {
    let mut terminal = Terminal::run("encrypt");
    terminal.wait_for("Enter your mnemonic");
    // To the next word with Right, Down, Right as `vi` would have it sent, and Ctrl+Right
    for next in ["\x1b[C", "\x1b[B", "\x1bOC", "\x1b[1;5C"] {
        terminal.send("abandon");
        terminal.send(next);
    }
    terminal.wait_for("Word 5");
    // Back with Left and Up, and on again with Space
    terminal.send("\x1b[D\x1b[A");
    terminal.wait_for_count("Word 3", 2);
    terminal.send("  ");
    terminal.wait_for_count("Word 5", 2);
    // Keys that do nothing here: Tab, F5, Delete, Home, Alt+X, Ctrl+D
    for word in 5..12 {
        terminal.send("aban\t\x1b[15~\x1b[3~don\x1b[H\x1bx\x04 ");
        terminal.wait_for(&format!("Word {}", word + 1));
    }
    terminal.send("abouXY\x7f\x7ft\r");

    // The mnemonic is valid, so nothing but the words was typed
    terminal.wait_for("Choose a password");
    // Alt+X starts with what Esc sends, and isn't Esc: the program is still there after it
    terminal.send("\x1bx");
    thread::sleep(Duration::from_millis(200));
    assert!(terminal.child.try_wait().unwrap().is_none());
    terminal.send("\x1b");
    assert!(terminal.wait_for_exit().success());
}

#[test]
fn resizing_the_window_keeps_what_was_typed() {
    let mut terminal = Terminal::run("decrypt");
    terminal.wait_for("Enter your encrypted mnemonic");
    terminal.send(ENCRYPTED);
    terminal.send("\r");
    terminal.wait_for("Enter the password");
    let masked = terminal.shown().matches('●').count();
    let (typed, rest) = ENCRYPTED_PASSWORD.split_at(13);
    terminal.send(typed);
    terminal.wait_for_count("●", masked + typed.len());

    // The prompt is shown again, with as many characters masked as before
    terminal.resize();
    terminal.wait_for_count("Enter Password", 2);
    terminal.wait_for_count("●", masked + 2 * typed.len());
    terminal.send(rest);
    terminal.send("\r");
    terminal.wait_for("Word 1: abandon");
    terminal.send("\x1b");
    assert!(terminal.wait_for_exit().success());
}

#[test]
fn closing_the_terminal_ends_the_program() {
    let (mut master, slave) = open_pty();
    let mut child = spawn("encrypt", &slave);
    drop(slave);
    let mut shown = Vec::new();
    while !String::from_utf8_lossy(&shown).contains("Enter your mnemonic") {
        let mut buf = [0; 4096];
        let len = master.read(&mut buf).unwrap();
        shown.extend_from_slice(&buf[..len]);
    }

    // As when the terminal's window is closed while the program waits for a key
    drop(master);
    let start = Instant::now();
    let mut status = None;
    while status.is_none() && start.elapsed() < TIMEOUT {
        thread::sleep(Duration::from_millis(5));
        status = child.try_wait().unwrap();
    }
    let _ = child.kill();
    let _ = child.wait();
    assert!(!status.expect("the program is still running").success());
}

/// Whether `memory` holds any `len` characters in a row of `secret`: as text, or as 32-bit values
/// a fixed number of bytes apart, which is how they sit in a queue of key events.
#[cfg(target_os = "linux")]
fn holds(memory: &[u8], secret: &str, len: usize) -> bool {
    let as_text = |piece: &[u8]| memory.windows(piece.len()).any(|bytes| bytes == piece);
    if secret.as_bytes().windows(len).any(as_text) {
        return true;
    }
    let chars = secret.chars().map(u32::from).collect::<Vec<_>>();
    let value_at = |at: usize| {
        let bytes = memory.get(at..at + 4)?;
        Some(u32::from_le_bytes(bytes.try_into().unwrap()))
    };
    // Whether the characters of `piece` are here, the first one at `at`, the next `apart` bytes on
    let spaced = |piece: &[u32], at: usize, apart: usize| {
        piece.iter().enumerate().all(|(i, &ch)| value_at(at + i * apart) == Some(ch))
    };
    (0..memory.len()).any(|at| {
        let first = value_at(at);
        let mut pieces = chars.windows(len).filter(|piece| Some(piece[0]) == first);
        pieces.any(|piece| (4..=96).step_by(4).any(|apart| spaced(piece, at, apart)))
    })
}

/// Run this as root, with `--ignored`: the program keeps its memory from other programs of the
/// same user.
#[test]
#[cfg(target_os = "linux")]
#[ignore = "needs root, to read the program's memory"]
fn what_was_typed_is_wiped_from_memory() {
    use std::os::unix::fs::FileExt;

    const WORDS: &str = "legal winner thank year wave sausage worth useful legal winner thank yellow";
    const PASSWORD: &str = "Tr0ub4dor&3-Zq9!xW7#mK2$vB8@nL5^jH6*pY1";

    for pasted in [false, true] {
        let mut terminal = Terminal::run("encrypt");
        let enter = |terminal: &mut Terminal, text: &str| {
            if pasted {
                terminal.send(text);
            } else {
                for key in text.chars() {
                    terminal.send(key.encode_utf8(&mut [0; 4]));
                    thread::sleep(Duration::from_millis(2));
                }
            }
            thread::sleep(Duration::from_millis(50));
            terminal.send("\r");
        };
        terminal.wait_for("Enter your mnemonic");
        enter(&mut terminal, WORDS);
        terminal.wait_for("Choose a password");
        enter(&mut terminal, PASSWORD);
        terminal.wait_for("Repeat Password");
        enter(&mut terminal, PASSWORD);
        terminal.wait_for("Encryption successful");
        thread::sleep(Duration::from_millis(100));

        // What is typed can only be in memory that the program can write to
        let pid = terminal.child.id();
        let maps = std::fs::read_to_string(format!("/proc/{pid}/maps")).unwrap();
        let memory = File::open(format!("/proc/{pid}/mem")).unwrap();
        let mut scanned = 0;
        for line in maps.lines() {
            let mut fields = line.split(' ');
            let (addresses, mode) = (fields.next().unwrap(), fields.next().unwrap());
            if !mode.starts_with("rw") {
                continue;
            }
            let (start, end) = addresses.split_once('-').unwrap();
            let start = u64::from_str_radix(start, 16).unwrap();
            let end = u64::from_str_radix(end, 16).unwrap();
            let mut region = vec![0; (end - start) as usize];
            memory.read_exact_at(&mut region, start).expect("root can read the program's memory");
            assert!(!holds(&region, PASSWORD, 4), "the password is in memory: {line}");
            assert!(!holds(&region, WORDS, 8), "the mnemonic is in memory: {line}");
            scanned += region.len();
        }
        // The heap and the stack, at the least
        assert!(scanned > 128 * 1024);
    }
}
