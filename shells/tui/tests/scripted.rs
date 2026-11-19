//! The shell against a scripted session: the test plays the session's
//! side of the socket, answering each request as a run's session would,
//! and reads what the shell draws on ratatui's test backend.

#![cfg(unix)]

use std::io::{BufRead as _, BufReader, Write as _};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::mpsc;

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::backend::TestBackend;
use ratatui::style::{Color, Modifier};
use ratatui::Terminal;
use rawr_tui::client::Client;
use rawr_tui::{Input, Shell};
use serde_json::{json, Value};

const SOURCE: &str = "\
struct App {
    count: u32,
    items: Vec<u32>,
}

fn update(app: &mut App) {
    app.count += 1;
    app.items.push(app.count);
}

fn main() {
    let mut app = App { count: 0, items: Vec::new() };
    update(&mut app);
}
";

/// The session's end of the socket.
struct Session {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
}

impl Session {
    /// The next request the shell sent, past its notifications.
    fn request(&mut self) -> Value {
        loop {
            let mut line = String::new();
            self.reader.read_line(&mut line).unwrap();
            let message: Value = serde_json::from_str(&line).unwrap();
            if message.get("id").is_some() {
                return message;
            }
        }
    }

    fn answer(&mut self, request: &Value, result: Value) {
        let answer = json!({ "jsonrpc": "2.0", "id": request["id"], "result": result });
        writeln!(self.writer, "{answer}").unwrap();
    }

    fn event(&mut self, event: Value) {
        let told = json!({ "jsonrpc": "2.0", "method": "event", "params": event });
        writeln!(self.writer, "{told}").unwrap();
    }
}

/// A shell on a scripted session, with the inputs it hears.
struct Rig {
    shell: Shell,
    inputs: mpsc::Receiver<Input>,
    session: Session,
    terminal: Terminal<TestBackend>,
    file: PathBuf,
}

impl Rig {
    fn new(name: &str) -> Self {
        let directory = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
        std::fs::create_dir_all(&directory).unwrap();
        let file = directory.join("main.rs");
        std::fs::write(&file, SOURCE).unwrap();
        let (ours, theirs) = UnixStream::pair().unwrap();
        let (send, inputs) = mpsc::channel();
        let client = Client::new(ours, send, Input::Session).unwrap();
        let shell = Shell::new(client, file.clone());
        let session = Session {
            reader: BufReader::new(theirs.try_clone().unwrap()),
            writer: theirs,
        };
        Self {
            shell,
            inputs,
            session,
            terminal: Terminal::new(TestBackend::new(110, 30)).unwrap(),
            file,
        }
    }

    /// Let the shell take the next `count` messages the session sent.
    fn hear(&mut self, count: usize) {
        for _ in 0..count {
            let input = self.inputs.recv().unwrap();
            self.shell.handle(input);
        }
    }

    fn press(&mut self, code: KeyCode) {
        self.shell.handle(Input::Key(KeyEvent::from(code)));
    }

    fn type_text(&mut self, text: &str) {
        for char in text.chars() {
            self.press(KeyCode::Char(char));
        }
        self.press(KeyCode::Enter);
    }

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

    fn file(&self) -> String {
        self.file.to_string_lossy().into_owned()
    }

    /// The paused program's answer to a read of frame 0, with `count`.
    fn frame(&self, line: u64, count: u32) -> Value {
        json!({
            "reached": true,
            "frame": 0,
            "file": self.file(),
            "line": line,
            "function": "app::update",
            "bindings": [
                { "name": "app", "value": format!("App {{ count: {count}, items: [1, 2] }}") },
                { "name": "secret", "error": "`Secret` has no `Debug`" },
            ],
            "unread": null,
            "frames": [
                { "function": "app::update", "file": self.file(), "line": line },
                { "function": "app::main", "file": self.file(), "line": 13 },
            ],
        })
    }

