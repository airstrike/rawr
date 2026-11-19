//! The debugger window against a scripted session: each test plays the
//! session's side, answering the requests the window sends, and drives
//! the window through iced_test's simulator.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use iced::keyboard::key::Named;
use iced::theme::Mode;
use iced_test::{simulator, Simulator};
use rawr_gui::app::Message;
use rawr_gui::session::{Event, Link, Phase, Transport};
use rawr_gui::{console, source, App};
use serde_json::{json, Value};

const FILE: &str = "/p/src/main.rs";

const SOURCE: &str = "\
#[derive(Debug)]
struct Point { x: i32, y: i32 }

fn tick(count: u32, point: Point) -> u32 {
    let next = count + 1;
    next
}

fn main() {
    tick(2, Point { x: 1, y: 2 });
}
";

/// The session's side of the connection: what the window sent, oldest
/// first.
#[derive(Default)]
struct Script {
    sent: Mutex<Vec<Value>>,
}

impl Transport for Script {
    fn send(&self, line: &str) {
        let message = serde_json::from_str(line).expect("the window sends JSON lines");
        self.sent
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(message);
    }
}

impl Script {
    /// The newest request whose params' `op` is `op`, or whose method is
    /// `op` for a request that isn't the program's.
    fn request(&self, op: &str) -> Value {
        self.sent
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .rev()
            .find(|sent| sent["params"]["op"] == op || (sent["method"] == op))
            .cloned()
            .unwrap_or_else(|| panic!("the window asked for `{op}`"))
    }

    /// The newest `program` request with no `op`: a read of values.
    fn inspect(&self) -> Value {
        self.sent
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .rev()
            .find(|sent| sent["method"] == "program" && sent["params"].get("op").is_none())
            .cloned()
            .expect("the window read the frame's values")
    }
}

fn line(message: &Value) -> Message {
    Message::Session(Event::Line(message.to_string()))
}

fn reply(app: &mut App, request: &Value, result: Value) {
    let _ = app.update(line(
        &json!({ "jsonrpc": "2.0", "id": request["id"], "result": result }),
    ));
}

/// The breakpoints at `lines` of the file, as the session tells them at
/// `revision`.
fn breakpoints(revision: u64, lines: &[u64]) -> Value {
    let specs: Vec<Value> = lines
        .iter()
        .map(|line| json!({ "file": FILE, "line": line, "text": "" }))
        .collect();
    json!({ "event": "breakpoints", "revision": revision, "breakpoints": specs })
}

/// The session tells every shell the breakpoints at `lines`.
fn told(app: &mut App, revision: u64, lines: &[u64]) {
    let _ = app.update(line(&json!({
        "jsonrpc": "2.0", "method": "event", "params": breakpoints(revision, lines),
    })));
}

/// A window connected to `script`, greeted by a running program and then
/// paused in `tick`, with its frames and bindings read.
fn paused(script: &Arc<Script>) -> App {
    let mut app = App::new(PathBuf::from("/tmp/rawr-test/session.sock"));
    let _ = app.update(Message::Session(Event::Connected(Link::new(
        Arc::clone(script) as Arc<dyn Transport>,
    ))));
    let hello = script.request("hello");
    reply(
        &mut app,
        &hello,
        json!({
            "protocol": 1, "root": "/p", "capabilities": ["events", "inspect"],
            "events": [{ "event": "running", "seq": 1 }],
            "output": [{ "stream": "stdout", "text": "ticking\n" }],
            "breakpoints": breakpoints(0, &[]),
        }),
    );
    let _ = app.update(line(&json!({
        "jsonrpc": "2.0", "method": "event",
        "params": { "event": "paused", "seq": 2, "file": FILE, "line": 5,
                    "function": "p::tick", "reason": "breakpoint" },
    })));
    let _ = app.update(Message::Source(source::Message::Loaded {
        file: FILE.to_owned(),
        text: Ok(SOURCE.to_owned()),
    }));
    let inspect = script.inspect();
    assert_eq!(inspect["params"]["frame"], 0, "{inspect}");
    reply(
        &mut app,
        &inspect,
        json!({
            "reached": true, "frame": 0, "file": FILE, "line": 5, "function": "p::tick",
            "bindings": [
                { "name": "count", "value": "2" },
                { "name": "point", "value": "Point { x: 1, y: 2 }" },
            ],
            "unread": null,
            "frames": [
                { "function": "p::tick", "file": FILE, "line": 5 },
                { "function": "p::main", "file": FILE, "line": 10 },
            ],
        }),
    );
    app
}

