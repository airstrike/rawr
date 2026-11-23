//! The values of the program's `thread_local!` keys, each thread's own, as
//! std keeps them: a key's value on a thread is made by its initializer at
//! its first access there, and a thread's values drop as it ends, the last
//! made first, as std runs a thread's destructors.
//!
//! A key is found by its path and a fingerprint of its type and its
//! initializer, as a kept static is (#2519, D862). A generation that
//! declares the key alike finds the value an earlier one made on the same
//! thread, and a changed declaration has a new key, so it starts anew.
//!
//! A thread's values live in the operating system thread's own storage, so
//! a thread that ends frees them. One of a type with drop glue is dropped
//! by the program's glue as the thread ends (`end`): a spawned thread's as
//! its closure returns (`eval::threads`), and the main thread's as `main`
//! returns, as std's exit runs them.
//!
//! As std's storage does, a key whose type has drop glue in Rust is
//! destroyed as its thread ends, and its key is then refused: `try_with`
//! gives std's `AccessError`, and each other access panics as std's
//! `with` does. A key of another type keeps its value to the thread's end.

use std::cell::RefCell;
use std::collections::BTreeSet;

use crate::func::NativeCallContext;
use crate::{Dynamic, FnPtr, ImmutableString, RawrResult, RawrResultOf};

/// A `LocalKey<T>`: what finds the key's value on a thread, and makes it.
#[derive(Clone)]
pub(crate) struct LocalKey {
    key: ImmutableString,
    /// Makes the key's value, at its first access on a thread.
    initializer: FnPtr,
    /// Drops a value of a type with drop glue as its thread ends.
    glue: Option<FnPtr>,
    end: End,
}

/// What a thread's end does to a key's value, as std's storage for the
/// key's type does.
#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) enum End {
    /// A type with drop glue in Rust: the value is destroyed, the last
    /// made first, and the key is refused after.
    Destroyed,
    /// Any other type: the value is kept to the thread's end.
    Kept,
}

/// One key's value on a thread.
struct Slot {
    key: ImmutableString,
    value: Dynamic,
    glue: Option<FnPtr>,
    end: End,
}

std::thread_local! {
    /// This thread's values, in the order made.
    static SLOTS: RefCell<Vec<Slot>> = const { RefCell::new(Vec::new()) };
    /// The keys whose values this thread's end destroyed, or is destroying.
    static ENDED: RefCell<BTreeSet<ImmutableString>> = const { RefCell::new(BTreeSet::new()) };
}

/// std's panic of an access to a key its thread's end destroyed.
const ENDED_ACCESS: &str =
    "cannot access a Thread Local Storage value during or after destruction: AccessError";

impl LocalKey {
    pub(crate) fn new(
        key: ImmutableString,
        initializer: FnPtr,
        glue: Option<FnPtr>,
        end: End,
    ) -> Self {
        Self {
            key,
            initializer,
            glue,
            end,
        }
    }

    /// Whether this thread's end destroyed the key's value, or is
    /// destroying it: what `try_with` refuses.
    pub(crate) fn ended(&self) -> bool {
        ENDED.with_borrow(|ended| ended.contains(&self.key))
    }

    /// The key's value on this thread, which its initializer makes, through
    /// `context`, at its first access here; std's panic once the thread's
    /// end destroyed it.
    pub(crate) fn value(&self, context: &NativeCallContext<'_>) -> RawrResult {
        if self.ended() {
            return Err(ended_access());
        }
        if let Some(value) = self.find() {
            return Ok(value);
        }
        let value = self.initializer.call_raw(context, None, [])?;
        // An initializer that accessed its own key made a value first,
        // which the one it returns replaces, as std's lazy storage does.
        if let Some(replaced) = self.take() {
            self.drop_value(context, replaced)?;
        }
        self.put(value.clone());
        Ok(value)
    }

    /// Whether the key has no value on this thread yet; std's panic of
    /// `set` once the thread's end destroyed it.
    pub(crate) fn vacant(&self) -> RawrResultOf<bool> {
        if self.ended() {
            return Err(ended_access());
        }
        Ok(self.find().is_none())
    }

    /// Give the key `value` on this thread, where it has none yet, without
    /// its initializer: what std's `set` does before a first access.
    pub(crate) fn put(&self, value: Dynamic) {
        SLOTS.with_borrow_mut(|slots| {
            slots.push(Slot {
                key: self.key.clone(),
                value,
                glue: self.glue.clone(),
                end: self.end,
            });
        });
    }

    fn find(&self) -> Option<Dynamic> {
        SLOTS.with_borrow(|slots| {
            slots
                .iter()
                .find(|slot| slot.key == self.key)
                .map(|slot| slot.value.clone())
        })
    }

    fn take(&self) -> Option<Dynamic> {
        SLOTS.with_borrow_mut(|slots| {
            let index = slots.iter().position(|slot| slot.key == self.key)?;
            Some(slots.remove(index).value)
        })
    }

    fn drop_value(&self, context: &NativeCallContext<'_>, value: Dynamic) -> RawrResultOf<()> {
        if let Some(glue) = &self.glue {
            let _ = glue.call_raw(context, None, [value])?;
        }
        Ok(())
    }
}

/// End this thread's values: each a thread's end destroys, the last made
/// first, its key refused from before its drop, and each of a type with
/// drop glue dropped through `context`, as std runs a thread's destructors
/// when it ends. A destructor that accesses a key not made yet makes its
/// value, which is destroyed in turn. Then the thread holds nothing, so a
/// later run on it starts anew.
pub(crate) fn end(context: &NativeCallContext<'_>) -> RawrResultOf<()> {
    while let Some(slot) = SLOTS.with_borrow_mut(|slots| {
        let last = slots.iter().rposition(|slot| slot.end == End::Destroyed)?;
        Some(slots.remove(last))
    }) {
        ENDED.with_borrow_mut(|ended| ended.insert(slot.key));
        if let Some(glue) = slot.glue {
            let _ = glue.call_raw(context, None, [slot.value])?;
        }
    }
    SLOTS.with_borrow_mut(Vec::clear);
    ENDED.with_borrow_mut(BTreeSet::clear);
    Ok(())
}

fn ended_access() -> Box<crate::EvalAltResult> {
    crate::EvalAltResult::ErrorRuntime(ENDED_ACCESS.into(), crate::Position::NONE).into()
}