    /// The breakpoints at `lines` of the file, as the session tells them
    /// at `revision`.
    fn breakpoints(&self, revision: u64, lines: &[u64]) -> Value {
        let specs: Vec<Value> = lines
            .iter()
            .map(|line| json!({ "file": self.file(), "line": line, "text": "" }))
            .collect();
        json!({ "event": "breakpoints", "revision": revision, "breakpoints": specs })
    }

    /// Attach: the session says the program runs and gives its output.
    fn attach(&mut self) {
        self.attach_with(&[]);
    }

    /// Attach to a session whose breakpoints are at `lines`.
    fn attach_with(&mut self, lines: &[u64]) {
        let breakpoints = self.breakpoints(1, lines);
        let hello = self.session.request();
        assert_eq!(hello["method"], "hello", "{hello}");
        self.session.answer(
            &hello,
            json!({
                "protocol": 1,
                "root": "/p/app",
                "capabilities": ["events", "inspect", "eval", "break", "step", "continue"],
                "events": [{ "event": "running", "seq": 1 }],
                "output": [{ "stream": "stdout", "text": "started\n" }],
                "breakpoints": breakpoints,
            }),
        );
        self.hear(1);
    }

    /// Break at line 7 through the prompt, as the inspector answers it.
    fn break_at_seven(&mut self) {
        self.press(KeyCode::Char('B'));
        self.type_text("7");
        let request = self.session.request();
        assert_eq!(
            request["params"],
            json!({ "op": "break", "file": self.file(), "line": 7 })
        );
        let spec = json!({ "file": self.file(), "line": 7, "text": "app.count += 1;" });
        self.session.answer(
            &request,
            json!({ "set": spec, "lines": [spec], "compiled": true, "paused": null }),
        );
        let told = self.breakpoints(2, &[7]);
        self.session.event(told);
        self.hear(2);
    }

    /// The program pauses at `line`, as another shell's click or a step
    /// makes it, and the shell reads its innermost frame.
    fn pause(&mut self, line: u64, reason: &str, count: u32) {
        self.session.event(json!({
            "event": "paused", "file": self.file(), "line": line,
            "function": "app::update", "reason": reason, "seq": 2,
        }));
        self.hear(1);
        let read = self.session.request();
        assert_eq!(read["params"], json!({ "frame": 0 }), "{read}");
        let frame = self.frame(line, count);
        self.session.answer(&read, frame);
        self.hear(1);
    }
}

fn line_with<'s>(screen: &'s str, needle: &str) -> &'s str {
    screen
        .lines()
        .find(|line| line.contains(needle))
        .unwrap_or_else(|| panic!("the screen shows `{needle}`:\n{screen}"))
}

/// A breakpoint another shell sets or clears marks its line as the session
/// tells it, and one set before the shell attached shows at once (#2278).
#[test]
fn the_breakpoints_every_shell_sets_mark_their_lines() {
    let mut rig = Rig::new("told");
    rig.attach_with(&[13]);
    let screen = rig.screen();
    assert!(screen.contains("● main.rs:13"), "{screen}");
    let thirteen = line_with(&screen, "update(&mut app);");
    assert!(thirteen.contains("●13"), "{thirteen}");

    let told = rig.breakpoints(2, &[7, 13]);
    rig.session.event(told);
    rig.hear(1);
    let screen = rig.screen();
    assert!(
        line_with(&screen, "app.count += 1;").contains("● 7"),
        "{screen}"
    );
    // A list older than the one shown changes nothing.
    let stale = rig.breakpoints(1, &[]);
    rig.session.event(stale);
    rig.hear(1);
    assert!(rig.screen().contains("● main.rs:7"));
    let cleared = rig.breakpoints(3, &[13]);
    rig.session.event(cleared);
    rig.hear(1);
    let screen = rig.screen();
    assert!(!screen.contains("● main.rs:7"), "{screen}");
    assert!(screen.contains("● main.rs:13"), "{screen}");
}

