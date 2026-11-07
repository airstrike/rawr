//! Snippets on the universe runner (#434, #436): the runner `rawr-import`
//! built for `play/universe` when the image was built, which links iced,
//! iced_test and tokio through generated bindings. A snippet is compiled
//! there against the runner's own native surface, with no Cargo and no
//! nightly, and runs once. A snippet whose native calls the runner doesn't
//! cover fails as any program whose native calls are missing does.
//!
//! The runner is a server (#515): it decodes its bindings, installs them
//! in an engine and binds their runtime once, then runs each snippet in a
//! fork of itself (`rawr::play`), under the snippet's own time and memory
//! limits. A [`Server`] is one such runner. The bot starts one and names
//! its socket to each run (`PLAY_UNIVERSE_SOCKET`). A run with none named
//! starts a server for itself.
//!
//! The universe's iced has no window: its shell (`play/universe/shell`,
//! patched in for iced_winit) runs an iced program headless until it
//! settles, and writes its frame as a PNG, or a GIF when it animated, to
//! the file `PLAY_FRAME` names, before the deadline `PLAY_DEADLINE` gives
//! or the sooner one the file `PLAY_DEADLINE_FILE` names may give later.
//! A run that wrote one gives that image beside the text it printed.

use std::io::BufRead as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use crate::plain::{Output, Stream};
use crate::{Limits, PlayError};

/// The variable naming the universe's root: the directory `rawr-import`
/// built the runner from, whose `Cargo.toml` every snippet is compiled
/// beside.
pub const ROOT: &str = "PLAY_UNIVERSE";
/// The variable naming the runner binary; `<root>/runner` when unset.
pub const RUNNER: &str = "PLAY_UNIVERSE_RUNNER";
/// The variable naming the socket of a running [`Server`].
pub const SOCKET: &str = "PLAY_UNIVERSE_SOCKET";
/// The variable the universe's iced shell reads: the file it writes an
/// iced program's frame to.
const FRAME: &str = "PLAY_FRAME";
/// The variable naming the run's deadline for the shell, in milliseconds
/// since the Unix epoch, so it writes its frame before the run is stopped.
const DEADLINE: &str = "PLAY_DEADLINE";
/// The variable naming the file a sooner deadline may be written to as the
/// run goes on (see [`rawr::play::deadline`]), which the shell reads at
/// each frame.
const DEADLINE_FILE: &str = "PLAY_DEADLINE_FILE";

/// The prebuilt runner, the root it was built from, and the socket of a
/// server already running it, if one is.
#[derive(Clone, Debug)]
pub struct Universe {
    root: PathBuf,
    runner: PathBuf,
    socket: Option<PathBuf>,
}

impl Universe {
    /// The universe the environment names, if it names one.
    #[must_use]
    pub fn from_env() -> Option<Self> {
        let root = PathBuf::from(std::env::var_os(ROOT)?);
        let runner = std::env::var_os(RUNNER).map_or_else(|| root.join("runner"), PathBuf::from);
        let socket = std::env::var_os(SOCKET).map(PathBuf::from);
        Some(Self {
            root,
            runner,
            socket,
        })
    }

    /// The universe at `root`, run by `runner`.
    #[must_use]
    pub fn new(root: PathBuf, runner: PathBuf) -> Self {
        Self {
            root,
            runner,
            socket: None,
        }
    }

    /// Run `source` on the named server, or on one started for this run:
    /// the text a program wrote, and the frame an iced program drew.
    pub fn run(&self, source: &str, limits: &Limits) -> Result<Output, PlayError> {
        match &self.socket {
            Some(socket) => run_on(&self.root, socket, source, limits),
            None => self.serve()?.run(source, limits),
        }
    }

