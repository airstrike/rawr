//! Statics declared inside bodies (D862). Native code can't name one, so
//! a static of a native type is a place the runner makes the first time
//! its site runs and keeps for the program's life, at a stable `'static`
//! address, as a registry that keeps a callsite by its address assumes.
//!
//! A static is found by its key: its site and the fingerprint of its
//! block's declarations. A new generation that declares it alike reuses
//! the place and never runs its initializer again. A changed declaration
//! has a new key, so it gets a new place.
//!
//! A static is written before anything takes its address: the lowering
//! refuses an initializer that names a sibling not written yet, so statics
//! that refer to each other, as tracing's `META` and `__CALLSITE` do, are
//! refused at `rawr check`.

use super::{failure, Access, Operation, Place, Storage};
use crate::host::runtime::Witness;
use crate::{Dynamic, RawrResult, RawrResultOf, Shared};
use std::any::Any;
use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};

/// One static's place, shared by every generation that declares it alike.
#[derive(Clone)]
pub struct Slot(Shared<Mutex<State>>);

enum State {
    /// Not written yet: its initializer runs.
    Reserved,
    /// Written: its place, which marked code reads.
    Settled(Place),
}

/// A written static: its value, leaked, reached where it is and never
/// moved or written again.
struct Leaked(&'static dyn Any);

// SAFETY: a `static`'s type is `Sync` (rustc's E0277), so its value may be
// read from any thread; `Leaked` only ever gives shared access.
unsafe impl Send for Leaked {}
// SAFETY: as for `Send`.
unsafe impl Sync for Leaked {}

impl Storage for Leaked {
    fn invoke(
        &self,
        _witness: &Witness,
        _path: &[usize],
        operation: Operation,
        _arguments: &mut [Dynamic],
        _rest: Option<&mut crate::scope::Rest<'_>>,
    ) -> RawrResult {
        Err(failure(format!(
            "{operation:?} of a static declared in a body is not supported in Rawr yet"
        )))
    }

    fn reach(
        &self,
        _witness: &Witness,
        path: &[usize],
        mutable: bool,
        visit: &mut dyn FnMut(Access<'_, dyn Any>) -> RawrResult,
    ) -> RawrResult {
        if mutable {
            return Err(failure("cannot borrow a static item as mutable (E0596)"));
        }
        if !path.is_empty() {
            return Err(failure(
                "a part of a static declared in a body is not supported in Rawr yet",
            ));
        }
        visit(Access::Shared(self.0))
    }

    fn copies(&self, _path: &[usize]) -> bool {
        false
    }

    fn lasting(&self) -> Option<Box<dyn Any>> {
        let value: *const dyn Any = self.0;
        Some(Box::new(value))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// The static the lowering keys `key`, reserved when no generation made it
/// yet.
#[must_use]
pub fn slot(key: &str) -> Slot {
    let reserve = || Slot(Shared::new(Mutex::new(State::Reserved)));
    #[cfg(feature = "sync")]
    {
        static SLOTS: std::sync::OnceLock<Mutex<HashMap<Box<str>, Slot>>> =
            std::sync::OnceLock::new();
        SLOTS
            .get_or_init(Mutex::default)
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(key.into())
            .or_insert_with(reserve)
            .clone()
    }
    #[cfg(not(feature = "sync"))]
    {
        thread_local! {
            static SLOTS: std::cell::RefCell<HashMap<Box<str>, Slot>> =
                std::cell::RefCell::default();
        }
        SLOTS.with_borrow_mut(|slots| slots.entry(key.into()).or_insert_with(reserve).clone())
    }
}

impl Slot {
    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Whether the static has no value yet, so its initializer runs.
    #[must_use]
    pub fn reserved(&self) -> bool {
        matches!(*self.state(), State::Reserved)
    }

    /// The static as marked code reads it: its place, once its block wrote
    /// it.
    pub fn value(&self) -> RawrResult {
        match &*self.state() {
            State::Settled(place) => Ok(Dynamic::from(place.clone())),
            State::Reserved => Err(failure(
                "a static read before its block initialized it is not supported in Rawr yet",
            )),
        }
    }

    /// Write the static's value, the native value `value` holds, which
    /// moves out of the runner's arena into a leaked allocation of its own.
    pub fn settle(&self, value: Dynamic) -> RawrResultOf<()> {
        let (value, witness) = taken(&value)?;
        let witness =
            witness.ok_or_else(|| failure("a static's native value has no type witness"))?;
        let mut state = self.state();
        if !matches!(*state, State::Reserved) {
            return Err(failure("a static is written once"));
        }
        let value: &'static dyn Any = Box::leak(value);
        *state = State::Settled(Place {
            witness,
            storage: Shared::new(Leaked(value)),
            mutable: false,
        });
        Ok(())
    }
}

/// The `&'static T` a value marked code passes for a `&'static T`
/// parameter is: a static's, of a body or of native code. `None` for a
/// value that doesn't live as long as the program.
#[must_use]
pub fn lasting<T: 'static>(value: &Dynamic) -> Option<&'static T> {
    lasting_place(&super::get(value)?)
}

fn lasting_place<T: 'static>(place: &Place) -> Option<&'static T> {
    let pointer = *place.storage.lasting()?.downcast::<*const dyn Any>().ok()?;
    // SAFETY: lasting storage is a static's, which lives as long as the
    // program and is only read.
    unsafe { &*pointer }.downcast_ref()
}

/// The native value `value` holds, moved out of the runner's arena, with
/// its type's witness.
fn taken(value: &Dynamic) -> RawrResultOf<(Box<dyn Any>, Option<Witness>)> {
    #[cfg(feature = "rust-import")]
    if let Some(taken) = crate::island::settled(value) {
        return taken.map_err(failure);
    }
    let _ = value;
    Err(failure(
        "a static declared in a body holds a native value only",
    ))
}
