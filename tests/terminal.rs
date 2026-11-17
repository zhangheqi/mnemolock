//! Runs the program in a pseudo-terminal, to check what reaches the screen.

#![cfg(unix)]

use std::fs::File;
use std::io::{Read, Write};
use std::process::{Child, Command, ExitStatus};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use rustix::fs::{self, Mode, OFlags};
use rustix::pty::{self, OpenptFlags};

const TIMEOUT: Duration = Duration::from_secs(30);

const MNEMONIC: &str = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
/// None of these letters appears in the program's own text, so seeing one means it was echoed.
const PASSWORD: &str = "QJZXKQJZXK";

struct Terminal {
    child: Child,
    master: File,
    /// Everything the terminal was given to show
    output: Arc<Mutex<Vec<u8>>>,
}

impl Terminal {
    fn run(command: &str) -> Self {
        let master = pty::openpt(OpenptFlags::RDWR | OpenptFlags::NOCTTY).unwrap();
        pty::grantpt(&master).unwrap();
        pty::unlockpt(&master).unwrap();
        let name = pty::ptsname(&master, Vec::new()).unwrap();
        let slave = fs::open(name, OFlags::RDWR | OFlags::NOCTTY, Mode::empty()).unwrap();
        let child = Command::new(env!("CARGO_BIN_EXE_mnemolock"))
            .arg(command)
            .stdin(slave.try_clone().unwrap())
            .stdout(slave.try_clone().unwrap())
            .stderr(slave)
            .spawn()
            .unwrap();
        let master = File::from(master);
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

        Self { child, master, output }
    }

    fn send(&mut self, keys: &str) {
        self.master.write_all(keys.as_bytes()).unwrap();
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

    fn wait_for(&self, text: &str) {
        let start = Instant::now();
        while !self.shown().contains(text) {
            assert!(start.elapsed() < TIMEOUT, "{text:?} was never shown: {:?}", self.shown());
            thread::sleep(Duration::from_millis(5));
        }
    }

    fn wait_for_exit(&mut self) -> ExitStatus {
        let start = Instant::now();
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                return status;
            }
            assert!(start.elapsed() < TIMEOUT, "the program is still running: {:?}", self.shown());
            thread::sleep(Duration::from_millis(5));
        }
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