    /// Start the runner as a server, and wait until it is prepared.
    pub fn serve(&self) -> Result<Server, PlayError> {
        static SERVERS: AtomicU64 = AtomicU64::new(0);
        let failed = |error: std::io::Error| {
            PlayError::Compile(format!("?play could not start the runner: {error}"))
        };
        let server = SERVERS.fetch_add(1, Ordering::Relaxed);
        let name = format!("rawr-play-{}-{server}.sock", std::process::id());
        let socket = std::env::temp_dir().join(name);
        let mut child = Command::new(&self.runner)
            .env_clear()
            .env("RAWR_PLAY_ROOT", &self.root)
            .env(rawr::play::SOCKET, &socket)
            // The server has no GPU: iced_test draws with tiny-skia.
            .env("ICED_TEST_BACKEND", "tiny-skia")
            // The server stops when this end closes.
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(failed)?;
        let mut line = String::new();
        let stdout = child.stdout.take().expect("stdout is piped");
        let read = std::io::BufReader::new(stdout).read_line(&mut line);
        let server = Server {
            child,
            root: self.root.clone(),
            socket,
        };
        match read {
            Ok(_) if line.trim_end() == rawr::play::READY => Ok(server),
            Ok(_) => Err(failed(std::io::Error::other(
                "it stopped before it was ready",
            ))),
            Err(error) => Err(failed(error)),
        }
    }
}

/// A runner serving snippets, prepared once. Dropping it stops it.
#[derive(Debug)]
pub struct Server {
    child: Child,
    root: PathBuf,
    socket: PathBuf,
}

impl Server {
    /// The socket it serves on.
    #[must_use]
    pub fn socket(&self) -> &Path {
        &self.socket
    }

    /// Whether it is still running.
    pub fn running(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// Run `source` on it, as [`Universe::run`] does.
    pub fn run(&self, source: &str, limits: &Limits) -> Result<Output, PlayError> {
        run_on(&self.root, &self.socket, source, limits)
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.socket);
    }
}

/// A universe served for many runs: its server starts at the first, and
/// again after one stops.
#[derive(Debug)]
pub struct Served {
    universe: Universe,
    server: Mutex<Option<Server>>,
}

impl Served {
    /// Serve `universe`; nothing starts until [`Served::socket`].
    #[must_use]
    pub fn new(universe: Universe) -> Self {
        Self {
            universe,
            server: Mutex::new(None),
        }
    }

    /// The socket of the running server, started first if none runs.
    pub fn socket(&self) -> Result<PathBuf, PlayError> {
        let mut server = self
            .server
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !server.as_mut().is_some_and(Server::running) {
            *server = None;
            *server = Some(self.universe.serve()?);
        }
        Ok(server
            .as_ref()
            .expect("a server was just started")
            .socket()
            .to_owned())
    }
}

/// Run `source` on the server at `socket`, which serves the universe at
/// `root`.
fn run_on(root: &Path, socket: &Path, source: &str, limits: &Limits) -> Result<Output, PlayError> {
    let frame = Frame::new();
    // The run's deadline counts from its fork, a little after this.
    let deadline = std::time::SystemTime::now() + limits.time;
    let deadline = deadline
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_millis());
    let mut env = vec![
        (FRAME.into(), frame.0.clone().into()),
        (DEADLINE.into(), deadline.to_string().into()),
    ];
    if let Some(file) = &limits.deadline {
        env.push((DEADLINE_FILE.into(), file.clone().into()));
    }
    let wrapped = program(source);
    let request = rawr::play::Request {
        source: wrapped.clone(),
        env,
        time: limits.time,
        deadline: limits.deadline.clone(),
        memory: limits.memory,
    };
    let ran = rawr::play::run(socket, &request)
        .map_err(|error| PlayError::Compile(format!("?play could not run the snippet: {error}")))?;
    let stdout = String::from_utf8_lossy(&ran.stdout);
    let stderr = String::from_utf8_lossy(&ran.stderr);
    // A program that did not compile exits 1 having printed nothing.
    if ran.status == 1 && !ran.limit && stdout.is_empty() {
        let entry = root.join("src").join("main.rs");
        let report = unmarked(&stderr, &entry.to_string_lossy(), &wrapped, source);
        return Err(PlayError::Compile(report.trim_end().to_owned()));
    }
    let mut output = Output::default();
    output.write(Stream::Stdout, &stdout);
    output.write(Stream::Stderr, &stderr);
    if ran.limit {
        output.write(Stream::Stderr, &format!("{}\n", crate::plain::TIME_LIMIT));
    }
    output.status = ran.status;
    output.limit = ran.limit;
    output.frame = std::fs::read(&frame.0)
        .ok()
        .filter(|image| output.status == 0 && crate::plain::image_kind(image).is_some());
    Ok(output)
}

