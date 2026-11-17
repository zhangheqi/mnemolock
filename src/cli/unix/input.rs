//! Keyboard input on Unix, read straight from the terminal.
//!
//! crossterm keeps what it reads from the terminal in a buffer and two queues that it never
//! wipes, and that nothing outside it can reach: a password stayed in them, key by key, long
//! after it was used. So the terminal is read here instead, into one buffer, and each key's bytes
//! are wiped from it as soon as the key is handed on.

use std::fs::File;
use std::io::{self, ErrorKind, IsTerminal, Write};
use std::os::fd::{AsFd, AsRawFd, BorrowedFd};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};
use crossterm::event::{Event, KeyCode};
use crossterm::terminal;
use rustix::event::{self, FdSetElement, Timespec};
use zeroize::{Zeroize, Zeroizing};
use super::parse::{self, ESC, Input};

/// How long the rest of an escape sequence may take to follow its first byte. After that, the
/// byte was the Esc key.
const ESC_TIMEOUT: Duration = Duration::from_millis(50);

/// How long the terminal may take to say where the cursor is.
const CURSOR_POSITION_TIMEOUT: Duration = Duration::from_secs(2);

static READER: Mutex<Option<Reader>> = Mutex::new(None);

/// Runs `action` with the reader, which is set up the first time it's needed.
fn with_reader<T>(action: impl FnOnce(&mut Reader) -> io::Result<T>) -> io::Result<T> {
    let mut reader = READER.lock().unwrap_or_else(PoisonError::into_inner);
    let reader = match reader.as_mut() {
        Some(reader) => reader,
        None => reader.insert(Reader::new(Tty::open()?)),
    };
    action(reader)
}

/// Reads the next event, or fails once a termination signal is received.
pub fn read_event() -> io::Result<Event> {
    with_reader(Reader::read_event)
}

/// Asks the terminal where the cursor is: column and row, counted from 0. Keys typed while the
/// answer is on its way are kept for later.
pub fn cursor_position() -> io::Result<(u16, u16)> {
    with_reader(|reader| reader.cursor_position(&mut io::stdout()))
}

/// Throws away keys that were typed but not read yet.
pub fn discard_pending_input() -> io::Result<()> {
    with_reader(Reader::discard_pending_input)
}

/// Wipes the keys that were typed but not used.
pub fn wipe() {
    if let Some(reader) = READER.lock().unwrap_or_else(PoisonError::into_inner).as_mut() {
        reader.pending.clear();
        reader.searched = 0;
    }
}

/// The terminal that the keyboard is read from.
enum Tty {
    Stdin,
    Opened(File),
}

impl Tty {
    /// Standard input, or the terminal the program was started from if input was redirected.
    /// This is the terminal that crossterm puts in raw mode.
    fn open() -> io::Result<Self> {
        if io::stdin().is_terminal() {
            Ok(Tty::Stdin)
        } else {
            File::open("/dev/tty").map(Tty::Opened)
        }
    }
}

impl AsFd for Tty {
    fn as_fd(&self) -> BorrowedFd<'_> {
        match self {
            Tty::Stdin => rustix::stdio::stdin(),
            Tty::Opened(file) => file.as_fd(),
        }
    }
}

/// Bytes from the terminal that are yet to be used. They are wiped as they are used.
struct Pending {
    /// The bytes are `buf[start..end]`. Everything around them is zeros.
    buf: Zeroizing<Vec<u8>>,
    start: usize,
    end: usize,
}

impl Pending {
    /// More than a terminal sends at once, unless a lot of text is pasted.
    const INITIAL_CAPACITY: usize = 1024;

    fn new() -> Self {
        Self { buf: Zeroizing::new(vec![0; Self::INITIAL_CAPACITY]), start: 0, end: 0 }
    }

    fn bytes(&self) -> &[u8] {
        &self.buf[self.start..self.end]
    }

    /// Takes out `len` bytes that come after the first `offset` bytes, and wipes them.
    fn remove(&mut self, offset: usize, len: usize) {
        let from = self.start + offset;
        if offset == 0 {
            self.buf[from..from + len].zeroize();
            self.start += len;
        } else {
            // Close the gap, then wipe what that leaves behind at the end
            self.buf.copy_within(from + len..self.end, from);
            self.buf[self.end - len..self.end].zeroize();
            self.end -= len;
        }
        if self.start == self.end {
            (self.start, self.end) = (0, 0);
        }
    }

