//! What the program tells the session of its output, in the process that
//! runs it, when no shell started the run: the program writes to the
//! terminal itself, so no shell holds its output (#2288).
//!
//! The process connects to the session socket [`SOCKET`] names and says
//! `tell`. Until a shell says `hello`, each piece the program
//! writes is kept in memory, the newest 256 KiB of it as the session keeps
//! for shells, and nothing more is done. At the first `hello` the session
//! says `listen`: the process tells the pieces it kept as `output`
//! notifications, says `told`, and from then on tells each piece as the
//! program writes it, after it reaches the terminal. A program that ends
//! before any shell listened tells what it kept as it ends.
//!
//! A process whose owner reads some of its streams off pipes tells only
//! the others ([`Streams::Terminals`]): the owner tells what it copies
//! from the pipes itself.

use std::collections::VecDeque;
use std::io::{BufRead as _, BufReader, IsTerminal as _, Write as _};
use std::os::unix::net::UnixStream;
use std::sync::{Mutex, OnceLock, PoisonError};

use serde_json::{json, Value};

/// The variable naming the session socket a program tells what it prints
/// on, as it is written to the terminal. The session sets it in the
/// environment of a program no shell started.
pub const SOCKET: &str = "RAWR_SESSION_OUTPUT";

/// The output kept for the session while no shell listens, oldest first
/// out: as much as the session keeps for shells that attach later.
pub const OUTPUT: usize = 256 * 1024;

/// The connection this process tells its output on, once [`open`] made it.
static TELLER: OnceLock<Teller> = OnceLock::new();

/// Which of the process's streams it tells its session.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Streams {
    /// Both: nothing else hears what the program writes.
    Both,
    /// Those that are terminals: the process's owner reads the others off
    /// pipes and tells what it copies itself.
    Terminals,
}

struct Teller {
    session: UnixStream,
    told: Told,
    /// What the program wrote while no shell listened; `None` once the
    /// session asked for it.
    kept: Mutex<Option<Kept>>,
}

/// The newest pieces of output, at most [`OUTPUT`] bytes of text.
#[derive(Default)]
struct Kept {
    pieces: VecDeque<Piece>,
    bytes: usize,
}

/// The streams a [`Teller`] tells, as [`Streams`] picked them when it
/// opened.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Told {
    stdout: bool,
    stderr: bool,
}

/// Text the program wrote to `stream`, `stdout` or `stderr`.
struct Piece {
    stream: &'static str,
    text: String,
}

/// Connect to the session [`SOCKET`] names, when it names one, and keep
/// the program's output on `streams` for it from now on. A run whose
/// session can't be reached, or that has none of `streams` to tell, runs
/// as it would without one.
pub fn open(streams: Streams) {
    let told = Told::of(
        streams,
        std::io::stdout().is_terminal(),
        std::io::stderr().is_terminal(),
    );
    if told.is_none() {
        return;
    }
    let Some(socket) = std::env::var_os(SOCKET).filter(|path| !path.is_empty()) else {
        return;
    };
    let Ok(session) = UnixStream::connect(socket) else {
        return;
    };
    let Ok(reading) = session.try_clone() else {
        return;
    };
    let teller = Teller {
        session,
        told,
        kept: Mutex::new(Some(Kept::default())),
    };
    if teller
        .send(&json!({ "jsonrpc": "2.0", "method": "tell" }))
        .is_err()
        || TELLER.set(teller).is_err()
    {
        return;
    }
    let _ = std::thread::Builder::new()
        .name("rawr-session-tell".into())
        .spawn(move || {
            for line in BufReader::new(reading).lines() {
                let Ok(line) = line else {
                    return;
                };
                let asked = serde_json::from_str::<Value>(&line)
                    .is_ok_and(|message| message["method"] == "listen");
                if asked && let Some(teller) = TELLER.get() {
                    teller.listen();
                }
            }
        });
}

/// Whether [`open`] connected this process to its session.
#[must_use]
pub fn is_open() -> bool {
    TELLER.get().is_some()
}

