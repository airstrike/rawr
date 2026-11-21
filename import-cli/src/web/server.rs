//! The page's server, on this machine's loopback address. It serves the
//! page's files and the program's package, streams what the page must do
//! (take a save's files, or load a new runner) as server-sent events, and
//! hears what the page reports: what each reload did and what the program
//! wrote. A connection is served on a thread of its own. One thread owns
//! the program `program.json` gives and the pages listening for events:
//! the session, the builder and the connections send it what changes and
//! ask it for the program, in turn.

use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::Arc;

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
    /// The runner a save asked for didn't build: the program runs its
    /// last code, and the terminal says why.
    Unbuilt,
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
    site: Arc<Path>,
    /// The program as `program.json` gives it, until the owner starts.
    program: Value,
    reports: mpsc::Sender<Report>,
    inbox: mpsc::Sender<Message>,
    heard: mpsc::Receiver<Message>,
}

/// What the owner of the program and the listening pages hears.
enum Message {
    /// The program `program.json` gives from now on.
    Program(Value),
    /// An event, as the stream frames it, for every page listening.
    Event(String),
    /// A page's `/events` stream, its response's head written.
    Listen(TcpStream),
    /// A connection's ask for `program.json`'s text.
    Ask(mpsc::Sender<String>),
}

/// One connection's view of the server.
struct Connection {
    site: Arc<Path>,
    reports: mpsc::Sender<Report>,
    inbox: mpsc::Sender<Message>,
}

/// What tells the page what to do while the server serves it.
#[derive(Clone)]
pub(super) struct Handle {
    inbox: mpsc::Sender<Message>,
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
        let (inbox, heard) = mpsc::channel();
        Ok(Self {
            listener,
            site: Arc::from(site),
            program,
            reports,
            inbox,
            heard,
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
            inbox: self.inbox.clone(),
        }
    }

    /// Start the owner, and serve connections on threads of their own, for
    /// as long as the process runs.
    pub(super) fn spawn(self) {
        let Self {
            listener,
            site,
            program,
            reports,
            inbox,
            heard,
        } = self;
        std::thread::spawn(move || own(program, &heard));
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let connection = Connection {
                    site: Arc::clone(&site),
                    reports: reports.clone(),
                    inbox: inbox.clone(),
                };
                std::thread::spawn(move || connection.serve(stream));
            }
        });
    }
}

/// Own the program and the pages listening: take each change, write each
/// event to every page still listening, and answer each ask for the
/// program.
fn own(mut program: Value, heard: &mpsc::Receiver<Message>) {
    let mut listeners: Vec<TcpStream> = Vec::new();
    for message in heard {
        match message {
            Message::Program(next) => program = next,
            Message::Event(event) => {
                listeners.retain_mut(|listener| listener.write_all(event.as_bytes()).is_ok());
            }
            Message::Listen(stream) => listeners.push(stream),
            Message::Ask(reply) => {
                let _ = reply.send(program.to_string());
            }
        }
    }
}

impl Handle {
    /// The program `program.json` gives from now on.
    pub(super) fn set_program(&self, program: Value) {
        let _ = self.inbox.send(Message::Program(program));
    }

    /// Tell every page listening `event`.
    pub(super) fn send(&self, event: &Event) {
        let message = match event {
            Event::Files(files) => format!("event: files\ndata: {files}\n\n"),
            Event::Reload => "event: reload\ndata: {}\n\n".to_owned(),
            Event::Unbuilt => "event: unbuilt\ndata: {}\n\n".to_owned(),
        };
        let _ = self.inbox.send(Message::Event(message));
    }
}

impl Connection {
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
                    let _ = self.inbox.send(Message::Listen(stream));
                }
                return;
            }
            ("GET", "/program.json") => {
                let (reply, answer) = mpsc::channel();
                let _ = self.inbox.send(Message::Ask(reply));
                let program = answer.recv().unwrap_or_default();
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
