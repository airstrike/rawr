//! The shell in the terminal: every source of input, the keys, the
//! session and the output of a run the shell started, feeds one channel,
//! and the screen draws after each input. Nothing waits on a clock.

use std::io::Read;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::mpsc;

use crossterm::event::{self, Event};

use crate::client::Client;
use crate::shell::{Flow, Input, Shell, Stream};

/// What the shell attaches to: a connection to a run's session, the file
/// it shows first, where the run's inspector keeps its breakpoints, and the
/// output of the run when this shell started it.
pub struct Session {
    stream: UnixStream,
    source: PathBuf,
    saved: PathBuf,
    output: Vec<(Stream, Box<dyn Read + Send>)>,
}

impl Session {
    /// A session reached on `stream`, showing `source` until the program
    /// pauses, with its breakpoints kept at `saved`.
    #[must_use]
    pub fn new(stream: UnixStream, source: PathBuf, saved: PathBuf) -> Self {
        Self {
            stream,
            source,
            saved,
            output: Vec::new(),
        }
    }

    /// The run's `stream`, read from `pipe`: the shell shows it and tells
    /// it to the session for the other shells.
    #[must_use]
    pub fn with_output(mut self, stream: Stream, pipe: impl Read + Send + 'static) -> Self {
        self.output.push((stream, Box::new(pipe)));
        self
    }
}

/// Run the shell on `session` in this terminal until the person quits or
/// the terminal goes away.
///
/// # Errors
///
/// When the terminal can't be drawn on, or a thread can't start.
pub fn run(session: Session) -> std::io::Result<()> {
    let (send, inputs) = mpsc::channel();
    let client = Client::new(session.stream, send.clone(), Input::Session)?;
    for (stream, pipe) in session.output {
        let send = send.clone();
        std::thread::Builder::new()
            .name("rawr-tui-output".into())
            .spawn(move || read_output(stream, pipe, &send))?;
    }
    std::thread::Builder::new()
        .name("rawr-tui-keys".into())
        .spawn(move || read_keys(&send))?;
    let mut shell = Shell::new(client, session.source, session.saved);
    let mut terminal = ratatui::init();
    let result = (|| loop {
        terminal.draw(|screen| shell.draw(screen))?;
        let Ok(input) = inputs.recv() else {
            return Ok(());
        };
        if shell.handle(input) == Flow::Quit {
            return Ok(());
        }
    })();
    ratatui::restore();
    result
}

fn read_output(stream: Stream, mut pipe: Box<dyn Read + Send>, send: &mpsc::Sender<Input>) {
    let mut buffer = [0_u8; 8192];
    while let Ok(read) = pipe.read(&mut buffer) {
        if read == 0 {
            return;
        }
        let text = String::from_utf8_lossy(&buffer[..read]).into_owned();
        if send.send(Input::Output { stream, text }).is_err() {
            return;
        }
    }
}

fn read_keys(send: &mpsc::Sender<Input>) {
    while let Ok(event) = event::read() {
        let input = match event {
            Event::Key(key) => Input::Key(key),
            Event::Resize(..) => Input::Resize,
            _ => continue,
        };
        if send.send(input).is_err() {
            return;
        }
    }
}
