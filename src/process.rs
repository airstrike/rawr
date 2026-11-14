//! A generated runner's own process: how it starts and how it leaves when
//! its owner ends it.
//!
//! `rawr` spawns each runner with `posix_spawn`, which keeps the owner's
//! signal mask, and the owner blocks SIGINT, SIGTERM and SIGHUP in every
//! thread so one thread can wait for them. A runner clears that mask before
//! anything else runs, so Ctrl-C and a program's own signal handlers reach
//! it as under `cargo run`.

use std::io::Write as _;

/// Unblock every signal on this thread, which the threads it spawns then
/// inherit. A runner calls it first in `main`, before it spawns any.
pub fn unblock_signals() {
    let mut empty = std::mem::MaybeUninit::<libc::sigset_t>::uninit();
    // SAFETY: `sigemptyset` initialises the set it is given, and
    // `pthread_sigmask` only reads it. Neither can fail with these
    // arguments.
    unsafe {
        libc::sigemptyset(empty.as_mut_ptr());
        libc::pthread_sigmask(libc::SIG_SETMASK, empty.as_ptr(), std::ptr::null_mut());
    }
}

/// The status a runner ends with when rawr's Restart item asked it to:
/// `rawr run` starts the program again (#1990).
pub const RESTART: i32 = 76;

/// End the runner with `status` once its owner has ended it, to restart
/// the program or because it refused a generation: what it wrote reaches
/// stdout and stderr, and no exit handler runs. The owner starts the next
/// runner when this one is gone, and a GPU's or a window system's teardown
/// would only delay it. A program's own exit goes through
/// [`std::process::exit`] instead.
pub fn leave(status: i32) -> ! {
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    // SAFETY: `_exit` ends the process at once and returns nothing.
    unsafe { libc::_exit(status) }
}
