//! A shell attached to `rawr run` of a program with a Rust dependency,
//! started at a terminal (#2319): the runner writes the program's output
//! to the terminal itself, and tells the run's session each piece, so the
//! shell hears what was printed before it attached in `hello` and each
//! piece from then on as an `output` notification. Build the driver
//! first, as for `plain`.

#![cfg(unix)]

use std::fs;
use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};

use crate::support::copy;

const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/inspect-dependency"
);

/// A marked `main` that prints, then hands the thread to the dependency's
/// native loop, which calls `update` back and prints `running` itself.
const PROGRAM: &str = r#"// rawr
use pump::Count;

fn update(tick: i64) -> i64 {
    if tick % 5 == 0 {
        println!("tick {}", Count::at(tick).tick);
    }
    tick
}

fn main() {
    println!("early");
    pump::run(update);
}
"#;

#[test]
fn a_shell_attached_to_a_terminal_run_with_a_dependency_hears_its_output() {
    let root = crate::support::fresh("attach-dependency-output");
    copy(Path::new(FIXTURE), &root);
    fs::write(root.join("src/main.rs"), PROGRAM).unwrap();
    let stop = root.join("stop");
    let _ = fs::remove_file(&stop);

    let size = nix::pty::Winsize {
        ws_row: 24,
        ws_col: 80,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    let pty = nix::pty::openpty(&size, None).unwrap();
    let tty = || Stdio::from(pty.slave.try_clone().unwrap());
    let mut child = crate::support::rawr_import()
        .arg("run")
        .arg(&*root)
        .env("RAWR_HOME", crate::support::runtime_home())
        .env("PUMP_STOP", &stop)
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .env_remove("RAWR_VERBOSE")
        .stdin(Stdio::null())
        .stdout(tty())
        .stderr(tty())
        .spawn()
        .unwrap();
    drop(pty.slave);
    // The terminal is read throughout, as a terminal shows what it gets.
    let shown = Arc::new(Mutex::new(String::new()));
    let reading = Arc::clone(&shown);
    let mut master = fs::File::from(pty.master);
    std::thread::spawn(move || {
        let mut buffer = [0_u8; 65536];
        while let Ok(count @ 1..) = master.read(&mut buffer) {
            reading
                .lock()
                .unwrap()
                .push_str(&String::from_utf8_lossy(&buffer[..count]));
        }
    });
    // The first run builds the runner; the loop's first call prints
    // `running`, natively, after the program's `early`.
    while !shown.lock().unwrap().contains("running") {
        assert!(
            child.try_wait().unwrap().is_none(),
            "the run exited first:\n{}",
            shown.lock().unwrap()
        );
        std::thread::sleep(Duration::from_millis(20));
    }

    let place = rawr_cli::events::session::Place::new(&root).unwrap();
    let stream = UnixStream::connect(place.socket()).unwrap();
    let mut lines = BufReader::new(stream.try_clone().unwrap());
    let request = json!({ "jsonrpc": "2.0", "id": 1, "method": "hello", "params": {} });
    writeln!(&stream, "{request}").unwrap();
    let mut next = || {
        let mut line = String::new();
        assert!(
            lines.read_line(&mut line).unwrap() > 0,
            "the session answers"
        );
        serde_json::from_str::<Value>(&line).unwrap()
    };
    let hello = loop {
        let message = next();
        if message["id"] == 1 {
            break message["result"].clone();
        }
    };
    let kept: Vec<&str> = hello["output"]
        .as_array()
        .unwrap_or_else(|| panic!("{hello}"))
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
    // A piece the program prints after the attach is told as it prints it.
    let heard = loop {
        let message = next();
        if message["method"] == "output" {
            break message;
        }
    };
    assert_eq!(heard["params"]["stream"], "stdout", "{heard}");
    let text = heard["params"]["text"].as_str().unwrap_or_default();
    assert!(text.starts_with("tick ") && text.ends_with('\n'), "{heard}");

    fs::write(&stop, "").unwrap();
    let status = child.wait().unwrap();
    assert!(status.success(), "{status}\n{}", shown.lock().unwrap());
    // The terminal got what the session was told.
    let shown = shown.lock().unwrap();
    assert!(shown.contains("early"), "{shown}");
    assert!(shown.contains(text.trim_end()), "{shown}");
}
