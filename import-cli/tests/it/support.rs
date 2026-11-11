//! What the end-to-end tests share: fixtures prepared in lanes where Cargo
//! keeps test files ([`fixture`]), one `RAWR_HOME` for the suite, and
//! `rawr-import run` with its output read as it comes. The suite runs its
//! tests in parallel; a test that needs a project or a home no other test
//! touches makes its own with [`fresh`], by a name no other test uses.

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

pub mod alone;
mod events;
pub mod fixture;
pub mod runtime;

pub use alone::alone;
pub use fixture::Fixture;

use events::{Events, News};

/// How often a wait looks at the run's events and whether it exited. A
/// look that finds neither looks again: no wait has a deadline.
const LOOK: Duration = Duration::from_millis(50);

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
    /// The `RAWR_MCP_EVENTS` file the run was given, if any.
    events: Option<Events>,
}

impl Run {
    /// The next stdout line that `accept` takes. A run that says `failed`
    /// in its events fails the test at once, with the failure, what it
    /// printed and its events; one that exits, whose program ends, or that
    /// ends its output, fails it once its output has given what it printed.
    pub fn until(&mut self, accept: impl Fn(&str) -> bool) -> String {
        let mut looked = Instant::now();
        let mut gone = None;
        loop {
            match self.lines.recv_timeout(LOOK) {
                Ok(Line::Out(line)) => {
                    self.seen.push(line.clone());
                    if accept(&line) {
                        return line;
                    }
                }
                Ok(Line::Err(line)) => self.seen.push(format!("stderr: {line}")),
                Err(RecvTimeoutError::Disconnected) => self.fail("the run ended without it"),
                // The run or its program is gone and its pipes are quiet:
                // they gave what it printed, and a process it left may hold
                // them open.
                Err(RecvTimeoutError::Timeout) => {
                    if let Some(why) = &gone {
                        self.fail(&format!("{why} without it"));
                    }
                }
            }
            if looked.elapsed() < LOOK {
                continue;
            }
            looked = Instant::now();
            if gone.is_none() {
                gone = self.gone();
            }
        }
    }

    /// The text of the `RAWR_MCP_EVENTS` file `events` once `accept` takes
    /// it. A run that says `failed` first, exits first, or whose program
    /// ends first fails the test.
    pub fn told(&mut self, events: &Path, accept: impl Fn(&str) -> bool) -> String {
        loop {
            let text = fs::read_to_string(events).unwrap_or_default();
            if accept(&text) {
                if let Some(heard) = self.events.as_mut() {
                    heard.hear(&text);
                }
                return text;
            }
            if let Some(why) = self.gone() {
                self.fail(&format!("{why} first"));
            }
            std::thread::sleep(LOOK);
        }
    }

    /// Why the run can print no more since the last look: it exited, or it
    /// said its program ended. A run that said `failed` fails the test.
    fn gone(&mut self) -> Option<String> {
        let ended = match self.events.as_mut().and_then(Events::news) {
            Some(News::Failed(failure)) => self.fail(&format!("the run failed:\n{failure}")),
            Some(News::Ended(ended)) => Some(format!("the program ended ({ended})")),
            None => None,
        };
        match self.child.try_wait().unwrap() {
            Some(status) => Some(format!("the run exited with {status}")),
            None => ended,
        }
    }

    /// Fail the test for `why`, with what the run printed and its events.
    fn fail(&self, why: &str) -> ! {
        let events = self.events.as_ref().map_or_else(String::new, Events::text);
        panic!(
            "{why}\nits output:\n{}\nits events:\n{events}",
            self.seen.join("\n")
        )
    }

    /// Wait for the run to end, and keep the output it printed since the
    /// last [`Run::until`], to the end of its pipes.
    pub fn wait(&mut self) -> std::process::ExitStatus {
        let status = self.child.wait().unwrap();
        while let Ok(line) = self.lines.recv() {
            self.seen.push(match line {
                Line::Out(line) => line,
                Line::Err(line) => format!("stderr: {line}"),
            });
        }
        status
    }
}

/// A run ends with its test, which passed or panicked, as a stopped `rawr
/// run` ends: told SIGTERM, it retires its runner with the program and
/// the builds it started. A test that dies itself ends the run too, which
/// watches its parent.
impl Drop for Run {
    fn drop(&mut self) {
        // A reaped run's pid may be another process's by now.
        if let Ok(None) = self.child.try_wait() {
            #[cfg(unix)]
            {
                use nix::sys::signal::{kill, Signal};
                let pid = i32::try_from(self.child.id()).expect("a pid is an i32");
                let _ = kill(nix::unistd::Pid::from_raw(pid), Signal::SIGTERM);
            }
            #[cfg(not(unix))]
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
    }
}

/// Cargo's report in the stderr of a run or check that `cargo check`
/// failed too: the whole report, with no error of rawr's own before it.
pub fn cargo_s_report(stderr: &str) -> &str {
    let Some((own, cargo)) = stderr.split_once("cargo check found") else {
        panic!("cargo check found no error:\n{stderr}");
    };
    assert!(
        !own.contains("error"),
        "rawr reported its own error:\n{stderr}"
    );
    cargo
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

/// A directory of one test's own under [`scratch`], by a name no other test
/// uses: a project or a home. It starts empty, whatever an earlier run left
/// there, so no test reuses a build of older sources (#1712). It goes when
/// the test passes and stays when the test fails, to be looked at (#1731).
pub struct Scratch {
    path: PathBuf,
}

/// The empty directory `name` under [`scratch`], removed when the test
/// passes.
pub fn fresh(name: impl AsRef<Path>) -> Scratch {
    let path = scratch().join(name);
    match fs::remove_dir_all(&path) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
            panic!("{} did not go: {error}", path.display())
        }
        _ => {}
    }
    fs::create_dir_all(&path).unwrap();
    Scratch { path }
}

impl std::ops::Deref for Scratch {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.path
    }
}

impl AsRef<Path> for Scratch {
    fn as_ref(&self) -> &Path {
        &self.path
    }
}

impl AsRef<std::ffi::OsStr> for Scratch {
    fn as_ref(&self) -> &std::ffi::OsStr {
        self.path.as_os_str()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
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
        .env("RAWR_HOME", runtime_home())
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .env_remove("RAWR_VERBOSE");
    // A test's own variables stand, `RAWR_VERBOSE` among them.
    for (name, value) in variables {
        child.env(name, value);
    }
    let mut child = child
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
    let events = variables
        .iter()
        .find(|(name, _)| *name == "RAWR_MCP_EVENTS")
        .map(|(_, path)| Events::new(path.clone()));
    Run {
        child,
        lines,
        seen: Vec::new(),
        events,
    }
}
