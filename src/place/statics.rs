//! Statics declared inside bodies (D862). Native code can't name one, so
//! a static of a native type is a place the runner makes the first time
//! its site runs and keeps for the program's life, at a stable `'static`
//! address, as a registry that keeps a callsite by its address assumes.
//!
//! A static the interpreter holds, of a body or of the program, is kept the
//! same way: its initializer runs at its first read, and every read, on
//! every thread, shares the one value it made (#2519).
//!
//! A static is found by its key: its site and the fingerprint of its
//! block's declarations. A new generation that declares it alike reuses
//! the place and never runs its initializer again. A changed declaration
//! has a new key, so it gets a new place.
//!
//! A block's statics are reserved before any of them is initialized, so an
//! initializer may keep the `&'static` address of a sibling not written
//! yet, as tracing's `META` and `__CALLSITE` refer to each other. Statics
//! that do are built as `place::cycle` builds them (D865).

use super::cycle::{Cycle, Memory, Pointers, Written};
use super::{failure, Access, Operation, Place, Storage};
use crate::host::runtime::Witness;
use crate::{Dynamic, RawrResult, RawrResultOf, Shared};
use std::any::Any;
use std::collections::BTreeMap;
use std::sync::{Mutex, PoisonError};

/// One static's place, shared by every generation that declares it alike.
#[derive(Clone)]
pub struct Slot(Shared<Mutex<State>>);

enum State {
    /// Not written yet: its initializer runs. `memory` is where its value
    /// goes, once a sibling's initializer kept its address.
    Reserved { memory: Option<Memory> },
    /// Written: its place, which marked code reads.
    Settled(Place),
    /// Written: a value the interpreter holds, whose copies share what a
    /// `Mutex` or a `RefCell` in it holds.
    Kept(Dynamic),
}

/// A written static: its value, reached where it is and never moved.
struct Leaked(Written);

/// The statics being built that refer to each other: how many were given
/// as an address before they were written and aren't written yet, and the
/// statics written since the first was.
struct Building {
    open: usize,
    cycle: Cycle,
}

/// Every static's slot, by its key, kept across generations, and the
/// statics being built. Program threads read and initialize statics, so
/// they share one lock. A slot's own lock is taken before this one, never
/// after.
struct Statics {
    slots: BTreeMap<Box<str>, Slot>,
    building: Building,
}

static STATICS: Mutex<Statics> = Mutex::new(Statics {
    slots: BTreeMap::new(),
    building: Building {
        open: 0,
        cycle: Cycle::new(),
    },
});

