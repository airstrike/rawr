//! The Rust half of the polyglot fixture: Rawr names it as `crate::model`.

pub struct Thing {
    pub count: i64,
}

impl Thing {
    pub fn new(count: i64) -> Self {
        Self { count }
    }

    pub fn doubled(&self) -> i64 {
        self.count * 2
    }
}
