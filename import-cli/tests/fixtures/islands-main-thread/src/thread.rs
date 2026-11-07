//! Native: which thread runs, and a native error type with an alias of a
//! `Result`, as `iced::Error` and `iced::Result` are.

/// Why a run failed.
#[derive(Debug)]
pub struct Failure(String);

impl Failure {
    pub fn new(reason: &str) -> Failure {
        Failure(reason.to_owned())
    }
}

/// A run's outcome, as `iced::Result` is iced's.
pub type Outcome = Result<(), Failure>;

#[cfg(target_os = "macos")]
pub fn on_main_thread() -> bool {
    unsafe extern "C" {
        fn pthread_main_np() -> i32;
    }
    // SAFETY: it only reads which thread calls it.
    unsafe { pthread_main_np() == 1 }
}

#[cfg(not(target_os = "macos"))]
pub fn on_main_thread() -> bool {
    unsafe extern "C" {
        fn getpid() -> i32;
        fn gettid() -> i32;
    }
    // SAFETY: both only read the calling process's and thread's ids.
    unsafe { getpid() == gettid() }
}
