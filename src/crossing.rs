//! Script types and functions where native generic code needs Rust types
//! (D594).
//!
//! A generic Rust item a script calls with its own types, as
//! `iced::run(update, view)` is called with the program's `State`,
//! `Message`, `update` and `view`, is built in the runner at Rust types the
//! runner generates for them:
//!
//! - A script type is a [`Proxy`] newtype named as the script names it, in
//!   the project's own crate, so `type_name` and `TypeId` are its own and a
//!   bound such as `Default` is its own implementation's. The proxy holds a
//!   shared handle to the script value, so native code that lends it to
//!   script code lends the value itself, and nothing is copied.
//! - A script function is a [`Slot`]: a function item the runner
//!   generates, which calls the script function in the generation current
//!   where native code calls it (D553). Two functions are two slots, of
//!   two Rust types, as two Rust functions are, and each is zero-sized.
//!
//! The runner's generated code reaches script code only through this
//! module; it names no crate (D497).

use crate::{Dynamic, RawrResultOf};

/// A script value where native generic code takes a Rust type: what a
/// runner's newtype for a script type holds (D594).
///
/// It holds the value by a shared handle. Native code that lends the
/// proxy to a script function, as `view(&state)` or `update(&mut state,
/// message)`, lends the handle, so the function reads and changes the
/// value itself. Script strings that native values borrow from it while
/// it is lent shared are held in its [`Pins`](crate::scope::Pins).
pub struct Proxy {
    value: Dynamic,
    pins: crate::scope::Pins,
}

impl Proxy {
    /// The proxy of `value`, which it now owns.
    #[must_use]
    pub fn new(value: Dynamic) -> Self {
        Self {
            value: value.into_shared(),
            pins: crate::scope::Pins::new(),
        }
    }

    /// The proxy of the script value an argument's `place` holds: a handle
    /// to it when script code shares it, as a borrow of `self` or of a
    /// state native code lent does (`Canvas::new(self)`), and else the
    /// value itself, which the call takes.
    pub fn from_place(place: &mut Dynamic) -> Self {
        if place.is_shared() {
            Self::new(place.clone())
        } else {
            Self::new(place.take())
        }
    }

    /// The proxy of a value `make` gives where it can, which is made the
    /// first time the value is used where `make` gives `Some`. A script
    /// function native code calls on a thread with no arena for the native
    /// values it takes, as iced's executor calls `Message::Loaded` for
    /// `Task::map`, runs so on the owner thread, where its message is
    /// delivered.
    #[must_use]
    pub fn deferred(make: impl FnMut() -> Option<Dynamic> + Send + 'static) -> Self {
        Self::new(Dynamic::from(Deferred(std::sync::Arc::new(
            std::sync::Mutex::new(Made::Pending(Box::new(make))),
        ))))
    }

    /// Make a deferred value where it can be made, and hold it from then on.
    fn settle(&self) {
        let Some(deferred) = self.value.read_lock::<Deferred>().map(|held| held.clone()) else {
            return;
        };
        let mut made = deferred
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let value = match &mut *made {
            Made::Made(value) => value.clone(),
            Made::Pending(make) => match make() {
                Some(value) => {
                    *made = Made::Made(value.clone());
                    value
                }
                None => return,
            },
        };
        drop(made);
        // Another handle to the one shared value, which it writes.
        let mut handle = self.value.clone();
        if let Some(mut held) = handle.write_lock::<Dynamic>() {
            *held = value.flatten();
        }
    }

    /// The value, given back to script code: the value itself when no
    /// handle to it remains elsewhere.
    pub fn into_dynamic(self) -> Dynamic {
        self.settle();
        self.value.flatten()
    }

    /// The value, lent shared to a script function for one call, as
    /// `view(&state)` takes it.
    pub fn lend(&self) -> Dynamic {
        self.settle();
        self.value.clone()
    }

