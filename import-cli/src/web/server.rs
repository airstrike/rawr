//! The page's server, on this machine's loopback address. It serves the
//! page's files and the program's package, streams what the page must do
//! (take a save's files, or load a new runner) as server-sent events, and
//! hears what the page reports: what each reload did and what the program
//! wrote. A connection is served on a thread of its own.

use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use serde_json::Value;

use super::{build, Error};

/// The port a page is served on when none is asked for and it is free.
const PORT: u16 = 7878;

/// The most a report from the page may hold, in bytes.
const MAX_BODY: usize = 1 << 20;

/// What the server tells the page.
pub(super) enum Event {
    /// The package's files after a save, which the running program takes.
    Files(Value),
    /// A new runner, which the page loads and runs the program on.
    Reload,
}

/// What the page told the server.
pub(super) enum Report {
    /// What starting the program, or a reload, did: the runner's answer.
    Answer(Value),
    /// What the program wrote.
    Output { stderr: bool, text: String },
}

/// The page's server, listening.
pub(super) struct Server {
    listener: TcpListener,
    shared: Arc<Shared>,
}

/// What the server's connections share.
struct Shared {
    site: PathBuf,
    /// The program as `program.json` gives it.
    program: Mutex<Value>,
    /// The pages listening for events.
    listeners: Mutex<Vec<TcpStream>>,
    reports: mpsc::Sender<Report>,
}

/// What tells the page what to do while the server serves it.
#[derive(Clone)]
pub(super) struct Handle {
    shared: Arc<Shared>,
}

impl Server {
    /// Listen on `port`, or on [`PORT`] when it is free and on any free
    /// port when it isn't, to serve the files in `site` and `program`. The
    /// page's reports go to `reports`.
    pub(super) fn bind(
        port: Option<u16>,
        site: PathBuf,
        program: Value,
        reports: mpsc::Sender<Report>,
    ) -> Result<Self, Error> {
        let address = |port| SocketAddr::from((Ipv4Addr::LOCALHOST, port));
        let listener = match port {
            Some(port) => TcpListener::bind(address(port)),
            None => TcpListener::bind(address(PORT)).or_else(|_| TcpListener::bind(address(0))),
        }
        .map_err(|source| Error::Serve {
            address: address(port.unwrap_or(PORT)).to_string(),
            source,
        })?;
        Ok(Self {
            listener,
            shared: Arc::new(Shared {
                site,
                program: Mutex::new(program),
                listeners: Mutex::new(Vec::new()),
                reports,
            }),
        })
    }

    /// The page's address.
    pub(super) fn url(&self) -> String {
        match self.listener.local_addr() {
            Ok(address) => format!("http://{address}/"),
            Err(_) => "the page's address can't be read".to_owned(),
        }
    }

    /// What tells the page what to do.
    pub(super) fn handle(&self) -> Handle {
        Handle {
            shared: Arc::clone(&self.shared),
        }
    }

    /// Serve connections on threads of their own, for as long as the
    /// process runs.
    pub(super) fn spawn(self) {
        std::thread::spawn(move || {
            for stream in self.listener.incoming().flatten() {
                let shared = Arc::clone(&self.shared);
                std::thread::spawn(move || shared.serve(stream));
            }
        });
    }
}

impl Handle {
    /// The program `program.json` gives from now on.
    pub(super) fn set_program(&self, program: Value) {
        *lock(&self.shared.program) = program;
    }

    /// Tell every page listening `event`.
    pub(super) fn send(&self, event: &Event) {
        let message = match event {
            Event::Files(files) => format!("event: files\ndata: {files}\n\n"),
            Event::Reload => "event: reload\ndata: {}\n\n".to_owned(),
        };
        lock(&self.shared.listeners)
            .retain_mut(|listener| listener.write_all(message.as_bytes()).is_ok());
    }
}

impl Shared {
    /// Serve one connection's request.
    fn serve(&self, stream: TcpStream) {
        let Ok(reading) = stream.try_clone() else {
            return;
        };
        let mut reader = BufReader::new(reading);
        let Some(request) = Request::read(&mut reader) else {
            return;
        };
        let mut stream = stream;
        let _ = match (request.method.as_str(), request.path.as_str()) {
            ("GET", "/events") => {
                let opened = stream.write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\
                      Cache-Control: no-store\r\n\r\n",
                );
                if opened.is_ok() {
                    lock(&self.listeners).push(stream);
                }
                return;
            }
            ("GET", "/program.json") => {
                let program = lock(&self.program).to_string();
                respond(
                    &mut stream,
                    "200 OK",
                    "application/json",
                    program.as_bytes(),
                )
            }
            ("GET", path) => self.file(&mut stream, path),
            ("POST", "/answer" | "/output") => {
                let report = request.body(&mut reader).and_then(|body| {
                    let value: Value = serde_json::from_slice(&body).ok()?;
                    Some(if request.path == "/answer" {
                        Report::Answer(value)
                    } else {
                        Report::Output {
                            stderr: value["stream"] == "stderr",
                            text: value["text"].as_str()?.to_owned(),
                        }
                    })
                });
                match report {
                    Some(report) => {
                        let _ = self.reports.send(report);
                        respond(&mut stream, "204 No Content", "text/plain", b"")
                    }
                    None => respond(&mut stream, "400 Bad Request", "text/plain", b""),
                }
            }
            _ => respond(&mut stream, "405 Method Not Allowed", "text/plain", b""),
        };
    }

    /// Answer a GET of the page's file at `path`.
    fn file(&self, stream: &mut TcpStream, path: &str) -> std::io::Result<()> {
        let name = match path.split('?').next().unwrap_or_default() {
            "/" => build::INDEX,
            "/universe.js" => build::GLUE,
            "/universe_bg.wasm" => build::MODULE,
            _ => return respond(stream, "404 Not Found", "text/plain", b"not found"),
        };
        let kind = match name.rsplit('.').next() {
            Some("js") => "text/javascript",
            Some("wasm") => "application/wasm",
            _ => "text/html; charset=utf-8",
        };
        match std::fs::read(self.site.join(name)) {
            Ok(bytes) => respond(stream, "200 OK", kind, &bytes),
            Err(_) => respond(stream, "404 Not Found", "text/plain", b"not built"),
        }
    }
}

/// An HTTP request's line and the length of its body.
struct Request {
    method: String,
    path: String,
    length: usize,
}

impl Request {
    /// Read a request's line and headers.
    fn read(reader: &mut BufReader<TcpStream>) -> Option<Self> {
        let mut line = String::new();
        reader.read_line(&mut line).ok()?;
        let mut words = line.split_whitespace();
        let method = words.next()?.to_owned();
        let path = words.next()?.to_owned();
        let mut length = 0;
        loop {
            let mut header = String::new();
            if reader.read_line(&mut header).ok()? == 0 || header.trim().is_empty() {
                break;
            }
            if let Some((name, value)) = header.split_once(':')
                && name.eq_ignore_ascii_case("content-length")
            {
                length = value.trim().parse().ok()?;
            }
        }
        Some(Self {
            method,
            path,
            length,
        })
    }

    /// Read the request's body.
    fn body(&self, reader: &mut BufReader<TcpStream>) -> Option<Vec<u8>> {
        if self.length > MAX_BODY {
            return None;
        }
        let mut body = vec![0; self.length];
        reader.read_exact(&mut body).ok()?;
        Some(body)
    }
}

/// Write a whole response.
fn respond(stream: &mut TcpStream, status: &str, kind: &str, body: &[u8]) -> std::io::Result<()> {
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\n\
         Cache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(body)
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
