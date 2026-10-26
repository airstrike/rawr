//! A place an imported method's borrow guard reaches, as Freya's
//! `State::write` and std's `RefCell::borrow_mut` give one. Rawr can't keep
//! a native guard, because it borrows the value it came from, so the place
//! keeps what the call needs instead. Each access calls the method again,
//! reaches the guard's target inside that one native call, and drops the
//! guard before the call returns. A read gives a copy of the target, a
//! store puts a value in its place, and an update reads, applies and stores
//! inside one guard, as rustc's `*state.write() += 1` does. Rawr's `RefCell`
//! operations read and store it as they do a borrow of a cell, so
//! `*state.write() += 1` and `let mut value = state.write();` lower as they
//! do for a cell.

use crate::{Dynamic, EvalAltResult};

/// What an access through a place does.
pub enum Access<'a> {
    /// A copy of the target.
    Read,
    /// The value put in the target's place.
    Store(Dynamic),
    /// The target replaced by what the callback makes of a copy of it, all
    /// inside one guard.
    Update(&'a mut dyn FnMut(Dynamic) -> Result<Dynamic, Box<EvalAltResult>>),
}

/// The generated runner's access to the target.
#[cfg(feature = "sync")]
type Accessor = dyn Fn(Access<'_>) -> Result<Dynamic, Box<EvalAltResult>> + Send + Sync;
#[cfg(not(feature = "sync"))]
type Accessor = dyn Fn(Access<'_>) -> Result<Dynamic, Box<EvalAltResult>>;

/// A guard's place: what an access of it calls.
#[derive(Clone)]
pub struct Place(crate::Shared<Accessor>);

impl Place {
    /// A place whose accesses go through `access`.
    #[cfg(feature = "sync")]
    pub fn new(
        access: impl Fn(Access<'_>) -> Result<Dynamic, Box<EvalAltResult>> + Send + Sync + 'static,
    ) -> Self {
        Self(crate::Shared::new(access))
    }

    /// A place whose accesses go through `access`.
    #[cfg(not(feature = "sync"))]
    pub fn new(
        access: impl Fn(Access<'_>) -> Result<Dynamic, Box<EvalAltResult>> + 'static,
    ) -> Self {
        Self(crate::Shared::new(access))
    }

    /// A copy of the target.
    pub(crate) fn get(&self) -> Result<Dynamic, Box<EvalAltResult>> {
        (self.0)(Access::Read)
    }

    /// `value` stored in the target.
    pub(crate) fn set(&self, value: Dynamic) -> Result<Dynamic, Box<EvalAltResult>> {
        (self.0)(Access::Store(value))
    }

    /// The target replaced by `apply` of a copy of it, through one call of
    /// the guard's method.
    pub(crate) fn update(
        &self,
        apply: &mut dyn FnMut(Dynamic) -> Result<Dynamic, Box<EvalAltResult>>,
    ) -> Result<Dynamic, Box<EvalAltResult>> {
        (self.0)(Access::Update(apply))
    }
}
