//! A shell attaching to a base program `rawr run` started without one: the
//! running program takes the debugger in place, so it keeps its state and
//! its process (#2297).

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
