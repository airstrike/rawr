//! What the end-to-end tests share: a fixture copied where Cargo keeps
//! test files, and `rawr-import run` on it with its output read as it comes.

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

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
}

impl Drop for Run {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// `rawr-import run root`, with `variables` in its environment.
pub fn start(root: &Path, variables: &[(&str, PathBuf)]) -> Run {
    let directory = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let mut child = Command::new(env!("CARGO_BIN_EXE_rawr-import"));
    child
        .arg("run")
        .arg(root)
        .env("RAWR_HOME", directory.join("rawr-home"));
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
