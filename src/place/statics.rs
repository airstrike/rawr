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
//! A block's statics are reserved before any of them is initialized, so an
//! initializer may take a sibling's `&'static` address while the sibling
//! has no value yet, as tracing's `META` and `__CALLSITE` refer to each
//! other. The address is memory for the sibling's type, which the sibling's
//! value moves into once it is made.

use super::{failure, Access, Operation, Place, Storage};
use crate::host::runtime::Witness;
use crate::{Dynamic, RawrResult, RawrResultOf, Shared};
use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::mem::MaybeUninit;
use std::ptr::NonNull;
use std::sync::{Mutex, PoisonError};

/// One static's place, shared by every generation that declares it alike.
#[derive(Clone)]
pub struct Slot(Shared<Mutex<State>>);

enum State {
    /// Not written yet. `memory` is where its value will be, once a
    /// sibling's initializer took its address.
    Reserved { memory: Option<Memory> },
    /// Written: its place, which marked code reads.
    Settled(Place),
}

/// Leaked memory for one value of the type `ty`, not written yet.
struct Memory {
    pointer: NonNull<u8>,
    ty: TypeId,
    /// The memory as a `*const dyn Any` of its type, once written.
    view: fn(NonNull<u8>) -> *const dyn Any,
}

// SAFETY: `Memory` is an address of leaked memory and no value. It is only
// written by `Slot::settle` and read through the references `Slot::address`
// gives, as a static's value is.
unsafe impl Send for Memory {}

impl Memory {
    fn of<T: 'static>() -> Self {
        let memory: &'static mut MaybeUninit<T> = Box::leak(Box::new(MaybeUninit::uninit()));
        Self {
            pointer: NonNull::from(memory).cast(),
            ty: TypeId::of::<T>(),
            view: |pointer| pointer.cast::<T>().as_ptr().cast_const(),
        }
    }
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
    let reserve = || Slot(Shared::new(Mutex::new(State::Reserved { memory: None })));
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
        matches!(*self.state(), State::Reserved { .. })
    }

    /// The static as marked code reads it: its place once written. Before,
    /// the slot itself, whose address a `&'static` parameter takes.
    pub fn value(&self) -> Dynamic {
        match &*self.state() {
            State::Settled(place) => Dynamic::from(place.clone()),
            State::Reserved { .. } => Dynamic::from(self.clone()),
        }
    }

    /// Write the static's value, the native value `value` holds, which
    /// moves out of the runner's arena into the static's memory: the memory
    /// a sibling took the address of, or a new allocation, leaked.
    pub fn settle(&self, value: Dynamic) -> RawrResultOf<()> {
        let (value, witness) = taken(&value)?;
        let witness =
            witness.ok_or_else(|| failure("a static's native value has no type witness"))?;
        let mut state = self.state();
        let State::Reserved { memory } = &mut *state else {
            return Err(failure("a static is written once"));
        };
        let value: &'static dyn Any = match memory.take() {
            None => Box::leak(value),
            Some(memory) => {
                if (*value).type_id() != memory.ty {
                    return Err(failure("mismatched types (E0308)"));
                }
                let layout = std::alloc::Layout::for_value(&*value);
                let source: *mut dyn Any = Box::into_raw(value);
                // SAFETY: `memory` was allocated for a value of this very
                // type, so of its layout, and nothing has written it. The
                // value's bytes move there, and its box is freed without
                // dropping it, as a move does.
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        source.cast::<u8>(),
                        memory.pointer.as_ptr(),
                        layout.size(),
                    );
                    if layout.size() != 0 {
                        std::alloc::dealloc(source.cast(), layout);
                    }
                }
                // SAFETY: the memory now holds the value, and it is never
                // freed, moved or written again.
                unsafe { &*(memory.view)(memory.pointer) }
            }
        };
        *state = State::Settled(Place {
            witness,
            storage: Shared::new(Leaked(value)),
            mutable: false,
        });
        Ok(())
    }

    /// The static's `&'static T`: its value once written, or else the
    /// memory its value will be in, which nothing reads before it is.
    fn address<T: 'static>(&self) -> Option<&'static T> {
        let mut state = self.state();
        match &mut *state {
            State::Settled(place) => {
                let place = place.clone();
                drop(state);
                lasting_place(&place)
            }
            State::Reserved { memory } => {
                let memory = memory.get_or_insert_with(Memory::of::<T>);
                if memory.ty != TypeId::of::<T>() {
                    return None;
                }
                // SAFETY: the memory is leaked, so it lives as long as the
                // program, and it is for a `T`. It holds no value yet: the
                // reference only names where the sibling's value will be,
                // and the block's initializers don't read it before
                // `settle` writes it. Miri's aliasing models reject the
                // write after this reference exists (D862's check), as they
                // reject any cycle of statics built at run time.
                Some(unsafe { memory.pointer.cast::<T>().as_ref() })
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
