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
            "echo",
            "hang"
        ]
    );
    let echo = &tools[8];
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
