//! A plain Rust library the marked crate beside it depends on by path.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Gauge {
    pub level: i32,
}

impl Default for Gauge {
    fn default() -> Self {
        Gauge { level: 7 }
    }
}

impl Gauge {
    pub fn level(&self) -> i32 {
        self.level
    }
}

pub fn double(value: i32) -> i32 {
    value * 2
}