/// A report the runner rendered for `program`, the snippet compiled as the
/// universe's `entry`, as the snippet as written shows it: in `play.rs`,
/// counting without the marker's line, and with what the wrapping added
/// cut, as a report on a plain run is.
fn unmarked(text: &str, entry: &str, program: &str, source: &str) -> String {
    let wrapped = program.strip_prefix(MARKER).unwrap_or(program);
    let name = rawr_snippet::SOURCE_NAME;
    // The line before, in the snippet: the marker is line 1.
    let earlier = |line: &str| match line.parse::<usize>() {
        Ok(number) => number.saturating_sub(1).max(1).to_string(),
        Err(_) => line.to_owned(),
    };
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        let located = [" --> ", " ::: "].into_iter().find_map(|arrow| {
            let location = line.strip_prefix(arrow)?.strip_prefix(entry)?;
            Some((arrow, location.strip_prefix(':')?))
        });
        if let Some((arrow, location)) = located {
            let location = location
                .split('-')
                .map(|point| match point.split_once(':') {
                    Some((line, column)) => format!("{}:{column}", earlier(line)),
                    None => point.to_owned(),
                })
                .collect::<Vec<_>>()
                .join("-");
            out.push_str(&format!("{arrow}{name}:{location}"));
        } else if let Some((gutter, rest)) = line.split_once(" | ")
            && gutter.trim().parse::<usize>().is_ok()
        {
            let width = gutter.len() - 1;
            out.push_str(&format!(" {:>width$} | {rest}", earlier(gutter.trim())));
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    rawr_snippet::report(&out, wrapped, source)
}

/// The line that marks the snippet as Rawr code for the runner.
const MARKER: &str = "// rawr\n";

/// The file one run's frame is written to, removed when dropped.
struct Frame(PathBuf);

impl Frame {
    fn new() -> Self {
        static RUNS: AtomicU64 = AtomicU64::new(0);
        let run = RUNS.fetch_add(1, Ordering::Relaxed);
        let name = format!("rawr-play-{}-{run}.frame", std::process::id());
        let path = std::env::temp_dir().join(name);
        let _ = std::fs::remove_file(&path);
        Self(path)
    }
}

impl Drop for Frame {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// The program the runner compiles for `source`: marked as Rawr code, and
/// a snippet of statements wrapped as `?play` wraps it.
#[must_use]
pub fn program(source: &str) -> String {
    format!("{MARKER}{}", rawr_snippet::plain::program(source))
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_snippet_is_marked_and_wrapped_for_the_runner() {
        assert_eq!(
            super::program("fn main() {\n    println!(\"hi\");\n}\n"),
            "// rawr\nfn main() {\n    println!(\"hi\");\n}\n"
        );
        assert!(super::program("1 + 2").starts_with("// rawr\nfn main() {"));
    }

    /// The runner's report counts the marker's line and names the
    /// universe's entry; the snippet's names `play.rs` and counts its own.
    #[test]
    fn a_compile_error_points_into_the_snippet_as_written() {
        let source = "fn main() {\n    let x = 1;\n    nothing(x);\n}\n";
        let program = super::program(source);
        let rendered = "error[RAWR-RESOLVE-HOST-OPERATION]: this use of a crate is not available in this prebuilt runner\n \
             --> /universe/src/main.rs:4:5-4:12\n \
             4 |     nothing(x);\n   \
             |     ^^^^^^^ the runner was not built with this call\n";
        assert_eq!(
            super::unmarked(rendered, "/universe/src/main.rs", &program, source),
            "error[RAWR-RESOLVE-HOST-OPERATION]: this use of a crate is not available in this prebuilt runner\n \
             --> play.rs:3:5-3:12\n \
             3 |     nothing(x);\n   \
             |     ^^^^^^^ the runner was not built with this call\n"
        );
    }
}
