//! Native `main`: says whether it runs on the process's main thread, where
//! winit's event loop must run, and calls the marked module `answer`, once
//! with a `u64` past `i64::MAX`.
mod answer;

fn main() {
    println!("main thread {}", on_main_thread());
    println!("answer {}", answer::answer());
    println!("wide {}", answer::wide(0xC0FF_EEDE_ADBE_EF00));
}

#[cfg(target_os = "macos")]
fn on_main_thread() -> bool {
    unsafe extern "C" {
        fn pthread_main_np() -> i32;
    }
    // SAFETY: it only reads which thread calls it.
    unsafe { pthread_main_np() == 1 }
}

#[cfg(not(target_os = "macos"))]
fn on_main_thread() -> bool {
    unsafe extern "C" {
        fn getpid() -> i32;
        fn gettid() -> i32;
    }
    // SAFETY: both only read the calling process's and thread's ids.
    unsafe { getpid() == gettid() }
}
