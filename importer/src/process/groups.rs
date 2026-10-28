//! Child process groups this process leads, retired if it is told to stop.

use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, PoisonError};

/// The groups this process leads and the files they leave behind.
struct Registry {
    owned: Mutex<Vec<u32>>,
    /// Files a child leaves behind when it is killed, such as a runner
    /// window's agent socket under `target/rawr`, removed when their owner
    /// retires.
    left: Mutex<Vec<PathBuf>>,
}

static REGISTRY: Registry = Registry::new();

impl Registry {
    const fn new() -> Self {
        Self {
            owned: Mutex::new(Vec::new()),
            left: Mutex::new(Vec::new()),
        }
    }

    fn owned(&self) -> MutexGuard<'_, Vec<u32>> {
        self.owned.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn left(&self) -> MutexGuard<'_, Vec<PathBuf>> {
        self.left.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn own(&self, leader: u32) {
        self.owned().push(leader);
    }

    fn release(&self, leader: u32) {
        let mut owned = self.owned();
        if let Some(index) = owned.iter().position(|owned| *owned == leader) {
            owned.swap_remove(index);
        }
    }

    fn retire(&self) -> MutexGuard<'_, Vec<u32>> {
        let owned = self.owned();
        for leader in owned.iter() {
            super::kill_group(*leader);
        }
        for path in self.left().iter() {
            let _ = std::fs::remove_file(path);
        }
        owned
    }
}

/// Remove `path` when this process retires its children, or when the
/// returned guard drops, whichever is first.
pub fn remove_on_exit(path: PathBuf) -> Left {
    REGISTRY.left().push(path.clone());
    Left(path)
}

/// Removes its file on drop. See [`remove_on_exit`].
pub struct Left(PathBuf);

impl Drop for Left {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
        REGISTRY.left().retain(|path| *path != self.0);
    }
}

pub(super) fn own(leader: u32) {
    REGISTRY.own(leader);
}

/// Forget a group once its leader is reaped, before its PGID can be reused.
pub(super) fn release(leader: u32) {
    REGISTRY.release(leader);
}

/// Kill every owned group and keep the registry locked, so no group spawned
/// after this point can escape the exiting process. A command calls it when
/// it is told to stop (SIGINT, SIGTERM or SIGHUP), before it exits: a runner
/// and its window, or a Cargo build, then go with it.
pub fn retire_owned() -> MutexGuard<'static, Vec<u32>> {
    REGISTRY.retire()
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

    fn sleeper() -> Command {
        let mut command = Command::new("/bin/sh");
        command
            .args(["-c", "sleep 30"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0);
        command
    }

    #[test]
    fn a_runner_group_is_owned_until_its_leader_is_reaped() {
        let mut child = super::super::ownership::Child::spawn(&mut sleeper()).unwrap();
        let leader = child.id();
        assert!(super::REGISTRY.owned().contains(&leader));
        child.terminate().unwrap();
        assert!(!super::REGISTRY.owned().contains(&leader));
    }

    /// Retires a registry of its own. Retiring the process's registry would
    /// kill the groups of every other test running beside it.
    #[test]
    fn termination_retires_owned_groups() {
        let registry = super::Registry::new();
        let mut child = sleeper().spawn().unwrap();
        registry.own(child.id());
        drop(registry.retire());
        let deadline = Instant::now() + Duration::from_secs(5);
        while child.try_wait().unwrap().is_none() {
            assert!(Instant::now() < deadline, "owned group survived");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
