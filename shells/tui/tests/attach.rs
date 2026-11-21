//! The shell on a real run's session, driven as a person drives it: it
//! breaks at a line of the program's entry file, waits for the program to
//! stop there, opens a binding one level, takes a picture of the screen,
//! clears the breakpoint, continues and quits. It needs a `rawr run` going,
//! so it is ignored unless asked for:
//!
//! - `RAWR_TUI_SOCKET`: the run's session socket;
//! - `RAWR_TUI_LINE`: the line of the program's entry file to stop at,
//!   which the program reaches by itself, as a loop does;
//! - `RAWR_TUI_BINDING`: a binding of the stop's innermost frame whose
//!   value has fields, keys or items to open.
//!
//! The picture goes to `target/screens/tui-live-dark.png`.

#![cfg(unix)]

#[path = "shared/pane.rs"]
mod pane;
#[path = "shared/picture.rs"]
mod picture;

use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::mpsc;

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::backend::TestBackend;
use ratatui::style::Modifier;
use ratatui::Terminal;
use rawr_tui::client::{Client, Message, Reply};
use rawr_tui::home;
use rawr_tui::{Flow, Input, Shell};
use serde_json::Value;

/// A typical terminal's size, in cells.
const WIDTH: u16 = 120;
const HEIGHT: u16 = 36;

/// The shell, the inputs its connection hears, and the terminal it draws
/// on.
struct Live {
    shell: Shell,
    inputs: mpsc::Receiver<Input>,
    terminal: Terminal<TestBackend>,
}

impl Live {
    /// The screen, one line per row.
    fn screen(&mut self) -> String {
        self.terminal
            .draw(|screen| self.shell.draw(screen))
            .unwrap();
        let buffer = self.terminal.backend().buffer();
        let mut text = String::new();
        for y in 0..buffer.area.height {
            let row: String = (0..buffer.area.width)
                .map(|x| buffer.cell((x, y)).map_or(" ", |cell| cell.symbol()))
                .collect();
            text.push_str(row.trim_end());
            text.push('\n');
        }
        text
    }

    /// Take what the session says until the screen shows what `done`
    /// looks for.
    fn until(&mut self, what: &str, done: impl Fn(&str) -> bool) {
        loop {
            let screen = self.screen();
            if done(&screen) {
                return;
            }
            let input = self.inputs.recv().expect("the connection's reader goes on");
            if matches!(input, Input::Session(Message::Closed)) {
                panic!("the session closed waiting for {what}:\n{screen}");
            }
            self.shell.handle(input);
        }
    }

    fn press(&mut self, code: KeyCode) -> Flow {
        self.shell.handle(Input::Key(KeyEvent::from(code)))
    }

    fn type_text(&mut self, text: &str) {
        for char in text.chars() {
            self.press(KeyCode::Char(char));
        }
        self.press(KeyCode::Enter);
    }

    /// The text of the selected binding row: the only row drawn reversed
    /// while the bindings pane has the focus.
    fn selected(&mut self) -> String {
        self.screen();
        let buffer = self.terminal.backend().buffer();
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .filter_map(|x| buffer.cell((x, y)))
                    .filter(|cell| cell.modifier.contains(Modifier::REVERSED))
                    .map(|cell| cell.symbol())
                    .collect::<String>()
            })
            .find(|row| !row.is_empty())
            .unwrap_or_default()
    }
}

/// The run's root and its entry file, as the session's `hello` names the
/// root: a package's directory, whose entry is `src/main.rs`, or a single
/// file.
fn entry(socket: &Path) -> (String, PathBuf) {
    let stream = UnixStream::connect(socket).expect("RAWR_TUI_SOCKET is a run's session");
    let closing = stream.try_clone().unwrap();
    let (send, heard) = mpsc::channel();
    let mut client = Client::new(stream, send, std::convert::identity).unwrap();
    let hello = client.request("hello", Value::Null);
    let root = loop {
        match heard.recv().expect("the connection's reader goes on") {
            Message::Answer {
                id,
                reply: Reply::Result(answer),
            } if id == hello => break answer["root"].as_str().unwrap_or_default().to_owned(),
            Message::Answer {
                id,
                reply: Reply::Error { message, .. },
            } if id == hello => panic!("the session refused `hello`: {message}"),
            Message::Closed => panic!("the session closed before it said hello"),
            _ => {}
        }
    };
    // Shutting the socket down ends the reader's thread, which holds a
    // handle of its own.
    let _ = closing.shutdown(Shutdown::Both);
    let path = Path::new(&root);
    let entry = if path.is_dir() {
        path.join("src").join("main.rs")
    } else {
        path.to_path_buf()
    };
    (root, entry)
}

