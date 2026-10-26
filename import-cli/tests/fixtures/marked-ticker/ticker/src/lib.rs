//! A native loop that calls back into the program, as a framework's event
//! loop does: it owns the thread until it ends.

/// Print the process id, then call `step` with 1, 2, 3 and so on every
/// 250 ms and print what it gives, until the file `TICKER_STOP` names
/// exists or `count` steps ran.
pub fn run<F: Fn(i64) -> String + 'static>(step: F, count: i64) {
    let stop = std::env::var_os("TICKER_STOP");
    println!("pid {}", std::process::id());
    for tick in 1..=count {
        if stop
            .as_ref()
            .is_some_and(|path| std::path::Path::new(path).exists())
        {
            break;
        }
        println!("{}", step(tick));
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
}
