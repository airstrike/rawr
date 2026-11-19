//! `rawr mcp`'s `inspect` (D675, #510) on a program with a Rust dependency:
//! its runner serves the inspector socket in its run's session place
//! (D1053), whose owner names it in `RAWR_MCP_INSPECT` and has the runner
//! register its debugger at the first request (#2308), and reads the
//! bindings of a script function a native loop calls back, as iced calls
//! `update`, with a crate value printed through its own `Debug`. Build the
//! driver first, as for `plain`.

#![cfg(unix)]

use std::fs;
use std::io::{BufRead as _, BufReader, Write as _};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{json, Value};

use crate::support::{copy, start};

const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/inspect-dependency"
);

/// One request and its answer over the program's socket, as `rawr mcp`
/// asks it, once the runner serves the socket.
fn ask(socket: &Path, request: &Value) -> Value {
    let stream = loop {
        match UnixStream::connect(socket) {
            Ok(stream) => break stream,
            Err(_) => std::thread::sleep(Duration::from_millis(20)),
        }
    };
    writeln!(&stream, "{request}").unwrap();
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line).unwrap();
    serde_json::from_str(&line).unwrap()
}

/// The inspector socket of the program at `root`: its run's owner hosts
/// the run's session and names this socket to the program, in place of
/// one the run's environment names.
fn inspector(root: &Path) -> PathBuf {
    rawr_cli::events::session::Place::new(root)
        .unwrap()
        .inspector()
}

/// The pause the program is in, once it pauses: setting the breakpoint
/// `set` again answers with it, and keeps the breakpoint as it was.
fn pause(run: &mut crate::support::Run, socket: &Path, set: &Value) -> Value {
    loop {
        let answer = ask(socket, set);
        if answer["paused"].is_object() {
            return answer["paused"].clone();
        }
        assert!(
            run.child.try_wait().unwrap().is_none(),
            "the run exited first: {answer}\n{}",
            run.seen.join("\n")
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn inspect_reads_a_callbacks_bindings_in_a_runner_with_crate_values() {
    crate::support::driver();
    let root = crate::support::fresh("inspect-dependency");
    let stop = root.join("stop");
    let _ = fs::remove_file(&stop);
    copy(Path::new(FIXTURE), &root);
    let socket = inspector(&root);
    let mut run = start(&root, &[("PUMP_STOP", stop.clone())]);
    // The first run builds the runner.
    run.until(|line| line.starts_with("running"));
    let main = fs::canonicalize(root.join("src/main.rs")).unwrap();

    // The program's own type holding a crate value, inside the function
    // the native loop calls.
    let read = ask(&socket, &json!({ "name": "state", "file": main }));
    assert_eq!(read["reached"], true, "{read}");
    assert_eq!(read["file"], json!(main), "{read}");
    assert_eq!(read["function"], "inspect_dependency::update", "{read}");
    let line = read["line"].as_u64().unwrap_or_default();
    assert!((17..=19).contains(&line), "{read}");
    let value = read["bindings"][0]["value"].as_str().unwrap_or_default();
    assert!(
        value.starts_with("State { count: Count { tick: ")
            && value.ends_with(" }, label: \"ticks\" }"),
        "{read}"
    );

    // A parameter, and a crate value without `Debug`, which says so as
    // rustc does.
    let tick = ask(&socket, &json!({ "name": "tick" }));
    let read = ask(&socket, &json!({ "name": "sealed" }));
    assert!(
        tick["bindings"][0]["value"]
            .as_str()
            .is_some_and(|value| value.parse::<i64>().is_ok_and(|tick| tick > 0)),
        "{tick}"
    );
    assert_eq!(
        read["bindings"],
        json!([{ "name": "sealed", "error": "`Sealed` doesn't implement `Debug`" }]),
        "{read}"
    );

    // A breakpoint in the function the native loop calls pauses the loop
    // there (#489). Paused, the same values read again.
    let main_text = main.to_string_lossy().into_owned();
    let line_18 = json!({ "op": "break", "file": main_text, "line": 18 });
    let set = ask(&socket, &line_18);
    assert!(set.get("error").is_none(), "{set}");
    let paused = pause(&mut run, &socket, &line_18);
    assert_eq!(paused["line"], 18, "{paused}");
    assert_eq!(paused["function"], "inspect_dependency::update", "{paused}");
    let first = ask(&socket, &json!({ "name": "tick" }));
    let again = ask(&socket, &json!({ "name": "tick" }));
    assert_eq!(first["bindings"], again["bindings"], "{again}");
    // The answer lists the calls the program is in (#511).
    assert_eq!(
        first["frames"][0],
        json!({ "function": "inspect_dependency::update", "file": main, "line": 18 }),
        "{first}"
    );
    let step = ask(&socket, &json!({ "op": "step", "kind": "over" }));
    assert_eq!(step["paused"]["line"], 19, "{step}");
    // Past the function's end, the step goes on into the native loop, which
    // it can't stop in, and says so; it pauses at the loop's next call.
    let step = ask(&socket, &json!({ "op": "step", "kind": "over" }));
    assert_eq!(step, json!({ "paused": null, "native": true }), "{step}");
    assert_eq!(pause(&mut run, &socket, &line_18)["line"], 13);
    let next = ask(&socket, &json!({ "name": "tick" }));
    assert_ne!(next["bindings"], first["bindings"], "{next}");
    let continued = ask(&socket, &json!({ "op": "continue" }));
    assert!(continued["paused"].is_null(), "{continued}");
    assert_eq!(pause(&mut run, &socket, &line_18)["line"], 18);
    let cleared = ask(&socket, &json!({ "op": "clear" }));
    assert_eq!(cleared["cleared"], 1, "{cleared}");
    let continued = ask(&socket, &json!({ "op": "continue" }));
    assert!(continued["paused"].is_null(), "{continued}");

    fs::write(&stop, "").unwrap();
}

const LOOP_FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/inspect-loop");

/// The value of `count` a read through `socket` answers with.
fn count(socket: &Path) -> u64 {
    let read = ask(socket, &json!({ "name": "count" }));
    assert_eq!(read["reached"], true, "{read}");
    read["bindings"][0]["value"]
        .as_str()
        .and_then(|value| value.parse().ok())
        .unwrap_or_else(|| panic!("{read}"))
}

/// A marked `main` that loops in script code, calling its dependency and
/// never returning to native code, which its run's session starts without
/// a debugger: the first read registers the debugger with the engine that
/// runs it (#2308). The read answers in the loop, the count
/// goes on from where the program had it, and the process is the one
/// that started.
#[test]
fn a_read_registers_the_debugger_in_a_marked_main_that_never_returns_to_native_code() {
    crate::support::driver();
    let root = crate::support::fresh("inspect-loop");
    copy(Path::new(LOOP_FIXTURE), &root);
    let socket = inspector(&root);
    let mut run = start(&root, &[]);
    let pid: i32 = run
        .until(|line| line.starts_with("pid "))
        .trim_start_matches("pid ")
        .trim()
        .parse()
        .unwrap();

    let first = count(&socket);
    let second = count(&socket);
    assert!(second >= first, "the count went from {first} to {second}");
    // A restart would have ended the process that printed its pid.
    assert!(
        nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None).is_ok(),
        "the runner that started is gone: {}",
        run.seen.join("\n")
    );
    assert!(
        !run.seen
            .iter()
            .any(|line| line.starts_with("pid ") && !line.ends_with(&pid.to_string())),
        "{}",
        run.seen.join("\n")
    );
}
