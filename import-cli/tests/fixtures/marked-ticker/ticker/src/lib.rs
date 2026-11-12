//! A native loop that calls back into the program, as a framework's event
//! loop does: it owns the thread until it ends.

/// Print the process id, then call `step` with 1, 2, 3 and so on every
/// 250 ms and print what it gives, until the file `TICKER_STOP` names
/// exists or `count` steps ran. With `TICKER_LOCK`, it first locks that
/// file for as long as it runs, and says `lock held`, or `lock busy` when
/// another process holds it.
pub fn run<F: Fn(i64) -> String + 'static>(step: F, count: i64) {
    let stop = std::env::var_os("TICKER_STOP");
    let _lock = std::env::var_os("TICKER_LOCK").map(|path| {
        let file = std::fs::File::create(path).unwrap();
        let held = file.try_lock().is_ok();
        println!("lock {}", if held { "held" } else { "busy" });
        file
    });
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
