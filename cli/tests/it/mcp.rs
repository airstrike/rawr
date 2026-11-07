//! `rawr mcp` end to end: the JSON-RPC loop with a stand-in extension, and
//! `new`, `check` and `run` on a plain crate.

use std::io::{BufRead as _, BufReader, Write as _};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::Sandbox;

/// A stand-in `rawr-fake` whose manifest declares tools, and whose
/// `mcp call` answers `echo` with a progress record and a result, and
/// spins on `hang` after writing its pid.
const FAKE: &str = r#"#!/bin/sh
if [ "$1" = --rawr-manifest ]; then
  printf '%s\n' 'format = 1' 'name = "rawr-fake"' 'crates = ["fakecrate"]' \
    '[[tool]]' 'name = "echo"' 'description = "Echo."' \
    'input = "{\"type\": \"object\", \"properties\": {\"word\": {\"type\": \"string\"}}}"' \
    '[[tool]]' 'name = "hang"' 'description = "Hang."' 'input = "{\"type\": \"object\"}"' \
    '[[tool]]' 'name = "check"' 'description = "Hidden by core."' 'input = "{\"type\": \"object\"}"'
  exit 0
fi
if [ "$1" = mcp ] && [ "$2" = call ]; then
  read -r input || true
  case "$3" in
    echo)
      case "$input" in *'"root":"'*) found=has-root ;; *) found=no-root ;; esac
      printf '%s\n' '{"progress": 1, "total": 2, "message": "half"}'
      printf '{"result": {"content": [{"type": "text", "text": "%s"}], "isError": false}}\n' "$found"
      ;;
    hang)
      echo $$ > "$PWD/hang.pid"
      while :; do :; done
      ;;
  esac
fi
"#;

struct Client {
    child: Child,
    input: Option<ChildStdin>,
    messages: mpsc::Receiver<Value>,
    next: u64,
}

impl Client {
    fn start(sandbox: &Sandbox) -> Self {
        let missing = sandbox.path.join("missing");
        let mut child = Command::new(sandbox.path.join("bin/rawr"))
            .arg("mcp")
            .current_dir(sandbox.work())
            .env("PATH", "")
            .env("HOME", &missing)
            .env("CARGO_HOME", &missing)
            .env("RUSTUP_HOME", &missing)
            .env("RAWR_HOME", sandbox.path.join("home"))
            .env_remove("CARGO_TARGET_DIR")
            .env_remove("CARGO")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let (sender, messages) = mpsc::channel();
        let stdout = child.stdout.take().unwrap();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if sender.send(serde_json::from_str(&line).unwrap()).is_err() {
                    break;
                }
            }
        });
        let input = child.stdin.take();
        Self {
            child,
            input,
            messages,
            next: 0,
        }
    }

    fn send(&mut self, message: &Value) {
        let input = self.input.as_mut().unwrap();
        writeln!(input, "{message}").unwrap();
        input.flush().unwrap();
    }

    /// Send a request; its id.
    fn ask(&mut self, method: &str, params: Value) -> u64 {
        self.next += 1;
        let id = self.next;
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        id
    }

    /// The messages until the reply to `id`, which comes last.
    fn until(&mut self, id: u64) -> Vec<Value> {
        let deadline = Instant::now() + Duration::from_secs(60);
        let mut seen = Vec::new();
        loop {
            let message = self
                .messages
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .expect("a reply in time");
            let done = message["id"] == id && message.get("method").is_none();
            seen.push(message);
            if done {
                return seen;
            }
        }
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.ask(method, params);
        self.until(id).pop().unwrap()
    }

    fn call(&mut self, name: &str, arguments: Value) -> Value {
        self.request(
            "tools/call",
            json!({ "name": name, "arguments": arguments }),
        )["result"]
            .clone()
    }
}

