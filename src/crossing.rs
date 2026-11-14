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
//!   bound such as `Default` is its own implementation's. A universe
//!   runner (`rawr_import::universe`), which serves many programs, names it
//!   by its index in the program's definition order (`__RawrScript0`, ..)
//!   instead, and finds its `Default` by that index. A module with its own
//!   `main` is a program of its own there, whose types count from zero. The proxy holds a
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

/// The variants of a script enum whose variants carry nothing and which is
/// `Copy`, in the order of their discriminants, with the path the runner
/// names it by (#1940).
pub struct Variants {
    path: &'static str,
    names: &'static [&'static str],
}

impl Variants {
    /// The variants `names` of the script enum at `path`.
    #[must_use]
    pub const fn new(path: &'static str, names: &'static [&'static str]) -> Self {
        Self { path, names }
    }
}

/// A value of a script enum whose variants carry nothing and which is
/// `Copy`, where native generic code takes a Rust type: which of its
/// [`Variants`] it is. It holds no script value, so it is `Copy` as the
/// script type is, and `radio`'s `V: Copy + Eq` holds of it (#1940).
///
/// It is made from a script value and makes one wherever script code takes
/// it, with the method names a [`Proxy`] has, so the runner lends it as it
/// lends a proxy.
#[derive(Clone, Copy)]
pub struct Variant {
    of: &'static Variants,
    index: usize,
}

impl Variant {
    /// Which of `of` the script value `value` is. A panic when it is none
    /// of them: the runner gives it only values of its type.
    #[must_use]
    pub fn new(of: &'static Variants, value: &Dynamic) -> Self {
        let value = value.flatten_clone();
        let index = value
            .read_lock::<crate::types::nominal::Enum>()
            .and_then(|made| {
                let name = made.variant().name();
                of.names.iter().position(|known| *known == name)
            });
        match index {
            Some(index) => Self { of, index },
            None => panic!("rawr: {value:?} is no variant of `{}`", of.path),
        }
    }

    /// Which of `of` the value `proxy` holds is.
    #[must_use]
    pub fn from_proxy(of: &'static Variants, proxy: Proxy) -> Self {
        Self::new(of, &proxy.into_dynamic())
    }

    /// Its position among its type's variants, in the order of their
    /// discriminants, which a derived `PartialEq`, `Ord` or `Hash` reads.
    #[must_use]
    pub const fn index(self) -> usize {
        self.index
    }

    /// The script value, in the generation current on this thread.
    pub fn into_dynamic(self) -> Dynamic {
        let (path, name) = (self.of.path, self.of.names[self.index]);
        let _entry = crate::future::entry();
        let made = crate::future::current_program()
            .ok_or_else(|| crate::future::error("rawr: no program runs script code on this thread"))
            .and_then(|program| {
                program.variant_of(path, name).unwrap_or_else(|| {
                    Err(crate::future::error(format!(
                        "rawr: the program has no script type `{path}`"
                    )))
                })
            });
        match made {
            Ok(value) => value,
            Err(error) => panic!("{}", crate::future::message(&error)),
        }
    }

    /// The script value, lent shared to a script function for one call: a
    /// copy, as Rust lends a `Copy` value's.
    pub fn lend(&self) -> Dynamic {
        self.into_dynamic()
    }

    /// The script value, lent `&mut` to a script function for one call;
    /// [`Variant::restore`] reads back what the function left.
    pub fn lend_mut(&mut self) -> Dynamic {
        self.into_dynamic()
    }

    /// Hold what a script function that took the value `&mut` left in its
    /// place.
    pub fn restore(&mut self, value: Dynamic) {
        *self = Self::new(self.of, &value);
    }

    /// The strings native values borrow from the value while it is lent
    /// shared: none, as its variants carry nothing.
    #[must_use]
    pub fn pins(&self) -> &'static crate::scope::Pins {
        static NONE: crate::scope::Pins = crate::scope::Pins::new();
        &NONE
    }

