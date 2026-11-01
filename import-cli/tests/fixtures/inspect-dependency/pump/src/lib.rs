//! A native loop that owns the thread and calls back into the program
//! each tick, as iced's event loop calls `update` and `view`.

/// A crate value with its own `Debug`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Count {
    pub tick: i64,
}

impl Count {
    pub fn at(tick: i64) -> Self {
        Self { tick }
    }
}

/// A crate value without `Debug`.
pub struct Sealed;

impl Sealed {
    pub fn new() -> Self {
        Self
    }
}

/// Call `update` with 1, 2, 3 and so on every 20 ms, until the file
/// `PUMP_STOP` names exists. Print `running` after the first call.
pub fn run<F: Fn(i64) -> i64 + 'static>(update: F) {
    let stop = std::env::var_os("PUMP_STOP");
    let mut tick = 0;
    while !stop
        .as_ref()
        .is_some_and(|path| std::path::Path::new(path).exists())
    {
        tick += 1;
        let total = update(tick);
        if tick == 1 {
            println!("running {total}");
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}
