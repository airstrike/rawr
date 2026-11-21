//! `rawr dap` end to end: a scripted Debug Adapter Protocol client launches
//! a plain crate, stops it at a breakpoint, reads its frames, scopes and
//! values down to their parts, evaluates, steps, continues, pauses and
//! ends it. Every message the adapter writes is checked as the protocol
//! frames and shapes it.

use std::io::{BufReader, Write as _};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;

use serde_json::{json, Value};

use super::Sandbox;

/// The program: `main` calls `tick` in a loop; line 17 is the call.
const PROGRAM: &str = "// rawr
#[derive(Debug)]
struct Counter {
    count: u64,
    label: String,
    last: (u64, Option<u64>),
}

fn tick(counter: &mut Counter, step: u64) {
    counter.last = (counter.count, Some(step));
    counter.count += step;
}

fn main() {
    let mut counter = Counter { count: 0, label: String::from(\"ticks\"), last: (0, None) };
    loop {
        tick(&mut counter, 1);
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}
";

struct Client {
    child: Child,
    input: Option<ChildStdin>,
    messages: mpsc::Receiver<Value>,
    next: u64,
    /// Events heard while waiting for something else.
    heard: Vec<Value>,
}

/// One framed message from `reader`, checked as the protocol has it.
fn read(reader: &mut impl std::io::BufRead) -> Option<Value> {
    let mut length = None;
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header).ok()? == 0 {
            return None;
        }
        assert!(
            header.ends_with("\r\n"),
            "a header ends with CRLF: {header:?}"
        );
        let header = header.trim_end();
        if header.is_empty() {
            break;
        }
        let (name, value) = header.split_once(": ").expect("a header is `Name: value`");
        assert_eq!(name, "Content-Length", "{header}");
        length = value.parse::<usize>().ok();
    }
    let mut body = vec![0; length.expect("a message says its length")];
    reader.read_exact(&mut body).ok()?;
    let message: Value = serde_json::from_slice(&body).expect("a body is JSON");
    assert!(
        message["seq"].as_u64().is_some_and(|seq| seq > 0),
        "{message}"
    );
    match message["type"].as_str() {
        Some("response") => {
            assert!(message["request_seq"].is_u64(), "{message}");
            assert!(message["command"].is_string(), "{message}");
            assert!(message["success"].is_boolean(), "{message}");
            if message["success"] == false {
                assert!(message["message"].is_string(), "{message}");
            }
        }
        Some("event") => {
            assert!(message["event"].is_string(), "{message}");
            assert!(message["body"].is_object(), "{message}");
        }
        _ => panic!("an adapter writes responses and events: {message}"),
    }
    Some(message)
}

impl Client {
    fn start(sandbox: &Sandbox) -> Self {
        let missing = sandbox.path.join("missing");
        let mut child = Command::new(sandbox.path.join("bin/rawr"))
            .arg("dap")
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
            let mut reader = BufReader::new(stdout);
            let mut last = 0;
            while let Some(message) = read(&mut reader) {
                let seq = message["seq"].as_u64().unwrap();
                assert!(seq > last, "each message is numbered on: {message}");
                last = seq;
                if sender.send(message).is_err() {
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
            heard: Vec::new(),
        }
    }

    /// Send a request; its seq.
    fn ask(&mut self, command: &str, arguments: Value) -> u64 {
        self.next += 1;
        let body =
            json!({ "seq": self.next, "type": "request", "command": command, "arguments": arguments })
                .to_string();
        let input = self.input.as_mut().unwrap();
        write!(input, "Content-Length: {}\r\n\r\n{body}", body.len()).unwrap();
        input.flush().unwrap();
        self.next
    }

    fn receive(&self) -> Value {
        self.messages
            .recv()
            .expect("the adapter answers before it exits")
    }

    /// The response to request `seq`, keeping the events before it.
    fn response(&mut self, seq: u64, command: &str) -> Value {
        loop {
            let message = self.receive();
            if message["type"] == "response" && message["request_seq"] == seq {
                assert_eq!(message["command"], command, "{message}");
                return message;
            }
            self.heard.push(message);
        }
    }

    /// The body of a successful response to `command`.
    fn request(&mut self, command: &str, arguments: Value) -> Value {
        let seq = self.ask(command, arguments);
        let response = self.response(seq, command);
        assert_eq!(response["success"], true, "{response}");
        response["body"].clone()
    }

    /// The next event `name`, heard already or to come.
    fn event(&mut self, name: &str) -> Value {
        if let Some(index) = self.heard.iter().position(|event| event["event"] == name) {
            return self.heard.remove(index)["body"].clone();
        }
        loop {
            let message = self.receive();
            if message["event"] == name {
                return message["body"].clone();
            }
            self.heard.push(message);
        }
    }

    /// The top frame's name and line once the program stopped for
    /// `reason`.
    fn stopped(&mut self, reason: &str) -> (String, u64) {
        let stopped = self.event("stopped");
        assert_eq!(stopped["reason"], reason, "{stopped}");
        assert_eq!(stopped["threadId"], 1, "{stopped}");
        let trace = self.request("stackTrace", json!({ "threadId": 1 }));
        let top = &trace["stackFrames"][0];
        (
            top["name"].as_str().unwrap_or_default().to_owned(),
            top["line"].as_u64().unwrap_or_default(),
        )
    }

    /// The variables behind `reference`, by name: each value and its own
    /// reference.
    fn variables(&mut self, reference: &Value) -> Vec<(String, String, u64)> {
        let body = self.request("variables", json!({ "variablesReference": reference }));
        body["variables"]
            .as_array()
            .unwrap()
            .iter()
            .map(|variable| {
                (
                    variable["name"].as_str().unwrap().to_owned(),
                    variable["value"].as_str().unwrap().to_owned(),
                    variable["variablesReference"].as_u64().unwrap(),
                )
            })
            .collect()
    }
}

fn find<'v>(variables: &'v [(String, String, u64)], name: &str) -> &'v (String, String, u64) {
    variables
        .iter()
        .find(|(found, ..)| found == name)
        .unwrap_or_else(|| panic!("no `{name}` in {variables:?}"))
}

