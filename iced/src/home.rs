//! The native home: the native values an application's State holds by
//! handle, owned by the application instance (D546).
//!
//! iced's widgets that borrow app state (`text_editor(&self.content)`,
//! `combo_box(&self.state, ..)`) need the value to outlive the element, and
//! `update` needs it mutably between views. A script State is a `Dynamic`,
//! so it holds a counted [`Handle`] and the value lives here:
//!
//! - A transition (boot, `update`, a view evaluation) gets `&mut` through
//!   [`enter`], which moves the home into this thread for the call and back
//!   out when it returns. The facade's native methods reach it with
//!   [`with_mut`] and [`get`].
//! - A frame gets `&'a Home` through the lowering context, so an element
//!   borrows a value for iced's view lifetime with a plain safe borrow.
//! - Script code a frame runs (a style function, a component's view, a
//!   widget's hook) reaches the home through the call's loan
//!   (`widget::custom::lend`), shared only: asking it for `&mut` is a borrow
//!   error, as in Rust.
//!
//! A slot lives while any handle to it does: the State, the last-good view
//! and a retained descriptor each hold one. [`Home::release`] frees the rest
//! at the end of each transition. The Adapter's State rollback does not undo
//! a change made in place (`Content::perform`); D481.4 accepted the same of
//! native effects.

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Weak};

use crate::widget::custom::lend;

/// The native values of one application instance.
#[derive(Default)]
pub struct Home {
    slots: HashMap<u64, Slot>,
    /// Whether this is the place of a home moved into a transition.
    lent: bool,
}

struct Slot {
    key: Weak<Key>,
    value: Box<dyn Any + Send>,
}

/// A counted reference to one value in a [`Home`]. A script value holds
/// it; copies share the slot, which is freed once the last copy drops.
#[derive(Clone)]
pub(crate) struct Handle(Arc<Key>);

struct Key {
    id: u64,
}

/// Why a native value could not be reached.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub(crate) enum Error {
    #[error("cannot borrow `{0}` as mutable, because the view borrows it as immutable (E0502)")]
    Borrowed(&'static str),
    #[error("`{0}` is no longer held by this application")]
    Stale(&'static str),
    #[error("`{0}` is not this type")]
    Type(&'static str),
    #[error("`{0}` is not lent to this call: the application's native values are reachable from its boot, `update`, view and the hooks a view runs")]
    Unavailable(&'static str),
}

thread_local! {
    /// The homes moved in for the transitions running on this thread, the
    /// newest last.
    static ENTERED: RefCell<Vec<Home>> = const { RefCell::new(Vec::new()) };
}

impl Home {
    /// The place of a home moved into a transition.
    fn placeholder() -> Self {
        Self {
            slots: HashMap::new(),
            lent: true,
        }
    }

    /// Store `value` and return its first handle.
    pub(crate) fn insert<T: Any + Send>(&mut self, value: T) -> Handle {
        // 0 is the orphan's.
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let key = Arc::new(Key {
            id: NEXT.fetch_add(1, Ordering::Relaxed),
        });
        self.slots.insert(
            key.id,
            Slot {
                key: Arc::downgrade(&key),
                value: Box::new(value),
            },
        );
        Handle(key)
    }

    /// The value `handle` names, borrowed for as long as the home is.
    pub(crate) fn get<T: Any>(&self, handle: &Handle) -> Result<&T, Error> {
        let slot = self
            .slots
            .get(&handle.0.id)
            .ok_or(Error::Stale(short::<T>()))?;
        slot.value
            .downcast_ref::<T>()
            .ok_or(Error::Type(short::<T>()))
    }

    fn get_mut<T: Any>(&mut self, handle: &Handle) -> Result<&mut T, Error> {
        let slot = self
            .slots
            .get_mut(&handle.0.id)
            .ok_or(Error::Stale(short::<T>()))?;
        slot.value
            .downcast_mut::<T>()
            .ok_or(Error::Type(short::<T>()))
    }

    fn holds(&self, handle: &Handle) -> bool {
        self.slots.contains_key(&handle.0.id)
    }

    /// Free each slot whose last handle dropped.
    pub(crate) fn release(&mut self) {
        self.slots.retain(|_, slot| slot.key.strong_count() > 0);
    }

    /// Take every slot of `other`: the home of the application this one
    /// continues (an edit kept its State).
    pub(crate) fn absorb(&mut self, other: Self) {
        self.slots.extend(other.slots);
    }

    /// The number of values held.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.slots.len()
    }
}

impl Handle {
    /// A handle to no value: what a native value made outside a transition
    /// holds. Every use of it is [`Error::Stale`].
    pub(crate) fn orphan() -> Self {
        Self(Arc::new(Key { id: 0 }))
    }
}

impl PartialEq for Handle {
    fn eq(&self, other: &Self) -> bool {
        self.0.id == other.0.id
    }
}

impl Eq for Handle {}

impl std::fmt::Debug for Handle {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "Handle({})", self.0.id)
    }
}

