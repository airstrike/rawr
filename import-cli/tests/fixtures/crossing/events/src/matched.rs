//! An enum whose variants hold values without `Clone`, which a match moves
//! out by value and borrows through a reference, as rustc binds them.

/// A resource without `Clone`.
#[derive(Debug)]
pub struct Door {
    name: String,
}

impl Door {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_owned(),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn close(self) -> String {
        format!("closed {}", self.name)
    }

    pub fn rename(&mut self, name: &str) {
        self.name = name.to_owned();
    }
}

#[derive(Debug)]
pub enum Outcome {
    Opened(Door),
    Failed {
        error: std::io::Error,
        attempts: u32,
    },
    Idle,
}

/// An enum whose variants hold an `Outcome` and a `Door`, neither `Clone`,
/// beside fields that are.
#[derive(Debug)]
pub enum Wrapped {
    Held(Outcome, String),
    Counted { door: Door, count: u32 },
}

pub fn held(outcome: Outcome, label: &str) -> Wrapped {
    Wrapped::Held(outcome, label.to_owned())
}

pub fn counted(name: &str, count: u32) -> Wrapped {
    Wrapped::Counted {
        door: Door::new(name),
        count,
    }
}

pub fn open(name: &str) -> Outcome {
    Outcome::Opened(Door::new(name))
}

pub fn fail(attempts: u32) -> Outcome {
    Outcome::Failed {
        error: std::io::Error::other("refused"),
        attempts,
    }
}

pub fn describe(door: &Door) -> String {
    format!("door {}", door.name)
}

pub fn reason(error: &std::io::Error) -> String {
    error.to_string()
}

pub fn consume(error: std::io::Error) -> String {
    format!("consumed {error}")
}

pub fn state(outcome: &Outcome) -> &'static str {
    match outcome {
        Outcome::Opened(_) => "open",
        Outcome::Failed { .. } => "failed",
        Outcome::Idle => "idle",
    }
}

/// A label with `Clone`, which a match through a reference borrows where
/// it is rather than copies.
#[derive(Clone, Debug)]
pub struct Tag {
    label: String,
}

impl Tag {
    pub fn label(&self) -> &str {
        &self.label
    }
}

#[derive(Debug)]
pub enum Shelf {
    Stocked(Tag),
    Empty,
}

pub fn stocked(label: &str) -> Shelf {
    Shelf::Stocked(Tag {
        label: label.to_owned(),
    })
}

/// Whether `tag` is the one `shelf` holds, not a copy of it.
pub fn shelved(tag: &Tag, shelf: &Shelf) -> bool {
    matches!(shelf, Shelf::Stocked(held) if std::ptr::eq(held, tag))
}

/// Whether `door` is the one `outcome` holds, not a copy of it.
pub fn holds(door: &Door, outcome: &Outcome) -> bool {
    matches!(outcome, Outcome::Opened(held) if std::ptr::eq(held, door))
}