#[test]
fn a_dap_client_launches_stops_reads_values_by_reference_evaluates_steps_and_ends_a_program() {
    let sandbox = Sandbox::new("dap");
    let made = sandbox.rawr(&["new", "looper"]);
    super::assert_success(&made);
    let main = sandbox.work().join("looper/src/main.rs");
    std::fs::write(&main, PROGRAM).unwrap();
    let mut client = Client::start(&sandbox);

    let capabilities = client.request(
        "initialize",
        json!({ "adapterID": "rawr", "linesStartAt1": true }),
    );
    assert_eq!(
        capabilities["supportsConfigurationDoneRequest"], true,
        "{capabilities}"
    );
    client.request("launch", json!({ "program": "looper" }));
    client.event("initialized");
    let set = client.request(
        "setBreakpoints",
        json!({ "source": { "path": main }, "breakpoints": [{ "line": 17 }] }),
    );
    assert_eq!(
        set["breakpoints"],
        json!([{ "id": 1, "verified": true, "line": 17, "source": { "path": main } }]),
        "{set}"
    );
    client.request("configurationDone", json!({}));

    assert_eq!(
        client.stopped("breakpoint"),
        ("looper::main".to_owned(), 17)
    );
    let threads = client.request("threads", json!({}));
    assert_eq!(threads["threads"], json!([{ "id": 1, "name": "main" }]));
    let trace = client.request("stackTrace", json!({ "threadId": 1 }));
    assert_eq!(trace["totalFrames"], 1, "{trace}");
    assert_eq!(
        trace["stackFrames"][0]["source"]["path"],
        json!(main),
        "{trace}"
    );
    // Once more round the loop, so `tick` has run.
    client.request("continue", json!({ "threadId": 1 }));
    client.event("continued");
    assert_eq!(
        client.stopped("breakpoint"),
        ("looper::main".to_owned(), 17)
    );

    // The frame's bindings, then a value's parts by reference, down to a
    // leaf.
    let scopes = client.request("scopes", json!({ "frameId": 1 }));
    let locals = &scopes["scopes"][0]["variablesReference"];
    let bindings = client.variables(locals);
    let (_, text, counter) = find(&bindings, "counter").clone();
    assert!(text.starts_with("Counter { count: "), "{text}");
    assert!(counter > 0, "a struct has parts: {bindings:?}");
    let fields = client.variables(&json!(counter));
    let count: u64 = find(&fields, "count").1.parse().unwrap();
    assert_eq!(
        find(&fields, "label"),
        &("label".to_owned(), "\"ticks\"".to_owned(), 0)
    );
    let (_, last, pair) = find(&fields, "last").clone();
    assert_eq!(last, format!("({}, Some(1))", count - 1), "{fields:?}");
    let pair = client.variables(&json!(pair));
    let (_, some, inner) = find(&pair, "1").clone();
    assert_eq!(some, "Some(1)");
    assert_eq!(
        client.variables(&json!(inner)),
        [("0".to_owned(), "1".to_owned(), 0)]
    );

    // Evaluate where it is paused.
    let evaluated = client.request(
        "evaluate",
        json!({ "expression": "counter.count * 2", "frameId": 1, "context": "repl" }),
    );
    assert_eq!(evaluated["result"], (count * 2).to_string(), "{evaluated}");
    assert_eq!(evaluated["variablesReference"], 0, "{evaluated}");
    // A console entry may have written a place, so the client reads again.
    assert_eq!(client.event("invalidated")["areas"], json!(["variables"]));

    // Over the call, then back to the breakpoint, into `tick` and out.
    client.request("next", json!({ "threadId": 1 }));
    assert_eq!(client.stopped("step"), ("looper::main".to_owned(), 18));
    client.request("continue", json!({ "threadId": 1 }));
    client.event("continued");
    assert_eq!(
        client.stopped("breakpoint"),
        ("looper::main".to_owned(), 17)
    );
    client.request("stepIn", json!({ "threadId": 1 }));
    assert_eq!(client.stopped("step").0, "looper::tick");
    let trace = client.request("stackTrace", json!({ "threadId": 1 }));
    assert_eq!(trace["stackFrames"][1]["name"], "looper::main", "{trace}");
    assert_eq!(trace["stackFrames"][1]["line"], 17, "{trace}");
    client.request("stepOut", json!({ "threadId": 1 }));
    assert_eq!(client.stopped("step"), ("looper::main".to_owned(), 18));

    // Without breakpoints it runs on until a pause.
    let cleared = client.request(
        "setBreakpoints",
        json!({ "source": { "path": main }, "breakpoints": [] }),
    );
    assert_eq!(cleared["breakpoints"], json!([]));
    client.request("continue", json!({ "threadId": 1 }));
    client.request("pause", json!({ "threadId": 1 }));
    let (function, _) = client.stopped("pause");
    assert!(function.starts_with("looper::"), "{function}");

    // A stepping request on a running program is refused.
    client.request("continue", json!({ "threadId": 1 }));
    client.event("continued");
    let seq = client.ask("next", json!({ "threadId": 1 }));
    let refused = client.response(seq, "next");
    assert_eq!(refused["success"], false, "{refused}");

    client.request("terminate", json!({}));
    client.event("terminated");
    client.request("disconnect", json!({}));
    drop(client.input.take());
    assert!(client.child.wait().unwrap().success());
}

