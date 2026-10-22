#![forbid(unsafe_code)]

include!(concat!(env!("OUT_DIR"), "/fallback.rs"));

pub fn fallback() -> u32 {
    FALLBACK
}
