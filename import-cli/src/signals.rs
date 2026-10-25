//! A program starts with no blocked signals, as under `cargo run`.
//!
//! `rawr-import` blocks SIGINT, SIGTERM and SIGHUP in every thread so one
//! thread can wait for them (`main.rs`), and a spawned process inherits its
//! spawning thread's mask. A runner would then never see Ctrl-C at the
//! terminal it reads, nor a program's own signal handlers fire. This is one
//! isolated `unsafe` call: the hook std offers for code that runs in the
//! child before it executes.
#![allow(unsafe_code)]

use std::os::unix::process::CommandExt as _;
use std::process::Command;

/// Clear the signal mask of the process `command` spawns.
pub(crate) fn unblocked(command: &mut Command) {
    // SAFETY: the closure runs in the child between fork and exec. It
    // allocates nothing and calls only `pthread_sigmask`, which is
    // async-signal-safe.
    unsafe {
        command.pre_exec(|| {
            nix::sys::signal::SigSet::empty()
                .thread_set_mask()
                .map_err(std::io::Error::from)
        });
    }
}
