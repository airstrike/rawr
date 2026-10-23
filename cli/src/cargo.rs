//! `cargo check` beside Rawr, for a Cargo crate of marked `.rs` files.
//!
//! Rawr runs a subset of Rust and accepts some programs rustc rejects.
//! rustc stays the authority: `rawr check` runs `cargo check` too and
//! reports its errors after Rawr's, and `rawr run` keeps a `cargo check`
//! going in the background (see [`Background`]) without holding up a
//! reload. Cargo runs with the crate's own manifest and target directory.
//!
//! The command is `$CARGO` when set (Cargo sets it for `cargo rawr`), else
//! `cargo` on `PATH`. Without it Rawr says so once and goes on.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

/// How long `rawr run` waits after a save before it starts `cargo check`.
/// Another save in that time restarts the wait.
pub const DEBOUNCE: Duration = Duration::from_millis(300);

const POLL: Duration = Duration::from_millis(25);

/// The `Cargo.toml` of the crate `graph` loaded, when it is a Cargo crate
/// of marked `.rs` files.
#[must_use]
pub fn manifest(graph: &rawr::package::Graph) -> Option<PathBuf> {
    let package = graph.entry_package();
    let manifest = package.root().join("Cargo.toml");
    manifest.is_file().then_some(manifest)
}

/// The cargo command Rawr runs.
#[must_use]
pub fn program() -> OsString {
    std::env::var_os("CARGO")
        .filter(|cargo| !cargo.is_empty())
        .unwrap_or_else(|| "cargo".into())
}

/// What one `cargo check` said.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Outcome {
    /// Each error rustc reported, rendered as rustc renders it.
    pub errors: Vec<String>,
    /// How many warnings rustc reported.
    pub warnings: usize,
    /// Cargo's own failure, when it failed without a rustc error (a bad
    /// manifest or a dependency it cannot fetch).
    pub failure: Option<String>,
}

impl Outcome {
    /// Whether cargo found nothing wrong.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.errors.is_empty() && self.failure.is_none()
    }

    /// One line saying what cargo found.
    #[must_use]
    pub fn heading(&self) -> String {
        match self.errors.len() {
            0 if self.failure.is_some() => "cargo check failed".to_owned(),
            0 => "cargo check passed".to_owned(),
            1 => "cargo check found 1 error".to_owned(),
            count => format!("cargo check found {count} errors"),
        }
    }

    /// The errors as rustc rendered them, or cargo's own failure.
    #[must_use]
    pub fn details(&self) -> String {
        if self.errors.is_empty() {
            return self.failure.clone().unwrap_or_default();
        }
        self.errors
            .iter()
            .map(|error| error.trim_end())
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    /// The heading and the details, for a terminal.
    #[must_use]
    pub fn render(&self) -> String {
        let details = self.details();
        if details.is_empty() {
            format!("{}\n", self.heading())
        } else {
            format!("{}:\n\n{details}\n", self.heading())
        }
    }

    /// Read cargo's `--message-format=json` output and its stderr.
    #[must_use]
    pub fn read(stdout: &str, stderr: &str, success: bool) -> Self {
        let mut outcome = Self::default();
        for line in stdout.lines() {
            let Ok(message) = serde_json::from_str::<serde_json::Value>(line) else {
                continue;
            };
            if message["reason"] != "compiler-message" {
                continue;
            }
            let message = &message["message"];
            let text = message["message"].as_str().unwrap_or_default();
            match message["level"].as_str() {
                Some("error" | "error: internal compiler error") => {
                    // rustc's closing summary repeats what the errors said.
                    if text.starts_with("aborting due to") {
                        continue;
                    }
                    let rendered = message["rendered"].as_str().unwrap_or(text);
                    outcome.errors.push(rendered.to_owned());
                }
                Some("warning")
                    if !text.contains("warning emitted") && !text.contains("warnings emitted") =>
                {
                    outcome.warnings += 1;
                }
                _ => {}
            }
        }
        if !success && outcome.errors.is_empty() {
            outcome.failure = Some(cargo_failure(stderr));
        }
        outcome
    }
}

/// Cargo's stderr from its first error on, without its progress lines.
fn cargo_failure(stderr: &str) -> String {
    let lines: Vec<&str> = stderr.lines().collect();
    let first = lines
        .iter()
        .position(|line| line.starts_with("error"))
        .unwrap_or(0);
    let text = lines[first..].join("\n");
    let text = text.trim();
    if text.is_empty() {
        "cargo check exited with an error and said nothing".to_owned()
    } else {
        text.to_owned()
    }
}

/// What a `cargo check` run came to.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Status {
    /// Cargo could not be started. The text says why.
    Unavailable(String),
    /// Cargo ran to the end.
    Finished(Outcome),
}

