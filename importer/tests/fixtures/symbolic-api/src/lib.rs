//! Governed Phase 2 fixture for symbolic imported contracts.

pub use facade_api::*;

pub fn integer_only() -> i64 {
    3
}

#[derive(Clone, Copy)]
pub struct Unit;

#[derive(Clone, Copy)]
pub struct Tuple(pub i64, pub i64);

#[derive(Clone, Copy)]
pub struct Record {
    pub left: i64,
    pub right: i64,
}

pub use Record as RecordA;
pub use Record as RecordB;

pub fn make_record() -> Record {
    Record { left: 1, right: 2 }
}

#[derive(Clone, Copy)]
pub enum Forms {
    Unit,
    Tuple(i64, i64),
    Named { left: i64, right: i64 },
}

pub struct PrivateRecord {
    pub visible: i64,
    hidden: i64,
}

pub fn private_record() -> PrivateRecord {
    PrivateRecord {
        visible: 1,
        hidden: 2,
    }
}

impl PrivateRecord {
    pub fn hidden(&self) -> i64 {
        self.hidden
    }

    pub fn owned(self) -> i64 {
        self.visible + self.hidden
    }
}

impl Record {
    pub const ZERO: Self = Self { left: 0, right: 0 };
    pub const ONE: i64 = 1;

    pub const fn new(left: i64, right: i64) -> Self {
        Self { left, right }
    }

    pub fn shared(&self) -> i64 {
        self.left + self.right
    }

    pub fn mutable(&mut self) -> i64 {
        self.left += 1;
        self.left
    }

    pub fn owned(self) -> i64 {
        self.left + self.right
    }

    pub fn recursive(&self) -> Self {
        *self
    }
}
