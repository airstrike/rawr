//! Waiting for a child process to end without reaping it.

/// Block until the child process `pid` ends. The child stays a zombie for
/// its owner to reap. Returns at once when `pid` is no child of this
/// process, or was reaped already.
pub fn wait(pid: u32) {
    let Some(pid) = i32::try_from(pid)
        .ok()
        .and_then(rustix::process::Pid::from_raw)
    else {
        return;
    };
    let options = rustix::process::WaitIdOptions::EXITED | rustix::process::WaitIdOptions::NOWAIT;
    // A signal handled on this thread interrupts the wait, which goes on.
    while let Err(rustix::io::Errno::INTR) =
        rustix::process::waitid(rustix::process::WaitId::Pid(pid), options)
    {}
}
