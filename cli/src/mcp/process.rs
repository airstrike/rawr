//! The processes the server starts. Each leads a process group of its own,
//! so ending it ends what it started there, and each watches the server
//! ([`crate::events::watch_parent`]), so the server's death ends it too.

use std::io::Write as _;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use super::call::{Call, Wake};

/// How long a group gets between SIGTERM and SIGKILL.
pub const GRACE: Duration = Duration::from_secs(2);

/// How much of each output stream a finished call keeps: its tail.
pub const KEPT: usize = 256 * 1024;

/// Start `command` as the leader of a new process group.
pub fn spawn(command: &mut Command) -> std::io::Result<Child> {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;
        command.process_group(0);
    }
    command.spawn()
}

/// Send `signal` to the group `group` leads.
#[cfg(unix)]
fn signal_group(group: u32, signal: nix::sys::signal::Signal) {
    if let Ok(group) = i32::try_from(group) {
        let _ = nix::sys::signal::killpg(nix::unistd::Pid::from_raw(group), signal);
    }
}

/// End the group `group` leads: SIGTERM now, so an owner retires the
/// runners it started in groups of their own, and SIGKILL after
/// [`GRACE`] for anything left.
pub fn end_group(group: u32) {
    #[cfg(unix)]
    {
        use nix::sys::signal::Signal;
        signal_group(group, Signal::SIGTERM);
        let _ = std::thread::Builder::new()
            .name("rawr-mcp-reaper".into())
            .spawn(move || {
                std::thread::sleep(GRACE);
                signal_group(group, Signal::SIGKILL);
            });
    }
    #[cfg(not(unix))]
    let _ = group;
}

/// End the group `group` leads at once.
pub fn kill_group(group: u32) {
    #[cfg(unix)]
    signal_group(group, nix::sys::signal::Signal::SIGKILL);
    #[cfg(not(unix))]
    let _ = group;
}

/// A stream read on a thread of its own as it arrives, keeping its last
/// [`KEPT`] bytes. [`Tail::finish`] joins the thread.
///
/// The thread reads until the pipe closes or `finish` tells it the child
/// is done. A process that left the child's group can keep the pipe open
/// after the group ended, and the thread then stops once the pipe holds
/// nothing more to read, so the join waits on the child, not on that
/// process.
#[derive(Debug, Default)]
pub struct Tail {
    reading: Option<Reading>,
}

#[derive(Debug)]
struct Reading {
    thread: std::thread::JoinHandle<Kept>,
    /// Dropped by `finish`: its read end then reports a hang-up, which
    /// tells the thread the child is done.
    #[cfg(unix)]
    done: std::io::PipeWriter,
}

/// A stream's last bytes, and whether its start was dropped.
#[derive(Debug, Default)]
struct Kept {
    bytes: Vec<u8>,
    cut: bool,
}

impl Kept {
    fn push(&mut self, bytes: &[u8]) {
        self.bytes.extend_from_slice(bytes);
        if self.bytes.len() > KEPT {
            let excess = self.bytes.len() - KEPT;
            self.bytes.drain(..excess);
            self.cut = true;
        }
    }
}

impl Tail {
    /// Read `pipe` on a thread.
    #[cfg(unix)]
    pub fn read(pipe: impl std::io::Read + std::os::fd::AsFd + Send + 'static) -> Self {
        let Ok((told, done)) = std::io::pipe() else {
            return Self::default();
        };
        let spawned = std::thread::Builder::new()
            .name("rawr-mcp-output".into())
            .spawn(move || keep(pipe, &told));
        Self {
            reading: spawned.ok().map(|thread| Reading { thread, done }),
        }
    }

    /// Read `pipe` on a thread until it closes.
    #[cfg(not(unix))]
    pub fn read(pipe: impl std::io::Read + Send + 'static) -> Self {
        let spawned = std::thread::Builder::new()
            .name("rawr-mcp-output".into())
            .spawn(move || {
                let mut pipe = pipe;
                let mut kept = Kept::default();
                let mut buffer = [0_u8; 8192];
                while let Ok(read @ 1..) = pipe.read(&mut buffer) {
                    kept.push(&buffer[..read]);
                }
                kept
            });
        Self {
            reading: spawned.ok().map(|thread| Reading { thread }),
        }
    }

    /// What the stream held once the child is done, and whether its start
    /// was dropped. Called after the child exited and its group ended: the
    /// thread reads what the pipe still holds and ends.
    #[must_use]
    pub fn finish(self) -> (String, bool) {
        let Some(reading) = self.reading else {
            return (String::new(), false);
        };
        #[cfg(unix)]
        drop(reading.done);
        let kept = reading.thread.join().unwrap_or_default();
        (String::from_utf8_lossy(&kept.bytes).into_owned(), kept.cut)
    }
}

