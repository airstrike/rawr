//! Child process groups this process leads, retired if it is told to stop.

use std::sync::{Mutex, MutexGuard, PoisonError};

use nix::sys::signal::{SigSet, Signal};

static OWNED: Mutex<Vec<u32>> = Mutex::new(Vec::new());

fn owned() -> MutexGuard<'static, Vec<u32>> {
    OWNED.lock().unwrap_or_else(PoisonError::into_inner)
}

pub(super) fn own(leader: u32) {
    owned().push(leader);
}

/// Forget a group once its leader is reaped, before its PGID can be reused.
pub(super) fn release(leader: u32) {
    let mut owned = owned();
    if let Some(index) = owned.iter().position(|owned| *owned == leader) {
        owned.swap_remove(index);
    }
}

/// Kill every owned group and keep the registry locked, so no group spawned
/// after this point can escape the exiting process.
fn retire_owned() -> MutexGuard<'static, Vec<u32>> {
    let owned = owned();
    for leader in owned.iter() {
        super::kill_group(*leader);
    }
    owned
}

/// Make SIGINT, SIGTERM and SIGHUP retire every owned child process group
/// (a runner and its window, a Cargo build) before this process exits with the
/// conventional status. Call before spawning threads: they inherit the blocked
/// mask, while spawned programs start with an empty one.
pub fn retire_on_termination() -> std::io::Result<()> {
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
            let _owned = retire_owned();
            std::process::exit(128 + signal as i32);
        })
        .map(drop)
}

#[cfg(test)]
mod tests {
    use std::os::unix::process::CommandExt as _;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    #[test]
    fn termination_retires_owned_runner_groups() {
        let mut command = Command::new("/bin/sh");
        command
            .args(["-c", "sleep 30"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0);
        let mut child = super::super::ownership::Child::spawn(&mut command).unwrap();
        drop(super::retire_owned());
        let deadline = Instant::now() + Duration::from_secs(5);
        while child.try_wait().unwrap().is_none() {
            assert!(Instant::now() < deadline, "owned runner group survived");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!super::owned().contains(&child.id()));
    }
}