fn fake(sandbox: &Sandbox) {
    let path = sandbox.path.join("bin/rawr-fake");
    std::fs::write(&path, FAKE).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

#[test]
fn the_server_lists_extension_tools_relays_progress_and_stays_responsive_while_a_call_is_cancelled()
{
    let sandbox = Sandbox::new("mcp-loop");
    fake(&sandbox);
    let mut client = Client::start(&sandbox);
    let initialized = client.request(
        "initialize",
        json!({ "protocolVersion": "2025-03-26", "capabilities": {} }),
    );
    assert_eq!(initialized["result"]["protocolVersion"], "2025-03-26");
    assert_eq!(initialized["result"]["serverInfo"]["name"], "rawr");
    assert_eq!(
        initialized["result"]["serverInfo"]["version"],
        rawr::VERSION
    );
    let instructions = initialized["result"]["instructions"].as_str().unwrap();
    assert!(
        instructions.contains("rawr-fake (") && instructions.contains("adds echo, hang"),
        "{instructions}"
    );
    client.send(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));

    let listed = client.request("tools/list", json!({}));
    let tools = listed["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = tools
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect();
    assert_eq!(
        names,
        [
            "new",
            "check",
            "run",
            "start",
            "wait",
            "status",
            "stop",
            "screenshot",
            "inspect",
            "break",
            "step",
            "continue",
            "clear",
            "echo",
            "hang"
        ]
    );
    let echo = &tools[13];
    assert!(echo["description"].as_str().unwrap().contains("rawr-fake"));
    assert!(echo["inputSchema"]["properties"]["path"].is_object());
    assert!(echo["inputSchema"]["properties"]["word"].is_object());

    // Progress first, then the result, which the extension built from the
    // root core resolved.
    let id = client.ask(
        "tools/call",
        json!({ "name": "echo", "arguments": {}, "_meta": { "progressToken": "t" } }),
    );
    let messages = client.until(id);
    assert_eq!(
        messages[0]["method"], "notifications/progress",
        "{messages:?}"
    );
    assert_eq!(messages[0]["params"]["progressToken"], "t");
    assert_eq!(messages[0]["params"]["message"], "half");
    assert_eq!(
        messages.last().unwrap()["result"]["content"][0]["text"],
        "has-root"
    );

    // A hanging call doesn't hold up `ping`, and a cancel ends its process.
    let hanging = client.ask("tools/call", json!({ "name": "hang", "arguments": {} }));
    let pid_file = sandbox.work().join("hang.pid");
    let deadline = Instant::now() + Duration::from_secs(20);
    while !pid_file.exists() {
        assert!(Instant::now() < deadline, "the hanging call never started");
        std::thread::sleep(Duration::from_millis(20));
    }
    let ping = client.request("ping", json!({}));
    assert_eq!(ping["result"], json!({}));
    client.send(&json!({
        "jsonrpc": "2.0",
        "method": "notifications/cancelled",
        "params": { "requestId": hanging },
    }));
    let pid = std::fs::read_to_string(&pid_file)
        .unwrap()
        .trim()
        .to_owned();
    let deadline = Instant::now() + Duration::from_secs(5);
    while Command::new("/bin/kill")
        .args(["-0", &pid])
        .stderr(Stdio::null())
        .status()
        .unwrap()
        .success()
    {
        assert!(
            Instant::now() < deadline,
            "the cancelled call's process lives on"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    // The cancelled request got no answer: the next reply is the next one's.
    let after = client.ask("ping", json!({}));
    let messages = client.until(after);
    assert!(messages.iter().all(|message| message["id"] != hanging));

    let unknown = client.request("nope", json!({}));
    assert_eq!(unknown["error"]["code"], -32601);

    drop(client.input.take());
    let status = client.child.wait().unwrap();
    assert!(status.success());
}

#[test]
fn new_check_and_run_make_and_run_a_plain_crate_with_arguments_and_stdin() {
    let sandbox = Sandbox::new("mcp-plain");
    let mut client = Client::start(&sandbox);
    client.request("initialize", json!({ "protocolVersion": "2025-06-18" }));

    let made = client.call("new", json!({ "path": "hello" }));
    assert_eq!(made["isError"], false, "{made}");
    let root = sandbox.work().join("hello");
    assert!(made["structuredContent"]["files"]
        .as_array()
        .unwrap()
        .iter()
        .any(|file| file == &json!(root.join("src/main.rs"))));
    std::fs::write(
        root.join("src/main.rs"),
        "// rawr\nuse std::io::Read;\n\nfn main() {\n    let args: Vec<String> = std::env::args().skip(1).collect();\n    let mut input = String::new();\n    std::io::stdin().read_to_string(&mut input).unwrap();\n    println!(\"{} {}\", args.join(\",\"), input.trim().to_uppercase());\n}\n",
    )
    .unwrap();

    let checked = client.call("check", json!({ "path": "hello" }));
    assert_eq!(checked["isError"], false, "{checked}");
    assert_eq!(checked["structuredContent"]["ok"], true);

    let ran = client.call(
        "run",
        json!({ "path": "hello", "args": ["-n", "pie"], "stdin": "apple" }),
    );
    assert_eq!(ran["structuredContent"]["status"], 0, "{ran}");
    assert_eq!(ran["structuredContent"]["stdout"], "-n,pie APPLE\n");

    // A type error comes back as data with an absolute file and a position.
    std::fs::write(
        root.join("src/main.rs"),
        "// rawr\nfn main() {\n    let count: i64 = \"three\";\n    println!(\"{count}\");\n}\n",
    )
    .unwrap();
    let failed = client.call("check", json!({ "path": "hello" }));
    assert_eq!(failed["isError"], true);
    let diagnostic = &failed["structuredContent"]["diagnostics"][0];
    assert_eq!(diagnostic["file"], json!(root.join("src/main.rs")));
    assert_eq!(diagnostic["line"], 3);
    assert_eq!(diagnostic["severity"], "error");
    let text = failed["content"][0]["text"].as_str().unwrap();
    assert!(
        text.starts_with(&format!("{}:3:", root.join("src/main.rs").display())),
        "{text}"
    );

    let refused = client.call("start", json!({ "path": "nowhere" }));
    assert_eq!(refused["isError"], true);
    drop(client.input.take());
    assert!(client.child.wait().unwrap().success());
}

#[test]
fn a_started_program_reports_its_end_and_runs_again_on_a_save() {
    let sandbox = Sandbox::new("mcp-start");
    let mut client = Client::start(&sandbox);
    client.request("initialize", json!({ "protocolVersion": "2025-06-18" }));
    let made = client.call("new", json!({ "path": "app" }));
    assert_eq!(made["isError"], false, "{made}");
    let main = sandbox.work().join("app/src/main.rs");
    std::fs::write(&main, "// rawr\nfn main() {\n    println!(\"one\");\n}\n").unwrap();

    let started = client.call("start", json!({ "path": "app", "wait_ms": 20000 }));
    assert_eq!(started["isError"], false, "{started}");
    let state = started["structuredContent"]["state"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(state == "running" || state == "ended", "{started}");
    // The program ends; `wait` returns that, with what it printed.
    let mut seen: Vec<String> = started["structuredContent"]["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|event| event["event"].as_str().unwrap().to_owned())
        .collect();
    let mut output = started["structuredContent"]["stdout"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    let deadline = Instant::now() + Duration::from_secs(30);
    while !seen.iter().any(|event| event == "ended") {
        assert!(Instant::now() < deadline, "no `ended`: {seen:?}");
        let waited = client.call("wait", json!({ "path": "app", "timeout_ms": 5000 }));
        output.push_str(
            waited["structuredContent"]["stdout"]
                .as_str()
                .unwrap_or_default(),
        );
        for event in waited["structuredContent"]["events"].as_array().unwrap() {
            seen.push(event["event"].as_str().unwrap().to_owned());
        }
    }
    assert!(output.contains("one\n"), "{output}");

    // Supervised, an ended program runs again on the next save.
    std::fs::write(&main, "// rawr\nfn main() {\n    println!(\"two\");\n}\n").unwrap();
    let mut output = String::new();
    let deadline = Instant::now() + Duration::from_secs(30);
    while !output.contains("two") {
        assert!(
            Instant::now() < deadline,
            "the save did not run it again: {output}"
        );
        let waited = client.call("wait", json!({ "path": "app", "timeout_ms": 5000 }));
        output.push_str(
            waited["structuredContent"]["stdout"]
                .as_str()
                .unwrap_or_default(),
        );
    }
    let status = client.call("status", json!({}));
    assert_eq!(
        status["structuredContent"]["sessions"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let stopped = client.call("stop", json!({ "path": "app" }));
    assert_eq!(stopped["structuredContent"]["stopped"], true, "{stopped}");
    let pid = started["structuredContent"]["pid"].to_string();
    assert!(!Command::new("/bin/kill")
        .args(["-0", &pid])
        .stderr(Stdio::null())
        .status()
        .unwrap()
        .success());
    drop(client.input.take());
    assert!(client.child.wait().unwrap().success());
}

#[test]
fn inspect_reads_a_running_programs_bindings_as_rust_prints_them() {
    let sandbox = Sandbox::new("mcp-inspect");
    let mut client = Client::start(&sandbox);
    client.request("initialize", json!({ "protocolVersion": "2025-06-18" }));
    let made = client.call("new", json!({ "path": "counter" }));
    assert_eq!(made["isError"], false, "{made}");
    let main = sandbox.work().join("counter/src/main.rs");
    std::fs::write(
        &main,
        "// rawr\n\
         #[derive(Debug)]\n\
         struct Counter {\n    count: u64,\n    label: String,\n}\n\
         \n\
         fn tick(counter: &mut Counter, step: u64) {\n    counter.count += step;\n}\n\
         \n\
         fn main() {\n    let mut counter = Counter { count: 0, label: String::from(\"ticks\") };\n    let names = vec![\"a\", \"b\"];\n    loop {\n        tick(&mut counter, 1);\n        std::thread::sleep(std::time::Duration::from_millis(2));\n    }\n}\n",
    )
    .unwrap();
    let started = client.call("start", json!({ "path": "counter", "wait_ms": 20000 }));
    assert_eq!(
        started["structuredContent"]["state"], "running",
        "{started}"
    );

    // One binding by name, at a step where it is in scope: `main`'s.
    let read = client.call(
        "inspect",
        json!({ "path": "counter", "name": "names", "timeout_ms": 5000 }),
    );
    assert_eq!(read["isError"], false, "{read}");
    let structured = &read["structuredContent"];
    assert_eq!(structured["file"], json!(main), "{read}");
    assert_eq!(structured["function"], "counter::main", "{read}");
    // Inspect pauses at whichever step of `main`'s loop comes next.
    let line = structured["line"].as_u64().unwrap_or_default();
    assert!((15..=17).contains(&line), "{read}");
    assert_eq!(
        structured["bindings"],
        json!([{ "name": "names", "value": "[\"a\", \"b\"]" }])
    );

    // A parameter is read in its function.
    let read = client.call(
        "inspect",
        json!({ "path": "counter", "name": "step", "timeout_ms": 5000 }),
    );
    assert_eq!(
        read["structuredContent"]["function"], "counter::tick",
        "{read}"
    );
    assert_eq!(
        read["structuredContent"]["bindings"],
        json!([{ "name": "step", "value": "1" }])
    );

    // The program's own type prints through its derived `Debug`.
    let read = client.call(
        "inspect",
        json!({ "path": "counter", "name": "counter", "file": "counter/src/main.rs", "timeout_ms": 5000 }),
    );
    let value = read["structuredContent"]["bindings"][0]["value"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    assert!(
        value.starts_with("Counter { count: ") && value.ends_with(", label: \"ticks\" }"),
        "{read}"
    );
    let text = read["content"][0]["text"].as_str().unwrap();
    assert!(text.starts_with(&format!("{}:", main.display())), "{text}");
    assert!(text.contains("counter = Counter { count: "), "{text}");

    // A binding the program never reaches says so.
    let missing = client.call(
        "inspect",
        json!({ "path": "counter", "name": "nowhere", "timeout_ms": 300 }),
    );
    assert_eq!(missing["isError"], true, "{missing}");
    assert!(
        missing["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("where `nowhere` is in scope"),
        "{missing}"
    );
    // Every binding in scope where the program is, and it still runs.
    let read = client.call("inspect", json!({ "path": "counter", "timeout_ms": 5000 }));
    assert_eq!(read["isError"], false, "{read}");
    let names: Vec<&str> = read["structuredContent"]["bindings"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|binding| binding["name"].as_str())
        .collect();
    assert!(
        names == ["counter", "names"] || names == ["counter", "step"],
        "{read}"
    );

    // Paused in `tick`, `wait` and `status` say where (#535).
    let set = client.call(
        "break",
        json!({ "path": "counter", "file": "counter/src/main.rs", "line": 9, "timeout_ms": 5000 }),
    );
    assert_eq!(paused(&set), (9, "counter::tick".to_owned()), "{set}");
    let status = client.call("status", json!({ "path": "counter" }));
    let session = &status["structuredContent"]["sessions"][0];
    assert_eq!(session["state"], "paused", "{status}");
    assert_eq!(session["paused"]["line"], 9, "{status}");
    assert!(
        status["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains(&format!(
                "paused at {}:9 in `counter::tick`",
                main.display()
            )),
        "{status}"
    );
    let waited = client.call("wait", json!({ "path": "counter", "timeout_ms": 5000 }));
    let waited = &waited["structuredContent"];
    assert_eq!(waited["state"], "paused", "{waited}");
    assert_eq!(waited["paused"]["line"], 9, "{waited}");
    assert!(
        waited["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|event| event["event"] == "paused" && event["line"] == 9),
        "{waited}"
    );

    // The `&mut` parameter reads under its own name (#534), first as the
    // program wrote it.
    let read = client.call("inspect", json!({ "path": "counter", "timeout_ms": 5000 }));
    let structured = &read["structuredContent"];
    let names: Vec<&str> = structured["bindings"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|binding| binding["name"].as_str())
        .collect();
    assert_eq!(names, ["counter", "step"], "{read}");
    assert!(
        structured["bindings"][0]["value"]
            .as_str()
            .unwrap_or_default()
            .starts_with("Counter { count: "),
        "{read}"
    );
    // The calls it is in, and the caller's bindings with `frame` (#511).
    assert_eq!(
        structured["frames"],
        json!([
            { "function": "counter::tick", "file": main, "line": 9 },
            { "function": "counter::main", "file": main, "line": 16 },
        ]),
        "{read}"
    );
    let text = read["content"][0]["text"].as_str().unwrap();
    assert!(
        text.contains(&format!("#1 counter::main at {}:16", main.display())),
        "{text}"
    );
    let caller = client.call(
        "inspect",
        json!({ "path": "counter", "frame": 1, "timeout_ms": 5000 }),
    );
    let structured = &caller["structuredContent"];
    assert_eq!(structured["function"], "counter::main", "{caller}");
    assert_eq!(structured["line"], 16, "{caller}");
    assert_eq!(structured["frame"], 1, "{caller}");
    let names: Vec<&str> = structured["bindings"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|binding| binding["name"].as_str())
        .collect();
    assert_eq!(names, ["counter", "names"], "{caller}");
    // `tick` holds `counter` by `&mut` while it runs, so the caller's binding
    // reads as borrowed, as rustc would describe it (#540).
    assert_eq!(
        structured["bindings"][0]["error"], "`counter` is borrowed by the call to `counter::tick`",
        "{caller}"
    );
    let deeper = client.call(
        "inspect",
        json!({ "path": "counter", "frame": 2, "timeout_ms": 300 }),
    );
    assert_eq!(deeper["isError"], true, "{deeper}");
    assert!(
        deeper["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("the program is 2 frames deep, so there is no frame 2"),
        "{deeper}"
    );

    // Going on, it runs again.
    client.call("clear", json!({ "path": "counter" }));
    let continued = client.call("continue", json!({ "path": "counter", "timeout_ms": 300 }));
    assert!(
        continued["structuredContent"]["paused"].is_null(),
        "{continued}"
    );
    let status = client.call("status", json!({ "path": "counter" }));
    assert_eq!(
        status["structuredContent"]["sessions"][0]["state"], "running",
        "{status}"
    );

    let stopped = client.call("stop", json!({ "path": "counter" }));
    assert_eq!(stopped["structuredContent"]["stopped"], true, "{stopped}");
    drop(client.input.take());
    assert!(client.child.wait().unwrap().success());
}

/// Where a debugger tool's answer says the program paused: its line and
/// function.
fn paused(answer: &Value) -> (u64, String) {
    let paused = &answer["structuredContent"]["paused"];
    (
        paused["line"]
            .as_u64()
            .unwrap_or_else(|| panic!("not paused: {answer}")),
        paused["function"].as_str().unwrap_or_default().to_owned(),
    )
}

fn count(client: &mut Client) -> u64 {
    let read = client.call(
        "inspect",
        json!({ "path": "looper", "name": "count", "timeout_ms": 5000 }),
    );
    assert_eq!(read["isError"], false, "{read}");
    read["structuredContent"]["bindings"][0]["value"]
        .as_str()
        .and_then(|value| value.parse().ok())
        .unwrap_or_else(|| panic!("{read}"))
}

#[test]
fn a_breakpoint_pauses_a_running_program_which_steps_continues_and_keeps_it_across_a_save() {
    let sandbox = Sandbox::new("mcp-break");
    let mut client = Client::start(&sandbox);
    client.request("initialize", json!({ "protocolVersion": "2025-06-18" }));
    let made = client.call("new", json!({ "path": "looper" }));
    assert_eq!(made["isError"], false, "{made}");
    let main = sandbox.work().join("looper/src/main.rs");
    let program = "// rawr\n\
         fn tick(count: u64) -> u64 {\n    count + 1\n}\n\
         \n\
         fn main() {\n    let mut count = 0;\n    loop {\n        count = tick(count);\n        std::thread::sleep(std::time::Duration::from_millis(2));\n    }\n}\n";
    std::fs::write(&main, program).unwrap();
    let started = client.call("start", json!({ "path": "looper", "wait_ms": 20000 }));
    assert_eq!(
        started["structuredContent"]["state"], "running",
        "{started}"
    );

    // A line with no statement is refused, as code. The program answers
    // once its process compiled, so wait for `main` to run first: an
    // inspect answers as soon as `count` is in scope.
    let read = client.call(
        "inspect",
        json!({ "path": "looper", "name": "count", "timeout_ms": 120000 }),
    );
    assert_eq!(read["isError"], false, "{read}");
    let refused = client.call(
        "break",
        json!({ "path": "looper", "file": "looper/src/main.rs", "line": 5, "timeout_ms": 5000 }),
    );
    assert_eq!(refused["isError"], true, "{refused}");
    assert!(
        refused["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("line 5 of"),
        "{refused}"
    );

    // Break on `count = tick(count);`: the program pauses there.
    let set = client.call(
        "break",
        json!({ "path": "looper", "file": "looper/src/main.rs", "line": 9, "timeout_ms": 5000 }),
    );
    assert_eq!(set["isError"], false, "{set}");
    assert_eq!(paused(&set), (9, "looper::main".to_owned()), "{set}");
    assert_eq!(set["structuredContent"]["paused"]["file"], json!(main));
    let text = set["content"][0]["text"].as_str().unwrap();
    assert!(
        text.contains(&format!("paused at {}:9", main.display())),
        "{text}"
    );
    let first = count(&mut client);
    // Paused, it reads the same value again.
    assert_eq!(count(&mut client), first);

    // Over the call to the next line, then on to the breakpoint again.
    let stepped = client.call("step", json!({ "path": "looper" }));
    assert_eq!(
        paused(&stepped),
        (10, "looper::main".to_owned()),
        "{stepped}"
    );
    assert_eq!(count(&mut client), first + 1);
    let continued = client.call("continue", json!({ "path": "looper", "timeout_ms": 5000 }));
    assert_eq!(
        paused(&continued),
        (9, "looper::main".to_owned()),
        "{continued}"
    );
    assert_eq!(
        continued["structuredContent"]["paused"]["reason"],
        "breakpoint"
    );

    // Into `tick`, where `count` is its parameter, and out again.
    let into = client.call("step", json!({ "path": "looper", "kind": "into" }));
    assert_eq!(paused(&into), (3, "looper::tick".to_owned()), "{into}");
    assert_eq!(count(&mut client), first + 1);
    // Its caller's `count`, at the call (#511).
    let caller = client.call(
        "inspect",
        json!({ "path": "looper", "name": "count", "frame": 1, "timeout_ms": 5000 }),
    );
    assert_eq!(
        (
            &caller["structuredContent"]["function"],
            &caller["structuredContent"]["line"],
            &caller["structuredContent"]["bindings"],
        ),
        (
            &json!("looper::main"),
            &json!(9),
            &json!([{ "name": "count", "value": (first + 1).to_string() }]),
        ),
        "{caller}"
    );
    let out = client.call("step", json!({ "path": "looper", "kind": "out" }));
    assert_eq!(paused(&out), (10, "looper::main".to_owned()), "{out}");
    assert_eq!(count(&mut client), first + 2);

    // Cleared, the program runs on and doesn't pause.
    let cleared = client.call("clear", json!({ "path": "looper" }));
    assert_eq!(cleared["structuredContent"]["cleared"], 1, "{cleared}");
    let continued = client.call("continue", json!({ "path": "looper", "timeout_ms": 300 }));
    assert_eq!(continued["isError"], false, "{continued}");
    assert!(
        continued["structuredContent"]["paused"].is_null(),
        "{continued}"
    );
    let refused = client.call("step", json!({ "path": "looper" }));
    assert_eq!(refused["isError"], true, "{refused}");
    assert!(
        refused["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("isn't paused"),
        "{refused}"
    );

    // A function's breakpoint pauses at its first statement.
    let set = client.call(
        "break",
        json!({ "path": "looper", "function": "tick", "timeout_ms": 5000 }),
    );
    assert_eq!(paused(&set), (3, "looper::tick".to_owned()), "{set}");
    let cleared = client.call("clear", json!({ "path": "looper", "function": "tick" }));
    assert_eq!(cleared["structuredContent"]["cleared"], 1, "{cleared}");
    client.call("continue", json!({ "path": "looper", "timeout_ms": 100 }));

    // A save that adds a line above restarts the program, and the
    // breakpoint follows its line's text to line 10.
    let set = client.call(
        "break",
        json!({ "path": "looper", "file": "looper/src/main.rs", "line": 9, "timeout_ms": 5000 }),
    );
    assert_eq!(paused(&set).0, 9, "{set}");
    std::fs::write(&main, program.replace("// rawr\n", "// rawr\n// counts\n")).unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let read = client.call("inspect", json!({ "path": "looper", "timeout_ms": 2000 }));
        if read["structuredContent"]["line"] == 10 {
            break;
        }
        assert!(Instant::now() < deadline, "no pause at line 10: {read}");
        std::thread::sleep(Duration::from_millis(100));
    }
    let cleared = client.call("clear", json!({ "path": "looper" }));
    assert_eq!(cleared["structuredContent"]["cleared"], 1, "{cleared}");
    assert_eq!(
        cleared["structuredContent"]["paused"]["line"], 10,
        "{cleared}"
    );
    client.call("continue", json!({ "path": "looper", "timeout_ms": 100 }));

    let stopped = client.call("stop", json!({ "path": "looper" }));
    assert_eq!(stopped["structuredContent"]["stopped"], true, "{stopped}");
    drop(client.input.take());
    assert!(client.child.wait().unwrap().success());
}

#[test]
fn input_tools_are_listed_and_reach_the_started_program_which_says_what_it_takes() {
    let sandbox = Sandbox::new("mcp-input");
    let mut client = Client::start(&sandbox);
    client.request("initialize", json!({ "protocolVersion": "2025-06-18" }));
    let listed = client.request("tools/list", json!({}));
    let tools = listed["result"]["tools"].as_array().unwrap();
    let schema = |name: &str| {
        tools
            .iter()
            .find(|tool| tool["name"] == name)
            .unwrap_or_else(|| panic!("no `{name}` in {listed}"))["inputSchema"]
            .clone()
    };
    for (name, required) in [
        ("move", json!(["x", "y"])),
        ("click", json!(["x", "y"])),
        ("drag", json!(["x", "y", "to_x", "to_y"])),
        ("scroll", json!(["x", "y"])),
        ("key", json!(["key"])),
        ("type", json!(["text"])),
    ] {
        let schema = schema(name);
        assert_eq!(schema["required"], required, "{name}: {schema}");
        for common in ["path", "title", "timeout_ms"] {
            assert!(
                schema["properties"].get(common).is_some(),
                "{name}: {schema}"
            );
        }
    }

    // Nothing started: the tool says to start it.
    let unstarted = client.call("click", json!({ "path": "looper", "x": 1, "y": 1 }));
    assert_eq!(unstarted["isError"], true, "{unstarted}");

    let made = client.call("new", json!({ "path": "looper" }));
    assert_eq!(made["isError"], false, "{made}");
    let main = sandbox.work().join("looper/src/main.rs");
    std::fs::write(
        &main,
        "// rawr\n\
         fn tick(count: u64) -> u64 {\n    count + 1\n}\n\
         \n\
         fn main() {\n    let mut count = 0;\n    loop {\n        count = tick(count);\n        std::thread::sleep(std::time::Duration::from_millis(2));\n    }\n}\n",
    )
    .unwrap();
    let started = client.call("start", json!({ "path": "looper", "wait_ms": 20000 }));
    assert_eq!(
        started["structuredContent"]["state"], "running",
        "{started}"
    );
    let read = client.call(
        "inspect",
        json!({ "path": "looper", "name": "count", "timeout_ms": 120000 }),
    );
    assert_eq!(read["isError"], false, "{read}");

    let text = |answer: &Value| {
        answer["content"][0]["text"]
            .as_str()
            .unwrap_or_default()
            .to_owned()
    };
    // The program checks what it is sent: a key it doesn't know, a click
    // with no point.
    let unknown = client.call("key", json!({ "path": "looper", "key": "Hyper" }));
    assert_eq!(unknown["isError"], true, "{unknown}");
    assert!(
        text(&unknown).starts_with("unknown key `Hyper`"),
        "{unknown}"
    );
    let pointless = client.call("click", json!({ "path": "looper", "x": 4 }));
    assert!(
        text(&pointless).starts_with("this input needs `y`"),
        "{pointless}"
    );

    // A program with no window takes no input, and says so at once.
    let windowless = client.call(
        "click",
        json!({ "path": "looper", "x": 10, "y": 10, "timeout_ms": 2000 }),
    );
    assert_eq!(windowless["isError"], true, "{windowless}");
    let expected = if cfg!(target_os = "macos") {
        "the program shows no window"
    } else {
        "sending a window input needs macOS here"
    };
    assert!(text(&windowless).starts_with(expected), "{windowless}");

    // A paused program holds its event loop: input waits for `continue`.
    let set = client.call(
        "break",
        json!({ "path": "looper", "file": "looper/src/main.rs", "line": 9, "timeout_ms": 5000 }),
    );
    assert_eq!(paused(&set).0, 9, "{set}");
    let held = client.call("type", json!({ "path": "looper", "text": "hi" }));
    assert_eq!(held["isError"], true, "{held}");
    assert!(text(&held).contains("is paused at "), "{held}");
    assert!(text(&held).contains("main.rs:9"), "{held}");
    client.call("clear", json!({ "path": "looper" }));
    client.call("continue", json!({ "path": "looper", "timeout_ms": 100 }));

    let stopped = client.call("stop", json!({ "path": "looper" }));
    assert_eq!(stopped["structuredContent"]["stopped"], true, "{stopped}");
    drop(client.input.take());
    assert!(client.child.wait().unwrap().success());
}
