//! A runner child and its process group retire together.

use std::process::{self, ExitStatus};
use std::time::Instant;

use crate::{Error, Result};

/// The unreaped runner leader whose process group is killed when it is reaped.
pub(super) struct Child {
    child: process::Child,
    retained: bool,
    /// Whether the child's group has this process's terminal, to give back
    /// when it ends.
    terminal: bool,
}

impl Child {
    pub(super) fn spawn(command: &mut process::Command) -> Result<Self> {
        let child = command.spawn().map_err(|error| io("spawn runner", error))?;
        super::groups::own(child.id());
        Ok(Self {
            child,
            retained: true,
            terminal: false,
        })
    }

    /// [`Self::spawn`], and when stdin is the terminal this process has in
    /// the foreground, the child's group gets it, as a shell gives it to a
    /// job, so the program can read it. A child that read it first and
    /// stopped goes on.
    pub(super) fn spawn_with_terminal(command: &mut process::Command) -> Result<Self> {
        let mut child = Self::spawn(command)?;
        child.terminal = terminal::give(child.id());
        Ok(child)
    }

    pub(super) fn id(&self) -> u32 {
        self.child.id()
    }

    pub(super) const fn is_reaped(&self) -> bool {
        !self.retained
    }

    pub(super) fn stdout(&mut self) -> Option<process::ChildStdout> {
        self.child.stdout.take()
    }

    pub(super) fn stderr(&mut self) -> Option<process::ChildStderr> {
        self.child.stderr.take()
    }

    /// Observe a natural exit; an exited leader's leftover group is retired with it.
    pub(super) fn try_wait(&mut self) -> Result<Option<ExitStatus>> {
        if !self.retained {
            return self
                .child
                .try_wait()
                .map_err(|error| io("observe retired runner", error));
        }
        let status = self
            .child
            .try_wait()
            .map_err(|error| io("observe runner exit", error))?;
        if status.is_some() {
            self.take_terminal();
            super::kill_group(self.id());
            super::groups::release(self.id());
            self.retained = false;
        }
        Ok(status)
    }

    /// Take the terminal back from an ending child's group.
    fn take_terminal(&mut self) {
        if std::mem::take(&mut self.terminal) {
            terminal::take_back(self.id());
        }
    }

    /// Wait briefly for a runner that already acknowledged its shutdown.
    pub(super) fn wait(&mut self) -> Result<ExitStatus> {
        let deadline = Instant::now() + super::DRAIN_DEADLINE;
        loop {
            if let Some(status) = self.try_wait()? {
                return Ok(status);
            }
            if Instant::now() >= deadline {
                return Err(Error::Process {
                    program: "native runner",
                    status: "reap runner".into(),
                    stderr: "runner did not exit after shutdown acknowledgement".into(),
                });
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }

    pub(super) fn terminate(&mut self) -> Result<()> {
        if !self.retained {
            return Ok(());
        }
        self.reap().map(|_| ())
    }

    fn reap(&mut self) -> Result<ExitStatus> {
        self.take_terminal();
        let status = super::reap_group(&mut self.child)?;
        super::groups::release(self.id());
        self.retained = false;
        Ok(status)
    }
}

impl Drop for Child {
    fn drop(&mut self) {
        let _ = self.terminate();
    }
}

/// The controlling terminal on stdin, handed between this process's group
/// and a runner's.
#[cfg(unix)]
mod terminal {
    use rustix::process::{getpgrp, Pid};
    use rustix::termios::{tcgetpgrp, tcsetpgrp};

    /// Give stdin's terminal to the group `leader` leads, when this
    /// process's group has it in the foreground. Whether it did.
    pub(super) fn give(leader: u32) -> bool {
        let stdin = std::io::stdin();
        let Some(leader) = i32::try_from(leader).ok().and_then(Pid::from_raw) else {
            return false;
        };
        if !tcgetpgrp(&stdin).is_ok_and(|foreground| foreground == getpgrp()) {
            return false;
        }
        if tcsetpgrp(&stdin, leader).is_err() {
            return false;
        }
        // A read before the hand-off stopped the program with SIGTTIN.
        let _ = rustix::process::kill_process_group(leader, rustix::process::Signal::CONT);
        true
    }

    /// Take stdin's terminal back from the group `leader` leads, when it
    /// still has it.
    pub(super) fn take_back(leader: u32) {
        let stdin = std::io::stdin();
        let Some(leader) = i32::try_from(leader).ok().and_then(Pid::from_raw) else {
            return;
        };
        if tcgetpgrp(&stdin).is_ok_and(|foreground| foreground == leader) {
            let _ = tcsetpgrp(&stdin, getpgrp());
        }
    }
}

#[cfg(not(unix))]
mod terminal {
    pub(super) fn give(_leader: u32) -> bool {
        false
    }

    pub(super) fn take_back(_leader: u32) {}
}

fn io(operation: &'static str, error: std::io::Error) -> Error {
    Error::Process {
        program: "native runner",
        status: operation.into(),
        stderr: error.to_string().into_boxed_str(),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn naturally_exited_leader_drains_descendants_before_retirement() {
        use std::io::Read as _;
        use std::os::unix::process::CommandExt as _;

        let mut command = process::Command::new("/bin/sh");
        command
            .args(["-c", "sleep 30 & exit 0"])
            .stdin(process::Stdio::null())
            .stdout(process::Stdio::piped())
            .stderr(process::Stdio::null())
            .process_group(0);
        let mut child = Child::spawn(&mut command).expect("isolated runner starts");
        let mut pipe = child.stdout().expect("descendant inherits log pipe");
        let (sender, receiver) = std::sync::mpsc::channel();
        let reader = std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let result = pipe.read_to_end(&mut bytes);
            let _ = sender.send(result);
        });
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = child
                .try_wait()
                .expect("non-reaping exit observation succeeds")
            {
                assert!(status.success());
                break;
            }
            assert!(Instant::now() < deadline, "fixture leader did not exit");
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(child.is_reaped());
        receiver
            .recv_timeout(super::super::DRAIN_DEADLINE)
            .expect("drained descendants release inherited log pipes")
            .expect("bounded log reader reaches EOF");
        reader.join().expect("log reader finishes");
        child
            .terminate()
            .expect("already retired ownership cannot signal again");
    }
}
