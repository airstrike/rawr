//! The number Rust's panic hook names a thread by, after its name: the
//! operating system's id of the thread, or, where it has none, std's own
//! `ThreadId`, as std's `thread::current_os_id` gives it.

/// The id of the thread that calls, as Rust's hook prints it in
/// `thread 'main' (<id>) panicked at`.
#[must_use]
pub(crate) fn current() -> u64 {
    os().unwrap_or_else(std_id)
}

/// std's `ThreadId` of the thread that calls, which std prints where the
/// system gives no id. Its number is public only through `Debug`, as
/// `ThreadId(<number>)`.
fn std_id() -> u64 {
    let id = format!("{:?}", std::thread::current().id());
    id.strip_prefix("ThreadId(")
        .and_then(|id| id.strip_suffix(')'))
        .and_then(|id| id.parse().ok())
        .unwrap_or_default()
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn os() -> Option<u64> {
    // SAFETY: `gettid` takes no arguments and always succeeds. std calls
    // the system call, as glibc before 2.30 has no wrapper.
    let id = unsafe { libc::syscall(libc::SYS_gettid) };
    u64::try_from(id).ok()
}

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "tvos",
    target_os = "watchos"
))]
fn os() -> Option<u64> {
    let mut id = 0u64;
    // SAFETY: a null thread is the calling thread, and `id` is a valid
    // place for the call to write its id to.
    let status = unsafe { libc::pthread_threadid_np(0, &raw mut id) };
    (status == 0).then_some(id)
}

#[cfg(windows)]
fn os() -> Option<u64> {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentThreadId() -> u32;
    }
    // SAFETY: `GetCurrentThreadId` takes no arguments and always succeeds.
    Some(u64::from(unsafe { GetCurrentThreadId() }))
}

#[cfg(not(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios",
    target_os = "tvos",
    target_os = "watchos",
    windows
)))]
fn os() -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    #[test]
    fn two_threads_have_two_ids_and_one_thread_keeps_its_own() {
        let here = super::current();
        assert_eq!(here, super::current());
        let there = std::thread::spawn(super::current).join().unwrap();
        assert_ne!(here, there);
        assert_ne!(super::std_id(), 0);
    }
}