fn statics() -> std::sync::MutexGuard<'static, Statics> {
    STATICS.lock().unwrap_or_else(PoisonError::into_inner)
}

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
        visit(Access::Shared(self.0.get()))
    }

    fn copies(&self, _path: &[usize]) -> bool {
        false
    }

    fn lasting(&self) -> Option<Box<dyn Any>> {
        Some(Box::new(self.0.pointer()))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// What a `&self` method of a static's place gives that borrows it, as
/// tracing's `__CALLSITE.metadata()` gives a `&'static Metadata<'static>`
/// (#1602): a `&'static` native code made, reached where it is, as the
/// static is.
struct Given(&'static dyn Lasting);

/// A value a [`Given`] place reaches: any native value that may be shared.
trait Lasting: Any + crate::func::SendSync {
    fn any(&self) -> &dyn Any;
}

impl<T: Any + crate::func::SendSync> Lasting for T {
    fn any(&self) -> &dyn Any {
        self
    }
}

impl Storage for Given {
    fn invoke(
        &self,
        _witness: &Witness,
        _path: &[usize],
        operation: Operation,
        _arguments: &mut [Dynamic],
        _rest: Option<&mut crate::scope::Rest<'_>>,
    ) -> RawrResult {
        Err(failure(format!(
            "{operation:?} of a borrow of a static declared in a body is not supported in Rawr yet"
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
            return Err(failure(
                "cannot borrow data in a `&` reference as mutable (E0596)",
            ));
        }
        if !path.is_empty() {
            return Err(failure(
                "a part of a borrow of a static declared in a body is not supported in Rawr yet",
            ));
        }
        visit(Access::Shared(self.0.any()))
    }

    fn copies(&self, _path: &[usize]) -> bool {
        false
    }

    fn lasting(&self) -> Option<Box<dyn Any>> {
        let pointer: *const dyn Any = self.0.any();
        Some(Box::new(pointer))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// The place of `value`, a `&'static` a method of a static's place gave
/// that borrows it, whose type `witness` attests (#1602): marked code
/// passes it on and calls its methods, each reaching it where it is.
pub fn given<T: Any + crate::func::SendSync>(witness: Witness, value: &'static T) -> Dynamic {
    Dynamic::from(Place {
        witness,
        storage: Shared::new(Given(value)),
        mutable: false,
    })
}

/// The static the lowering keys `key`, reserved when no generation made it
/// yet.
#[must_use]
pub fn slot(key: &str) -> Slot {
    let reserve = || Slot(Shared::new(Mutex::new(State::Reserved { memory: None })));
    statics()
        .slots
        .entry(key.into())
        .or_insert_with(reserve)
        .clone()
}

/// The statics being built that refer to each other, for `change`.
fn building<R>(change: impl FnOnce(&mut Building) -> R) -> R {
    change(&mut statics().building)
}

impl Slot {
    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Whether the static has no value yet, so its initializer runs.
    #[must_use]
    pub fn reserved(&self) -> bool {
        matches!(*self.state(), State::Reserved { .. })
    }

    /// The static as marked code reads it: its place once written. Before,
    /// the slot itself, whose address a `&'static` parameter takes.
    pub fn value(&self) -> Dynamic {
        match &*self.state() {
            State::Settled(place) => Dynamic::from(place.clone()),
            State::Kept(value) => value.clone(),
            State::Reserved { .. } => Dynamic::from(self.clone()),
        }
    }

    /// Keep `value`, the interpreter's, as the static's, and give the value
    /// every read shares: `value`, or the one another thread's read of the
    /// static kept first.
    pub fn keep(&self, value: Dynamic) -> RawrResult {
        let mut state = self.state();
        match &*state {
            State::Reserved { memory: None } => {
                *state = State::Kept(value.clone());
                Ok(value)
            }
            State::Kept(kept) => Ok(kept.clone()),
            State::Reserved { memory: Some(_) } | State::Settled(_) => {
                Err(failure("a static is written once"))
            }
        }
    }

    /// Write the static's value, the native value `value` holds, which
    /// moves out of the runner's arena into the static's memory: the memory
    /// a sibling kept the address of, or a leaked allocation of its own.
    /// `pointers` is where its type keeps the addresses of its pointers, as
    /// rustc lays it out, offsets into a value of `size` bytes; `None` when
    /// the import driver gave no layout of the type.
    pub fn settle(
        &self,
        value: Dynamic,
        layout: Option<(usize, Box<[usize]>)>,
    ) -> RawrResultOf<()> {
        let (value, witness) = taken(&value)?;
        let witness =
            witness.ok_or_else(|| failure("a static's native value has no type witness"))?;
        let mut state = self.state();
        let State::Reserved { memory } = &mut *state else {
            return Err(failure("a static is written once"));
        };
        if memory.as_ref().is_some_and(|memory| !memory.fits(&*value)) {
            return Err(failure("mismatched types (E0308)"));
        }
        let kept = memory.is_some();
        let written = match memory.take() {
            Some(memory) => memory.write(value),
            None => Written::leak(value),
        };
        building(|building| {
            if building.open == 0 {
                return Ok(());
            }
            // SAFETY: the lowering gives the import driver's layout of the
            // static's declared type, rustc's, and `value` was coerced to
            // that type, as its witness says.
            let pointers =
                layout.and_then(|(size, offsets)| unsafe { Pointers::new(size, offsets) });
            if !pointers.is_some_and(|pointers| building.cycle.join(&written, pointers)) {
                return Err(failure(
                    "a `static` in a cycle of statics whose type's layout rawr doesn't know is not \
                     supported in Rawr yet",
                ));
            }
            if kept {
                building.open -= 1;
            }
            if building.open == 0 {
                std::mem::take(&mut building.cycle).close();
            }
            Ok(())
        })?;
        *state = State::Settled(Place {
            witness,
            storage: Shared::new(Leaked(written)),
            mutable: false,
        });
        Ok(())
    }

    /// The static's `&'static T`: its value once written, or else the
    /// memory its value goes in, which a sibling's initializer keeps.
    fn address<T: 'static>(&self) -> Option<&'static T> {
        let mut state = self.state();
        match &mut *state {
            State::Settled(place) => {
                let place = place.clone();
                drop(state);
                lasting_place(&place)
            }
            State::Kept(_) => None,
            State::Reserved { memory } => {
                if memory.is_none() {
                    *memory = Some(Memory::of::<T>());
                    building(|building| building.open += 1);
                }
                memory.as_ref()?.address::<T>()
            }
        }
    }
}

/// The `&'static T` a value marked code passes for a `&'static T`
/// parameter is: a static's, of a body or of native code, or the slot of a
/// sibling static not written yet. `None` for a value that doesn't live as
/// long as the program.
#[must_use]
pub fn lasting<T: 'static>(value: &Dynamic) -> Option<&'static T> {
    if let Some(slot) = value.read_lock::<Slot>().map(|slot| slot.clone()) {
        return slot.address::<T>();
    }
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
