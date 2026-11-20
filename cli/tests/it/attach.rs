//! A shell attaching to a base program `rawr run` started without one: the
//! running program takes the debugger in place, so it keeps its state and
//! its process (#2297), and its output reaches the shell (#2288).

use std::io::{BufRead as _, BufReader, Write as _};
use std::os::unix::net::UnixStream;
use std::process::{Command, Stdio};

use serde_json::{json, Value};

use super::Sandbox;

/// The program: it says its pid once it has counted to 50, then counts on.
const LOOPING: &str = "fn main() {
    let pid = std::process::id();
    let mut count: u64 = 0;
    loop {
        count += 1;
        if count == 50 {
            println!(\"{pid}\");
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}
";

/// A connection to a run's session.
struct Shell {
    stream: UnixStream,
    lines: BufReader<UnixStream>,
    next: u64,
}

impl Shell {
    fn connect(socket: &std::path::Path) -> Self {
        let stream = UnixStream::connect(socket).unwrap();
        let lines = BufReader::new(stream.try_clone().unwrap());
        Self {
            stream,
            lines,
            next: 0,
        }
    }

    /// The result of `method` with `params`, past the notifications
    /// before it.
    fn call(&mut self, method: &str, params: Value) -> Value {
        self.next += 1;
        let id = self.next;
        let request = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        writeln!(self.stream, "{request}").unwrap();
        loop {
            let mut line = String::new();
            assert!(
                self.lines.read_line(&mut line).unwrap() > 0,
                "the session answers"
            );
            let message: Value = serde_json::from_str(&line).unwrap();
            if message["id"] == id {
                return message["result"].clone();
            }
        }
    }

    /// The next notification of `method`, past the others.
    fn heard(&mut self, method: &str) -> Value {
        loop {
            let mut line = String::new();
            assert!(
                self.lines.read_line(&mut line).unwrap() > 0,
                "the session tells it"
            );
            let message: Value = serde_json::from_str(&line).unwrap();
            if message["method"] == method {
                return message;
            }
        }
    }

    /// The value of the program's binding `name`, as its `{:?}` prints it.
    fn read(&mut self, name: &str) -> String {
        let read = self.call("program", json!({ "name": name }));
        assert_eq!(read["reached"], true, "{read}");
        read["bindings"][0]["value"]
            .as_str()
            .unwrap_or_else(|| panic!("{read}"))
            .to_owned()
    }
}

#[test]
fn a_shell_reads_a_running_base_program_without_restarting_it() {
    let sandbox = Sandbox::new("attach");
    let file = sandbox.work().join("looping.rs");
    std::fs::write(&file, LOOPING).unwrap();
    let missing = sandbox.path.join("missing");
    let mut child = Command::new(sandbox.path.join("bin/rawr"))
        .args(["run", "looping.rs"])
        .current_dir(sandbox.work())
        .env("PATH", "")
        .env("HOME", &missing)
        .env("CARGO_HOME", &missing)
        .env("RUSTUP_HOME", &missing)
        .env("RAWR_HOME", sandbox.path.join("home"))
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("CARGO")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    let mut said = String::new();
    output.read_line(&mut said).unwrap();
    let pid = said.trim().to_owned();
    assert!(!pid.is_empty(), "the program says its pid");

    let place = rawr_cli::events::session::Place::new(&file).unwrap();
    let mut shell = Shell::connect(&place.socket());
    let hello = shell.call("hello", json!({}));
    let count: u64 = shell.read("count").parse().unwrap();
    let again: u64 = shell.read("count").parse().unwrap();
    let read_pid = shell.read("pid");
    let events = shell.call("hello", json!({}))["events"].clone();
    let _ = child.kill();
    let _ = child.wait();

    // The count went on from where the program had it: nothing restarted.
    assert!(count >= 50, "{count}");
    assert!(again >= count, "{count} then {again}");
    assert_eq!(read_pid, pid);
    let told: Vec<&str> = events
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|event| event["event"].as_str())
        .collect();
    assert!(!told.contains(&"restarting"), "{told:?}");
    assert_eq!(hello["pid"], json!(child.id()), "{hello}");
}

/// The program: it says `early`, then counts, a line each few
/// milliseconds.
const COUNTING: &str = "fn main() {
    println!(\"early\");
    let mut count: u64 = 0;
    loop {
        count += 1;
        println!(\"tick {count}\");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}
";

/// A shell attached to a run started at a terminal hears the program's
/// output: what it printed before the attach in `hello`, and each line
/// from then on as an `output` notification. The terminal, a pipe here,
/// still gets every line, in order (#2288).
#[test]
fn a_shell_attached_to_a_terminal_run_hears_its_output() {
    let sandbox = Sandbox::new("attach-output");
    let file = sandbox.work().join("counting.rs");
    std::fs::write(&file, COUNTING).unwrap();
    let missing = sandbox.path.join("missing");
    let mut child = Command::new(sandbox.path.join("bin/rawr"))
        .args(["run", "counting.rs"])
        .current_dir(sandbox.work())
        .env("PATH", "")
        .env("HOME", &missing)
        .env("CARGO_HOME", &missing)
        .env("RUSTUP_HOME", &missing)
        .env("RAWR_HOME", sandbox.path.join("home"))
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("CARGO")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut terminal = BufReader::new(child.stdout.take().unwrap());
    let mut first = String::new();
    terminal.read_line(&mut first).unwrap();
    assert_eq!(first, "early\n");
    // The terminal is read throughout, as a terminal shows each line.
    let shown = std::thread::spawn(move || {
        let mut lines = vec![first];
        let mut line = String::new();
        for _ in 0..400 {
            if terminal.read_line(&mut line).unwrap() == 0 {
                break;
            }
            lines.push(std::mem::take(&mut line));
        }
        lines
    });

    let place = rawr_cli::events::session::Place::new(&file).unwrap();
    let mut shell = Shell::connect(&place.socket());
    let hello = shell.call("hello", json!({}));
    let kept: Vec<&str> = hello["output"]
        .as_array()
        .unwrap()
        .iter()
        .map(|piece| piece["text"].as_str().unwrap())
        .collect();
    assert_eq!(kept.first(), Some(&"early\n"), "{hello}");
    assert!(
        hello["capabilities"]
            .as_array()
            .unwrap()
            .contains(&json!("output")),
        "{hello}"
    );
    let heard = shell.heard("output");
    let lines = shown.join().unwrap();
    let _ = child.kill();
    let _ = child.wait();

    // The pieces `hello` kept and the one heard next follow on, as the
    // terminal shows them.
    assert_eq!(heard["params"]["stream"], "stdout", "{heard}");
    let mut told = kept.clone();
    told.push(heard["params"]["text"].as_str().unwrap());
    assert_eq!(told, lines[..told.len()], "{heard}");
    let expected: Vec<String> = std::iter::once("early\n".to_owned())
        .chain((1..lines.len()).map(|count| format!("tick {count}\n")))
        .collect();
    assert_eq!(lines, expected);
}
