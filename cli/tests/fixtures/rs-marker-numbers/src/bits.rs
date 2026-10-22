// Native Rust: Rawr builds this module with Cargo and calls it.

pub fn low_byte(value: u32) -> u8 {
    (value & 0xff) as u8
}

pub fn widen(byte: u8, times: u16) -> u64 {
    u64::from(byte) * u64::from(times)
}

pub fn half(value: f32) -> f32 {
    value / 2.0
}

pub fn negate(value: i8) -> i16 {
    -i16::from(value)
}

pub fn before(count: usize) -> isize {
    count as isize - 1
}

pub fn to_byte(value: i64) -> Option<u8> {
    u8::try_from(value).ok()
}

pub fn sum(a: i8, b: i16, c: i32, d: u64) -> i64 {
    i64::from(a) + i64::from(b) + i64::from(c) + d as i64
}
