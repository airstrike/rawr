//! The listening socket a shell hands to the run it starts. The shell binds
//! the session's socket before it starts `rawr run` and connects at once,
//! so it waits on no file to appear: the kernel holds the connection until
//! the run's owner accepts it, and a run that ends first closes it.
//!
//! The descriptor goes to `rawr run` alone. The run takes it before it
//! starts any process, as a [`Handed`] its `main` holds, and passes it on
//! only by `exec`, to the binary that serves the program, or to its
//! owner's session.

#![allow(unsafe_code)]

use std::os::fd::{AsRawFd as _, FromRawFd as _, IntoRawFd as _, RawFd};
use std::os::unix::net::UnixListener;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};

/// The variable naming the descriptor of the listener a shell handed to the
/// run it started.
pub const LISTENER: &str = "RAWR_SESSION_LISTENER";

/// Whether a shell started this run.
static HANDED: AtomicBool = AtomicBool::new(false);

/// The listener a shell handed this run, which its owner's session serves
/// on, or which `exec` keeps for the binary that serves the program.
#[derive(Debug)]
#[must_use = "a run's session serves on the listener its shell handed it"]
pub struct Handed(UnixListener);

impl Handed {
    pub(super) fn into_listener(self) -> UnixListener {
        self.0
    }
}

/// Take the listener a shell handed this run, if one did. Called before
/// the run starts any process, so none inherits it.
pub fn adopt() -> Option<Handed> {
    let descriptor = std::env::var(LISTENER)
        .ok()
        .and_then(|text| text.parse::<RawFd>().ok())?;
    // SAFETY: the shell that started this process left `descriptor` open
    // for it alone. A descriptor that isn't a Unix socket's fails
    // `local_addr`, and is released below without being closed.
    let listener = unsafe { UnixListener::from_raw_fd(descriptor) };
    let bound = listener
        .local_addr()
        .is_ok_and(|address| address.as_pathname().is_some());
    if !bound || set_inherited(descriptor, false).is_err() {
        let _ = listener.into_raw_fd();
        return None;
    }
    HANDED.store(true, Ordering::Release);
    Some(Handed(listener))
}

/// Whether a shell started this run and handed it its session's listener.
#[must_use]
pub fn handed() -> bool {
    HANDED.load(Ordering::Acquire)
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

/// Keep this run's listener, `handed`, through `exec` into the binary that
/// serves the program, which takes it again with [`adopt`].
pub(crate) fn pass_on(command: &mut Command, handed: Option<&Handed>) {
    match handed {
        Some(Handed(listener)) if set_inherited(listener.as_raw_fd(), true).is_ok() => {
            command.env(LISTENER, listener.as_raw_fd().to_string());
        }
        _ => {
            command.env_remove(LISTENER);
        }
    }
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