/// Draw the window as it is now into `name`, for a person to look at, in
/// the window's own theme: the file it wrote.
fn snapshot(app: &App, name: &str) -> PathBuf {
    let directory = Path::new(env!("CARGO_TARGET_TMPDIR")).join("rawr-gui");
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join(name);
    // The snapshot writes the image when none is there.
    for entry in std::fs::read_dir(&directory).unwrap().flatten() {
        if entry
            .file_name()
            .to_string_lossy()
            .starts_with(name.trim_end_matches(".png"))
        {
            let _ = std::fs::remove_file(entry.path());
        }
    }
    let mut ui = simulator(app.view());
    let shot = ui.snapshot(&app.theme()).unwrap();
    assert!(shot.matches_image(&path).unwrap());
    // The renderer's name goes after the one given.
    let stem = name.trim_end_matches(".png");
    std::fs::read_dir(&directory)
        .unwrap()
        .flatten()
        .map(|entry| entry.path())
        .find(|written| {
            written.file_stem().is_some_and(|file| {
                let file = file.to_string_lossy();
                file == stem || file.starts_with(&format!("{stem}-"))
            })
        })
        .expect("the snapshot wrote its image")
}

/// Drive the window with `act`, then hand it the messages that made.
fn interact(app: &mut App, act: impl FnOnce(&mut Simulator<'_, Message>)) {
    let messages: Vec<Message> = {
        let mut ui = simulator(app.view());
        act(&mut ui);
        ui.into_messages().collect()
    };
    for message in messages {
        let _ = app.update(message);
    }
}

/// Whether the window shows `text`.
fn shows(app: &App, text: &str) -> bool {
    simulator(app.view()).find(text).is_ok()
}

#[test]
fn a_stop_shows_its_frames_source_and_bindings_which_fold_open() {
    let script = Arc::new(Script::default());
    let mut app = paused(&script);
    for shown in [
        "p::tick",
        "p::main",
        "count",
        "Point { x: 1, y: 2 }",
        "   5",
        "ticking\n",
    ] {
        assert!(shows(&app, shown), "the window shows {shown:?}");
    }
    interact(&mut app, |ui| {
        let _ = ui.click("▸").unwrap();
    });
    assert!(shows(&app, "x"), "the open value shows its field");
    assert!(shows(&app, "Point {"));
    snapshot(&app, "stop.png");

    // A caller's frame reads its own bindings.
    interact(&mut app, |ui| {
        let _ = ui.click("p::main").unwrap();
    });
    assert_eq!(script.inspect()["params"]["frame"], 1);
}

#[test]
fn the_gutter_sets_and_clears_a_breakpoint() {
    let script = Arc::new(Script::default());
    let mut app = paused(&script);
    interact(&mut app, |ui| {
        let _ = ui.click("   7").unwrap();
    });
    let set = script.request("break");
    assert_eq!(
        (&set["params"]["file"], &set["params"]["line"]),
        (&json!(FILE), &json!(7))
    );
    reply(
        &mut app,
        &set,
        json!({ "set": { "file": FILE, "line": 7 }, "lines": [], "compiled": true, "paused": null }),
    );
    told(&mut app, 1, &[7]);
    assert!(shows(&app, "●"), "the line shows its breakpoint");
    snapshot(&app, "breakpoint.png");

    interact(&mut app, |ui| {
        let _ = ui.click("   7").unwrap();
    });
    let clear = script.request("clear");
    assert_eq!(clear["params"]["line"], 7);
    reply(
        &mut app,
        &clear,
        json!({ "cleared": 1, "left": [], "paused": null }),
    );
    told(&mut app, 2, &[]);
    assert!(!shows(&app, "●"), "the breakpoint is gone");
}

/// A breakpoint another shell sets or clears shows as the session tells
/// it, and a window that attaches later shows those set already (#2278).
#[test]
fn the_gutter_shows_the_breakpoints_every_shell_sets() {
    let script = Arc::new(Script::default());
    let mut app = paused(&script);
    assert!(!shows(&app, "●"));
    told(&mut app, 1, &[7]);
    assert!(shows(&app, "●"), "another shell's breakpoint shows");
    // A list older than the one shown changes nothing.
    told(&mut app, 0, &[]);
    assert!(shows(&app, "●"));
    told(&mut app, 2, &[]);
    assert!(!shows(&app, "●"), "another shell's clear shows");

    let mut late = App::new(PathBuf::from("/tmp/rawr-test/session.sock"));
    let _ = late.update(Message::Session(Event::Connected(Link::new(
        Arc::clone(&script) as Arc<dyn Transport>,
    ))));
    reply(
        &mut late,
        &script.request("hello"),
        json!({
            "protocol": 1, "root": FILE, "capabilities": ["events"],
            "events": [{ "event": "running", "seq": 1 }], "output": [],
            "breakpoints": breakpoints(3, &[7]),
        }),
    );
    let _ = late.update(Message::Source(source::Message::Loaded {
        file: FILE.to_owned(),
        text: Ok(SOURCE.to_owned()),
    }));
    assert!(shows(&late, "●"), "hello's breakpoints show");
    assert!(
        script
            .sent
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .all(|sent| sent["params"]["op"] != "clear"),
        "the window asks the program nothing to learn its breakpoints"
    );
}

#[test]
fn the_console_evaluates_where_the_program_is_paused_and_a_step_moves_the_stop() {
    let script = Arc::new(Script::default());
    let mut app = paused(&script);
    let _ = app.update(Message::Console(console::Message::Input(
        "count + 1".to_owned(),
    )));
    interact(&mut app, |ui| {
        let _ = ui.click(iced::widget::Id::new(console::INPUT)).unwrap();
        let _ = ui.tap_key(Named::Enter);
    });
    let eval = script.request("eval");
    assert_eq!(eval["params"]["code"], "count + 1");
    reply(&mut app, &eval, json!({ "value": "3" }));
    assert!(shows(&app, "> count + 1"));
    assert!(shows(&app, "3"), "the console shows the answer");
    snapshot(&app, "eval.png");

    interact(&mut app, |ui| {
        let _ = ui.click("Step over").unwrap();
    });
    let step = script.request("step");
    assert_eq!(step["params"]["kind"], "over");
    reply(
        &mut app,
        &step,
        json!({ "paused": { "file": FILE, "line": 6, "function": "p::tick", "reason": "step" } }),
    );
    assert!(shows(&app, "paused (step) at line 6"));
    // The new stop's frame is read.
    assert_eq!(script.inspect()["params"]["frame"], 0);
}

#[test]
fn a_stop_draws_its_rust_source_in_the_colours_of_the_windows_mode() {
    let script = Arc::new(Script::default());
    let mut app = paused(&script);
    // The stop's line keeps its breakpoint's dot among the colours.
    interact(&mut app, |ui| {
        let _ = ui.click("   5").unwrap();
    });
    reply(
        &mut app,
        &script.request("break"),
        json!({ "set": { "file": FILE, "line": 5 }, "lines": [], "compiled": true, "paused": null }),
    );
    told(&mut app, 1, &[5]);
    assert!(shows(&app, "●"));
    let dark = snapshot(&app, "highlighted.png");
    let screens = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .parent()
        .unwrap()
        .join("screens");
    std::fs::create_dir_all(&screens).unwrap();
    std::fs::copy(&dark, screens.join("gui-highlighted.png")).unwrap();

    // The system turning light turns the window and its source light.
    let _ = app.update(Message::Mode(Mode::Light));
    assert_eq!(app.theme(), iced::Theme::Light);
    let light = snapshot(&app, "lit.png");
    std::fs::copy(&light, screens.join("gui-highlighted-light.png")).unwrap();
    assert_ne!(
        std::fs::read(&dark).unwrap(),
        std::fs::read(&light).unwrap()
    );
}

#[test]
fn a_step_that_pauses_where_it_started_reads_the_frame_again() {
    let script = Arc::new(Script::default());
    let mut app = paused(&script);
    let first = script.inspect();
    interact(&mut app, |ui| {
        let _ = ui.click("Step over").unwrap();
    });
    let step = script.request("step");
    assert_eq!(step["params"]["kind"], "over");
    // A loop comes back to the breakpoint it stepped from: the answer
    // names the same place, and the values there are new.
    reply(
        &mut app,
        &step,
        json!({ "paused": { "file": FILE, "line": 5, "function": "p::tick", "reason": "breakpoint" } }),
    );
    let read = script.inspect();
    assert_ne!(read["id"], first["id"], "the new pause's frame is read");
    reply(
        &mut app,
        &read,
        json!({
            "reached": true, "frame": 0, "file": FILE, "line": 5, "function": "p::tick",
            "bindings": [{ "name": "count", "value": "3" }],
            "unread": null,
            "frames": [{ "function": "p::tick", "file": FILE, "line": 5 }],
        }),
    );
    assert!(shows(&app, "3"), "the window shows the new value");
}

#[test]
fn a_step_into_native_code_keeps_the_pause_its_event_told_first() {
    let script = Arc::new(Script::default());
    let mut app = paused(&script);
    interact(&mut app, |ui| {
        let _ = ui.click("Step over").unwrap();
    });
    let step = script.request("step");
    // The program runs on once asked: only a pause can be asked of it, as
    // a step may wait long for its pause.
    assert_eq!(*app.phase(), Phase::Running);
    interact(&mut app, |ui| {
        let _ = ui.click("Continue").unwrap();
    });
    assert!(
        script
            .sent
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .all(|sent| sent["params"]["op"] != "continue"),
        "a running program is asked no continue"
    );
    // The step went on into native code, which called the program again:
    // the pause it led to is told before the step's answer comes.
    let _ = app.update(line(&json!({
        "jsonrpc": "2.0", "method": "event",
        "params": { "event": "paused", "seq": 3, "file": FILE, "line": 10,
                    "function": "p::main", "reason": "step" },
    })));
    let read = script.inspect();
    reply(
        &mut app,
        &read,
        json!({
            "reached": true, "frame": 0, "file": FILE, "line": 10, "function": "p::main",
            "bindings": [], "unread": null,
            "frames": [{ "function": "p::main", "file": FILE, "line": 10 }],
        }),
    );
    reply(&mut app, &step, json!({ "paused": null, "native": true }));
    assert_eq!(
        app.phase().stop().and_then(|stop| stop.line),
        Some(10),
        "the step's answer leaves the pause its event told"
    );
    assert!(shows(&app, "p::main"), "the new pause's frames show");

    // Continue reaches the program from the new pause.
    interact(&mut app, |ui| {
        let _ = ui.click("Continue").unwrap();
    });
    assert_eq!(script.request("continue")["params"]["op"], "continue");
    assert_eq!(*app.phase(), Phase::Running);
}

#[test]
fn each_capture_writes_a_file_of_its_own() {
    let script = Arc::new(Script::default());
    let mut app = paused(&script);
    let first = script.request("screenshot");
    // A restart starts the program again, which the window draws again
    // while the first capture may still be on its way.
    for (seq, event) in [(3, "restarting"), (4, "running")] {
        let _ = app.update(line(&json!({
            "jsonrpc": "2.0", "method": "event", "params": { "event": event, "seq": seq },
        })));
    }
    let second = script.request("screenshot");
    assert_ne!(first["id"], second["id"]);
    assert_ne!(
        first["params"]["file"], second["params"]["file"],
        "two captures in flight never share a file"
    );
}

/// What the session tells when the program pauses at `line` of `function`
/// for a pause asked of it.
fn paused_at(seq: u64, line: u64, function: &str) -> Value {
    json!({
        "jsonrpc": "2.0", "method": "event",
        "params": { "event": "paused", "seq": seq, "file": FILE, "line": line,
                    "function": function, "reason": "pause" },
    })
}

#[test]
fn each_pause_reads_its_frame_and_shows_only_the_newest_answer() {
    let script = Arc::new(Script::default());
    let mut app = paused(&script);
    interact(&mut app, |ui| {
        let _ = ui.click("Continue").unwrap();
    });
    // The program pauses in `tick`, and the window reads that pause; it
    // goes on and pauses in `main` before the first read is answered.
    let _ = app.update(line(&paused_at(3, 6, "p::tick")));
    let first = script.inspect();
    interact(&mut app, |ui| {
        let _ = ui.click("Continue").unwrap();
    });
    let _ = app.update(line(&json!({
        "jsonrpc": "2.0", "method": "event", "params": { "event": "resumed", "seq": 4 },
    })));
    let _ = app.update(line(&paused_at(5, 10, "p::main")));
    let second = script.inspect();
    assert_ne!(first["id"], second["id"], "the second pause is read again");
    reply(
        &mut app,
        &second,
        json!({
            "reached": true, "frame": 0, "file": FILE, "line": 10, "function": "p::main",
            "bindings": [{ "name": "total", "value": "11" }],
            "unread": null,
            "frames": [{ "function": "p::main", "file": FILE, "line": 10 }],
        }),
    );
    // The first pause's read is answered last, with that pause's values.
    reply(
        &mut app,
        &first,
        json!({
            "reached": true, "frame": 0, "file": FILE, "line": 6, "function": "p::tick",
            "bindings": [{ "name": "next", "value": "3" }],
            "unread": null,
            "frames": [
                { "function": "p::tick", "file": FILE, "line": 6 },
                { "function": "p::main", "file": FILE, "line": 10 },
            ],
        }),
    );
    assert!(shows(&app, "paused (pause) at line 10"));
    assert!(shows(&app, "total"), "the newest pause's bindings show");
    assert!(shows(&app, "11"));
    assert!(!shows(&app, "next"), "an earlier pause's bindings are gone");
    assert!(
        !shows(&app, "p::tick"),
        "an earlier pause's frames are gone"
    );
}
