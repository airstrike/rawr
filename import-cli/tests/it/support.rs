//! What the end-to-end tests share: fixtures prepared in lanes where Cargo
//! keeps test files ([`fixture`]), one `RAWR_HOME` for the suite, and
//! `rawr-import run` with its output read as it comes. The suite runs its
//! tests in parallel; a test that needs a project or a home no other test
//! touches makes its own under [`scratch`] by a name no other test uses.

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

pub mod alone;
pub mod fixture;
pub mod runtime;

pub use alone::alone;
pub use fixture::Fixture;

pub fn copy(from: &Path, to: &Path) {
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
pub enum Line {
    Out(String),
    Err(String),
}

pub struct Run {
    pub child: Child,
    lines: mpsc::Receiver<Line>,
    pub seen: Vec<String>,
}

impl Run {
    /// The next stdout line that `accept` takes, waiting up to `limit`.
    pub fn until(&mut self, limit: Duration, accept: impl Fn(&str) -> bool) -> String {
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

    /// Wait for the run to end, and keep the output it printed since the
    /// last [`Run::until`].
    pub fn wait(&mut self) -> std::process::ExitStatus {
        let status = self.child.wait().unwrap();
        // A process the run left behind may hold its pipes open.
        let deadline = Instant::now() + Duration::from_secs(5);
        while let Ok(line) = self
            .lines
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        {
            self.seen.push(match line {
                Line::Out(line) => line,
                Line::Err(line) => format!("stderr: {line}"),
            });
        }
        status
    }
}

impl Drop for Run {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The directory a test's projects and `RAWR_HOME` live in: `RAWR_TEST_TMPDIR`
/// when set, so concurrent runs from different checkouts keep apart, and
/// cargo's target tmp directory otherwise. A test that asks for it runs
/// beside others from then on, unless it runs [`alone`].
pub fn scratch() -> PathBuf {
    alone::share();
    std::env::var_os("RAWR_TEST_TMPDIR")
        .map_or_else(|| PathBuf::from(env!("CARGO_TARGET_TMPDIR")), PathBuf::from)
}

/// The suite's one `RAWR_RUNTIME_HOME`, the shared `RAWR_HOME`, with
/// rawr's runtime already compiled in it ([`runtime::home`]): a test with a
/// home of its own still starts cold, but for rawr's host sources and
/// compiled runtime, which no such test is about and which would otherwise
/// compile again for each such home.
pub fn runtime_home() -> PathBuf {
    runtime::home()
}

/// `rawr-import run root`, with `variables` in its environment.
pub fn start(root: &Path, variables: &[(&str, PathBuf)]) -> Run {
    start_with(root, variables, &[], Stdio::null())
}

/// `rawr-import run root -- arguments`, with `variables` in its
/// environment and `stdin` as its stdin.
pub fn start_with(
    root: &Path,
    variables: &[(&str, PathBuf)],
    arguments: &[&str],
    stdin: Stdio,
) -> Run {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rawr-import"));
    child
        .arg("run")
        .arg(root)
        .arg("--")
        .args(arguments)
        .stdin(stdin)
        .env("RAWR_HOME", runtime_home());
    for (name, value) in variables {
        child.env(name, value);
    }
    let mut child = child
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
