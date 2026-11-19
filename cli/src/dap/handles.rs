//! The variable references a client reads children by: a frame's
//! bindings, or a value's parts as the inspector listed them. They hold
//! while the program stays paused where it was, and a stop or a resume
//! drops them, as the protocol has it.

use serde_json::{json, Value};

/// What one reference reads.
#[derive(Clone, Debug)]
pub enum Handle {
    /// The bindings of frame `0` (the function the program is in), `1`
    /// (its caller), and on.
    Frame(u64),
    /// A value's parts, each `{"name", "value", "children"}`.
    Parts(Vec<Value>),
}

/// The references given out since the program last stopped or went on,
/// each its index from 1.
#[derive(Debug, Default)]
pub struct Handles(Vec<Handle>);

impl Handles {
    /// A reference to `handle`.
    pub fn give(&mut self, handle: Handle) -> u64 {
        self.0.push(handle);
        self.0.len() as u64
    }

    /// What `reference` reads, while it holds.
    #[must_use]
    pub fn get(&self, reference: u64) -> Option<&Handle> {
        let index = usize::try_from(reference).ok()?.checked_sub(1)?;
        self.0.get(index)
    }

    /// The program stopped or went on: no reference holds.
    pub fn drop_all(&mut self) {
        self.0.clear();
    }

    /// A protocol `Variable` for a value named `name` that reads as
    /// `value`, its `children` behind a reference.
    pub fn variable(&mut self, name: &str, value: &str, children: Value) -> Value {
        let reference = match children {
            Value::Array(parts) if !parts.is_empty() => self.give(Handle::Parts(parts)),
            _ => 0,
        };
        json!({ "name": name, "value": value, "variablesReference": reference })
    }
}
