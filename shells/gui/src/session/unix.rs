//! The session's local socket as a [`Transport`]: a thread writes the
//! shell's lines in order and another reads the session's, so the window
//! never waits on the socket.

use std::io::{BufRead as _, BufReader, Write as _};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex, PoisonError};

use iced::futures::channel::mpsc as stream;
use iced::futures::{SinkExt as _, Stream, StreamExt as _};

use super::transport::{Event, Link, Transport};

/// The writing half of a socket connection.
struct Socket {
    lines: Mutex<mpsc::Sender<String>>,
}

impl Transport for Socket {
    fn send(&self, line: &str) {
        let _ = self
            .lines
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .send(line.to_owned());
    }
}

/// Connect to the session listening on `socket`: [`Event::Connected`]
/// first, then each line it sends, then [`Event::Closed`].
#[expect(
    clippy::ptr_arg,
    reason = "a subscription builds its stream from a reference to its data"
)]
pub fn connect(socket: &PathBuf) -> impl Stream<Item = Event> + use<> {
    let socket = socket.clone();
    iced::stream::channel(64, async move |mut output| {
        let (heard, mut lines) = stream::unbounded::<Event>();
        match open(&socket, heard) {
            Ok(link) => {
                let _ = output.send(Event::Connected(link)).await;
                while let Some(event) = lines.next().await {
                    let closed = matches!(event, Event::Closed(_));
                    let _ = output.send(event).await;
                    if closed {
                        break;
                    }
                }
            }
            Err(error) => {
                let _ = output
                    .send(Event::Closed(format!("{}: {error}", socket.display())))
                    .await;
            }
        }
        // The subscription lives as long as the window: nothing more comes.
        std::future::pending::<()>().await;
    })
}

/// Open the connection, with the threads that carry it.
fn open(socket: &Path, heard: stream::UnboundedSender<Event>) -> std::io::Result<Link> {
    let stream = UnixStream::connect(socket)?;
    let reading = stream.try_clone()?;
    let mut writing = stream;
    let (lines, written) = mpsc::channel::<String>();
    std::thread::Builder::new()
        .name("rawr-gui-write".into())
        .spawn(move || {
            for line in written {
                if writing.write_all(line.as_bytes()).is_err() {
                    return;
                }
            }
        })?;
    std::thread::Builder::new()
        .name("rawr-gui-read".into())
        .spawn(move || {
            for line in BufReader::new(reading).lines() {
                let Ok(line) = line else {
                    break;
                };
                if heard.unbounded_send(Event::Line(line)).is_err() {
                    return;
                }
            }
            let _ = heard.unbounded_send(Event::Closed("the session closed".to_owned()));
        })?;
    Ok(Link::new(Arc::new(Socket {
        lines: Mutex::new(lines),
    })))
}