/// Keep what `pipe` gives until it closes, or until `told` hangs up and
/// the pipe holds nothing more to read.
#[cfg(unix)]
fn keep(mut pipe: impl std::io::Read + std::os::fd::AsFd, told: &std::io::PipeReader) -> Kept {
    use nix::poll::{poll, PollFd, PollFlags};

    let mut kept = Kept::default();
    let mut buffer = [0_u8; 8192];
    loop {
        let readable = {
            let mut fds = [
                PollFd::new(&pipe, PollFlags::POLLIN),
                PollFd::new(told, PollFlags::POLLIN),
            ];
            match poll(&mut fds, -1) {
                Ok(_) => {}
                Err(nix::errno::Errno::EINTR) => continue,
                Err(_) => return kept,
            }
            // A pipe whose writers are gone reports a hang-up, and reads
            // its end of file.
            fds[0].revents().is_some_and(|events| !events.is_empty())
        };
        if !readable {
            // The child is done and the pipe holds nothing more.
            return kept;
        }
        match pipe.read(&mut buffer) {
            Ok(0) | Err(_) => return kept,
            Ok(read) => kept.push(&buffer[..read]),
        }
    }
}

/// How a finished child went.
#[derive(Debug)]
pub struct Finished {
    /// Its exit status, unless it was stopped first.
    pub status: Option<ExitStatus>,
    /// Its stdout.
    pub stdout: Tail,
    /// Its stderr.
    pub stderr: Tail,
    /// How long it ran.
    pub elapsed: Duration,
}

/// Run `command` in a group of its own with `stdin` as its input, to its
/// exit or the call's cancel; then its group is ended.
///
/// A thread waits in the kernel for the child's exit without reaping it
/// and wakes the call, whose cancel wakes it on the same channel, so the
/// call blocks on whichever comes first.
pub fn run(command: &mut Command, stdin: Option<&str>, call: &Call) -> std::io::Result<Finished> {
    let started = Instant::now();
    command
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = spawn(command)?;
    let group = child.id();
    let waker = call.waker();
    let waiting = std::thread::Builder::new()
        .name("rawr-mcp-exit".into())
        .spawn(move || {
            rawr_watch::exit::wait(group);
            waker.wake(Wake::Exited { group });
        });
    if let Err(error) = waiting {
        kill_group(group);
        let _ = child.wait();
        return Err(error);
    }
    if let (Some(text), Some(mut pipe)) = (stdin, child.stdin.take()) {
        let text = text.to_owned();
        let _ = std::thread::Builder::new()
            .name("rawr-mcp-stdin".into())
            .spawn(move || {
                let _ = pipe.write_all(text.as_bytes());
            });
    }
    let stdout = child.stdout.take().map(Tail::read).unwrap_or_default();
    let stderr = child.stderr.take().map(Tail::read).unwrap_or_default();
    let exited = call.wait(|wake| match wake {
        Wake::Exited { group: exited } if exited == group => Some(()),
        _ => None,
    });
    let status = match exited {
        None => {
            end_group(group);
            let _ = child.wait();
            None
        }
        Some(()) => {
            // Whatever the leader left running in its group goes with it.
            kill_group(group);
            Some(child.wait()?)
        }
    };
    Ok(Finished {
        status,
        stdout,
        stderr,
        elapsed: started.elapsed(),
    })
}

/// An exit status as a number: the code, or 128 plus the signal.
#[must_use]
pub fn code(status: ExitStatus) -> i32 {
    if let Some(code) = status.code() {
        return code;
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt as _;
        if let Some(signal) = status.signal() {
            return 128 + signal;
        }
    }
    1
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;
    use std::process::Command;

    use super::Tail;
    use crate::mcp::call::Call;

    /// A child's exit answers the call with its status and output, and a
    /// cancel ends a child that would run on.
    #[cfg(unix)]
    #[test]
    fn a_run_ends_at_the_childs_exit_or_at_its_cancel() {
        let mut exiting = Command::new("sh");
        exiting.args(["-c", "echo said; exit 3"]);
        let finished = super::run(&mut exiting, None, &Call::default()).unwrap();
        assert_eq!(finished.status.map(super::code), Some(3));
        assert_eq!(finished.stdout.finish().0, "said\n");

        let call = Call::default();
        call.waker().cancel();
        let mut lasting = Command::new("sleep");
        lasting.arg("1000");
        let finished = super::run(&mut lasting, None, &call).unwrap();
        assert!(finished.status.is_none(), "a cancelled run has no status");
    }

    /// A writer that outlives the child, as a process that left its group
    /// keeps the pipe open, holds up no `finish`: it gets what the pipe
    /// held once the child was done.
    #[cfg(unix)]
    #[test]
    fn a_pipe_held_open_past_the_child_still_finishes() {
        let (pipe, mut writer) = std::io::pipe().unwrap();
        let tail = Tail::read(pipe);
        writer.write_all(b"last words\n").unwrap();
        let (text, cut) = tail.finish();
        assert_eq!(text, "last words\n");
        assert!(!cut);
        drop(writer);
    }
}