/// Run `f` with `home` moved into this thread, so the native values it
/// holds are reachable mutably, and move it back when `f` returns. The
/// slots no handle reaches any more are freed on the way out. Inside a
/// transition that already moved `home` in, `f` just runs.
pub(crate) fn enter<R>(home: &mut Home, f: impl FnOnce() -> R) -> R {
    struct Leave<'h>(&'h mut Home);
    impl Drop for Leave<'_> {
        fn drop(&mut self) {
            if let Some(mut home) = ENTERED.with_borrow_mut(Vec::pop) {
                home.release();
                *self.0 = home;
            }
        }
    }
    if home.lent {
        return f();
    }
    let moved = std::mem::replace(home, Home::placeholder());
    ENTERED.with_borrow_mut(|entered| entered.push(moved));
    let _leave = Leave(home);
    f()
}

/// Run `f` on `owner` with the home `home` finds in it moved into this
/// thread, as [`enter`] does. A transition inside another of the same
/// owner (`update` settling its view) runs in the home already moved in.
pub(crate) fn transition<A, R>(
    owner: &mut A,
    home: fn(&mut A) -> &mut Home,
    f: impl FnOnce(&mut A) -> R,
) -> R {
    /// Pops the home this transition moved in, also when `f` unwinds.
    struct Moved(bool);
    impl Drop for Moved {
        fn drop(&mut self) {
            if self.0 {
                ENTERED.with_borrow_mut(Vec::pop);
            }
        }
    }
    let place = home(owner);
    if place.lent {
        return f(owner);
    }
    let moved = std::mem::replace(place, Home::placeholder());
    ENTERED.with_borrow_mut(|entered| entered.push(moved));
    let mut guard = Moved(true);
    let result = f(owner);
    guard.0 = false;
    if let Some(mut moved) = ENTERED.with_borrow_mut(Vec::pop) {
        moved.release();
        *home(owner) = moved;
    }
    result
}

/// Store `value` in the home of the transition running now.
pub(crate) fn insert<T: Any + Send>(value: T) -> Result<Handle, Error> {
    ENTERED.with_borrow_mut(|entered| {
        entered
            .last_mut()
            .map(|home| home.insert(value))
            .ok_or(Error::Unavailable(short::<T>()))
    })
}

/// Read the value `handle` names: from the transition's home, or from the
/// home the running call borrows.
pub(crate) fn get<T: Any, R>(handle: &Handle, f: impl FnOnce(&T) -> R) -> Result<R, Error> {
    let mut f = Some(f);
    let mut read = |value: &T| f.take().map(|f| f(value));
    let entered = ENTERED.with_borrow(|entered| {
        entered
            .iter()
            .rev()
            .find(|home| home.holds(handle))
            .map(|home| home.get::<T>(handle).map(&mut read))
    });
    let read = match entered {
        Some(read) => read,
        None => lend::with_home(|home| match home {
            Some(home) => home.get::<T>(handle).map(&mut read),
            None => Err(Error::Unavailable(short::<T>())),
        }),
    }?;
    read.ok_or(Error::Stale(short::<T>()))
}

