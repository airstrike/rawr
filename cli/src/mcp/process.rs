//! The processes the server starts. Each leads a process group of its own,
//! so ending it ends what it started there, and each is told the server's
//! pid ([`crate::events::PARENT`]) so the server's death ends it too.

use std::io::Write as _;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

/// How long a group gets between SIGTERM and SIGKILL.
pub const GRACE: Duration = Duration::from_secs(2);

/// How much of each output stream a finished call keeps: its tail.
pub const KEPT: usize = 256 * 1024;

/// A call's cancellation: the client's `notifications/cancelled`, or the
/// server shutting down. It ends the groups the call started.
#[derive(Debug, Default)]
pub struct Cancel {
    cancelled: AtomicBool,
    groups: Mutex<Vec<u32>>,
}

impl Cancel {
    /// End every group the call started, and any it starts later.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        for group in self.lock().drain(..) {
            end_group(group);
        }
    }

    /// Whether the call was cancelled.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<u32>> {
        self.groups.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn adopt(&self, group: u32) {
        self.lock().push(group);
        if self.is_cancelled() {
            self.cancel();
        }
    }

    fn release(&self, group: u32) {
        self.lock().retain(|owned| *owned != group);
    }
}

/// Start `command` as the leader of a new process group, told the
/// server's pid.
pub fn spawn(command: &mut Command) -> std::io::Result<Child> {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;
        command.process_group(0);
    }
    command
        .env(crate::events::PARENT, std::process::id().to_string())
        .spawn()
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

/// A stream read as it arrives, keeping its last [`KEPT`] bytes.
#[derive(Clone, Debug, Default)]
pub struct Tail(Arc<Mutex<(Vec<u8>, bool)>>);

impl Tail {
    /// Read `pipe` on a thread until it closes.
    pub fn read(pipe: impl std::io::Read + Send + 'static) -> Self {
        let tail = Self::default();
        let filling = tail.clone();
        let _ = std::thread::Builder::new()
            .name("rawr-mcp-output".into())
            .spawn(move || {
                let mut pipe = pipe;
                let mut buffer = [0_u8; 8192];
                while let Ok(read) = pipe.read(&mut buffer) {
                    if read == 0 {
                        break;
                    }
                    filling.push(&buffer[..read]);
                }
            });
        tail
    }

    fn push(&self, bytes: &[u8]) {
        let mut kept = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        kept.0.extend_from_slice(bytes);
        if kept.0.len() > KEPT {
            let excess = kept.0.len() - KEPT;
            kept.0.drain(..excess);
            kept.1 = true;
        }
    }

    /// What arrived so far, and whether its start was dropped.
    #[must_use]
    pub fn text(&self) -> (String, bool) {
        let kept = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        (String::from_utf8_lossy(&kept.0).into_owned(), kept.1)
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
    /// It ran past its deadline and was ended.
    pub timed_out: bool,
    /// How long it ran.
    pub elapsed: Duration,
}

/// Run `command` in a group of its own with `stdin` as its input, until it
/// exits, `deadline` passes or `cancel` fires; then its group is ended.
pub fn run(
    command: &mut Command,
    stdin: Option<&str>,
    deadline: Instant,
    cancel: &Cancel,
) -> std::io::Result<Finished> {
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
    cancel.adopt(group);
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
    let mut timed_out = false;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break Some(status);
        }
        if cancel.is_cancelled() {
            break None;
        }
        if Instant::now() >= deadline {
            timed_out = true;
            break None;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let ended = status.is_none();
    if ended {
        end_group(group);
        let _ = child.wait();
    } else {
        // Whatever the leader left running in its group goes with it.
        kill_group(group);
    }
    cancel.release(group);
    // The readers finish once every writer is gone; a moment covers the
    // last bytes in flight.
    std::thread::sleep(Duration::from_millis(20));
    Ok(Finished {
        status,
        stdout,
        stderr,
        timed_out,
        elapsed: started.elapsed(),
    })
}

/// The pids under `root`, `root` included, from `ps`.
#[must_use]
pub fn tree(root: u32) -> Vec<u32> {
    let Ok(output) = Command::new("ps")
        .args(["-axo", "pid=,ppid="])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
    else {
        return vec![root];
    };
    let pairs: Vec<(u32, u32)> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            Some((fields.next()?.parse().ok()?, fields.next()?.parse().ok()?))
        })
        .collect();
    let mut kept = vec![root];
    let mut index = 0;
    while index < kept.len() {
        let parent = kept[index];
        kept.extend(
            pairs
                .iter()
                .filter(|(_, ppid)| *ppid == parent)
                .map(|(pid, _)| *pid),
        );
        index += 1;
    }
    kept
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
