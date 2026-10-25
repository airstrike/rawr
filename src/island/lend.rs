//! A value its owner lends, while a native call it makes runs, to the code
//! that call calls back on the same thread (D551.6). The generated runner's
//! arena is the one value lent: an adapter holds it while it calls native
//! code, and Freya's `launch` calls a script closure before it returns,
//! whose own native calls need the arena. The pattern is D481's loan.

use std::any::TypeId;
use std::cell::Cell;

thread_local! {
    /// The lent value's type and place, while it is lent and not in use.
    static LENT: Cell<Option<(TypeId, *mut ())>> = const { Cell::new(None) };
}

/// Puts back what [`LENT`] held before.
struct Restore(Option<(TypeId, *mut ())>);

impl Drop for Restore {
    fn drop(&mut self) {
        LENT.with(|lent| lent.set(self.0));
    }
}

/// Run `call` with `value` lent to [`with`] on this thread. `value` stays
/// borrowed until `call` returns, so its owner can't use it meanwhile.
pub fn lend<T: 'static, R>(value: &mut T, call: impl FnOnce() -> R) -> R {
    let place = std::ptr::from_mut(value).cast::<()>();
    let _restore = Restore(LENT.with(|lent| lent.replace(Some((TypeId::of::<T>(), place)))));
    call()
}

/// `call` with the `T` a [`lend`] on this thread lends. `None` when no `T`
/// is lent, or when it is in use further up this thread's stack.
pub fn with<T: 'static, R>(call: impl FnOnce(&mut T) -> R) -> Option<R> {
    // Taken while it is in use, so a nested use finds none.
    let lent = LENT.with(Cell::take)?;
    let _restore = Restore(Some(lent));
    if lent.0 != TypeId::of::<T>() {
        return None;
    }
    // SAFETY: `lend` made this pointer from a `&mut T` it holds until the
    // native call that led here returns, on this thread, so the value is
    // alive and its owner doesn't use it. `LENT` is empty while this
    // reference lives, so no other reference to the value is made, and the
    // reference lives only for `call`.
    Some(call(unsafe { &mut *lent.1.cast::<T>() }))
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_lent_value_reaches_a_nested_call_once() {
        let mut value = 1_i32;
        let seen = super::lend(&mut value, || {
            super::with::<i32, _>(|value| {
                *value += 1;
                // In use: a nested use finds none, and another type none.
                (
                    super::with::<i32, _>(|_| ()).is_none(),
                    super::with::<u8, _>(|_| ()).is_none(),
                )
            })
        });
        assert_eq!(seen, Some((true, true)));
        assert_eq!(value, 2);
        assert!(super::with::<i32, _>(|_| ()).is_none());
    }
}
