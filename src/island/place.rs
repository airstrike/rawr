//! A place an imported method's borrow guard reaches, as Freya's
//! `State::write` and std's `RefCell::borrow_mut` give one. A runner keeps
//! the guard of a value its arena holds with that value (a [`Yoke`]), so
//! the method is called once and each access reaches that guard, as Rust
//! holds it (#1325). A read gives a copy of the target, a store puts a
//! value in its place, and an update reads, applies and stores, as rustc's
//! `*state.write() += 1` does. Rawr's `RefCell` operations read and store
//! it as they do a borrow of a cell, so `*state.write() += 1` and `let mut
//! value = state.write();` lower as they do for a cell.

use std::any::Any;
use std::ops::DerefMut;
use std::ptr::NonNull;

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

/// The guard a crate method gave of a value it borrows mutably, kept with
/// that value, as `buffer.get_mut(2)` gives a `&mut Pixel` of the buffer
/// (#1325): the method is called once, and each access of its place
/// reaches the one guard, as Rust holds the reference. A runner's arena
/// keeps the yoke where the value's slot was, so the value is reached
/// through it alone, and a later use of the value ends the guard first, as
/// rustc ends the borrow at its last use.
pub struct Yoke<U: ?Sized + 'static> {
    /// The guard, which borrows `*owner` at a lifetime only this yoke
    /// knows; `None` once it ended.
    guard: Option<Box<dyn DerefMut<Target = U>>>,
    /// The value, as `Box::leak` made it.
    owner: NonNull<dyn Any>,
}

impl<U: ?Sized + 'static> Yoke<U> {
    /// `owner`, kept with the guard `reach` makes of it.
    pub fn new<T: Any>(
        owner: Box<T>,
        reach: impl for<'a> FnOnce(&'a mut T) -> Box<dyn DerefMut<Target = U> + 'a>,
    ) -> Self {
        let value = NonNull::from(Box::leak(owner));
        // The yoke frees the value if `reach` unwinds.
        let mut yoke = Self {
            guard: None,
            owner: value,
        };
        // SAFETY: the pointer came from a box, so it is valid, aligned and
        // unique, and nothing else reaches the value while the guard lasts.
        let guard = reach(unsafe { &mut *value.as_ptr() });
        // SAFETY: only the lifetime changes, which has no layout. The guard
        // ends before the value is reached any other way, given back or
        // freed (`owner`, `into_owner`, `drop`), and it is reached only
        // through `target`, for as long as the yoke is borrowed.
        let guard = unsafe {
            std::mem::transmute::<Box<dyn DerefMut<Target = U> + '_>, Box<dyn DerefMut<Target = U>>>(
                guard,
            )
        };
        yoke.guard = Some(guard);
        yoke
    }

    /// The guard's target, while the guard lasts.
    pub fn target(&mut self) -> Option<&mut U> {
        self.guard.as_mut().map(|guard| &mut ***guard)
    }

    /// The value, the guard ended first.
    pub fn owner(&mut self) -> &mut dyn Any {
        self.guard = None;
        // SAFETY: the guard, the one other reference to the value, is gone,
        // and the borrow of `self` keeps another from starting.
        unsafe { &mut *self.owner.as_ptr() }
    }

    /// The value, given back, the guard ended first.
    #[must_use]
    pub fn into_owner(self) -> Box<dyn Any> {
        let mut yoke = std::mem::ManuallyDrop::new(self);
        yoke.guard = None;
        // SAFETY: `Box::leak` made the pointer, the guard that borrowed the
        // value is gone, and the yoke is never dropped, so the box is made
        // again once.
        unsafe { Box::from_raw(yoke.owner.as_ptr()) }
    }
}

impl<U: ?Sized + 'static> Drop for Yoke<U> {
    fn drop(&mut self) {
        self.guard = None;
        // SAFETY: as in `into_owner`: the guard is gone, and the box is made
        // again once, here.
        drop(unsafe { Box::from_raw(self.owner.as_ptr()) });
    }
}

#[cfg(test)]
mod tests {
    use super::Yoke;

    /// The guard's writes reach the value; the value is reached again
    /// once the guard ends, and given back whole.
    #[test]
    fn a_yoke_keeps_one_guard_with_its_value() {
        let mut calls = 0;
        let mut yoke = Yoke::new(Box::new(vec![1_u8, 2, 3]), |pixels: &mut Vec<u8>| {
            calls += 1;
            Box::new(&mut pixels[1])
        });
        *yoke.target().unwrap() = 7;
        *yoke.target().unwrap() += 1;
        assert_eq!(calls, 1);
        let owner = yoke.owner().downcast_mut::<Vec<u8>>().unwrap();
        assert_eq!(owner, &[1, 8, 3]);
        owner.push(4);
        assert!(yoke.target().is_none());
        let owner = yoke.into_owner().downcast::<Vec<u8>>().unwrap();
        assert_eq!(*owner, [1, 8, 3, 4]);
    }
}
