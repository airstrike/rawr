// rawr
pub fn h() -> i64 {
    super::g() * 10
}

/// Parameters and locals named as the old engine's words.
pub fn debug(call: i64) -> i64 {
    let switch = call;
    let this = switch + 1;
    this
}
