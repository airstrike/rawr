//! `rawr mcp`'s `inspect` (D675, #510) on a program with a Rust dependency:
//! its runner serves the socket `RAWR_MCP_INSPECT` names and reads the
//! bindings of a script function a native loop calls back, as iced calls
//! `update`, with a crate value printed through its own `Debug`. Build the
//! driver first, as for `plain`.

#![cfg(unix)]

use std::fs;
use std::io::{BufRead as _, BufReader, Write as _};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::support::{copy, start};

const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/inspect-dependency"
);

/// One request and its answer over the program's socket, as `rawr mcp`
/// asks it.
fn ask(socket: &Path, request: &Value) -> Value {
    let deadline = Instant::now() + Duration::from_secs(10);
    let stream = loop {
        match UnixStream::connect(socket) {
            Ok(stream) => break stream,
            Err(error) if Instant::now() >= deadline => panic!("{}: {error}", socket.display()),
            Err(_) => std::thread::sleep(Duration::from_millis(20)),
        }
    };
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    writeln!(&stream, "{request}").unwrap();
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line).unwrap();
    serde_json::from_str(&line).unwrap()
}

#[test]
fn inspect_reads_a_callbacks_bindings_in_a_runner_with_crate_values() {
    let driver = Path::new(env!("CARGO_BIN_EXE_rawr-import")).with_file_name(format!(
        "rawr-import-driver{}",
        std::env::consts::EXE_SUFFIX
    ));
    assert!(
        driver.is_file(),
        "build {} first: cargo build -p rawr-import --features rustc-driver --bin rawr-import-driver",
        driver.display()
    );
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("inspect-dependency");
    let stop = root.join("stop");
    let _ = fs::remove_file(&stop);
    copy(Path::new(FIXTURE), &root);
    // A Unix socket's path is short: under the system's temp directory, as
    // `rawr mcp` makes it.
    let directory = std::env::temp_dir().join(format!("rawr-inspect-{}", std::process::id()));
    fs::create_dir_all(&directory).unwrap();
    let socket = directory.join("s");
    let mut run = start(
        &root,
        &[
            ("RAWR_MCP_INSPECT", socket.clone()),
            ("PUMP_STOP", stop.clone()),
        ],
    );
    // The first run builds the runner.
    run.until(Duration::from_secs(900), |line| line.starts_with("running"));
    let main = fs::canonicalize(root.join("src/main.rs")).unwrap();

    // The program's own type holding a crate value, inside the function
    // the native loop calls.
    let read = ask(
        &socket,
        &json!({ "name": "state", "file": main, "timeout_ms": 5000 }),
    );
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
    let tick = ask(&socket, &json!({ "name": "tick", "timeout_ms": 5000 }));
    let read = ask(&socket, &json!({ "name": "sealed", "timeout_ms": 5000 }));
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

    fs::write(&stop, "").unwrap();
    let _ = fs::remove_dir_all(&directory);
}
