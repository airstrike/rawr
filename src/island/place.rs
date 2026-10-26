//! A place an imported method's borrow guard reaches, as Freya's
//! `State::write` and std's `RefCell::borrow_mut` give one. Rawr can't keep
//! a native guard, because it borrows the value it came from, so the place
//! keeps what the call needs instead. Each read or store calls the method
//! again, reaches the guard's target inside that one native call, and
//! drops the guard before the call returns. A read gives a copy of
//! the target, and a store puts a value in its place. Rawr's `RefCell`
//! operations read and store it as they do a borrow of a cell, so
//! `*state.write() += 1` and `let mut value = state.write();` lower as they
//! do for a cell.

use crate::{Dynamic, EvalAltResult};

/// The generated runner's access to the target: `None` reads it, and
/// `Some(value)` stores `value`.
#[cfg(feature = "sync")]
type Access = dyn Fn(Option<Dynamic>) -> Result<Dynamic, Box<EvalAltResult>> + Send + Sync;
#[cfg(not(feature = "sync"))]
type Access = dyn Fn(Option<Dynamic>) -> Result<Dynamic, Box<EvalAltResult>>;

/// A guard's place: what a read or a store of it calls.
#[derive(Clone)]
pub struct Place(crate::Shared<Access>);

impl Place {
    /// A place whose reads and stores go through `access`.
    #[cfg(feature = "sync")]
    pub fn new(
        access: impl Fn(Option<Dynamic>) -> Result<Dynamic, Box<EvalAltResult>> + Send + Sync + 'static,
    ) -> Self {
        Self(crate::Shared::new(access))
    }

    /// A place whose reads and stores go through `access`.
    #[cfg(not(feature = "sync"))]
    pub fn new(
        access: impl Fn(Option<Dynamic>) -> Result<Dynamic, Box<EvalAltResult>> + 'static,
    ) -> Self {
        Self(crate::Shared::new(access))
    }

    /// A copy of the target.
    pub(crate) fn get(&self) -> Result<Dynamic, Box<EvalAltResult>> {
        (self.0)(None)
    }

    /// `value` stored in the target.
    pub(crate) fn set(&self, value: Dynamic) -> Result<Dynamic, Box<EvalAltResult>> {
        (self.0)(Some(value))
    }
}