/// Write `text` to `stream`, as a program's `print!` or `eprint!` does,
/// and tell it to the session: an engine's output in a process that tells
/// its session.
pub fn write(stream: crate::Output, text: &str) {
    match stream {
        crate::Output::Stderr => eprint!("{text}"),
        _ => print!("{text}"),
    }
    tell(stream, text);
}

/// Tell `text`, which the program wrote to `stream` and the terminal has
/// already, to the session: kept until a shell listens, then told at once.
pub fn tell(stream: crate::Output, text: &str) {
    let Some(teller) = TELLER.get().filter(|teller| teller.told.tells(stream)) else {
        return;
    };
    let stream = match stream {
        crate::Output::Stderr => "stderr",
        _ => "stdout",
    };
    let mut kept = teller.kept();
    match kept.as_mut() {
        Some(kept) => kept.push(Piece {
            stream,
            text: text.to_owned(),
        }),
        None => {
            let _ = teller.send(&output(stream, text));
        }
    }
}

/// The program ended: tell what it kept, which the session keeps for the
/// shells that attach later.
pub fn finish() {
    if let Some(teller) = TELLER.get() {
        let kept = teller.kept().take();
        for piece in kept.into_iter().flat_map(|kept| kept.pieces) {
            let _ = teller.send(&output(piece.stream, &piece.text));
        }
    }
}

impl Teller {
    fn kept(&self) -> std::sync::MutexGuard<'_, Option<Kept>> {
        self.kept.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The session asked for the output: tell what was kept and `told`,
    /// and each piece from now on. The lock holds back the program's
    /// writes meanwhile, so the pieces reach the session in order.
    fn listen(&self) {
        let mut kept = self.kept();
        for piece in kept.take().into_iter().flat_map(|kept| kept.pieces) {
            let _ = self.send(&output(piece.stream, &piece.text));
        }
        let _ = self.send(&json!({ "jsonrpc": "2.0", "method": "told" }));
    }

    fn send(&self, message: &Value) -> std::io::Result<()> {
        writeln!(&self.session, "{message}")
    }
}

impl Told {
    /// The streams `streams` picks, given which of stdout and stderr are
    /// terminals.
    fn of(streams: Streams, stdout: bool, stderr: bool) -> Self {
        match streams {
            Streams::Both => Self {
                stdout: true,
                stderr: true,
            },
            Streams::Terminals => Self { stdout, stderr },
        }
    }

    fn is_none(self) -> bool {
        !self.stdout && !self.stderr
    }

    fn tells(self, stream: crate::Output) -> bool {
        match stream {
            crate::Output::Stderr => self.stderr,
            _ => self.stdout,
        }
    }
}

impl Kept {
    fn push(&mut self, piece: Piece) {
        self.bytes += piece.text.len();
        self.pieces.push_back(piece);
        while self.bytes > OUTPUT {
            let Some(dropped) = self.pieces.pop_front() else {
                break;
            };
            self.bytes -= dropped.text.len();
        }
    }
}

/// `text` written to `stream`, as the session hears output.
fn output(stream: &str, text: &str) -> Value {
    json!({ "jsonrpc": "2.0", "method": "output", "params": { "stream": stream, "text": text } })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_process_whose_owner_reads_a_pipe_tells_only_its_terminals() {
        let told = Told::of(Streams::Terminals, true, false);
        assert!(told.tells(crate::Output::Stdout));
        assert!(!told.tells(crate::Output::Stderr));
        assert!(Told::of(Streams::Terminals, false, false).is_none());
    }

    #[test]
    fn a_process_nothing_else_hears_tells_both_streams() {
        let told = Told::of(Streams::Both, false, false);
        assert!(told.tells(crate::Output::Stdout));
        assert!(told.tells(crate::Output::Stderr));
    }

    #[test]
    fn what_is_kept_is_the_newest_output_within_its_bound() {
        let mut kept = Kept::default();
        for text in ["old".to_owned(), "x".repeat(OUTPUT - 1), "new".to_owned()] {
            kept.push(Piece {
                stream: "stdout",
                text,
            });
        }
        let texts: Vec<&str> = kept
            .pieces
            .iter()
            .map(|piece| piece.text.as_str())
            .collect();
        assert_eq!(texts, ["new"]);
        assert_eq!(kept.bytes, 3);
    }
}
