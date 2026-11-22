//! An enum whose variants hold a `bool` and an integer, and a struct with
//! an `Option` field, which a program's patterns cover by their literals
//! and variants below them, as rustc reads them.

pub enum Flag {
    On(bool),
    Level(i64),
    Off,
}

pub struct Slot {
    pub a: Option<i64>,
    pub b: bool,
}

/// The flags a program matches, one of each kind of value.
pub fn flags() -> Vec<Flag> {
    vec![
        Flag::On(true),
        Flag::On(false),
        Flag::Level(-3),
        Flag::Level(0),
        Flag::Level(9),
        Flag::Off,
    ]
}

/// A slot holding `a` and `b`.
pub fn slot(a: Option<i64>, b: bool) -> Slot {
    Slot { a, b }
}
