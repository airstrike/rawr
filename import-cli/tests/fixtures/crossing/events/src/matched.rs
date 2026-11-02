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