#[test]
#[ignore = "needs a `rawr run` going: RAWR_TUI_SOCKET, RAWR_TUI_LINE and RAWR_TUI_BINDING"]
fn the_shell_attaches_to_a_run_stops_at_its_breakpoint_and_opens_a_binding() {
    let socket = PathBuf::from(std::env::var("RAWR_TUI_SOCKET").expect("RAWR_TUI_SOCKET"));
    let line: usize = std::env::var("RAWR_TUI_LINE")
        .expect("RAWR_TUI_LINE")
        .parse()
        .expect("RAWR_TUI_LINE is a line number");
    let binding = std::env::var("RAWR_TUI_BINDING").expect("RAWR_TUI_BINDING");
    let (root, entry) = entry(&socket);
    let name = entry
        .file_name()
        .expect("an entry file has a name")
        .to_string_lossy()
        .into_owned();

    let (send, inputs) = mpsc::channel();
    let stream = UnixStream::connect(&socket).unwrap();
    let client = Client::new(stream, send, Input::Session).unwrap();
    let mut live = Live {
        shell: Shell::new(client, entry),
        inputs,
        terminal: Terminal::new(TestBackend::new(WIDTH, HEIGHT)).unwrap(),
    };
    live.until("the hello", |screen| {
        screen
            .lines()
            .next()
            .is_some_and(|top| top.contains(&home::shorten(&root)))
    });

    // `B` breaks at a line of the shown file, the entry until a stop.
    live.press(KeyCode::Char('B'));
    live.type_text(&line.to_string());
    let mark = format!("● {name}:{line}");
    live.until("the breakpoint", |screen| screen.contains(&mark));

    // The binding is looked for in the bindings pane alone: the source
    // beside it names the binding too, as `players: HashMap<..>` does.
    let stopped = format!("paused at {name}:{line}");
    let opened = format!("{binding} = ");
    let unreadable = format!("{binding}: ");
    let listed = |screen: &str| {
        pane::pane(screen, "bindings").is_some_and(|rows| {
            rows.lines().any(|row| {
                let row = row.trim_start().trim_start_matches(['▸', '▾', ' ']);
                row.starts_with(&opened) || row.starts_with(&unreadable)
            })
        })
    };
    live.until("the stop and its bindings", |screen| {
        screen
            .lines()
            .next()
            .is_some_and(|top| top.contains(&stopped))
            && listed(screen)
    });
    let closed = format!("▸ {opened}");
    let screen = live.screen();
    assert!(
        pane::pane(&screen, "bindings").is_some_and(|rows| rows.contains(&closed)),
        "`{binding}` has a value to open:\n{screen}"
    );

    // The bindings pane takes the focus, its selection moves down to the
    // binding, and Enter opens it one level.
    live.press(KeyCode::Tab);
    live.press(KeyCode::Tab);
    let mut rows = 0;
    while !live.selected().trim_start().starts_with(&closed) {
        rows += 1;
        assert!(
            rows < 1000,
            "the bindings pane lists `{binding}`:\n{}",
            live.screen()
        );
        live.press(KeyCode::Down);
    }
    live.press(KeyCode::Enter);
    let open = format!("▾ {opened}");
    live.until("the open binding", |screen| screen.contains(&open));
    let shown = picture::save(live.terminal.backend().buffer(), "tui-live-dark.png");
    println!("{}\n{}", live.screen(), shown.display());

    // The breakpoint comes off at the stop's line, where the source's
    // cursor is, so the program runs on after the continue.
    live.press(KeyCode::Char('b'));
    live.until("the breakpoint to clear", |screen| !screen.contains(&mark));
    live.press(KeyCode::Char('c'));
    live.until("the program to run on", |screen| {
        screen
            .lines()
            .next()
            .is_some_and(|top| top.contains("· running"))
    });
    assert_eq!(live.press(KeyCode::Char('q')), Flow::Quit);
}
