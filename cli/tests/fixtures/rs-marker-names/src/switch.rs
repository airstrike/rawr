// rawr
//! Named as words the legacy lexer reads as keywords, which are names in
//! Rust.

pub fn f() -> i64 {
    1
}

pub struct Lever {
    pub on: bool,
}

impl Lever {
    pub fn flip(&mut self) {
        self.on = !self.on;
    }
}
