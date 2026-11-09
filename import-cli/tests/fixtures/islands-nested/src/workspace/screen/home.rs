// rawr
//! A screen of an existing app, marked to hot-reload two native modules
//! below the crate root (#1467): it names the root's `Message` through
//! `crate::` and a `use`, its parent's private constant and its private
//! sibling's items through `super::`, its native child's through `self::`,
//! and native code calls its functions and builds its `State`.

use crate::Message;

use super::other;

pub mod wizard;

const FACTOR: usize = 1;

pub struct State {
    pub visits: usize,
}

impl State {
    pub fn new() -> State {
        State { visits: 0 }
    }
}

pub fn view(state: &State) -> String {
    format!(
        "home {} {}",
        state.visits,
        other::helper(state.visits) + super::OFFSET
    )
}

pub fn open(visits: usize) -> Message {
    if visits == 0 {
        return crate::Message::Closed;
    }
    Message::Opened(visits * super::other::SCALE)
}

pub fn steps() -> usize {
    self::wizard::count()
}

pub fn label(tick: usize) -> String {
    format!("value {}", tick * FACTOR)
}
