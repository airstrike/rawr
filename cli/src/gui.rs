//! `rawr gui [path]`: the debugger window on the program's run, in a
//! process of its own (`rawr-gui`, beside `rawr`), so it keeps drawing
//! while the program is paused (D1053.5). It attaches to the session of a
//! run already going. With none, this process starts `rawr run` the way a
//! shell starts one: it binds the session's socket and hands the run its
//! listener, and holds the run's output, which it tells the session for
//! the window and echoes to its own terminal. Closing the window ends the
//! run it started.

use std::io::Write as _;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::sync::mpsc;

use serde_json::{json, Value};

use crate::events::session;

/// The window's binary, beside this one.
const WINDOW: &str = "rawr-gui";

/// Open the window on the run of the program at `path`, or the current
/// directory's.
#[must_use]
pub fn open(path: Option<&PathBuf>, pid: Option<u32>) -> ExitCode {
    match run(path, pid) {
        Ok(code) => code,
        Err(error) => {
            eprintln!(
                "{}: rawr gui: {error}",
                crate::color::label(crate::color::Level::Error)
            );
            ExitCode::FAILURE
        }
    }
}

fn run(path: Option<&PathBuf>, pid: Option<u32>) -> Result<ExitCode, String> {
    let target = crate::base::discover(path, &mut rawr::project::cargo::Answers::new()).map_err(
        |report| {
            let here = std::env::current_dir().unwrap_or_default();
            crate::diagnostic::lines(&crate::diagnostic::shape(&report, &here))
        },
    )?;
    let root = session::root(&target);
    let runs = session::Runs::of(&root).map_err(|error| error.to_string())?;
    let rawr = std::env::current_exe().map_err(|error| error.to_string())?;
    let window = rawr.with_file_name(WINDOW);
    if !window.is_file() {
        return Err(format!(
            "{} isn't there: the window is its own binary, installed beside rawr",
            window.display()
        ));
    }
    if let Some(reached) = runs.connect(pid) {
        if let Some(notice) = reached.notice("Pick one with `rawr gui --pid <pid>`.") {
            eprintln!("{notice}");
        }
        let socket = reached.socket();
        drop(reached);
        return status(Command::new(&window).arg(socket));
    }
    if let Some(pid) = pid {
        return Err(runs.unserved(pid).to_string());
    }
    let place = session::Place::new(&root).map_err(|error| error.to_string())?;
    let socket = place.socket();

    // No run serves the program: start one, as a shell does.
    place.make().map_err(|error| error.to_string())?;
    let _ = std::fs::remove_file(&socket);
    let listener = UnixListener::bind(&socket).map_err(|error| error.to_string())?;
    let directory = if root.is_dir() {
        root.clone()
    } else {
        root.parent()
            .map_or_else(|| root.clone(), Path::to_path_buf)
    };
    let mut command = Command::new(&rawr);
    command
        .arg("run")
        .args(path)
        .current_dir(&directory)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    {
        use std::os::unix::process::CommandExt as _;
        command.process_group(0);
    }
    session::hand(&mut command, &listener);
    let mut child = command.spawn().map_err(|error| error.to_string())?;
    drop(listener);
    // The kernel holds the connection until the run accepts it.
    let told = UnixStream::connect(&socket).map_err(|error| error.to_string())?;
    let pieces = teller(told);
    if let Some(pipe) = child.stdout.take() {
        relay(pipe, "stdout", pieces.clone());
    }
    if let Some(pipe) = child.stderr.take() {
        relay(pipe, "stderr", pieces);
    }
    let closed = status(Command::new(&window).arg(&socket));
    if let Ok(None) = child.try_wait()
        && let Ok(group) = i32::try_from(child.id())
    {
        let _ = nix::sys::signal::killpg(
            nix::unistd::Pid::from_raw(group),
            nix::sys::signal::Signal::SIGTERM,
        );
    }
    let _ = child.wait();
    place.remove();
    closed
}

/// Run the window to its end.
fn status(command: &mut Command) -> Result<ExitCode, String> {
    let status = command.status().map_err(|error| error.to_string())?;
    Ok(status
        .code()
        .and_then(|code| u8::try_from(code).ok())
        .map_or(ExitCode::FAILURE, ExitCode::from))
}

/// Start the thread that writes each piece it is sent to the session as a
/// line of its own, until both relays end.
fn teller(mut told: UnixStream) -> mpsc::Sender<Value> {
    let (pieces, heard) = mpsc::channel::<Value>();
    let _ = std::thread::Builder::new()
        .name("rawr-gui-tell".into())
        .spawn(move || {
            for piece in heard {
                if writeln!(told, "{piece}").is_err() {
                    return;
                }
            }
        });
    pieces
}

/// Echo `pipe` to this process's own `stream`, and send each piece to the
/// teller as `output` for the shells attached to the session.
fn relay(
    mut pipe: impl std::io::Read + Send + 'static,
    stream: &'static str,
    pieces: mpsc::Sender<Value>,
) {
    let _ = std::thread::Builder::new()
        .name("rawr-gui-output".into())
        .spawn(move || {
            let mut buffer = [0_u8; 8192];
            // A character a read splits waits for the rest of it.
            let mut held = Vec::new();
            loop {
                let read = match pipe.read(&mut buffer) {
                    Ok(0) | Err(_) => return,
                    Ok(read) => read,
                };
                let bytes = &buffer[..read];
                let _ = if stream == "stderr" {
                    std::io::stderr().write_all(bytes)
                } else {
                    std::io::stdout().write_all(bytes)
                };
                held.extend_from_slice(bytes);
                let whole = match std::str::from_utf8(&held) {
                    Err(error) if error.error_len().is_none() => error.valid_up_to(),
                    _ => held.len(),
                };
                let text = String::from_utf8_lossy(&held[..whole]).into_owned();
                held.drain(..whole);
                let piece = json!({
                    "jsonrpc": "2.0",
                    "method": "output",
                    "params": { "stream": stream, "text": text },
                });
                // The echo goes on when the session is gone, so the run
                // never blocks on a full pipe.
                let _ = pieces.send(piece);
            }
        });
}