    fn clear(&mut self) {
        self.remove(0, self.end - self.start);
    }

    /// Room for more bytes, after those that are here.
    fn spare(&mut self) -> &mut [u8] {
        if self.end == self.buf.len() {
            let len = self.end - self.start;
            if self.start >= self.buf.len() / 2 {
                // Half or more is used up: move the rest to the front and wipe where it was
                self.buf.copy_within(self.start..self.end, 0);
                self.buf[len..self.end].zeroize();
            } else {
                let mut grown = Zeroizing::new(vec![0; 2 * self.buf.len()]);
                grown[..len].copy_from_slice(self.bytes());
                // Drops the old buffer, which wipes it
                self.buf = grown;
            }
            (self.start, self.end) = (0, len);
        }
        &mut self.buf[self.end..]
    }

    /// Takes in `len` bytes that were put in the spare room.
    fn filled(&mut self, len: usize) {
        self.end += len;
    }
}

struct Reader {
    tty: Tty,
    pending: Pending,
    /// How many of the pending bytes `cursor_position` has been through. They are whole keys,
    /// still to be read, and not what it was looking for. This saves going through them again
    /// for every key, when keys arrive faster than they are read.
    searched: usize,
    /// For `select`: the set of file descriptors to wait for, which is just the terminal
    waiting_for: Vec<FdSetElement>,
    esc_timeout: Duration,
}

impl Reader {
    fn new(tty: Tty) -> Self {
        let fd = tty.as_fd().as_raw_fd();
        Self {
            tty,
            pending: Pending::new(),
            searched: 0,
            waiting_for: vec![FdSetElement::default(); event::fd_set_num_elements(1, fd + 1)],
            esc_timeout: ESC_TIMEOUT,
        }
    }

    /// Waits until the terminal has something to read, for up to `timeout`. Fails with
    /// `Interrupted` if a signal arrives first.
    fn wait(&mut self, timeout: Duration) -> io::Result<bool> {
        let fd = self.tty.as_fd().as_raw_fd();
        let timeout = Timespec::try_from(timeout).map_err(|_| ErrorKind::InvalidInput)?;
        // `select` leaves in the set what is ready, so the terminal may no longer be in it. And
        // `select` it is, not `poll`, which doesn't work for `/dev/tty` on macOS.
        self.waiting_for.fill(FdSetElement::default());
        event::fd_set_insert(&mut self.waiting_for, fd);
        // SAFETY: the only file descriptor in the set is `fd`, which `self.tty` keeps open.
        let ready = unsafe {
            event::select(fd + 1, Some(&mut self.waiting_for), None, None, Some(&timeout))
        }?;
        Ok(ready > 0)
    }

    /// Reads what the terminal has sent, waiting up to `timeout` for there to be something.
    /// Returns whether anything was read. Fails with `Interrupted` if a signal arrives first.
    fn fill(&mut self, timeout: Duration) -> io::Result<bool> {
        if !self.wait(timeout)? {
            return Ok(false);
        }
        // Straight into the buffer that is wiped, with no copy made on the way
        match rustix::io::read(&self.tty, self.pending.spare()) {
            Ok(0) => Err(io::Error::new(ErrorKind::UnexpectedEof, "the terminal was closed")),
            Ok(len) => {
                self.pending.filled(len);
                Ok(true)
            }
            Err(error) => match io::Error::from(error) {
                // Something else got to the input first
                error if error.kind() == ErrorKind::WouldBlock => Ok(false),
                error => Err(error),
            },
        }
    }

    /// Wipes the first `len` pending bytes.
    fn consume(&mut self, len: usize) {
        self.pending.remove(0, len);
        self.searched = self.searched.saturating_sub(len);
    }

