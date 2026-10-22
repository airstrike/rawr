//! A runner child and its process group retire together.

use std::process::{self, ExitStatus};
use std::time::Instant;

use crate::{Error, Result};

/// The unreaped runner leader whose process group is killed when it is reaped.
pub(super) struct Child {
    child: process::Child,
    retained: bool,
}

impl Child {
    pub(super) fn spawn(command: &mut process::Command) -> Result<Self> {
        let child = command.spawn().map_err(|error| io("spawn runner", error))?;
        super::groups::own(child.id());
        Ok(Self {
            child,
            retained: true,
        })
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
            super::kill_group(self.id());
            super::groups::release(self.id());
            self.retained = false;
        }
        Ok(status)
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
