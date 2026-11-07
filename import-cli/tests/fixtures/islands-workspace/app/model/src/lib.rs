//! A member whose build script generates its grammar.
include!(concat!(env!("OUT_DIR"), "/grammar.rs"));

pub fn rule() -> String {
    format!("{RULE} ({} logo bytes)", common::logo().len())
}

pub fn double(value: i64) -> i64 {
    value * 2
}