    /// A proxy of the script value, as a generic holder hands it on.
    #[must_use]
    pub fn into_proxy(self) -> Proxy {
        Proxy::new(self.into_dynamic())
    }

    /// The value's text as its script type's own `Display` writes it.
    ///
    /// # Errors
    ///
    /// When the formatter fails.
    pub fn display(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.into_proxy().display(f)
    }

    /// `self == other` by the `PartialEq` the program writes for the type.
    #[must_use]
    pub fn equal(&self, other: &Self) -> bool {
        self.into_proxy() == other.into_proxy()
    }
}

impl std::fmt::Debug for Variant {
    /// The script type's `Debug` text.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(&self.into_proxy(), f)
    }
}

/// One of std's types that Rawr's own std holds as its Rust value
/// (`rawr_import::classify::std_binding`): a crate's `PathBuf` or
/// `io::Error` is the program's, so native code takes, gives and borrows
/// the value itself.
pub trait Std: Sized + 'static {
    /// What a shared borrow of a script value of this type lends native
    /// code: the Rust value it holds.
    type Lent: std::ops::Deref<Target = Self>;

    /// The script value holding `self`.
    fn give(self) -> Dynamic;

    /// The Rust value a script value holds, moved out of it.
    fn take(value: Dynamic) -> Option<Self>;

    /// The Rust value a script value holds, lent for a call. `arena` is
    /// the calling runner's, where a value a crate's value lends is reached
    /// ([`Borrow`]).
    fn lend(value: Dynamic, arena: &mut dyn std::any::Any) -> Option<Self::Lent>;

    /// The script value of a `&Self` a crate's value lends (#680), as a
    /// `&self` method's `&io::Error` borrows from its receiver: `borrow`
    /// reaches the value where its owner holds it at each use, so none is
    /// copied. `None` for a type whose borrow crosses as a copy, as a
    /// `&Path` does.
    fn borrowed(borrow: Borrow<Self>) -> Option<Dynamic> {
        let _ = borrow;
        None
    }
}

/// A borrow of a `T` a crate's value lends (`Std::borrowed`): given the
/// runner's arena, or `None` to reach it itself, it claims the owner and
/// gives a guard that derefs to the borrowed value while it lives, or why
/// the owner can't be reached.
pub type Borrow<T> = std::sync::Arc<
    dyn Fn(Option<&mut dyn std::any::Any>) -> RawrResultOf<Lending<T>> + Send + Sync,
>;

/// A guard a [`Borrow`] gives, which holds the owner claimed.
pub type Lending<T> = Box<dyn std::ops::Deref<Target = T> + Send>;

/// A std value a script value shares, lent for a call: one it holds, or
/// one a crate's value lends ([`Borrow`]).
pub enum Shared<T: 'static> {
    /// The value the script value holds.
    Held(std::sync::Arc<T>),
    /// The value where a crate's value holds it, its owner claimed.
    Borrowed(Lending<T>),
}

impl<T: 'static> std::ops::Deref for Shared<T> {
    type Target = T;

    fn deref(&self) -> &T {
        match self {
            Self::Held(value) => value,
            Self::Borrowed(lending) => lending,
        }
    }
}

/// A std value a script value holds as itself, lent for a call.
pub struct Lent<T>(T);

impl<T> std::ops::Deref for Lent<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.0
    }
}

macro_rules! std_held {
    ($($ty:ty),*) => {$(
        impl Std for $ty {
            type Lent = Lent<Self>;

            fn give(self) -> Dynamic {
                Dynamic::from(self)
            }

            fn take(value: Dynamic) -> Option<Self> {
                value.try_cast::<Self>()
            }

            fn lend(value: Dynamic, _arena: &mut dyn std::any::Any) -> Option<Lent<Self>> {
                value.try_cast::<Self>().map(Lent)
            }
        }
    )*};
}

