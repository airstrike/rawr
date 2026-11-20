//! D1: a debugger session. The inspector keeps the running generation's
//! locator, program and files behind three locks; placing a compiled
//! generation's breakpoints locks the locator once per statement, and a
//! stepping session locks it again where a request reads a frame's line.
//!
//! The inspector serves its socket as `rawr run`'s does, and a tool thread
//! asks over it as `rawr dbg` and the editor do, while this thread calls
//! the program's function as native code does.

use serde_json::{json, Value};
use std::io::{BufRead as _, Write as _};
use std::path::{Path, PathBuf};

use crate::workload::{About, Ran, Workload};

pub const NAME: &str = "debugger-steps";

/// Pauses at `work`'s breakpoint in one session, each stepped over to
/// its end.
const PAUSES: usize = 5;

const SOURCE: &str = "fn work(step: i64) -> i64 {
    let doubled = step * 2;
    let tripled = step * 3;
    let mut value = doubled;
    value -= tripled;
    value += step;
    value
}
";

/// The inspector's socket, in a directory of its own that goes with it.
struct Socket {
    directory: PathBuf,
    path: PathBuf,
}

impl Drop for Socket {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

pub fn workload() -> Workload {
    // A Unix socket's path is short (104 bytes on macOS), and a worktree's
    // target directory is often longer: the system's temporary directory
    // holds it, and the directory goes when the workload does.
    let directory = std::env::temp_dir().join(format!("rawr-lock-bench-{}", std::process::id()));
    std::fs::create_dir_all(&directory).expect("the temporary directory takes a directory");
    let socket = Socket {
        path: directory.join("s"),
        directory,
    };
    let inspector = rawr::inspect::Inspector::bind(&socket.path).expect("the socket binds");
    let mut engine = rawr::Engine::new();
    inspector.attach(&mut engine);
    let ast = engine
        .compile(SOURCE)
        .unwrap_or_else(|error| panic!("the debugged program compiles: {error}"));
    inspector.locate_with(|position| Some(("main.rs".to_owned(), position.line()?)));
    let about = About {
        name: NAME,
        smells: &["D1"],
        run: "a generation placed, then 5 pauses at a breakpoint, each stepped over to the function's end",
    };
    Workload::new(about, SOURCE.to_owned(), move || {
        inspector.compiled(&ast);
        let set = ask(&socket.path, &json!({ "op": "break", "function": "work" }));
        assert!(set.get("error").is_none(), "the breakpoint is set: {set}");
        let path = socket.path.clone();
        let tool = std::thread::spawn(move || session(&path));
        while !tool.is_finished() {
            let _: i64 = engine
                .call_fn(&mut rawr::Scope::new(), &ast, "work", (1_i64,))
                .unwrap_or_else(|error| panic!("the debugged program runs: {error}"));
        }
        let steps = tool.join().expect("the tool's session ends");
        let mut ran = Ran::default();
        ran.report("steps", "count", vec![steps as f64]);
        ran
    })
}

/// A tool's session: at each pause, step over until the function returns
/// to native code; then clear the breakpoint and let the program go.
/// How many steps it took.
fn session(socket: &Path) -> usize {
    let mut steps = 0;
    for _ in 0..PAUSES {
        let paused = ask(socket, &json!({ "name": "nothing" }));
        assert_eq!(paused["paused"]["function"], "work", "{paused}");
        loop {
            let step = ask(socket, &json!({ "op": "step", "kind": "over" }));
            steps += 1;
            assert!(steps < PAUSES * 20, "a step reaches native code: {step}");
            if step["native"] == true {
                break;
            }
        }
    }
    let _ = ask(socket, &json!({ "name": "nothing" }));
    let cleared = ask(socket, &json!({ "op": "clear" }));
    assert_eq!(cleared["cleared"], 1, "{cleared}");
    let continued = ask(socket, &json!({ "op": "continue" }));
    assert!(continued["paused"].is_null(), "{continued}");
    steps
}

/// One request on the socket, and its answer.
fn ask(socket: &Path, request: &Value) -> Value {
    let mut stream =
        std::os::unix::net::UnixStream::connect(socket).expect("the inspector listens");
    writeln!(stream, "{request}").expect("the inspector reads a request");
    let mut line = String::new();
    std::io::BufReader::new(stream)
        .read_line(&mut line)
        .expect("the inspector answers");
    serde_json::from_str(&line).unwrap_or(Value::Null)
}