/// The program a client changes values in: `main` holds a struct, an
/// immutable binding and a map; line 15 adds the step.
const CHANGED: &str = "// rawr
use std::collections::HashMap;

#[derive(Debug)]
struct Counter {
    count: u64,
    label: String,
}

fn main() {
    let mut counter = Counter { count: 0, label: String::from(\"ticks\") };
    let step = 1;
    let mut seen = HashMap::from([(String::from(\"loops\"), 0_u64)]);
    loop {
        counter.count += step;
        *seen.get_mut(\"loops\").unwrap() += 1;
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}
";

/// The response to `setVariable` for the variable `name` behind
/// `reference`.
fn set(client: &mut Client, reference: &Value, name: &str, value: &str) -> Value {
    let seq = client.ask(
        "setVariable",
        json!({ "variablesReference": reference, "name": name, "value": value }),
    );
    client.response(seq, "setVariable")
}

#[test]
fn a_dap_client_sets_a_field_a_map_entry_and_an_expression_and_is_refused_an_immutable_binding() {
    let sandbox = Sandbox::new("dap-set");
    let made = sandbox.rawr(&["new", "setter"]);
    super::assert_success(&made);
    let main = sandbox.work().join("setter/src/main.rs");
    std::fs::write(&main, CHANGED).unwrap();
    let mut client = Client::start(&sandbox);

    let capabilities = client.request(
        "initialize",
        json!({ "adapterID": "rawr", "linesStartAt1": true }),
    );
    assert_eq!(capabilities["supportsSetVariable"], true, "{capabilities}");
    assert_eq!(
        capabilities["supportsSetExpression"], true,
        "{capabilities}"
    );
    client.request("launch", json!({ "program": "setter" }));
    client.event("initialized");
    client.request(
        "setBreakpoints",
        json!({ "source": { "path": main }, "breakpoints": [{ "line": 15 }] }),
    );
    client.request("configurationDone", json!({}));
    assert_eq!(
        client.stopped("breakpoint"),
        ("setter::main".to_owned(), 15)
    );

    let scopes = client.request("scopes", json!({ "frameId": 1 }));
    let locals = scopes["scopes"][0]["variablesReference"].clone();
    let bindings = client.variables(&locals);
    let counter = find(&bindings, "counter").2;

    // A field, through its struct's reference.
    let field = set(&mut client, &json!(counter), "count", "40");
    assert_eq!(field["success"], true, "{field}");
    assert_eq!(field["body"]["value"], "40", "{field}");
    let bindings = client.variables(&locals);
    let text = &find(&bindings, "counter").1;
    assert!(text.starts_with("Counter { count: 40,"), "{text}");

    // An immutable binding: rustc's error.
    let refused = set(&mut client, &locals, "step", "5");
    assert_eq!(refused["success"], false, "{refused}");
    let message = refused["message"].as_str().unwrap();
    assert!(message.contains("immutable"), "{message}");

    // A map's entry, by its key.
    let seen = find(&bindings, "seen").2;
    let entries = client.variables(&json!(seen));
    assert_eq!(find(&entries, "\"loops\"").1, "0", "{entries:?}");
    let entry = set(&mut client, &json!(seen), "\"loops\"", "7");
    assert_eq!(entry["success"], true, "{entry}");
    assert_eq!(entry["body"]["value"], "7", "{entry}");

    // An expression the client watches.
    let label = client.request(
        "setExpression",
        json!({ "expression": "counter.label", "value": "String::from(\"set\")", "frameId": 1 }),
    );
    assert_eq!(label["value"], "\"set\"", "{label}");

    // The program goes on with the values set.
    client.request("continue", json!({ "threadId": 1 }));
    client.event("continued");
    assert_eq!(
        client.stopped("breakpoint"),
        ("setter::main".to_owned(), 15)
    );
    let evaluated = client.request(
        "evaluate",
        json!({ "expression": "(counter.count, seen[\"loops\"], &counter.label)", "frameId": 1 }),
    );
    assert_eq!(evaluated["result"], "(41, 8, \"set\")", "{evaluated}");

    client.request("terminate", json!({}));
    client.event("terminated");
    client.request("disconnect", json!({}));
    drop(client.input.take());
    assert!(client.child.wait().unwrap().success());
}

/// The program writes a line to each stream and fails.
const SAYS: &str = "// rawr
fn main() {
    println!(\"to stdout\");
    eprintln!(\"to stderr\");
    std::process::exit(3);
}
";

/// Rawr's own lines reach the client as `console` output, and the
/// program's streams as `stdout` and `stderr`, though rawr's lines and the
/// program's stderr both leave `rawr run` (#2367).
#[test]
fn a_dap_client_hears_rawr_s_lines_as_console_and_the_program_s_stderr_as_stderr() {
    let sandbox = Sandbox::new("dap-streams");
    let made = sandbox.rawr(&["new", "sayer"]);
    super::assert_success(&made);
    std::fs::write(sandbox.work().join("sayer/src/main.rs"), SAYS).unwrap();
    let mut client = Client::start(&sandbox);
    client.request("initialize", json!({ "adapterID": "rawr" }));
    client.request("launch", json!({ "program": "sayer" }));
    client.event("initialized");
    client.request("configurationDone", json!({}));
    // A program that failed under a shell waits for a save, which rawr
    // says on its own stderr.
    let lines = ["to stdout", "to stderr", "a save runs it again"];
    let mut heard = Vec::new();
    while !lines.iter().all(|line| {
        heard
            .iter()
            .any(|(_, output): &(String, String)| output.contains(line))
    }) {
        let output = client.event("output");
        heard.push((
            output["category"].as_str().unwrap().to_owned(),
            output["output"].as_str().unwrap().to_owned(),
        ));
    }
    let category = |line: &str| {
        heard
            .iter()
            .find(|(_, output)| output.contains(line))
            .map(|(category, _)| category.as_str())
    };
    assert_eq!(category("to stdout"), Some("stdout"), "{heard:?}");
    assert_eq!(category("to stderr"), Some("stderr"), "{heard:?}");
    assert_eq!(
        category("a save runs it again"),
        Some("console"),
        "{heard:?}"
    );
    client.request("terminate", json!({}));
    client.event("terminated");
    client.request("disconnect", json!({}));
    drop(client.input.take());
    let _ = client.child.wait();
}