std_held!(
    std::path::PathBuf,
    std::ffi::OsString,
    std::io::ErrorKind,
    std::io::SeekFrom
);

/// One of std's generic types Rawr's own std holds as its Rust value at
/// Rawr's values (`std_facade::held`), as `io::Cursor<T>` is a
/// `Cursor<Dynamic>`: native code takes and gives it at its own `T`, as
/// image's `ImageReader::new` takes a `Cursor<Bytes>`, each value it holds
/// made where it crosses.
pub trait StdOf<T>: Sized {
    /// The Rust value a script value holds, moved out of it, its value made
    /// a `T` by `inner`. `None` when the script value holds none.
    fn take<E>(
        value: Dynamic,
        inner: impl FnOnce(Dynamic) -> Result<T, E>,
    ) -> Option<Result<Self, E>>;

    /// The script value holding `self`, its `T` made a script value by
    /// `inner`.
    ///
    /// # Errors
    ///
    /// When `inner` fails.
    fn give<E>(self, inner: impl FnOnce(T) -> Result<Dynamic, E>) -> Result<Dynamic, E>;
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

/// A function of a script impl where a runner's impl for the owner's proxy
/// calls it (D594): the function the running program holds at its path,
/// its owner's path and the member (`Arc::Program<Message>::draw`). It is
/// found once per generation, with the witness of each parameter, as a
/// [`Slot`] holds a function item's callable, so a call looks nothing up.
pub struct Member {
    path: &'static str,
    found: std::sync::RwLock<Option<std::sync::Arc<Found>>>,
}

/// What a [`Member`] found in one generation.
struct Found {
    program: crate::future::Program,
    callable: crate::future::Callable,
    witnesses: Box<[Option<crate::host::runtime::Witness>]>,
}

impl Member {
    /// The member at `path`, found at its first call.
    #[must_use]
    pub const fn new(path: &'static str) -> Self {
        Self {
            path,
            found: std::sync::RwLock::new(None),
        }
    }

    /// Begin a call from native code into the member, in the generation
    /// current on this thread once a newer one offered is admitted, so an
    /// edit to its body takes effect at the next call. A panic with the
    /// reason when no program holds it, as a Rust function that fails does.
    pub fn enter(&self) -> Call {
        let _entry = crate::future::entry();
        let found = self
            .found()
            .unwrap_or_else(|error| panic!("{}", crate::future::message(&error)));
        Call { found }
    }

    /// The member in the generation current on this thread, found again
    /// only when that generation is another.
    fn found(&self) -> RawrResultOf<std::sync::Arc<Found>> {
        let program = crate::future::current_program().ok_or_else(|| {
            crate::future::error("rawr: no program runs script code on this thread")
        })?;
        let read = |found: &Option<std::sync::Arc<Found>>| {
            found
                .as_ref()
                .filter(|found| {
                    std::ptr::addr_eq(
                        std::sync::Arc::as_ptr(&found.program),
                        std::sync::Arc::as_ptr(&program),
                    )
                })
                .cloned()
        };
        if let Some(found) = read(
            &self
                .found
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        ) {
            return Ok(found);
        }
        let function = program.member(self.path).ok_or_else(|| {
            crate::future::error(format!("rawr: the running program has no `{}`", self.path))
        })?;
        let found = std::sync::Arc::new(Found {
            witnesses: program.member_witnesses(self.path).into_boxed_slice(),
            callable: crate::future::Callable::of_item(function, program.clone()),
            program,
        });
        *self
            .found
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(found.clone());
        Ok(found)
    }
}

/// One call from native code into a [`Member`], in one generation.
pub struct Call {
    found: std::sync::Arc<Found>,
}

impl Call {
    /// The witness of each parameter of the member, where it has one: a
    /// native value native code gives it is attested as its parameter's
    /// type, so script code reads it as that type.
    #[must_use]
    pub fn witnesses(&self) -> &[Option<crate::host::runtime::Witness>] {
        &self.found.witnesses
    }

