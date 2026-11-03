//! A crate type's derived order keys a B-tree map (#363), and a method
//! gives the same enum at a type that borrows its receiver (#345), as
//! iced's `window::Id` and `Key::as_ref` do.

use std::sync::atomic::{self, AtomicU64};

/// A window's id: ordered as its number, not as its hash.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Id(u64);

static COUNT: AtomicU64 = AtomicU64::new(10);

impl Id {
    /// A new id, counting down, so an order by creation is backwards.
    pub fn unique() -> Id {
        Id(COUNT.fetch_sub(1, atomic::Ordering::Relaxed))
    }
}

/// A named key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Named {
    Space,
    Escape,
}

/// A key, holding its character as `C`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Key<C = String> {
    Named(Named),
    Character(C),
    Unidentified,
}

impl Key {
    /// The key with its character borrowed, to match on.
    pub fn as_ref(&self) -> Key<&str> {
        match self {
            Self::Named(named) => Key::Named(*named),
            Self::Character(c) => Key::Character(c.as_str()),
            Self::Unidentified => Key::Unidentified,
        }
    }
}

/// A character key the program presses.
pub fn character(c: &str) -> Key {
    Key::Character(c.to_string())
}

/// A named key the program presses.
pub fn named(named: Named) -> Key {
    Key::Named(named)
}

/// A key with no name or character.
pub fn unidentified() -> Key {
    Key::Unidentified
}