    fn read_event(&mut self) -> io::Result<Event> {
        // When a lone `ESC` stops being the possible start of an escape sequence
        let mut esc_deadline = None;
        loop {
            super::check_signal()?;
            if super::take_resize() {
                let (columns, rows) = terminal::size()?;
                return Ok(Event::Resize(columns, rows));
            }
            if let Some((len, input)) = parse::parse(self.pending.bytes()) {
                self.consume(len);
                esc_deadline = None;
                if let Input::Key(event) = input {
                    return Ok(Event::Key(event));
                }
                continue;
            }
            let mut timeout = super::CHECK_INTERVAL;
            if self.pending.bytes() == [ESC] {
                let deadline =
                    *esc_deadline.get_or_insert_with(|| Instant::now() + self.esc_timeout);
                let left = deadline.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    self.consume(1);
                    return Ok(Event::Key(KeyCode::Esc.into()));
                }
                timeout = timeout.min(left);
            }
            match self.fill(timeout) {
                Ok(true) => esc_deadline = None,
                Ok(false) => (),
                Err(error) if error.kind() == ErrorKind::Interrupted => (),
                Err(error) => return Err(error),
            }
        }
    }

    /// Asks the terminal, by writing to `output`, where the cursor is. Its answer comes in with
    /// the keys, after any that were typed first. Those stay where they are, to be read later.
    fn cursor_position(&mut self, output: &mut impl Write) -> io::Result<(u16, u16)> {
        output.write_all(b"\x1b[6n")?;
        output.flush()?;
        let deadline = Instant::now() + CURSOR_POSITION_TIMEOUT;
        loop {
            super::check_signal()?;
            match parse::parse(&self.pending.bytes()[self.searched..]) {
                Some((len, Input::CursorPosition(column, row))) => {
                    self.pending.remove(self.searched, len);
                    return Ok((column, row));
                }
                Some((len, _)) => self.searched += len,
                None => {
                    let left = deadline.saturating_duration_since(Instant::now());
                    if left.is_zero() {
                        return Err(io::Error::new(
                            ErrorKind::TimedOut,
                            "The cursor position could not be read within a normal duration",
                        ));
                    }
                    match self.fill(left.min(super::CHECK_INTERVAL)) {
                        Err(error) if error.kind() != ErrorKind::Interrupted => return Err(error),
                        _ => (),
                    }
                }
            }
        }
    }

    fn discard_pending_input(&mut self) -> io::Result<()> {
        loop {
            self.pending.clear();
            self.searched = 0;
            match self.fill(Duration::ZERO) {
                Ok(true) => (),
                Ok(false) => return Ok(()),
                Err(error) if error.kind() == ErrorKind::Interrupted => (),
                Err(error) => return Err(error),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::thread;
    use crossterm::event::KeyEvent;
    use rustix::fs::{self, Mode, OFlags};
    use rustix::pty::{self, OpenptFlags};
    use rustix::termios::{self, OptionalActions};
    use super::*;

    const TIMEOUT: Duration = Duration::from_secs(30);

    /// A reader on a new pseudo-terminal in raw mode, and the terminal's end of it: what is
    /// written there is what the reader gets as typed.
    fn reader() -> (Reader, File) {
        let master = pty::openpt(OpenptFlags::RDWR | OpenptFlags::NOCTTY).unwrap();
        pty::grantpt(&master).unwrap();
        pty::unlockpt(&master).unwrap();
        let name = pty::ptsname(&master, Vec::new()).unwrap();
        let slave = fs::open(name, OFlags::RDWR | OFlags::NOCTTY, Mode::empty()).unwrap();
        let mut modes = termios::tcgetattr(&slave).unwrap();
        modes.make_raw();
        termios::tcsetattr(&slave, OptionalActions::Now, &modes).unwrap();
        (Reader::new(Tty::Opened(File::from(slave))), File::from(master))
    }

    impl Reader {
        fn read_key(&mut self) -> KeyEvent {
            match self.read_event().unwrap() {
                Event::Key(event) => event,
                event => panic!("not a key: {event:?}"),
            }
        }

        fn read_char(&mut self) -> char {
            match self.read_key().code {
                KeyCode::Char(ch) => ch,
                code => panic!("not a character: {code:?}"),
            }
        }

        fn read_text(&mut self, len: usize) -> String {
            (0..len).map(|_| self.read_char()).collect()
        }

        /// Takes in what the terminal has sent so far, without reading keys from it.
        fn receive(&mut self, bytes: &[u8]) {
            while !self.pending.bytes().ends_with(bytes) {
                assert!(self.fill(TIMEOUT).unwrap(), "timed out waiting for {bytes:x?}");
            }
        }

        fn is_wiped(&self) -> bool {
            self.pending.bytes().is_empty() && self.pending.buf.iter().all(|&byte| byte == 0)
        }
    }

    #[test]
    fn keys_are_wiped_once_read() {
        let (mut reader, mut terminal) = reader();
        terminal.write_all("pässwörd 🔒\r".as_bytes()).unwrap();
        assert_eq!(reader.read_text(10), "pässwörd 🔒");
        // The keys not read yet are still there, and nothing else
        reader.receive(b"\r");
        assert_eq!(reader.pending.bytes(), b"\r");
        assert_eq!(reader.pending.buf.iter().filter(|&&byte| byte != 0).count(), 1);
        assert_eq!(reader.read_key().code, KeyCode::Enter);
        assert!(reader.is_wiped());
    }

    #[test]
    fn character_split_between_reads_is_kept_whole() {
        let (mut reader, mut terminal) = reader();
        let lock = "🔒".as_bytes();
        for cut in 1..lock.len() {
            terminal.write_all(&lock[..cut]).unwrap();
            reader.receive(&lock[..cut]);
            terminal.write_all(&lock[cut..]).unwrap();
            assert_eq!(reader.read_char(), '🔒');
            assert!(reader.is_wiped());
        }
    }

    #[test]
    fn lone_esc_is_the_esc_key() {
        let (mut reader, mut terminal) = reader();
        terminal.write_all(b"\x1b").unwrap();
        reader.receive(b"\x1b");
        let start = Instant::now();
        assert_eq!(reader.read_key().code, KeyCode::Esc);
        // Not before the rest of an escape sequence had its time to arrive
        assert!(start.elapsed() >= ESC_TIMEOUT);
        assert!(reader.is_wiped());
    }

    #[test]
    fn escape_sequence_split_between_reads_is_not_the_esc_key() {
        let (mut reader, mut terminal) = reader();
        // Long enough that a slow machine can't make the rest arrive late
        reader.esc_timeout = TIMEOUT;
        for arrow in [&b"\x1b[C"[..], b"\x1bOC", b"\x1b[1;5C"] {
            for cut in 1..arrow.len() {
                terminal.write_all(&arrow[..cut]).unwrap();
                reader.receive(&arrow[..cut]);
                terminal.write_all(&arrow[cut..]).unwrap();
                assert_eq!(reader.read_key().code, KeyCode::Right);
                assert!(reader.is_wiped());
            }
        }
    }

    #[test]
    fn esc_is_the_esc_key_when_followed_by_an_escape_sequence() {
        let (mut reader, mut terminal) = reader();
        reader.esc_timeout = TIMEOUT;
        terminal.write_all(b"\x1b\x1b[C").unwrap();
        assert_eq!(reader.read_key().code, KeyCode::Esc);
        assert_eq!(reader.read_key().code, KeyCode::Right);
    }

    #[test]
    fn cursor_position_is_found_among_keys() {
        let (mut reader, mut terminal) = reader();
        // `ab` was typed before the terminal got to answer, and `c` after. The answer has
        // row 3, column 7.
        terminal.write_all(b"ab\x1b[3;7Rc").unwrap();
        let mut asked = Vec::new();
        assert_eq!(reader.cursor_position(&mut asked).unwrap(), (6, 2));
        assert_eq!(asked, b"\x1b[6n");
        // The answer is gone, the keys are not
        reader.receive(b"c");
        assert_eq!(reader.pending.bytes(), b"abc");

        // Asked again, with `ab` still unread
        terminal.write_all(b"d\x1b[4;1R").unwrap();
        assert_eq!(reader.cursor_position(&mut asked).unwrap(), (0, 3));
        assert_eq!(reader.read_text(1), "a");
        terminal.write_all(b"\x1b[5;2Re").unwrap();
        assert_eq!(reader.cursor_position(&mut asked).unwrap(), (1, 4));
        assert_eq!(reader.read_text(4), "bcde");
        assert!(reader.is_wiped());
    }

    #[test]
    fn cursor_position_is_found_after_esc() {
        let (mut reader, mut terminal) = reader();
        // Esc was pressed just before the terminal answered
        terminal.write_all(b"\x1b\x1b[3;7R").unwrap();
        assert_eq!(reader.cursor_position(&mut Vec::new()).unwrap(), (6, 2));
        assert_eq!(reader.read_key().code, KeyCode::Esc);
        assert!(reader.is_wiped());
    }

    #[test]
    fn cursor_position_is_found_after_more_keys_than_fit_in_the_buffer() {
        let (mut reader, mut terminal) = reader();
        let capacity = reader.pending.buf.len();
        // As when a lot of text is pasted: all of it is ahead of the terminal's answer. It is
        // more than a terminal holds, so it goes in as the reader takes it out.
        let pasted = "pässwörd 🔒".repeat(1000);
        let typing = thread::spawn({
            let pasted = pasted.clone();
            move || {
                terminal.write_all(pasted.as_bytes()).unwrap();
                terminal.write_all(b"\x1b[3;7R").unwrap();
                terminal
            }
        });
        assert_eq!(reader.cursor_position(&mut Vec::new()).unwrap(), (6, 2));
        assert!(reader.pending.buf.len() > capacity);
        assert_eq!(reader.pending.bytes(), pasted.as_bytes());

        // Asking again doesn't go through all of that again
        let mut terminal = typing.join().unwrap();
        assert_eq!(reader.searched, pasted.len());
        terminal.write_all(b"\x1b[4;1R").unwrap();
        assert_eq!(reader.cursor_position(&mut Vec::new()).unwrap(), (0, 3));

        assert_eq!(reader.read_text(pasted.chars().count()), pasted);
        assert!(reader.is_wiped());
    }

    #[test]
    fn discarded_input_is_wiped() {
        let (mut reader, mut terminal) = reader();
        terminal.write_all(b"ab").unwrap();
        assert_eq!(reader.read_char(), 'a');
        // One key waits in the buffer, the rest with the terminal
        terminal.write_all(b"secret\r").unwrap();
        assert!(reader.wait(TIMEOUT).unwrap());
        reader.discard_pending_input().unwrap();
        assert!(reader.is_wiped());
        assert!(!reader.wait(Duration::ZERO).unwrap());

        terminal.write_all(b"c").unwrap();
        assert_eq!(reader.read_char(), 'c');
    }

    #[test]
    fn closed_terminal_is_an_error() {
        let (mut reader, mut terminal) = reader();
        terminal.write_all(b"a").unwrap();
        assert_eq!(reader.read_char(), 'a');
        drop(terminal);
        assert!(reader.read_event().is_err());
        assert!(reader.cursor_position(&mut Vec::new()).is_err());
    }

    #[test]
    fn pending_bytes_are_wiped_when_removed() {
        let mut pending = Pending::new();
        let capacity = pending.buf.len();
        let zeros_around = |pending: &Pending| {
            let around = [&pending.buf[..pending.start], &pending.buf[pending.end..]].concat();
            around.iter().all(|&byte| byte == 0)
        };
        let add = |pending: &mut Pending, bytes: &[u8]| {
            pending.spare()[..bytes.len()].copy_from_slice(bytes);
            pending.filled(bytes.len());
        };

        add(&mut pending, b"abcdefgh");
        pending.remove(0, 2);
        assert_eq!(pending.bytes(), b"cdefgh");
        pending.remove(2, 3);
        assert_eq!(pending.bytes(), b"cdh");
        assert!(zeros_around(&pending));
        pending.remove(1, 2);
        assert_eq!(pending.bytes(), b"c");
        pending.clear();
        assert_eq!((pending.start, pending.end), (0, 0));
        assert!(zeros_around(&pending));

        // Full, with more than half used up: the rest moves to the front
        add(&mut pending, &vec![b'x'; capacity - 2]);
        add(&mut pending, b"yz");
        pending.remove(0, capacity - 2);
        assert_eq!(pending.spare().len(), capacity - 2);
        assert_eq!((pending.bytes(), pending.buf.len()), (&b"yz"[..], capacity));
        assert!(zeros_around(&pending));

        // Full, with less than half used up: the buffer grows
        add(&mut pending, &vec![b'x'; capacity - 2]);
        pending.remove(0, 1);
        assert_eq!(pending.spare().len(), capacity + 1);
        assert_eq!(pending.bytes(), [&b"z"[..], &vec![b'x'; capacity - 2]].concat());
        assert!(zeros_around(&pending));
    }
}