    /// Call the member with `arguments`. Each index in `mutable` is an
    /// argument it takes `&mut`, as in [`Slot::call`]. A panic with the
    /// reason when it fails, as a Rust function that fails does.
    pub fn call(&self, arguments: &mut [Dynamic], mutable: &[usize]) -> Dynamic {
        let _entry = crate::future::entry();
        self.found
            .callable
            .call_places_in(&self.found.program, arguments, mutable)
            .unwrap_or_else(|error| panic!("{}", crate::future::message(&error)))
    }
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

/// Make this process a universe runner's (`rawr_import::universe`): a
/// generated universe runner calls it before it compiles a program, so it
/// names script types as the build that generated it did.
#[cfg(feature = "rust-import")]
pub fn universe() {
    rawr_import::universe::set();
}

pub use crate::resolve::lower_nominal::counted::Link;

/// A rawr `Rc` or `Arc` of `value`, the handle of a crate value a crate's
/// own pointer holds, which `link` reads (#775).
pub fn counted(value: Dynamic, link: Link) -> Dynamic {
    crate::resolve::lower_nominal::counted::native(value, link)
}

/// A new rawr `Rc` or `Arc` of `value`, the value Rust's own pointer to a
/// value that crosses by value held, as an `Arc<String>` a crate gives.
pub fn shared(value: Dynamic) -> Dynamic {
    crate::resolve::lower_nominal::counted::new(value)
}

/// The value the rawr `Rc` or `Arc` `pointer` points to, rawr's strong
/// count of it, and whether `pointer` is rawr's only pointer to it, strong
/// or weak.
#[must_use]
pub fn pointee(pointer: &Dynamic) -> Option<(Dynamic, usize, bool)> {
    crate::resolve::lower_nominal::counted::pointee(pointer)
}

/// Link the rawr `Rc` or `Arc` `pointer` to the crate's own pointer `link`
/// reads, where its value moved into one when it crossed (#775).
pub fn link(pointer: &Dynamic, link: Link) {
    crate::resolve::lower_nominal::counted::link(pointer, link);
}

/// Rawr's thread handle of a crate's `JoinHandle<T>`, as a crate's function
/// gives one: its `join` waits for the crate's thread and `give` makes the
/// thread's `T` rawr's, where it is joined, with the witness of `T` the
/// handle was attested with (#1266, #1275).
pub fn joined<T, F>(handle: std::thread::JoinHandle<T>, give: F) -> Dynamic
where
    T: Send + 'static,
    F: FnOnce(T, Option<&crate::host::runtime::Witness>) -> RawrResultOf<Dynamic> + Send + 'static,
{
    Dynamic::from(crate::eval::threads::Handle::native(handle, give))
}

/// Attest the value a crate's thread gives with `witness`, the witness of
/// its `T`, when `value` is rawr's handle of the thread: whether it is
/// one (#1275).
pub fn attest_joined(
    value: &Dynamic,
    witness: impl FnOnce() -> Option<crate::host::runtime::Witness>,
) -> bool {
    let Some(handle) = value.read_lock::<crate::eval::threads::Handle>() else {
        return false;
    };
    if let Some(witness) = witness() {
        handle.attest(witness);
    }
    true
}

/// The crate's `JoinHandle<T>` of rawr's thread handle `handle`, as a crate
/// takes one (#1274): the crate's own, as it gave it, or one of a thread
/// that joins the program's and makes what it gave a `T` with `make`.
/// `None` for a value that is no handle, or one already joined.
pub fn join_handle<T, F>(handle: Dynamic, make: F) -> Option<std::thread::JoinHandle<T>>
where
    T: Send + 'static,
    F: FnOnce(Dynamic) -> Result<T, String> + Send + 'static,
{
    handle
        .read_lock::<crate::eval::threads::Handle>()
        .map(|handle| handle.clone())?
        .take_native(make)
}