    /// The value, lent `&mut` to a script function for one call, as
    /// `update(&mut state, message)` takes it. No native value can borrow
    /// a string of it any longer, so its pins are let go.
    pub fn lend_mut(&mut self) -> Dynamic {
        self.settle();
        self.pins.clear();
        self.value.clone()
    }

    /// Store what a script function that took the value `&mut` left in
    /// its place: the value it changed in place through the lent handle,
    /// or one it assigned whole.
    pub fn restore(&mut self, value: Dynamic) {
        self.settle();
        if shares(&self.value, &value) {
            return;
        }
        let value = value.flatten();
        if let Some(mut held) = self.value.write_lock::<Dynamic>() {
            *held = value;
            return;
        }
        self.value = value.into_shared();
    }

    /// The strings native values borrow from the value while it is lent
    /// shared.
    #[must_use]
    pub const fn pins(&self) -> &crate::scope::Pins {
        &self.pins
    }

    /// The `Default` of the script type at `path` (`State`, `app::State`):
    /// its own `impl Default`, or the derived one. A panic when it has
    /// none, as rustc would refuse the call.
    #[must_use]
    pub fn default_of(path: &str) -> Self {
        match default_of(path) {
            Ok(value) => Self::new(value),
            Err(error) => panic!("{}", crate::future::message(&error)),
        }
    }
}

/// A value [`Proxy::deferred`] makes, shared by the proxy's copies.
#[derive(Clone)]
struct Deferred(std::sync::Arc<std::sync::Mutex<Made>>);

enum Made {
    Pending(Box<dyn FnMut() -> Option<Dynamic> + Send>),
    Made(Dynamic),
}

/// Whether `a` and `b` are handles to one shared value.
fn shares(a: &Dynamic, b: &Dynamic) -> bool {
    use crate::types::dynamic::Union;
    match (&a.0, &b.0) {
        (Union::Shared(a, ..), Union::Shared(b, ..)) => crate::Shared::ptr_eq(a, b),
        _ => false,
    }
}

impl Clone for Proxy {
    /// The script type's own `Clone` where the program wrote one, and else
    /// a copy, as a derived `Clone` makes. Rust code clones where it calls
    /// `clone`, so this copy is the one Rust makes.
    fn clone(&self) -> Self {
        self.settle();
        let copy = self.value.flatten_clone();
        Self::new(match crate::future::clone_value(&copy) {
            Some(Ok(cloned)) => cloned,
            Some(Err(error)) => panic!("{}", crate::future::message(&error)),
            None => copy,
        })
    }
}

impl PartialEq for Proxy {
    /// The script type's own `PartialEq` where the program wrote one, and
    /// else as a derived one compares: Rawr's checker holds a script type
    /// a native bound asks `PartialEq` of to having it.
    fn eq(&self, other: &Self) -> bool {
        self.settle();
        other.settle();
        let (lhs, rhs) = (self.value.flatten_clone(), other.value.flatten_clone());
        match crate::future::equal(&lhs, &rhs) {
            Some(Ok(equal)) => equal,
            Some(Err(error)) => panic!("{}", crate::future::message(&error)),
            None => crate::host::value::structural_equal(&lhs, &rhs),
        }
    }
}

impl Proxy {
    /// The value's text as its script type's own `Display` writes it, as
    /// a runner's newtype's `Display` gives it, and else its `Debug` text.
    ///
    /// # Errors
    ///
    /// When the formatter fails.
    pub fn display(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.settle();
        let value = self.value.flatten_clone();
        match crate::future::display(&value) {
            Some(Ok(text)) => f.write_str(&text),
            Some(Err(error)) => panic!("{}", crate::future::message(&error)),
            None => std::fmt::Debug::fmt(self, f),
        }
    }
}