#[test]
fn a_stop_shows_the_source_the_frames_and_the_bindings() {
    let mut rig = Rig::new("stop");
    rig.attach();
    rig.break_at_seven();
    rig.pause(7, "breakpoint", 1);
    let screen = rig.screen();
    println!("{screen}");
    assert!(
        screen.contains("paused at main.rs:7 in `app::update` (breakpoint)"),
        "{screen}"
    );
    // The breakpoint's mark and the stop's arrow are on line 7.
    let seven = line_with(&screen, "app.count += 1;");
    assert!(seven.contains("● 7 ▶"), "{seven}");
    assert!(line_with(&screen, "#0 app::update main.rs:7").contains("> "));
    assert!(screen.contains("#1 app::main main.rs:13"), "{screen}");
    assert!(
        screen.contains("▸ app = App { count: 1, items: [1, 2] }"),
        "{screen}"
    );
    assert!(
        screen.contains("secret: `Secret` has no `Debug`"),
        "{screen}"
    );
    assert!(screen.contains("● main.rs:7"), "{screen}");
    assert!(screen.contains("started"), "{screen}");

    // The value opens into its fields.
    rig.press(KeyCode::Tab);
    rig.press(KeyCode::Tab);
    rig.press(KeyCode::Enter);
    let screen = rig.screen();
    assert!(screen.contains("▾ app = App {"), "{screen}");
    assert!(screen.contains("count = 1"), "{screen}");
    assert!(screen.contains("▸ items = [1, 2]"), "{screen}");
}

#[test]
fn a_step_moves_the_stop_and_keeps_an_open_value_open() {
    let mut rig = Rig::new("step");
    rig.attach();
    rig.break_at_seven();
    rig.pause(7, "breakpoint", 1);
    rig.press(KeyCode::Tab);
    rig.press(KeyCode::Tab);
    rig.press(KeyCode::Enter);

    rig.press(KeyCode::Char('s'));
    let step = rig.session.request();
    assert_eq!(step["params"], json!({ "op": "step", "kind": "over" }));
    rig.session.event(json!({ "event": "resumed", "seq": 3 }));
    rig.hear(1);
    let screen = rig.screen();
    assert!(screen.contains("· running"), "{screen}");
    assert!(!screen.contains("#0 app::update"), "{screen}");
    rig.pause(8, "step", 2);
    let at = json!({ "file": rig.file(), "line": 8, "function": "app::update", "reason": "step" });
    rig.session.answer(&step, json!({ "paused": at }));
    rig.hear(1);
    let screen = rig.screen();
    println!("{screen}");
    assert!(
        screen.contains("paused at main.rs:8 in `app::update` (step)"),
        "{screen}"
    );
    assert!(
        line_with(&screen, "app.items.push").contains("8 ▶"),
        "{screen}"
    );
    assert!(
        line_with(&screen, "app.count += 1;").contains("● 7   "),
        "{screen}"
    );
    assert!(screen.contains("▾ app = App {"), "{screen}");
    assert!(screen.contains("count = 2"), "{screen}");
}

#[test]
fn an_eval_shows_its_code_and_its_value_in_the_console() {
    let mut rig = Rig::new("eval");
    rig.attach();
    rig.pause(7, "breakpoint", 1);
    rig.press(KeyCode::Char('e'));
    rig.type_text("app.count * 2");
    let eval = rig.session.request();
    assert_eq!(
        eval["params"],
        json!({ "op": "eval", "code": "app.count * 2", "frame": 0 })
    );
    rig.session.answer(&eval, json!({ "value": "2" }));
    rig.hear(1);
    rig.press(KeyCode::Char('e'));
    rig.type_text("app.missing");
    let eval = rig.session.request();
    rig.session.answer(
        &eval,
        json!({ "error": "error[E0609]: no field `missing` on type `&mut App`" }),
    );
    rig.hear(1);
    let screen = rig.screen();
    println!("{screen}");
    assert!(screen.contains("> app.count * 2"), "{screen}");
    assert!(screen.contains("= 2"), "{screen}");
    assert!(screen.contains("> app.missing"), "{screen}");
    assert!(screen.contains("no field `missing`"), "{screen}");

    // A continue answered at once; the program runs on.
    rig.press(KeyCode::Char('c'));
    let resume = rig.session.request();
    assert_eq!(resume["params"], json!({ "op": "continue" }));
    rig.session.answer(&resume, json!({ "paused": null }));
    rig.session.event(json!({ "event": "resumed", "seq": 3 }));
    rig.hear(2);
    assert!(rig.screen().contains("· running"));
}

