//! Functions that never return: `-> !`, whose call is a value of any type.

/// Panic with `message`.
pub fn fail(message: &str) -> ! {
    panic!("{message}")
}

/// Print `message`, then end the process with `code`.
pub fn finish(message: &str, code: i32) -> ! {
    println!("{message}");
    std::process::exit(code)
}
