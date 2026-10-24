#![forbid(unsafe_code)]

use std::process::ExitCode;

fn main() -> ExitCode {
    // A stopped command retires the runner and the builds it owns.
    if let Err(error) = retire_on_termination() {
        eprintln!("warning: child processes may outlive an interrupted rawr: {error}");
    }
    rawr_import_cli::command::main(std::env::args_os().skip(1).collect())
}

/// Make SIGINT, SIGTERM and SIGHUP retire every child process group this
/// process owns (a runner, a Cargo build) before it exits with the
/// conventional status. Called before any thread starts: threads inherit
/// the blocked mask, while spawned programs start with an empty one.
fn retire_on_termination() -> std::io::Result<()> {
    use nix::sys::signal::{SigSet, Signal};

    let mut signals = SigSet::empty();
    for signal in [Signal::SIGINT, Signal::SIGTERM, Signal::SIGHUP] {
        signals.add(signal);
    }
    signals.thread_block().map_err(std::io::Error::from)?;
    std::thread::Builder::new()
        .name("rawr-termination".into())
        .spawn(move || {
            let signal = loop {
                if let Ok(signal) = signals.wait() {
                    break signal;
                }
            };
            let _owned = rawr_import::process::retire_owned();
            std::process::exit(128 + signal as i32);
        })
        .map(drop)
}