#[test]
fn a_pause_stops_the_running_program_where_its_event_says() {
    let mut rig = Rig::new("pause");
    rig.attach();
    rig.press(KeyCode::Char('p'));
    let pause = rig.session.request();
    assert_eq!(pause["params"], json!({ "op": "pause" }));
    rig.session.answer(&pause, json!({ "paused": null }));
    rig.hear(1);
    assert!(rig.screen().contains("· running"));
    rig.pause(8, "pause", 1);
    let screen = rig.screen();
    assert!(
        screen.contains("paused at main.rs:8 in `app::update` (pause)"),
        "{screen}"
    );
    assert!(
        line_with(&screen, "app.items.push").contains("8 ▶"),
        "{screen}"
    );
    // A paused program takes no second pause: the next request is the
    // continue.
    rig.press(KeyCode::Char('p'));
    rig.press(KeyCode::Char('c'));
    let next = rig.session.request();
    assert_eq!(next["params"], json!({ "op": "continue" }));
}

/// A cell's colour as `#rrggbb`, or its name for one of the terminal's own.
fn colour(color: Color) -> String {
    match color {
        Color::Rgb(red, green, blue) => format!("#{red:02x}{green:02x}{blue:02x}"),
        other => format!("{other:?}"),
    }
}

#[test]
fn the_source_draws_rust_in_colour_with_its_breakpoint_and_stop() {
    let mut rig = Rig::new("colour");
    rig.attach();
    rig.break_at_seven();
    rig.pause(7, "breakpoint", 1);
    let screen = rig.screen();
    let buffer = rig.terminal.backend().buffer().clone();
    let (row, line) = screen
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains("fn update(app: &mut App)"))
        .expect("the source shows `update`");
    let column = line[..line.find("fn update").unwrap()].chars().count();
    let at = |x: usize| {
        buffer
            .cell((u16::try_from(x).unwrap(), u16::try_from(row).unwrap()))
            .unwrap()
            .fg
    };
    // `fn` is a keyword and `update` a function's name: each in its own
    // colour, neither the terminal's.
    let keyword = at(column);
    let name = at(column + 3);
    assert!(matches!(keyword, Color::Rgb(..)), "{keyword:?}");
    assert!(matches!(name, Color::Rgb(..)), "{name:?}");
    assert_ne!(keyword, name);
    assert_eq!(at(column + 1), keyword);

    // The stop's line keeps its breakpoint's dot and the arrow.
    let seven = line_with(&screen, "app.count += 1;");
    assert!(seven.contains("● 7 ▶"), "{seven}");

    // The screen's cells, for a picture of it.
    let screens = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .parent()
        .unwrap()
        .join("screens");
    std::fs::create_dir_all(&screens).unwrap();
    let mut cells = format!("{}\t{}\n", buffer.area.width, buffer.area.height);
    for cell in &buffer.content {
        let bold = cell.modifier.contains(Modifier::BOLD);
        cells.push_str(&format!(
            "{}\t{}\t{}\t{bold}\n",
            cell.symbol(),
            colour(cell.fg),
            colour(cell.bg)
        ));
    }
    std::fs::write(screens.join("tui-highlighted.cells"), cells).unwrap();
}