/// Change the value `handle` names in place. Only a transition may: a
/// frame's call borrows the home shared, so asking is a borrow error.
pub(crate) fn with_mut<T: Any, R>(
    handle: &Handle,
    f: impl FnOnce(&mut T) -> R,
) -> Result<R, Error> {
    let found = ENTERED.with_borrow_mut(|entered| {
        entered
            .iter_mut()
            .rev()
            .find(|home| home.holds(handle))
            .map(|home| home.get_mut::<T>(handle).map(f))
    });
    match found {
        Some(result) => result,
        None => lend::with_home(|home| match home {
            Some(home) if home.holds(handle) => Err(Error::Borrowed(short::<T>())),
            Some(_) => Err(Error::Stale(short::<T>())),
            None => Err(Error::Unavailable(short::<T>())),
        }),
    }
}

/// The last two segments of `T`'s path, as a message names a type.
fn short<T: ?Sized>() -> &'static str {
    let name = std::any::type_name::<T>();
    let name = name.split('<').next().unwrap_or(name);
    let mut segments = name.rsplitn(3, "::");
    let last = segments.next().unwrap_or(name);
    match segments.next() {
        Some(module) => {
            let at = name.len() - last.len() - module.len() - 2;
            &name[at..]
        }
        None => last,
    }
}

#[cfg(test)]
mod tests {
    use super::{enter, get, insert, with_mut, Error, Home};

    #[test]
    fn a_transition_reaches_its_home_and_the_last_handle_frees_a_slot() {
        let mut home = Home::default();
        let handle = enter(&mut home, || insert(String::from("abc")).unwrap());
        assert_eq!(home.len(), 1);
        enter(&mut home, || {
            with_mut::<String, _>(&handle, |text| text.push('d')).unwrap();
            assert_eq!(get::<String, _>(&handle, Clone::clone).unwrap(), "abcd");
        });
        assert_eq!(home.get::<String>(&handle).unwrap(), "abcd");
        let copy = handle.clone();
        drop(handle);
        enter(&mut home, || {});
        assert_eq!(home.len(), 1, "a copy keeps the slot");
        drop(copy);
        enter(&mut home, || {});
        assert_eq!(home.len(), 0);
    }

    #[test]
    fn a_nested_transition_runs_in_the_home_already_moved_in() {
        struct Owner {
            home: Home,
        }
        fn home(owner: &mut Owner) -> &mut Home {
            &mut owner.home
        }
        let mut owner = Owner {
            home: Home::default(),
        };
        let handle = super::transition(&mut owner, home, |owner| {
            let handle = insert(7_u8).unwrap();
            super::transition(owner, home, |_| {
                with_mut::<u8, _>(&handle, |value| *value += 1).unwrap();
            });
            handle
        });
        assert_eq!(owner.home.get::<u8>(&handle), Ok(&8));
        assert!(!owner.home.lent);
    }

    #[test]
    fn outside_a_transition_nothing_is_lent() {
        let mut home = Home::default();
        let handle = enter(&mut home, || insert(1_u32).unwrap());
        assert_eq!(
            get::<u32, _>(&handle, |value| *value),
            Err(Error::Unavailable("u32"))
        );
        assert_eq!(insert(2_u32).err(), Some(Error::Unavailable("u32")));
    }

    #[test]
    fn a_frame_lends_its_home_shared_and_refuses_a_change() {
        let mut home = Home::default();
        let handle = enter(&mut home, || insert(5_i64).unwrap());
        let _loan = crate::widget::custom::lend::Loan::home(&home);
        assert_eq!(get::<i64, _>(&handle, |value| *value), Ok(5));
        assert_eq!(
            with_mut::<i64, _>(&handle, |value| *value += 1),
            Err(Error::Borrowed("i64"))
        );
    }

    #[test]
    fn a_type_is_named_by_its_last_two_segments() {
        assert_eq!(super::short::<String>(), "string::String");
        assert_eq!(super::short::<u8>(), "u8");
    }
}
