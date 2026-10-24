//! A program whose `main` is native Rust and calls a marked module (D501),
//! run by the real `rawr-import` binary while its marked file is edited: a
//! body edit reaches the running program at its next call, and a struct's
//! new field restarts it with a one-line reason. Build the driver first, as
//! for `plain`.

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/islands");

fn copy(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            copy(&path, &to.join(entry.file_name()));
        } else {
            fs::copy(&path, to.join(entry.file_name())).unwrap();
        }
    }
}

/// Lines of the program's output and of rawr's, as they come.
enum Line {
    Out(String),
    Err(String),
}

struct Run {
    child: Child,
    lines: mpsc::Receiver<Line>,
    seen: Vec<String>,
}

impl Run {
    /// The next stdout line that `accept` takes, waiting up to `limit`.
    fn until(&mut self, limit: Duration, accept: impl Fn(&str) -> bool) -> String {
        let deadline = Instant::now() + limit;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.lines.recv_timeout(left) {
                Ok(Line::Out(line)) => {
                    self.seen.push(line.clone());
                    if accept(&line) {
                        return line;
                    }
                }
                Ok(Line::Err(line)) => self.seen.push(format!("stderr: {line}")),
                Err(_) => panic!("timed out; output so far:\n{}", self.seen.join("\n")),
            }
        }
    }
}

impl Drop for Run {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn start(root: &Path) -> Run {
    let directory = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let mut child = Command::new(env!("CARGO_BIN_EXE_rawr-import"))
        .arg("run")
        .arg(root)
        .env("RAWR_HOME", directory.join("rawr-home"))
        .env("ISLANDS_STOP", root.join("stop"))
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .env_remove("RAWR_VERBOSE")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let (send, lines) = mpsc::channel();
    let out = BufReader::new(child.stdout.take().unwrap());
    let err = BufReader::new(child.stderr.take().unwrap());
    let sender = send.clone();
    std::thread::spawn(move || {
        for line in out.lines().map_while(Result::ok) {
            let _ = sender.send(Line::Out(line));
        }
    });
    std::thread::spawn(move || {
        for line in err.lines().map_while(Result::ok) {
            let _ = send.send(Line::Err(line));
        }
    });
    Run {
        child,
        lines,
        seen: Vec::new(),
    }
}

/// `tick value`, from a line the fixture prints.
fn parse(line: &str) -> Option<(i64, i64)> {
    let (tick, value) = line.split_once(" value ")?;
    Some((tick.trim().parse().ok()?, value.trim().parse().ok()?))
}

fn edit(path: &Path, from: &str, to: &str) {
    let text = fs::read_to_string(path).unwrap();
    assert!(text.contains(from), "{text}");
    fs::write(path, text.replace(from, to)).unwrap();
}

#[test]
fn a_native_main_sees_a_body_edit_at_its_next_call_and_restarts_on_a_new_field() {
    let driver = Path::new(env!("CARGO_BIN_EXE_rawr-import")).with_file_name(format!(
        "rawr-import-driver{}",
        std::env::consts::EXE_SUFFIX
    ));
    assert!(
        driver.is_file(),
        "build {} first: cargo build -p rawr-import --features rustc-driver --bin rawr-import-driver",
        driver.display()
    );
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("islands");
    let _ = fs::remove_file(root.join("stop"));
    let _ = fs::remove_dir_all(root.join("src"));
    copy(Path::new(FIXTURE), &root);
    let physics = root.join("src/physics.rs");
    let mut run = start(&root);

    // The first run builds the runner.
    let first = run.until(Duration::from_secs(900), |line| {
        parse(line).is_some_and(|(tick, _)| tick >= 3)
    });
    assert_eq!(parse(&first), Some((3, 3)));

    // A body edit: the running native loop keeps its count, and its next
    // calls run the new code.
    edit(&physics, "const SCALE: i64 = 1;", "const SCALE: i64 = 100;");
    let swapped = run.until(Duration::from_secs(120), |line| {
        parse(line).is_some_and(|(tick, value)| value == tick * 100)
    });
    let (tick, _) = parse(&swapped).unwrap();
    assert!(tick > 3, "{}", run.seen.join("\n"));
    assert!(
        !run.seen.iter().any(|line| line.contains("restarting")),
        "{}",
        run.seen.join("\n")
    );

    // A new field changes what native code sees: a new native build, and
    // the program starts again from its first tick.
    edit(
        &physics,
        "    pub ticks: i64,\n}",
        "    pub ticks: i64,\n    pub label: String,\n}",
    );
    edit(
        &physics,
        "Counter { ticks: 0 }",
        "Counter { ticks: 0, label: String::new() }",
    );
    let restarted = run.until(Duration::from_secs(600), |line| {
        parse(line) == Some((1, 100))
    });
    assert_eq!(restarted, "1 value 100");
    assert!(
        run.seen.iter().any(|line| line
            == "stderr: rawr: `physics::Counter` changed its fields; restarting the program"),
        "{}",
        run.seen.join("\n")
    );

    fs::write(root.join("stop"), "").unwrap();
    let status = run.child.wait().unwrap();
    assert!(status.success(), "{}", run.seen.join("\n"));
}
