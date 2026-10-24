//! Child process groups this process leads, retired if it is told to stop.

use std::sync::{Mutex, MutexGuard, PoisonError};

static OWNED: Mutex<Vec<u32>> = Mutex::new(Vec::new());

/// Files a child leaves behind when it is killed, such as a runner window's
/// agent socket under `target/rawr`, removed when their owner retires.
static LEFT: Mutex<Vec<std::path::PathBuf>> = Mutex::new(Vec::new());

/// Remove `path` when this process retires its children, or when the
/// returned guard drops, whichever is first.
pub fn remove_on_exit(path: std::path::PathBuf) -> Left {
    LEFT.lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(path.clone());
    Left(path)
}

/// Removes its file on drop. See [`remove_on_exit`].
pub struct Left(std::path::PathBuf);

impl Drop for Left {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
        LEFT.lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|path| *path != self.0);
    }
}

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
/// after this point can escape the exiting process. A command calls it when
/// it is told to stop (SIGINT, SIGTERM or SIGHUP), before it exits: a runner
/// and its window, or a Cargo build, then go with it.
pub fn retire_owned() -> MutexGuard<'static, Vec<u32>> {
    let owned = owned();
    for leader in owned.iter() {
        super::kill_group(*leader);
    }
    for path in LEFT.lock().unwrap_or_else(PoisonError::into_inner).iter() {
        let _ = std::fs::remove_file(path);
    }
    owned
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_left_file_goes_with_its_guard() {
        let path = std::env::temp_dir().join(format!("rawr-left-{}.sock", std::process::id()));
        std::fs::write(&path, b"").unwrap();
        let left = super::remove_on_exit(path.clone());
        assert!(path.exists());
        drop(left);
        assert!(!path.exists());
    }

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
