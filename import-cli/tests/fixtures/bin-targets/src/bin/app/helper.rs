// rawr
//! The app's line, through the host-only dependency.

pub fn line() -> String {
    format!("{}, twice", gated::greeting("app"))
}
