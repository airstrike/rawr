//! A run's place goes with the run when a signal ends it: SIGINT, SIGTERM
//! or SIGHUP, whose default action would otherwise leave it for the next
//! shell's sweep.
//!
//! The handler only unlinks and removes directories, both
//! async-signal-safe, on paths made before it is installed, then lets the
//! signal take its default action again.

#![allow(unsafe_code)]

use std::ffi::{c_int, CString};
use std::os::unix::ffi::OsStrExt as _;
use std::sync::OnceLock;

use nix::sys::signal::{SaFlags, SigAction, SigHandler, SigSet, Signal};

use super::Place;

/// The place's files, then its directory and the program's, as the
/// handler removes them.
static PATHS: OnceLock<Paths> = OnceLock::new();

/// The paths a place is removed by, made before any signal can need them.
struct Paths {
    files: [CString; 3],
    directories: [CString; 2],
}

/// Remove `place` when SIGINT, SIGTERM or SIGHUP ends this process, for
/// each of them whose default action ends it now. A signal this process
/// ignores or handles keeps its disposition. A process that blocks them
/// and waits for them calls [`retire`](super::retire) itself.
pub(super) fn on_termination(place: &Place) {
    let Some(paths) = Paths::of(place) else {
        return;
    };
    if PATHS.set(paths).is_err() {
        return;
    }
    let handler = SigAction::new(
        SigHandler::Handler(retire),
        SaFlags::SA_RESETHAND,
        SigSet::empty(),
    );
    for signal in [Signal::SIGINT, Signal::SIGTERM, Signal::SIGHUP] {
        // SAFETY: `retire` calls only async-signal-safe functions on paths
        // made before it was installed.
        let Ok(previous) = (unsafe { nix::sys::signal::sigaction(signal, &handler) }) else {
            continue;
        };
        if !matches!(previous.handler(), SigHandler::SigDfl) {
            // SAFETY: puts back the disposition this process had.
            let _ = unsafe { nix::sys::signal::sigaction(signal, &previous) };
        }
    }
}

impl Paths {
    fn of(place: &Place) -> Option<Self> {
        let path = |path: &std::path::Path| CString::new(path.as_os_str().as_bytes()).ok();
        let directory = place.socket().parent()?.to_path_buf();
        Some(Self {
            files: [
                path(&place.socket())?,
                path(&place.inspector())?,
                path(&place.breakpoints())?,
            ],
            directories: [path(&directory)?, path(directory.parent()?)?],
        })
    }
}

/// Remove the place, then end the process as `signal` does by default:
/// the handler was reset on entry.
extern "C" fn retire(signal: c_int) {
    if let Some(paths) = PATHS.get() {
        for file in &paths.files {
            let _ = nix::unistd::unlink(file.as_c_str());
        }
        for directory in &paths.directories {
            let _ = nix::unistd::unlinkat(
                None,
                directory.as_c_str(),
                nix::unistd::UnlinkatFlags::RemoveDir,
            );
        }
    }
    if let Ok(signal) = Signal::try_from(signal) {
        let _ = nix::sys::signal::raise(signal);
    }
}
