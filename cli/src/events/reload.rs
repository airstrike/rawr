//! The stream a program under `rawr run` reads its reload outcomes from
//! (#1166).
//!
//! The owner listens on a Unix socket and names it in [`VARIABLE`] in the
//! program's environment. Each connection hears every event the owner
//! emits from then on, one JSON object per line, the same lines a
//! supervising `rawr mcp` reads. A restarted program's connections first
//! hear `restarted` with the reason. A connection is written to from a
//! thread of its own, so a program that connects late, never reads or
//! never connects holds up nothing. What a connection writes is emitted
//! in turn: the runner says `swapped` this way when the program's thread
//! admits a generation.

use std::io::{BufRead as _, BufReader, Write as _};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex, OnceLock, PoisonError};

use serde_json::{json, Value};

/// The socket a program under `rawr run` reads its reload outcomes from.
pub const VARIABLE: &str = "RAWR_RELOAD_EVENTS";

/// This process's stream, once [`Stream::open`] made it.
static SHARED: OnceLock<Shared> = OnceLock::new();

struct Shared {
    directory: PathBuf,
    socket: PathBuf,
    open: AtomicBool,
    listeners: Mutex<Vec<mpsc::Sender<Arc<str>>>>,
    /// What each new connection hears first: `restarted`, while the
    /// program is one a save restarted.
    greeting: Mutex<Option<Arc<str>>>,
}

/// The owner's reload stream, removed from the file system on drop.
pub struct Stream(&'static Shared);

impl Stream {
    /// Listen on a socket in a new private directory under `/tmp`. A
    /// process has one stream.
    ///
    /// # Errors
    ///
    /// When the directory or the socket can't be made, or a stream is
    /// already open in this process.
    pub fn open() -> std::io::Result<Self> {
        if SHARED.get().is_some() {
            return Err(std::io::Error::from(std::io::ErrorKind::AlreadyExists));
        }
        let directory = directory()?;
        let socket = directory.join("reload.sock");
        let listener = UnixListener::bind(&socket).inspect_err(|_| {
            let _ = std::fs::remove_dir(&directory);
        })?;
        let shared = Shared {
            directory,
            socket,
            open: AtomicBool::new(true),
            listeners: Mutex::new(Vec::new()),
            greeting: Mutex::new(None),
        };
        if SHARED.set(shared).is_err() {
            return Err(std::io::Error::from(std::io::ErrorKind::AlreadyExists));
        }
        let shared = SHARED.get().expect("the stream was just set");
        std::thread::Builder::new()
            .name("rawr-reload-accept".into())
            .spawn(move || accept(&listener, shared))?;
        Ok(Self(shared))
    }

    /// The socket's path, for the program's environment.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.0.socket
    }

    /// The program starts again because a save restarted it, for
    /// `reason`: its connections hear `restarted` first. `None` for a
    /// program that starts again after it ended.
    pub fn starting(&self, reason: Option<&str>) {
        let greeting = reason.map(|reason| {
            let mut line = json!({ "event": "restarted", "reason": reason }).to_string();
            line.push('\n');
            Arc::from(line)
        });
        *self
            .0
            .greeting
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = greeting;
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        self.0.open.store(false, Ordering::Release);
        let _ = std::fs::remove_file(&self.0.socket);
        let _ = std::fs::remove_dir(&self.0.directory);
    }
}

/// The open stream's socket, for a command a program runs from.
#[must_use]
pub fn path() -> Option<&'static Path> {
    SHARED
        .get()
        .filter(|shared| shared.open.load(Ordering::Acquire))
        .map(|shared| shared.socket.as_path())
}

/// Send `line`, which ends in a newline, to every connection.
pub(super) fn broadcast(line: &str) {
    let Some(shared) = SHARED.get() else {
        return;
    };
    let line: Arc<str> = Arc::from(line);
    shared
        .listeners
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .retain(|listener| listener.send(Arc::clone(&line)).is_ok());
}

fn accept(listener: &UnixListener, shared: &'static Shared) {
    for connection in listener.incoming() {
        let Ok(connection) = connection else {
            continue;
        };
        let Ok(reading) = connection.try_clone() else {
            continue;
        };
        let (send, receive) = mpsc::channel();
        let greeting = shared
            .greeting
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        if let Some(greeting) = greeting {
            let _ = send.send(greeting);
        }
        shared
            .listeners
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(send);
        let _ = std::thread::Builder::new()
            .name("rawr-reload-write".into())
            .spawn(move || write(connection, &receive));
        let _ = std::thread::Builder::new()
            .name("rawr-reload-read".into())
            .spawn(move || read(reading));
    }
}

/// Write each line to the connection until it goes away.
fn write(mut connection: UnixStream, lines: &mpsc::Receiver<Arc<str>>) {
    for line in lines {
        if connection.write_all(line.as_bytes()).is_err() {
            return;
        }
    }
}

/// Emit each event the connection writes, as the owner's own.
fn read(connection: UnixStream) {
    for line in BufReader::new(connection).lines() {
        let Ok(line) = line else {
            return;
        };
        if let Ok(event @ Value::Object(_)) = serde_json::from_str::<Value>(&line) {
            super::emit(&event);
        }
    }
}

/// A new directory only this user can enter.
fn directory() -> std::io::Result<PathBuf> {
    use std::os::unix::fs::DirBuilderExt as _;
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let base = std::fs::canonicalize("/tmp")?;
    loop {
        let directory = base.join(format!(
            ".rawr-e-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        match std::fs::DirBuilder::new().mode(0o700).create(&directory) {
            Ok(()) => return Ok(directory),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead as _, BufReader, Write as _};
    use std::os::unix::net::UnixStream;

    use serde_json::json;

    #[test]
    fn a_connection_hears_the_greeting_then_each_event_and_what_another_writes() {
        let stream = super::Stream::open().unwrap();
        assert_eq!(super::path(), Some(stream.path()));
        stream.starting(Some("`Scale` changed its fields"));
        let mut reader = BufReader::new(UnixStream::connect(stream.path()).unwrap());
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        assert_eq!(
            line,
            "{\"event\":\"restarted\",\"reason\":\"`Scale` changed its fields\"}\n"
        );

        super::super::emit(&json!({ "event": "queued" }));
        line.clear();
        reader.read_line(&mut line).unwrap();
        assert_eq!(line, "{\"event\":\"queued\"}\n");

        let mut runner = UnixStream::connect(stream.path()).unwrap();
        runner.write_all(b"{\"event\":\"swapped\"}\n").unwrap();
        drop(runner);
        line.clear();
        reader.read_line(&mut line).unwrap();
        assert_eq!(line, "{\"event\":\"swapped\"}\n");

        let socket = stream.path().to_path_buf();
        drop(stream);
        assert!(!socket.exists());
        assert_eq!(super::path(), None);
    }
}
