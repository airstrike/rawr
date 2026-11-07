// rawr
use crate::thread::{Failure, Outcome};

pub fn answer() -> i64 {
    6 * 7
}

/// A `u64` past `i64::MAX` crosses both ways.
pub fn wide(value: u64) -> u64 {
    value ^ 1
}

/// Whether marked code runs on the thread native code called it from.
pub fn on_main_thread() -> bool {
    crate::thread::on_main_thread()
}

/// A `Result` of types that cross.
pub fn checked(n: i64) -> Result<i64, String> {
    if n > 0 {
        Ok(n)
    } else {
        Err(format!("{n} is not positive"))
    }
}

/// An alias of a `Result` whose `Err` is a native type, as `iced::Result`.
pub fn run(fail: bool) -> Outcome {
    if fail {
        return Err(Failure::new("no window"));
    }
    Ok(())
}