impl std::fmt::Debug for Proxy {
    /// The script type's derived `Debug` text.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.settle();
        match self.value.read_lock::<Dynamic>() {
            Some(value) => match crate::future::debug(&value) {
                Some(text) => f.write_str(&text),
                None => std::fmt::Debug::fmt(&*value, f),
            },
            None => std::fmt::Debug::fmt(&self.value, f),
        }
    }
}

/// The `Default` of the script type at `path`, in the generation current
/// on this thread.
pub fn default_of(path: &str) -> RawrResultOf<Dynamic> {
    let _entry = crate::future::entry();
    let program = crate::future::current_program()
        .ok_or_else(|| crate::future::error("rawr: no program runs script code on this thread"))?;
    program.default_of(path).unwrap_or_else(|| {
        Err(crate::future::error(format!(
            "rawr: the script type `{path}` has no `Default`"
        )))
    })
}

/// Call the function the running program holds at `path`, its owner's
/// path and the member (`Arc::Program<Message>::draw`), with `arguments`:
/// a function of a script impl, which a runner's impl for the owner's proxy
/// calls where native code calls it (D594). It runs in the generation
/// current on this thread, so an edit to its body takes effect at the next
/// call. Each index in `mutable` is an argument it takes `&mut`, as in
/// [`Slot::call`]. A panic with the reason when it fails, as a Rust
/// function that fails does.
pub fn call_member(path: &str, arguments: &mut [Dynamic], mutable: &[usize]) -> Dynamic {
    let _entry = crate::future::entry();
    let called = crate::future::current_program()
        .ok_or_else(|| crate::future::error("rawr: no program runs script code on this thread"))
        .and_then(|program| {
            let function = program.member(path).ok_or_else(|| {
                crate::future::error(format!("rawr: the running program has no `{path}`"))
            })?;
            crate::future::Callable::of_item(function, program).call_places(arguments, mutable)
        });
    called.unwrap_or_else(|error| panic!("{}", crate::future::message(&error)))
}

/// The witness of each parameter of the function the running program
/// holds at `path`, as [`call_member`] calls it, where it has one: a native
/// value native code gives the function is attested as its parameter's type,
/// so script code reads it as that type.
#[must_use]
pub fn member_witnesses(path: &str) -> Vec<Option<crate::host::runtime::Witness>> {
    crate::future::current_program()
        .map(|program| program.member_witnesses(path))
        .unwrap_or_default()
}

/// A script function where native generic code takes a callable: what a
/// runner's generated function item calls (D594). The runner keeps one
/// per script function, set each time script code passes the function,
/// and the function item reads it, so the item itself captures nothing.
pub struct Slot(std::sync::RwLock<Option<crate::future::Callable>>);

impl Default for Slot {
    fn default() -> Self {
        Self::new()
    }
}

impl Slot {
    /// A slot no function fills yet.
    #[must_use]
    pub const fn new() -> Self {
        Self(std::sync::RwLock::new(None))
    }

    /// Fill the slot with the script function `function`.
    pub fn set(&self, function: Dynamic) -> RawrResultOf<()> {
        let callable = crate::future::Callable::new(function)?;
        *self
            .0
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(callable);
        Ok(())
    }

    /// Call the slot's function with `arguments`. Each index in `mutable`
    /// is an argument the function takes `&mut`: it is the function's own
    /// place afterwards, changed in place or given back.
    pub fn call(&self, arguments: &mut [Dynamic], mutable: &[usize]) -> RawrResultOf<Dynamic> {
        let callable = self
            .0
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .ok_or_else(|| crate::future::error("rawr: a slot was called before it was filled"))?;
        callable.call_places(arguments, mutable)
    }

    /// [`Slot::call`], or a panic with the error's message, as a Rust
    /// function that fails does.
    pub fn invoke(&self, arguments: &mut [Dynamic], mutable: &[usize]) -> Dynamic {
        self.call(arguments, mutable)
            .unwrap_or_else(|error| panic!("{}", crate::future::message(&error)))
    }
}
