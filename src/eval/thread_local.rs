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

use std::cell::RefCell;

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
}

/// One key's value on a thread.
struct Slot {
    key: ImmutableString,
    value: Dynamic,
    glue: Option<FnPtr>,
}

std::thread_local! {
    /// This thread's values, in the order made.
    static SLOTS: RefCell<Vec<Slot>> = const { RefCell::new(Vec::new()) };
}

impl LocalKey {
    pub(crate) fn new(key: ImmutableString, initializer: FnPtr, glue: Option<FnPtr>) -> Self {
        Self {
            key,
            initializer,
            glue,
        }
    }

    /// The key's value on this thread, which its initializer makes, through
    /// `context`, at its first access here.
    pub(crate) fn value(&self, context: &NativeCallContext<'_>) -> RawrResult {
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

    /// Whether the key has no value on this thread yet.
    pub(crate) fn vacant(&self) -> bool {
        self.find().is_none()
    }

    /// Give the key `value` on this thread, where it has none yet, without
    /// its initializer: what std's `set` does before a first access.
    pub(crate) fn put(&self, value: Dynamic) {
        SLOTS.with_borrow_mut(|slots| {
            slots.push(Slot {
                key: self.key.clone(),
                value,
                glue: self.glue.clone(),
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

/// End this thread's values, the last made first, each of a type with drop
/// glue dropped through `context`, as std runs a thread's destructors when
/// it ends. A destructor that accesses a key makes its value again, which
/// drops in turn.
pub(crate) fn end(context: &NativeCallContext<'_>) -> RawrResultOf<()> {
    while let Some(slot) = SLOTS.with_borrow_mut(Vec::pop) {
        if let Some(glue) = slot.glue {
            let _ = glue.call_raw(context, None, [slot.value])?;
        }
    }
    Ok(())
}
