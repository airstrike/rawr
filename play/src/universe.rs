//! Snippets on the universe runner (#434, #436): the runner `rawr-import`
//! built for `play/universe` when the image was built, which links iced,
//! iced_test and tokio through generated bindings. A snippet is compiled
//! there against the runner's own native surface, with no Cargo and no
//! nightly, and runs once. A snippet whose native calls the runner doesn't
//! cover fails as any program whose native calls are missing does.
//!
//! The universe's iced has no window: its shell (`play/universe/shell`,
//! patched in for iced_winit) runs an iced program in iced_test's emulator
//! until its boot settles and writes that frame as a PNG to the file
//! `PLAY_FRAME` names. A run that wrote one gives that image.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crate::plain::{Output, Stream};
use crate::{Limits, Outcome, PlayError};

/// The variable naming the universe's root: the directory `rawr-import`
/// built the runner from, whose `Cargo.toml` every snippet is compiled
/// beside.
pub const ROOT: &str = "PLAY_UNIVERSE";
/// The variable naming the runner binary; `<root>/runner` when unset.
pub const RUNNER: &str = "PLAY_UNIVERSE_RUNNER";
/// The variable the universe's iced shell reads: the file it writes an
/// iced program's settled frame to.
const FRAME: &str = "PLAY_FRAME";

/// The prebuilt runner and the root it was built from.
#[derive(Clone, Debug)]
pub struct Universe {
    root: PathBuf,
    runner: PathBuf,
}

impl Universe {
    /// The universe the environment names, if it names one.
    #[must_use]
    pub fn from_env() -> Option<Self> {
        let root = PathBuf::from(std::env::var_os(ROOT)?);
        let runner = std::env::var_os(RUNNER).map_or_else(|| root.join("runner"), PathBuf::from);
        Some(Self { root, runner })
    }

    /// Run `source` on the runner, stopped at `limits.time`: the frame an
    /// iced program settled on, or the text a program wrote.
    pub fn run(&self, source: &str, limits: &Limits) -> Result<Outcome, PlayError> {
        let frame = Frame::new();
        let output = self.run_with(source, limits, &frame.0)?;
        match std::fs::read(&frame.0) {
            Ok(png) if output.status == 0 && !png.is_empty() => Ok(Outcome::Image(png)),
            _ => Ok(Outcome::Text(output)),
        }
    }

    fn run_with(&self, source: &str, limits: &Limits, frame: &Path) -> Result<Output, PlayError> {
        let failed = |what: &str, error: std::io::Error| {
            PlayError::Compile(format!("?play could not {what}: {error}"))
        };
        let mut child = Command::new(&self.runner)
            .env_clear()
            .env("RAWR_PLAY_ROOT", &self.root)
            .env(FRAME, frame)
            // The server has no GPU: iced_test draws with tiny-skia.
            .env("ICED_TEST_BACKEND", "tiny-skia")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| failed("start the runner", error))?;
        let mut stdin = child.stdin.take().expect("stdin is piped");
        stdin
            .write_all(program(source).as_bytes())
            .map_err(|error| failed("hand the runner the snippet", error))?;
        drop(stdin);
        let (mut stdout, mut stderr) = (child.stdout.take(), child.stderr.take());
        let out = std::thread::spawn(move || read_all(stdout.as_mut()));
        let err = std::thread::spawn(move || read_all(stderr.as_mut()));
        let deadline = Instant::now() + limits.time;
        let (status, limit) = loop {
            if let Some(status) = child.try_wait().map_err(|error| failed("wait", error))? {
                break (status.code().unwrap_or(101), false);
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                break (1, true);
            }
            std::thread::sleep(Duration::from_millis(5));
        };
        let stdout = out.join().unwrap_or_default();
        let stderr = err.join().unwrap_or_default();
        // A program that did not compile exits 1 having printed nothing.
        if status == 1 && !limit && stdout.is_empty() {
            return Err(PlayError::Compile(stderr.trim_end().to_owned()));
        }
        let mut output = Output::default();
        output.write(Stream::Stdout, &stdout);
        output.write(Stream::Stderr, &stderr);
        if limit {
            output.write(Stream::Stderr, &format!("{}\n", crate::plain::TIME_LIMIT));
        }
        output.status = status;
        output.limit = limit;
        Ok(output)
    }
}

/// The file one run's frame is written to, removed when dropped.
struct Frame(PathBuf);

impl Frame {
    fn new() -> Self {
        static RUNS: AtomicU64 = AtomicU64::new(0);
        let run = RUNS.fetch_add(1, Ordering::Relaxed);
        let name = format!("rawr-play-{}-{run}.png", std::process::id());
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
    format!("// rawr\n{}", rawr_snippet::plain::program(source))
}

fn read_all(pipe: Option<&mut impl Read>) -> String {
    let mut bytes = Vec::new();
    if let Some(pipe) = pipe {
        let _ = pipe.read_to_end(&mut bytes);
    }
    String::from_utf8_lossy(&bytes).into_owned()
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
}
