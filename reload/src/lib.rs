//! What a program running under `rawr run` hears about its reloads.
//!
//! `rawr run` names a socket in the program's environment, in
//! [`VARIABLE`], and writes one JSON object per line to each connection:
//! `restarted` first in a process a save restarted, then `queued`,
//! `swapped`, `failed` and `restarting` as saves come. Under `cargo run`
//! the variable is absent, and [`outcomes`] is `None`.
//!
//! ```no_run
//! if let Some(outcomes) = rawr_reload::outcomes() {
//!     std::thread::spawn(move || {
//!         for outcome in outcomes {
//!             eprintln!("{outcome}");
//!         }
//!     });
//! }
//! ```

mod json;
mod outcome;

pub use outcome::{Outcome, Span};

/// The variable `rawr run` names the program's reload stream in.
pub const VARIABLE: &str = "RAWR_RELOAD_EVENTS";

/// Whether this process runs under `rawr run`, which names its reload
/// stream.
#[must_use]
pub fn running() -> bool {
    std::env::var_os(VARIABLE).is_some_and(|path| !path.is_empty())
}

/// This process's reload outcomes, in order, from now on: each `next`
/// blocks until rawr says what a save did. `None` when the process doesn't
/// run under `rawr run`. The iterator ends when rawr does.
#[must_use]
pub fn outcomes() -> Option<Outcomes> {
    let path = std::env::var_os(VARIABLE).filter(|path| !path.is_empty())?;
    #[cfg(unix)]
    let lines = std::os::unix::net::UnixStream::connect(path)
        .ok()
        .map(std::io::BufReader::new);
    #[cfg(not(unix))]
    let lines = {
        let _ = path;
        None
    };
    Some(Outcomes { lines })
}

/// The reload outcomes [`outcomes`] reads.
pub struct Outcomes {
    #[cfg(unix)]
    lines: Option<std::io::BufReader<std::os::unix::net::UnixStream>>,
    #[cfg(not(unix))]
    lines: Option<std::io::Empty>,
}

impl Iterator for Outcomes {
    type Item = Outcome;

    fn next(&mut self) -> Option<Outcome> {
        use std::io::BufRead as _;
        let lines = self.lines.as_mut()?;
        let mut line = String::new();
        loop {
            line.clear();
            match lines.read_line(&mut line) {
                Ok(0) | Err(_) => {
                    self.lines = None;
                    return None;
                }
                // Other events (`building`, `running`, `ended`) are rawr's
                // own; a later rawr may add more.
                Ok(_) => {
                    if let Some(outcome) = Outcome::parse(&line) {
                        return Some(outcome);
                    }
                }
            }
        }
    }
}
