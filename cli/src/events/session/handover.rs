//! The listening socket a shell hands to the run it starts. The shell binds
//! the session's socket before it starts `rawr run` and connects at once,
//! so it waits on no file to appear: the kernel holds the connection until
//! the run's owner accepts it, and a run that ends first closes it.
//!
//! The descriptor goes to `rawr run` alone. The run takes it before it
//! starts any process, and passes it on only by `exec`, to the binary that
//! serves the program.

#![allow(unsafe_code)]

use std::os::fd::{AsRawFd as _, FromRawFd as _, IntoRawFd as _, RawFd};
use std::os::unix::net::UnixListener;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};

/// The variable naming the descriptor of the listener a shell handed to the
/// run it started.
pub const LISTENER: &str = "RAWR_SESSION_LISTENER";

/// The listener this run was handed, until its owner opens the session.
static ADOPTED: Mutex<Option<UnixListener>> = Mutex::new(None);
/// Whether a shell started this run.
static HANDED: AtomicBool = AtomicBool::new(false);

/// Take the listener a shell handed this run, if one did. Called before
/// the run starts any process, so none inherits it.
pub fn adopt() {
    let Some(descriptor) = std::env::var(LISTENER)
        .ok()
        .and_then(|text| text.parse::<RawFd>().ok())
    else {
        return;
    };
    // SAFETY: the shell that started this process left `descriptor` open
    // for it alone. A descriptor that isn't a Unix socket's fails
    // `local_addr`, and is released below without being closed.
    let listener = unsafe { UnixListener::from_raw_fd(descriptor) };
    let bound = listener
        .local_addr()
        .is_ok_and(|address| address.as_pathname().is_some());
    if !bound || set_inherited(descriptor, false).is_err() {
        let _ = listener.into_raw_fd();
        return;
    }
    HANDED.store(true, Ordering::Release);
    *lock() = Some(listener);
}

/// Whether a shell started this run and handed it its session's listener.
#[must_use]
pub fn handed() -> bool {
    HANDED.load(Ordering::Acquire)
}

/// The listener this run was handed, for its owner's session.
pub(super) fn take() -> Option<UnixListener> {
    lock().take()
}

/// Hand `listener` to the run `command` starts.
pub(crate) fn hand(command: &mut Command, listener: &UnixListener) {
    use std::os::unix::process::CommandExt as _;

    let descriptor = listener.as_raw_fd();
    command.env(LISTENER, descriptor.to_string());
    // SAFETY: the hook runs in the child between `fork` and `exec`, and
    // calls only `fcntl`, which is async-signal-safe.
    unsafe {
        command.pre_exec(move || set_inherited(descriptor, true).map_err(std::io::Error::from));
    }
}

/// Keep this run's listener through `exec` into the binary that serves the
/// program, which takes it again with [`adopt`].
pub(crate) fn pass_on(command: &mut Command) {
    match &*lock() {
        Some(listener) if set_inherited(listener.as_raw_fd(), true).is_ok() => {
            command.env(LISTENER, listener.as_raw_fd().to_string());
        }
        _ => {
            command.env_remove(LISTENER);
        }
    }
}

fn lock() -> std::sync::MutexGuard<'static, Option<UnixListener>> {
    ADOPTED.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Whether `exec` keeps `descriptor` open.
fn set_inherited(descriptor: RawFd, inherited: bool) -> nix::Result<()> {
    use nix::fcntl::{fcntl, FcntlArg, FdFlag};

    let flags = if inherited {
        FdFlag::empty()
    } else {
        FdFlag::FD_CLOEXEC
    };
    fcntl(descriptor, FcntlArg::F_SETFD(flags)).map(drop)
}