/// Bring `Cargo.lock` up to date with `Cargo.toml` before `cargo check`
/// starts, when it is missing or older. Rawr reads the same lock to build
/// the crate's native part at the same time, and a lock that changes under
/// it fails that build. `cargo update --workspace` adds what the manifest
/// added and leaves every other locked version as it is. Best effort:
/// `cargo check` reports any problem.
fn settle_lock(program: &OsString, manifest: &Path) {
    let lock = manifest.with_file_name("Cargo.lock");
    let modified = |path: &Path| fs::metadata(path).and_then(|metadata| metadata.modified()).ok();
    let current = match (modified(manifest), modified(&lock)) {
        (Some(manifest), Some(lock)) => lock >= manifest,
        (Some(_), None) => false,
        (None, _) => true,
    };
    if !current {
        let _ = Command::new(program)
            .args(["update", "--workspace", "--quiet", "--manifest-path"])
            .arg(manifest)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

/// One `cargo check` running.
struct Running {
    child: Child,
    stdout: thread::JoinHandle<String>,
    stderr: thread::JoinHandle<String>,
}

impl Running {
    fn start(program: &OsString, manifest: &Path) -> Result<Self, String> {
        settle_lock(program, manifest);
        let mut child = Command::new(program)
            .args(["check", "--message-format=json", "--manifest-path"])
            .arg(manifest)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| {
                if error.kind() == std::io::ErrorKind::NotFound {
                    "cargo is not on PATH. Rawr skipped `cargo check`.".to_owned()
                } else {
                    format!(
                        "cannot start `{}`: {error}. Rawr skipped `cargo check`.",
                        program.to_string_lossy()
                    )
                }
            })?;
        let stdout = read_all(child.stdout.take());
        let stderr = read_all(child.stderr.take());
        Ok(Self {
            child,
            stdout,
            stderr,
        })
    }

    fn finish(mut self, status: Option<ExitStatus>) -> Outcome {
        let status = status.or_else(|| self.child.wait().ok());
        let stdout = self.stdout.join().unwrap_or_default();
        let stderr = self.stderr.join().unwrap_or_default();
        Outcome::read(
            &stdout,
            &stderr,
            status.is_some_and(|status| status.success()),
        )
    }

    /// Stop this run. Its output readers are left to end on their own: a
    /// process cargo started may hold the pipes open a little longer.
    fn kill(mut self) {
        drop(self.child.kill());
        drop(self.child.wait());
    }
}

fn read_all(pipe: Option<impl std::io::Read + Send + 'static>) -> thread::JoinHandle<String> {
    thread::spawn(move || {
        let mut text = String::new();
        if let Some(mut pipe) = pipe {
            let mut bytes = Vec::new();
            drop(pipe.read_to_end(&mut bytes));
            text = String::from_utf8_lossy(&bytes).into_owned();
        }
        text
    })
}

/// A `cargo check` started now and read later, for `rawr check`: cargo
/// works while Rawr checks.
pub struct Pending(Result<Running, String>);

impl Pending {
    /// Start `cargo check` on the crate of `manifest`.
    #[must_use]
    pub fn start(manifest: &Path) -> Self {
        Self::start_with(&program(), manifest)
    }

    /// [`Pending::start`] with an explicit cargo command.
    #[must_use]
    pub fn start_with(program: &OsString, manifest: &Path) -> Self {
        Self(Running::start(program, manifest))
    }

    /// Wait for cargo and read what it said.
    #[must_use]
    pub fn finish(self) -> Status {
        match self.0 {
            Ok(running) => Status::Finished(running.finish(None)),
            Err(reason) => Status::Unavailable(reason),
        }
    }
}

/// Print what a `cargo check` came to after Rawr's own output: errors and
/// unavailability to stderr, a clean check to stdout. Returns whether
/// cargo found errors.
pub fn print(status: &Status) -> bool {
    match status {
        Status::Unavailable(reason) => {
            eprintln!("{reason}");
            false
        }
        Status::Finished(outcome) if outcome.is_clean() => {
            println!("{}", outcome.heading());
            false
        }
        Status::Finished(outcome) => {
            eprint!("\n{}", outcome.render());
            true
        }
    }
}

/// Start `cargo check` for the program `graph` loaded, when it is a Cargo
/// crate of marked files.
#[must_use]
pub fn begin(graph: &rawr::package::Graph) -> Option<Pending> {
    manifest(graph).map(|manifest| Pending::start(&manifest))
}

/// End `rawr check`: Rawr's result first, then cargo's under its own
/// heading. Returns whether both passed.
pub fn conclude(checked: Result<&Path, &rawr::diagnostic::Report>, cargo: Option<Pending>) -> bool {
    let checked = match checked {
        Ok(source) => {
            println!("Checked {}", source.display());
            true
        }
        Err(report) => {
            eprint!("{}", rawr::diagnostic::render(report));
            false
        }
    };
    let cargo_failed = cargo.is_some_and(|cargo| print(&cargo.finish()));
    checked && !cargo_failed
}

/// End `rawr run` of a program that ran once: report cargo's errors, if
/// any, after the program's output. The program's exit code stands.
pub fn after_run(cargo: Option<Pending>) {
    if let Some(cargo) = cargo {
        match cargo.finish() {
            Status::Finished(outcome) if outcome.is_clean() => {}
            status => {
                print(&status);
            }
        }
    }
}

enum Event {
    Saved,
    Stop,
}

/// `cargo check` in the background of `rawr run`. It starts on launch and
/// again after each save, once saves pause for the debounce. A save while
/// cargo runs stops that run: only a check of the newest files is
/// reported. Each finished run is handed to the report callback, on the
/// background thread.
pub struct Background {
    events: mpsc::Sender<Event>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Background {
    /// Start checking the crate of `manifest` now.
    #[must_use]
    pub fn start(manifest: PathBuf, report: impl FnMut(Status) + Send + 'static) -> Self {
        Self::start_with(program(), manifest, DEBOUNCE, report)
    }

    /// [`Background::start`] with an explicit cargo command and debounce.
    #[must_use]
    pub fn start_with(
        program: OsString,
        manifest: PathBuf,
        debounce: Duration,
        mut report: impl FnMut(Status) + Send + 'static,
    ) -> Self {
        let (events, received) = mpsc::channel();
        let thread = thread::Builder::new()
            .name("rawr-cargo-check".into())
            .spawn(move || {
                let mut running: Option<Running> = None;
                // The first check starts at once.
                let mut due = Some(Instant::now());
                loop {
                    let wait =
                        due.map_or(POLL, |due| due.saturating_duration_since(Instant::now()));
                    match received.recv_timeout(wait.min(POLL)) {
                        Ok(Event::Saved) => {
                            if let Some(stale) = running.take() {
                                stale.kill();
                            }
                            due = Some(Instant::now() + debounce);
                        }
                        Ok(Event::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                    }
                    if due.is_some_and(|due| Instant::now() >= due) {
                        due = None;
                        match Running::start(&program, &manifest) {
                            Ok(started) => running = Some(started),
                            Err(reason) => {
                                // Said once: a missing cargo stays missing.
                                report(Status::Unavailable(reason));
                                break;
                            }
                        }
                    }
                    if let Some(current) = running.as_mut()
                        && let Ok(Some(status)) = current.child.try_wait()
                        && let Some(done) = running.take()
                    {
                        report(Status::Finished(done.finish(Some(status))));
                    }
                }
                if let Some(stale) = running.take() {
                    stale.kill();
                }
            })
            .ok();
        Self { events, thread }
    }

    /// A source was saved: check again once saves pause.
    pub fn saved(&self) {
        drop(self.events.send(Event::Saved));
    }
}

impl Drop for Background {
    fn drop(&mut self) {
        drop(self.events.send(Event::Stop));
        if let Some(thread) = self.thread.take() {
            drop(thread.join());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Outcome;

    #[test]
    fn rustc_errors_are_read_from_cargo_json_and_the_summary_is_dropped() {
        let stdout = [
            r#"{"reason":"compiler-artifact","package_id":"x"}"#,
            r#"{"reason":"compiler-message","message":{"level":"warning","message":"unused variable: `a`","rendered":"warning: unused variable: `a`\n"}}"#,
            r#"{"reason":"compiler-message","message":{"level":"error","message":"mismatched types","rendered":"error[E0308]: mismatched types\n --> src/main.rs:3:22\n"}}"#,
            r#"{"reason":"compiler-message","message":{"level":"error","message":"aborting due to 1 previous error","rendered":"error: aborting due to 1 previous error\n"}}"#,
            r#"{"reason":"build-finished","success":false}"#,
        ]
        .join("\n");
        let outcome = Outcome::read(&stdout, "    Checking x v0.1.0\n", false);
        assert_eq!(outcome.errors.len(), 1);
        assert_eq!(outcome.warnings, 1);
        assert_eq!(outcome.failure, None);
        assert_eq!(outcome.heading(), "cargo check found 1 error");
        assert!(outcome.render().contains("error[E0308]: mismatched types"));
    }

    #[test]
    fn a_cargo_failure_without_rustc_errors_keeps_cargo_s_own_error() {
        let stderr = "    Updating crates.io index\nerror: failed to parse manifest at `/x/Cargo.toml`\n\nCaused by:\n  oops\n";
        let outcome = Outcome::read("", stderr, false);
        assert!(!outcome.is_clean());
        assert_eq!(outcome.heading(), "cargo check failed");
        assert!(outcome
            .details()
            .starts_with("error: failed to parse manifest"));
        assert!(Outcome::read("", "", true).is_clean());
    }
}
