//! The package's library, which its marked binary names by the crate name,
//! as Cargo builds the binary (#1164): an iced-shaped `run` generic over
//! the program's state and messages, and a type of its own.

/// A floor of the building.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Floor {
    number: i64,
}

impl Floor {
    pub fn new(number: i64) -> Self {
        Self { number }
    }

    pub fn above(self) -> Self {
        Self::new(self.number + 1)
    }

    pub fn below(self) -> Self {
        Self::new(self.number - 1)
    }

    pub fn label(&self) -> String {
        format!("floor {}", self.number)
    }
}

/// `state` after `update` takes each of `messages`, as `view` shows it.
pub fn run<State, Message>(
    mut state: State,
    messages: Vec<Message>,
    update: impl Fn(&mut State, Message),
    view: impl Fn(&State) -> String,
) -> String {
    for message in messages {
        update(&mut state, message);
    }
    view(&state)
}
