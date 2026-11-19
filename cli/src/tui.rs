//! `rawr tui [path]`: the terminal debugger shell ([`rawr_tui`]) on the
//! session of the program at `path`. A run that serves the program is
//! attached to; otherwise the shell starts `rawr run` as `rawr mcp`'s
//! `start` does, handing it the session's listener, shows its output, and
//! ends it on quitting.

use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitCode, Stdio};

use crate::color;
use crate::events::session;

/// Run the shell on the program at `path`, or the current directory's.
#[must_use]
pub fn run(path: Option<PathBuf>) -> ExitCode {
    let path = match path.map_or_else(std::env::current_dir, Ok) {
        Ok(path) => std::fs::canonicalize(&path).unwrap_or(path),
        Err(error) => return failed(&error),
    };
    let target = match crate::base::discover(Some(&path)) {
        Ok(target) => target,
        Err(report) => {
            crate::diagnostic::emit(&report);
            return ExitCode::FAILURE;
        }
    };
    let root = session::root(&target);
    let source = target.source().path().to_path_buf();
    match attach(&root, &path, source) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => failed(&error),
    }
}

/// Attach to the run that serves `root`, or start one for `path`.
fn attach(root: &Path, path: &Path, source: PathBuf) -> std::io::Result<()> {
    let place = session::Place::new(root)?;
    let socket = place.socket();
    // The inspector keeps its breakpoints beside its socket.
    let saved = place.inspector().with_file_name("breakpoints.json");
    if let Ok(stream) = UnixStream::connect(&socket) {
        return rawr_tui::run(rawr_tui::Session::new(stream, source, saved));
    }
    place.make()?;
    let _ = std::fs::remove_file(&socket);
    let listener = UnixListener::bind(&socket)?;
    let mut child = start(root, path, &listener)?;
    drop(listener);
    // The kernel holds the connection until the run accepts it.
    let shown = UnixStream::connect(&socket).and_then(|stream| {
        let mut attached = rawr_tui::Session::new(stream, source, saved);
        if let Some(pipe) = child.stdout.take() {
            attached = attached.with_output(rawr_tui::Stream::Stdout, pipe);
        }
        if let Some(pipe) = child.stderr.take() {
            attached = attached.with_output(rawr_tui::Stream::Stderr, pipe);
        }
        rawr_tui::run(attached)
    });
    end(&mut child);
    place.remove();
    shown
}

/// Start `rawr run <path>` for `root`, with its output piped to the shell
/// and the session's listener handed to it.
fn start(root: &Path, path: &Path, listener: &UnixListener) -> std::io::Result<Child> {
    let mut command = Command::new(std::env::current_exe()?);
    command
        .arg("run")
        .arg(path)
        .current_dir(if root.is_dir() {
            root
        } else {
            root.parent().unwrap_or(root)
        })
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    session::hand(&mut command, listener);
    command.spawn()
}

/// End the run the shell started: SIGTERM, on which its owner retires the
/// runners it started, then wait for it.
fn end(child: &mut Child) {
    if let Ok(pid) = i32::try_from(child.id()) {
        let _ = nix::sys::signal::kill(
            nix::unistd::Pid::from_raw(pid),
            nix::sys::signal::Signal::SIGTERM,
        );
    }
    let _ = child.wait();
}

fn failed(error: &std::io::Error) -> ExitCode {
    eprintln!("{}: rawr tui: {error}", color::label(color::Level::Error));
    ExitCode::FAILURE
}
