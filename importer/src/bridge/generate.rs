//! Normalized standalone runner source generation.

use std::io::{self, Write};

use sha2::{Digest as _, Sha256};
use toml_edit::{value, DocumentMut, Item};

use super::request;
use crate::classify::representability::convert;
use crate::classify::representability::ownership::Receiver;
use crate::surface::adapter::{EntryIdentity, Operation, Plan};

pub(crate) mod cached;
mod view;

const MANIFEST_PATH: &str = "Cargo.toml";
const LOCK_PATH: &str = "Cargo.lock";
const BUILD_PATH: &str = "build.rs";
const SOURCE_PATH: &str = "src/main.rs";
/// The crate name of a runner binary no script type names.
const RUNNER_BINARY: &str = "rawr_generated_runner";
const TOP_LEVEL_SOURCE: &[u8] = include_bytes!("generate/top_level.rs");
const BUILD_SCRIPT: &[u8] = b"use std::{env, fs, path::PathBuf};\n\
fn main() {\n\
    let root = PathBuf::from(env::var_os(\"CARGO_MANIFEST_DIR\").expect(\"manifest directory\"));\n\
    let source = root.join(\"rawr-generated-manifest.carrier\");\n\
    let bytes = fs::read(&source).expect(\"generated manifest carrier\");\n\
    assert!(bytes.starts_with(b\"RAWRGM01\"), \"generated manifest carrier magic\");\n\
    assert!(bytes.len() <= 16 * 1024 * 1024, \"generated manifest carrier bound\");\n\
    let output = PathBuf::from(env::var_os(\"OUT_DIR\").expect(\"OUT_DIR\"));\n\
    fs::write(output.join(\"rawr-generated-manifest.bin\"), bytes).expect(\"copy generated manifest carrier\");\n\
    println!(\"cargo:rerun-if-changed={}\", source.display());\n\
}\n";
const RUNTIME_SOURCE: &[u8] = br#"
use std::any::Any;
use std::cell::RefCell;
use __rawr_internal::{Dynamic, ImmutableString};

#[derive(Clone, Copy)]
struct Adapter { identity: [u8; 32], signature: [u8; 32], selector: [u8; 32], instance: [u8; 32], bound: bool, application: bool, root: &'static str, witness: u8, arguments: u32, invoke: Invoke }
type Invoke = fn(&mut Arguments<'_, '_, '_>) -> Result<Dynamic, Error>;
type Project = fn(
    Box<dyn Any>,
    &__rawr_internal::host::runtime::Call<'_>,
    &__rawr_internal::host::runtime::Witness,
) -> Result<Dynamic, Box<__rawr_internal::EvalAltResult>>;
struct Arguments<'slice, 'value, 'arena> { values: &'slice mut [&'value mut Dynamic], arena: &'arena mut Arena }

/// A script value's handle on one arena slot. Every script copy of it
/// shares one [`Release`], so the slot's native value is dropped with the
/// last copy, as Rust drops a value with its last owner: a channel closes
/// when its last sender goes (D513).
#[derive(Clone)]
struct NativeHandle { epoch: [u8; 32], slot: u32, generation: u64, release: Option<std::sync::Arc<Release>> }

/// The claim the copies of one handle share on its slot.
struct Release { epoch: [u8; 32], slot: u32, generation: u64 }

/// Values futures lent and gave back, restored to their slots on the owner
/// thread the next time it has the arena (D513.6).
#[allow(clippy::type_complexity)]
static RESTORES: std::sync::Mutex<Vec<(NativeHandle, Box<dyn Any + Send>)>> =
    std::sync::Mutex::new(Vec::new());

thread_local! {
    /// The handles the protocol gave out: a handle on the wire keeps its
    /// value alive, as the caller may pass it back (review 5).
    static EXPORTED: RefCell<Vec<NativeHandle>> = const { RefCell::new(Vec::new()) };
}

/// Slots whose last handle was dropped, freed on the owner thread the next
/// time it has the arena.
static RELEASES: std::sync::Mutex<Vec<([u8; 32], u32, u64)>> = std::sync::Mutex::new(Vec::new());

impl Drop for Release {
    fn drop(&mut self) {
        RELEASES
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push((self.epoch, self.slot, self.generation));
        // A handle may be dropped on any thread; the arena is the owner's.
        __rawr_internal::future::on_owner(|| {
            let _ = with_arena(|_| Ok(()));
        });
    }
}
struct NativeSlot { generation: u64, value: Option<Box<dyn Any>>, homed: Option<Box<dyn Homed>>, leased: bool, debug: Option<DebugText>, display: Option<DebugText>, clone: Option<CloneValue>, relations: Relations, witness: Option<__rawr_internal::host::runtime::Witness>, project: Option<Project>, mapper: Option<[u8; 16]>, default_of: Option<[u8; 32]>, opaque: Option<OpaqueTable> }

/// A native value an iced view borrows lives in the application's home
/// (D568): its slot reaches it there, and holds no value of its own.
#[allow(dead_code)]
trait Homed: Any {
    fn shared(&self, call: &mut dyn FnMut(&dyn Any)) -> Result<(), Error>;
    fn mutable(&self, call: &mut dyn FnMut(&mut dyn Any)) -> Result<(), Error>;
    fn as_any(&self) -> &dyn Any;
}

/// Rust's `Debug` text of a stored value, for a type that implements it.
type DebugText = fn(&dyn Any) -> Option<String>;

fn debug_text<T: std::fmt::Debug + 'static>(value: &dyn Any) -> Option<String> {
    value.downcast_ref::<T>().map(|value| format!("{value:?}"))
}

/// Chooses `debug_text::<T>` when `T` implements `Debug` and nothing
/// otherwise, by method resolution on a concrete `T`.
struct DebugProbe<T>(std::marker::PhantomData<T>);

#[allow(dead_code)]
fn debug_probe<T>(_: &T) -> DebugProbe<T> {
    DebugProbe(std::marker::PhantomData)
}

#[allow(dead_code)]
trait ViaDebug {
    fn native_debug(&self) -> Option<DebugText>;
}

impl<T: std::fmt::Debug + 'static> ViaDebug for DebugProbe<T> {
    fn native_debug(&self) -> Option<DebugText> {
        Some(debug_text::<T>)
    }
}

#[allow(dead_code)]
trait WithoutDebug {
    fn native_debug(&self) -> Option<DebugText>;
}

impl<T> WithoutDebug for &DebugProbe<T> {
    fn native_debug(&self) -> Option<DebugText> {
        None
    }
}

/// The inputs of a script function native code calls on a thread with no
/// arena for the native values among them, as iced's executor calls
/// `Message::Loaded` for `Task::map`. When they are `Send`, the call is
/// made on the owner thread where its value is first used
/// (`Proxy::deferred`); else it fails there, by method resolution on a
/// concrete `T`.
#[allow(dead_code)]
struct DeferProbe<T>(std::cell::Cell<Option<T>>);

#[allow(dead_code)]
trait DeferSend<T> {
    fn defer(&self, call: fn(T) -> Dynamic) -> Option<__rawr_internal::crossing::Proxy>;
}

impl<T: Send + 'static> DeferSend<T> for DeferProbe<T> {
    fn defer(&self, call: fn(T) -> Dynamic) -> Option<__rawr_internal::crossing::Proxy> {
        let mut inputs = Some(self.0.take()?);
        Some(__rawr_internal::crossing::Proxy::deferred(move || {
            arena_access(|_| ())?;
            inputs.take().map(call)
        }))
    }
}

#[allow(dead_code)]
trait DeferNot<T> {
    fn defer(&self, call: fn(T) -> Dynamic) -> Option<__rawr_internal::crossing::Proxy>;
}

impl<T> DeferNot<T> for &DeferProbe<T> {
    fn defer(&self, _: fn(T) -> Dynamic) -> Option<__rawr_internal::crossing::Proxy> {
        None
    }
}

fn display_text<T: std::fmt::Display + 'static>(value: &dyn Any) -> Option<String> {
    value.downcast_ref::<T>().map(ToString::to_string)
}

#[allow(dead_code)]
trait ViaDisplay {
    fn native_display(&self) -> Option<DebugText>;
}

impl<T: std::fmt::Display + 'static> ViaDisplay for DebugProbe<T> {
    fn native_display(&self) -> Option<DebugText> {
        Some(display_text::<T>)
    }
}

#[allow(dead_code)]
trait WithoutDisplay {
    fn native_display(&self) -> Option<DebugText>;
}

impl<T> WithoutDisplay for &DebugProbe<T> {
    fn native_display(&self) -> Option<DebugText> {
        None
    }
}

/// A copy of a stored value, for a type that implements `Clone`.
type CloneValue = fn(&dyn Any) -> Option<Box<dyn Any>>;

fn clone_value<T: Clone + 'static>(value: &dyn Any) -> Option<Box<dyn Any>> {
    value.downcast_ref::<T>().map(|value| Box::new(value.clone()) as Box<dyn Any>)
}

#[allow(dead_code)]
trait ViaClone {
    fn native_clone(&self) -> Option<CloneValue>;
}

impl<T: Clone + 'static> ViaClone for DebugProbe<T> {
    fn native_clone(&self) -> Option<CloneValue> {
        Some(clone_value::<T>)
    }
}

#[allow(dead_code)]
trait WithoutClone {
    fn native_clone(&self) -> Option<CloneValue>;
}

impl<T> WithoutClone for &DebugProbe<T> {
    fn native_clone(&self) -> Option<CloneValue> {
        None
    }
}

/// `==` of two stored values of one type, for a type that implements
/// `PartialEq`.
type EqualValues = fn(&dyn Any, &dyn Any) -> Option<bool>;

fn equal_values<T: PartialEq + 'static>(lhs: &dyn Any, rhs: &dyn Any) -> Option<bool> {
    Some(lhs.downcast_ref::<T>()? == rhs.downcast_ref::<T>()?)
}

#[allow(dead_code)]
trait ViaEqual {
    fn native_equal(&self) -> Option<EqualValues>;
}

impl<T: PartialEq + 'static> ViaEqual for DebugProbe<T> {
    fn native_equal(&self) -> Option<EqualValues> {
        Some(equal_values::<T>)
    }
}

#[allow(dead_code)]
trait WithoutEqual {
    fn native_equal(&self) -> Option<EqualValues>;
}

impl<T> WithoutEqual for &DebugProbe<T> {
    fn native_equal(&self) -> Option<EqualValues> {
        None
    }
}

/// The hash of a stored value, for a type that implements `Hash`.
type HashValue = fn(&dyn Any) -> Option<u64>;

fn hash_value<T: std::hash::Hash + 'static>(value: &dyn Any) -> Option<u64> {
    use std::hash::{Hash as _, Hasher as _};
    let mut hasher = std::hash::DefaultHasher::new();
    value.downcast_ref::<T>()?.hash(&mut hasher);
    Some(hasher.finish())
}

#[allow(dead_code)]
trait ViaHash {
    fn native_hash(&self) -> Option<HashValue>;
}

impl<T: std::hash::Hash + 'static> ViaHash for DebugProbe<T> {
    fn native_hash(&self) -> Option<HashValue> {
        Some(hash_value::<T>)
    }
}

#[allow(dead_code)]
trait WithoutHash {
    fn native_hash(&self) -> Option<HashValue>;
}

impl<T> WithoutHash for &DebugProbe<T> {
    fn native_hash(&self) -> Option<HashValue> {
        None
    }
}

/// A stored value's `PartialEq` and `Hash`, when its type has them.
#[derive(Clone, Copy, Default)]
struct Relations { equal: Option<EqualValues>, hash: Option<HashValue> }

/// Store `value` in the arena, remembering its `Debug` text and its
/// `Clone` when its type has them.
#[allow(unused_macros)]
macro_rules! insert_native {
    ($arena:expr, $value:expr) => {{
        #[allow(unused_imports)]
        use $crate::{
            ViaClone as _, ViaDebug as _, ViaDisplay as _, ViaEqual as _, ViaHash as _,
            WithoutClone as _, WithoutDebug as _, WithoutDisplay as _, WithoutEqual as _,
            WithoutHash as _,
        };
        let value = $value;
        let debug = (&$crate::debug_probe(&value)).native_debug();
        let display = (&$crate::debug_probe(&value)).native_display();
        let clone = (&$crate::debug_probe(&value)).native_clone();
        let relations = $crate::Relations {
            equal: (&$crate::debug_probe(&value)).native_equal(),
            hash: (&$crate::debug_probe(&value)).native_hash(),
        };
        let handle = $arena.insert_debug(value, debug, clone);
        if let Ok(handle) = &handle {
            $arena.set_display(handle.clone(), display);
            $arena.set_relations(handle.clone(), relations);
        }
        handle
    }};
}

/// A copy of what a borrow guard dereferences to, made inside the call that
/// holds the guard: its `Clone`, or an error naming the type when it has
/// none, so a target without `Clone` fails where it is read and not where
/// the runner compiles.
#[allow(unused_macros)]
macro_rules! guard_copy {
    ($target:expr) => {{
        #[allow(unused_imports)]
        use $crate::{ViaClone as _, WithoutClone as _};
        let target = $target;
        $crate::copied(target, (&$crate::debug_probe(target)).native_clone())
    }};
}

/// `target` copied by `clone`, the `Clone` of its type when it has one.
#[allow(dead_code)]
fn copied<T: 'static>(target: &T, clone: Option<CloneValue>) -> Result<T, Error> {
    let clone = clone.ok_or(Error::NotClone(std::any::type_name::<T>()))?;
    clone(target as &dyn Any)
        .and_then(|value| value.downcast::<T>().ok())
        .map(|value| *value)
        .ok_or(Error::TypeMismatch)
}

/// `to_string()` and `{}` of an imported value print its Rust `Display`
/// text.
impl std::fmt::Display for NativeHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = arena_access(|arena| arena.display(self.clone())).flatten();
        match text {
            Some(text) => f.write_str(&text),
            None => f.write_str("NativeHandle"),
        }
    }
}

/// `{:?}` of an imported value prints its Rust `Debug` text.
impl std::fmt::Debug for NativeHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = arena_access(|arena| arena.debug(self.clone())).flatten();
        match text {
            Some(text) => f.write_str(&text),
            None => f.write_str("NativeHandle"),
        }
    }
}
struct Arena { epoch: [u8; 32], slots: Vec<NativeSlot>, free: Vec<u32> }

thread_local! {
    static NATIVE_ARENA: RefCell<Option<Arena>> = const { RefCell::new(None) };
}

/// Run the native call `call` with `arena` lent to the script closures it
/// calls back on this thread, as Freya's event loop calls an app's `fn
/// app()` from inside `launch` (D551.6).
#[allow(dead_code)]
fn lend<R>(arena: &mut Arena, call: impl FnOnce() -> R) -> R {
    __rawr_internal::island::lend::lend(arena, call)
}

/// The epochs of the arenas [`CallArena`] made, while each lives.
static CALL_EPOCHS: std::sync::Mutex<Vec<[u8; 32]>> = std::sync::Mutex::new(Vec::new());

/// An arena for one call on a thread that has none, as iced's executor
/// calls `Task::map`'s function: the native values the call takes and gives
/// move through it, and it ends with the call. A call that only moves its
/// values, as `Result::ok` does, needs nothing of the owner's arena. A value
/// the call keeps past its end, in a script value it gives, can't reach the
/// owner's arena, so the call fails there and says so.
#[allow(dead_code)]
struct CallArena(Option<[u8; 32]>);

#[allow(dead_code)]
impl CallArena {
    fn enter() -> Self {
        if arena_access(|_| ()).is_some() {
            return Self(None);
        }
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let mut epoch = [0xca; 32];
        epoch[..8].copy_from_slice(
            &NEXT
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                .to_le_bytes(),
        );
        let installed = NATIVE_ARENA.with(|cell| match cell.try_borrow_mut() {
            Ok(mut arena) if arena.is_none() => {
                *arena = Some(Arena::new(epoch));
                true
            }
            _ => false,
        });
        if !installed {
            return Self(None);
        }
        CALL_EPOCHS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(epoch);
        Self(Some(epoch))
    }
}

impl Drop for CallArena {
    fn drop(&mut self) {
        let Some(epoch) = self.0 else {
            return;
        };
        // What the call let go of is freed; what is left, a handle still
        // holds.
        let kept = NATIVE_ARENA.with(|cell| {
            let mut arena = cell.borrow_mut().take()?;
            arena.release_dropped();
            let slot = arena.slots.iter().find(|slot| slot.value.is_some())?;
            Some(
                slot.debug
                    .zip(slot.value.as_deref())
                    .and_then(|(debug, value)| debug(value))
                    .map(|text| format!(", {text},"))
                    .unwrap_or_default(),
            )
        });
        CALL_EPOCHS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .retain(|live| *live != epoch);
        RELEASES
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .retain(|(released, ..)| *released != epoch);
        if let Some(kept) = kept
            && !std::thread::panicking()
        {
            panic!(
                "rawr: a script function native code called on a thread with no arena kept a native value it was given{kept} past the call, which Rawr can't hand to the owner's thread"
            );
        }
    }
}

/// `call` with this thread's arena: the one in its cell, or else the one a
/// native call running on this thread lends. `None` when neither is free.
fn arena_access<R>(call: impl FnOnce(&mut Arena) -> R) -> Option<R> {
    NATIVE_ARENA.with(|cell| match cell.try_borrow_mut() {
        Ok(mut arena) => Some(call(arena.as_mut()?)),
        Err(_) => __rawr_internal::island::lend::with(call),
    })
}

enum Error {
    MissingArgument,
    TypeMismatch,
    StaleHandle,
    ArenaFull,
    Allocation,
    Malformed,
    Cardinality,
    MissingAdapter,
    Stale,
    /// A number that does not fit the Rust type it crosses into.
    OutOfRange { value: String, ty: &'static str },
    /// The value is lent to a future that is still running (D513.6).
    Leased,
    /// A script value an erased generic gave back is not of its type.
    Value(String),
    /// A borrow guard's target read without `Clone`, by its type's name.
    NotClone(&'static str),
    /// A value native code lent the call (D558.3), by its type's name,
    /// that can't be reached.
    Lent(&'static str, __rawr_internal::scope::Error),
    /// A value an iced view borrows, in the application's home, that
    /// can't be reached, as the home says (D568).
    Home(String),
}

impl std::fmt::Debug for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingArgument => f.write_str("MissingArgument"),
            Self::TypeMismatch => f.write_str("TypeMismatch"),
            Self::StaleHandle => f.write_str("StaleHandle"),
            Self::ArenaFull => f.write_str("ArenaFull"),
            Self::Allocation => f.write_str("Allocation"),
            Self::Malformed => f.write_str("Malformed"),
            Self::Cardinality => f.write_str("Cardinality"),
            Self::MissingAdapter => f.write_str("MissingAdapter"),
            Self::Stale => f.write_str("Stale"),
            Self::OutOfRange { value, ty } => write!(f, "{value} is out of range for `{ty}`"),
            Self::Leased => f.write_str(
                "cannot borrow this value: a future or a borrow guard that borrows it mutably is still running (E0499)",
            ),
            Self::Value(reason) => write!(f, "a value native code gave back is not its type: {reason}"),
            Self::NotClone(ty) => write!(
                f,
                "`{ty}` is read through a borrow guard, so Rawr copies it, and it does not implement `Clone`"
            ),
            Self::Lent(ty, error) => write!(f, "`{ty}`: {error}"),
            Self::Home(reason) => f.write_str(reason),
        }
    }
}

impl Error {
    fn code(&self) -> u8 {
        match self {
            Self::MissingArgument => 1,
            Self::TypeMismatch => 2,
            Self::StaleHandle => 3,
            Self::ArenaFull => 4,
            Self::Allocation => 5,
            Self::Malformed => 6,
            Self::Cardinality => 7,
            Self::MissingAdapter => 8,
            Self::Stale => 9,
            Self::OutOfRange { .. } => 10,
            Self::Leased => 11,
            Self::Value(_) => 12,
            Self::NotClone(_) => 13,
            Self::Lent(..) => 14,
            Self::Home(_) => 15,
        }
    }
}

/// A Rust integer as Rawr keeps it in an `i64`. Rawr keeps a `u64` or a
/// `usize` as the `i64` with the same bits, so every value of theirs
/// crosses. The narrower types are checked as `try_from` checks.
trait Int: Sized + Copy + std::fmt::Display {
    fn from_int(value: i64) -> Option<Self>;
    fn into_int(self) -> Option<i64>;
}

macro_rules! checked_int {
    ($($ty:ty),*) => {$(
        impl Int for $ty {
            fn from_int(value: i64) -> Option<Self> {
                Self::try_from(value).ok()
            }
            fn into_int(self) -> Option<i64> {
                i64::try_from(self).ok()
            }
        }
    )*};
}
checked_int!(i8, i16, i32, i64, isize, u8, u16, u32);

macro_rules! wide_int {
    ($($ty:ty),*) => {$(
        impl Int for $ty {
            fn from_int(value: i64) -> Option<Self> {
                Some(value as u64 as $ty)
            }
            fn into_int(self) -> Option<i64> {
                Some(self as u64 as i64)
            }
        }
    )*};
}
wide_int!(u64, usize);

/// Rawr's `i64` as the Rust integer `T`.
#[allow(dead_code)]
fn int_from<T: Int>(value: i64) -> Result<T, Error> {
    T::from_int(value).ok_or_else(|| Error::OutOfRange {
        value: value.to_string(),
        ty: std::any::type_name::<T>(),
    })
}

/// The Rust integer `value` as Rawr's `i64`.
#[allow(dead_code)]
fn int_into<T: Int>(value: T) -> Result<i64, Error> {
    value.into_int().ok_or_else(|| Error::OutOfRange {
        value: value.to_string(),
        ty: "i64",
    })
}

/// Rawr's inclusive integer range with each end checked into `T`.
#[allow(dead_code)]
fn range_from<T: Int>(range: std::ops::RangeInclusive<i64>) -> Result<std::ops::RangeInclusive<T>, Error> {
    let (start, end) = range.into_inner();
    Ok(int_from(start)?..=int_from(end)?)
}

impl Arena {
    fn new(epoch: [u8; 32]) -> Self {
        Self { epoch, slots: Vec::new(), free: Vec::new() }
    }

    fn insert<T: 'static>(&mut self, value: T) -> Result<NativeHandle, Error> {
        let slot = match self.free.pop() {
            Some(slot) => slot,
            None => {
                if self.slots.len() >= 1_000_000 { return Err(Error::ArenaFull); }
                self.slots.try_reserve_exact(1).map_err(|_| Error::Allocation)?;
                let slot = u32::try_from(self.slots.len()).map_err(|_| Error::ArenaFull)?;
                self.slots.push(NativeSlot { generation: 1, value: None, homed: None, leased: false, debug: None, display: None, clone: None, relations: Relations::default(), witness: None, project: None, mapper: None, default_of: None, opaque: None });
                slot
            }
        };
        let entry = self.slots.get_mut(slot as usize).ok_or(Error::StaleHandle)?;
        if entry.value.is_some() || entry.homed.is_some() { return Err(Error::StaleHandle); }
        entry.value = Some(Box::new(value));
        entry.leased = false;
        entry.debug = None;
        entry.display = None;
        entry.clone = None;
        entry.relations = Relations::default();
        entry.witness = None;
        entry.project = None;
        entry.mapper = None;
        entry.default_of = None;
        entry.opaque = None;
        let release = std::sync::Arc::new(Release { epoch: self.epoch, slot, generation: entry.generation });
        Ok(NativeHandle { epoch: self.epoch, slot, generation: entry.generation, release: Some(release) })
    }

    /// Give the value at `handle`, whose type holds an opaque `impl Trait`
    /// type, the calls that take it, compiled where it was made.
    #[allow(dead_code)]
    fn set_opaque(&mut self, handle: &NativeHandle, table: OpaqueTable) {
        if let Some(entry) = self.slots.get_mut(handle.slot as usize) {
            entry.opaque = Some(table);
        }
    }

    /// The calls that take the value at `handle`, compiled where it was
    /// made.
    #[allow(dead_code)]
    fn opaque(&self, handle: &NativeHandle) -> Result<OpaqueTable, Error> {
        let entry = self.slots.get(handle.slot as usize).ok_or(Error::StaleHandle)?;
        if handle.epoch != self.epoch || handle.generation != entry.generation {
            return Err(Error::StaleHandle);
        }
        entry.opaque.ok_or(Error::TypeMismatch)
    }

    /// Mark the value at `handle` as made by the `Default` of the imported
    /// type `identity`, outside any call: it takes that type's witness where
    /// it is first checked as it.
    #[allow(dead_code)]
    fn made_by_default(&mut self, handle: &NativeHandle, identity: [u8; 32]) {
        if let Some(entry) = self.slots.get_mut(handle.slot as usize) {
            entry.default_of = Some(identity);
        }
    }

    /// The imported type whose `Default` made the value at `handle`, while
    /// it has no witness.
    fn default_of(&self, handle: &NativeHandle) -> Option<[u8; 32]> {
        let entry = self.slots.get(handle.slot as usize)?;
        (handle.epoch == self.epoch && handle.generation == entry.generation && entry.witness.is_none())
            .then_some(entry.default_of)
            .flatten()
    }

    fn insert_debug<T: 'static>(&mut self, value: T, debug: Option<DebugText>, clone: Option<CloneValue>) -> Result<NativeHandle, Error> {
        let handle = self.insert(value)?;
        if let Some(entry) = self.slots.get_mut(handle.slot as usize) {
            entry.debug = debug;
            entry.clone = clone;
        }
        Ok(handle)
    }

    fn debug(&self, handle: NativeHandle) -> Option<String> {
        let entry = self.slots.get(handle.slot as usize)?;
        if handle.epoch != self.epoch || handle.generation != entry.generation {
            return None;
        }
        let debug = entry.debug?;
        if let Some(homed) = &entry.homed {
            let mut text = None;
            homed.shared(&mut |value| text = debug(value)).ok()?;
            return text;
        }
        debug(entry.value.as_deref()?)
    }

    #[allow(dead_code)]
    fn set_display(&mut self, handle: NativeHandle, display: Option<DebugText>) {
        if let Some(entry) = self.slots.get_mut(handle.slot as usize) {
            entry.display = display;
        }
    }

    #[allow(dead_code)]
    fn set_relations(&mut self, handle: NativeHandle, relations: Relations) {
        if let Some(entry) = self.slots.get_mut(handle.slot as usize) {
            entry.relations = relations;
        }
    }

    /// The live value of `handle`, with its slot.
    fn live(&self, handle: NativeHandle) -> Option<(&NativeSlot, &dyn Any)> {
        let entry = self.slots.get(handle.slot as usize)?;
        if handle.epoch != self.epoch || handle.generation != entry.generation {
            return None;
        }
        Some((entry, entry.value.as_deref()?))
    }

    /// `==` of two values of one type by its `PartialEq`.
    /// `call` of the value `handle` names.
    fn with_value<R>(&self, handle: NativeHandle, call: impl FnOnce(&dyn Any) -> R) -> Option<R> {
        if let Some(homed) = self.homed(&handle) {
            let mut call = Some(call);
            let mut output = None;
            homed.shared(&mut |value| output = call.take().map(|call| call(value))).ok()?;
            return output;
        }
        let (_, value) = self.live(handle)?;
        Some(call(value))
    }

    /// The home a homed slot's value lives in ([`Homed`]).
    fn homed(&self, handle: &NativeHandle) -> Option<&dyn Homed> {
        if handle.epoch != self.epoch { return None; }
        let entry = self.slots.get(handle.slot as usize)?;
        if entry.generation != handle.generation { return None; }
        entry.homed.as_deref()
    }

    /// Move the value of `handle` into the application's home, where an
    /// iced view borrows it (D568), once: `home` keeps it there, and the
    /// slot reaches it through what `home` gives back.
    #[allow(dead_code)]
    fn home<T: 'static, H: Homed + Clone>(
        &mut self,
        handle: NativeHandle,
        home: impl FnOnce(T) -> Result<H, Error>,
    ) -> Result<H, Error> {
        if let Some(homed) = self.homed(&handle) {
            return homed.as_any().downcast_ref::<H>().cloned().ok_or(Error::TypeMismatch);
        }
        let index = self.index::<T>(&handle)?;
        let value = self.slots[index].value.take().ok_or(Error::StaleHandle)?;
        let value = value.downcast::<T>().map(|value| *value).map_err(|_| Error::TypeMismatch)?;
        let homed = home(value)?;
        self.slots[index].homed = Some(Box::new(homed.clone()));
        Ok(homed)
    }

    fn equal(&self, lhs: NativeHandle, rhs: NativeHandle) -> Option<bool> {
        let (entry, lhs) = self.live(lhs)?;
        let (_, rhs) = self.live(rhs)?;
        if lhs.type_id() != rhs.type_id() {
            return None;
        }
        (entry.relations.equal?)(lhs, rhs)
    }

    /// The order a hashed collection keeps two values of one type in: by
    /// hash, then by `Debug` text when two unequal values share one.
    fn compare(&self, lhs: NativeHandle, rhs: NativeHandle) -> Option<std::cmp::Ordering> {
        let (left, lhs) = self.live(lhs)?;
        let (right, rhs) = self.live(rhs)?;
        if lhs.type_id() != rhs.type_id() {
            return None;
        }
        let hash = left.relations.hash?;
        match hash(lhs)?.cmp(&hash(rhs)?) {
            std::cmp::Ordering::Equal if (left.relations.equal?)(lhs, rhs)? => {
                Some(std::cmp::Ordering::Equal)
            }
            std::cmp::Ordering::Equal => (left.debug?)(lhs)?.partial_cmp(&(right.debug?)(rhs)?),
            ordering => Some(ordering),
        }
    }

    fn display(&self, handle: NativeHandle) -> Option<String> {
        let entry = self.slots.get(handle.slot as usize)?;
        if handle.epoch != self.epoch || handle.generation != entry.generation {
            return None;
        }
        let display = entry.display?;
        if let Some(homed) = &entry.homed {
            let mut text = None;
            homed.shared(&mut |value| text = display(value)).ok()?;
            return text;
        }
        display(entry.value.as_deref()?)
    }

    fn insert_projectable<T: 'static>(&mut self, value: T, project: Project, mapper: [u8; 16]) -> Result<NativeHandle, Error> {
        let handle = self.insert(value)?;
        let entry = self.slots.get_mut(handle.slot as usize).ok_or(Error::StaleHandle)?;
        entry.project = Some(project);
        entry.mapper = Some(mapper);
        Ok(handle)
    }

    fn index<T: 'static>(&self, handle: &NativeHandle) -> Result<usize, Error> {
        if handle.epoch != self.epoch { return Err(Error::StaleHandle); }
        let index = handle.slot as usize;
        let entry = self.slots.get(index).ok_or(Error::StaleHandle)?;
        if entry.generation == handle.generation && entry.leased {
            return Err(Error::Leased);
        }
        if entry.generation != handle.generation || entry.value.is_none() {
            return Err(Error::StaleHandle);
        }
        if !entry.value.as_ref().is_some_and(|value| value.is::<T>()) {
            return Err(Error::TypeMismatch);
        }
        Ok(index)
    }

    fn take<T: 'static>(&mut self, handle: NativeHandle) -> Result<T, Error> {
        // A homed value is copied out by its `Clone`, as a slot's is; one
        // without it stays where the view borrows it.
        if let Some(homed) = self.homed(&handle) {
            let clone = self.slots[handle.slot as usize].clone.ok_or(Error::Leased)?;
            let mut copied = None;
            homed.shared(&mut |value| copied = clone(value))?;
            return copied
                .ok_or(Error::TypeMismatch)?
                .downcast::<T>()
                .map(|value| *value)
                .map_err(|_| Error::TypeMismatch);
        }
        let index = self.index::<T>(&handle)?;
        // A script copies a handle where it copies the value, so a `Clone`
        // value is cloned out and its slot stays valid for every copy.
        if let Some(clone) = self.slots[index].clone {
            let value = self.slots[index].value.as_deref().ok_or(Error::StaleHandle)?;
            let value = clone(value).ok_or(Error::TypeMismatch)?;
            return value.downcast::<T>().map(|value| *value).map_err(|_| Error::TypeMismatch);
        }
        let reusable = self.slots[index].generation.checked_add(1);
        if reusable.is_some() {
            self.free.try_reserve_exact(1).map_err(|_| Error::Allocation)?;
        }
        let value = self.slots[index].value.take().ok_or(Error::StaleHandle)?;
        self.slots[index].witness = None;
        self.slots[index].project = None;
        self.slots[index].mapper = None;
        match reusable {
            Some(generation) => {
                self.slots[index].generation = generation;
                self.free.push(handle.slot);
            }
            None => self.slots[index].generation = u64::MAX,
        }
        value.downcast::<T>().map(|value| *value).map_err(|_| Error::TypeMismatch)
    }

    fn project(
        &mut self,
        handle: NativeHandle,
        call: &__rawr_internal::host::runtime::Call<'_>,
    ) -> Result<Dynamic, Box<__rawr_internal::EvalAltResult>> {
        if handle.epoch != self.epoch { return Err(runtime_error(Error::StaleHandle)); }
        let index = handle.slot as usize;
        let entry = self.slots.get(index).ok_or_else(|| runtime_error(Error::StaleHandle))?;
        if entry.generation != handle.generation || entry.value.is_none() {
            return Err(runtime_error(Error::StaleHandle));
        }
        if entry.mapper.as_ref() != call.event_mapper() {
            return Err(runtime_error(Error::TypeMismatch));
        }
        let witness = entry.witness.clone().ok_or_else(|| runtime_error(Error::StaleHandle))?;
        let project = entry.project.ok_or_else(|| runtime_error(Error::TypeMismatch))?;
        let reusable = self.slots[index].generation.checked_add(1);
        if reusable.is_some() {
            self.free.try_reserve_exact(1).map_err(|_| runtime_error(Error::Allocation))?;
        }
        let value = self.slots[index].value.take().ok_or_else(|| runtime_error(Error::StaleHandle))?;
        self.slots[index].witness = None;
        self.slots[index].project = None;
        self.slots[index].mapper = None;
        match reusable {
            Some(generation) => {
                self.slots[index].generation = generation;
                self.free.push(handle.slot);
            }
            None => self.slots[index].generation = u64::MAX,
        }
        project(value, call, &witness)
    }

    fn witness(&self, handle: NativeHandle) -> Option<&__rawr_internal::host::runtime::Witness> {
        let index = handle.slot as usize;
        let entry = self.slots.get(index)?;
        // A value lent to a future keeps its witness.
        (handle.epoch == self.epoch
            && handle.generation == entry.generation
            && (entry.value.is_some() || entry.leased || entry.homed.is_some()))
            .then_some(entry.witness.as_ref())
            .flatten()
    }

    fn attest(
        &mut self,
        handle: NativeHandle,
        witness: __rawr_internal::host::runtime::Witness,
    ) -> Result<(), Error> {
        let index = handle.slot as usize;
        let entry = self.slots.get_mut(index).ok_or(Error::StaleHandle)?;
        if handle.epoch != self.epoch
            || handle.generation != entry.generation
            || (entry.value.is_none() && entry.homed.is_none())
        {
            return Err(Error::StaleHandle);
        }
        entry.witness = Some(witness);
        Ok(())
    }

    /// Lend the value to a future (D513.6): it leaves its slot, which
    /// refuses every use until the future gives it back.
    #[allow(dead_code)]
    fn lease<T: Send + 'static>(&mut self, handle: NativeHandle) -> Result<Held<T>, Error> {
        if self.homed(&handle).is_some() {
            return Err(Error::Leased);
        }
        let index = self.index::<T>(&handle)?;
        let value = self.slots[index].value.take().ok_or(Error::StaleHandle)?;
        self.slots[index].leased = true;
        let value = value.downcast::<T>().map(|value| *value).map_err(|_| Error::TypeMismatch)?;
        Ok(Held::Leased { value: Some(value), handle })
    }

    /// Free each slot whose last handle was dropped. Dropping a native value
    /// may drop handles it holds, which are freed in the next round.
    fn release_dropped(&mut self) {
        // Another arena's values wait for their own arena.
        let restores = {
            let mut queued = RESTORES.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let (mine, others) = std::mem::take(&mut *queued)
                .into_iter()
                .partition::<Vec<_>, _>(|(handle, _)| handle.epoch == self.epoch);
            *queued = others;
            mine
        };
        for (handle, value) in restores {
            self.restore(handle, value);
        }
        // Another live arena's releases wait for it: the owner's for the
        // owner, and a call arena's for that call (`CallArena`). Those of a
        // past program's arena are dropped.
        let live = |epoch: &[u8; 32]| {
            CALL_EPOCHS
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .contains(epoch)
        };
        let call = live(&self.epoch);
        loop {
            let batch = {
                let mut queued = RELEASES.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                let (mine, others) = std::mem::take(&mut *queued)
                    .into_iter()
                    .partition::<Vec<_>, _>(|(epoch, ..)| *epoch == self.epoch);
                *queued = others;
                mine
            };
            if !call {
                RELEASES
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .retain(|(epoch, ..)| live(epoch));
            }
            if batch.is_empty() {
                return;
            }
            let mut dropped = Vec::with_capacity(batch.len());
            let mut homed = Vec::new();
            for (_, slot, generation) in batch {
                let Some(entry) = self.slots.get_mut(slot as usize) else {
                    continue;
                };
                if entry.generation != generation {
                    continue;
                }
                dropped.push(entry.value.take());
                homed.push(entry.homed.take());
                entry.leased = false;
                entry.witness = None;
                entry.project = None;
                entry.mapper = None;
                match entry.generation.checked_add(1) {
                    Some(next) => {
                        entry.generation = next;
                        self.free.push(slot);
                    }
                    None => entry.generation = u64::MAX,
                }
            }
            drop(dropped);
            drop(homed);
        }
    }

    /// The value a future borrowed, back in its slot.
    fn restore(&mut self, handle: NativeHandle, value: Box<dyn Any + Send>) {
        if handle.epoch != self.epoch { return; }
        if let Some(entry) = self.slots.get_mut(handle.slot as usize)
            && entry.generation == handle.generation
            && entry.leased
        {
            entry.value = Some(value);
            entry.leased = false;
        }
    }

    fn shared<T: 'static, R>(
        &self,
        handle: NativeHandle,
        call: impl FnOnce(&T) -> R,
    ) -> Result<R, Error> {
        if let Some(homed) = self.homed(&handle) {
            let mut call = Some(call);
            let mut output = None;
            homed.shared(&mut |value| {
                if let (Some(call), Some(value)) = (call.take(), value.downcast_ref::<T>()) {
                    output = Some(call(value));
                }
            })?;
            return output.ok_or(Error::TypeMismatch);
        }
        let index = self.index::<T>(&handle)?;
        let value = self.slots[index]
            .value
            .as_ref()
            .and_then(|value| value.downcast_ref::<T>())
            .ok_or(Error::TypeMismatch)?;
        Ok(call(value))
    }

    fn mutable<T: 'static, R>(
        &mut self,
        handle: NativeHandle,
        call: impl FnOnce(&mut T) -> R,
    ) -> Result<R, Error> {
        if let Some(homed) = self.homed(&handle) {
            let mut call = Some(call);
            let mut output = None;
            homed.mutable(&mut |value| {
                if let (Some(call), Some(value)) = (call.take(), value.downcast_mut::<T>()) {
                    output = Some(call(value));
                }
            })?;
            return output.ok_or(Error::TypeMismatch);
        }
        let index = self.index::<T>(&handle)?;
        let value = self.slots[index]
            .value
            .as_mut()
            .and_then(|value| value.downcast_mut::<T>())
            .ok_or(Error::TypeMismatch)?;
        Ok(call(value))
    }

    /// `call` with the value of `handle` out of its slot, so the arena it
    /// is also given can be lent while the value is borrowed, as an update
    /// through a borrow guard lends it to the script change it runs. The
    /// slot refuses every use meanwhile, and the value goes back after,
    /// unless its last handle went.
    #[allow(dead_code)]
    fn held<T: 'static, R>(
        &mut self,
        handle: NativeHandle,
        call: impl FnOnce(&mut Self, &mut T) -> R,
    ) -> Result<R, Error> {
        // A homed value is in the home, so the arena is free to lend.
        if let Some(index) = self.homed(&handle).map(|_| handle.slot as usize) {
            let homed = self.slots[index].homed.take().ok_or(Error::StaleHandle)?;
            let mut call = Some(call);
            let mut output = None;
            let result = homed.mutable(&mut |value| {
                if let (Some(call), Some(value)) = (call.take(), value.downcast_mut::<T>()) {
                    output = Some(call(self, value));
                }
            });
            if let Some(entry) = self.slots.get_mut(index)
                && entry.generation == handle.generation
                && entry.homed.is_none()
            {
                entry.homed = Some(homed);
            }
            result?;
            return output.ok_or(Error::TypeMismatch);
        }
        let index = self.index::<T>(&handle)?;
        let mut value = self.slots[index].value.take().ok_or(Error::StaleHandle)?;
        self.slots[index].leased = true;
        let result = match value.downcast_mut::<T>() {
            Some(target) => Ok(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                call(self, target)
            }))),
            None => Err(Error::TypeMismatch),
        };
        if let Some(entry) = self.slots.get_mut(index)
            && entry.generation == handle.generation
            && entry.leased
            && entry.value.is_none()
        {
            entry.value = Some(value);
            entry.leased = false;
        }
        match result? {
            Ok(output) => Ok(output),
            Err(panic) => std::panic::resume_unwind(panic),
        }
    }
}

fn install_arena(epoch: [u8; 32]) {
    __rawr_internal::host::debug::register::<NativeHandle>();
    __rawr_internal::host::display::register::<NativeHandle>();
    __rawr_internal::host::debug::register::<__rawr_internal::scope::Scoped>();
    __rawr_internal::host::display::register::<__rawr_internal::scope::Scoped>();
    NATIVE_ARENA.with(|arena| *arena.borrow_mut() = Some(Arena::new(epoch)));
    // A new program's arena: the handles the last one gave out are stale.
    EXPORTED.with_borrow_mut(Vec::clear);
}

fn with_arena<T>(call: impl FnOnce(&mut Arena) -> Result<T, Error>) -> Result<T, Error> {
    arena_access(|arena| {
        arena.release_dropped();
        let result = call(arena);
        arena.release_dropped();
        result
    })
    .unwrap_or(Err(Error::Stale))
}

/// A receiver an async method borrows, held by its future (D513.6): a copy
/// of a `Clone` value for `&self`, or the value itself, lent from its slot,
/// which it goes back to when the future ends or is dropped.
#[allow(dead_code)]
enum Held<T: Send + 'static> {
    Cloned(T),
    Leased { value: Option<T>, handle: NativeHandle },
}

#[allow(dead_code)]
impl<T: Send + 'static> Held<T> {
    fn get_mut(&mut self) -> &mut T {
        match self {
            Self::Cloned(value) => value,
            Self::Leased { value, .. } => value.as_mut().expect("a lent value is held until dropped"),
        }
    }
}

impl<T: Send + 'static> Drop for Held<T> {
    fn drop(&mut self) {
        if let Self::Leased { value, handle } = self
            && let Some(value) = value.take()
        {
            // The arena is the owner thread's, and may be borrowed by the
            // native call that drops this future, or absent on an executor's
            // thread: the value waits in the queue its next use drains.
            RESTORES
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push((handle.clone(), Box::new(value)));
            __rawr_internal::future::on_owner(|| {
                let _ = with_arena(|_| Ok(()));
            });
        }
    }
}

/// Chooses a copy of a `Clone` receiver for a `&self` async method, and a
/// lease otherwise, by method resolution on a concrete `T`.
#[allow(dead_code)]
struct HoldProbe<T>(std::marker::PhantomData<T>);

#[allow(dead_code)]
impl<T> HoldProbe<T> {
    fn new() -> Self {
        Self(std::marker::PhantomData)
    }
}

#[allow(dead_code)]
trait HoldClone<T: Send + 'static> {
    fn hold(&self, arena: &mut Arena, handle: NativeHandle) -> Result<Held<T>, Error>;
}

impl<T: Clone + Send + 'static> HoldClone<T> for HoldProbe<T> {
    fn hold(&self, arena: &mut Arena, handle: NativeHandle) -> Result<Held<T>, Error> {
        arena.shared::<T, _>(handle, |value| Held::Cloned(value.clone()))
    }
}

#[allow(dead_code)]
trait HoldLease<T: Send + 'static> {
    fn hold(&self, arena: &mut Arena, handle: NativeHandle) -> Result<Held<T>, Error>;
}

impl<T: Send + 'static> HoldLease<T> for &HoldProbe<T> {
    fn hold(&self, arena: &mut Arena, handle: NativeHandle) -> Result<Held<T>, Error> {
        arena.lease::<T>(handle)
    }
}

/// Turn one value a Rust task or future yielded into a Rawr value, on the
/// thread that owns the arena. An imported value takes the witness of the
/// task's output type.
#[allow(dead_code)]
fn materialize(
    witness: Option<&__rawr_internal::host::runtime::Witness>,
    convert: impl FnOnce(&mut Arguments<'_, '_, '_>) -> Result<Dynamic, Error>,
) -> Result<Dynamic, String> {
    with_arena(|arena| {
        let mut values: [&mut Dynamic; 0] = [];
        let mut arguments = Arguments { values: &mut values, arena };
        let output = convert(&mut arguments)?;
        if let (Some(handle), Some(witness)) = (output.read_lock::<NativeHandle>().map(|value| value.clone()), witness) {
            arguments.arena.attest(handle, witness.clone())?;
        }
        Ok(output)
    })
    .map_err(|error| format!("{error:?}"))
}

/// [`materialize`] of an output that holds script values an erased generic
/// gave back: each is checked against the script type it stands for.
#[allow(dead_code)]
fn materialize_checked(
    witness: Option<&__rawr_internal::host::runtime::Witness>,
    convert: impl FnOnce(&mut Arguments<'_, '_, '_>) -> Result<Dynamic, Error>,
) -> Result<Dynamic, String> {
    let output = materialize(witness, convert)?;
    check_values(witness, &output).map_err(|error| format!("{error:?}"))?;
    Ok(output)
}

fn take_primitive<T: 'static>(arguments: &mut Arguments<'_, '_, '_>, index: usize) -> Result<T, Error> {
    let value = arguments.values.get_mut(index).ok_or(Error::MissingArgument)?;
    (**value).take().try_cast_result::<T>().map_err(|_| Error::TypeMismatch)
}

#[allow(dead_code)]
fn take_int<T: Int>(arguments: &mut Arguments<'_, '_, '_>, index: usize) -> Result<T, Error> {
    int_from(take_primitive::<i64>(arguments, index)?)
}

#[allow(dead_code)]
fn take_f32(arguments: &mut Arguments<'_, '_, '_>, index: usize) -> Result<f32, Error> {
    take_primitive::<f64>(arguments, index).map(|value| value as f32)
}

/// An inclusive range whose integer type Rust infers from the call.
#[allow(dead_code)]
fn take_range<T: Int>(arguments: &mut Arguments<'_, '_, '_>, index: usize) -> Result<std::ops::RangeInclusive<T>, Error> {
    range_from(take_primitive::<std::ops::RangeInclusive<i64>>(arguments, index)?)
}

#[allow(dead_code)]
fn take_string(arguments: &mut Arguments<'_, '_, '_>, index: usize) -> Result<String, Error> {
    take_primitive::<ImmutableString>(arguments, index).map(|value| value.to_string())
}

fn native_handle(
    arguments: &mut Arguments<'_, '_, '_>,
    index: usize,
    consume: bool,
) -> Result<NativeHandle, Error> {
    let value = arguments.values.get_mut(index).ok_or(Error::MissingArgument)?;
    if consume {
        (**value).take().try_cast_result::<NativeHandle>().map_err(|_| Error::TypeMismatch)
    } else if (**value).is::<NativeHandle>() {
        Ok((**value).clone_cast::<NativeHandle>())
    } else {
        Err(Error::TypeMismatch)
    }
}

/// A script value an erased generic takes, as it is (D513.2).
#[allow(dead_code)]
fn take_value(arguments: &mut Arguments<'_, '_, '_>, index: usize) -> Result<__rawr_internal::Value, Error> {
    let value = arguments.values.get_mut(index).ok_or(Error::MissingArgument)?;
    Ok(__rawr_internal::Value::from((**value).take()))
}

/// A script future a generic item's `F: Future` takes, as the std future
/// that runs it (D527).
#[allow(dead_code)]
fn take_script(arguments: &mut Arguments<'_, '_, '_>, index: usize) -> Result<__rawr_internal::future::Script, Error> {
    let value = arguments.values.get_mut(index).ok_or(Error::MissingArgument)?;
    (**value)
        .take()
        .try_cast_result::<__rawr_internal::future::Future>()
        .map(__rawr_internal::future::Future::into_script)
        .map_err(|_| Error::TypeMismatch)
}

/// Check script values that came back from an erased generic against the
/// script type they stand for, on the thread that owns the program.
#[allow(dead_code)]
fn check_values(
    witness: Option<&__rawr_internal::host::runtime::Witness>,
    value: &Dynamic,
) -> Result<(), Error> {
    match witness {
        Some(witness) => __rawr_internal::island::validate(witness, value).map_err(Error::Value),
        None => Ok(()),
    }
}

/// The value native code lent the call at argument `index` (D558.3), if
/// that argument is one.
#[allow(dead_code)]
fn lent_value(arguments: &Arguments<'_, '_, '_>, index: usize) -> Option<__rawr_internal::scope::Lent> {
    arguments
        .values
        .get(index)
        .and_then(|value| value.read_lock::<__rawr_internal::scope::Lent>().map(|lent| *lent))
}

/// The value in a scope frame's region at argument `index` (D558 tier 1),
/// if that argument is one.
#[allow(dead_code)]
fn scoped_value(arguments: &Arguments<'_, '_, '_>, index: usize) -> Option<__rawr_internal::scope::Scoped> {
    arguments
        .values
        .get(index)
        .and_then(|value| value.read_lock::<__rawr_internal::scope::Scoped>().map(|scoped| *scoped))
}

/// The first value of a scope frame's region in the array at `index`, as
/// `column![..]`'s elements are.
#[allow(dead_code)]
fn scoped_item(arguments: &Arguments<'_, '_, '_>, index: usize) -> Option<__rawr_internal::scope::Scoped> {
    let value = arguments.values.get(index)?;
    let items = value.read_lock::<__rawr_internal::Array>()?;
    items
        .iter()
        .find_map(|item| item.read_lock::<__rawr_internal::scope::Scoped>().map(|scoped| *scoped))
}

/// A value native code lent the call (D558.3) that an argument reaches,
/// erased: its type is `'static`, as ratatui's `ListState` is.
#[allow(dead_code)]
enum Reached<'a> {
    Shared(&'a (dyn Any + 'static)),
    Mutable(&'a mut (dyn Any + 'static)),
}

#[allow(dead_code)]
impl<'a> Reached<'a> {
    fn reborrow(&mut self) -> Reached<'_> {
        match self {
            Self::Shared(value) => Reached::Shared(*value),
            Self::Mutable(value) => Reached::Mutable(&mut **value),
        }
    }

    fn shared<T: 'static>(self) -> Option<&'a T> {
        match self {
            Self::Shared(value) => value.downcast_ref(),
            Self::Mutable(value) => value.downcast_ref(),
        }
    }

    fn mutable<T: 'static>(self) -> Option<&'a mut T> {
        match self {
            Self::Shared(_) => None,
            Self::Mutable(value) => value.downcast_mut(),
        }
    }
}

/// The lent arguments a call reached, by argument index.
type LentArguments<'r> = [(usize, Option<Reached<'r>>)];

/// Reaches the value `lent` names through the accessor its trampoline made
/// for a `T`, mutably or not, and hands it to `visit`.
type Reach = fn(__rawr_internal::scope::Lent, bool, &mut dyn FnMut(Reached<'_>)) -> Result<(), Error>;

#[allow(dead_code)]
fn reach_lent<T: 'static>(
    lent: __rawr_internal::scope::Lent,
    mutable: bool,
    visit: &mut dyn FnMut(Reached<'_>),
) -> Result<(), Error> {
    let failed = |error| Error::Lent(lent.name(), error);
    let reached = if mutable {
        let access: fn(u64, &mut dyn FnMut(&mut T)) -> Result<(), u8> = lent.accessor(true).map_err(failed)?;
        access(lent.nonce().bits(), &mut |value| visit(Reached::Mutable(value)))
    } else {
        let access: fn(u64, &mut dyn FnMut(&T)) -> Result<(), u8> = lent.accessor(false).map_err(failed)?;
        access(lent.nonce().bits(), &mut |value| visit(Reached::Shared(value)))
    };
    reached.map_err(|code| failed(__rawr_internal::scope::Error::from_code(code)))
}

/// Run `body` with each argument at `lendable` (its index, whether the call
/// takes it `&mut`, and how to reach it) that is a value native code lent
/// (D558.3) reached for the call, nested in its accessor, as ratatui's
/// `render_stateful_widget(.., state)` takes a `&mut ListState` lent to a
/// marked function. Each is claimed for the call alone, as a borrow is.
#[allow(dead_code)]
fn with_lent_arguments(
    arguments: &mut Arguments<'_, '_, '_>,
    lendable: &[(usize, bool, Reach)],
    body: fn(&mut Arguments<'_, '_, '_>, &mut LentArguments<'_>) -> Result<Dynamic, Error>,
) -> Result<Dynamic, Error> {
    let found: Vec<_> = lendable
        .iter()
        .filter_map(|&(index, mutable, reach)| lent_value(arguments, index).map(|lent| (index, mutable, lent, reach)))
        .collect();
    nest_lent(arguments, &found, &mut Vec::new(), body)
}

#[allow(dead_code)]
fn nest_lent(
    arguments: &mut Arguments<'_, '_, '_>,
    found: &[(usize, bool, __rawr_internal::scope::Lent, Reach)],
    reached: &mut Vec<(usize, Option<Reached<'_>>)>,
    body: fn(&mut Arguments<'_, '_, '_>, &mut LentArguments<'_>) -> Result<Dynamic, Error>,
) -> Result<Dynamic, Error> {
    let Some((&(index, mutable, lent, reach), rest)) = found.split_first() else {
        return body(arguments, reached);
    };
    let mut result = None;
    reach(lent, mutable, &mut |value| {
        let mut inner: Vec<(usize, Option<Reached<'_>>)> = reached
            .iter_mut()
            .map(|(index, value)| (*index, value.as_mut().map(Reached::reborrow)))
            .collect();
        inner.push((index, Some(value)));
        result = Some(nest_lent(arguments, rest, &mut inner, body));
    })?;
    result.unwrap_or(Err(Error::Lent(lent.name(), __rawr_internal::scope::Error::Busy)))
}

/// The lent value at argument `index`, once: the call's borrow of it.
#[allow(dead_code)]
fn lent_argument<'r>(lent: &mut LentArguments<'r>, index: usize) -> Option<Reached<'r>> {
    lent.iter_mut().find(|(at, _)| *at == index).and_then(|(_, value)| value.take())
}

fn take_native<T: 'static>(arguments: &mut Arguments<'_, '_, '_>, index: usize) -> Result<T, Error> {
    let handle = native_handle(arguments, index, true)?;
    arguments.arena.take(handle)
}

/// The calls that take a value whose type holds an opaque `impl Trait`
/// type, which Rust code can't write: each one is compiled where such a
/// value is made, where rustc knows its type, and a call that takes it runs
/// the one at its index.
#[derive(Clone, Copy)]
struct OpaqueTable(&'static [OpaqueEntry]);

type OpaqueEntry = Box<dyn Fn(&mut Arguments<'_, '_, '_>, OpaqueTable) -> Result<Dynamic, Error> + Send + Sync>;

impl OpaqueTable {
    #[allow(dead_code)]
    fn call(self, index: usize, arguments: &mut Arguments<'_, '_, '_>) -> Result<Dynamic, Error> {
        let entry = self.0.get(index).ok_or(Error::TypeMismatch)?;
        entry(arguments, self)
    }
}

/// The table of `entries`, each a call that takes a value of `T`'s type.
#[allow(dead_code)]
fn opaque_table<T: 'static, const N: usize>(
    _: &T,
    entries: [fn(&mut Arguments<'_, '_, '_>, OpaqueTable, std::marker::PhantomData<T>) -> Result<Dynamic, Error>; N],
) -> Box<[OpaqueEntry]> {
    entries
        .into_iter()
        .map(|entry| {
            Box::new(move |arguments: &mut Arguments<'_, '_, '_>, table: OpaqueTable| {
                entry(arguments, table, std::marker::PhantomData)
            }) as OpaqueEntry
        })
        .collect()
}

/// Strings a call takes as `&'static str`, as iced's `Font::new` does,
/// pinned for as long as the runner lives.
#[allow(dead_code)]
fn static_pins() -> &'static __rawr_internal::scope::Pins {
    static PINS: std::sync::OnceLock<__rawr_internal::scope::Pins> = std::sync::OnceLock::new();
    PINS.get_or_init(__rawr_internal::scope::Pins::default)
}

/// Bytes a call takes as `&'static [u8]`, as a font's are, kept for as long
/// as the runner lives: once for each content, however often it is given.
#[allow(dead_code)]
fn static_bytes(bytes: Vec<u8>) -> &'static [u8] {
    static KEPT: std::sync::Mutex<Vec<&'static [u8]>> = std::sync::Mutex::new(Vec::new());
    let mut kept = KEPT.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(found) = kept.iter().find(|kept| ***kept == *bytes) {
        return found;
    }
    let leaked: &'static [u8] = Box::leak(bytes.into_boxed_slice());
    kept.push(leaked);
    leaked
}

/// [`take_native`] at the type `witness` names, which Rust code can't write.
#[allow(dead_code)]
fn take_like<T: 'static>(
    arguments: &mut Arguments<'_, '_, '_>,
    index: usize,
    _witness: std::marker::PhantomData<T>,
) -> Result<T, Error> {
    take_native(arguments, index)
}

fn take_bytes<'a>(input: &mut &'a [u8], length: usize) -> Result<&'a [u8], Error> {
    if length > input.len() { return Err(Error::Malformed); }
    let (value, remaining) = input.split_at(length);
    *input = remaining;
    Ok(value)
}

fn decode_value(input: &mut &[u8]) -> Result<Dynamic, Error> {
    let tag = take_bytes(input, 1)?[0];
    Ok(match tag {
        0 => Dynamic::from(()),
        1 => match take_bytes(input, 1)?[0] {
            0 => Dynamic::from(false),
            1 => Dynamic::from(true),
            _ => return Err(Error::Malformed),
        },
        2 => Dynamic::from(i64::from_le_bytes(take_bytes(input, 8)?.try_into().map_err(|_| Error::Malformed)?)),
        3 => Dynamic::from(f64::from_bits(u64::from_le_bytes(take_bytes(input, 8)?.try_into().map_err(|_| Error::Malformed)?))),
        4 => Dynamic::from(char::from_u32(u32::from_le_bytes(take_bytes(input, 4)?.try_into().map_err(|_| Error::Malformed)?)).ok_or(Error::Malformed)?),
        5 => {
            let length = u32::from_le_bytes(take_bytes(input, 4)?.try_into().map_err(|_| Error::Malformed)?) as usize;
            let value = std::str::from_utf8(take_bytes(input, length)?).map_err(|_| Error::Malformed)?;
            Dynamic::from(ImmutableString::from(value))
        }
        6 => {
            let epoch = take_bytes(input, 32)?.try_into().map_err(|_| Error::Malformed)?;
            let slot = u32::from_le_bytes(take_bytes(input, 4)?.try_into().map_err(|_| Error::Malformed)?);
            let generation = u64::from_le_bytes(take_bytes(input, 8)?.try_into().map_err(|_| Error::Malformed)?);
            // A handle the protocol gave out shares its claim (review 5).
            let release = EXPORTED.with_borrow(|exported| {
                exported
                    .iter()
                    .find(|handle| handle.epoch == epoch && handle.slot == slot && handle.generation == generation)
                    .and_then(|handle| handle.release.clone())
            });
            Dynamic::from(NativeHandle { epoch, slot, generation, release })
        }
        7 => match take_bytes(input, 1)?[0] {
            0 => __rawr_internal::option::none(),
            1 => __rawr_internal::option::some(decode_value(input)?),
            _ => return Err(Error::Malformed),
        },
        8 => match take_bytes(input, 1)?[0] {
            0 => __rawr_internal::result::ok(decode_value(input)?),
            1 => __rawr_internal::result::err(decode_value(input)?),
            _ => return Err(Error::Malformed),
        },
        _ => return Err(Error::Malformed),
    })
}

fn push_bytes(output: &mut Vec<u8>, value: &[u8]) -> Result<(), Error> {
    let actual = output.len().checked_add(value.len()).and_then(|bytes| bytes.checked_add(5)).ok_or(Error::Allocation)?;
    if actual > 8 * 1024 * 1024 { return Err(Error::Allocation); }
    output.try_reserve_exact(value.len()).map_err(|_| Error::Allocation)?;
    output.extend_from_slice(value);
    Ok(())
}

fn encode_value(value: Dynamic, output: &mut Vec<u8>) -> Result<(), Error> {
    if value.is::<()>() {
        let _: () = value.try_cast_result().map_err(|_| Error::TypeMismatch)?;
        output.push(0);
    } else if value.is::<bool>() {
        output.push(1);
        output.push(u8::from(value.try_cast_result::<bool>().map_err(|_| Error::TypeMismatch)?));
    } else if value.is::<i64>() {
        output.push(2);
        push_bytes(output, &value.try_cast_result::<i64>().map_err(|_| Error::TypeMismatch)?.to_le_bytes())?;
    } else if value.is::<f64>() {
        output.push(3);
        push_bytes(output, &value.try_cast_result::<f64>().map_err(|_| Error::TypeMismatch)?.to_bits().to_le_bytes())?;
    } else if value.is::<char>() {
        output.push(4);
        push_bytes(output, &u32::from(value.try_cast_result::<char>().map_err(|_| Error::TypeMismatch)?).to_le_bytes())?;
    } else if value.is::<ImmutableString>() {
        let value = value.try_cast_result::<ImmutableString>().map_err(|_| Error::TypeMismatch)?;
        output.push(5);
        let length = u32::try_from(value.len()).map_err(|_| Error::Allocation)?;
        push_bytes(output, &length.to_le_bytes())?;
        push_bytes(output, value.as_bytes())?;
    } else if value.is::<NativeHandle>() {
        let value = value.try_cast_result::<NativeHandle>().map_err(|_| Error::TypeMismatch)?;
        EXPORTED.with_borrow_mut(|exported| exported.push(value.clone()));
        output.push(6);
        push_bytes(output, &value.epoch)?;
        push_bytes(output, &value.slot.to_le_bytes())?;
        push_bytes(output, &value.generation.to_le_bytes())?;
    } else if __rawr_internal::option::is(&value) {
        let mut value = value;
        match __rawr_internal::option::take(&mut value).ok_or(Error::TypeMismatch)? {
            None => push_bytes(output, &[7, 0])?,
            Some(inner) => {
                push_bytes(output, &[7, 1])?;
                encode_value(inner, output)?;
            }
        }
    } else if __rawr_internal::result::is(&value) {
        let mut value = value;
        match __rawr_internal::result::take(&mut value).ok_or(Error::TypeMismatch)? {
            Ok(inner) => {
                push_bytes(output, &[8, 0])?;
                encode_value(inner, output)?;
            }
            Err(inner) => {
                push_bytes(output, &[8, 1])?;
                encode_value(inner, output)?;
            }
        }
    } else {
        return Err(Error::TypeMismatch);
    }
    Ok(())
}

fn invoke_adapter(key: &[u8; 32], mut payload: &[u8]) -> Result<Vec<u8>, Error> {
    if payload.len() < 100 || &payload[..32] != key { return Err(Error::Stale); }
    payload = &payload[32..];
    let identity: [u8; 32] = take_bytes(&mut payload, 32)?.try_into().map_err(|_| Error::Malformed)?;
    let signature: [u8; 32] = take_bytes(&mut payload, 32)?.try_into().map_err(|_| Error::Malformed)?;
    let count = u32::from_le_bytes(take_bytes(&mut payload, 4)?.try_into().map_err(|_| Error::Malformed)?);
    let adapter = adapters().find(|adapter| adapter.identity == identity && adapter.signature == signature).ok_or(Error::MissingAdapter)?;
    if count != adapter.arguments { return Err(Error::Cardinality); }
    let count = usize::try_from(count).map_err(|_| Error::Cardinality)?;
    let mut values = Vec::new();
    values.try_reserve_exact(count).map_err(|_| Error::Allocation)?;
    for _ in 0..count { values.push(decode_value(&mut payload)?); }
    if !payload.is_empty() { return Err(Error::Malformed); }
    let mut references: Vec<&mut Dynamic> = values.iter_mut().collect();
    let value = with_arena(|arena| {
        let mut arguments = Arguments { values: &mut references, arena };
        (adapter.invoke)(&mut arguments)
    })?;
    let mut output = Vec::new();
    output.try_reserve_exact(77).map_err(|_| Error::Allocation)?;
    output.extend_from_slice(key);
    encode_value(value, &mut output)?;
    Ok(output)
}

fn runtime_error(error: impl std::fmt::Debug) -> Box<__rawr_internal::EvalAltResult> {
    __rawr_internal::EvalAltResult::ErrorRuntime(
        format!("{error:?}").into(),
        __rawr_internal::Position::NONE,
    )
    .into()
}

/// The witness-mode bit of an adapter whose output holds script values an
/// erased generic gave back, which are checked against the call's witness.
const CHECK_VALUES: u8 = 0x80;

thread_local! {
    /// The witness of each operand of the call an adapter runs: a closure's
    /// parameters take theirs (review 2).
    static OPERANDS: RefCell<Vec<Option<__rawr_internal::host::runtime::Witness>>> =
        const { RefCell::new(Vec::new()) };
    /// The witness of the output of the call an adapter runs: a borrow
    /// guard's place gives its reads it, as the call would its output.
    static OUTPUT: RefCell<Option<__rawr_internal::host::runtime::Witness>> =
        const { RefCell::new(None) };
}

/// The witness of the output of the call the adapter runs.
#[allow(dead_code)]
fn output_witness() -> Option<__rawr_internal::host::runtime::Witness> {
    OUTPUT.with_borrow(Clone::clone)
}

/// `value`, a read of a guard's target, attested as the guard's call's
/// output is.
#[allow(dead_code)]
fn attest_read(
    arena: &mut Arena,
    value: Dynamic,
    witness: &Option<__rawr_internal::host::runtime::Witness>,
) -> Result<Dynamic, Error> {
    if let Some(witness) = witness {
        match value.read_lock::<NativeHandle>().map(|handle| handle.clone()) {
            Some(handle) => arena.attest(handle, witness.clone())?,
            None => {
                attest_inner(&value, witness, arena)?;
                attest_elements(&value, witness, arena)?;
            }
        }
    }
    Ok(value)
}

/// A native value a closure's input crosses as, with the closure
/// parameter's witness when the call that made the closure had one.
#[allow(dead_code)]
fn attest_input(
    arena: &mut Arena,
    value: Dynamic,
    witness: Option<&Option<__rawr_internal::host::runtime::Witness>>,
) -> Result<Dynamic, Error> {
    match witness {
        Some(Some(witness)) => attest_nested(arena, value, witness),
        _ => Ok(value),
    }
}

/// `value` with each native value in it attested, as its place in
/// `witness` gives: inside an `Option`, a `Result` or an array too, as the
/// `Vec<Family>` of iced's `Result<Vec<Family>, Error>` is.
#[allow(dead_code)]
fn attest_nested(
    arena: &mut Arena,
    mut value: Dynamic,
    witness: &__rawr_internal::host::runtime::Witness,
) -> Result<Dynamic, Error> {
    if let Some(handle) = value.read_lock::<NativeHandle>().map(|handle| handle.clone()) {
        arena.attest(handle, witness.clone())?;
        return Ok(value);
    }
    if __rawr_internal::option::is(&value) {
        return Ok(match __rawr_internal::option::take(&mut value).ok_or(Error::TypeMismatch)? {
            Some(inner) => match witness.argument_witness(0) {
                Some(element) => __rawr_internal::option::some(attest_nested(arena, inner, &element)?),
                None => __rawr_internal::option::some(inner),
            },
            None => __rawr_internal::option::none(),
        });
    }
    if __rawr_internal::result::is(&value) {
        return Ok(match __rawr_internal::result::take(&mut value).ok_or(Error::TypeMismatch)? {
            Ok(inner) => __rawr_internal::result::ok(match witness.argument_witness(0) {
                Some(ok) => attest_nested(arena, inner, &ok)?,
                None => inner,
            }),
            Err(inner) => __rawr_internal::result::err(match witness.argument_witness(1) {
                Some(err) => attest_nested(arena, inner, &err)?,
                None => inner,
            }),
        });
    }
    if value.is_array() {
        let Some(element) = witness.argument_witness(0) else {
            return Ok(value);
        };
        let items = value.into_array().map_err(|_| Error::TypeMismatch)?;
        return items
            .into_iter()
            .map(|item| attest_nested(arena, item, &element))
            .collect::<Result<Vec<_>, Error>>()
            .map(Dynamic::from_array);
    }
    Ok(value)
}

/// The witnesses of the `count` parameters of the closure that is operand
/// `index` of the running call.
#[allow(dead_code)]
fn closure_inputs(index: usize, count: usize) -> Vec<Option<__rawr_internal::host::runtime::Witness>> {
    OPERANDS.with_borrow(|operands| {
        let closure = operands.get(index).cloned().flatten();
        (0..count)
            .map(|input| closure.as_ref().and_then(|closure| closure.argument_witness(input)))
            .collect()
    })
}

fn runtime_invoke(
    adapter: Adapter,
    call: &__rawr_internal::host::runtime::Call<'_>,
    values: &mut [&mut Dynamic],
) -> Result<Dynamic, Box<__rawr_internal::EvalAltResult>> {
    OPERANDS.set((0..values.len()).map(|index| call.operand_witness(index).ok()).collect());
    OUTPUT.set(
        call.output_witness()
            .or_else(|_| call.borrowed_output_witness())
            .ok(),
    );
    let output = invoke_in_arena(adapter, call, values);
    OPERANDS.with_borrow_mut(Vec::clear);
    OUTPUT.set(None);
    let output = output?;
    // Script values that came back are checked outside the arena's borrow:
    // a member of an imported type attests through the arena (review 9).
    if adapter.witness & CHECK_VALUES != 0 {
        check_values(call.output_witness().ok().as_ref(), &output).map_err(runtime_error)?;
    }
    Ok(output)
}

fn invoke_in_arena(
    adapter: Adapter,
    call: &__rawr_internal::host::runtime::Call<'_>,
    values: &mut [&mut Dynamic],
) -> Result<Dynamic, Box<__rawr_internal::EvalAltResult>> {
    with_arena(|arena| {
        let receiver = values
            .first()
            .and_then(|value| value.read_lock::<NativeHandle>().map(|value| value.clone()))
            .and_then(|handle| arena.witness(handle).cloned());
        let mut arguments = Arguments { values, arena };
        let output = (adapter.invoke)(&mut arguments)?;
        // A projected iced Task takes this call's output witness.
        let output = match output.try_cast_result::<__rawr_internal::host::runtime::Unwitnessed>() {
            Ok(pending) => {
                // A method's task takes its witness from its receiver's.
                let witness = call
                    .output_witness()
                    .ok()
                    .or_else(|| {
                        receiver
                            .as_ref()
                            .and_then(|witness| call.transform_native_witness(witness).ok())
                    })
                    // An async method that borrows its receiver (D513.6).
                    .or_else(|| call.borrowed_output_witness().ok())
                    .ok_or(Error::Stale)?;
                pending.witness(witness).ok_or(Error::Stale)?
            }
            Err(output) => output,
        };
        if let Some(handle) = output.read_lock::<NativeHandle>().map(|value| value.clone()) {
            let witness = match adapter.witness & !CHECK_VALUES {
                1 => call.output_witness().ok(),
                2 => receiver
                    .as_ref()
                    .and_then(|witness| call.transform_native_witness(witness).ok()),
                // A method that borrows its receiver gives an output of its
                // declared type, a receiver native code lent (D558.3) too.
                _ => call.borrowed_output_witness().ok(),
            };
            if let Some(witness) = witness {
                arguments.arena.attest(handle, witness)?;
            }
        } else if let Ok(witness) = call
            .output_witness()
            .or_else(|_| call.borrowed_output_witness())
        {
            // A Rust value inside an `Option` or a `Result` takes the
            // witness of its own type argument.
            attest_inner(&output, &witness, arguments.arena)?;
            attest_elements(&output, &witness, arguments.arena)?;
        }
        Ok(output)
    })
    .map_err(runtime_error)
}

fn attest_inner(
    value: &Dynamic,
    witness: &__rawr_internal::host::runtime::Witness,
    arena: &mut Arena,
) -> Result<(), Error> {
    let mut copy = value.clone();
    let inner = if __rawr_internal::option::is(value) {
        __rawr_internal::option::take(&mut copy).flatten().map(|inner| (inner, 0))
    } else if __rawr_internal::result::is(value) {
        match __rawr_internal::result::take(&mut copy) {
            Some(Ok(inner)) => Some((inner, 0)),
            Some(Err(inner)) => Some((inner, 1)),
            None => None,
        }
    } else {
        None
    };
    if let Some((inner, index)) = &inner
        && let Some(handle) = inner.read_lock::<NativeHandle>().map(|value| value.clone())
        && let Some(witness) = witness.argument_witness(*index)
    {
        arena.attest(handle, witness)?;
    } else if let Some((inner, index)) = inner_tuple(value)
        && let Some(tuple) = witness.argument_witness(index)
    {
        // A variant's fields, as its matcher gives them.
        for (member, element) in inner.iter().enumerate() {
            if let Some(handle) = element.read_lock::<NativeHandle>().map(|value| value.clone())
                && let Some(witness) = tuple.argument_witness(member)
            {
                arena.attest(handle, witness)?;
            }
        }
    } else if let Some((inner, index)) = &inner
        && inner.as_array_ref().is_ok()
        && let Some(witness) = witness.argument_witness(*index)
    {
        // A tuple or a `Vec` inside a `Result`, as changelog's `Result<(Self,
        // Vec<Contribution>), Error>`: each Rust value in it takes the
        // witness of its own type.
        attest_elements(inner, &witness, arena)?;
    }
    Ok(())
}

/// The Rust values inside an array or tuple, as a `Vec`, a slice or a
/// tuple output gives them, each with the witness of its own type.
fn attest_elements(
    value: &Dynamic,
    witness: &__rawr_internal::host::runtime::Witness,
    arena: &mut Arena,
) -> Result<(), Error> {
    let Ok(elements) = value.as_array_ref() else {
        return Ok(());
    };
    for (index, element) in elements.iter().enumerate() {
        // An array's elements share its argument; a tuple's members have
        // one each.
        let Some(witness) = witness
            .argument_witness(index)
            .or_else(|| witness.argument_witness(0))
        else {
            continue;
        };
        match element.read_lock::<NativeHandle>().map(|value| value.clone()) {
            Some(handle) => arena.attest(handle, witness)?,
            None => attest_elements(element, &witness, arena)?,
        }
    }
    Ok(())
}

/// The members of the tuple inside an `Option`'s `Some`.
fn inner_tuple(value: &Dynamic) -> Option<(Vec<Dynamic>, usize)> {
    let mut copy = value.clone();
    let inner = __rawr_internal::option::take(&mut copy).flatten()?;
    let members = inner.as_array_ref().ok()?.to_vec();
    Some((members, 0))
}

fn runtime_project(
    call: &__rawr_internal::host::runtime::Call<'_>,
    values: &mut [&mut Dynamic],
) -> Result<Dynamic, Box<__rawr_internal::EvalAltResult>> {
    if values.len() != 1 {
        return Err(runtime_error(Error::Cardinality));
    }
    let value = values
        .first_mut()
        .ok_or_else(|| runtime_error(Error::MissingArgument))?;
    let handle = (**value)
        .take()
        .try_cast_result::<NativeHandle>()
        .map_err(|_| runtime_error(Error::TypeMismatch))?;
    arena_access(|arena| arena.project(handle, call))
        .unwrap_or_else(|| Err(runtime_error(Error::Stale)))
}

fn native_attestation(
    value: &Dynamic,
    expected: &__rawr_internal::host::runtime::Instantiation<'_>,
) -> Option<__rawr_internal::host::runtime::Attested> {
    let handle = value.read_lock::<NativeHandle>().map(|value| value.clone())?;
    arena_access(|arena| {
        // A value an imported type's `Default` made takes its witness here.
        if let Some(witness) = arena
            .default_of(&handle)
            .and_then(|identity| expected.default_witness(&identity))
        {
            arena.attest(handle.clone(), witness).ok()?;
        }
        expected.attest_native_witness(arena.witness(handle))
    })
    .flatten()
}

/// `==` of two imported values by their Rust type's `PartialEq`.
fn native_equal(lhs: &Dynamic, rhs: &Dynamic) -> Option<bool> {
    // A value in a scope frame's region (D558 tier 1), with one of its
    // region or one the arena holds.
    let scoped = |value: &Dynamic| value.read_lock::<__rawr_internal::scope::Scoped>().map(|scoped| *scoped);
    match (scoped(lhs), scoped(rhs)) {
        (Some(lhs), Some(rhs)) => return lhs.equal(&rhs).ok().flatten(),
        (Some(scoped), None) | (None, Some(scoped)) => {
            let other = if lhs.is::<__rawr_internal::scope::Scoped>() { rhs } else { lhs };
            let handle = other.read_lock::<NativeHandle>().map(|value| value.clone())?;
            return arena_access(|arena| arena.with_value(handle, |value| scoped.equal_native(value).ok().flatten()))
                .flatten()
                .flatten();
        }
        (None, None) => {}
    }
    let lhs = lhs.read_lock::<NativeHandle>().map(|value| value.clone())?;
    let rhs = rhs.read_lock::<NativeHandle>().map(|value| value.clone())?;
    arena_access(|arena| arena.equal(lhs, rhs)).flatten()
}

/// The order of two imported values that key a hashed collection.
fn native_compare(lhs: &Dynamic, rhs: &Dynamic) -> Option<std::cmp::Ordering> {
    let lhs = lhs.read_lock::<NativeHandle>().map(|value| value.clone())?;
    let rhs = rhs.read_lock::<NativeHandle>().map(|value| value.clone())?;
    arena_access(|arena| arena.compare(lhs, rhs)).flatten()
}

fn imported_runtime(
    engine: &__rawr_internal::Engine,
    definitions: &[__rawr_internal::host::facade::Definition],
) -> Result<__rawr_internal::host::runtime::Registry, __rawr_internal::host::runtime::Error> {
    __rawr_internal::host::value::register_relations::<NativeHandle>(native_equal, native_compare);
    __rawr_internal::host::value::register_relations::<__rawr_internal::scope::Scoped>(native_equal, native_compare);
    register_defaults();
    let mut registry = __rawr_internal::host::runtime::Registry::new();
    // The core's future, which a crate's async functions give in a program
    // with no host profile (D513.3), before the facades that name it.
    registry.install(
        __rawr_internal::future::facade::runtime()
            .map_err(__rawr_internal::host::runtime::Error::profile)?,
    )?;
    let mut native = engine.begin_imported_runtime()?;
    native.authenticate_structural(definitions, STRUCTURAL_ROWS)?;
    native.authenticate_events(definitions, EVENT_ROWS)?;
    let mut builders = definitions
        .iter()
        .cloned()
        .map(__rawr_internal::host::runtime::Builder::new)
        .collect::<Vec<_>>();
    __rawr_profile::bind_runtime(definitions, &mut builders)
        .map_err(__rawr_internal::host::runtime::Error::profile)?;
    bind_profile_projections(definitions, &mut builders)?;
    for (definition, mut builder) in definitions.iter().zip(builders) {
        let imported_rows = definition
            .types()
            .filter(|ty| ty.identity().imported().is_some())
            .count();
        let imported_identities = definition
            .types()
            .filter_map(|ty| ty.identity().imported().map(|identity| *identity.as_bytes()))
            .collect::<std::collections::BTreeSet<_>>();
        let expected_imported_identities = imported_identities.len();
        let mut installed_imported_identities = 0_usize;
        let mut installed_imported_rows = 0_usize;
        for selector in imported_identities {
            let proof = builder.bind_imported_type_attestation(selector, native_attestation)?;
            if proof.selector() != &selector {
                return Err(__rawr_internal::host::runtime::Error::profile(
                    "generated imported type attestation identity",
                ));
            }
            installed_imported_identities = installed_imported_identities
                .checked_add(1)
                .ok_or_else(|| __rawr_internal::host::runtime::Error::profile(
                    "generated imported type attestation identity count",
                ))?;
            installed_imported_rows = installed_imported_rows
                .checked_add(proof.rows())
                .ok_or_else(|| __rawr_internal::host::runtime::Error::profile(
                    "generated imported type attestation row count",
                ))?;
        }
        if installed_imported_identities != expected_imported_identities
            || installed_imported_rows != imported_rows
        {
            return Err(__rawr_internal::host::runtime::Error::profile(
                "generated imported type attestation totality",
            ));
        }
        for ty in definition.types().filter(|ty| {
            ty.identity().imported().is_none() && !ty.parameters().is_empty()
        }) {
            let path = ty.path();
            if !builder.has_type_attestation(&path) {
                builder.bind_type_attestation(path, native_attestation)?;
            }
        }
        for event in native.events().iter().filter(|event| event.root() == definition.root()) {
            builder.bind_event_replay(event, runtime_project)?;
        }
        for adapter in adapters().copied().filter(|adapter| adapter.bound && adapter.root == definition.root()) {
            // Each instance of a generic item has its own adapter, which
            // a call reaches by the key of its instance's types.
            if adapter.instance != [0; 32] {
                builder.bind_imported_instance(adapter.selector, adapter.instance, move |call, values| runtime_invoke(adapter, call, values))?;
            }
            if builder.accepts_imported(&adapter.selector) {
                builder.bind_imported(adapter.selector, move |call, values| runtime_invoke(adapter, call, values))?;
            }
        }
        // A proof-backed application has no facade selector: a call reaches
        // its adapter by the application's identity.
        for adapter in adapters().copied().filter(|adapter| adapter.application && adapter.root == definition.root()) {
            builder.bind_imported_application(adapter.identity, move |call, values| runtime_invoke(adapter, call, values))?;
        }
        for proof in native.structural().iter().filter(|proof| proof.root() == definition.root()) {
            let adapter = adapters()
                .copied()
                .find(|adapter| &adapter.identity == proof.identity())
                .ok_or_else(|| __rawr_internal::host::runtime::Error::profile("generated structural adapter identity"))?;
            builder.bind_imported_structural(proof, move |call, values| runtime_invoke(adapter, call, values))?;
        }
        registry.install(builder.finish())?;
    }
    native.finish(registry)
}

fn invoke_rejection(key: &[u8; 32], error: &Error) -> Vec<u8> {
    let mut output = Vec::with_capacity(33);
    output.extend_from_slice(key);
    output.push(error.code());
    output
}

struct Program {
    engine: __rawr_internal::Shared<__rawr_internal::Engine>,
    ast: Option<__rawr_internal::AST>,
    candidate: Option<__rawr_internal::reload::Candidate>,
    application_contract: __rawr_profile::ApplicationContract,
    /// Shared by every generation compiled against the same engine.
    _facades: std::sync::Arc<[__rawr_internal::host::facade::Definition]>,
    /// The account this program's own native reconstruction observed; a
    /// resident generation compiled against its predecessor's engine
    /// reports that engine's account instead.
    observation: Option<__rawr_internal::package::import::runner::account::Observation>,
    /// Whether `main` is Rawr code, which runs on the thread that owns the
    /// program while the protocol goes on beside it.
    marked_main: bool,
}

fn program_account(program: &Option<Program>) -> Option<__rawr_internal::package::import::runner::account::Snapshot> {
    program.as_ref().and_then(|program| match &program.observation {
        Some(observation) => observation.snapshot(),
        None => program.engine.imported_native_snapshot(),
    })
}

fn observed_account(observation: &Option<__rawr_internal::package::import::runner::account::Observation>) -> Option<__rawr_internal::package::import::runner::account::Snapshot> {
    observation.as_ref().and_then(|observation| observation.snapshot())
}

fn retire_program(program: Option<Program>) -> Option<__rawr_internal::package::import::runner::account::Observation> {
    program.and_then(|program| program.observation)
}

fn account_payload(
    tag: u8,
    request: &[u8],
    accounts: [Option<__rawr_internal::package::import::runner::account::Snapshot>; 2],
) -> Option<[u8; __rawr_internal::package::import::runner::account::wire::BYTES]> {
    use __rawr_internal::package::import::runner::account::{self, wire};
    fn pair(values: [usize; 2]) -> Option<[u64; 2]> {
        Some([u64::try_from(values[0]).ok()?, u64::try_from(values[1]).ok()?])
    }
    fn snapshot(value: account::Snapshot) -> Option<wire::Snapshot> {
        let first_refusal = match value.first_refusal {
            Some(value) => Some(wire::Refusal {
                axis: match value.axis {
                    account::Axis::Items => wire::Axis::Items,
                    account::Axis::Bytes => wire::Axis::Bytes,
                },
                cause: match value.cause {
                    account::Cause::MissingAmount => wire::Cause::MissingAmount,
                    account::Cause::OwnedOverflow => wire::Cause::OwnedOverflow,
                    account::Cause::AggregateOverflow => wire::Cause::AggregateOverflow,
                },
                current: pair(value.current)?,
                requested: match value.requested {
                    Some(value) => Some(u64::try_from(value).ok()?),
                    None => None,
                },
                limit: u64::try_from(value.limit).ok()?,
            }),
            None => None,
        };
        Some(wire::Snapshot {
            account: value.account,
            current: pair(value.current)?,
            peak: pair(value.peak)?,
            limits: pair(value.limits)?,
            first_refusal,
        })
    }
    let tag = wire::Tag::try_from(tag).ok()?;
    let request = wire::Request::from_parts(tag, request).ok()?;
    let mut converted = [None; 2];
    let mut count = 0;
    for value in accounts.into_iter().flatten() {
        let value = snapshot(value)?;
        if let Some(index) = converted.iter().position(|prior: &Option<wire::Snapshot>| {
            prior.as_ref().is_some_and(|prior| prior.account.is_some() && prior.account == value.account)
        }) {
            converted[index] = Some(value);
        } else {
            converted[count] = Some(value);
            count += 1;
        }
    }
    wire::Observation { request, accounts: converted }.encode().ok()
}

fn report_accounts(
    stream: &mut std::os::unix::net::UnixStream,
    tag: u8,
    request: &[u8],
    accounts: [Option<__rawr_internal::package::import::runner::account::Snapshot>; 2],
) {
    // Reporting grants no control authority and cannot replace an original
    // error. Missing evidence is a parent qualification fault, never a zero.
    if let Some(payload) = account_payload(tag, request, accounts) {
        let _ = send_frame(stream, 45, &payload);
    }
}

fn account_result<T>(
    result: std::io::Result<T>,
    stream: &mut std::os::unix::net::UnixStream,
    tag: u8,
    request: &[u8],
    accounts: [Option<__rawr_internal::package::import::runner::account::Snapshot>; 2],
) -> std::io::Result<T> {
    if result.is_err() { report_accounts(stream, tag, request, accounts); }
    result
}

fn snapshot_bytes<'a>(input: &mut &'a [u8], limit: usize) -> std::io::Result<&'a [u8]> {
    if input.len() < 8 { return Err(std::io::Error::other("snapshot length")); }
    let length = u64::from_le_bytes(input[..8].try_into().map_err(|_| std::io::Error::other("snapshot length"))?);
    let length = usize::try_from(length).map_err(|_| std::io::Error::other("snapshot length"))?;
    if length > limit || input.len() < 8 + length { return Err(std::io::Error::other("snapshot bound")); }
    let value = &input[8..8 + length];
    *input = &input[8 + length..];
    Ok(value)
}

fn decode_snapshot(mut input: &[u8]) -> std::io::Result<__rawr_internal::package::snapshot::Snapshot> {
    let domain = b"rawr.snapshot.entry/v1";
    if !input.starts_with(domain) { return Err(std::io::Error::other("snapshot entry")); }
    input = &input[domain.len()..];
    let entry = std::str::from_utf8(snapshot_bytes(&mut input, 1024 * 1024)?)
        .map_err(|_| std::io::Error::other("snapshot entry UTF-8"))?;
    let mut files = Vec::new();
    let mut previous: Option<&[u8]> = None;
    while !input.is_empty() {
        let path = snapshot_bytes(&mut input, 1024 * 1024)?;
        let path_text = std::str::from_utf8(path).map_err(|_| std::io::Error::other("snapshot path UTF-8"))?;
        if previous.is_some_and(|prior| prior >= path) { return Err(std::io::Error::other("snapshot order")); }
        let contents = std::str::from_utf8(snapshot_bytes(&mut input, 8 * 1024 * 1024)?)
            .map_err(|_| std::io::Error::other("snapshot source UTF-8"))?;
        files.try_reserve_exact(1).map_err(|_| std::io::Error::other("snapshot allocation"))?;
        files.push((std::path::PathBuf::from(path_text), Box::<str>::from(contents)));
        previous = Some(path);
    }
    if files.is_empty() { return Err(std::io::Error::other("snapshot files")); }
    __rawr_internal::package::snapshot::Snapshot::new(std::path::PathBuf::from(entry), files)
        .map_err(|error| std::io::Error::other(error.to_string()))
}

enum Compilation<'a> {
    Initial,
    Replacement(&'a Option<Program>),
    /// Beside a running marked `main`, against the engine it runs.
    Live(&'a __rawr_internal::Shared<__rawr_internal::Engine>),
}

fn compile_program(
    input: &[u8],
    compilation: Compilation<'_>,
    progress: &mut impl FnMut(u8) -> std::io::Result<()>,
    observation: &mut Option<__rawr_internal::package::import::runner::account::Observation>,
) -> std::io::Result<Option<Program>> {
    let (expected, input) = input
        .split_first_chunk::<32>()
        .ok_or_else(|| std::io::Error::other("application semantic"))?;
    let snapshot = decode_snapshot(input)?;
    progress(1)?;
    if !COMPILE_SNAPSHOT { return Ok(None); }
    let (native, observed) = match compilation {
        Compilation::Initial => __rawr_internal::package::import::runner::account::Native::decode_observed(GENERATED_MANIFEST),
        Compilation::Replacement(current) => current.as_ref()
            .ok_or_else(|| std::io::Error::other("missing original compiler ownership"))?
            .engine.reconstruct_imported_native(GENERATED_MANIFEST),
        Compilation::Live(engine) => engine.reconstruct_imported_native(GENERATED_MANIFEST),
    };
    *observation = Some(observed);
    let mut native = native.map_err(|error| std::io::Error::other(error.to_string()))?;
    let mut engine = __rawr_internal::Engine::new();
    let (facades, application_contract) = __rawr_profile::install_compiler_facades(&mut engine, &mut native)?;
    progress(2)?;
    __rawr_internal::package::import::runner::admit_generated_structural_retained(
        &mut engine,
        &facades,
        STRUCTURAL_ROWS,
        &mut native,
    )
    .map_err(|error| std::io::Error::other(error.to_string()))?;
    __rawr_internal::package::import::runner::admit_generated_applications(
        &mut engine,
        &facades,
        rawr_applications::APPLICATIONS,
        adapters().filter(|adapter| adapter.application).map(|adapter| adapter.identity),
    )
    .map_err(|error| std::io::Error::other(error.to_string()))?;
    progress(3)?;
    let engine = engine.retain_imported_native(native)
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    let (candidate, ast) = __rawr_profile::compile_initial(
        &engine,
        &snapshot,
        &application_contract,
        expected,
    )?;
    progress(4)?;
    progress(5)?;
    let observation = observation.take().ok_or_else(|| std::io::Error::other("native observation unavailable"))?;
    let marked_main = snapshot.marked_root();
    Ok(Some(Program { engine, ast: Some(ast), candidate, application_contract,
        _facades: facades.into(), observation: Some(observation), marked_main }))
}

fn compile_live(
    engine: &__rawr_internal::Shared<__rawr_internal::Engine>,
    application_contract: &__rawr_profile::ApplicationContract,
    input: &[u8],
    progress: &mut impl FnMut(u8) -> std::io::Result<()>,
) -> std::io::Result<Option<__rawr_internal::reload::Candidate>> {
    let (expected, input) = input
        .split_first_chunk::<32>()
        .ok_or_else(|| std::io::Error::other("application semantic"))?;
    let snapshot = decode_snapshot(input)?;
    progress(1)?;
    let candidate = __rawr_profile::compile_generation(
        engine,
        &snapshot,
        application_contract,
        expected,
    )?;
    progress(4)?;
    Ok(candidate)
}

#[cfg(test)]
mod native_arena_tests {
    use super::*;

    struct NonClone { value: i64 }

    #[test]
    fn native_account_payload_keeps_reached_failure_and_distinct_owners() {
        use __rawr_internal::package::import::runner::account::{self, wire};
        let failed = account::Snapshot {
            account: std::num::NonZeroU64::new(1),
            current: [2, 20],
            peak: [3, 30],
            limits: [4, 40],
            first_refusal: Some(account::Refusal {
                axis: account::Axis::Items,
                cause: account::Cause::OwnedOverflow,
                current: [3, 30],
                requested: Some(5),
                limit: 4,
            }),
            exceeded: Default::default(),
        };
        let request = [7; 48];
        let bytes = account_payload(3, &request, [None, Some(failed)]).expect("reached constructor failure");
        let observed = wire::Observation::decode(&bytes).expect("fixed account observation");
        let reached = observed.accounts[0].expect("failed new account is packed first");
        assert!(observed.accounts[1].is_none());
        assert_eq!(reached.current, [2, 20]);
        assert_eq!(reached.peak, [3, 30]);
        assert_eq!(reached.first_refusal.expect("original refusal").requested, Some(5));
        let mut retired = failed;
        retired.current = [1, 10];
        let bytes = account_payload(25, &request, [Some(failed), Some(retired)]).expect("one original account");
        let observed = wire::Observation::decode(&bytes).expect("coalesced observation");
        assert_eq!(observed.accounts[0].expect("latest original facts").current, [1, 10]);
        assert!(observed.accounts[1].is_none());
        retired.account = std::num::NonZeroU64::new(2);
        let bytes = account_payload(25, &request, [Some(failed), Some(retired)]).expect("distinct old and pending owners");
        let observed = wire::Observation::decode(&bytes).expect("two original accounts");
        assert!(observed.accounts.into_iter().all(|account| account.is_some()));
        let bytes = account_payload(3, &request, [None, None]).expect("unreached account");
        assert_eq!(wire::Observation::decode(&bytes).expect("unreached observation").accounts, [None, None]);
        retired.account = None;
        let bytes = account_payload(3, &request, [Some(retired), None]).expect("exhausted identity is still reached");
        assert!(wire::Observation::decode(&bytes).expect("exhausted observation").accounts[0]
            .expect("actual account remains present").account.is_none());
    }

    #[test]
    fn non_clone_values_borrow_move_reuse_and_reject_stale_handles() {
        let mut arena = Arena::new([7; 32]);
        let handle = arena.insert(NonClone { value: 2 }).expect("insert");
        assert_eq!(arena.shared::<NonClone, _>(handle.clone(), |value| value.value).expect("shared"), 2);
        arena.mutable::<NonClone, _>(handle.clone(), |value| value.value += 1).expect("mutable");
        assert!(matches!(arena.shared::<String, _>(handle.clone(), |_| ()), Err(Error::TypeMismatch)));
        assert_eq!(arena.take::<NonClone>(handle.clone()).expect("owned").value, 3);
        assert!(matches!(arena.take::<NonClone>(handle.clone()), Err(Error::StaleHandle)));
        let reused = arena.insert(NonClone { value: 5 }).expect("reuse");
        assert_eq!(reused.slot, handle.slot);
        assert_ne!(reused.generation, handle.generation);
        assert!(matches!(arena.shared::<NonClone, _>(handle, |_| ()), Err(Error::StaleHandle)));
        let other = Arena::new([8; 32]);
        assert!(matches!(other.shared::<NonClone, _>(reused, |_| ()), Err(Error::StaleHandle)));
    }

    #[test]
    fn exhausted_generation_retires_instead_of_wrapping() {
        let mut arena = Arena::new([9; 32]);
        let mut handle = arena.insert(NonClone { value: 1 }).expect("insert");
        arena.slots[handle.slot as usize].generation = u64::MAX;
        handle.generation = u64::MAX;
        assert_eq!(arena.take::<NonClone>(handle.clone()).expect("last take").value, 1);
        let next = arena.insert(NonClone { value: 2 }).expect("new slot");
        assert_ne!(next.slot, handle.slot);
    }

    /// A value in the owner's arena, and that handle.
    fn installed(epoch: u8, value: i64) -> NativeHandle {
        install_arena([epoch; 32]);
        with_arena(|arena| arena.insert(NonClone { value })).expect("insert")
    }

    fn read(handle: &NativeHandle) -> Result<i64, Error> {
        with_arena(|arena| arena.shared::<NonClone, _>(handle.clone(), |value| value.value))
    }

    #[test]
    fn a_handle_the_protocol_gave_out_stays_alive_for_its_next_call() {
        let handle = installed(21, 6);
        let mut wire = Vec::new();
        // Encoding takes the call's output, its only handle (review 5).
        encode_value(Dynamic::from(handle), &mut wire).expect("encode");
        let decoded = decode_value(&mut wire.as_slice())
            .expect("decode")
            .try_cast::<NativeHandle>()
            .expect("a handle");
        assert_eq!(read(&decoded).expect("alive"), 6);
    }

    #[test]
    fn a_lent_value_given_back_on_a_thread_without_the_arena_returns_at_its_next_use() {
        let handle = installed(22, 4);
        let held = with_arena(|arena| arena.lease::<NonClone>(handle.clone())).expect("lease");
        // An executor's thread, as iced's, has no arena (review 6).
        std::thread::spawn(move || drop(held)).join().expect("dropped elsewhere");
        assert_eq!(read(&handle).expect("restored"), 4);
    }

    #[test]
    fn a_lent_value_given_back_while_the_arena_is_borrowed_returns_after() {
        let handle = installed(23, 5);
        let held = with_arena(|arena| arena.lease::<NonClone>(handle.clone())).expect("lease");
        // A native call that drops the future holds the arena (review 7).
        with_arena(|_| {
            drop(held);
            Ok(())
        })
        .expect("dropped inside a call");
        assert_eq!(read(&handle).expect("restored"), 5);
    }
}
"#;
const NEUTRAL_PROFILE_SOURCE: &[u8] = br#"
/// What a host profile gives the adapters of a program without one: a Rust
/// future crosses as the core's future (D513.3), made on the thread that
/// first polls it and made a Rawr value on the thread that owns the program.
mod __rawr_profile_host {
    pub mod task {
        use __rawr_internal::{host, Dynamic};

        pub type Materialize =
            Box<dyn FnOnce(Option<&host::runtime::Witness>) -> Result<Dynamic, String> + Send + 'static>;
        type Boxed<T> = std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send>>;

        /// A Rust future, not yet made.
        pub struct Task<T>(Box<dyn FnOnce() -> Boxed<T> + Send>);

        impl<T: Send + 'static> Task<T> {
            pub fn map<U: Send + 'static>(self, f: impl FnOnce(T) -> U + Send + 'static) -> Task<U> {
                Task(Box::new(move || {
                    let future = (self.0)();
                    Box::pin(async move { f(future.await) })
                }))
            }
        }

        /// A Rust function that returns a future runs when the future is
        /// first polled, where its runtime is, as tokio's `sleep` needs.
        pub fn lazy<F>(make: impl FnOnce() -> F + Send + 'static) -> Task<F::Output>
        where
            F: std::future::Future + Send + 'static,
            F::Output: Send + 'static,
        {
            // Inside the runtime entry's poll, the call runs now, in the
            // runtime's context, as Rust makes it: `spawn` starts its task
            // and `sleep` fixes its deadline (D527).
            if __rawr_internal::future::entered() {
                let future = make();
                return Task(Box::new(move || Box::pin(future)));
            }
            // Elsewhere it runs at the first poll, on whatever runs the
            // future: a runtime the call needs and that thread lacks is
            // named, not reported as tokio's missing reactor (D527).
            Task(Box::new(move || {
                match std::panic::catch_unwind(std::panic::AssertUnwindSafe(make)) {
                    Ok(future) => Box::pin(future),
                    Err(payload) => {
                        let said = payload
                            .downcast_ref::<&str>()
                            .map(|said| (*said).to_owned())
                            .or_else(|| payload.downcast_ref::<String>().cloned())
                            .unwrap_or_default();
                        if said.contains("reactor") || said.contains("runtime") {
                            panic!(
                                "rawr: this native call needs its async runtime, and the program runs none: give `async fn main` the runtime's attribute, as `#[tokio::main]` (D527). It said: {said}"
                            );
                        }
                        std::panic::resume_unwind(payload)
                    }
                }
            }))
        }

        pub struct Project<T>(std::cell::Cell<Option<T>>);

        impl<T> Project<T> {
            pub fn new(value: T) -> Self {
                Self(std::cell::Cell::new(Some(value)))
            }

            fn take(&self) -> T {
                self.0.take().expect("a projected Rust output is taken once")
            }
        }

        pub trait ViaTask<T> {
            #[allow(clippy::wrong_self_convention)]
            fn into_task(&self) -> Task<T>;
        }

        impl<T> ViaTask<T> for Project<Task<T>> {
            fn into_task(&self) -> Task<T> {
                self.take()
            }
        }

        pub trait ViaFuture<T> {
            #[allow(clippy::wrong_self_convention)]
            fn into_task(&self) -> Task<T>;
        }

        impl<F> ViaFuture<F::Output> for &Project<F>
        where
            F: std::future::Future + Send + 'static,
            F::Output: Send + 'static,
        {
            fn into_task(&self) -> Task<F::Output> {
                let future = self.take();
                Task(Box::new(move || Box::pin(future)))
            }
        }

        /// The core's future of a Rust future, stamped with the call's
        /// output witness, whose argument its output takes.
        pub fn imported(task: Task<Materialize>) -> Dynamic {
            Dynamic::from(host::runtime::Unwitnessed::new(move |witness| {
                let element = witness.argument_witness(0);
                Dynamic::from(__rawr_internal::future::Future::native(move || {
                    let future = (task.0)();
                    Box::pin(async move {
                        let materialize = future.await;
                        Box::new(move || {
                            materialize(element.as_ref()).map_err(__rawr_internal::future::error)
                        }) as __rawr_internal::future::Finish
                    })
                }))
            }))
        }
    }
}

mod __rawr_profile {
    #[derive(Clone)]
    pub struct ApplicationContract;

    pub fn compile_initial(
        engine: &__rawr_internal::Engine,
        snapshot: &__rawr_internal::package::snapshot::Snapshot,
        _contract: &ApplicationContract,
        _expected: &[u8; 32],
    ) -> std::io::Result<(Option<__rawr_internal::reload::Candidate>, __rawr_internal::AST)> {
        engine.compile_package_snapshot_generation_with_top_level(snapshot)
            .map(|(candidate, ast)| (Some(candidate), ast))
            .map_err(|error| std::io::Error::other(error.to_string()))
    }

    thread_local! {
        /// A marked `main`'s runtime registry, which each generation it
        /// admits binds a next generation of (D475.2, D553.4), so native
        /// values keep their witnesses.
        static RUNTIME: std::cell::RefCell<Option<__rawr_internal::host::runtime::Registry>> =
            const { std::cell::RefCell::new(None) };
        /// The program a marked `main` admitted last, kept while it runs.
        static ADMITTED: std::cell::RefCell<Option<super::Program>> =
            const { std::cell::RefCell::new(None) };
    }

    fn bind(program: &mut super::Program) -> Result<__rawr_internal::reload::installation::Bootstrap, String> {
        let candidate = program.candidate.take().ok_or("generation program unavailable")?;
        let kept = program
            .marked_main
            .then(|| RUNTIME.with_borrow(|runtime| runtime.as_ref().map(|runtime| runtime.next_generation())))
            .flatten();
        let runtime = match kept {
            Some(runtime) => runtime,
            None => {
                let runtime = super::imported_runtime(&program.engine, &program._facades)
                    .map_err(|error| error.to_string())?;
                if program.marked_main {
                    RUNTIME.set(Some(runtime.next_generation()));
                }
                runtime
            }
        };
        candidate.bind(&program.engine, runtime).map_err(|error| error.to_string())
    }

    /// Make `program`, compiled beside a running marked `main`, the
    /// generation its next calls from native code run in (D553.3). This
    /// runs on the thread that owns the program, at an outermost entry. The
    /// arena stays: its native values live on (D553.4).
    pub fn admit(mut program: super::Program) -> Result<(), String> {
        __rawr_internal::island::publish(bind(&mut program)?);
        ADMITTED.set(Some(program));
        super::admitted();
        Ok(())
    }

    /// Run the program's `fn main` once, as Rust runs it, bound to the
    /// imported runtime: an `Err` from `main` is `Error: {err:?}` on stderr
    /// with status 1, and a run-time failure is a panic, status 101. The
    /// status is the process's.
    ///
    /// A program whose crate root is native (D501) registers its native
    /// `main` from its Rawr one. `None` is that `main` running on a thread
    /// of its own, its calls into marked code handed to `forward` for this
    /// thread to answer; the process exits when it returns.
    pub fn run(
        program: &mut super::Program,
        forward: impl Fn(__rawr_internal::island::Call) + Send + Sync + 'static,
    ) -> super::Started {
        let bootstrap = match bind(program) {
            Ok(bootstrap) => bootstrap,
            Err(error) => {
                eprintln!("error: {error}");
                return super::Started::Exit(101);
            }
        };
        __rawr_internal::island::publish(bootstrap);
        let mut result = __rawr_internal::island::invoke_main();
        // An async `main` gives its future, which the native entry its
        // runtime's attribute carries awaits (D513.4); its `main` registered
        // that entry.
        if let Some(Ok(value)) = &result
            && value.is::<__rawr_internal::future::Future>()
        {
            let Some(Ok(value)) = result.take() else {
                unreachable!("the result was just matched");
            };
            __rawr_internal::island::await_main(value.cast());
            // This thread owns the program and runs the entry (D527).
            return match __rawr_internal::island::take_entry() {
                Some(entry) => super::Started::Entry(entry),
                None => {
                    eprintln!("error[E0752]: `main` function is not allowed to be `async` without a runtime's entry, such as `#[tokio::main]`");
                    super::Started::Exit(101)
                }
            };
        }
        if __rawr_internal::island::start(forward) {
            return super::Started::Serving;
        }
        super::Started::Exit(match result {
            None => 0,
            Some(Ok(value)) => match value.try_cast::<Result<__rawr_internal::Dynamic, __rawr_internal::Dynamic>>() {
                Some(Err(error)) => {
                    eprintln!("Error: {error:?}");
                    1
                }
                _ => 0,
            },
            Some(Err(error)) => {
                eprintln!("error: {error}");
                101
            }
        })
    }

    /// Make a committed generation the one a running native `main` calls.
    pub fn publish(program: &mut super::Program) -> Result<(), String> {
        __rawr_internal::island::publish(bind(program)?);
        Ok(())
    }

    pub fn compile_generation(
        _engine: &__rawr_internal::Engine,
        _snapshot: &__rawr_internal::package::snapshot::Snapshot,
        _contract: &ApplicationContract,
        _expected: &[u8; 32],
    ) -> std::io::Result<Option<__rawr_internal::reload::Candidate>> {
        Ok(None)
    }
    /// Compile the next generation once against the running program's
    /// engine and its retained native surface (D355).
    pub fn compile_resident(
        engine: &__rawr_internal::Engine,
        snapshot: &__rawr_internal::package::snapshot::Snapshot,
        _contract: &ApplicationContract,
    ) -> Result<__rawr_internal::package::import::resident::Outcome, __rawr_internal::package::import::resident::Error> {
        engine.compile_package_snapshot_resident_plain(snapshot)
    }
    pub fn bind_runtime(
        _definitions: &[__rawr_internal::host::facade::Definition],
        _builders: &mut [__rawr_internal::host::runtime::Builder],
    ) -> Result<(), &'static str> {
        Ok(())
    }
    pub fn install_compiler_facades(
        engine: &mut __rawr_internal::Engine,
        native: &mut __rawr_internal::package::import::runner::account::Native,
    ) -> std::io::Result<(Box<[__rawr_internal::host::facade::Definition]>, ApplicationContract)> {
        // Async code compiles against the core's future (D513.3).
        engine
            .register_host_facade(
                __rawr_internal::future::facade::definition().map_err(std::io::Error::other)?,
            )
            .map_err(std::io::Error::other)?;
        if super::IMPORTED_FACADES.is_empty() {
            return Ok((Box::default(), ApplicationContract));
        }
        __rawr_internal::package::import::runner::install_retained(engine, super::IMPORTED_FACADES, native)
            .map(|definitions| (definitions, ApplicationContract))
            .map_err(|error| std::io::Error::other(error.to_string()))
    }
}
"#;
const RUNNER_MAIN: &[u8] = br#"
fn send_frame(stream: &mut std::os::unix::net::UnixStream, tag: u8, payload: &[u8]) -> std::io::Result<()> {
    use std::io::Write as _;
    let length = 5_usize.checked_add(payload.len()).ok_or_else(|| std::io::Error::other("frame length"))?;
    if length > 8 * 1024 * 1024 { return Err(std::io::Error::other("frame limit")); }
    stream.write_all(&(length as u32).to_be_bytes())?;
    stream.write_all(&11_u32.to_be_bytes())?;
    stream.write_all(&[tag])?;
    stream.write_all(payload)?;
    stream.flush()
}

fn send_progress(
    stream: &mut std::os::unix::net::UnixStream,
    key: &[u8; 32],
    nonce: &[u8; 16],
    operation: u8,
    ordinal: u8,
    stage: u8,
) -> std::io::Result<()> {
    let mut payload = Vec::with_capacity(51);
    payload.extend_from_slice(key);
    payload.extend_from_slice(nonce);
    payload.extend_from_slice(&[operation, ordinal, stage]);
    send_frame(stream, 24, &payload)
}

fn receive_frame(stream: &mut std::os::unix::net::UnixStream) -> std::io::Result<(u8, Vec<u8>)> {
    use std::io::Read as _;
    let mut header = [0_u8; 4];
    stream.read_exact(&mut header)?;
    let length = u32::from_be_bytes(header) as usize;
    if !(5..=8 * 1024 * 1024).contains(&length) { return Err(std::io::Error::other("frame bound")); }
    let mut frame = vec![0_u8; length];
    stream.read_exact(&mut frame)?;
    if frame[..4] != 11_u32.to_be_bytes() { return Err(std::io::Error::other("wire version")); }
    Ok((frame[4], frame.split_off(5)))
}

fn decode_hex(value: &str) -> std::io::Result<[u8; 32]> {
    if value.len() != 64 { return Err(std::io::Error::other("capability length")); }
    let mut output = [0_u8; 32];
    for (index, byte) in output.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
            .map_err(|_| std::io::Error::other("capability encoding"))?;
    }
    Ok(output)
}

/// What the protocol thread waits for: the parent's next frame, or a call
/// from the program's native `main` into marked code (D501). Its name is
/// one no script type takes, since a script type's proxy is declared at the
/// crate root beside it (`table`'s `Event`).
enum __RawrProtocolEvent {
    Frame(std::io::Result<(u8, Vec<u8>)>),
    Call(__rawr_internal::island::Call),
}

/// The parent's next frame, answering the native calls that come first.
fn next_frame(events: &std::sync::mpsc::Receiver<__RawrProtocolEvent>) -> std::io::Result<(u8, Vec<u8>)> {
    loop {
        match events.recv() {
            Ok(__RawrProtocolEvent::Frame(frame)) => return frame,
            Ok(__RawrProtocolEvent::Call(call)) => call.answer(),
            Err(_) => return Err(std::io::Error::other("protocol reader stopped")),
        }
    }
}

/// What starting the program left this thread to do.
enum Started {
    /// The program ended with this status.
    Exit(i32),
    /// A native `main` runs on its own thread, and this one serves its calls.
    Serving,
    /// An async `main`'s runtime entry, for this thread to run.
    Entry(fn() -> i32),
}

fn exit(status: i32) -> ! {
    use std::io::Write as _;
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    std::process::exit(status)
}

/// What the protocol thread keeps of a running marked `main`'s program to
/// compile the next generation beside it (D553.3): the engine it compiles
/// against and the generation it compares with.
struct Live {
    engine: __rawr_internal::Shared<__rawr_internal::Engine>,
    generation: std::sync::Arc<__rawr_internal::reload::Generation>,
    facades: std::sync::Arc<[__rawr_internal::host::facade::Definition]>,
    contract: __rawr_profile::ApplicationContract,
}

/// The parent's early requests for a next generation (HotBegin), each
/// answered at most once and in order.
#[derive(Default)]
struct Hot {
    begun: Option<Begun>,
    sequence: u64,
}

/// One early request: its identity, and whether the parent sent the
/// inputs it watches for it. The parent's own watcher revokes the request;
/// this runner only correlates it.
struct Begun {
    identity: [u8; 56],
    observed: bool,
}

impl Hot {
    /// Whether a Compile with `nonce` may answer the current request: the
    /// one begun with it, or none begun at all (an owner that did not
    /// start the program sends none).
    fn admits(&self, nonce: &[u8]) -> bool {
        self.begun.as_ref().is_none_or(|begun| begun.identity[32..48] == *nonce)
    }
}

/// What a resident compilation compiles against: the running program's
/// engine and native surface, and the generation a running marked `main`
/// runs, which the next one must be able to replace (D553.6).
struct Resident<'a> {
    engine: &'a __rawr_internal::Shared<__rawr_internal::Engine>,
    facades: &'a std::sync::Arc<[__rawr_internal::host::facade::Definition]>,
    contract: &'a __rawr_profile::ApplicationContract,
    running: Option<&'a std::sync::Arc<__rawr_internal::reload::Generation>>,
}

enum ResidentResult {
    Compiled(Program, Option<[u8; 32]>, [u8; 32], [u8; 32]),
    Miss,
    Diagnostic(String),
    Resource(String),
}

/// Compile the next generation once, against the running engine (D355).
/// A native requirement its surface lacks, or a generation the running
/// `main` can't take, needs the parent's full preparation.
fn compile_resident(
    resident: &Resident<'_>,
    snapshot: &__rawr_internal::package::snapshot::Snapshot,
) -> std::io::Result<ResidentResult> {
    use __rawr_internal::package::import::resident::{Error, Outcome};
    let compiled = match __rawr_profile::compile_resident(resident.engine, snapshot, resident.contract) {
        Ok(Outcome::Compiled(compiled)) => compiled,
        Ok(Outcome::NeedsPreparation) => return Ok(ResidentResult::Miss),
        Err(Error::Source(error)) => return Ok(ResidentResult::Diagnostic(error.to_string())),
        Err(Error::Resource(error)) => return Ok(ResidentResult::Resource(error.to_string())),
        Err(error) => return Err(std::io::Error::other(error.to_string())),
    };
    let (compiled, ast) = compiled.with_top_level();
    let (candidate, semantic, carrier, coverage) = compiled.into_parts();
    if let Some(running) = resident.running
        && __rawr_internal::island::refusal(running, &candidate).is_some()
    {
        return Ok(ResidentResult::Miss);
    }
    let program = Program {
        engine: resident.engine.clone(),
        ast: Some(ast),
        candidate: Some(candidate),
        application_contract: resident.contract.clone(),
        _facades: resident.facades.clone(),
        observation: None,
        marked_main: snapshot.marked_root(),
    };
    Ok(ResidentResult::Compiled(program, semantic.map(|semantic| *semantic.as_bytes()), carrier, coverage))
}

fn resident_response(
    stream: &mut std::os::unix::net::UnixStream,
    header: &[u8],
    tag: u8,
    diagnostic: &str,
) -> std::io::Result<()> {
    let length = header.len().checked_add(diagnostic.len())
        .filter(|length| *length <= 8 * 1024 * 1024 - 5)
        .ok_or_else(|| std::io::Error::other("resident response frame limit"))?;
    let mut payload = Vec::new();
    payload.try_reserve_exact(length)
        .map_err(|_| std::io::Error::other("resident response allocation"))?;
    payload.extend_from_slice(header);
    payload.extend_from_slice(diagnostic.as_bytes());
    send_frame(stream, tag, &payload)
}

/// Serve the parent's early request, its cancellation, its observation
/// and its one resident compilation. `false` when `tag` is none of them.
/// Without `resident`, no program runs here to compile against, and the
/// compilation needs full preparation.
fn serve_hot(
    stream: &mut std::os::unix::net::UnixStream,
    tag: u8,
    payload: &[u8],
    key: &[u8; 32],
    hot: &mut Hot,
    pending: &mut Option<PendingProgram>,
    resident: Option<Resident<'_>>,
) -> std::io::Result<bool> {
    let accounts = || [resident.as_ref().and_then(|resident| resident.engine.imported_native_snapshot()), None];
    match tag {
        29 if pending.is_none() && hot.begun.is_none() && payload.len() == 56 && payload[..32] == *key => {
            let sequence = u64::from_le_bytes(payload[48..56].try_into()
                .map_err(|_| std::io::Error::other("hot sequence"))?);
            if sequence <= hot.sequence {
                return Err(std::io::Error::other("replayed hot begin"));
            }
            let identity = payload.try_into().map_err(|_| std::io::Error::other("hot begin identity"))?;
            hot.sequence = sequence;
            hot.begun = Some(Begun { identity, observed: false });
            send_frame(stream, 30, payload)?;
        }
        31 if pending.is_none() && hot.begun.as_ref().is_some_and(|begun| payload == begun.identity) => {
            hot.begun = None;
            report_accounts(stream, tag, payload, accounts());
            send_frame(stream, 32, payload)?;
        }
        33 if pending.is_none() && payload.len() > 56
            && hot.begun.as_ref().is_some_and(|begun| !begun.observed && payload[..56] == begun.identity) => {
            if let Some(begun) = hot.begun.as_mut() {
                begun.observed = true;
            }
            send_frame(stream, 34, &payload[..56])?;
        }
        29 | 31 | 33 => {
            report_accounts(stream, tag, payload, accounts());
            return Err(std::io::Error::other("invalid hot preparation transition"));
        }
        37 if pending.is_none() && hot.begun.as_ref().is_some_and(|begun| begun.observed) => {
            let begun = hot.begun.take().ok_or_else(|| std::io::Error::other("missing original resident request"))?;
            let decoded = __rawr_internal::package::import::resident::wire::Attempt::decode(payload, &begun.identity)
                .map_err(|error| std::io::Error::other(error.to_string()));
            let (attempt, input) = account_result(decoded, stream, tag, payload, accounts())?;
            let snapshot = account_result(decode_snapshot(input), stream, tag, payload, accounts())?;
            let nonce: [u8; 16] = begun.identity[32..48].try_into()
                .map_err(|_| std::io::Error::other("resident transition nonce"))?;
            send_progress(stream, key, &nonce, 3, 1, 1)?;
            let output = match &resident {
                Some(resident) => compile_resident(resident, &snapshot),
                None => Ok(ResidentResult::Miss),
            };
            let output = account_result(output, stream, tag, payload, accounts())?;
            send_progress(stream, key, &nonce, 3, 2, 4)?;
            let (response, diagnostic) = match output {
                ResidentResult::Compiled(program, semantic, carrier, coverage) => {
                    let compiled = attempt.compiled(carrier, coverage, semantic);
                    *pending = Some(PendingProgram { key: compiled.key(), nonce, program: Some(program) });
                    report_accounts(stream, tag, payload, accounts());
                    send_frame(stream, 38, compiled.encode())?;
                    return Ok(true);
                }
                ResidentResult::Miss => (39, String::new()),
                ResidentResult::Diagnostic(reason) => (40, reason),
                ResidentResult::Resource(reason) => (42, reason),
            };
            // Every other answer consumes the request it answers.
            report_accounts(stream, tag, payload, accounts());
            resident_response(stream, attempt.response_header(), response, &diagnostic)?;
        }
        37 => {
            report_accounts(stream, tag, payload, accounts());
            return Err(std::io::Error::other("invalid resident preparation transition"));
        }
        _ => return Ok(false),
    }
    Ok(true)
}

/// A generation the owner thread admitted, for a `rawr mcp` that
/// supervises the run: the owner said `queued` when it committed (D553.3).
fn admitted() {
    use std::io::Write as _;
    let Some(path) = std::env::var_os("RAWR_MCP_EVENTS").filter(|path| !path.is_empty()) else {
        return;
    };
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = file.write_all(b"{\"event\":\"swapped\"}\n");
    }
}

/// The protocol while the program's own code runs on the owner thread: an
/// async `main`'s runtime entry, which asks for no refresh, carries no
/// State and takes no hot generation, since a save restarts it (D527), or a
/// marked `main` (`live`). A marked `main`'s next generation compiles here
/// and is offered to the owner thread, which admits it at its next call
/// from native code into script code (D553.3). A generation that can't
/// replace the running one is refused with its reason, and the owner
/// restarts the program with it.
fn serve_entry(
    mut stream: std::os::unix::net::UnixStream,
    received: std::sync::mpsc::Receiver<__RawrProtocolEvent>,
    mut key: [u8; 32],
    mut live: Option<Live>,
) {
    let mut pending: Option<PendingProgram> = None;
    let mut hot = Hot::default();
    let mut serve = || -> std::io::Result<()> {
        loop {
            let (tag, payload) = next_frame(&received)?;
            let resident = live.as_ref().map(|running| Resident {
                engine: &running.engine,
                facades: &running.facades,
                contract: &running.contract,
                running: Some(&running.generation),
            });
            if serve_hot(&mut stream, tag, &payload, &key, &mut hot, &mut pending, resident)? {
                continue;
            }
            match tag {
                43 | 46 | 48 if payload.len() >= 32 && payload[..32] == key => {
                    let mut response = [0_u8; 33];
                    response[..32].copy_from_slice(&key);
                    send_frame(&mut stream, tag + 1, &response)?;
                }
                35 | 11 if payload.len() >= 32 && payload[..32] == key => {
                    send_frame(&mut stream, if tag == 35 { 36 } else { 11 }, &key)?;
                }
                9 if live.is_some() && pending.is_none() && payload.len() >= 112 && hot.admits(&payload[96..112]) => {
                    let Some(running) = live.as_ref() else { unreachable!("matched above") };
                    let next: [u8; 32] = payload[..32].try_into().map_err(|_| std::io::Error::other("generation key"))?;
                    let nonce: [u8; 16] = payload[96..112].try_into().map_err(|_| std::io::Error::other("transition nonce"))?;
                    let mut response = Vec::with_capacity(48);
                    response.extend_from_slice(&next);
                    response.extend_from_slice(&nonce);
                    if payload[32..64] != COMPILED_SURFACE {
                        report_accounts(&mut stream, tag, &payload, [None, None]);
                        send_frame(&mut stream, 10, &response)?;
                        continue;
                    }
                    let mut ordinal = 0_u8;
                    let mut progress = |stage| {
                        ordinal = ordinal.checked_add(1).ok_or_else(|| std::io::Error::other("progress ordinal"))?;
                        send_progress(&mut stream, &next, &nonce, 3, ordinal, stage)
                    };
                    let mut observation = None;
                    let compiled = compile_program(&payload[112..], Compilation::Live(&running.engine), &mut progress, &mut observation);
                    drop(progress);
                    let candidate = account_result(compiled, &mut stream, tag, &payload, [None, observed_account(&observation)])?;
                    let refused = candidate
                        .as_ref()
                        .and_then(|program| program.candidate.as_ref())
                        .and_then(|next| __rawr_internal::island::refusal(&running.generation, next));
                    report_accounts(&mut stream, tag, &payload, [None, program_account(&candidate)]);
                    if let Some(reason) = refused {
                        send_frame(&mut stream, 16, reason.as_bytes())?;
                        continue;
                    }
                    pending = Some(PendingProgram { key: next, nonce, program: candidate });
                    hot.begun = None;
                    send_frame(&mut stream, 4, &response)?;
                }
                25 | 26 if live.is_some() && payload.len() == 48 => {
                    let next: [u8; 32] = payload[..32].try_into().map_err(|_| std::io::Error::other("generation key"))?;
                    let nonce: [u8; 16] = payload[32..48].try_into().map_err(|_| std::io::Error::other("transition nonce"))?;
                    let Some(candidate) = pending.take() else {
                        return Err(std::io::Error::other("missing hot preparation"));
                    };
                    if candidate.key != next || candidate.nonce != nonce {
                        return Err(std::io::Error::other("stale hot transition"));
                    }
                    if tag == 26 {
                        drop(candidate);
                        send_frame(&mut stream, 27, &payload)?;
                        continue;
                    }
                    if let Some(mut program) = candidate.program {
                        let evaluated = evaluate_program(&mut program);
                        account_result(evaluated, &mut stream, tag, &payload, [None, None])?;
                        if let (Some(running), Some(next)) = (live.as_mut(), program.candidate.as_ref()) {
                            running.engine = program.engine.clone();
                            running.generation = next.generation().clone();
                            running.facades = program._facades.clone();
                        }
                        // The owner thread admits it at its next call from
                        // native code; a newer save's replaces it first.
                        __rawr_internal::future::offer(move || {
                            if let Err(error) = __rawr_profile::admit(program) {
                                eprintln!("rawr: the program keeps running the previous generation: {error}");
                            }
                        });
                    }
                    key = next;
                    send_frame(&mut stream, 8, &payload)?;
                }
                17 => send_frame(&mut stream, 19, &invoke_rejection(&key, &Error::Stale))?,
                12 if payload.as_slice() == key => {
                    send_frame(&mut stream, 13, &key)?;
                    exit(0);
                }
                _ => return Err(std::io::Error::other(format!("invalid protocol transition while the program runs (message {tag})"))),
            }
        }
    };
    if let Err(error) = serve() {
        eprintln!("runner protocol failed: {error}");
        exit(1);
    }
}

struct PendingProgram {
    key: [u8; 32],
    nonce: [u8; 16],
    program: Option<Program>,
}

/// The protocol's opening, before any thread owns the program: connect,
/// authenticate, and read the parent's first frame.
fn open_protocol() -> std::io::Result<(std::os::unix::net::UnixStream, [u8; 32], (u8, Vec<u8>))> {
    let socket = std::env::var_os("RAWR_PROTOCOL_SOCKET").ok_or_else(|| std::io::Error::other("protocol socket"))?;
    let capability = decode_hex(&std::env::var("RAWR_PROTOCOL_CAPABILITY").map_err(|_| std::io::Error::other("protocol capability"))?)?;
    let key = decode_hex(&std::env::var("RAWR_GENERATION_KEY").map_err(|_| std::io::Error::other("generation key"))?)?;
    let mut stream = std::os::unix::net::UnixStream::connect(socket)?;
    let mut authentication = Vec::with_capacity(32 + GENERATED_MANIFEST.len());
    authentication.extend_from_slice(&capability);
    authentication.extend_from_slice(GENERATED_MANIFEST);
    send_frame(&mut stream, 1, &authentication)?;
    let first = receive_frame(&mut stream)?;
    Ok((stream, key, first))
}

/// Whether the first frame prepares a program whose crate root is marked,
/// so its `main` is Rawr code and nothing native needs the main thread
/// kept for it.
fn marked_main(key: &[u8; 32], (tag, payload): &(u8, Vec<u8>)) -> bool {
    *tag == 3
        && payload.len() >= 80
        && payload[..32] == *key
        && decode_snapshot(&payload[80..]).is_ok_and(|snapshot| snapshot.marked_root())
}

fn run_protocol(
    mut stream: std::os::unix::net::UnixStream,
    mut key: [u8; 32],
    first: (u8, Vec<u8>),
) -> std::io::Result<()> {
    install_arena(key);
    let mut program: Option<Program> = None;
    let mut pending: Option<PendingProgram> = None;
    let mut prepared = false;
    let mut admitted = false;
    let mut hot = Hot::default();
    let mut first = Some(first);
    let (events, received) = std::sync::mpsc::channel::<__RawrProtocolEvent>();
    let mut reader = stream.try_clone()?;
    let frames = events.clone();
    std::thread::spawn(move || loop {
        let frame = receive_frame(&mut reader);
        let end = frame.is_err();
        if frames.send(__RawrProtocolEvent::Frame(frame)).is_err() || end {
            break;
        }
    });
    loop {
        let (tag, payload) = match first.take() {
            Some(frame) => frame,
            None => next_frame(&received)?,
        };
        if admitted {
            let resident = program.as_ref().map(|program| Resident {
                engine: &program.engine,
                facades: &program._facades,
                contract: &program.application_contract,
                running: None,
            });
            if serve_hot(&mut stream, tag, &payload, &key, &mut hot, &mut pending, resident)? {
                continue;
            }
        }
        match tag {
            3 if !prepared && payload.len() >= 48 && payload[..32] == key => {
                let nonce: [u8; 16] = payload[32..48].try_into().map_err(|_| std::io::Error::other("transition nonce"))?;
                let mut ordinal = 0_u8;
                let mut progress = |stage| {
                    ordinal = ordinal.checked_add(1).ok_or_else(|| std::io::Error::other("progress ordinal"))?;
                    send_progress(&mut stream, &key, &nonce, 1, ordinal, stage)
                };
                let mut observation = None;
                program = match compile_program(&payload[48..], Compilation::Initial, &mut progress, &mut observation) {
                    Ok(program) => program,
                    Err(error) => {
                        drop(progress);
                        report_accounts(&mut stream, tag, &payload, [program_account(&program), observed_account(&observation)]);
                        return Err(error);
                    }
                };
                drop(progress);
                prepared = true;
                let mut response = Vec::with_capacity(48);
                response.extend_from_slice(&key);
                response.extend_from_slice(&nonce);
                report_accounts(&mut stream, tag, &payload, [program_account(&program), None]);
                send_frame(&mut stream, 4, &response)?;
            }
            5 if payload.len() == 96 && payload[..32] == key => {
                let mut response = Vec::with_capacity(64);
                response.extend_from_slice(&key);
                response.extend_from_slice(&payload[64..96]);
                send_frame(&mut stream, 6, &response)?;
            }
            7 if payload.len() == 48 && payload[..32] == key => {
                let nonce: [u8; 16] = payload[32..48].try_into().map_err(|_| std::io::Error::other("transition nonce"))?;
                let result = (|| {
                    send_progress(&mut stream, &key, &nonce, 2, 1, 7)?;
                    if let Some(program) = &mut program { evaluate_program(program)?; }
                    send_progress(&mut stream, &key, &nonce, 2, 2, 8)
                })();
                report_accounts(&mut stream, tag, &payload, [program_account(&program), None]);
                result?;
                admitted = true;
                send_frame(&mut stream, 8, &payload)?;
            }
            9 if admitted && pending.is_none() && payload.len() >= 112 && hot.admits(&payload[96..112]) => {
                let next: [u8; 32] = payload[..32].try_into().map_err(|_| std::io::Error::other("generation key"))?;
                let nonce: [u8; 16] = payload[96..112].try_into().map_err(|_| std::io::Error::other("transition nonce"))?;
                if payload[32..64] != COMPILED_SURFACE {
                    let mut response = Vec::with_capacity(48);
                    response.extend_from_slice(&key);
                    response.extend_from_slice(&nonce);
                    report_accounts(&mut stream, tag, &payload, [program_account(&program), None]);
                    send_frame(&mut stream, 10, &response)?;
                    continue;
                }
                let mut ordinal = 0_u8;
                let mut progress = |stage| {
                    ordinal = ordinal.checked_add(1).ok_or_else(|| std::io::Error::other("progress ordinal"))?;
                    send_progress(&mut stream, &next, &nonce, 3, ordinal, stage)
                };
                let mut observation = None;
                let compiled = compile_program(&payload[112..], Compilation::Replacement(&program), &mut progress, &mut observation);
                drop(progress);
                let candidate = account_result(compiled, &mut stream, tag, &payload,
                    [program_account(&program), observed_account(&observation)])?;
                let mut response = Vec::with_capacity(48);
                response.extend_from_slice(&next);
                response.extend_from_slice(&nonce);
                pending = Some(PendingProgram { key: next, nonce, program: candidate });
                hot.begun = None;
                report_accounts(&mut stream, tag, &payload,
                    [program_account(&program), pending.as_ref().and_then(|pending| program_account(&pending.program))]);
                send_frame(&mut stream, 4, &response)?;
            }
            25 if payload.len() == 48 => {
                let next: [u8; 32] = payload[..32]
                    .try_into()
                    .map_err(|_| std::io::Error::other("generation key"))?;
                let nonce: [u8; 16] = payload[32..48]
                    .try_into()
                    .map_err(|_| std::io::Error::other("transition nonce"))?;
                let Some(mut candidate) = pending.take() else {
                    report_accounts(&mut stream, tag, &payload, [program_account(&program), None]);
                    return Err(std::io::Error::other("missing hot preparation"));
                };
                if candidate.key != next || candidate.nonce != nonce {
                    pending = Some(candidate);
                    report_accounts(&mut stream, tag, &payload,
                        [program_account(&program), pending.as_ref().and_then(|pending| program_account(&pending.program))]);
                    return Err(std::io::Error::other("stale hot commit"));
                }
                if let Some(candidate_program) = &mut candidate.program {
                    let evaluated = evaluate_program(candidate_program);
                    account_result(evaluated, &mut stream, tag, &payload,
                        [program_account(&program), program_account(&candidate.program)])?;
                }
                install_arena(next);
                key = next;
                let retired = retire_program(std::mem::replace(&mut program, candidate.program));
                // A running native `main` calls the new generation next.
                if __rawr_internal::island::running()
                    && let Some(program) = &mut program
                    && let Err(error) = __rawr_profile::publish(program)
                {
                    eprintln!("rawr: native code keeps calling the previous generation: {error}");
                }
                report_accounts(&mut stream, tag, &payload, [program_account(&program), observed_account(&retired)]);
                send_frame(&mut stream, 8, &payload)?;
            }
            26 if payload.len() == 48 => {
                let next: [u8; 32] = payload[..32]
                    .try_into()
                    .map_err(|_| std::io::Error::other("generation key"))?;
                let nonce: [u8; 16] = payload[32..48]
                    .try_into()
                    .map_err(|_| std::io::Error::other("transition nonce"))?;
                let Some(candidate) = pending.take() else {
                    report_accounts(&mut stream, tag, &payload, [program_account(&program), None]);
                    return Err(std::io::Error::other("missing hot preparation"));
                };
                if candidate.key != next || candidate.nonce != nonce {
                    pending = Some(candidate);
                    report_accounts(&mut stream, tag, &payload,
                        [program_account(&program), pending.as_ref().and_then(|pending| program_account(&pending.program))]);
                    return Err(std::io::Error::other("stale hot abort"));
                }
                let retired = retire_program(candidate.program);
                report_accounts(&mut stream, tag, &payload, [program_account(&program), observed_account(&retired)]);
                send_frame(&mut stream, 27, &payload)?;
            }
            35 if admitted && payload.len() > 32 && payload[..32] == key => {
                send_frame(&mut stream, 36, &key)?;
            }
            // A program that keeps running (a native `main`, D501) hears from
            // its owner: it asks for no refresh, prints no diagnostic of its
            // own (the owner printed it), and carries no State to the runner
            // a restart starts.
            43 if admitted && payload.as_slice() == key => {
                let mut response = [0_u8; 33];
                response[..32].copy_from_slice(&key);
                send_frame(&mut stream, 44, &response)?;
            }
            11 if admitted && payload.len() >= 32 && payload[..32] == key => {
                send_frame(&mut stream, 11, &key)?;
            }
            46 if admitted && pending.is_none() && payload.as_slice() == key => {
                let mut response = [0_u8; 33];
                response[..32].copy_from_slice(&key);
                send_frame(&mut stream, 47, &response)?;
            }
            48 if admitted && payload.len() >= 32 && payload[..32] == key => {
                let mut response = [0_u8; 33];
                response[..32].copy_from_slice(&key);
                send_frame(&mut stream, 49, &response)?;
            }
            20 if admitted && payload.as_slice() == key => {
                send_frame(&mut stream, 21, &key)?;
                // A marked `main` runs here, on the thread that owns the
                // program, for as long as it runs; the protocol goes on
                // beside it, and a save restarts it (D500.1). That thread
                // is the process's main thread, so a windowing crate's
                // event loop runs as under `cargo run`. A panic in it ends
                // the program with Rust's status, 101.
                if let Some(running) = program.as_mut().filter(|program| program.marked_main) {
                    let stream = stream.try_clone()?;
                    let live = running.candidate.as_ref().map(|candidate| Live {
                        engine: running.engine.clone(),
                        generation: candidate.generation().clone(),
                        facades: running._facades.clone(),
                        contract: running.application_contract.clone(),
                    });
                    std::thread::Builder::new()
                        .name("rawr protocol".into())
                        .spawn(move || serve_entry(stream, received, key, live))?;
                    let started = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        __rawr_profile::run(running, |_| {})
                    }));
                    match started {
                        Ok(Started::Exit(status)) => exit(status),
                        Ok(Started::Entry(entry)) => exit(std::panic::catch_unwind(entry).unwrap_or(101)),
                        Ok(Started::Serving) | Err(_) => exit(101),
                    }
                }
                let calls = events.clone();
                let forward = move |call| {
                    let _ = calls.send(__RawrProtocolEvent::Call(call));
                };
                let started = match &mut program {
                    Some(program) => __rawr_profile::run(program, forward),
                    None => Started::Exit(0),
                };
                match started {
                    // A native `main` runs on its own thread; this one serves it.
                    Started::Serving => {}
                    Started::Exit(status) => exit(status),
                    // An async `main`'s runtime entry runs here, on the thread
                    // that owns the program, and the protocol goes on beside
                    // it (D527).
                    Started::Entry(entry) => {
                        let stream = stream.try_clone()?;
                        std::thread::Builder::new()
                            .name("rawr protocol".into())
                            .spawn(move || serve_entry(stream, received, key, None))?;
                        exit(std::panic::catch_unwind(entry).unwrap_or(101));
                    }
                }
            }
            17 if admitted => match invoke_adapter(&key, &payload) {
                Ok(value) => send_frame(&mut stream, 18, &value)?,
                Err(error) => send_frame(&mut stream, 19, &invoke_rejection(&key, &error))?,
            },
            17 => send_frame(&mut stream, 19, &invoke_rejection(&key, &Error::Stale))?,
            12 if payload.as_slice() == key => {
                report_accounts(&mut stream, tag, &payload,
                    [program_account(&program), pending.as_ref().and_then(|pending| program_account(&pending.program))]);
                send_frame(&mut stream, 13, &key)?;
                return Ok(());
            }
            _ => {
                report_accounts(&mut stream, tag, &payload,
                    [program_account(&program), pending.as_ref().and_then(|pending| program_account(&pending.program))]);
                return Err(std::io::Error::other(format!("invalid protocol transition (message {tag})")));
            }
        }
    }
}

/// End the process when the protocol ends.
fn finish(result: std::io::Result<()>) -> ! {
    match result {
        Ok(()) => exit(0),
        Err(error) => {
            eprintln!("runner protocol failed: {error}");
            exit(1);
        }
    }
}

/// A program whose `main` is Rawr code runs its protocol and interpreter on
/// the process's main thread, where its `main` then runs. Otherwise they
/// run on a thread of their own, so the main thread is free for a native
/// `main`. winit's event loop must run there either way (D533).
fn main() {
    for adapter in adapters() {
        let _ = (adapter.identity, adapter.signature, adapter.invoke);
    }
    let (stream, key, first) = match open_protocol() {
        Ok(opened) => opened,
        Err(error) => finish(Err(error)),
    };
    if marked_main(&key, &first) {
        finish(run_protocol(stream, key, first));
    }
    let main_thread = __rawr_internal::island::keep_main_thread();
    let protocol = std::thread::Builder::new()
        .name("rawr protocol".into())
        // The stack a process's own main thread usually has.
        .stack_size(8 << 20)
        .spawn(move || finish(run_protocol(stream, key, first)));
    if let Err(error) = protocol {
        eprintln!("runner protocol failed: {error}");
        exit(1);
    }
    main_thread.serve()
}
"#;
const PROFILE_RUNNER_MAIN: &[u8] = br#"
fn send_frame(stream: &mut std::os::unix::net::UnixStream, tag: u8, payload: &[u8]) -> std::io::Result<()> {
    use std::io::Write as _;
    let length = 5_usize.checked_add(payload.len()).ok_or_else(|| std::io::Error::other("frame length"))?;
    if length > 8 * 1024 * 1024 { return Err(std::io::Error::other("frame limit")); }
    stream.write_all(&(length as u32).to_be_bytes())?;
    stream.write_all(&11_u32.to_be_bytes())?;
    stream.write_all(&[tag])?;
    stream.write_all(payload)?;
    stream.flush()
}

fn send_progress(
    stream: &mut std::os::unix::net::UnixStream,
    key: &[u8; 32],
    nonce: &[u8; 16],
    operation: u8,
    ordinal: u8,
    stage: u8,
) -> std::io::Result<()> {
    let mut payload = Vec::with_capacity(51);
    payload.extend_from_slice(key);
    payload.extend_from_slice(nonce);
    payload.extend_from_slice(&[operation, ordinal, stage]);
    send_frame(stream, 24, &payload)
}

fn receive_frame(stream: &mut std::os::unix::net::UnixStream) -> std::io::Result<(u8, Vec<u8>)> {
    use std::io::Read as _;
    let mut header = [0_u8; 4];
    stream.read_exact(&mut header)?;
    let length = u32::from_be_bytes(header) as usize;
    if !(5..=8 * 1024 * 1024).contains(&length) { return Err(std::io::Error::other("frame bound")); }
    let mut frame = vec![0_u8; length];
    stream.read_exact(&mut frame)?;
    if frame[..4] != 11_u32.to_be_bytes() { return Err(std::io::Error::other("wire version")); }
    Ok((frame[4], frame.split_off(5)))
}

fn decode_hex(value: &str) -> std::io::Result<[u8; 32]> {
    if value.len() != 64 { return Err(std::io::Error::other("capability length")); }
    let mut output = [0_u8; 32];
    for (index, byte) in output.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
            .map_err(|_| std::io::Error::other("capability encoding"))?;
    }
    Ok(output)
}

fn reject(stream: &mut std::os::unix::net::UnixStream, reason: &str) -> std::io::Result<()> {
    send_frame(stream, 16, reason.as_bytes())
}

struct PendingProfileProgram {
    key: [u8; 32],
    receipt: [u8; 32],
    nonce: [u8; 16],
    program: Option<Program>,
}

fn run_protocol() -> std::io::Result<()> {
    let socket = std::env::var_os("RAWR_PROTOCOL_SOCKET").ok_or_else(|| std::io::Error::other("protocol socket"))?;
    let capability = decode_hex(&std::env::var("RAWR_PROTOCOL_CAPABILITY").map_err(|_| std::io::Error::other("protocol capability"))?)?;
    let mut key = decode_hex(&std::env::var("RAWR_GENERATION_KEY").map_err(|_| std::io::Error::other("generation key"))?)?;
    install_arena(key);
    let mut program: Option<Program> = None;
    let mut profile = __rawr_profile::Runtime::new();
    let mut receipt: Option<[u8; 32]> = None;
    let mut prepared = false;
    let mut activated = false;
    let mut admitted = false;
    let mut sequence = 0_u64;
    let mut pending: Option<PendingProfileProgram> = None;
    let mut stream = std::os::unix::net::UnixStream::connect(socket)?;
    let mut authentication = Vec::with_capacity(32 + GENERATED_MANIFEST.len());
    authentication.extend_from_slice(&capability);
    authentication.extend_from_slice(GENERATED_MANIFEST);
    send_frame(&mut stream, 1, &authentication)?;
    loop {
        let (tag, payload) = receive_frame(&mut stream)?;
        match tag {
            3 if !prepared && payload.len() >= 48 && payload[..32] == key => {
                let nonce: [u8; 16] = payload[32..48].try_into().map_err(|_| std::io::Error::other("transition nonce"))?;
                let mut ordinal = 0_u8;
                let mut progress = |stage| {
                    ordinal = ordinal.checked_add(1).ok_or_else(|| std::io::Error::other("progress ordinal"))?;
                    send_progress(&mut stream, &key, &nonce, 1, ordinal, stage)
                };
                let mut observation = None;
                program = match compile_program(&payload[48..], Compilation::Initial, &mut progress, &mut observation) {
                    Ok(program) => program,
                    Err(error) => {
                        drop(progress);
                        report_accounts(&mut stream, tag, &payload, [program_account(&program), observed_account(&observation)]);
                        return Err(error);
                    }
                };
                match profile.prepare() {
                    Ok(()) => {
                        let advanced = progress(6);
                        drop(progress);
                        account_result(advanced, &mut stream, tag, &payload, [program_account(&program), None])?;
                        prepared = true;
                        let mut response = Vec::with_capacity(48);
                        response.extend_from_slice(&key);
                        response.extend_from_slice(&nonce);
                        report_accounts(&mut stream, tag, &payload, [program_account(&program), None]);
                        send_frame(&mut stream, 4, &response)?;
                    }
                    Err(reason) => {
                        drop(progress);
                        report_accounts(&mut stream, tag, &payload, [program_account(&program), None]);
                        reject(&mut stream, reason)?;
                    }
                }
            }
            5 if prepared && !activated && payload.len() == 96 && payload[..32] == key => {
                match profile.activate() {
                    Ok(()) => {
                        let next: [u8; 32] = payload[64..96].try_into().map_err(|_| std::io::Error::other("activation receipt"))?;
                        receipt = Some(next);
                        activated = true;
                        let mut response = Vec::with_capacity(64);
                        response.extend_from_slice(&key);
                        response.extend_from_slice(&next);
                        send_frame(&mut stream, 6, &response)?;
                    }
                    Err(reason) => reject(&mut stream, reason)?,
                }
            }
            7 if activated && !admitted && payload.len() == 48 && payload[..32] == key => {
                let nonce: [u8; 16] = payload[32..48].try_into().map_err(|_| std::io::Error::other("transition nonce"))?;
                match profile.admit(program.as_mut()) {
                    Ok(()) => {
                        let result = (|| {
                            send_progress(&mut stream, &key, &nonce, 2, 1, 7)?;
                            if let Some(program) = &mut program { evaluate_program(program)?; }
                            send_progress(&mut stream, &key, &nonce, 2, 2, 8)
                        })();
                        report_accounts(&mut stream, tag, &payload, [program_account(&program), None]);
                        result?;
                        admitted = true;
                        send_frame(&mut stream, 8, &payload)?;
                    }
                    Err(reason) => {
                        report_accounts(&mut stream, tag, &payload, [program_account(&program), None]);
                        reject(&mut stream, reason)?;
                    }
                }
            }
            9 if admitted && pending.is_none() && payload.len() >= 112 => {
                let next: [u8; 32] = payload[..32].try_into().map_err(|_| std::io::Error::other("generation key"))?;
                let nonce: [u8; 16] = payload[96..112].try_into().map_err(|_| std::io::Error::other("transition nonce"))?;
                if payload[32..64] != COMPILED_SURFACE {
                    let mut response = Vec::with_capacity(48);
                    response.extend_from_slice(&key);
                    response.extend_from_slice(&nonce);
                    report_accounts(&mut stream, tag, &payload, [program_account(&program), None]);
                    send_frame(&mut stream, 10, &response)?;
                    continue;
                }
                let mut ordinal = 0_u8;
                let mut progress = |stage| {
                    ordinal = ordinal.checked_add(1).ok_or_else(|| std::io::Error::other("progress ordinal"))?;
                    send_progress(&mut stream, &next, &nonce, 3, ordinal, stage)
                };
                let next_receipt: [u8; 32] = payload[64..96].try_into().map_err(|_| std::io::Error::other("activation receipt"))?;
                let mut observation = None;
                let compiled = compile_program(&payload[112..], Compilation::Replacement(&program), &mut progress, &mut observation);
                drop(progress);
                let candidate = account_result(compiled, &mut stream, tag, &payload,
                    [program_account(&program), observed_account(&observation)])?;
                let mut response = Vec::with_capacity(48);
                response.extend_from_slice(&next);
                response.extend_from_slice(&nonce);
                pending = Some(PendingProfileProgram {
                    key: next,
                    receipt: next_receipt,
                    nonce,
                    program: candidate,
                });
                report_accounts(&mut stream, tag, &payload,
                    [program_account(&program), pending.as_ref().and_then(|pending| program_account(&pending.program))]);
                send_frame(&mut stream, 4, &response)?;
            }
            25 if payload.len() == 48 => {
                let next: [u8; 32] = payload[..32]
                    .try_into()
                    .map_err(|_| std::io::Error::other("generation key"))?;
                let nonce: [u8; 16] = payload[32..48]
                    .try_into()
                    .map_err(|_| std::io::Error::other("transition nonce"))?;
                let Some(mut candidate) = pending.take() else {
                    report_accounts(&mut stream, tag, &payload, [program_account(&program), None]);
                    return Err(std::io::Error::other("missing hot preparation"));
                };
                if candidate.key != next || candidate.nonce != nonce {
                    pending = Some(candidate);
                    report_accounts(&mut stream, tag, &payload,
                        [program_account(&program), pending.as_ref().and_then(|pending| program_account(&pending.program))]);
                    return Err(std::io::Error::other("stale hot commit"));
                }
                if let Some(candidate_program) = &mut candidate.program {
                    let evaluated = evaluate_program(candidate_program);
                    account_result(evaluated, &mut stream, tag, &payload,
                        [program_account(&program), program_account(&candidate.program)])?;
                }
                install_arena(next);
                key = next;
                let retired = retire_program(std::mem::replace(&mut program, candidate.program));
                receipt = Some(candidate.receipt);
                sequence = 0;
                report_accounts(&mut stream, tag, &payload, [program_account(&program), observed_account(&retired)]);
                send_frame(&mut stream, 8, &payload)?;
            }
            26 if payload.len() == 48 => {
                let next: [u8; 32] = payload[..32]
                    .try_into()
                    .map_err(|_| std::io::Error::other("generation key"))?;
                let nonce: [u8; 16] = payload[32..48]
                    .try_into()
                    .map_err(|_| std::io::Error::other("transition nonce"))?;
                let Some(candidate) = pending.take() else {
                    report_accounts(&mut stream, tag, &payload, [program_account(&program), None]);
                    return Err(std::io::Error::other("missing hot preparation"));
                };
                if candidate.key != next || candidate.nonce != nonce {
                    pending = Some(candidate);
                    report_accounts(&mut stream, tag, &payload,
                        [program_account(&program), pending.as_ref().and_then(|pending| program_account(&pending.program))]);
                    return Err(std::io::Error::other("stale hot abort"));
                }
                let retired = retire_program(candidate.program);
                report_accounts(&mut stream, tag, &payload, [program_account(&program), observed_account(&retired)]);
                send_frame(&mut stream, 27, &payload)?;
            }
            14 if admitted && payload.len() >= 72 && payload[..32] == key && receipt.is_some_and(|value| payload[32..64] == value) => {
                let next = u64::from_le_bytes(payload[64..72].try_into().map_err(|_| std::io::Error::other("message sequence"))?);
                if next != sequence.checked_add(1).ok_or_else(|| std::io::Error::other("message sequence"))? {
                    reject(&mut stream, "stale profile message")?;
                    continue;
                }
                match profile.message(&payload[72..]) {
                    Ok(message) => {
                        sequence = next;
                        let mut response = Vec::with_capacity(72 + message.len());
                        response.extend_from_slice(&key);
                        response.extend_from_slice(&receipt.ok_or_else(|| std::io::Error::other("activation receipt"))?);
                        response.extend_from_slice(&sequence.to_le_bytes());
                        response.extend_from_slice(&message);
                        send_frame(&mut stream, 15, &response)?;
                    }
                    Err(reason) => reject(&mut stream, reason)?,
                }
            }
            14 => reject(&mut stream, "profile route is not admitted")?,
            17 if admitted => match invoke_adapter(&key, &payload) {
                Ok(value) => send_frame(&mut stream, 18, &value)?,
                Err(error) => send_frame(&mut stream, 19, &invoke_rejection(&key, &error))?,
            },
            17 => send_frame(&mut stream, 19, &invoke_rejection(&key, &Error::Stale))?,
            12 if payload.as_slice() == key => {
                if activated { profile.retire(); }
                report_accounts(&mut stream, tag, &payload,
                    [program_account(&program), pending.as_ref().and_then(|pending| program_account(&pending.program))]);
                send_frame(&mut stream, 13, &key)?;
                return Ok(());
            }
            _ => {
                report_accounts(&mut stream, tag, &payload,
                    [program_account(&program), pending.as_ref().and_then(|pending| program_account(&pending.program))]);
                return Err(std::io::Error::other("invalid protocol transition"));
            }
        }
    }
}

fn main() {
    for adapter in adapters() {
        let _ = (adapter.identity, adapter.signature, adapter.invoke);
    }
    if let Err(error) = run_protocol() {
        eprintln!("runner protocol failed: {error}");
        std::process::exit(1);
    }
}
"#;
const LIFECYCLE_PROFILE_RUNNER_MAIN: &[u8] = br#"
fn send_frame(stream: &mut std::os::unix::net::UnixStream, tag: u8, payload: &[u8]) -> std::io::Result<()> {
    use std::io::Write as _;
    let length = 5_usize.checked_add(payload.len()).ok_or_else(|| std::io::Error::other("frame length"))?;
    if length > 8 * 1024 * 1024 { return Err(std::io::Error::other("frame limit")); }
    stream.write_all(&(length as u32).to_be_bytes())?;
    stream.write_all(&11_u32.to_be_bytes())?;
    stream.write_all(&[tag])?;
    stream.write_all(payload)?;
    stream.flush()
}

fn send_progress(
    stream: &mut std::os::unix::net::UnixStream,
    key: &[u8; 32],
    nonce: &[u8; 16],
    operation: u8,
    ordinal: u8,
    stage: u8,
) -> std::io::Result<()> {
    let mut payload = Vec::with_capacity(51);
    payload.extend_from_slice(key);
    payload.extend_from_slice(nonce);
    payload.extend_from_slice(&[operation, ordinal, stage]);
    send_frame(stream, 24, &payload)
}

fn receive_frame(stream: &mut std::os::unix::net::UnixStream) -> std::io::Result<(u8, Vec<u8>)> {
    use std::io::Read as _;
    let mut header = [0_u8; 4];
    stream.read_exact(&mut header)?;
    let length = u32::from_be_bytes(header) as usize;
    if !(5..=8 * 1024 * 1024).contains(&length) { return Err(std::io::Error::other("frame bound")); }
    let mut frame = vec![0_u8; length];
    stream.read_exact(&mut frame)?;
    if frame[..4] != 11_u32.to_be_bytes() { return Err(std::io::Error::other("wire version")); }
    Ok((frame[4], frame.split_off(5)))
}

fn decode_hex(value: &str) -> std::io::Result<[u8; 32]> {
    if value.len() != 64 { return Err(std::io::Error::other("capability length")); }
    let mut output = [0_u8; 32];
    for (index, byte) in output.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
            .map_err(|_| std::io::Error::other("capability encoding"))?;
    }
    Ok(output)
}

fn reject(stream: &mut std::os::unix::net::UnixStream, reason: &str) -> std::io::Result<()> {
    send_frame(stream, 16, reason.as_bytes())
}

fn consumed_hot_rejection(
    stream: &mut std::os::unix::net::UnixStream,
    identity: &[u8],
    reason: &str,
) -> std::io::Result<()> {
    if identity.len() != 48 { return Err(std::io::Error::other("hot rejection identity")); }
    // Include version and tag in the existing frame bound before allocation
    // or copying any diagnostic bytes. No separate enlarged error frame.
    let length = identity.len().checked_add(reason.len())
        .filter(|length| *length <= 8 * 1024 * 1024 - 5)
        .ok_or_else(|| std::io::Error::other("hot rejection frame limit"))?;
    let mut payload = Vec::new();
    payload.try_reserve_exact(length)
        .map_err(|_| std::io::Error::other("hot rejection allocation"))?;
    payload.extend_from_slice(identity);
    payload.extend_from_slice(reason.as_bytes());
    send_frame(stream, 28, &payload)
}

fn lifecycle() -> std::io::Result<__rawr_profile::Mode> {
    match std::env::var("RAWR_RUNNER_LIFECYCLE").as_deref() {
        Ok("check") => Ok(__rawr_profile::Mode::Check),
        Ok("run") => Ok(__rawr_profile::Mode::Run),
        Ok("preview") => Ok(__rawr_profile::Mode::Preview),
        Ok("snapshot-check") => Ok(__rawr_profile::Mode::SnapshotCheck),
        Ok("snapshot-update") => Ok(__rawr_profile::Mode::SnapshotUpdate),
        _ => Err(std::io::Error::other("runner lifecycle")),
    }
}

struct PendingLive {
    installation: __rawr_profile::PendingInstall,
    key: [u8; 32],
    receipt: [u8; 32],
    nonce: [u8; 16],
    candidate: __rawr_internal::reload::Candidate,
}

enum ResidentResult {
    Compiled(__rawr_internal::package::import::resident::Compiled),
    Miss,
    Diagnostic(String),
    Obsolete,
    Resource(String),
}

fn resident_status(
    transport: &__rawr_profile::Transport,
    installation: &__rawr_profile::PendingInstall,
) -> std::io::Result<Option<ResidentResult>> {
    // Native invalidation occurs under the gate lock. Read current status
    // after eligibility so a concurrent callback cannot become a stale miss.
    let eligible = transport.resident_eligible(installation);
    let current = transport.request_is_current(installation)
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    if !current {
        return Ok(Some(ResidentResult::Obsolete));
    }
    if !eligible {
        return Ok(Some(ResidentResult::Miss));
    }
    Ok(None)
}

fn prepare_resident(
    engine: &__rawr_internal::Engine,
    contract: &__rawr_profile::ApplicationContract,
    transport: &__rawr_profile::Transport,
    installation: &__rawr_profile::PendingInstall,
    snapshot: &__rawr_internal::package::snapshot::Snapshot,
    progress: &mut impl FnMut(u8) -> std::io::Result<()>,
) -> std::io::Result<ResidentResult> {
    use __rawr_internal::package::import::resident::{Error, Outcome};
    if let Some(status) = resident_status(transport, installation)? {
        return Ok(status);
    }
    let output = match __rawr_profile::compile_resident(engine, snapshot, contract) {
        Ok(Outcome::Compiled(compiled)) => ResidentResult::Compiled(compiled),
        Ok(Outcome::NeedsPreparation) => ResidentResult::Miss,
        Err(Error::Source(error)) => ResidentResult::Diagnostic(error.to_string()),
        Err(Error::Resource(error)) => ResidentResult::Resource(error.to_string()),
        Err(error) => return Err(std::io::Error::other(error.to_string())),
    };
    progress(4)?;
    let eligible = transport.resident_eligible(installation);
    let current = transport.request_is_current(installation)
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    if !current {
        return Ok(ResidentResult::Obsolete);
    }
    // Native invalidation cannot turn a source or resource error into a miss.
    if matches!(&output, ResidentResult::Compiled(_) | ResidentResult::Miss)
        && !eligible
    {
        return Ok(ResidentResult::Miss);
    }
    Ok(output)
}

fn resident_response(
    stream: &mut std::os::unix::net::UnixStream,
    header: &[u8],
    tag: u8,
    diagnostic: &str,
) -> std::io::Result<()> {
    let length = header.len().checked_add(diagnostic.len())
        .filter(|length| *length <= 8 * 1024 * 1024 - 5)
        .ok_or_else(|| std::io::Error::other("resident response frame limit"))?;
    let mut payload = Vec::new();
    payload.try_reserve_exact(length)
        .map_err(|_| std::io::Error::other("resident response allocation"))?;
    payload.extend_from_slice(header);
    payload.extend_from_slice(diagnostic.as_bytes());
    send_frame(stream, tag, &payload)
}

fn live_protocol(
    mut stream: std::os::unix::net::UnixStream,
    mut key: [u8; 32],
    mut receipt: [u8; 32],
    engine: __rawr_internal::Shared<__rawr_internal::Engine>,
    application_contract: __rawr_profile::ApplicationContract,
    transport: __rawr_profile::Transport,
) -> std::io::Result<()> {
    let mut sequence = 0_u64;
    let mut pending: Option<PendingLive> = None;
    let mut begun: Option<([u8; 56], __rawr_profile::PendingInstall, bool)> = None;
    let mut hot_sequence = 0_u64;
    loop {
        let (tag, payload) = match receive_frame(&mut stream) {
            Ok(frame) => frame,
            Err(error) => {
                let _ = transport.exit();
                return match error.kind() {
                    std::io::ErrorKind::UnexpectedEof
                    | std::io::ErrorKind::ConnectionReset
                    | std::io::ErrorKind::BrokenPipe => Ok(()),
                    _ => Err(error),
                };
            }
        };
        match tag {
            43 if begun.is_none() && pending.is_none() && payload.as_slice() == key => {
                let mut response = [0_u8; 33];
                response[..32].copy_from_slice(&key);
                response[32] = u8::from(transport.take_refresh());
                send_frame(&mut stream, 44, &response)?;
            }
            43 => return Err(std::io::Error::other("invalid refresh poll transition")),
            46 if begun.is_none() && pending.is_none() && payload.as_slice() == key => {
                // The State, for the runner a `.rs` edit starts.
                let state = transport.export_state()
                    .filter(|state| !state.is_empty() && state.len() <= 8 * 1024 * 1024 - 64);
                let mut response = Vec::with_capacity(33 + state.as_ref().map_or(0, Vec::len));
                response.extend_from_slice(&key);
                match state {
                    Some(state) => {
                        response.push(1);
                        response.extend_from_slice(&state);
                    }
                    None => response.push(0),
                }
                send_frame(&mut stream, 47, &response)?;
            }
            46 => return Err(std::io::Error::other("invalid state export transition")),
            29 if pending.is_none() && begun.is_none() && payload.len() == 56
                && payload[..32] == key => {
                let next_sequence = u64::from_le_bytes(payload[48..56].try_into()
                    .map_err(|_| std::io::Error::other("hot sequence"))?);
                if Some(next_sequence) != hot_sequence.checked_add(1) {
                    return Err(std::io::Error::other("replayed hot begin"));
                }
                let installation = transport.begin_install()
                    .map_err(|error| std::io::Error::other(error.to_string()))?;
                let identity = payload.as_slice().try_into()
                    .map_err(|_| std::io::Error::other("hot begin identity"))?;
                hot_sequence = next_sequence;
                begun = Some((identity, installation, false));
                send_frame(&mut stream, 30, &payload)?;
            }
            31 if pending.is_none() && payload.len() == 56
                && begun.as_ref().is_some_and(|(identity, _, _)| payload.as_slice() == identity) => {
                let (_, installation, _) = begun.take()
                    .ok_or_else(|| std::io::Error::other("missing hot cancellation"))?;
                let result = transport.cancel_install(installation)
                    .map_err(|error| std::io::Error::other(error.to_string()));
                report_accounts(&mut stream, tag, &payload, [engine.imported_native_snapshot(), None]);
                result?;
                send_frame(&mut stream, 32, &payload)?;
            }
            33 if pending.is_none() && payload.len() > 56
                && begun.as_ref().is_some_and(|(identity, _, observed)| !observed && payload[..56] == *identity) => {
                let (_, installation, observed) = begun.as_mut()
                    .ok_or_else(|| std::io::Error::other("missing hot observation request"))?;
                match transport.observe_install(installation, &payload[56..]) {
                    Ok(()) => {
                        *observed = true;
                        send_frame(&mut stream, 34, &payload[..56])?;
                    }
                    Err(error) => reject(&mut stream, &error.to_string())?,
                }
            }
            29 | 31 | 33 => {
                report_accounts(&mut stream, tag, &payload, [engine.imported_native_snapshot(), None]);
                return Err(std::io::Error::other("invalid hot preparation transition"));
            }
            37 if pending.is_none()
                && begun.as_ref().is_some_and(|(_, _, observed)| *observed) => {
                let (identity, installation, _) = begun.take()
                    .ok_or_else(|| std::io::Error::other("missing original resident request"))?;
                let decoded = __rawr_internal::package::import::resident::wire::Attempt::decode(
                    &payload, &identity,
                ).map_err(|error| std::io::Error::other(error.to_string()));
                let (attempt, input) = account_result(decoded, &mut stream, tag, &payload,
                    [engine.imported_native_snapshot(), None])?;
                let snapshot = account_result(decode_snapshot(input), &mut stream, tag, &payload,
                    [engine.imported_native_snapshot(), None])?;
                let nonce: [u8; 16] = identity[32..48].try_into()
                    .map_err(|_| std::io::Error::other("resident transition nonce"))?;
                let mut ordinal = 0_u8;
                let mut progress = |stage| {
                    ordinal = ordinal.checked_add(1).ok_or_else(|| std::io::Error::other("progress ordinal"))?;
                    send_progress(&mut stream, &key, &nonce, 3, ordinal, stage)
                };
                let output = (|| {
                    progress(1)?;
                    prepare_resident(
                        &engine, &application_contract, &transport, &installation, &snapshot, &mut progress,
                    )
                })();
                drop(progress);
                let output = account_result(output, &mut stream, tag, &payload,
                    [engine.imported_native_snapshot(), None])?;
                let (response_tag, diagnostic) = match output {
                    ResidentResult::Compiled(compiled) => {
                        let (candidate, semantic, carrier, coverage) = compiled.into_parts();
                        let compiled = attempt.compiled(
                            carrier, coverage, semantic.map(|semantic| *semantic.as_bytes()),
                        );
                        pending = Some(PendingLive {
                            installation,
                            key: compiled.key(),
                            receipt: compiled.advance_receipt(receipt),
                            nonce,
                            candidate,
                        });
                        report_accounts(&mut stream, tag, &payload, [engine.imported_native_snapshot(), None]);
                        send_frame(&mut stream, 38, compiled.encode())?;
                        continue;
                    }
                    ResidentResult::Miss => (39, String::new()),
                    ResidentResult::Diagnostic(reason) => (40, reason),
                    ResidentResult::Obsolete => (41, String::new()),
                    ResidentResult::Resource(reason) => (42, reason),
                };
                // Every nonprepared response consumes the exact begun request.
                // Cancellation failure is owner loss, never guessed non-installation.
                let cancelled = transport.cancel_install(installation)
                    .map_err(|error| std::io::Error::other(error.to_string()));
                report_accounts(&mut stream, tag, &payload, [engine.imported_native_snapshot(), None]);
                cancelled?;
                resident_response(&mut stream, attempt.response_header(), response_tag, &diagnostic)?;
            }
            37 => {
                report_accounts(&mut stream, tag, &payload, [engine.imported_native_snapshot(), None]);
                return Err(std::io::Error::other("invalid resident preparation transition"));
            }
            9 if pending.is_none() && payload.len() >= 112
                && begun.as_ref().is_some_and(|(identity, _, _)| payload[96..112] == identity[32..48]) => {
                let next: [u8; 32] = payload[..32]
                    .try_into()
                    .map_err(|_| std::io::Error::other("generation key"))?;
                let nonce: [u8; 16] = payload[96..112]
                    .try_into()
                    .map_err(|_| std::io::Error::other("transition nonce"))?;
                if payload[32..64] != COMPILED_SURFACE {
                    let mut response = Vec::with_capacity(48);
                    response.extend_from_slice(&key);
                    response.extend_from_slice(&nonce);
                    report_accounts(&mut stream, tag, &payload, [engine.imported_native_snapshot(), None]);
                    send_frame(&mut stream, 10, &response)?;
                    continue;
                }
                let next_receipt: [u8; 32] = payload[64..96]
                    .try_into()
                    .map_err(|_| std::io::Error::other("activation receipt"))?;
                let mut ordinal = 0_u8;
                let mut progress = |stage| {
                    ordinal = ordinal.checked_add(1).ok_or_else(|| std::io::Error::other("progress ordinal"))?;
                    send_progress(&mut stream, &next, &nonce, 3, ordinal, stage)
                };
                let candidate = match compile_live(&engine, &application_contract, &payload[112..], &mut progress) {
                    Ok(Some(candidate)) => candidate,
                    Ok(None) => {
                        drop(progress);
                        report_accounts(&mut stream, tag, &payload, [engine.imported_native_snapshot(), None]);
                        reject(&mut stream, "profile produced no live generation")?;
                        continue;
                    }
                    Err(error) => {
                        drop(progress);
                        report_accounts(&mut stream, tag, &payload, [engine.imported_native_snapshot(), None]);
                        reject(&mut stream, &error.to_string())?;
                        continue;
                    }
                };
                drop(progress);
                let mut response = Vec::with_capacity(48);
                response.extend_from_slice(&next);
                response.extend_from_slice(&nonce);
                let (_, installation, _) = begun.take()
                    .ok_or_else(|| std::io::Error::other("missing retained hot request"))?;
                pending = Some(PendingLive {
                    installation,
                    key: next,
                    receipt: next_receipt,
                    nonce,
                    candidate,
                });
                report_accounts(&mut stream, tag, &payload, [engine.imported_native_snapshot(), None]);
                send_frame(&mut stream, 4, &response)?;
            }
            25 if payload.len() == 48 => {
                let next: [u8; 32] = payload[..32]
                    .try_into()
                    .map_err(|_| std::io::Error::other("generation key"))?;
                let nonce: [u8; 16] = payload[32..48]
                    .try_into()
                    .map_err(|_| std::io::Error::other("transition nonce"))?;
                let Some(candidate) = pending.take() else {
                    report_accounts(&mut stream, tag, &payload, [engine.imported_native_snapshot(), None]);
                    return Err(std::io::Error::other("missing hot preparation"));
                };
                if candidate.key != next || candidate.nonce != nonce {
                    pending = Some(candidate);
                    report_accounts(&mut stream, tag, &payload, [engine.imported_native_snapshot(), None]);
                    return Err(std::io::Error::other("stale hot commit"));
                }
                let installed = transport.install(candidate.installation, candidate.candidate);
                report_accounts(&mut stream, tag, &payload, [engine.imported_native_snapshot(), None]);
                if let Err(error) = installed {
                    if let Some(reason) = error.not_installed_diagnostic() {
                        // Pending was consumed. Only correlated no-install
                        // feedback permits preserving this exact old owner.
                        // An empty reason means superseded by a newer event.
                        let reason = match (error.is_superseded(), reason) {
                            (true, _) => "",
                            (false, "") => "reload candidate was not installed",
                            (false, reason) => reason,
                        };
                        consumed_hot_rejection(&mut stream, &payload, reason)?;
                        continue;
                    }
                    // A submitted install may already have changed the iced
                    // owner. Exit instead of claiming rejection or accepting
                    // an impossible HotAbort against consumed pending state.
                    return Err(std::io::Error::other(error.to_string()));
                }
                key = next;
                receipt = candidate.receipt;
                sequence = 0;
                send_frame(&mut stream, 8, &payload)?;
            }
            26 if payload.len() == 48 => {
                let next: [u8; 32] = payload[..32]
                    .try_into()
                    .map_err(|_| std::io::Error::other("generation key"))?;
                let nonce: [u8; 16] = payload[32..48]
                    .try_into()
                    .map_err(|_| std::io::Error::other("transition nonce"))?;
                let Some(candidate) = pending.take() else {
                    report_accounts(&mut stream, tag, &payload, [engine.imported_native_snapshot(), None]);
                    return Err(std::io::Error::other("missing hot preparation"));
                };
                if candidate.key != next || candidate.nonce != nonce {
                    pending = Some(candidate);
                    report_accounts(&mut stream, tag, &payload, [engine.imported_native_snapshot(), None]);
                    return Err(std::io::Error::other("stale hot abort"));
                }
                drop(candidate.candidate);
                let cancelled = transport.cancel_install(candidate.installation)
                    .map_err(|error| std::io::Error::other(error.to_string()));
                report_accounts(&mut stream, tag, &payload, [engine.imported_native_snapshot(), None]);
                cancelled?;
                send_frame(&mut stream, 27, &payload)?;
            }
            11 if payload.len() >= 32 && payload[..32] == key => {
                let report = match std::str::from_utf8(&payload[32..]) {
                    Ok(report) => report,
                    Err(_) => {
                        reject(&mut stream, "diagnostic is not UTF-8")?;
                        continue;
                    }
                };
                match transport.report(report) {
                    Ok(()) => send_frame(&mut stream, 11, &key)?,
                    Err(error) => reject(&mut stream, &error.to_string())?,
                }
            }
            14 if payload.len() >= 72
                && payload[..32] == key
                && payload[32..64] == receipt =>
            {
                let next = u64::from_le_bytes(
                    payload[64..72]
                        .try_into()
                        .map_err(|_| std::io::Error::other("message sequence"))?,
                );
                if next != sequence
                    .checked_add(1)
                    .ok_or_else(|| std::io::Error::other("message sequence"))?
                {
                    reject(&mut stream, "stale profile message")?;
                    continue;
                }
                sequence = next;
                let mut response = Vec::with_capacity(payload.len());
                response.extend_from_slice(&key);
                response.extend_from_slice(&receipt);
                response.extend_from_slice(&sequence.to_le_bytes());
                response.extend_from_slice(&payload[72..]);
                send_frame(&mut stream, 15, &response)?;
            }
            17 => send_frame(
                &mut stream,
                19,
                &invoke_rejection(&key, &Error::Stale),
            )?,
            12 if begun.is_none() && pending.is_none() && payload.len() == 33
                && payload[..32] == key && payload[32] == 1 => {
                let retired = transport.retire()
                    .map_err(|error| std::io::Error::other(error.to_string()));
                report_accounts(&mut stream, tag, &payload, [engine.imported_native_snapshot(), None]);
                let refresh = retired?;
                let mut response = [0_u8; 33];
                response[..32].copy_from_slice(&key);
                response[32] = u8::from(refresh);
                send_frame(&mut stream, 13, &response)?;
                return Ok(());
            }
            12 if payload.as_slice() == key => {
                let exited = transport.exit()
                    .map_err(|error| std::io::Error::other(error.to_string()));
                report_accounts(&mut stream, tag, &payload, [engine.imported_native_snapshot(), None]);
                exited?;
                send_frame(&mut stream, 13, &key)?;
                return Ok(());
            }
            12 => {
                report_accounts(&mut stream, tag, &payload, [engine.imported_native_snapshot(), None]);
                return Err(std::io::Error::other("invalid live retirement transition"));
            }
            _ => {
                report_accounts(&mut stream, tag, &payload, [engine.imported_native_snapshot(), None]);
                reject(&mut stream, "invalid live protocol transition")?;
            }
        }
    }
}

fn run_protocol() -> std::io::Result<()> {
    let socket = std::env::var_os("RAWR_PROTOCOL_SOCKET").ok_or_else(|| std::io::Error::other("protocol socket"))?;
    let capability = decode_hex(&std::env::var("RAWR_PROTOCOL_CAPABILITY").map_err(|_| std::io::Error::other("protocol capability"))?)?;
    let mut key = decode_hex(&std::env::var("RAWR_GENERATION_KEY").map_err(|_| std::io::Error::other("generation key"))?)?;
    install_arena(key);
    let mut program: Option<Program> = None;
    let root = std::env::var_os("RAWR_PROJECT_ROOT")
        .map(std::path::PathBuf::from)
        .ok_or_else(|| std::io::Error::other("project root"))?;
    let mode = lifecycle()?;
    let mut profile = __rawr_profile::Runtime::new(mode, root);
    let mut receipt: Option<[u8; 32]> = None;
    let mut prepared = false;
    let mut activated = false;
    let mut admitted = false;
    let mut observed = false;
    let mut sequence = 0_u64;
    let mut stream = std::os::unix::net::UnixStream::connect(socket)?;
    let mut authentication = Vec::with_capacity(32 + GENERATED_MANIFEST.len());
    authentication.extend_from_slice(&capability);
    authentication.extend_from_slice(GENERATED_MANIFEST);
    send_frame(&mut stream, 1, &authentication)?;
    loop {
        let (tag, payload) = receive_frame(&mut stream)?;
        match tag {
            3 if !prepared && payload.len() >= 48 && payload[..32] == key => {
                let nonce: [u8; 16] = payload[32..48].try_into().map_err(|_| std::io::Error::other("transition nonce"))?;
                let mut ordinal = 0_u8;
                let mut progress = |stage| {
                    ordinal = ordinal.checked_add(1).ok_or_else(|| std::io::Error::other("progress ordinal"))?;
                    send_progress(&mut stream, &key, &nonce, 1, ordinal, stage)
                };
                let mut observation = None;
                program = match compile_program(&payload[48..], Compilation::Initial, &mut progress, &mut observation) {
                    Ok(program) => program,
                    Err(error) => {
                        drop(progress);
                        report_accounts(&mut stream, tag, &payload, [program_account(&program), observed_account(&observation)]);
                        return Err(error);
                    }
                };
                match profile.prepare() {
                    Ok(()) => {
                        let advanced = progress(6);
                        drop(progress);
                        account_result(advanced, &mut stream, tag, &payload, [program_account(&program), None])?;
                        prepared = true;
                        let mut response = Vec::with_capacity(48);
                        response.extend_from_slice(&key);
                        response.extend_from_slice(&nonce);
                        report_accounts(&mut stream, tag, &payload, [program_account(&program), None]);
                        send_frame(&mut stream, 4, &response)?;
                    }
                    Err(reason) => {
                        drop(progress);
                        report_accounts(&mut stream, tag, &payload, [program_account(&program), None]);
                        reject(&mut stream, reason)?;
                    }
                }
            }
            5 if prepared && !activated && payload.len() == 96 && payload[..32] == key => {
                match profile.activate() {
                    Ok(()) => {
                        let next: [u8; 32] = payload[64..96].try_into().map_err(|_| std::io::Error::other("activation receipt"))?;
                        receipt = Some(next);
                        activated = true;
                        let mut response = Vec::with_capacity(64);
                        response.extend_from_slice(&key);
                        response.extend_from_slice(&next);
                        send_frame(&mut stream, 6, &response)?;
                    }
                    Err(reason) => reject(&mut stream, reason)?,
                }
            }
            7 if activated && !admitted && payload.len() == 48 && payload[..32] == key => {
                let nonce: [u8; 16] = payload[32..48].try_into().map_err(|_| std::io::Error::other("transition nonce"))?;
                match profile.admit(program.as_mut()) {
                    Ok(()) => {
                        let result = (|| {
                            send_progress(&mut stream, &key, &nonce, 2, 1, 7)?;
                            if let Some(program) = &mut program { evaluate_program(program)?; }
                            send_progress(&mut stream, &key, &nonce, 2, 2, 8)
                        })();
                        report_accounts(&mut stream, tag, &payload, [program_account(&program), None]);
                        result?;
                        admitted = true;
                        send_frame(&mut stream, 8, &payload)?;
                    }
                    Err(reason) => {
                        report_accounts(&mut stream, tag, &payload, [program_account(&program), None]);
                        reject(&mut stream, &reason)?;
                    }
                }
            }
            48 if admitted && payload.len() >= 32 && payload[..32] == key => {
                // Start from the State the previous runner exported. No State
                // means it could not carry over, and the window says why.
                let mut response = [0_u8; 33];
                response[..32].copy_from_slice(&key);
                response[32] = u8::from(profile.seed_state(&payload[32..]));
                send_frame(&mut stream, 49, &response)?;
            }
            35 if admitted && !observed && payload.len() > 32 && payload[..32] == key => {
                profile.observe(&payload[32..])
                    .map_err(|error| std::io::Error::other(error.to_string()))?;
                observed = true;
                send_frame(&mut stream, 36, &key)?;
            }
            20 if admitted && payload.as_slice() == key => {
                let (running, transport) = match profile.start(program.as_mut()) {
                    Ok(live) => live,
                    Err(reason) => {
                        report_accounts(&mut stream, tag, &payload, [program_account(&program), None]);
                        reject(&mut stream, reason)?;
                        continue;
                    }
                };
                let context = (|| {
                    let program = program.as_ref().ok_or_else(|| std::io::Error::other("generation program"))?;
                    let engine = program.engine.clone();
                    let application_contract = program.application_contract.clone();
                    let receipt = receipt.ok_or_else(|| std::io::Error::other("activation receipt"))?;
                    Ok((engine, application_contract, receipt))
                })();
                let (engine, application_contract, receipt) = account_result(context, &mut stream, tag, &payload,
                    [program_account(&program), None])?;
                let mut control = account_result(stream.try_clone(), &mut stream, tag, &payload,
                    [program_account(&program), None])?;
                let worker = std::thread::Builder::new()
                    .name("rawr-generated-protocol".to_owned())
                    .spawn(move || live_protocol(stream, key, receipt, engine, application_contract, transport));
                report_accounts(&mut control, tag, &payload, [program_account(&program), None]);
                let worker = worker?;
                send_frame(&mut control, 21, &key)?;
                let result = running
                    .run()
                    .map_err(|error| std::io::Error::other(error.to_string()));
                // Wake an idle reader after window closure without preempting
                // the worker's final acknowledgement of a requested exit.
                let _ = std::os::unix::net::UnixStream::shutdown(
                    &control,
                    std::net::Shutdown::Read,
                );
                let worker = worker
                    .join()
                    .map_err(|_| std::io::Error::other("protocol worker panicked"))?;
                result?;
                worker?;
                return Ok(());
            }
            22 if admitted && payload.len() == 48 && payload[..32] == key => match profile.snapshot() {
                Ok(outcome) => {
                    let nonce: [u8; 16] = payload[32..48].try_into().map_err(|_| std::io::Error::other("transition nonce"))?;
                    let advanced = send_progress(&mut stream, &key, &nonce, 4, 1, 10);
                    report_accounts(&mut stream, tag, &payload, [program_account(&program), None]);
                    advanced?;
                    let mut response = Vec::with_capacity(48 + outcome.len());
                    response.extend_from_slice(&key);
                    response.extend_from_slice(&nonce);
                    response.extend_from_slice(&outcome);
                    send_frame(&mut stream, 23, &response)?;
                }
                Err(reason) => {
                    report_accounts(&mut stream, tag, &payload, [program_account(&program), None]);
                    reject(&mut stream, &reason)?;
                }
            },
            9 if admitted => {
                report_accounts(&mut stream, tag, &payload, [program_account(&program), None]);
                reject(&mut stream, "live lifecycle has not started")?;
            }
            14 if admitted && payload.len() >= 72 && payload[..32] == key && receipt.is_some_and(|value| payload[32..64] == value) => {
                let next = u64::from_le_bytes(payload[64..72].try_into().map_err(|_| std::io::Error::other("message sequence"))?);
                if next != sequence.checked_add(1).ok_or_else(|| std::io::Error::other("message sequence"))? {
                    reject(&mut stream, "stale profile message")?;
                    continue;
                }
                match profile.message(&payload[72..]) {
                    Ok(message) => {
                        sequence = next;
                        let mut response = Vec::with_capacity(72 + message.len());
                        response.extend_from_slice(&key);
                        response.extend_from_slice(&receipt.ok_or_else(|| std::io::Error::other("activation receipt"))?);
                        response.extend_from_slice(&sequence.to_le_bytes());
                        response.extend_from_slice(&message);
                        send_frame(&mut stream, 15, &response)?;
                    }
                    Err(reason) => reject(&mut stream, reason)?,
                }
            }
            14 => reject(&mut stream, "profile route is not admitted")?,
            17 if admitted => match invoke_adapter(&key, &payload) {
                Ok(value) => send_frame(&mut stream, 18, &value)?,
                Err(error) => send_frame(&mut stream, 19, &invoke_rejection(&key, &error))?,
            },
            17 => send_frame(&mut stream, 19, &invoke_rejection(&key, &Error::Stale))?,
            12 if payload.as_slice() == key => {
                if activated { profile.retire(); }
                report_accounts(&mut stream, tag, &payload, [program_account(&program), None]);
                send_frame(&mut stream, 13, &key)?;
                return Ok(());
            }
            _ => {
                report_accounts(&mut stream, tag, &payload, [program_account(&program), None]);
                return Err(std::io::Error::other("invalid protocol transition"));
            }
        }
    }
}

fn main() {
    for adapter in adapters() {
        let _ = (adapter.identity, adapter.signature, adapter.invoke);
    }
    if let Err(error) = run_protocol() {
        eprintln!("runner protocol failed: {error}");
        std::process::exit(1);
    }
}
"#;

/// A file's digest and length.
type FileDigest = ([u8; 32], usize);

pub(crate) struct Expected {
    aggregate: [u8; 32],
    files: Vec<(Box<str>, FileDigest)>,
    source_map: FileDigest,
    entries: [u8; 32],
}

struct Product {
    manifest: Vec<u8>,
    lock: Vec<u8>,
    source: Vec<u8>,
    source_map: Vec<u8>,
    spans: Vec<(EntryIdentity, Box<str>, std::ops::Range<u32>)>,
}

pub(crate) fn materialize<B: crate::surface::Runner>(
    key: super::GenerationRequestKey<B>,
    limits: &crate::cargo::Limits,
) -> Result<crate::runner::Manifest<crate::runner::Generated, B>, request::Error> {
    let product = render(&key.request, &key.native, key.profile.value())?;
    let actual = expected(&key.request, &product)?;
    assemble(key, product, actual, None, limits)
}

pub(crate) fn semantic_digest<B: crate::surface::Runner>(
    request: &crate::surface::Request<B>,
) -> Result<[u8; 32], request::Error> {
    let mut digest = Sha256::new();
    digest.update(b"rawr.generated.semantic-input/v1");
    digest.update([u8::from(request.compile_snapshot())]);
    match request.facade_plan() {
        Some(facade) => {
            digest.update([1]);
            digest.update(
                u64::try_from(facade.len())
                    .unwrap_or(u64::MAX)
                    .to_le_bytes(),
            );
            digest.update(<[u8; 32]>::from(Sha256::digest(facade)));
        }
        None => digest.update([0]),
    }
    // The native items, not where the program names them: an edit that
    // moves a use site builds the same runner, and a running program
    // keeps it (D553).
    let mut entries = Hashing(Sha256::new());
    serde_json::to_writer(
        &mut entries,
        &crate::surface::UniqueIdentities(&request.entries),
    )
    .map_err(|_| request::Error::Encoding)?;
    digest.update(entries.0.finalize());
    Ok(digest.finalize().into())
}

fn assemble<B: crate::surface::Runner>(
    key: super::GenerationRequestKey<B>,
    product: Product,
    actual: Expected,
    chain: Option<crate::node::Chain>,
    limits: &crate::cargo::Limits,
) -> Result<crate::runner::Manifest<crate::runner::Generated, B>, request::Error> {
    // The generated size is an accounting target recorded on a bridge
    // account for this generation; checked-arithmetic overflow and the
    // reservations are the physical bounds.
    let mut account = crate::surface::account::Account::bridge();
    assemble_accounted(key, product, actual, chain, limits, &mut account)
}

/// The bridge account axis charging generated files and inventory together.
const GENERATED_COMBINED_RESOURCE: &str = "generated and inventory retained bytes";

fn assemble_accounted<B: crate::surface::Runner>(
    mut key: super::GenerationRequestKey<B>,
    product: Product,
    actual: Expected,
    chain: Option<crate::node::Chain>,
    limits: &crate::cargo::Limits,
    account: &mut crate::surface::account::Account,
) -> Result<crate::runner::Manifest<crate::runner::Generated, B>, request::Error> {
    let total = product
        .manifest
        .len()
        .checked_add(product.lock.len())
        .and_then(|bytes| bytes.checked_add(BUILD_SCRIPT.len()))
        .and_then(|bytes| bytes.checked_add(product.source.len()))
        .and_then(|bytes| bytes.checked_add(product.source_map.len()))
        .ok_or(request::Error::Limit {
            resource: "generated retained bytes",
            limit: limits.json_bytes,
            actual: usize::MAX,
        })?;
    account.exceeded("generated retained bytes", limits.json_bytes, total);
    let mut contents = vec![
        (LOCK_PATH, product.lock.as_slice()),
        (MANIFEST_PATH, product.manifest.as_slice()),
        (BUILD_PATH, BUILD_SCRIPT),
    ];
    contents.extend(decode_sources(&product.source)?);
    if contents.len() != actual.files.len() {
        return Err(request::Error::Drift {
            field: "generated file cardinality",
        });
    }
    let mut files = Vec::new();
    let mut file_records = Vec::new();
    for ((path, bytes), (expected, (digest, length))) in contents.into_iter().zip(&actual.files) {
        if path != &**expected {
            return Err(request::Error::Drift {
                field: "generated file paths",
            });
        }
        files.push(crate::runner::manifest::File::new(
            path,
            Box::from(bytes),
            *digest,
        ));
        file_records.push(crate::runner::manifest::FileRecord {
            path: path.into(),
            bytes: *length,
            digest: *digest,
        });
    }
    let files = files.into_boxed_slice();
    let file_records = file_records.into_boxed_slice();
    let mut entries = Vec::new();
    entries
        .try_reserve_exact(key.request.plans.len())
        .map_err(|_| request::Error::Allocation)?;
    for plan in &key.request.plans {
        entries.push(entry(plan)?);
    }
    // The inventory receives the whole JSON target rather than the remainder
    // after `total`: the remainder is not a gate, and a generation
    // already past its target must not turn the inventory's own comparison
    // into a refusal.
    let coverage = crate::runner::manifest::coverage::Inventory::project(
        &mut key.request,
        &entries,
        key.profile.value(),
        limits.json_bytes,
        account,
    )
    .map_err(|error| request::Error::Manifest {
        reason: error.to_string().into_boxed_str(),
    })?;
    // The files and the inventory share the one JSON target (previously the
    // inventory received only the remainder after `total`). Each side can
    // stay under it while together they pass it, so the combined retained
    // total is charged on the same bridge account and its crossing recorded.
    let combined = total
        .checked_add(coverage.retained_usage().1)
        .ok_or(request::Error::Allocation)?;
    account.exceeded(GENERATED_COMBINED_RESOURCE, limits.json_bytes, combined);
    // Each variant's matcher, after the entries the inventory pairs with
    // plans one to one.
    for plan in &key.request.plans {
        if has_matcher(plan) {
            entries.push(crate::runner::manifest::Entry::new(
                matcher_identity(plan),
                signature(plan)?,
                1,
            ));
        }
        // Each field's store, after them.
        if has_store(plan) {
            entries.push(crate::runner::manifest::Entry::new(
                store_identity(plan),
                signature(plan)?,
                2,
            ));
        }
    }
    let generated = crate::runner::manifest::GeneratedManifest::with_coverage(
        key.build,
        file_records,
        entries.into_boxed_slice(),
        coverage,
        product.source_map.into_boxed_slice(),
        actual.source_map.0,
    );
    generated
        .validate(&files)
        .map_err(|error| request::Error::Manifest {
            reason: error.to_string().into_boxed_str(),
        })?;
    Ok(crate::runner::Manifest {
        request: key.request,
        native: key.native,
        profile: key.profile.into_current()?,
        revision: key.revision,
        generated,
        files,
        chain,
        overlay: None,
        phase: crate::runner::Generated { _private: () },
    })
}

pub(crate) fn entry(plan: &Plan) -> Result<crate::runner::manifest::Entry, request::Error> {
    Ok(crate::runner::manifest::Entry::new(
        *plan.identity().as_bytes(),
        signature(plan)?,
        arguments(plan)?,
    ))
}

fn render<B: crate::surface::Runner>(
    request: &crate::surface::Request<B>,
    native: &crate::input::proof::Proof,
    profile: &crate::profile::Proof,
) -> Result<Product, request::Error> {
    let harness = native.runner_harness(request.allocation.limits().1)?;
    let mut hidden = Vec::new();
    let mut sources = Vec::new();
    let mut spans = Vec::new();
    spans
        .try_reserve_exact(request.plans.len())
        .map_err(|_| request::Error::Allocation)?;
    // A runner whose proxies stand for a script's types is built as a
    // binary named as the script's crate, so their `type_name` is theirs.
    let named = render_source(
        &request.plans,
        request.fingerprint,
        profile,
        request.compile_snapshot(),
        request.facade_plan(),
        (harness.lock(), &mut hidden),
        &mut sources,
        &mut spans,
    )?;
    let lock = runner_lock(harness.lock(), &hidden)?;
    let manifest = runner_manifest(harness.manifest(), named.as_deref(), &hidden)?;
    let source = encode_sources(&sources)?;
    let mut source_map = Vec::new();
    super::source_map::write(request, &spans, &mut source_map)?;
    Ok(Product {
        manifest,
        lock,
        source,
        source_map,
        spans,
    })
}

fn expected<B>(
    request: &crate::surface::Request<B>,
    product: &Product,
) -> Result<Expected, request::Error> {
    if product.spans.len() != request.plans.len() {
        return Err(request::Error::Drift {
            field: "adapter span cardinality",
        });
    }
    expected_parts(
        request,
        &product.lock,
        &product.manifest,
        &product.source,
        &product.source_map,
    )
}

fn expected_parts<B>(
    request: &crate::surface::Request<B>,
    lock: &[u8],
    manifest: &[u8],
    source: &[u8],
    source_map_bytes: &[u8],
) -> Result<Expected, request::Error> {
    let mut files: Vec<(Box<str>, FileDigest)> = vec![
        (LOCK_PATH.into(), digest(lock)),
        (MANIFEST_PATH.into(), digest(manifest)),
        (BUILD_PATH.into(), digest(BUILD_SCRIPT)),
    ];
    for (path, bytes) in decode_sources(source)? {
        files.push((path.into(), digest(bytes)));
    }
    let source_map = digest(source_map_bytes);
    let mut entry_hasher = Sha256::new();
    entry_hasher.update(b"rawr.import.generated-entries/v1");
    for plan in &request.plans {
        entry_hasher.update(plan.identity().as_bytes());
        entry_hasher.update(signature(plan)?);
        entry_hasher.update(arguments(plan)?.to_le_bytes());
    }
    let entries: [u8; 32] = entry_hasher.finalize().into();
    let mut aggregate = Sha256::new();
    aggregate.update(b"rawr.import.generated-product/v2");
    for (path, (digest, length)) in &files {
        aggregate.update(u64::try_from(path.len()).unwrap_or(u64::MAX).to_le_bytes());
        aggregate.update(path.as_bytes());
        aggregate.update(digest);
        aggregate.update(u64::try_from(*length).unwrap_or(u64::MAX).to_le_bytes());
    }
    aggregate.update(source_map.0);
    aggregate.update(
        u64::try_from(source_map.1)
            .unwrap_or(u64::MAX)
            .to_le_bytes(),
    );
    aggregate.update(entries);
    Ok(Expected {
        aggregate: aggregate.finalize().into(),
        files,
        source_map,
        entries,
    })
}

/// A crate a macro's `$crate` names (`__rawr_crate_<name>_<id>`), which the
/// runner depends on directly under that segment, as Rust's `$crate`
/// reaches it whether or not the root re-exports it: the segment, the
/// package's name and version, and its source as the lock writes it.
struct HiddenCrate {
    segment: String,
    package: String,
    version: String,
    source: String,
}

/// The crate `segment` names, when the lock holds exactly one package of
/// its name from a source a manifest can name.
fn hidden_crate(lock: &[u8], segment: &str) -> Option<HiddenCrate> {
    let name = segment
        .strip_prefix(crate::api::ledger::CRATE_SEGMENT)?
        .rsplit_once('_')?
        .0;
    let document = std::str::from_utf8(lock)
        .ok()?
        .parse::<DocumentMut>()
        .ok()?;
    let packages = document.get("package")?.as_array_of_tables()?;
    let mut found = packages.iter().filter(|package| {
        package
            .get("name")
            .and_then(Item::as_str)
            .is_some_and(|package| package.replace('-', "_") == name)
    });
    let package = found.next()?;
    if found.next().is_some() {
        return None;
    }
    let source = package.get("source").and_then(Item::as_str)?;
    (source.starts_with("git+")
        || source == "registry+https://github.com/rust-lang/crates.io-index")
        .then(|| HiddenCrate {
            segment: segment.to_owned(),
            package: package
                .get("name")
                .and_then(Item::as_str)
                .unwrap_or(name)
                .to_owned(),
            version: package
                .get("version")
                .and_then(Item::as_str)
                .unwrap_or_default()
                .to_owned(),
            source: source.to_owned(),
        })
}

/// `hidden`'s dependency table: the package at its locked source, with no
/// features of its own, so its unit is the one the root's crates build.
fn hidden_dependency(hidden: &HiddenCrate) -> toml_edit::InlineTable {
    let mut table = toml_edit::InlineTable::new();
    table.insert("package", hidden.package.as_str().into());
    if let Some(git) = hidden.source.strip_prefix("git+") {
        let (url, _) = git.split_once('#').unwrap_or((git, ""));
        let (url, query) = url.split_once('?').unwrap_or((url, ""));
        table.insert("git", url.into());
        for pair in query.split('&') {
            if let Some((key, value)) = pair.split_once('=')
                && matches!(key, "rev" | "branch" | "tag")
            {
                table.insert(key, value.into());
            }
        }
    } else {
        table.insert("version", format!("={}", hidden.version).into());
    }
    table.insert("default-features", false.into());
    table
}

fn runner_manifest(
    bytes: &[u8],
    binary: Option<&str>,
    hidden: &[HiddenCrate],
) -> Result<Vec<u8>, request::Error> {
    let text = std::str::from_utf8(bytes).map_err(|error| request::Error::Manifest {
        reason: error.to_string().into_boxed_str(),
    })?;
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut document =
        normalized
            .parse::<DocumentMut>()
            .map_err(|error| request::Error::Manifest {
                reason: error.to_string().into_boxed_str(),
            })?;
    if !document.contains_key("workspace") {
        return Err(request::Error::Manifest {
            reason: "final-form harness has no standalone workspace".into(),
        });
    }
    let package = document
        .get_mut("package")
        .and_then(Item::as_table_like_mut)
        .ok_or_else(|| request::Error::Manifest {
            reason: "final-form harness has no package table".into(),
        })?;
    package.insert("name", value("rawr-generated-runner"));
    package.insert("version", value("0.0.0"));
    package.insert("edition", value("2024"));
    package.insert("build", value(BUILD_PATH));
    document.remove("lib");
    // Rawr's own crate is a build dependency of the harness, which builds no
    // unit of it. The runner links it, and its build script must not.
    if let Some(item) = document.remove(crate::cargo::BUILD_DEPENDENCIES) {
        let build = item.into_table().map_err(|_| request::Error::Manifest {
            reason: "final-form harness build dependencies are not a table".into(),
        })?;
        let dependencies = document
            .get_mut("dependencies")
            .and_then(Item::as_table_mut)
            .ok_or_else(|| request::Error::Manifest {
                reason: "final-form harness has no dependencies table".into(),
            })?;
        for (key, item) in build {
            dependencies.insert(&key, item);
        }
        dependencies.sort_values();
    }
    // Each crate a macro's `$crate` names, as Rust reaches it.
    if !hidden.is_empty() {
        let dependencies = document
            .get_mut("dependencies")
            .and_then(Item::as_table_like_mut)
            .ok_or_else(|| request::Error::Manifest {
                reason: "final-form harness has no dependencies table".into(),
            })?;
        for hidden in hidden {
            dependencies.insert(
                &hidden.segment,
                Item::Value(toml_edit::Value::InlineTable(hidden_dependency(hidden))),
            );
        }
    }
    // The binary is the script crate's name; its bundle keeps the executable
    // as `rawr-generated-runner`.
    if let Some(binary) = binary.filter(|binary| *binary != RUNNER_BINARY) {
        let mut target = toml_edit::Table::new();
        target.insert("name", value(binary));
        target.insert("path", value(SOURCE_PATH));
        let mut targets = toml_edit::ArrayOfTables::new();
        targets.push(target);
        document.insert("bin", Item::ArrayOfTables(targets));
    }
    // The project's own profile, `[patch]` and `[replace]` come with the
    // harness manifest unchanged, so the runner's units are the ones the
    // project's `cargo build` compiles (D609.1).
    Ok(normalize(document.to_string().as_bytes()))
}

fn runner_lock(bytes: &[u8], hidden: &[HiddenCrate]) -> Result<Vec<u8>, request::Error> {
    let text = std::str::from_utf8(bytes).map_err(|error| request::Error::Manifest {
        reason: error.to_string().into_boxed_str(),
    })?;
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut document =
        normalized
            .parse::<DocumentMut>()
            .map_err(|error| request::Error::Manifest {
                reason: error.to_string().into_boxed_str(),
            })?;
    let packages = document
        .get_mut("package")
        .and_then(Item::as_array_of_tables_mut)
        .ok_or_else(|| request::Error::Manifest {
            reason: "final-form harness lock has no package array".into(),
        })?;
    let mut roots = 0_usize;
    for package in packages.iter_mut() {
        if package.get("name").and_then(Item::as_str) == Some("rawr-resolution-harness")
            && package.get("version").and_then(Item::as_str) == Some("0.0.0")
            && !package.contains_key("source")
        {
            package.insert("name", value("rawr-generated-runner"));
            // The runner depends on each crate a macro's `$crate` names.
            if !hidden.is_empty() {
                let dependencies = package
                    .entry("dependencies")
                    .or_insert(toml_edit::value(toml_edit::Array::new()))
                    .as_array_mut()
                    .ok_or_else(|| request::Error::Manifest {
                        reason: "final-form harness lock root dependencies are not a list".into(),
                    })?;
                for hidden in hidden {
                    if !dependencies
                        .iter()
                        .any(|dependency| dependency.as_str() == Some(hidden.package.as_str()))
                    {
                        dependencies.push(hidden.package.as_str());
                    }
                }
                let mut sorted = dependencies
                    .iter()
                    .filter_map(|dependency| dependency.as_str().map(str::to_owned))
                    .collect::<Vec<_>>();
                sorted.sort();
                let mut array = toml_edit::Array::new();
                for dependency in sorted {
                    array.push(dependency);
                }
                *dependencies = array;
            }
            roots += 1;
        }
    }
    if roots != 1 {
        return Err(request::Error::Manifest {
            reason: format!("expected one final-form harness lock root, found {roots}")
                .into_boxed_str(),
        });
    }
    Ok(normalize(document.to_string().as_bytes()))
}

fn normalize(bytes: &[u8]) -> Vec<u8> {
    let mut output = Vec::with_capacity(bytes.len().saturating_add(1));
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'\r' if bytes.get(index + 1) == Some(&b'\n') => {
                output.push(b'\n');
                index += 2;
            }
            b'\r' => {
                output.push(b'\n');
                index += 1;
            }
            byte => {
                output.push(byte);
                index += 1;
            }
        }
    }
    if !output.ends_with(b"\n") {
        output.push(b'\n');
    }
    output
}

#[allow(clippy::too_many_arguments)]
fn render_source(
    plans: &[Plan],
    surface: crate::surface::Fingerprint,
    profile: &crate::profile::Proof,
    compile_snapshot: bool,
    facade_plan: Option<&[u8]>,
    (lock, hidden): (&[u8], &mut Vec<HiddenCrate>),
    sources: &mut Vec<(Box<str>, Vec<u8>)>,
    spans: &mut Vec<(EntryIdentity, Box<str>, std::ops::Range<u32>)>,
) -> Result<Option<String>, request::Error> {
    let mut main = Vec::new();
    let output = &mut main;
    if !profile.projections().is_empty() && facade_plan.is_none() {
        return Err(request::Error::Drift {
            field: "generated profile facade plan",
        });
    }
    let facade = facade_plan
        .map(|bytes| {
            crate::surface::facade::Plan::decode(bytes, &crate::cargo::Limits::standard()).map_err(
                |_| request::Error::Drift {
                    field: "generated profile facade plan",
                },
            )
        })
        .transpose()?;
    output
        .write_all(
            b"#![forbid(unsafe_code)]\n\
              static GENERATED_MANIFEST: &[u8] = include_bytes!(concat!(env!(\"OUT_DIR\"), \"/rawr-generated-manifest.bin\"));\n",
        )
        .map_err(|_| request::Error::Encoding)?;
    // The per-generation stamps change with every demand change; their own
    // module keeps `src/main.rs` byte-identical across such edits.
    let mut stamps = Vec::new();
    stamps
        .write_all(b"pub(super) static COMPILED_SURFACE: [u8; 32] = ")
        .map_err(|_| request::Error::Encoding)?;
    byte_array(&mut stamps, surface.as_bytes())?;
    stamps
        .write_all(b";\npub(super) static PROFILE_FINGERPRINT: [u8; 32] = ")
        .map_err(|_| request::Error::Encoding)?;
    byte_array(&mut stamps, profile.owner_fingerprint())?;
    writeln!(
        stamps,
        ";\npub(super) static COMPILE_SNAPSHOT: bool = {compile_snapshot};"
    )
    .map_err(|_| request::Error::Encoding)?;
    // Each trait the program names is in scope for every adapter, so a
    // method it provides resolves as it does in the program.
    let _type_paths = TypePaths::install(facade.as_ref(), profile.has_generated_projections());
    if let Some(plan) = &facade {
        for facade in plan.facades() {
            // A macro's `$crate` path to a trait names the crate that
            // defines it, which the runner depends on as Rust's `$crate`
            // reaches it (`HiddenCrate`).
            for path in facade.traits() {
                if path.contains(crate::api::ledger::CRATE_SEGMENT) {
                    let Some((segment, rest)) = path
                        .split("::")
                        .position(|segment| segment.starts_with(crate::api::ledger::CRATE_SEGMENT))
                        .map(|at| {
                            let segments = path.split("::").collect::<Vec<_>>();
                            (segments[at].to_owned(), segments[at + 1..].join("::"))
                        })
                    else {
                        continue;
                    };
                    if rest.is_empty() {
                        continue;
                    }
                    if !hidden.iter().any(|known| known.segment == segment) {
                        let Some(found) = hidden_crate(lock, &segment) else {
                            continue;
                        };
                        hidden.push(found);
                    }
                    writeln!(
                        stamps,
                        "#[allow(unused_imports, deprecated)]\npub(super) use ::{segment}::{rest} as _;"
                    )
                    .map_err(|_| request::Error::Encoding)?;
                    continue;
                }
                writeln!(
                    stamps,
                    "#[allow(unused_imports, deprecated)]\npub(super) use {}::{path} as _;",
                    facade.root()
                )
                .map_err(|_| request::Error::Encoding)?;
            }
        }
    }
    output
        .write_all(
            b"mod rawr_surface;\nuse rawr_surface::{COMPILED_SURFACE, COMPILE_SNAPSHOT, PROFILE_FINGERPRINT};\n#[allow(unused_imports)]\nuse rawr_surface::*;\nmod rawr_families;\n#[allow(unused_imports)]\nuse rawr_families::*;\ninclude!(\"rawr_proxies.rs\");\n",
        )
        .map_err(|_| request::Error::Encoding)?;
    // The facade plan is megabytes of data: rustc reads it with
    // `include_bytes!` instead of parsing it as an integer array literal.
    output
        .write_all(b"static IMPORTED_FACADES: &[u8] = include_bytes!(\"imported_facades.bin\");\n")
        .map_err(|_| request::Error::Encoding)?;
    sources.push((
        FACADES_PATH.into(),
        facade_plan.unwrap_or_default().to_vec(),
    ));
    output
        .write_all(RUNTIME_SOURCE)
        .map_err(|_| request::Error::Encoding)?;
    output
        .write_all(TOP_LEVEL_SOURCE)
        .map_err(|_| request::Error::Encoding)?;
    let profile_source = if profile.has_generated_projections() {
        profile.generated_source()
    } else {
        NEUTRAL_PROFILE_SOURCE
    };
    output
        .write_all(profile_source)
        .map_err(|_| request::Error::Encoding)?;
    render_profile_projection_bindings(profile, facade.as_ref(), output)?;
    let mut modules = std::collections::BTreeMap::<String, Vec<usize>>::new();
    for (index, plan) in plans.iter().enumerate() {
        modules.entry(adapter_module(plan)).or_default().push(index);
    }
    output
        .write_all(b"\nstatic ADAPTER_MODULES: &[&[Adapter]] = &[\n")
        .map_err(|_| request::Error::Encoding)?;
    for module in modules.keys() {
        writeln!(output, "    {module}::ADAPTERS,").map_err(|_| request::Error::Encoding)?;
    }
    output
        .write_all(
            b"];\n\nfn adapters() -> impl Iterator<Item = &'static Adapter> {\n    ADAPTER_MODULES.iter().flat_map(|rows| rows.iter())\n}\n\n",
        )
        .map_err(|_| request::Error::Encoding)?;
    render_imported_defaults(facade.as_ref(), output)?;
    output
        .write_all(
            b"static STRUCTURAL_ROWS: &[__rawr_internal::package::import::runner::StructuralRow] = &[\n",
        )
        .map_err(|_| request::Error::Encoding)?;
    // Structural rows bind to installed facades; without a facade plan there
    // are none, so no row could be admitted.
    for plan in plans.iter().filter(|_| facade_plan.is_some()) {
        let Some(kind) = structural_kind(plan) else {
            continue;
        };
        let selector = plan.selector().ok_or(request::Error::Drift {
            field: "generated structural selector",
        })?;
        write!(
            output,
            "    __rawr_internal::package::import::runner::StructuralRow::new({:?}, {:?}, ",
            plan.export().root_alias(),
            structural_owner(plan),
        )
        .map_err(|_| request::Error::Encoding)?;
        byte_array(output, selector)?;
        output
            .write_all(b", ")
            .map_err(|_| request::Error::Encoding)?;
        byte_array(output, plan.identity().as_bytes())?;
        output
            .write_all(b", ")
            .map_err(|_| request::Error::Encoding)?;
        byte_array(output, &signature(plan)?)?;
        write!(output, ", {}, {kind})", arguments(plan)?).map_err(|_| request::Error::Encoding)?;
        render_structural_instance(plan, output)?;
        if has_matcher(plan) {
            write!(
                output,
                "    __rawr_internal::package::import::runner::StructuralRow::new({:?}, {:?}, ",
                plan.export().root_alias(),
                structural_owner(plan),
            )
            .map_err(|_| request::Error::Encoding)?;
            byte_array(output, selector)?;
            output
                .write_all(b", ")
                .map_err(|_| request::Error::Encoding)?;
            byte_array(output, &matcher_identity(plan))?;
            output
                .write_all(b", ")
                .map_err(|_| request::Error::Encoding)?;
            byte_array(output, &signature(plan)?)?;
            writeln!(
                output,
                ", 1, __rawr_internal::package::import::runner::StructuralKind::Match)"
            )
            .map_err(|_| request::Error::Encoding)?;
            render_structural_instance(plan, output)?;
        }
        if has_store(plan) {
            write!(
                output,
                "    __rawr_internal::package::import::runner::StructuralRow::new({:?}, {:?}, ",
                plan.export().root_alias(),
                structural_owner(plan),
            )
            .map_err(|_| request::Error::Encoding)?;
            byte_array(output, selector)?;
            output
                .write_all(b", ")
                .map_err(|_| request::Error::Encoding)?;
            byte_array(output, &store_identity(plan))?;
            output
                .write_all(b", ")
                .map_err(|_| request::Error::Encoding)?;
            byte_array(output, &signature(plan)?)?;
            writeln!(
                output,
                ", 2, __rawr_internal::package::import::runner::StructuralKind::Store)"
            )
            .map_err(|_| request::Error::Encoding)?;
            render_structural_instance(plan, output)?;
        }
    }
    output
        .write_all(b"];\n\n")
        .map_err(|_| request::Error::Encoding)?;
    render_event_rows(plans, profile, output)?;
    render_execution_tests(modules.keys(), output)?;
    let runner = if profile.owns_product_lifecycle() {
        LIFECYCLE_PROFILE_RUNNER_MAIN
    } else if profile.has_generated_projections() {
        PROFILE_RUNNER_MAIN
    } else {
        RUNNER_MAIN
    };
    output
        .write_all(runner)
        .map_err(|_| request::Error::Encoding)?;
    output
        .write_all(b"\n")
        .map_err(|_| request::Error::Encoding)?;
    for module in modules.keys() {
        writeln!(output, "mod {module};").map_err(|_| request::Error::Encoding)?;
    }
    output
        .write_all(b"mod rawr_applications;\n")
        .map_err(|_| request::Error::Encoding)?;
    output
        .write_all(b"include!(\"rawr_impls.rs\");\n")
        .map_err(|_| request::Error::Encoding)?;
    sources.push((SOURCE_PATH.into(), main));
    let primitive = plans.iter().position(|plan| {
        matches!(
            plan.operation(),
            Operation::Function { signature, .. }
                if signature.inputs().is_empty()
                    && matches!(signature.output(), convert::Type::Builtin(convert::Builtin::I64))
        )
    });
    let builder = plans
        .iter()
        .position(|plan| builder_execution_inputs(plan.operation()).is_some());
    for (module, indices) in &modules {
        let mut file = Vec::new();
        file.write_all(b"use super::*;\n\npub(super) static ADAPTERS: &[Adapter] = &[\n")
            .map_err(|_| request::Error::Encoding)?;
        for index in indices {
            let plan = &plans[*index];
            write!(file, "    Adapter {{ identity: ").map_err(|_| request::Error::Encoding)?;
            byte_array(&mut file, plan.identity().as_bytes())?;
            write!(file, ", signature: ").map_err(|_| request::Error::Encoding)?;
            byte_array(&mut file, &signature(plan)?)?;
            write!(file, ", selector: ").map_err(|_| request::Error::Encoding)?;
            byte_array(&mut file, plan.selector().unwrap_or(&[0; 32]))?;
            write!(file, ", instance: ").map_err(|_| request::Error::Encoding)?;
            byte_array(&mut file, &instance_key(plan))?;
            write!(
                file,
                ", bound: {}, application: {}, root: {:?}, witness: {}",
                plan.selector().is_some(),
                plan.application_invocation().is_some(),
                plan.export().root_alias(),
                witness_mode(plan) | if returns_values(plan) { 0x80 } else { 0 }
            )
            .map_err(|_| request::Error::Encoding)?;
            writeln!(
                file,
                ", arguments: {}, invoke: {} }},",
                arguments(plan)?,
                adapter_name(plan)
            )
            .map_err(|_| request::Error::Encoding)?;
            if has_matcher(plan) {
                write!(file, "    Adapter {{ identity: ").map_err(|_| request::Error::Encoding)?;
                byte_array(&mut file, &matcher_identity(plan))?;
                write!(file, ", signature: ").map_err(|_| request::Error::Encoding)?;
                byte_array(&mut file, &signature(plan)?)?;
                write!(file, ", selector: ").map_err(|_| request::Error::Encoding)?;
                byte_array(&mut file, plan.selector().unwrap_or(&[0; 32]))?;
                write!(file, ", instance: ").map_err(|_| request::Error::Encoding)?;
                byte_array(&mut file, &[0; 32])?;
                writeln!(
                    file,
                    ", bound: false, application: false, root: {:?}, witness: 0, arguments: 1, invoke: {} }},",
                    plan.export().root_alias(),
                    matcher_name(plan)
                )
                .map_err(|_| request::Error::Encoding)?;
            }
            if has_store(plan) {
                write!(file, "    Adapter {{ identity: ").map_err(|_| request::Error::Encoding)?;
                byte_array(&mut file, &store_identity(plan))?;
                write!(file, ", signature: ").map_err(|_| request::Error::Encoding)?;
                byte_array(&mut file, &signature(plan)?)?;
                write!(file, ", selector: ").map_err(|_| request::Error::Encoding)?;
                byte_array(&mut file, plan.selector().unwrap_or(&[0; 32]))?;
                write!(file, ", instance: ").map_err(|_| request::Error::Encoding)?;
                byte_array(&mut file, &[0; 32])?;
                writeln!(
                    file,
                    ", bound: false, application: false, root: {:?}, witness: 0, arguments: 2, invoke: {} }},",
                    plan.export().root_alias(),
                    store_name(plan)
                )
                .map_err(|_| request::Error::Encoding)?;
            }
        }
        file.write_all(b"];\n\n")
            .map_err(|_| request::Error::Encoding)?;
        let path: Box<str> = format!("src/{module}.rs").into();
        for index in indices {
            let plan = &plans[*index];
            let start = span_offset(&file)?;
            render_adapter(plan, plans, profile, facade.as_ref(), &mut file)?;
            render_matcher(plan, &mut file)?;
            render_store(plan, &mut file)?;
            let end = span_offset(&file)?;
            spans.push((plan.identity(), path.clone(), start..end));
        }
        render_module_execution_tests(plans, indices, primitive, builder, &mut file)?;
        sources.push((path, file));
    }
    let mut applications = Vec::new();
    render_applications(plans, &mut applications)?;
    sources.push((APPLICATIONS_PATH.into(), applications));
    let mut families = Vec::new();
    render_families(&mut families)?;
    view::render_families(&mut families)?;
    sources.push((FAMILIES_PATH.into(), families));
    // The impls name the proxies of their owners, so they come first.
    let mut impls = Vec::new();
    render_impls(plans, &mut impls)?;
    let mut proxies = Vec::new();
    render_proxies(&mut proxies)?;
    // In path order, as the sources are framed.
    sources.push((IMPLS_PATH.into(), impls));
    sources.push((PROXIES_PATH.into(), proxies));
    sources.push((SURFACE_PATH.into(), stamps));
    Ok(PROXY_CRATE.with_borrow(Clone::clone))
}

/// The key a call of this adapter's instance carries, or zeros for an
/// item that is not generic.
fn instance_key(plan: &Plan) -> [u8; 32] {
    match plan.operation() {
        Operation::Function {
            substitutions,
            slots,
            ..
        }
        | Operation::Method {
            substitutions,
            slots,
            ..
        } if !substitutions.is_empty() || !slots.is_empty() => {
            crate::surface::instance::key_with_slots(substitutions, slots)
        }
        _ => [0; 32],
    }
}

/// Each adapter lives in one module per imported crate and identity
/// bucket, named by its identity, so adding, removing or changing one adapter
/// changes exactly one module file and rustc can reuse the others.
fn adapter_module(plan: &Plan) -> String {
    let mut module = String::from("rawr_adapters_");
    for character in plan.export().root_alias().chars() {
        module.push(if character.is_ascii_alphanumeric() {
            character.to_ascii_lowercase()
        } else {
            '_'
        });
    }
    module.push('_');
    module.push(char::from(
        b"0123456789abcdef"[usize::from(plan.identity().as_bytes()[0] >> 4)],
    ));
    module
}

fn adapter_name(plan: &Plan) -> String {
    identity_name("adapter", plan)
}

fn identity_name(prefix: &str, plan: &Plan) -> String {
    let mut name = String::with_capacity(prefix.len() + 65);
    name.push_str(prefix);
    name.push('_');
    for byte in plan.identity().as_bytes() {
        name.push(char::from(b"0123456789abcdef"[usize::from(byte >> 4)]));
        name.push(char::from(b"0123456789abcdef"[usize::from(byte & 0x0f)]));
    }
    name
}

fn span_offset(output: &[u8]) -> Result<u32, request::Error> {
    u32::try_from(output.len()).map_err(|_| request::Error::Limit {
        resource: "generated source span",
        limit: u32::MAX as usize,
        actual: output.len(),
    })
}

/// The scope families the adapters name (D558 tier 1), after every adapter
/// module in path order.
const FAMILIES_PATH: &str = "src/rawr_families.rs";

/// The runner's proxies of script types (D594), included at the crate root
/// so each is named as the script names it.
const PROXIES_PATH: &str = "src/rawr_proxies.rs";
/// The script's impls for the proxies (D594), included after the runtime
/// they call, whose macros come first.
const IMPLS_PATH: &str = "src/rawr_impls.rs";

/// The proof-backed applications' isolated call helpers and their encoded
/// applications, after every adapter module in path order.
const APPLICATIONS_PATH: &str = "src/rawr_applications.rs";

/// Render each application's helper module. A helper sees no inherited
/// glob: `#[no_implicit_prelude]` removes the implicit prelude, then the
/// module imports exactly the recorded prelude and traits of the source
/// call's lexical scope, so method lookup sees what the source saw.
fn render_applications(plans: &[Plan], output: &mut Vec<u8>) -> Result<(), request::Error> {
    let encoding = |_| request::Error::Encoding;
    let mut applications = Vec::new();
    for plan in plans {
        let Some(invocation) = plan.application_invocation() else {
            continue;
        };
        applications.push(crate::surface::application::Application {
            invocation: invocation.clone(),
        });
        let name = identity_name("application", plan);
        writeln!(output, "#[no_implicit_prelude]\n#[allow(unused_imports, dead_code, non_snake_case)]\npub(crate) mod {name} {{").map_err(encoding)?;
        if let Some(prelude) = &invocation.prelude {
            writeln!(output, "    use ::{}::*;", prelude.join("::")).map_err(encoding)?;
        }
        for path in invocation.traits.iter() {
            writeln!(output, "    use ::{} as _;", path.join("::")).map_err(encoding)?;
        }
        let lent = match invocation.ownership {
            crate::classify::representability::ownership::Receiver::Shared => "&",
            crate::classify::representability::ownership::Receiver::Mutable => "&mut ",
            crate::classify::representability::ownership::Receiver::Owned => "",
        };
        write!(output, "    pub(crate) fn call(receiver: {lent}").map_err(encoding)?;
        render_native(&invocation.native_receiver, output)?;
        for (index, input) in invocation.native_inputs.iter().enumerate() {
            write!(output, ", argument_{index}: ").map_err(encoding)?;
            render_native(input, output)?;
        }
        output.write_all(b") -> ").map_err(encoding)?;
        render_native(&invocation.native_output, output)?;
        output.write_all(b" {\n        ").map_err(encoding)?;
        let mut arguments = Vec::new();
        argument_list(invocation.native_inputs.len(), 0, &mut arguments)?;
        let arguments = String::from_utf8(arguments).map_err(|_| request::Error::Encoding)?;
        match &invocation.expression {
            crate::surface::application::Expression::Method { name } if lent.is_empty() => {
                writeln!(output, "receiver.{name}({arguments})").map_err(encoding)?;
            }
            crate::surface::application::Expression::Method { name } => {
                // The source's receiver is a place of the type: method
                // lookup starts there, not at the adapter's reference.
                writeln!(output, "(*receiver).{name}({arguments})").map_err(encoding)?;
            }
            crate::surface::application::Expression::Ufcs { path, name } => {
                let separator = if arguments.is_empty() { "" } else { ", " };
                writeln!(
                    output,
                    "::{}::{name}(receiver{separator}{arguments})",
                    path.join("::")
                )
                .map_err(encoding)?;
            }
        }
        output.write_all(b"    }\n}\n\n").map_err(encoding)?;
    }
    let encoded = crate::surface::application::Application::encode_all(&applications)
        .map_err(|_| request::Error::Encoding)?;
    output
        .write_all(b"pub(crate) static APPLICATIONS: &[u8] = &")
        .map_err(encoding)?;
    byte_array(output, &encoded)?;
    output.write_all(b";\n").map_err(encoding)
}

fn render_native(
    ty: &crate::surface::application::Type,
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    use crate::surface::application::Type;
    let encoding = |_| request::Error::Encoding;
    match ty {
        Type::Primitive(name) => output.write_all(name.as_bytes()).map_err(encoding),
        Type::Export { path, arguments } => {
            write!(output, "::{}", path.join("::")).map_err(encoding)?;
            if !arguments.is_empty() {
                output.write_all(b"<").map_err(encoding)?;
                for (index, argument) in arguments.iter().enumerate() {
                    if index != 0 {
                        output.write_all(b", ").map_err(encoding)?;
                    }
                    render_native(argument, output)?;
                }
                output.write_all(b">").map_err(encoding)?;
            }
            Ok(())
        }
        Type::Tuple(members) => {
            output.write_all(b"(").map_err(encoding)?;
            for member in members.iter() {
                render_native(member, output)?;
                output.write_all(b",").map_err(encoding)?;
            }
            output.write_all(b")").map_err(encoding)
        }
        Type::Reference { mutable, ty } => {
            output
                .write_all(if *mutable { b"&mut " } else { b"&" })
                .map_err(encoding)?;
            render_native(ty, output)
        }
    }
}

/// The per-generation stamps module, after every adapter module in path order.
const SURFACE_PATH: &str = "src/rawr_surface.rs";

/// The facade plan `src/main.rs` includes as bytes.
const FACADES_PATH: &str = "src/imported_facades.bin";

/// The generated runner's source files in path order: the facade plan,
/// `src/main.rs` and one file per adapter module, framed into one canonical
/// byte string.
const SOURCES_MAGIC: &[u8; 8] = b"RAWRSRC1";

pub(crate) fn encode_sources(sources: &[(Box<str>, Vec<u8>)]) -> Result<Vec<u8>, request::Error> {
    let length = sources
        .iter()
        .try_fold(SOURCES_MAGIC.len(), |total, (path, bytes)| {
            total
                .checked_add(16)?
                .checked_add(path.len())?
                .checked_add(bytes.len())
        });
    let mut encoded = Vec::new();
    encoded
        .try_reserve_exact(length.ok_or(request::Error::Allocation)?)
        .map_err(|_| request::Error::Allocation)?;
    encoded.extend_from_slice(SOURCES_MAGIC);
    for (path, bytes) in sources {
        encoded.extend_from_slice(&(path.len() as u64).to_le_bytes());
        encoded.extend_from_slice(path.as_bytes());
        encoded.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
        encoded.extend_from_slice(bytes);
    }
    Ok(encoded)
}

/// Make `root` hold exactly `files`, writing a file only when its bytes
/// changed (an unchanged file keeps its mtime and Cargo sees it unchanged) and
/// removing generated sources no longer produced. Returns the paths written.
pub fn sync_sources<'a>(
    root: &std::path::Path,
    files: impl IntoIterator<Item = (&'a str, &'a [u8])>,
) -> std::io::Result<Vec<std::path::PathBuf>> {
    let mut written = Vec::new();
    let mut kept = std::collections::BTreeSet::new();
    for (path, bytes) in files {
        let target = root.join(path);
        let unchanged = std::fs::metadata(&target)
            .is_ok_and(|metadata| metadata.len() == bytes.len() as u64)
            && std::fs::read(&target).is_ok_and(|current| current == bytes);
        if !unchanged {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&target, bytes)?;
            written.push(std::path::PathBuf::from(path));
        }
        kept.insert(target);
    }
    for entry in std::fs::read_dir(root.join("src"))?.flatten() {
        let path = entry.path();
        if !kept.contains(&path) && path.extension().is_some_and(|extension| extension == "rs") {
            std::fs::remove_file(&path)?;
        }
    }
    Ok(written)
}

fn take<'a>(bytes: &mut &'a [u8], length: usize) -> Option<&'a [u8]> {
    let (head, tail) = bytes.split_at_checked(length)?;
    *bytes = tail;
    Some(head)
}

fn length(bytes: &mut &[u8]) -> Option<usize> {
    let (head, tail) = bytes.split_first_chunk::<8>()?;
    *bytes = tail;
    usize::try_from(u64::from_le_bytes(*head)).ok()
}

pub(crate) fn decode_sources(mut bytes: &[u8]) -> Result<Vec<(&str, &[u8])>, request::Error> {
    let drift = request::Error::Drift {
        field: "generated source framing",
    };
    if take(&mut bytes, SOURCES_MAGIC.len()) != Some(SOURCES_MAGIC.as_slice()) {
        return Err(drift);
    }
    let mut sources = Vec::new();
    while !bytes.is_empty() {
        let path = length(&mut bytes)
            .and_then(|path| take(&mut bytes, path))
            .and_then(|path| std::str::from_utf8(path).ok())
            .ok_or(request::Error::Drift {
                field: "generated source framing",
            })?;
        let source = length(&mut bytes)
            .and_then(|source| take(&mut bytes, source))
            .ok_or(request::Error::Drift {
                field: "generated source framing",
            })?;
        if sources
            .last()
            .is_some_and(|(previous, _): &(&str, &[u8])| *previous >= path)
            || !(path == FACADES_PATH
                || path.starts_with("src/")
                    && path.ends_with(".rs")
                    && !path[4..path.len() - 3].contains(['/', '\\', '.']))
        {
            return Err(request::Error::Drift {
                field: "generated source paths",
            });
        }
        sources.push((path, source));
    }
    if !sources.iter().any(|(path, _)| *path == SOURCE_PATH) {
        return Err(request::Error::Drift {
            field: "generated source paths",
        });
    }
    Ok(sources)
}

fn render_event_rows(
    plans: &[Plan],
    profile: &crate::profile::Proof,
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    writeln!(
        output,
        "static EVENT_ROWS: &[__rawr_internal::package::import::runner::EventRow] = &["
    )
    .map_err(|_| request::Error::Encoding)?;
    for plan in plans {
        let Some(mapper) = event_mapper(plan)? else {
            continue;
        };
        validate_event_projection(mapper, profile)?;
        let witness = mapper.projection();
        let projection = profile
            .projections()
            .iter()
            .find(|projection| projection.selector() == witness.profile_projection())
            .ok_or(request::Error::Drift {
                field: "event runtime projection",
            })?;
        let target = profile
            .alias_targets()
            .iter()
            .find(|target| target.selector() == projection.alias_selector())
            .ok_or(request::Error::Drift {
                field: "event runtime target",
            })?;
        let selector = plan.selector().ok_or(request::Error::Drift {
            field: "event runtime producer",
        })?;
        write!(
            output,
            "    __rawr_internal::package::import::runner::EventRow::new({:?}, ",
            plan.export().root_alias()
        )
        .map_err(|_| request::Error::Encoding)?;
        byte_array(output, selector)?;
        output
            .write_all(b", ")
            .map_err(|_| request::Error::Encoding)?;
        byte_array(output, plan.identity().as_bytes())?;
        output
            .write_all(b", ")
            .map_err(|_| request::Error::Encoding)?;
        byte_array(output, &signature(plan)?)?;
        write!(output, ", {}, ", arguments(plan)?).map_err(|_| request::Error::Encoding)?;
        byte_array(output, witness.source().as_bytes())?;
        write!(
            output,
            ", {:?}, {:?}, ",
            target.root(),
            format!("{}::{}", target.root(), target.path())
        )
        .map_err(|_| request::Error::Encoding)?;
        byte_array(output, mapper.seal())?;
        writeln!(output, "),").map_err(|_| request::Error::Encoding)?;
    }
    writeln!(output, "];\n").map_err(|_| request::Error::Encoding)
}

fn render_profile_projection_bindings(
    profile: &crate::profile::Proof,
    facade: Option<&crate::surface::facade::Plan>,
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    output
        .write_all(
            b"\nfn bind_profile_projections(\n\
              \x20   definitions: &[__rawr_internal::host::facade::Definition],\n\
              \x20   builders: &mut [__rawr_internal::host::runtime::Builder],\n\
              ) -> Result<(), __rawr_internal::host::runtime::Error> {\n\
              \x20   let _ = (&definitions, &builders);\n",
        )
        .map_err(|_| request::Error::Encoding)?;
    if profile.projections().is_empty() {
        output
            .write_all(b"    let _ = (definitions, builders);\n    Ok(())\n}\n")
            .map_err(|_| request::Error::Encoding)?;
        return Ok(());
    }
    let facade = facade.ok_or(request::Error::Drift {
        field: "generated projection facade plan",
    })?;
    let mut support_attestation = false;
    for projection in profile.projections() {
        let source = profile
            .source_for_projection(projection)
            .ok_or(request::Error::Drift {
                field: "generated projection source site",
            })?;
        let mut targets = profile
            .alias_targets()
            .iter()
            .filter(|alias| alias.selector() == projection.alias_selector());
        let target = targets.next().ok_or(request::Error::Drift {
            field: "generated projection alias target",
        })?;
        if targets.next().is_some() {
            return Err(request::Error::Drift {
                field: "generated projection alias target",
            });
        }
        let mut sites = facade
            .aliases()
            .filter(|site| site.selector() == target.site_selector());
        let site = sites.next().ok_or(request::Error::Drift {
            field: "generated projection alias site",
        })?;
        if sites.next().is_some()
            || site.root() != target.root()
            || site.path() != target.path()
            || site.export_identity() != target.export_identity()
            || site.alias_identity() != target.alias_identity()
            || site.target_identity() != target.target_identity()
        {
            return Err(request::Error::Drift {
                field: "generated projection alias site",
            });
        }
        let runtime_source = facade.facades().any(|owner| {
            owner.root() == source.root() && owner.types().any(|ty| ty.path() == source.path())
        });
        if !runtime_source {
            if support_attestation {
                return Err(request::Error::Drift {
                    field: "generated support projection attestation cardinality",
                });
            }
            support_attestation = true;
            writeln!(
                output,
                "    let owner = definitions.iter().position(|definition| definition.root() == {:?})\n        .ok_or_else(|| __rawr_internal::host::runtime::Error::profile(\"generated support projection owner\"))?;",
                target.root(),
            )
            .map_err(|_| request::Error::Encoding)?;
            writeln!(
                output,
                "    builders.get_mut(owner)\n        .ok_or_else(|| __rawr_internal::host::runtime::Error::profile(\"generated support projection builder\"))?\n        .bind_type_attestation({:?}, |value, expected| {{\n            native_attestation(value, expected)\n                .or_else(|| __rawr_profile::attest_support_endpoint(value, expected))\n        }})?;",
                format!("{}::{}", target.root(), target.path()),
            )
            .map_err(|_| request::Error::Encoding)?;
            continue;
        }
        writeln!(
            output,
            "    let owner = definitions.iter().position(|definition| definition.root() == {:?})\n        .ok_or_else(|| __rawr_internal::host::runtime::Error::profile(\"generated projection owner\"))?;",
            source.root(),
        )
        .map_err(|_| request::Error::Encoding)?;
        writeln!(
            output,
            "    builders.get_mut(owner)\n        .ok_or_else(|| __rawr_internal::host::runtime::Error::profile(\"generated projection builder\"))?\n        .bind_projection_with_context({:?}, {:?}, runtime_project)?;",
            format!("{}::{}", source.root(), source.path()),
            format!("{}::{}", target.root(), target.path()),
        )
        .map_err(|_| request::Error::Encoding)?;
    }
    output
        .write_all(b"    Ok(())\n}\n")
        .map_err(|_| request::Error::Encoding)
}

/// Whether the plan's output, outside a future, holds a script value an
/// erased generic gives back (D513.2).
fn returns_values(plan: &Plan) -> bool {
    let output = match plan.operation() {
        Operation::Function {
            signature,
            substitutions,
            ..
        }
        | Operation::Method {
            signature,
            substitutions,
            ..
        } if !matches!(
            signature.guard(),
            Some(
                crate::classify::representability::signature::Guard::Mutable
                    | crate::classify::representability::signature::Guard::OptionMutable
            )
        ) =>
        {
            substituted(signature.output(), signature, substitutions).ok()
        }
        Operation::StructField { field, .. } => Some(field.ty().clone()),
        // A `DerefMut` guard's call gives its place, not its target.
        _ => None,
    };
    output.is_some_and(|output| !is_future(&output) && holds_value(&output))
}

/// Whether `ty` holds a script value an erased generic gives back.
fn holds_value(ty: &convert::Type) -> bool {
    match ty {
        // A native value, `Sender<rawr::Value>` too, is attested by its own
        // witness; only a script value in its place comes back as itself.
        convert::Type::Nominal { definition, .. } => {
            *definition == crate::surface::identity::Definition::value()
        }
        convert::Type::Option(element) | convert::Type::Sequence { element, .. } => {
            holds_value(element)
        }
        convert::Type::Result(ok, err) => holds_value(ok) || holds_value(err),
        convert::Type::Tuple(members) => members.iter().any(holds_value),
        convert::Type::ProfileMessage { payload, .. } => holds_value(payload),
        _ => false,
    }
}

/// A script value an erased generic takes or gives (D513.2).
fn is_value(ty: &convert::Type) -> bool {
    matches!(ty, convert::Type::Nominal { definition, .. }
        if *definition == crate::surface::identity::Definition::value())
}

fn witness_mode(plan: &Plan) -> u8 {
    match plan.operation() {
        Operation::StructConstructor { .. }
        | Operation::EnumVariant { .. }
        | Operation::Function { .. }
        | Operation::Constant { .. } => 1,
        // A trait's method on Rawr's string, which Rawr calls as a function
        // (`surface::text`).
        Operation::Method {
            receiver: Receiver::Owned,
            signature,
            ..
        } if signature.instance() == Some(&convert::Type::Builtin(convert::Builtin::StaticStr)) => {
            1
        }
        Operation::StructField { field, .. } if !copy_scalar(field.ty()) => 2,
        Operation::Method {
            receiver: Receiver::Owned,
            ..
        } => 2,
        Operation::StructField { .. }
        | Operation::Declaration { .. }
        | Operation::Method {
            receiver: Receiver::Shared | Receiver::Mutable,
            ..
        }
        | Operation::Builder { .. } => 0,
    }
}

/// Whether `ty` is Copy as the compiler sees it: a scalar, an imported
/// Copy type, or an `Option` or `Result` of them, as a `cpu_cores:
/// Option<usize>` field is.
fn copy_scalar(ty: &convert::Type) -> bool {
    match ty {
        convert::Type::Unit => true,
        convert::Type::Builtin(builtin) => builtin.rust_name().is_some(),
        convert::Type::Nominal { definition, .. } => {
            COPY_TYPES.with_borrow(|copy| copy.contains(definition.as_bytes()))
        }
        convert::Type::Option(inner) => copy_scalar(inner),
        convert::Type::Result(ok, err) => copy_scalar(ok) && copy_scalar(err),
        _ => false,
    }
}

/// A variant of a non-generic enum also has a matcher: the variant's
/// fields, cloned, when a value is that variant (`render_matcher`).
fn has_matcher(plan: &Plan) -> bool {
    matches!(plan.operation(), Operation::EnumVariant { parameters, .. } if parameters.is_empty())
}

/// The entry identity of a variant's matcher: its constructor's, tagged.
fn matcher_identity(plan: &Plan) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"rawr-variant-matcher");
    hasher.update(plan.identity().as_bytes());
    hasher.finalize().into()
}

fn matcher_name(plan: &Plan) -> String {
    format!("{}_match", adapter_name(plan))
}

/// `Some` of the variant's fields, cloned, when the value the first
/// argument holds is that variant; otherwise `None`.
fn render_matcher(plan: &Plan, output: &mut Vec<u8>) -> Result<(), request::Error> {
    let Operation::EnumVariant {
        name, form, fields, ..
    } = plan.operation()
    else {
        return Ok(());
    };
    if !has_matcher(plan) {
        return Ok(());
    }
    let path = rust_path(plan);
    let bound = (0..fields.len())
        .map(|index| format!("field_{index}"))
        .collect::<Vec<_>>();
    let pattern = match form {
        crate::api::model::FieldForm::Unit => format!("{path}::{name}"),
        crate::api::model::FieldForm::Tuple => format!("{path}::{name}({})", bound.join(", ")),
        crate::api::model::FieldForm::Record => format!(
            "{path}::{name} {{ {} }}",
            fields
                .iter()
                .zip(&bound)
                .map(|(field, local)| format!("{}: {local}", field.name()))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    };
    let cloned = match bound.len() {
        0 => "()".to_string(),
        1 => format!("(Clone::clone({}),)", bound[0]),
        _ => format!(
            "({})",
            bound
                .iter()
                .map(|local| format!("Clone::clone({local})"))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    };
    writeln!(
        output,
        "fn {}(arguments: &mut Arguments<'_, '_, '_>) -> Result<Dynamic, Error> {{",
        matcher_name(plan)
    )
    .map_err(|_| request::Error::Encoding)?;
    // A value in a scope frame's region (D558 tier 1): its fields that
    // carry the region's lifetime stay there.
    if let Some(family) = container_family(&path, &path) {
        let mut values = String::new();
        {
            let _region = InRegion::enter();
            for (index, field) in fields.iter().enumerate() {
                let mut converted = String::new();
                render_to_dynamic(field.ty(), &format!("fields.{index}"), 0, &mut converted)?;
                values.push_str(&format!("\n            values.push(({converted})?);"));
            }
        }
        writeln!(
            output,
            "    if let Some(scoped) = scoped_value(arguments, 0) {{
        return __rawr_internal::scope::with::<__rawr_internal::scope::RegionF, _>(scoped.nonce(), |region| -> Result<Dynamic, Error> {{
            #[allow(unreachable_patterns)]
            let output = match region.get::<{family}>(scoped).map_err(|error| Error::Lent(scoped.name(), error))? {{ {pattern} => Some({cloned}), _ => None }};
            let Some(fields) = output else {{ return Ok(__rawr_internal::option::none()); }};
            #[allow(unused_mut)]
            let mut values: Vec<Dynamic> = Vec::new();
            #[allow(clippy::let_unit_value)]
            let _ = &fields;{values}
            Ok(__rawr_internal::option::some(if values.is_empty() {{ Dynamic::from(()) }} else {{ Dynamic::from_array(values) }}))
        }})
        .map_err(|error| Error::Lent(scoped.name(), error))?;
    }}"
        )
        .map_err(|_| request::Error::Encoding)?;
    }
    // A value native code lent the script function that matches it, as a
    // canvas program's `update` takes `&Event` (D558.3), is reached through
    // its accessor; any other is in the arena.
    writeln!(
        output,
        "    #[allow(unreachable_patterns)]
    let output = match lent_value(arguments, 0) {{
        Some(lent) => {{
            let access: fn(u64, &mut dyn FnMut(&{path})) -> Result<(), u8> = lent
                .accessor(false)
                .map_err(|error| Error::Lent(lent.name(), error))?;
            let mut output = None;
            access(lent.nonce().bits(), &mut |input| {{
                output = Some(match input {{ {pattern} => Some({cloned}), _ => None }});
            }})
            .map_err(|code| Error::Lent(lent.name(), __rawr_internal::scope::Error::from_code(code)))?;
            output.ok_or(Error::Lent(lent.name(), __rawr_internal::scope::Error::Busy))?
        }}
        None => {{
            let handle = native_handle(arguments, 0, false)?;
            arguments.arena.shared::<{path}, _>(handle, |input| match input {{ {pattern} => Some({cloned}), _ => None }})?
        }}
    }};
    let Some(fields) = output else {{ return Ok(__rawr_internal::option::none()); }};"
    )
    .map_err(|_| request::Error::Encoding)?;
    if fields.is_empty() {
        output
            .write_all(b"    let () = fields;\n    Ok(__rawr_internal::option::some(Dynamic::from(())))\n}\n\n")
            .map_err(|_| request::Error::Encoding)?;
        return Ok(());
    }
    output
        .write_all(b"    let mut values = Vec::new();\n")
        .map_err(|_| request::Error::Encoding)?;
    for (index, field) in fields.iter().enumerate() {
        let mut converted = String::new();
        render_to_dynamic(field.ty(), &format!("fields.{index}"), 0, &mut converted)?;
        writeln!(output, "    values.push(({converted})?);")
            .map_err(|_| request::Error::Encoding)?;
    }
    output
        .write_all(b"    Ok(__rawr_internal::option::some(Dynamic::from_array(values)))\n}\n\n")
        .map_err(|_| request::Error::Encoding)
}

/// Key a structural row by the same concrete arguments as its adapter.
fn render_structural_instance(plan: &Plan, output: &mut Vec<u8>) -> Result<(), request::Error> {
    let types = plan.structural_instance();
    if !types.is_empty() {
        output
            .write_all(b".with_instance(")
            .map_err(|_| request::Error::Encoding)?;
        byte_array(output, &crate::surface::instance::key(types))?;
        output
            .write_all(b")")
            .map_err(|_| request::Error::Encoding)?;
    }
    output
        .write_all(b",\n")
        .map_err(|_| request::Error::Encoding)
}

fn structural_type(plan: &Plan) -> String {
    let path = rust_path(plan);
    let instance = plan.structural_instance();
    if instance.is_empty() {
        return path;
    }
    let arguments = instance
        .iter()
        .map(|ty| concrete_type(ty).unwrap_or_else(|| "_".to_owned()))
        .collect::<Vec<_>>()
        .join(", ");
    format!("{path}<{arguments}>")
}

/// A field has a store when the bridge can take its value without keeping
/// a borrow of a scope frame's region (`render_store`).
fn has_store(plan: &Plan) -> bool {
    let Operation::StructField { field, .. } = plan.operation() else {
        return false;
    };
    let path = rust_path(plan);
    container_family(&path, &path).is_none()
        && !may_borrow(field.ty())
        && render_take(0, 1, field.ty(), &mut Vec::new()).is_ok()
}

/// The entry identity of a field's store: its reader's, tagged.
fn store_identity(plan: &Plan) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"rawr-field-store");
    hasher.update(plan.identity().as_bytes());
    hasher.finalize().into()
}

fn store_name(plan: &Plan) -> String {
    format!("{}_store", adapter_name(plan))
}

/// The field set to the second argument in the value the first argument
/// holds, in place: a value native code lent the script function `&mut`
/// is reached through its accessor (D558.3); any other is in the arena,
/// changed where it lives, as a `&mut self` method changes it.
fn render_store(plan: &Plan, output: &mut Vec<u8>) -> Result<(), request::Error> {
    let Operation::StructField { field, .. } = plan.operation() else {
        return Ok(());
    };
    if !has_store(plan) {
        return Ok(());
    }
    let path = structural_type(plan);
    let name = field.name();
    writeln!(
        output,
        "fn {}(arguments: &mut Arguments<'_, '_, '_>) -> Result<Dynamic, Error> {{",
        store_name(plan)
    )
    .map_err(|_| request::Error::Encoding)?;
    render_take(0, 1, field.ty(), output)?;
    writeln!(
        output,
        "    let mut value = Some(argument_0);
    match lent_value(arguments, 0) {{
        Some(lent) => {{
            let access: fn(u64, &mut dyn FnMut(&mut {path})) -> Result<(), u8> = lent
                .accessor(true)
                .map_err(|error| Error::Lent(lent.name(), error))?;
            access(lent.nonce().bits(), &mut |receiver| {{
                if let Some(value) = value.take() {{
                    receiver.{name} = value;
                }}
            }})
            .map_err(|code| Error::Lent(lent.name(), __rawr_internal::scope::Error::from_code(code)))?;
            if value.is_some() {{
                return Err(Error::Lent(lent.name(), __rawr_internal::scope::Error::Busy));
            }}
        }}
        None => {{
            let handle = native_handle(arguments, 0, false)?;
            let value = value.take().ok_or(Error::TypeMismatch)?;
            arguments.arena.mutable::<{path}, _>(handle, |receiver| receiver.{name} = value)?;
        }}
    }}
    Ok(Dynamic::UNIT)
}}
"
    )
    .map_err(|_| request::Error::Encoding)
}

/// The path below the crate root of the type a structural adapter
/// belongs to: `echo::Event` for its variant `Connected`.
fn structural_owner(plan: &Plan) -> String {
    plan.export().path().collect::<Vec<_>>().join("::")
}

fn structural_kind(plan: &Plan) -> Option<String> {
    Some(match plan.operation() {
        Operation::StructConstructor { .. } => {
            "__rawr_internal::package::import::runner::StructuralKind::Constructor".to_string()
        }
        Operation::StructField { field, .. } => format!(
            "__rawr_internal::package::import::runner::StructuralKind::Field {{ copy: {} }}",
            copy_scalar(field.ty())
        ),
        Operation::EnumVariant { .. } => {
            "__rawr_internal::package::import::runner::StructuralKind::Variant".to_string()
        }
        Operation::Declaration { .. }
        | Operation::Function { .. }
        | Operation::Method { .. }
        | Operation::Constant { .. }
        | Operation::Builder { .. } => return None,
    })
}

fn byte_slice(output: &mut Vec<u8>, bytes: &[u8]) -> Result<(), request::Error> {
    output
        .write_all(b"[")
        .map_err(|_| request::Error::Encoding)?;
    for (index, byte) in bytes.iter().enumerate() {
        if index != 0 {
            output
                .write_all(b", ")
                .map_err(|_| request::Error::Encoding)?;
        }
        write!(output, "{byte}").map_err(|_| request::Error::Encoding)?;
    }
    output.write_all(b"]").map_err(|_| request::Error::Encoding)
}

fn render_adapter(
    plan: &Plan,
    cohort: &[Plan],
    profile: &crate::profile::Proof,
    facade: Option<&crate::surface::facade::Plan>,
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    let encoding = |_| request::Error::Encoding;
    let name = adapter_name(plan);
    // A call that makes or takes an iced view value keeps a recipe (D558
    // tier 2).
    if view::is_view_call(plan)? {
        writeln!(
            output,
            "fn {name}(arguments: &mut Arguments<'_, '_, '_>) -> Result<Dynamic, Error> {{\n    let _ = &mut *arguments;"
        )
        .map_err(encoding)?;
        view::render(plan, output)?;
        return output.write_all(b"}\n\n").map_err(encoding);
    }
    // A call that takes a value whose type holds an opaque `impl Trait`
    // type runs what the call that made the value compiled for it.
    if let Some((position, key)) = opaque_operand(plan)? {
        let index = opaque_consumers(cohort, &key)?
            .iter()
            .position(|consumer| std::ptr::eq(*consumer, plan))
            .ok_or(request::Error::Drift {
                field: "an opaque type's consumer",
            })?;
        return writeln!(
            output,
            "fn {name}(arguments: &mut Arguments<'_, '_, '_>) -> Result<Dynamic, Error> {{
    let handle = native_handle(arguments, {position}, false)?;
    let table = arguments.arena.opaque(&handle)?;
    table.call({index}, arguments)
}}
"
        )
        .map_err(encoding);
    }
    let _producing = OpaqueProducing::enter(opaque_table_source(plan, cohort, profile, facade)?);
    let plain = method_message(plan, cohort, profile, facade)?.is_none()
        && replay(plan, profile)?.is_none();
    let lent = if plain {
        lent_inputs(plan)?
    } else {
        Vec::new()
    };
    let region = if plain {
        region_inputs(plan)?
    } else {
        Vec::new()
    };
    let _lent = LentArguments::enter(!lent.is_empty());
    if lent.is_empty() {
        writeln!(
            output,
            "fn {name}(arguments: &mut Arguments<'_, '_, '_>) -> Result<Dynamic, Error> {{"
        )
        .map_err(encoding)?;
    } else {
        // Each argument native code lent (D558.3) is reached for the call
        // through its accessor, and the call runs inside them all.
        let reach = lent
            .iter()
            .map(|(index, mutable, rust)| {
                format!("({index}, {mutable}, reach_lent::<{rust}> as Reach)")
            })
            .collect::<Vec<_>>()
            .join(", ");
        writeln!(
            output,
            "fn {name}(arguments: &mut Arguments<'_, '_, '_>) -> Result<Dynamic, Error> {{
    with_lent_arguments(arguments, &[{reach}], {name}_lent)
}}

fn {name}_lent(arguments: &mut Arguments<'_, '_, '_>, lent: &mut LentArguments<'_>) -> Result<Dynamic, Error> {{
    let _ = &mut *lent;"
        )
        .map_err(encoding)?;
    }
    output
        .write_all(b"    let _ = &mut *arguments;\n")
        .map_err(encoding)?;
    if !region.is_empty() {
        // A value in a scope frame's region the call takes (D558 tier 1),
        // as `push_html(&mut s, parser)` takes `Parser::new(&text)`'s: the
        // call runs inside the region, which keeps what it makes that
        // carries the region's lifetime.
        let indices = region
            .iter()
            .map(usize::to_string)
            .collect::<Vec<_>>()
            .join(", ");
        writeln!(
            output,
            "    if let Some(scoped) = [{indices}].into_iter().find_map(|index| scoped_value(arguments, index).or_else(|| scoped_item(arguments, index))) {{
        return __rawr_internal::scope::with::<__rawr_internal::scope::RegionF, _>(scoped.nonce(), |region| -> Result<Dynamic, Error> {{"
        )
        .map_err(encoding)?;
        {
            let _region = InRegion::enter();
            let _arguments = RegionArguments::enter();
            render_adapter_body(plan, cohort, profile, facade, output)?;
        }
        output
            .write_all(
                b"        })
        .map_err(|error| Error::Lent(scoped.name(), error))?;
    }
",
            )
            .map_err(encoding)?;
    }
    render_adapter_body(plan, cohort, profile, facade, output)?;
    output.write_all(b"}\n\n").map_err(encoding)
}

/// The inputs of a call that are `&T` or `&mut T` of a `'static` type
/// another crate declares, which native code may have lent a marked
/// function (D558.3), as ratatui's `render_stateful_widget` takes `&mut
/// ListState`: each one's argument index, whether it is `&mut`, and its
/// Rust type.
fn lent_inputs(plan: &Plan) -> Result<Vec<(usize, bool, String)>, request::Error> {
    let Some((signature, substitutions, offset)) = call_signature(plan) else {
        return Ok(Vec::new());
    };
    // A scope frame's call takes what it borrows into the frame.
    if signature.scope().is_some() {
        return Ok(Vec::new());
    }
    let mut lent = Vec::new();
    for (index, input) in runtime_inputs(signature, offset) {
        let convert::Type::Borrowed { mutable, ty } = input else {
            continue;
        };
        if is_copied(ty) {
            continue;
        }
        let ty = substituted(ty, signature, substitutions)?;
        if !matches!(ty, convert::Type::Nominal { .. }) || carries_lifetime(&ty) || is_value(&ty) {
            continue;
        }
        if let Some(rust) = concrete_type(&ty) {
            lent.push((index, *mutable, rust));
        }
    }
    Ok(lent)
}

/// The inputs a call takes by value whose type carries a lifetime, so a
/// value in a scope frame's region may fill them (D558 tier 1), as
/// `push_html`'s `I` is a `Parser`: each one's argument index.
fn region_inputs(plan: &Plan) -> Result<Vec<usize>, request::Error> {
    let Some((signature, substitutions, offset)) = call_signature(plan) else {
        return Ok(Vec::new());
    };
    if signature.scope().is_some() {
        return Ok(Vec::new());
    }
    let mut region = Vec::new();
    for (index, input) in runtime_inputs(signature, offset) {
        if matches!(input, convert::Type::Borrowed { .. }) {
            continue;
        }
        let ty = substituted(input, signature, substitutions)?;
        // A value of a region, or an array of them, as `Column::
        // with_children` takes `column![..]`'s elements.
        let element = match &ty {
            convert::Type::Sequence {
                element,
                owned: true,
                length: None,
            } => element,
            ty => ty,
        };
        if family_of(element).is_some() {
            region.push(index);
        }
    }
    Ok(region)
}

thread_local! {
    /// While a call that makes a value whose type holds an opaque type is
    /// rendered: the expression of the table of calls that take it.
    static OPAQUE_PRODUCING: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
    /// While a call that takes such a value is rendered inside the call
    /// that made it: a value of the same type it gives takes `table`.
    static OPAQUE_CONSUMING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Sets `OPAQUE_PRODUCING` for as long as it lives.
struct OpaqueProducing(Option<String>);

impl OpaqueProducing {
    fn enter(table: Option<String>) -> Self {
        Self(OPAQUE_PRODUCING.replace(table))
    }
}

impl Drop for OpaqueProducing {
    fn drop(&mut self) {
        OPAQUE_PRODUCING.set(self.0.take());
    }
}

/// Sets `OPAQUE_CONSUMING` for as long as it lives.
struct OpaqueConsuming(bool);

impl OpaqueConsuming {
    fn enter(consuming: bool) -> Self {
        Self(OPAQUE_CONSUMING.replace(consuming))
    }
}

impl Drop for OpaqueConsuming {
    fn drop(&mut self) {
        OPAQUE_CONSUMING.set(self.0);
    }
}

/// The first opaque type `ty` holds, as its own type node.
fn first_opaque(ty: &convert::Type) -> Option<&convert::Type> {
    match ty {
        convert::Type::Nominal { definition, .. } if is_opaque(definition) => Some(ty),
        convert::Type::Nominal { arguments, .. } | convert::Type::Tuple(arguments) => {
            arguments.iter().find_map(first_opaque)
        }
        convert::Type::Option(inner)
        | convert::Type::Borrowed { ty: inner, .. }
        | convert::Type::Range(inner)
        | convert::Type::Sequence { element: inner, .. } => first_opaque(inner),
        convert::Type::Result(ok, err) => first_opaque(ok).or_else(|| first_opaque(err)),
        _ => None,
    }
}

/// The operand of `plan` whose type holds an opaque type, by its argument
/// index, with that opaque type: the receiver, or an input.
fn opaque_operand(plan: &Plan) -> Result<Option<(usize, convert::Type)>, request::Error> {
    let (signature, substitutions, receiver, functions) = match plan.operation() {
        Operation::Method {
            signature,
            substitutions,
            container_parameters,
            functions,
            ..
        } => {
            // A trait's method as a member of the opaque type itself, as
            // `Widget::_boxed` of what `iced(..)` gives.
            if let Some(instance) = signature.instance() {
                let instance = substituted(instance, signature, substitutions)?;
                if let Some(opaque) = first_opaque(&instance) {
                    return Ok(Some((0, opaque.clone())));
                }
            }
            if let Some(opaque) = substitutions
                .iter()
                .take(*container_parameters)
                .find_map(first_opaque)
            {
                return Ok(Some((0, opaque.clone())));
            }
            (signature, substitutions, 1, functions)
        }
        Operation::Function {
            signature,
            substitutions,
            functions,
            ..
        } => (signature, substitutions, 0, functions),
        _ => return Ok(None),
    };
    let mut runtime = receiver;
    for (position, input) in signature.inputs().iter().enumerate() {
        if functions
            .iter()
            .any(|function| usize::try_from(function.position()).ok() == Some(position))
        {
            continue;
        }
        let ty = substituted(input, signature, substitutions)?;
        if let Some(opaque) = first_opaque(&ty) {
            return Ok(Some((runtime, opaque.clone())));
        }
        runtime += 1;
    }
    Ok(None)
}

/// The calls in `cohort` that take a value holding the opaque type `key`,
/// in a fixed order: each one's index in the table such a value carries.
fn opaque_consumers<'p>(
    cohort: &'p [Plan],
    key: &convert::Type,
) -> Result<Vec<&'p Plan>, request::Error> {
    let mut consumers = Vec::new();
    for plan in cohort {
        if opaque_operand(plan)?.is_some_and(|(_, opaque)| opaque == *key) {
            consumers.push(plan);
        }
    }
    Ok(consumers)
}

/// The table of calls that take the value `plan` gives, when its type holds
/// an opaque type: each call's body is compiled here, where rustc knows the
/// value's type, and takes it at that type (`take_like`).
fn opaque_table_source(
    plan: &Plan,
    cohort: &[Plan],
    profile: &crate::profile::Proof,
    facade: Option<&crate::surface::facade::Plan>,
) -> Result<Option<String>, request::Error> {
    let returned = match plan.operation() {
        Operation::Function {
            signature,
            substitutions,
            ..
        }
        | Operation::Method {
            signature,
            substitutions,
            ..
        } => substituted(signature.output(), signature, substitutions)?,
        _ => return Ok(None),
    };
    let Some(key) = first_opaque(&returned).cloned() else {
        return Ok(None);
    };
    let mut entries = Vec::new();
    for consumer in opaque_consumers(cohort, &key)? {
        let Some((position, _)) = opaque_operand(consumer)? else {
            continue;
        };
        // A call that gives a value of another opaque type, as iced's
        // `Application::subscription` gives another `impl Program`, makes
        // that value's own table here, where rustc knows its type. One that
        // gives a value of the same type passes this table on.
        let produced = match consumer.operation() {
            Operation::Function {
                signature,
                substitutions,
                ..
            }
            | Operation::Method {
                signature,
                substitutions,
                ..
            } => substituted(signature.output(), signature, substitutions)?,
            _ => convert::Type::Unit,
        };
        let other = first_opaque(&produced).is_some_and(|produced| *produced != key);
        let inner = if other {
            opaque_table_source(consumer, cohort, profile, facade)?
        } else {
            None
        };
        let body = {
            let _producing = OpaqueProducing::enter(inner);
            let _consuming = OpaqueConsuming::enter(!other);
            let mut body = Vec::new();
            let region = region_inputs(consumer)?;
            if !region.is_empty() {
                let indices = region
                    .iter()
                    .map(usize::to_string)
                    .collect::<Vec<_>>()
                    .join(", ");
                writeln!(
                    body,
                    "    if let Some(scoped) = [{indices}].into_iter().find_map(|index| scoped_value(arguments, index).or_else(|| scoped_item(arguments, index))) {{
        return __rawr_internal::scope::with::<__rawr_internal::scope::RegionF, _>(scoped.nonce(), |region| -> Result<Dynamic, Error> {{"
                )
                .map_err(|_| request::Error::Encoding)?;
                {
                    let _region = InRegion::enter();
                    let _arguments = RegionArguments::enter();
                    render_adapter_body(consumer, cohort, profile, facade, &mut body)?;
                }
                body.write_all(
                    b"        })
        .map_err(|error| Error::Lent(scoped.name(), error))?;
    }
",
                )
                .map_err(|_| request::Error::Encoding)?;
            }
            render_adapter_body(consumer, cohort, profile, facade, &mut body)?;
            String::from_utf8(body).map_err(|_| request::Error::Encoding)?
        };
        // The operand is taken at the type of the value this call made.
        let body = body.replace(
            &format!("take_native(arguments, {position})"),
            &format!("take_like(arguments, {position}, _w)"),
        );
        entries.push(format!(
            "|arguments: &mut Arguments<'_, '_, '_>, table: OpaqueTable, _w| -> Result<Dynamic, Error> {{\n    let _ = (&table, &_w);\n{body}\n}}"
        ));
    }
    Ok(Some(format!(
        "{{
        static TABLE: std::sync::OnceLock<Box<[OpaqueEntry]>> = std::sync::OnceLock::new();
        OpaqueTable(TABLE.get_or_init(|| opaque_table(&__RAWR_VALUE__, [{}])))
    }}",
        entries.join(",\n")
    )))
}

/// The signature, substitutions and argument offset of a call: a
/// function's, or a method's after its receiver.
fn call_signature(
    plan: &Plan,
) -> Option<(
    &crate::classify::representability::signature::Signature,
    &[convert::Type],
    usize,
)> {
    match plan.operation() {
        Operation::Function {
            signature,
            substitutions,
            mappers,
            call: None,
            ..
        } if mappers.is_empty() => Some((signature, substitutions, 0)),
        Operation::Method {
            signature,
            substitutions,
            call: None,
            ..
        } => Some((signature, substitutions, 1)),
        _ => None,
    }
}

/// Each input a call takes at run time, with its argument index: a static
/// function item takes none.
fn runtime_inputs(
    signature: &crate::classify::representability::signature::Signature,
    offset: usize,
) -> impl Iterator<Item = (usize, &convert::Type)> {
    signature
        .inputs()
        .iter()
        .filter(|input| !matches!(input, convert::Type::FunctionItem { .. }))
        .enumerate()
        .map(move |(index, input)| (index + offset, input))
}

/// The body of `plan`'s adapter, which takes its arguments, calls, and
/// gives the output.
fn render_adapter_body(
    plan: &Plan,
    cohort: &[Plan],
    profile: &crate::profile::Proof,
    facade: Option<&crate::surface::facade::Plan>,
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    let path = rust_path(plan);
    let continuation = method_message(plan, cohort, profile, facade)?;
    let replay = match continuation {
        Some(message) => Some(Replay::Message(message)),
        None => replay(plan, profile)?,
    };
    match plan.operation() {
        Operation::Declaration { .. } => {
            writeln!(
                output,
                "    let _ = core::mem::size_of::<{path}>();\n    Ok(Dynamic::from(()))"
            )
            .map_err(|_| request::Error::Encoding)?;
        }
        Operation::StructConstructor { form, fields, .. } => {
            render_fields(fields, 0, output)?;
            let ty = structural_type(plan);
            write!(output, "    let output: {ty} = {path}")
                .map_err(|_| request::Error::Encoding)?;
            render_constructor(*form, fields, output)?;
            output
                .write_all(b";\n    insert_native!(arguments.arena, output).map(Dynamic::from)\n")
                .map_err(|_| request::Error::Encoding)?;
        }
        Operation::StructField { field, .. } => {
            let path = structural_type(plan);
            // A field of a value in a scope frame's region (D558 tier 1), as
            // quick-xml's `Attribute::value`: a copy, which stays in the
            // region when it carries the region's lifetime.
            if let Some(family) = container_family(&path, &path) {
                let converted = {
                    let _region = InRegion::enter();
                    let mut converted = String::new();
                    render_to_dynamic(field.ty(), "output", 0, &mut converted)?;
                    converted
                };
                writeln!(
                    output,
                    "    if let Some(scoped) = scoped_value(arguments, 0) {{
        return __rawr_internal::scope::with::<__rawr_internal::scope::RegionF, _>(scoped.nonce(), |region| -> Result<Dynamic, Error> {{
            let output = Clone::clone(&region.get::<{family}>(scoped).map_err(|error| Error::Lent(scoped.name(), error))?.{});
            {converted}
        }})
        .map_err(|error| Error::Lent(scoped.name(), error))?;
    }}",
                    field.name()
                )
                .map_err(|_| request::Error::Encoding)?;
            }
            if copy_scalar(field.ty()) {
                let mut converted = String::new();
                render_to_dynamic(field.ty(), "output", 0, &mut converted)?;
                writeln!(output, "    if let Some(lent) = lent_value(arguments, 0) {{
        let access: fn(u64, &mut dyn FnMut(&{path})) -> Result<(), u8> = lent
            .accessor(false).map_err(|error| Error::Lent(lent.name(), error))?;
        let mut output = None;
        access(lent.nonce().bits(), &mut |receiver| output = Some(receiver.{}))
            .map_err(|code| Error::Lent(lent.name(), __rawr_internal::scope::Error::from_code(code)))?;
        let output = output.ok_or(Error::Lent(lent.name(), __rawr_internal::scope::Error::Busy))?;
        return {converted};
    }}", field.name()).map_err(|_| request::Error::Encoding)?;
                output
                    .write_all(b"    let handle = native_handle(arguments, 0, false)?;\n")
                    .map_err(|_| request::Error::Encoding)?;
                writeln!(
                    output,
                    "    let output = arguments.arena.shared::<{path}, _>(handle, |input| input.{})?;",
                    field.name()
                )
                .map_err(|_| request::Error::Encoding)?;
            } else {
                writeln!(
                    output,
                    "    let input: {path} = take_native(arguments, 0)?;"
                )
                .map_err(|_| request::Error::Encoding)?;
                writeln!(output, "    let output = input.{};", field.name())
                    .map_err(|_| request::Error::Encoding)?;
            }
            render_give(field.ty(), "output", output)?;
        }
        Operation::EnumVariant {
            name, form, fields, ..
        } => {
            render_fields(fields, 0, output)?;
            write!(output, "    let output = {path}::{name}")
                .map_err(|_| request::Error::Encoding)?;
            render_constructor(*form, fields, output)?;
            output
                .write_all(b";\n    insert_native!(arguments.arena, output).map(Dynamic::from)\n")
                .map_err(|_| request::Error::Encoding)?;
        }
        Operation::Function {
            signature,
            substitutions,
            constants,
            functions,
            mappers,
            container_parameters,
            associated,
            call,
            slots,
            crossings,
        } => {
            if !mappers.is_empty() && replay.is_none() {
                return Err(request::Error::Drift {
                    field: "profile event mapper invocation lacks replay projection",
                });
            }
            let crossed = Crossed { slots, crossings };
            let opens = instance_opens(signature, substitutions);
            // A `&str` the output borrows through a lifetime its type only
            // carries, as iced's `text(url)` gives a `Text<'a>` of an `impl
            // IntoFragment<'a>`: pinned in a slot's region, as an opening's
            // strings are, and out of any region for the runner's life.
            let pins = !opens && replay.is_none() && lends_str_to_output(signature, substitutions)?;
            let _pins = PinnedStrings::enter(pins);
            if (opens || pins) && replay.is_none() {
                render_anchored_opening(
                    plan,
                    signature,
                    substitutions,
                    constants,
                    functions,
                    mappers,
                    *container_parameters,
                    *associated,
                    output,
                )?;
            }
            if replay.is_some() {
                render_function_arguments(
                    signature,
                    substitutions,
                    functions,
                    mappers,
                    Some(profile),
                    call.as_ref(),
                    crossed,
                    0,
                    output,
                )?;
            } else {
                render_function_arguments(
                    signature,
                    substitutions,
                    functions,
                    mappers,
                    None,
                    call.as_ref(),
                    crossed,
                    0,
                    output,
                )?;
            }
            if opens {
                render_opening(
                    plan,
                    signature,
                    substitutions,
                    constants,
                    functions,
                    mappers,
                    *container_parameters,
                    *associated,
                    output,
                )?;
            } else if let Some(replay) = replay {
                output
                    .write_all(b"    let output = __rawr_profile::Replay::new(move |_context| ")
                    .map_err(|_| request::Error::Encoding)?;
                render_function_path(
                    plan,
                    signature,
                    substitutions,
                    constants,
                    *container_parameters,
                    *associated,
                    output,
                )?;
                render_item_turbofish(
                    signature,
                    substitutions,
                    constants,
                    *container_parameters,
                    output,
                )?;
                output
                    .write_all(b"(")
                    .map_err(|_| request::Error::Encoding)?;
                function_argument_list(
                    signature,
                    substitutions,
                    functions,
                    mappers,
                    Some(profile),
                    crossed,
                    output,
                )?;
                output
                    .write_all(b"));\n")
                    .map_err(|_| request::Error::Encoding)?;
                render_replay_insert(replay, facade, output)?;
            } else {
                // A Rust future is made when its task first runs, on
                // iced's executor, as tokio's `sleep` needs.
                let lazy = matches!(
                    substitute(signature.output(), signature, substitutions)?,
                    convert::Type::Nominal { definition, .. }
                        if *definition == crate::surface::identity::Definition::future()
                );
                let mut call = Vec::new();
                render_function_path(
                    plan,
                    signature,
                    substitutions,
                    constants,
                    *container_parameters,
                    *associated,
                    &mut call,
                )?;
                render_item_turbofish(
                    signature,
                    substitutions,
                    constants,
                    *container_parameters,
                    &mut call,
                )?;
                call.write_all(b"(").map_err(|_| request::Error::Encoding)?;
                function_argument_list(
                    signature,
                    substitutions,
                    functions,
                    mappers,
                    None,
                    crossed,
                    &mut call,
                )?;
                call.write_all(b")").map_err(|_| request::Error::Encoding)?;
                let call = String::from_utf8(call).map_err(|_| request::Error::Encoding)?;
                let returned = &substituted(signature.output(), signature, substitutions)?;
                if !lazy && !lifetime_inputs(signature, substitutions)? {
                    render_anchored_output(returned, &call, output)?;
                }
                // A native call lends the arena to the script closures it
                // calls back, as Freya's `launch` calls `fn app()` (D551.6).
                if lazy {
                    writeln!(
                        output,
                        "    let output = __rawr_profile_host::task::lazy(move || {call});"
                    )
                } else {
                    writeln!(output, "    let output = lend(arguments.arena, || {call});")
                }
                .map_err(|_| request::Error::Encoding)?;
                if let Some((sipped, progress)) = sipper_output(returned) {
                    render_give_sipper(sipped, progress, "output", output)?;
                } else if let Some(element) = facade_task_output(returned, facade, profile) {
                    render_give_task(element, "output", output)?;
                } else {
                    render_give_with(signature, substitutions, "output", output)?;
                }
            }
        }
        Operation::Method {
            receiver,
            signature,
            substitutions,
            constants,
            container_parameters,
            ..
        } if signature.scope().is_some() => {
            render_arguments_with(signature, substitutions, 1, output)?;
            render_method_opening(
                plan,
                &path,
                *receiver,
                signature,
                substitutions,
                constants,
                *container_parameters,
                output,
            )?;
        }
        Operation::Method {
            receiver,
            signature,
            substitutions,
            constants,
            container_parameters,
            call,
            functions,
            slots,
            crossings,
        } => {
            if matches!(receiver, Receiver::Owned) && replay.is_some() {
                render_arguments_with_profile(signature, substitutions, 1, profile, output)?;
            } else if call.is_some() || !slots.is_empty() {
                // A script closure a method takes, as Freya's `on_press`
                // does (D551.6), a crate function in its place, or a script
                // function as a slot (D594), as iced's `Application::
                // subscription(App::subscription)` takes it.
                // A slot lends only script values by reference. A callable
                // whose inputs borrow a native value, as a style's `&Theme`,
                // crosses as the closure Rawr makes, as it did before
                // methods took slots.
                let lends = |crossing: &crate::classify::representability::callable::Call| {
                    crossing.inputs().iter().all(|input| {
                        match substituted(input, signature, substitutions) {
                            Ok(convert::Type::Borrowed { ty, .. }) => {
                                matches!(*ty, convert::Type::Script { .. })
                            }
                            Ok(_) => true,
                            Err(_) => false,
                        }
                    })
                };
                let crossed = if call.is_some() && !crossings.iter().all(lends) {
                    Crossed::default()
                } else {
                    Crossed { slots, crossings }
                };
                render_function_arguments(
                    signature,
                    substitutions,
                    functions,
                    &[],
                    None,
                    call.as_ref(),
                    crossed,
                    1,
                    output,
                )?;
                for (position, ty) in signature.inputs().iter().enumerate() {
                    if crossed.slot_at(position, ty).is_some() {
                        writeln!(output, "    let argument_{position} = slot_{position};")
                            .map_err(|_| request::Error::Encoding)?;
                    }
                }
            } else {
                render_arguments_with(signature, substitutions, 1, output)?;
            }
            if let Some(convert::Type::ProfileMessage { mapper, payload }) = continuation {
                output
                    .write_all(b"    __rawr_profile::map_projectable::<")
                    .map_err(|_| request::Error::Encoding)?;
                render_container_path(
                    &path,
                    signature,
                    substitutions,
                    constants,
                    *container_parameters,
                    true,
                    output,
                )?;
                output
                    .write_all(b", ")
                    .map_err(|_| request::Error::Encoding)?;
                render_rust_type(
                    payload,
                    facade.ok_or(request::Error::Drift {
                        field: "event continuation facade",
                    })?,
                    output,
                )?;
                output
                    .write_all(b">(arguments, &")
                    .map_err(|_| request::Error::Encoding)?;
                byte_slice(output, mapper)?;
                write!(
                    output,
                    ", move |receiver, _context| receiver.{}",
                    plan.name()
                )
                .map_err(|_| request::Error::Encoding)?;
                render_item_turbofish(
                    signature,
                    substitutions,
                    constants,
                    *container_parameters,
                    output,
                )?;
                output
                    .write_all(b"(")
                    .map_err(|_| request::Error::Encoding)?;
                replay_argument_list(signature, substitutions, profile, output)?;
                output
                    .write_all(b"))\n")
                    .map_err(|_| request::Error::Encoding)?;
            } else if let (Receiver::Owned, Some(replay)) = (receiver, replay) {
                output
                    .write_all(b"    let receiver: __rawr_profile::Replay<")
                    .map_err(|_| request::Error::Encoding)?;
                render_container_path(
                    &path,
                    signature,
                    substitutions,
                    constants,
                    *container_parameters,
                    true,
                    output,
                )?;
                output
                    .write_all(b"> = take_native(arguments, 0)?;\n")
                    .map_err(|_| request::Error::Encoding)?;
                write!(
                    output,
                    "    let output = __rawr_profile::Replay::new(move |_context| receiver.run(_context).{}",
                    plan.name()
                )
                .map_err(|_| request::Error::Encoding)?;
                render_item_turbofish(
                    signature,
                    substitutions,
                    constants,
                    *container_parameters,
                    output,
                )?;
                output
                    .write_all(b"(")
                    .map_err(|_| request::Error::Encoding)?;
                replay_argument_list(signature, substitutions, profile, output)?;
                output
                    .write_all(b"));\n")
                    .map_err(|_| request::Error::Encoding)?;
                render_replay_insert(replay, facade, output)?;
            } else if !matches!(receiver, Receiver::Owned)
                && is_future(substitute(signature.output(), signature, substitutions)?)
            {
                // An async method that borrows its receiver (D513.6): its
                // future holds a copy of a `Clone` receiver for `&self`, or
                // the receiver lent from its slot, and is made when first
                // polled, where its runtime is.
                writeln!(
                    output,
                    "    let receiver = native_handle(arguments, 0, false)?;"
                )
                .map_err(|_| request::Error::Encoding)?;
                output
                    .write_all(b"    let held: Held<")
                    .map_err(|_| request::Error::Encoding)?;
                render_container_path(
                    &path,
                    signature,
                    substitutions,
                    constants,
                    *container_parameters,
                    true,
                    output,
                )?;
                if matches!(receiver, Receiver::Mutable) {
                    output
                        .write_all(
                            b"> = arguments.arena.lease(receiver)?;
",
                        )
                        .map_err(|_| request::Error::Encoding)?;
                } else {
                    output
                        .write_all(
                            b"> = {
        #[allow(unused_imports)]
        use {HoldClone as _, HoldLease as _};
        (&HoldProbe::new()).hold(arguments.arena, receiver)?
    };
",
                        )
                        .map_err(|_| request::Error::Encoding)?;
                }
                write!(
                    output,
                    "    let output = __rawr_profile_host::task::lazy(move || async move {{
        let mut held = held;
        held.get_mut().{}",
                    plan.name()
                )
                .map_err(|_| request::Error::Encoding)?;
                render_item_turbofish(
                    signature,
                    substitutions,
                    constants,
                    *container_parameters,
                    output,
                )?;
                output
                    .write_all(b"(")
                    .map_err(|_| request::Error::Encoding)?;
                argument_list(signature.inputs().len(), 1, output)?;
                output
                    .write_all(b").await\n    });\n")
                    .map_err(|_| request::Error::Encoding)?;
                let returned = &substituted(signature.output(), signature, substitutions)?;
                let element =
                    facade_task_output(returned, facade, profile).ok_or(request::Error::Drift {
                        field: "borrowed async method output",
                    })?;
                render_give_task(element, "output", output)?;
            } else if let Some(guard) = signature.guard()
                && !matches!(receiver, Receiver::Owned)
            {
                render_guard(
                    plan,
                    &path,
                    *receiver,
                    guard,
                    signature,
                    substitutions,
                    constants,
                    *container_parameters,
                    output,
                )?;
            } else {
                match receiver {
                    // A trait's method of its impl for `&str`, called on
                    // Rawr's string (`surface::text`): the string, pinned
                    // for the runner's life as a `&'static str` input is.
                    Receiver::Owned
                        if signature.instance()
                            == Some(&convert::Type::Builtin(convert::Builtin::StaticStr)) =>
                    {
                        output
                            .write_all(
                                b"    let receiver: &'static str = static_pins().pin(take_primitive::<ImmutableString>(arguments, 0)?);\n",
                            )
                            .map_err(|_| request::Error::Encoding)?;
                        let annotation = concrete_type(&substituted(
                            signature.output(),
                            signature,
                            substitutions,
                        )?)
                        .map(|ty| format!(": {ty}"))
                        .unwrap_or_default();
                        write!(
                            output,
                            "    let output{annotation} = lend(arguments.arena, || receiver.{}",
                            plan.name()
                        )
                        .map_err(|_| request::Error::Encoding)?;
                        render_item_turbofish(
                            signature,
                            substitutions,
                            constants,
                            *container_parameters,
                            output,
                        )?;
                        output
                            .write_all(b"(")
                            .map_err(|_| request::Error::Encoding)?;
                        argument_list(signature.inputs().len(), 1, output)?;
                        output
                            .write_all(b"));\n")
                            .map_err(|_| request::Error::Encoding)?;
                    }
                    Receiver::Owned => {
                        render_scoped_receiver(
                            plan,
                            &path,
                            *receiver,
                            signature,
                            substitutions,
                            constants,
                            *container_parameters,
                            output,
                        )?;
                        // A `&str` lent to a receiver out of any region,
                        // whose output keeps it, has no lifetime to borrow
                        // at but `'static`: such a call needs the region a
                        // view's widgets are made in.
                        let returned = substituted(signature.output(), signature, substitutions)?;
                        if carries_lifetime(&returned)
                            && signature
                                .inputs()
                                .iter()
                                .any(|input| lends_str(input, signature, substitutions))
                        {
                            writeln!(
                                output,
                                "    Err(Error::Lent({:?}, __rawr_internal::scope::Error::Mismatch))",
                                format!("{}::{}", rust_path(plan), plan.name())
                            )
                            .map_err(|_| request::Error::Encoding)?;
                            return Ok(());
                        }
                        output
                            .write_all(b"    let receiver: ")
                            .map_err(|_| request::Error::Encoding)?;
                        render_container_path(
                            &path,
                            signature,
                            substitutions,
                            constants,
                            *container_parameters,
                            true,
                            output,
                        )?;
                        output
                            .write_all(b" = take_native(arguments, 0)?;\n")
                            .map_err(|_| request::Error::Encoding)?;
                        // An implementation's parameter the call infers, as
                        // `Renderer` of iced's `Widget` for `Button`, is
                        // named by the output's type.
                        let inferred = signature.parameters().iter().any(|parameter| {
                            parameter.is_argument_impl()
                                && output_names(signature.output(), parameter.index())
                        });
                        let annotation = if inferred {
                            concrete_type(&substituted(
                                signature.output(),
                                signature,
                                substitutions,
                            )?)
                            .map(|ty| format!(": {ty}"))
                            .unwrap_or_default()
                        } else {
                            String::new()
                        };
                        // The arena is lent while the method runs, as for a
                        // function (D551.6). A proof-backed application
                        // calls its isolated helper (`render_applications`).
                        if plan.application_invocation().is_some() {
                            write!(
                                output,
                                "    let output{annotation} = lend(arguments.arena, || rawr_applications::{}::call(receiver{}",
                                identity_name("application", plan),
                                if signature.inputs().is_empty() { "" } else { ", " },
                            )
                            .map_err(|_| request::Error::Encoding)?;
                        } else {
                            write!(
                                output,
                                "    let output{annotation} = lend(arguments.arena, || receiver.{}",
                                plan.name()
                            )
                            .map_err(|_| request::Error::Encoding)?;
                            render_item_turbofish(
                                signature,
                                substitutions,
                                constants,
                                *container_parameters,
                                output,
                            )?;
                            output
                                .write_all(b"(")
                                .map_err(|_| request::Error::Encoding)?;
                        }
                        argument_list(signature.inputs().len(), 1, output)?;
                        output
                            .write_all(b"));\n")
                            .map_err(|_| request::Error::Encoding)?;
                    }
                    Receiver::Shared | Receiver::Mutable => {
                        render_borrowed_receiver(
                            plan,
                            &path,
                            *receiver,
                            signature,
                            substitutions,
                            constants,
                            *container_parameters,
                            output,
                        )?;
                    }
                }
                // A method that takes `self` by value may give a Rust
                // future, as `Response::text` does.
                let returned = &substituted(signature.output(), signature, substitutions)?;
                match facade_task_output(returned, facade, profile) {
                    Some(element) if matches!(receiver, Receiver::Owned) => {
                        render_give_task(element, "output", output)?;
                    }
                    _ => render_give_with(signature, substitutions, "output", output)?,
                }
            }
        }
        Operation::Constant { ty, associated, .. } => {
            let constant = if *associated {
                format!("{path}::{}", plan.name())
            } else {
                path
            };
            writeln!(output, "    let output = {constant};")
                .map_err(|_| request::Error::Encoding)?;
            render_give(ty, "output", output)?;
        }
        Operation::Builder { steps } => {
            if let Some(replay) = replay {
                render_builder_inputs(steps, profile, output)?;
                output
                    .write_all(b"    let output = __rawr_profile::Replay::new(move |_context| {\n")
                    .map_err(|_| request::Error::Encoding)?;
                render_builder(steps, true, Some(profile), output)?;
                output
                    .write_all(b"        output\n    });\n")
                    .map_err(|_| request::Error::Encoding)?;
                render_replay_insert(replay, facade, output)?;
            } else {
                render_builder(steps, false, None, output)?;
            }
        }
    }
    Ok(())
}

/// Whether `input` is a `&str` the call borrows: written so, or a type
/// parameter the instance fills with `&str`, as a string literal fills
/// `button(content: W)` (Rust's `&'static str`).
fn lends_str(
    input: &convert::Type,
    signature: &crate::classify::representability::signature::Signature,
    substitutions: &[convert::Type],
) -> bool {
    match input {
        convert::Type::Builtin(convert::Builtin::Str) => true,
        convert::Type::Parameter(_) => matches!(
            substituted(input, signature, substitutions),
            Ok(convert::Type::Borrowed { mutable: false, ty })
                if matches!(*ty, convert::Type::Builtin(convert::Builtin::Str))
        ),
        _ => false,
    }
}

/// Whether the call's output borrows what it takes, so it opens a scope
/// frame (D558 tier 1): as its signature says, or because the instance
/// fills a type parameter the output names with a `&str`.
fn instance_opens(
    signature: &crate::classify::representability::signature::Signature,
    substitutions: &[convert::Type],
) -> bool {
    signature.scope().is_some()
        || signature.inputs().iter().any(|input| {
            matches!(input, convert::Type::Parameter(index)
                if output_names(signature.output(), *index)
                    && lends_str(input, signature, substitutions))
        })
}

/// A call whose output borrows its `&str` inputs (D558 tier 1), as
/// `Parser::new(&text)`: it gives a scope frame the `let` that binds it
/// opens. The frame owns the inputs the adapter took, makes the output,
/// keeps it in a region at the inputs' borrow, and runs the rest of the
/// scope with the output's handle.
#[allow(clippy::too_many_arguments)]
fn render_opening(
    plan: &Plan,
    signature: &crate::classify::representability::signature::Signature,
    substitutions: &[convert::Type],
    constants: &[convert::Constant],
    functions: &[crate::surface::adapter::FunctionItem],
    mappers: &[crate::surface::adapter::EventMapper],
    container_parameters: usize,
    associated: bool,
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    let encoding = |_| request::Error::Encoding;
    let mut call = Vec::new();
    render_function_path(
        plan,
        signature,
        substitutions,
        constants,
        container_parameters,
        associated,
        &mut call,
    )?;
    render_item_turbofish(
        signature,
        substitutions,
        constants,
        container_parameters,
        &mut call,
    )?;
    call.write_all(b"(").map_err(encoding)?;
    function_argument_list(
        signature,
        substitutions,
        functions,
        mappers,
        None,
        Crossed::default(),
        &mut call,
    )?;
    call.write_all(b")").map_err(encoding)?;
    let call = String::from_utf8(call).map_err(|_| request::Error::Encoding)?;
    let returned = substituted(signature.output(), signature, substitutions)?;
    let converted = {
        let _region = InRegion::enter();
        let mut converted = String::new();
        render_to_dynamic(&returned, "made", 0, &mut converted)?;
        converted
    };
    let name = rust_path(plan);
    // The frame owns each `&str` input's `String` and each copied array,
    // which the call borrows.
    let FrameCopies {
        rebound,
        before,
        back,
        with,
    } = frame_copies(signature, substitutions)?;
    // A frame is a script value, so what it holds is `Send` and `Sync`. A
    // native input stays in its arena slot by its handle until the frame
    // opens, on the thread that holds the arena, and a value another
    // region holds can't be parked so: such a call needs a view's region.
    let mut parked = String::new();
    let mut unparked = String::new();
    let mut selected = functions
        .iter()
        .map(|function| function.position())
        .peekable();
    for (position, input) in signature.inputs().iter().enumerate() {
        if selected
            .peek()
            .is_some_and(|at| usize::try_from(*at).ok() == Some(position))
        {
            selected.next();
            continue;
        }
        let ty = substituted(input, signature, substitutions)?;
        let native = matches!(ty, convert::Type::Nominal { .. })
            && !is_value(&ty)
            && !is_duration(&ty)
            && !is_facade_value(&ty)
            && !copy_scalar(&ty);
        if !native {
            continue;
        }
        if REGION_ARGUMENTS.get() || carries_lifetime(&ty) {
            return writeln!(
                output,
                "    let _ = argument_{position};\n    Err(Error::Lent({name:?}, __rawr_internal::scope::Error::Mismatch))"
            )
            .map_err(encoding);
        }
        parked.push_str(&format!(
            "let argument_{position} = arguments.arena.insert(argument_{position})?;\n    "
        ));
        unparked.push_str(&format!(
            "\n        let argument_{position} = with_arena(|arena| arena.take(argument_{position})).map_err(runtime_error)?;"
        ));
    }
    let before = format!("{parked}{before}");
    let rebound = format!("{unparked}{rebound}");
    writeln!(
        output,
        "    {before}let output = __rawr_internal::scope::Opening::new({name:?}, move |rest| {{{rebound}
        let made = {call};
        let mut region = __rawr_internal::scope::Region::new();
        let value = with_arena(|arena| {{
            let mut values: [&mut Dynamic; 0] = [];
            #[allow(unused_variables)]
            let arguments = &mut Arguments {{ values: &mut values, arena }};
            {converted}
        }})
        .map_err(runtime_error)?;
        let result = __rawr_internal::scope::open(&mut region, || rest(value));{back}
        result
    }}){with};
    Ok(Dynamic::from(output))"
    )
    .map_err(encoding)
}

/// A call whose output borrows its `&str` inputs, made while a slot's
/// region is open (D594): `view(&state)`'s `text(&state.label)`. The output
/// goes in that region, whose lifetime is the lent value's, and each string
/// it borrows is pinned in the lent value, so the slot gives native code
/// the output at that lifetime. Pinning holds the string's shared handle,
/// so no text is copied. Elsewhere the call opens a frame of its own
/// (`render_opening`).
#[allow(clippy::too_many_arguments)]
fn render_anchored_opening(
    plan: &Plan,
    signature: &crate::classify::representability::signature::Signature,
    substitutions: &[convert::Type],
    constants: &[convert::Constant],
    functions: &[crate::surface::adapter::FunctionItem],
    mappers: &[crate::surface::adapter::EventMapper],
    container_parameters: usize,
    associated: bool,
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    let encoding = |_| request::Error::Encoding;
    // A script closure, as `table::column(header, |event| ..)` takes with
    // its `&str` header, is made by the call's own slot, outside this path.
    if !functions.is_empty()
        || !mappers.is_empty()
        || signature.parameters().iter().any(|parameter| {
            parameter.kind() == crate::classify::representability::signature::Kind::Callable
        })
    {
        return Ok(());
    }
    let mut call = Vec::new();
    render_function_path(
        plan,
        signature,
        substitutions,
        constants,
        container_parameters,
        associated,
        &mut call,
    )?;
    render_item_turbofish(
        signature,
        substitutions,
        constants,
        container_parameters,
        &mut call,
    )?;
    call.write_all(b"(").map_err(encoding)?;
    function_argument_list(
        signature,
        substitutions,
        functions,
        mappers,
        None,
        Crossed::default(),
        &mut call,
    )?;
    call.write_all(b")").map_err(encoding)?;
    let call = String::from_utf8(call).map_err(|_| request::Error::Encoding)?;
    let returned = substituted(signature.output(), signature, substitutions)?;
    let converted = {
        let _region = InRegion::enter();
        let mut converted = String::new();
        render_to_dynamic(&returned, "made", 0, &mut converted)?;
        converted
    };
    let mut taken = Vec::new();
    let mut pinned = String::new();
    for (position, input) in signature.inputs().iter().enumerate() {
        if lends_str(input, signature, substitutions) {
            writeln!(
                taken,
                "        let argument_{position}_text = (**arguments.values.get_mut({position}).ok_or(Error::MissingArgument)?).take().into_immutable_string().map_err(|_| Error::TypeMismatch)?;"
            )
            .map_err(encoding)?;
            pinned.push_str(&format!(
                "\n            let argument_{position} = pins.pin(argument_{position}_text);"
            ));
        } else {
            render_take_input(
                position,
                position,
                input,
                signature,
                substitutions,
                None,
                &mut taken,
            )?;
        }
    }
    let taken = String::from_utf8(taken).map_err(|_| request::Error::Encoding)?;
    // An input that carries a lifetime, as a widget `view` made, is in a
    // region. In the slot's own region, which pins strings, the output
    // goes there too, at the lifetime the input has.
    if REGION_ARGUMENTS.get() {
        return writeln!(
            output,
            "    if let Some(pins) = region.pins() {{
{taken}        let _ = &pins;{pinned}
        let made = {call};
        return {converted};
    }}"
        )
        .map_err(encoding);
    }
    // Out of any region, such an input is at `'static`, which a string
    // pinned at the slot's lifetime can't join.
    if lifetime_inputs(signature, substitutions)? {
        return Ok(());
    }
    writeln!(
        output,
        "    if let Some(anchor) = __rawr_internal::scope::anchor() {{
{taken}        return __rawr_internal::scope::with::<__rawr_internal::scope::RegionF, _>(anchor, |region| -> Result<Dynamic, Error> {{
            let pins = region.pins().ok_or(Error::Lent(\"a slot's region\", __rawr_internal::scope::Error::Mismatch))?;{pinned}
            let made = {call};
            {converted}
        }})
        .map_err(|error| Error::Lent(\"a slot's region\", error))?;
    }}"
    )
    .map_err(encoding)
}

/// `register_defaults`, which gives the core each imported type's own
/// `Default`, so a script type's derived `Default` builds a field of that
/// type as Rust's derive does: `Tooltip { position: Position::default(),
/// .. }`. Only types with no parameters or lifetimes, whose `Default` the
/// crate implements for the type itself.
fn render_imported_defaults(
    facade: Option<&crate::surface::facade::Plan>,
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    let encoding = |_| request::Error::Encoding;
    output
        .write_all(b"fn register_defaults() {\n")
        .map_err(encoding)?;
    let mut seen = std::collections::BTreeSet::new();
    for owner in facade
        .into_iter()
        .flat_map(crate::surface::facade::Plan::facades)
    {
        for ty in owner.types() {
            if !ty.default()
                || ty.parameters().len() != 0
                || ty.lifetimes() != 0
                || !seen.insert(*ty.identity())
            {
                continue;
            }
            // The type's Rust spelling, which a macro's `$crate` path isn't.
            let Some(path) = TYPE_PATHS.with_borrow(|paths| paths.get(ty.identity()).cloned())
            else {
                continue;
            };
            if path.contains(crate::api::ledger::CRATE_SEGMENT) {
                continue;
            }
            let give = if FACADE_VALUES.with_borrow(|values| values.contains(ty.identity())) {
                "Ok(Dynamic::from(output))".to_owned()
            } else {
                let mut identity = Vec::new();
                byte_array(&mut identity, ty.identity())?;
                format!(
                    "let handle = insert_native!(arguments.arena, output)?;\n        arguments.arena.made_by_default(&handle, {});\n        Ok(Dynamic::from(handle))",
                    String::from_utf8(identity).map_err(|_| request::Error::Encoding)?
                )
            };
            output
                .write_all(b"    __rawr_internal::host::value::register_imported_default(")
                .map_err(encoding)?;
            byte_array(output, ty.identity())?;
            writeln!(
                output,
                ", || with_arena(|arena| {{
        let mut values: [&mut Dynamic; 0] = [];
        let arguments = &mut Arguments {{ values: &mut values, arena }};
        let output = <{path} as Default>::default();
        {give}
    }}).unwrap_or_else(|error| panic!(\"rawr: `{path}`'s `Default`: {{error:?}}\")));"
            )
            .map_err(encoding)?;
        }
    }
    output.write_all(b"}\n\n").map_err(encoding)
}

/// A call that gives a value carrying a lifetime, made while a slot's
/// region is open, as `view(&state)`'s `checkbox(state.done)`: the call is
/// made at that region's lifetime and its output stays there, so a widget
/// invariant in its lifetime joins the strings and widgets pinned there,
/// as rustc infers one lifetime for the whole view.
fn render_anchored_output(
    returned: &convert::Type,
    call: &str,
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    if REGION_ARGUMENTS.get() || !matches!(returned, convert::Type::Nominal { .. }) {
        return Ok(());
    }
    let Some(family) = family_of(returned) else {
        return Ok(());
    };
    writeln!(
        output,
        "    if let Some(anchor) = __rawr_internal::scope::anchor() {{
        return __rawr_internal::scope::with::<__rawr_internal::scope::RegionF, _>(anchor, |region| -> Result<Dynamic, Error> {{
            let made = lend(arguments.arena, || {call});
            Ok::<Dynamic, Error>(Dynamic::from(region.insert::<{family}>(made)))
        }})
        .map_err(|error| Error::Lent(\"a slot's region\", error))?;
    }}"
    )
    .map_err(|_| request::Error::Encoding)
}

/// Whether an input of the call is a native value that carries a lifetime,
/// as a widget a view made does.
fn lifetime_inputs(
    signature: &crate::classify::representability::signature::Signature,
    substitutions: &[convert::Type],
) -> Result<bool, request::Error> {
    for input in signature.inputs() {
        if lends_str(input, signature, substitutions) {
            continue;
        }
        let ty = substituted(input, signature, substitutions)?;
        // A native value, or a sequence of them, as `column(iterator)`
        // takes a view's `Text`s.
        let native = match &ty {
            convert::Type::Sequence { element, .. } => element.as_ref(),
            ty => ty,
        };
        if matches!(native, convert::Type::Nominal { .. }) && carries_lifetime(native) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// What a frame's opening of a call whose output borrows what it takes
/// writes besides the call: `rebound`, inside the frame, borrows each
/// `&str` and copied array the frame owns; `before` and `with` make the
/// frame's `Copies`; and `back`, once the frame ends and the output no
/// longer borrows them, gives back each `&mut` array the call changed, for
/// the statement after the frame to store (D558 slice 2).
struct FrameCopies {
    rebound: String,
    before: String,
    back: String,
    with: String,
}

fn frame_copies(
    signature: &crate::classify::representability::signature::Signature,
    substitutions: &[convert::Type],
) -> Result<FrameCopies, request::Error> {
    let copies = copied_inputs(signature);
    let mut rebound = String::new();
    for (position, input) in signature.inputs().iter().enumerate() {
        if lends_str(input, signature, substitutions) {
            rebound.push_str(&format!(
                "\n        let argument_{position} = argument_{position}_owned.as_str();"
            ));
        }
    }
    for (position, _) in &copies {
        rebound.push_str(&format!(
            "\n        let argument_{position} = &mut argument_{position}_copy;"
        ));
    }
    if copies.is_empty() {
        return Ok(FrameCopies {
            rebound,
            before: String::new(),
            back: String::new(),
            with: String::new(),
        });
    }
    let mut given = Vec::new();
    for (position, element) in &copies {
        let mut converted = String::new();
        render_to_dynamic(
            element,
            &format!("argument_{position}_copy"),
            0,
            &mut converted,
        )?;
        given.push(format!("({converted})?"));
    }
    Ok(FrameCopies {
        rebound,
        before: "let copies = __rawr_internal::scope::Copies::new();\n    let frame_copies = copies.clone();\n    ".to_owned(),
        back: format!(
            "\n        drop(region);\n        let given = with_arena(|arena| {{ let mut values: [&mut Dynamic; 0] = []; #[allow(unused_variables)] let arguments = &mut Arguments {{ values: &mut values, arena }}; Ok::<_, Error>(vec![{}]) }}).map_err(runtime_error)?;\n        frame_copies.set(given);",
            given.join(", ")
        ),
        with: ".with_copies(copies)".to_owned(),
    })
}

/// A method call whose output borrows what it takes besides its receiver
/// (D558 tier 1), as quick-xml's `read_event_into(&mut buf)`: it gives a
/// scope frame the statement it is in opens. The frame reaches the
/// receiver for the call alone, in its region or in the arena, makes the
/// output in a region of its own, and runs the rest of the scope.
#[allow(clippy::too_many_arguments)]
fn render_method_opening(
    plan: &Plan,
    path: &str,
    receiver: Receiver,
    signature: &crate::classify::representability::signature::Signature,
    substitutions: &[convert::Type],
    constants: &[convert::Constant],
    container_parameters: usize,
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    let encoding = |_| request::Error::Encoding;
    let mut container = Vec::new();
    render_container_path(
        path,
        signature,
        substitutions,
        constants,
        container_parameters,
        true,
        &mut container,
    )?;
    let container = String::from_utf8(container).map_err(|_| request::Error::Encoding)?;
    let mut call = format!("receiver.{}", plan.name()).into_bytes();
    render_item_turbofish(
        signature,
        substitutions,
        constants,
        container_parameters,
        &mut call,
    )?;
    call.write_all(b"(").map_err(encoding)?;
    argument_list(signature.inputs().len(), 1, &mut call)?;
    call.write_all(b")").map_err(encoding)?;
    let call = String::from_utf8(call).map_err(|_| request::Error::Encoding)?;
    let (access, reach) = match receiver {
        Receiver::Shared => ("shared", "get"),
        Receiver::Mutable => ("mutable", "get_mut"),
        Receiver::Owned => {
            return Err(request::Error::Drift {
                field: "a scope frame's call that takes its receiver",
            });
        }
    };
    let scoped = match container_family(path, &container) {
        Some(family) => format!(
            "Some(scoped) => __rawr_internal::scope::with::<__rawr_internal::scope::RegionF, _>(scoped.nonce(), |region| -> Result<_, Error> {{
                #[allow(unused_mut)]
                let mut receiver = region.{reach}::<{family}>(scoped).map_err(|error| Error::Lent(scoped.name(), error))?;
                Ok({call})
            }})
            .map_err(|error| Error::Lent(scoped.name(), error))
            .and_then(|made| made),"
        ),
        None => "Some(scoped) => Err(Error::Lent(scoped.name(), __rawr_internal::scope::Error::Mismatch)),".to_owned(),
    };
    let returned = substituted(signature.output(), signature, substitutions)?;
    let converted = {
        let _region = InRegion::enter();
        let mut converted = String::new();
        render_to_dynamic(&returned, "made", 0, &mut converted)?;
        converted
    };
    let FrameCopies {
        rebound,
        before,
        back,
        with,
    } = frame_copies(signature, &[])?;
    let name = format!("{}::{}", rust_path(plan), plan.name());
    // The frame itself, once the call has made its output.
    let frame = format!(
        "let mut region = __rawr_internal::scope::Region::new();
            let value = with_arena(|arena| {{
                let mut values: [&mut Dynamic; 0] = [];
                #[allow(unused_variables)]
                let arguments = &mut Arguments {{ values: &mut values, arena }};
                {converted}
            }})
            .map_err(runtime_error)?;
            let result = __rawr_internal::scope::open(&mut region, || rest(value));{back}
            result"
    );
    if signature.scope() == Some(crate::classify::representability::signature::Scope::Borrows) {
        // The output borrows the receiver: the frame holds it borrowed in
        // its region, shared for `&self`, or leased from its arena slot,
        // until the rest of the scope has run.
        let (claim, family_reach) = match receiver {
            Receiver::Shared => ("with_ref", "get"),
            _ => ("with", "get_mut"),
        };
        let scoped = match container_family(path, &container) {
            Some(family) => format!(
                "Some(scoped) => __rawr_internal::scope::{claim}::<__rawr_internal::scope::RegionF, _>(scoped.nonce(), |region| -> Result<Dynamic, Box<__rawr_internal::EvalAltResult>> {{
            #[allow(unused_mut)]
            let mut receiver = region.{family_reach}::<{family}>(scoped).map_err(|error| runtime_error(Error::Lent(scoped.name(), error)))?;
            let made = {call};
            {frame}
        }})
        .map_err(|error| runtime_error(Error::Lent(scoped.name(), error)))?,"
            ),
            None => "Some(scoped) => Err(runtime_error(Error::Lent(scoped.name(), __rawr_internal::scope::Error::Mismatch))),".to_owned(),
        };
        let borrow = if matches!(receiver, Receiver::Shared) {
            "&*"
        } else {
            ""
        };
        return writeln!(
            output,
            "    let scoped = scoped_value(arguments, 0);
    let handle = match scoped {{
        Some(_) => None,
        None => Some(native_handle(arguments, 0, false)?),
    }};
    {before}let output = __rawr_internal::scope::Opening::new({name:?}, move |rest| {{{rebound}
        match scoped {{
            {scoped}
            None => {{
                let mut held: Held<{container}> = with_arena(|arena| arena.lease(handle.ok_or(Error::Stale)?)).map_err(runtime_error)?;
                #[allow(unused_mut)]
                let mut receiver = {borrow}held.get_mut();
                let made = {call};
                {frame}
            }}
        }}
    }}){with};
    Ok(Dynamic::from(output))"
        )
        .map_err(encoding);
    }
    writeln!(
        output,
        "    let scoped = scoped_value(arguments, 0);
    let handle = match scoped {{
        Some(_) => None,
        None => Some(native_handle(arguments, 0, false)?),
    }};
    {before}let output = __rawr_internal::scope::Opening::new({name:?}, move |rest| {{{rebound}
        let made = match scoped {{
            {scoped}
            None => with_arena(|arena| arena.{access}::<{container}, _>(handle.ok_or(Error::Stale)?, |receiver| {call})),
        }}
        .map_err(runtime_error)?;
        let mut region = __rawr_internal::scope::Region::new();
        let value = with_arena(|arena| {{
            let mut values: [&mut Dynamic; 0] = [];
            #[allow(unused_variables)]
            let arguments = &mut Arguments {{ values: &mut values, arena }};
            {converted}
        }})
        .map_err(runtime_error)?;
        let result = __rawr_internal::scope::open(&mut region, || rest(value));{back}
        result
    }}){with};
    Ok(Dynamic::from(output))"
    )
    .map_err(encoding)
}

/// The call of a method whose receiver is a value in a scope frame's
/// region (D558 tier 1), when its type carries lifetimes: the receiver is
/// reached in its region, and an output that carries the region's
/// lifetime stays there. Nothing for another receiver type.
#[allow(clippy::too_many_arguments)]
fn render_scoped_receiver(
    plan: &Plan,
    path: &str,
    receiver: Receiver,
    signature: &crate::classify::representability::signature::Signature,
    substitutions: &[convert::Type],
    constants: &[convert::Constant],
    container_parameters: usize,
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    let encoding = |_| request::Error::Encoding;
    let mut container = Vec::new();
    render_container_path(
        path,
        signature,
        substitutions,
        constants,
        container_parameters,
        true,
        &mut container,
    )?;
    let container = String::from_utf8(container).map_err(|_| request::Error::Encoding)?;
    let Some(family) = container_family(path, &container) else {
        return Ok(());
    };
    let mut call = format!("receiver.{}", plan.name()).into_bytes();
    render_item_turbofish(
        signature,
        substitutions,
        constants,
        container_parameters,
        &mut call,
    )?;
    call.write_all(b"(").map_err(encoding)?;
    argument_list(signature.inputs().len(), 1, &mut call)?;
    let mut call = format!("receiver.{}", plan.name()).into_bytes();
    render_item_turbofish(
        signature,
        substitutions,
        constants,
        container_parameters,
        &mut call,
    )?;
    call.write_all(b"(").map_err(encoding)?;
    argument_list(signature.inputs().len(), 1, &mut call)?;
    call.write_all(b")").map_err(encoding)?;
    call.write_all(lent_copy(signature).as_bytes())
        .map_err(encoding)?;
    let call = String::from_utf8(call).map_err(|_| request::Error::Encoding)?;
    let reach = match receiver {
        Receiver::Owned => "take",
        Receiver::Shared => "get",
        Receiver::Mutable => "get_mut",
    };
    let returned = substituted(signature.output(), signature, substitutions)?;
    let converted = {
        let _region = InRegion::enter();
        let mut converted = String::new();
        render_to_dynamic(&returned, "output", 0, &mut converted)?;
        converted
    };
    // A `&str` the call lends the receiver, as `label(impl IntoFragment<'a>)`
    // takes one, is pinned in the region the receiver is in, at its
    // lifetime, as a slot's region pins its strings.
    let mut pinned = String::new();
    for (position, input) in signature.inputs().iter().enumerate() {
        if lends_str(input, signature, substitutions) {
            pinned.push_str(&format!(
                "\n        let argument_{position} = region.pins().ok_or(Error::Lent(scoped.name(), __rawr_internal::scope::Error::Mismatch))?.pin(argument_{position}_owned.clone());"
            ));
        }
    }
    // In the region the adapter's body already runs in, which an argument
    // of the call is in too.
    if REGION_ARGUMENTS.get() {
        return writeln!(
            output,
            "    if let Some(scoped) = scoped_value(arguments, 0) {{
        #[allow(unused_mut)]
        let mut receiver = region.{reach}::<{family}>(scoped).map_err(|error| Error::Lent(scoped.name(), error))?;{pinned}
        let output = {call};
        return {converted};
    }}"
        )
        .map_err(encoding);
    }
    writeln!(
        output,
        "    if let Some(scoped) = scoped_value(arguments, 0) {{
        return __rawr_internal::scope::with::<__rawr_internal::scope::RegionF, _>(scoped.nonce(), |region| -> Result<Dynamic, Error> {{
            #[allow(unused_mut)]
            let mut receiver = region.{reach}::<{family}>(scoped).map_err(|error| Error::Lent(scoped.name(), error))?;{pinned}
            let output = {call};
            {converted}
        }})
        .map_err(|error| Error::Lent(scoped.name(), error))?;
    }}"
    )
    .map_err(encoding)
}

/// The call of a method that borrows its receiver, into `output`. The
/// receiver is a value in the arena, or one native code lends the call
/// (D558.3), as ratatui's `&mut Frame<'_>` is. That one is reached through
/// the accessor its trampoline made, a `fn(u64, &mut dyn FnMut(&mut T))`
/// whose visitor takes the value at every lifetime, so the call can keep
/// nothing that borrows it, and the runner stays free of unsafe code.
#[allow(clippy::too_many_arguments)]
fn render_borrowed_receiver(
    plan: &Plan,
    path: &str,
    receiver: Receiver,
    signature: &crate::classify::representability::signature::Signature,
    substitutions: &[convert::Type],
    constants: &[convert::Constant],
    container_parameters: usize,
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    let encoding = |_| request::Error::Encoding;
    let mutable = matches!(receiver, Receiver::Mutable);
    let mut call = if plan.application_invocation().is_some() {
        // A proof-backed application calls its isolated helper, whose
        // scope is the source call's (`render_applications`).
        let mut call = format!(
            "rawr_applications::{}::call(receiver",
            identity_name("application", plan)
        )
        .into_bytes();
        if !signature.inputs().is_empty() {
            call.write_all(b", ").map_err(encoding)?;
        }
        call
    } else {
        let mut call = format!("receiver.{}", plan.name()).into_bytes();
        render_item_turbofish(
            signature,
            substitutions,
            constants,
            container_parameters,
            &mut call,
        )?;
        call.write_all(b"(").map_err(encoding)?;
        call
    };
    argument_list(signature.inputs().len(), 1, &mut call)?;
    call.write_all(b")").map_err(encoding)?;
    call.write_all(lent_copy(signature).as_bytes())
        .map_err(encoding)?;
    let call = String::from_utf8(call).map_err(|_| request::Error::Encoding)?;
    let mut container = Vec::new();
    render_container_path(
        path,
        signature,
        substitutions,
        constants,
        container_parameters,
        true,
        &mut container,
    )?;
    let container = String::from_utf8(container).map_err(|_| request::Error::Encoding)?;
    render_scoped_receiver(
        plan,
        path,
        receiver,
        signature,
        substitutions,
        constants,
        container_parameters,
        output,
    )?;
    let access = if mutable { "mutable" } else { "shared" };
    let borrow = if mutable { "&mut " } else { "&" };
    let mut taken = Vec::new();
    argument_list(signature.inputs().len(), 1, &mut taken)?;
    let taken = String::from_utf8(taken).map_err(|_| request::Error::Encoding)?;
    let taken = if taken.is_empty() {
        String::from("()")
    } else {
        format!("({taken},)")
    };
    // An output that borrows what a lent receiver carries can't leave the
    // accessor's visitor: such a receiver is reached only in the arena, at
    // `'static`, or in a scope frame's region (D558 tier 1).
    if container_family(path, &container).is_some()
        && may_borrow(&substituted(signature.output(), signature, substitutions)?)
    {
        return writeln!(
            output,
            "    let receiver = native_handle(arguments, 0, false)?;
    let output = arguments.arena.{access}::<{container}, _>(receiver, |receiver| {call})?;"
        )
        .map_err(encoding);
    }
    writeln!(
        output,
        "    let output = match lent_value(arguments, 0) {{
        Some(lent) => {{
            let access: fn(u64, &mut dyn FnMut({borrow}{container})) -> Result<(), u8> = lent
                .accessor({mutable})
                .map_err(|error| Error::Lent(lent.name(), error))?;
            let mut taken = Some({taken});
            let mut output = None;
            access(lent.nonce().bits(), &mut |receiver| {{
                #[allow(irrefutable_let_patterns, unused_parens)]
                if let Some({taken}) = taken.take() {{
                    output = Some({call});
                }}
            }})
            .map_err(|code| Error::Lent(lent.name(), __rawr_internal::scope::Error::from_code(code)))?;
            output.ok_or(Error::Lent(lent.name(), __rawr_internal::scope::Error::Busy))?
        }}
        None => {{
            let receiver = native_handle(arguments, 0, false)?;
            arguments.arena.{access}::<{container}, _>(receiver, |receiver| {call})?
        }}
    }};"
    )
    .map_err(encoding)
}

/// `render_guard` for a receiver in a scope frame's region (D558 tier 1),
/// as quick-xml's `reader.config_mut()` is on a `Reader<&[u8]>` its
/// `Reader::from_str` frame holds: the receiver is reached in its region at
/// each access instead of in the arena. Nothing for a receiver type that
/// carries no lifetime.
#[allow(clippy::too_many_arguments)]
fn render_scoped_guard(
    plan: &Plan,
    path: &str,
    guard: crate::classify::representability::signature::Guard,
    signature: &crate::classify::representability::signature::Signature,
    substitutions: &[convert::Type],
    constants: &[convert::Constant],
    container_parameters: usize,
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    use crate::classify::representability::signature::Guard;
    let encoding = |_| request::Error::Encoding;
    let mut container = Vec::new();
    render_container_path(
        path,
        signature,
        substitutions,
        constants,
        container_parameters,
        true,
        &mut container,
    )?;
    let container = String::from_utf8(container).map_err(|_| request::Error::Encoding)?;
    let Some(family) = container_family(path, &container) else {
        return Ok(());
    };
    let target = substituted(signature.output(), signature, substitutions)?;
    let mut method = format!("receiver.{}", plan.name()).into_bytes();
    render_item_turbofish(
        signature,
        substitutions,
        constants,
        container_parameters,
        &mut method,
    )?;
    method.write_all(b"(").map_err(encoding)?;
    argument_list(signature.inputs().len(), 1, &mut method)?;
    method.write_all(b")").map_err(encoding)?;
    let method = String::from_utf8(method).map_err(|_| request::Error::Encoding)?;
    // The guard on the receiver in its region, inside `with`.
    let reach = format!(
        "__rawr_internal::scope::with::<__rawr_internal::scope::RegionF, _>(scoped.nonce(), |region| -> Result<_, Error> {{
            let receiver = region.get_mut::<{family}>(scoped).map_err(|error| Error::Lent(scoped.name(), error))?;
            let mut guard = {method};
            let _ = &mut guard;"
    );
    let close = "})
        .map_err(|error| Error::Lent(scoped.name(), error))
        .and_then(|output| output)";
    let mut give = Vec::new();
    render_give(&target, "output", &mut give)?;
    let give = String::from_utf8(give).map_err(|_| request::Error::Encoding)?;
    match guard {
        // A receiver in a region lends no optional borrow yet.
        Guard::OptionShared | Guard::OptionMutable => Ok(()),
        Guard::Shared => writeln!(
            output,
            "    if let Some(scoped) = scoped_value(arguments, 0) {{
        return {reach}
            let output = guard_copy!(&*guard)?;
            {give}
        {close};
    }}"
        )
        .map_err(encoding),
        Guard::Mutable => {
            let mut take = Vec::new();
            render_take(0, 0, &target, &mut take)?;
            let take = String::from_utf8(take).map_err(|_| request::Error::Encoding)?;
            let give = format!(
                "let read: Result<Dynamic, Error> = {{\n{give}}};\nread.and_then(|value| attest_read(&mut *arguments.arena, value, &witness))"
            );
            writeln!(
                output,
                "    if let Some(scoped) = scoped_value(arguments, 0) {{
        let witness = output_witness();
        let place = __rawr_internal::island::place::Place::new(move |access: __rawr_internal::island::place::Access<'_>| {{
            use __rawr_internal::island::place::Access;
            let mut stored = match access {{
                Access::Read => None,
                Access::Store(value) => Some(value),
                Access::Update(apply) => {{
                    return with_arena(|arena| {{
                        {reach}
                            let output = guard_copy!(&*guard)?;
                            let value = {{
                                #[allow(unused_variables)]
                                let arguments = &mut Arguments {{ values: &mut [], arena: &mut *arena }};
                                {give}
                            }}?;
                            let mut stored = match lend(arena, || apply(value)) {{
                                Ok(stored) => stored,
                                Err(error) => return Ok(Err(error)),
                            }};
                            let mut values = [&mut stored];
                            let arguments = &mut Arguments {{ values: &mut values, arena }};
                            {take}
                            *guard = argument_0;
                            Ok(Ok(Dynamic::UNIT))
                        {close}
                    }})
                    .map_err(runtime_error)?;
                }}
            }};
            with_arena(|arena| {{
                let mut values: Vec<&mut Dynamic> = stored.iter_mut().collect();
                let arguments = &mut Arguments {{ values: values.as_mut_slice(), arena }};
                {reach}
                    if arguments.values.is_empty() {{
                        let output = guard_copy!(&*guard)?;
                        {give}
                    }} else {{
                        {take}
                        *guard = argument_0;
                        Ok(Dynamic::UNIT)
                    }}
                {close}
            }})
            .map_err(runtime_error)
        }});
        return Ok(Dynamic::from(place));
    }}"
            )
            .map_err(encoding)
        }
    }
}

/// A method whose Rust output is a borrow guard, as Freya's `State::read`
/// and `State::write` give. The guard borrows the receiver, which lives in
/// the arena, so it never leaves the one call that holds the receiver.
/// A `Deref` guard gives a copy of its target. A `DerefMut` guard gives a
/// place (`island::place::Place`) whose accesses each call the method
/// again and drop the guard before they return: a read copies the target,
/// a store assigns it, and an update (`op=`, D554.7(f)) copies it, has the
/// script change the copy and assigns the result, all through one guard.
/// An update takes the receiver out of its slot while the script runs, so
/// the arena can be lent to the native calls the change makes. The
/// method's inputs are taken first, so they are owned values the call
/// borrows.
#[allow(clippy::too_many_arguments)]
fn render_guard(
    plan: &Plan,
    path: &str,
    receiver: Receiver,
    guard: crate::classify::representability::signature::Guard,
    signature: &crate::classify::representability::signature::Signature,
    substitutions: &[convert::Type],
    constants: &[convert::Constant],
    container_parameters: usize,
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    use crate::classify::representability::signature::Guard;
    let encoding = |_| request::Error::Encoding;
    let borrow = if matches!(receiver, Receiver::Mutable) {
        "arguments.arena.mutable"
    } else {
        "arguments.arena.shared"
    };
    let target = substituted(signature.output(), signature, substitutions)?;
    // `arena.shared::<Receiver, _>(handle, |receiver| { let mut guard =
    // receiver.method(..); ` through `access`, whose closure takes `taking`.
    let call = |access: &str,
                handle: &str,
                taking: &str,
                output: &mut Vec<u8>|
     -> Result<(), request::Error> {
        write!(output, "{access}::<").map_err(encoding)?;
        render_container_path(
            path,
            signature,
            substitutions,
            constants,
            container_parameters,
            true,
            output,
        )?;
        write!(
            output,
            ", _>({handle}, |{taking}| {{ let mut guard = receiver.{}",
            plan.name()
        )
        .map_err(encoding)?;
        render_item_turbofish(
            signature,
            substitutions,
            constants,
            container_parameters,
            output,
        )?;
        output.write_all(b"(").map_err(encoding)?;
        argument_list(signature.inputs().len(), 1, output)?;
        output
            .write_all(b"); let _ = &mut guard; ")
            .map_err(encoding)
    };
    let indented = |lines: &[u8], indent: &str, output: &mut Vec<u8>| {
        for line in String::from_utf8_lossy(lines).lines() {
            writeln!(output, "{indent}{line}").map_err(encoding)?;
        }
        Ok::<_, request::Error>(())
    };
    render_scoped_guard(
        plan,
        path,
        guard,
        signature,
        substitutions,
        constants,
        container_parameters,
        output,
    )?;
    writeln!(
        output,
        "    let receiver = native_handle(arguments, 0, false)?;"
    )
    .map_err(encoding)?;
    // `call`, giving the method copies of its inputs, so a place reaches
    // its target again with each access.
    let call_again = |access: &str,
                      handle: &str,
                      taking: &str,
                      output: &mut Vec<u8>|
     -> Result<(), request::Error> {
        write!(output, "{access}::<").map_err(encoding)?;
        render_container_path(
            path,
            signature,
            substitutions,
            constants,
            container_parameters,
            true,
            output,
        )?;
        write!(
            output,
            ", _>({handle}, |{taking}| {{ let guard = receiver.{}",
            plan.name()
        )
        .map_err(encoding)?;
        render_item_turbofish(
            signature,
            substitutions,
            constants,
            container_parameters,
            output,
        )?;
        output.write_all(b"(").map_err(encoding)?;
        for input in 0..signature.inputs().len() {
            if input > 0 {
                output.write_all(b", ").map_err(encoding)?;
            }
            write!(output, "Clone::clone(&argument_{input})").map_err(encoding)?;
        }
        output
            .write_all(b"); let Some(mut guard) = guard else { return Err(Error::StaleHandle) }; let _ = &mut guard; ")
            .map_err(encoding)
    };
    match guard {
        Guard::Shared => {
            output.write_all(b"    let output = ").map_err(encoding)?;
            call(borrow, "receiver", "receiver", output)?;
            output
                .write_all(b"guard_copy!(&*guard) })??;\n")
                .map_err(encoding)?;
            render_give(&target, "output", output)
        }
        // `Some` of a copy of the target, or `None`.
        Guard::OptionShared => {
            output.write_all(b"    let output = ").map_err(encoding)?;
            call(borrow, "receiver", "receiver", output)?;
            output
                .write_all(b"guard.map(|target| guard_copy!(&*target)).transpose() })??;\n")
                .map_err(encoding)?;
            render_give(&target, "output", output)
        }
        // `Some` of a place whose accesses call the method again, or `None`.
        Guard::OptionMutable => {
            let convert::Type::Option(element) = &target else {
                return Err(request::Error::Drift {
                    field: "an optional borrow's output is no option",
                });
            };
            let mut given = Vec::new();
            render_give(element, "output", &mut given)?;
            let given = String::from_utf8(given).map_err(|_| request::Error::Encoding)?;
            let give = format!(
                "let read: Result<Dynamic, Error> = {{\n{given}}};\nread.and_then(|value| attest_read(&mut *arguments.arena, value, &witness))\n"
            )
            .into_bytes();
            let mut take = Vec::new();
            let stored = signature.inputs().len();
            render_take(stored, 0, element, &mut take)?;
            output.write_all(b"    let present = ").map_err(encoding)?;
            call_again(borrow, "receiver.clone()", "receiver", output)?;
            output
                .write_all(
                    b"Ok::<(), Error>(()) }).map(|reached| reached.is_ok())?;
    if !present {
        return Ok(__rawr_internal::option::none());
    }
    let witness = output_witness().and_then(|witness| witness.argument_witness(0));
    let place = __rawr_internal::island::place::Place::new(move |access: __rawr_internal::island::place::Access<'_>| {
        use __rawr_internal::island::place::Access;
        let mut stored = match access {
            Access::Read => None,
            Access::Store(value) => Some(value),
            Access::Update(apply) => {
                return with_arena(|arena| {
                    ",
                )
                .map_err(encoding)?;
            call_again("arena.held", "receiver.clone()", "arena, receiver", output)?;
            output
                .write_all(
                    b"
                        let output = guard_copy!(&*guard)?;
                        let value = {
                            #[allow(unused_variables)]
                            let arguments = &mut Arguments { values: &mut [], arena: &mut *arena };
",
                )
                .map_err(encoding)?;
            indented(&give, "                        ", output)?;
            output
                .write_all(
                    b"                        }?;
                        let mut stored = match lend(arena, || apply(value)) {
                            Ok(stored) => stored,
                            Err(error) => return Ok(Err(error)),
                        };
                        let mut values = [&mut stored];
                        let arguments = &mut Arguments { values: &mut values, arena };
",
                )
                .map_err(encoding)?;
            indented(&take, "                    ", output)?;
            output
                .write_all(
                    format!(
                        "                        *guard = argument_{stored};
                        Ok(Ok(Dynamic::UNIT))
                    }})?
                }})
                .map_err(runtime_error)?;
            }}
        }};
        with_arena(|arena| {{
            let mut values: Vec<&mut Dynamic> = stored.iter_mut().collect();
            let arguments = &mut Arguments {{ values: values.as_mut_slice(), arena }};
            if arguments.values.is_empty() {{
                let output = "
                    )
                    .as_bytes(),
                )
                .map_err(encoding)?;
            call_again(borrow, "receiver.clone()", "receiver", output)?;
            output
                .write_all(b"guard_copy!(&*guard) })??;\n")
                .map_err(encoding)?;
            indented(&give, "            ", output)?;
            output
                .write_all(b"            } else {\n")
                .map_err(encoding)?;
            indented(&take, "            ", output)?;
            output.write_all(b"                ").map_err(encoding)?;
            call_again(borrow, "receiver.clone()", "receiver", output)?;
            output
                .write_all(
                    format!(
                        "*guard = argument_{stored}; Ok::<(), Error>(()) }})??;
                Ok(Dynamic::UNIT)
            }}
        }})
        .map_err(runtime_error)
    }});
    Ok(__rawr_internal::option::some(Dynamic::from(place)))
"
                    )
                    .as_bytes(),
                )
                .map_err(encoding)
        }
        Guard::Mutable => {
            if !signature.inputs().is_empty() {
                return Err(request::Error::Drift {
                    field: "a mutable borrow guard's method takes inputs",
                });
            }
            let mut given = Vec::new();
            render_give(&target, "output", &mut given)?;
            let given = String::from_utf8(given).map_err(|_| request::Error::Encoding)?;
            // A read is attested as the guard's call's output is.
            let give = format!(
                "let read: Result<Dynamic, Error> = {{\n{given}}};\nread.and_then(|value| attest_read(&mut *arguments.arena, value, &witness))\n"
            )
            .into_bytes();
            let mut take = Vec::new();
            render_take(0, 0, &target, &mut take)?;
            output
                .write_all(
                    b"    let witness = output_witness();\n    let place = __rawr_internal::island::place::Place::new(move |access: __rawr_internal::island::place::Access<'_>| {
        use __rawr_internal::island::place::Access;
        let mut stored = match access {
            Access::Read => None,
            Access::Store(value) => Some(value),
            Access::Update(apply) => {
                return with_arena(|arena| {
                    ",
                )
                .map_err(encoding)?;
            call("arena.held", "receiver.clone()", "arena, receiver", output)?;
            output
                .write_all(
                    b"
                        let output = guard_copy!(&*guard)?;
                        let value = {
                            #[allow(unused_variables)]
                            let arguments = &mut Arguments { values: &mut [], arena: &mut *arena };
",
                )
                .map_err(encoding)?;
            indented(&give, "                        ", output)?;
            output
                .write_all(
                    b"                        }?;
                        let mut stored = match lend(arena, || apply(value)) {
                            Ok(stored) => stored,
                            Err(error) => return Ok(Err(error)),
                        };
                        let mut values = [&mut stored];
                        let arguments = &mut Arguments { values: &mut values, arena };
",
                )
                .map_err(encoding)?;
            indented(&take, "                    ", output)?;
            output
                .write_all(
                    b"                        *guard = argument_0;
                        Ok(Ok(Dynamic::UNIT))
                    })?
                })
                .map_err(runtime_error)?;
            }
        };
        with_arena(|arena| {
            let mut values: Vec<&mut Dynamic> = stored.iter_mut().collect();
            let arguments = &mut Arguments { values: values.as_mut_slice(), arena };
            if arguments.values.is_empty() {
                let output = ",
                )
                .map_err(encoding)?;
            call(borrow, "receiver.clone()", "receiver", output)?;
            output
                .write_all(b"guard_copy!(&*guard) })??;\n")
                .map_err(encoding)?;
            indented(&give, "            ", output)?;
            output
                .write_all(b"            } else {\n")
                .map_err(encoding)?;
            indented(&take, "            ", output)?;
            output.write_all(b"                ").map_err(encoding)?;
            call(borrow, "receiver.clone()", "receiver", output)?;
            output
                .write_all(
                    b"*guard = argument_0; })?;
                Ok(Dynamic::UNIT)
            }
        })
        .map_err(runtime_error)
    });
    Ok(Dynamic::from(place))
",
                )
                .map_err(encoding)
        }
    }
}

fn arguments(plan: &Plan) -> Result<u32, request::Error> {
    let count = match plan.operation() {
        Operation::Declaration { .. } | Operation::Constant { .. } => 0,
        Operation::StructConstructor { fields, .. } | Operation::EnumVariant { fields, .. } => {
            fields.len()
        }
        Operation::StructField { .. } => 1,
        Operation::Function {
            signature,
            functions,
            mappers,
            ..
        } => dynamic_function_arity(signature, functions, mappers)?,
        Operation::Method {
            signature,
            functions,
            ..
        } => dynamic_function_arity(signature, functions, &[])?
            .checked_add(1)
            .ok_or(request::Error::Limit {
                resource: "adapter argument cardinality",
                limit: u32::MAX as usize,
                actual: usize::MAX,
            })?,
        Operation::Builder { steps } => steps
            .iter()
            .flat_map(|step| step.arguments())
            .copied()
            .max()
            .map_or(0_usize, |position| position as usize + 1),
    };
    u32::try_from(count).map_err(|_| request::Error::Limit {
        resource: "adapter argument cardinality",
        limit: u32::MAX as usize,
        actual: count,
    })
}

fn render_builder(
    steps: &[crate::surface::adapter::BuilderStep],
    replay: bool,
    profile: Option<&crate::profile::Proof>,
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    for (index, step) in steps.iter().enumerate() {
        let final_step = index + 1 == steps.len();
        let binding = if final_step { "output" } else { "builder" };
        match step.plan().operation() {
            Operation::Function {
                signature,
                substitutions,
                constants,
                functions,
                mappers,
                container_parameters,
                associated: true,
                ..
            } if step.is_construct() => {
                if !functions.is_empty() {
                    return Err(request::Error::Drift {
                        field: "static function item entered builder replay",
                    });
                }
                if !replay {
                    render_builder_function_arguments(
                        signature.inputs(),
                        mappers,
                        step.arguments(),
                        profile,
                        output,
                    )?;
                }
                write!(output, "    let {binding} = ").map_err(|_| request::Error::Encoding)?;
                render_function_path(
                    step.plan(),
                    signature,
                    substitutions,
                    constants,
                    *container_parameters,
                    true,
                    output,
                )?;
                render_item_turbofish(
                    signature,
                    substitutions,
                    constants,
                    *container_parameters,
                    output,
                )?;
                output
                    .write_all(b"(")
                    .map_err(|_| request::Error::Encoding)?;
                builder_function_argument_list(
                    signature.inputs(),
                    mappers,
                    step.arguments(),
                    replay,
                    profile,
                    output,
                )?;
                output
                    .write_all(b");\n")
                    .map_err(|_| request::Error::Encoding)?;
                if final_step && !replay {
                    render_give_with(signature, substitutions, "output", output)?;
                }
            }
            Operation::StructConstructor { form, fields, .. } if step.is_construct() => {
                let types = fields.iter().map(crate::surface::adapter::Field::ty);
                if !replay {
                    render_builder_arguments_iter(types, step.arguments(), output)?;
                }
                write!(output, "    let {binding} = {}", rust_path(step.plan()))
                    .map_err(|_| request::Error::Encoding)?;
                render_builder_constructor(*form, fields, step.arguments(), replay, output)?;
                output
                    .write_all(b";\n")
                    .map_err(|_| request::Error::Encoding)?;
                if final_step && !replay {
                    output
                        .write_all(
                            b"    insert_native!(arguments.arena, output).map(Dynamic::from)\n",
                        )
                        .map_err(|_| request::Error::Encoding)?;
                }
            }
            Operation::Method {
                receiver: Receiver::Owned,
                signature,
                substitutions,
                constants,
                container_parameters,
                ..
            } if !step.is_construct() => {
                if !replay {
                    render_builder_arguments(signature.inputs(), step.arguments(), output)?;
                }
                write!(output, "    let {binding} = builder.{}", step.plan().name())
                    .map_err(|_| request::Error::Encoding)?;
                render_item_turbofish(
                    signature,
                    substitutions,
                    constants,
                    *container_parameters,
                    output,
                )?;
                output
                    .write_all(b"(")
                    .map_err(|_| request::Error::Encoding)?;
                builder_typed_argument_list(signature.inputs(), step.arguments(), replay, output)?;
                output
                    .write_all(b");\n")
                    .map_err(|_| request::Error::Encoding)?;
                if final_step && !replay {
                    render_give_with(signature, substitutions, "output", output)?;
                }
            }
            _ => {
                return Err(request::Error::Drift {
                    field: "consuming builder operation",
                });
            }
        }
    }
    Ok(())
}

fn render_builder_arguments(
    inputs: &[convert::Type],
    arguments: &[u32],
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    render_builder_arguments_iter(inputs.iter(), arguments, output)
}

fn render_builder_inputs(
    steps: &[crate::surface::adapter::BuilderStep],
    profile: &crate::profile::Proof,
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    for step in steps {
        match step.plan().operation() {
            Operation::Function {
                signature,
                functions,
                mappers,
                associated: true,
                ..
            } if step.is_construct() && functions.is_empty() => {
                render_builder_function_arguments(
                    signature.inputs(),
                    mappers,
                    step.arguments(),
                    Some(profile),
                    output,
                )?;
            }
            Operation::Method {
                receiver: Receiver::Owned,
                signature,
                ..
            } if !step.is_construct() => {
                render_builder_arguments(signature.inputs(), step.arguments(), output)?;
            }
            Operation::StructConstructor { fields, .. } if step.is_construct() => {
                render_builder_arguments_iter(
                    fields.iter().map(crate::surface::adapter::Field::ty),
                    step.arguments(),
                    output,
                )?;
            }
            _ => {
                return Err(request::Error::Drift {
                    field: "consuming replay input operation",
                });
            }
        }
    }
    Ok(())
}

fn render_builder_function_arguments(
    inputs: &[convert::Type],
    mappers: &[crate::surface::adapter::EventMapper],
    arguments: &[u32],
    profile: Option<&crate::profile::Proof>,
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    let mut mappers = mappers.iter().peekable();
    let mut arguments = arguments.iter();
    for (position, ty) in inputs.iter().enumerate() {
        let mapped_here = mappers
            .peek()
            .is_some_and(|mapper| usize::try_from(mapper.position()).ok() == Some(position));
        if mapped_here {
            if !matches!(ty, convert::Type::FunctionItem { .. }) {
                return Err(request::Error::Drift {
                    field: "builder profile mapper position",
                });
            }
            let mapper = mappers.next().ok_or(request::Error::Drift {
                field: "builder profile mapper cardinality",
            })?;
            validate_event_projection(
                mapper,
                profile.ok_or(request::Error::Drift {
                    field: "builder profile mapper proof",
                })?,
            )?;
            continue;
        }
        let argument = *arguments.next().ok_or(request::Error::Drift {
            field: "consuming builder argument cardinality",
        })? as usize;
        render_take(argument, argument, ty, output)?;
    }
    if arguments.next().is_some() || mappers.next().is_some() {
        return Err(request::Error::Drift {
            field: "consuming builder argument cardinality",
        });
    }
    Ok(())
}

fn builder_function_argument_list(
    inputs: &[convert::Type],
    mappers: &[crate::surface::adapter::EventMapper],
    arguments: &[u32],
    replay: bool,
    profile: Option<&crate::profile::Proof>,
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    let mut mappers = mappers.iter().peekable();
    let mut arguments = arguments.iter();
    for (position, ty) in inputs.iter().enumerate() {
        if position != 0 {
            output
                .write_all(b", ")
                .map_err(|_| request::Error::Encoding)?;
        }
        let mapped_here = mappers
            .peek()
            .is_some_and(|mapper| usize::try_from(mapper.position()).ok() == Some(position));
        if mapped_here {
            if !matches!(ty, convert::Type::FunctionItem { .. }) {
                return Err(request::Error::Drift {
                    field: "builder profile mapper position",
                });
            }
            let mapper = mappers.next().ok_or(request::Error::Drift {
                field: "builder profile mapper cardinality",
            })?;
            validate_event_projection(
                mapper,
                profile.ok_or(request::Error::Drift {
                    field: "builder profile mapper proof",
                })?,
            )?;
            output
                .write_all(b"__rawr_profile::payload")
                .map_err(|_| request::Error::Encoding)?;
        } else {
            let argument = arguments.next().ok_or(request::Error::Drift {
                field: "consuming builder argument cardinality",
            })?;
            if replay && !copy_scalar(ty) {
                write!(output, "argument_{argument}.clone()")
                    .map_err(|_| request::Error::Encoding)?;
            } else {
                write!(output, "argument_{argument}").map_err(|_| request::Error::Encoding)?;
            }
        }
    }
    if arguments.next().is_some() || mappers.next().is_some() {
        return Err(request::Error::Drift {
            field: "consuming builder argument cardinality",
        });
    }
    Ok(())
}

fn render_builder_arguments_iter<'a>(
    inputs: impl ExactSizeIterator<Item = &'a convert::Type>,
    arguments: &[u32],
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    if inputs.len() != arguments.len() {
        return Err(request::Error::Drift {
            field: "consuming builder argument cardinality",
        });
    }
    for (ty, position) in inputs.zip(arguments) {
        let position = *position as usize;
        render_take(position, position, ty, output)?;
    }
    Ok(())
}

fn builder_argument_list(arguments: &[u32], output: &mut Vec<u8>) -> Result<(), request::Error> {
    for (index, position) in arguments.iter().enumerate() {
        if index != 0 {
            output
                .write_all(b", ")
                .map_err(|_| request::Error::Encoding)?;
        }
        write!(output, "argument_{position}").map_err(|_| request::Error::Encoding)?;
    }
    Ok(())
}

fn builder_typed_argument_list(
    inputs: &[convert::Type],
    arguments: &[u32],
    replay: bool,
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    if inputs.len() != arguments.len() {
        return Err(request::Error::Drift {
            field: "consuming builder argument cardinality",
        });
    }
    for (index, (ty, position)) in inputs.iter().zip(arguments).enumerate() {
        if index != 0 {
            output
                .write_all(b", ")
                .map_err(|_| request::Error::Encoding)?;
        }
        if replay && !copy_scalar(ty) {
            write!(output, "argument_{position}.clone()").map_err(|_| request::Error::Encoding)?;
        } else {
            write!(output, "argument_{position}").map_err(|_| request::Error::Encoding)?;
        }
    }
    Ok(())
}

fn render_builder_constructor(
    form: crate::api::model::FieldForm,
    fields: &[crate::surface::adapter::Field],
    arguments: &[u32],
    replay: bool,
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    match form {
        crate::api::model::FieldForm::Unit => Ok(()),
        crate::api::model::FieldForm::Tuple => {
            output
                .write_all(b"(")
                .map_err(|_| request::Error::Encoding)?;
            if replay {
                builder_typed_argument_list(
                    &fields
                        .iter()
                        .map(|field| field.ty().clone())
                        .collect::<Vec<_>>(),
                    arguments,
                    true,
                    output,
                )?;
            } else {
                builder_argument_list(arguments, output)?;
            }
            output.write_all(b")").map_err(|_| request::Error::Encoding)
        }
        crate::api::model::FieldForm::Record => {
            output
                .write_all(b" { ")
                .map_err(|_| request::Error::Encoding)?;
            for (index, (field, position)) in fields.iter().zip(arguments).enumerate() {
                if index != 0 {
                    output
                        .write_all(b", ")
                        .map_err(|_| request::Error::Encoding)?;
                }
                if replay && !copy_scalar(field.ty()) {
                    write!(output, "{}: argument_{position}.clone()", field.name())
                        .map_err(|_| request::Error::Encoding)?;
                } else {
                    write!(output, "{}: argument_{position}", field.name())
                        .map_err(|_| request::Error::Encoding)?;
                }
            }
            output
                .write_all(b" }")
                .map_err(|_| request::Error::Encoding)
        }
    }
}

fn rust_path(plan: &Plan) -> String {
    let mut path = String::from(plan.export().root_alias());
    for segment in plan.export().path() {
        path.push_str("::");
        path.push_str(segment);
    }
    if !path.contains(crate::api::ledger::CRATE_SEGMENT) {
        return path;
    }
    // A macro's `$crate` path, which Rust cannot write: the root's own
    // spelling of the item, or of the type an associated item is on.
    if let Some(spelled) = plan
        .selector()
        .and_then(|selector| ITEM_PATHS.with_borrow(|paths| paths.get(selector).cloned()))
        .filter(|_| plan.export().path().last() == Some(plan.name()))
    {
        return spelled;
    }
    PATH_IDENTITIES
        .with_borrow(|identities| identities.get(&path).copied())
        .and_then(|identity| TYPE_PATHS.with_borrow(|paths| paths.get(&identity).cloned()))
        .unwrap_or_else(|| {
            // A trait reached through a macro's `$crate` is imported as
            // a direct dependency by `render_source`. Its provided method
            // uses that dependency's root, just as its trait import does.
            let segments = path.split("::").collect::<Vec<_>>();
            match segments
                .iter()
                .position(|segment| segment.starts_with(crate::api::ledger::CRATE_SEGMENT))
            {
                Some(at) => format!("::{}", segments[at..].join("::")),
                None => path,
            }
        })
}

fn render_constructor(
    form: crate::api::model::FieldForm,
    fields: &[crate::surface::adapter::Field],
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    match form {
        crate::api::model::FieldForm::Unit => Ok(()),
        crate::api::model::FieldForm::Tuple => {
            output
                .write_all(b"(")
                .map_err(|_| request::Error::Encoding)?;
            for (index, field) in fields.iter().enumerate() {
                if index != 0 {
                    output
                        .write_all(b", ")
                        .map_err(|_| request::Error::Encoding)?;
                }
                render_field_value(index, field, output)?;
            }
            output.write_all(b")").map_err(|_| request::Error::Encoding)
        }
        crate::api::model::FieldForm::Record => {
            output
                .write_all(b" { ")
                .map_err(|_| request::Error::Encoding)?;
            for (index, field) in fields.iter().enumerate() {
                if index != 0 {
                    output
                        .write_all(b", ")
                        .map_err(|_| request::Error::Encoding)?;
                }
                write!(output, "{}: ", field.name()).map_err(|_| request::Error::Encoding)?;
                render_field_value(index, field, output)?;
            }
            output
                .write_all(b" }")
                .map_err(|_| request::Error::Encoding)
        }
    }
}

/// A constructor's value for one field: the argument taken for it, or for
/// a field of the unit type, as palette's `standard: PhantomData<S>` is to
/// Rawr, the field type's default, which `()` and `PhantomData` both have.
fn render_field_value(
    index: usize,
    field: &crate::surface::adapter::Field,
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    if matches!(field.ty(), convert::Type::Unit) {
        write!(
            output,
            "{{ let () = argument_{index}; ::core::default::Default::default() }}"
        )
    } else {
        write!(output, "argument_{index}")
    }
    .map_err(|_| request::Error::Encoding)
}

#[derive(Clone, Copy)]
enum Replay<'a> {
    Visual,
    Message(&'a convert::Type),
}

fn replay<'a>(
    plan: &'a Plan,
    profile: &crate::profile::Proof,
) -> Result<Option<Replay<'a>>, request::Error> {
    let Some((output, signature, substitutions)) = projected_output(plan) else {
        return Ok(None);
    };
    let output = substitute(output, signature, substitutions)?;
    let convert::Type::Nominal {
        definition,
        arguments,
    } = output
    else {
        return Ok(None);
    };
    let selected = profile
        .projections()
        .iter()
        .filter_map(|projection| {
            profile
                .source_for_projection(projection)
                .map(|source| (projection, source))
        })
        .find(|(_, source)| source.source_identity() == definition.as_bytes());
    let Some((projection, _source)) = selected else {
        let Some(mapper) = event_mapper(plan)? else {
            return Ok(None);
        };
        validate_event_projection(mapper, profile)?;
        let message = projected_event_message(arguments, signature, substitutions, mapper.seal())?;
        return Ok(Some(Replay::Message(message)));
    };
    let [argument] = projection.arguments() else {
        return Err(request::Error::Drift {
            field: "generated projection target argument cardinality",
        });
    };
    let Some(source) = argument.source_index() else {
        return Ok(Some(Replay::Visual));
    };
    let source = usize::try_from(source).map_err(|_| request::Error::Drift {
        field: "generated projection source argument",
    })?;
    let message = arguments.get(source).ok_or(request::Error::Drift {
        field: "generated projection source argument",
    })?;
    Ok(Some(Replay::Message(substitute(
        message,
        signature,
        substitutions,
    )?)))
}

// A method consumes an existing mapped value; it does not acquire a callback formal.
fn method_message<'a>(
    plan: &'a Plan,
    cohort: &[Plan],
    profile: &crate::profile::Proof,
    facade: Option<&crate::surface::facade::Plan>,
) -> Result<Option<&'a convert::Type>, request::Error> {
    let Operation::Method {
        receiver: Receiver::Owned,
        signature,
        substitutions,
        constants,
        container_parameters,
        ..
    } = plan.operation()
    else {
        return Ok(None);
    };
    let convert::Type::Nominal {
        definition,
        arguments,
    } = substitute(signature.output(), signature, substitutions)?
    else {
        return Ok(None);
    };
    let mut message = None;
    for argument in arguments {
        let argument = substitute(argument, signature, substitutions)?;
        if matches!(argument, convert::Type::ProfileMessage { .. })
            && message.replace(argument).is_some()
        {
            return Err(request::Error::Drift {
                field: "event continuation Message cardinality",
            });
        }
    }
    let Some(message @ convert::Type::ProfileMessage { mapper, .. }) = message else {
        return Ok(None);
    };
    let fail = || request::Error::Drift {
        field: "event continuation receiver proof",
    };
    let facade = facade.ok_or_else(fail)?;
    let mut owners = facade
        .facades()
        .filter(|f| f.root() == plan.export().root_alias())
        .flat_map(|f| f.types())
        .filter(|ty| {
            // Adapter exports name the receiver container; the selected method
            // is retained separately in plan.name() and its semantic selector.
            ty.path().split("::").eq(plan.export().path())
        });
    let owner = owners.next().ok_or_else(fail)?;
    if owners.next().is_some()
        || owner.identity() != definition.as_bytes()
        || owner.parameters().len() != *container_parameters
        || arguments.len() != *container_parameters
        || !constants.is_empty()
    {
        return Err(fail());
    }
    // The output must preserve every concrete receiver parameter, not merely Message.
    for (argument, receiver) in arguments.iter().zip(substitutions) {
        if substitute(argument, signature, substitutions)? != receiver {
            return Err(fail());
        }
    }
    let mut selected = None;
    for producer in cohort {
        let Some(witness) = event_mapper(producer)? else {
            continue;
        };
        if witness.seal() != mapper {
            continue;
        }
        validate_event_projection(witness, profile)?;
        if witness.projection().source() != *definition {
            continue;
        }
        let Some((ty, producer_signature, producer_substitutions)) = projected_output(producer)
        else {
            continue;
        };
        let convert::Type::Nominal {
            definition: source,
            arguments: source_arguments,
        } = substitute(ty, producer_signature, producer_substitutions)?
        else {
            continue;
        };
        if source != definition || source_arguments.len() != arguments.len() {
            continue;
        }
        let mut same = true;
        for (left, right) in arguments.iter().zip(source_arguments) {
            same &= substitute(left, signature, substitutions)?
                == substitute(right, producer_signature, producer_substitutions)?;
        }
        if !same {
            continue;
        }
        if selected.is_some_and(|previous| previous != witness.projection()) {
            return Err(request::Error::Drift {
                field: "ambiguous event continuation producer",
            });
        }
        selected = Some(witness.projection());
    }
    if selected.is_none() {
        return Err(request::Error::Drift {
            field: "missing event continuation producer",
        });
    }
    Ok(Some(message))
}

fn projected_event_message<'a>(
    arguments: &'a [convert::Type],
    signature: &'a crate::classify::representability::signature::Signature,
    substitutions: &'a [convert::Type],
    mapper: &[u8; 16],
) -> Result<&'a convert::Type, request::Error> {
    let mut selected = None;
    for argument in arguments {
        let argument = substitute(argument, signature, substitutions)?;
        if !matches!(argument, convert::Type::ProfileMessage { mapper: seal, .. } if seal == mapper)
        {
            continue;
        }
        if selected.replace(argument).is_some() {
            return Err(request::Error::Drift {
                field: "generated event projection Message cardinality",
            });
        }
    }
    selected.ok_or(request::Error::Drift {
        field: "generated event projection Message",
    })
}

fn event_mapper(
    plan: &Plan,
) -> Result<Option<&crate::surface::adapter::EventMapper>, request::Error> {
    fn retain<'a>(
        selected: &mut Option<&'a crate::surface::adapter::EventMapper>,
        mappers: &'a [crate::surface::adapter::EventMapper],
    ) -> Result<(), request::Error> {
        for mapper in mappers {
            if selected.replace(mapper).is_some() {
                return Err(request::Error::Drift {
                    field: "generated event projection mapper cardinality",
                });
            }
        }
        Ok(())
    }

    let mut selected = None;
    match plan.operation() {
        Operation::Function { mappers, .. } => retain(&mut selected, mappers)?,
        Operation::Builder { steps } => {
            for step in steps {
                if let Operation::Function { mappers, .. } = step.plan().operation() {
                    retain(&mut selected, mappers)?;
                }
            }
        }
        Operation::Declaration { .. }
        | Operation::StructConstructor { .. }
        | Operation::StructField { .. }
        | Operation::EnumVariant { .. }
        | Operation::Method { .. }
        | Operation::Constant { .. } => {}
    }
    Ok(selected)
}

fn projected_output(
    plan: &Plan,
) -> Option<(
    &convert::Type,
    &crate::classify::representability::signature::Signature,
    &[convert::Type],
)> {
    match plan.operation() {
        Operation::Function {
            signature,
            substitutions,
            ..
        }
        | Operation::Method {
            signature,
            substitutions,
            ..
        } => Some((signature.output(), signature, substitutions)),
        Operation::Builder { steps } => {
            let final_step = steps.last()?;
            match final_step.plan().operation() {
                Operation::Function {
                    signature,
                    substitutions,
                    ..
                }
                | Operation::Method {
                    signature,
                    substitutions,
                    ..
                } => Some((signature.output(), signature, substitutions)),
                Operation::Declaration { .. }
                | Operation::StructConstructor { .. }
                | Operation::StructField { .. }
                | Operation::EnumVariant { .. }
                | Operation::Constant { .. }
                | Operation::Builder { .. } => None,
            }
        }
        Operation::Declaration { .. }
        | Operation::StructConstructor { .. }
        | Operation::StructField { .. }
        | Operation::EnumVariant { .. }
        | Operation::Constant { .. } => None,
    }
}

fn render_replay_insert(
    replay: Replay<'_>,
    facade: Option<&crate::surface::facade::Plan>,
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    match replay {
        Replay::Visual => output
            .write_all(
                b"    __rawr_profile::insert_visual(arguments.arena, output).map(Dynamic::from)\n",
            )
            .map_err(|_| request::Error::Encoding),
        Replay::Message(message) => {
            output
                .write_all(b"    __rawr_profile::insert_projectable::<_, ")
                .map_err(|_| request::Error::Encoding)?;
            render_rust_type(
                match message {
                    convert::Type::ProfileMessage { payload, .. } => payload,
                    message => message,
                },
                facade.ok_or(request::Error::Drift {
                    field: "generated projection facade plan",
                })?,
                output,
            )?;
            output
                .write_all(b">(arguments.arena, output")
                .map_err(|_| request::Error::Encoding)?;
            if let convert::Type::ProfileMessage { mapper, .. } = message {
                output
                    .write_all(b", &")
                    .map_err(|_| request::Error::Encoding)?;
                byte_slice(output, mapper)?;
            }
            output
                .write_all(b").map(Dynamic::from)\n")
                .map_err(|_| request::Error::Encoding)
        }
    }
}

fn render_rust_type(
    ty: &convert::Type,
    facade: &crate::surface::facade::Plan,
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    match ty {
        convert::Type::Script { .. } => output
            .write_all(
                concrete_type(ty)
                    .ok_or(request::Error::Drift {
                        field: "a script type's proxy",
                    })?
                    .as_bytes(),
            )
            .map_err(|_| request::Error::Encoding),
        convert::Type::Inferred => output.write_all(b"_").map_err(|_| request::Error::Encoding),
        convert::Type::Unit => output
            .write_all(b"()")
            .map_err(|_| request::Error::Encoding),
        convert::Type::Builtin(convert::Builtin::InclusiveRange) => output
            .write_all(b"std::ops::RangeInclusive<i64>")
            .map_err(|_| request::Error::Encoding),
        convert::Type::Builtin(convert::Builtin::String | convert::Builtin::Text) => output
            .write_all(b"String")
            .map_err(|_| request::Error::Encoding),
        convert::Type::Builtin(convert::Builtin::Str) => output
            .write_all(b"&str")
            .map_err(|_| request::Error::Encoding),
        convert::Type::Builtin(builtin) => output
            .write_all(builtin.rust_name().unwrap_or_default().as_bytes())
            .map_err(|_| request::Error::Encoding),
        convert::Type::Nominal { definition, .. } if is_opaque(definition) => {
            output.write_all(b"_").map_err(|_| request::Error::Encoding)
        }
        convert::Type::Nominal {
            definition,
            arguments,
        } => {
            let ty = facade
                .facades()
                .flat_map(crate::surface::facade::Facade::types)
                .find(|ty| ty.identity() == definition.as_bytes())
                .ok_or(request::Error::Drift {
                    field: "generated projection message type",
                })?;
            output
                .write_all(ty.path().as_bytes())
                .map_err(|_| request::Error::Encoding)?;
            if !arguments.is_empty() {
                output
                    .write_all(b"<")
                    .map_err(|_| request::Error::Encoding)?;
                for (index, argument) in arguments.iter().enumerate() {
                    if index != 0 {
                        output
                            .write_all(b", ")
                            .map_err(|_| request::Error::Encoding)?;
                    }
                    render_rust_type(argument, facade, output)?;
                }
                output
                    .write_all(b">")
                    .map_err(|_| request::Error::Encoding)?;
            }
            Ok(())
        }
        convert::Type::FunctionItem {
            input,
            output: result,
        } => {
            output
                .write_all(b"fn(")
                .map_err(|_| request::Error::Encoding)?;
            render_rust_type(input, facade, output)?;
            output
                .write_all(b") -> ")
                .map_err(|_| request::Error::Encoding)?;
            render_rust_type(result, facade, output)
        }
        convert::Type::ProfileMessage { payload, .. } => render_rust_type(payload, facade, output),
        convert::Type::Borrowed { mutable, ty } => {
            output
                .write_all(if *mutable { b"&mut " } else { b"&" })
                .map_err(|_| request::Error::Encoding)?;
            render_rust_type(ty, facade, output)
        }
        convert::Type::Option(element) => {
            output
                .write_all(b"Option<")
                .map_err(|_| request::Error::Encoding)?;
            render_rust_type(element, facade, output)?;
            output.write_all(b">").map_err(|_| request::Error::Encoding)
        }
        convert::Type::Range(element) => {
            output
                .write_all(b"std::ops::Range<")
                .map_err(|_| request::Error::Encoding)?;
            render_rust_type(element, facade, output)?;
            output.write_all(b">").map_err(|_| request::Error::Encoding)
        }
        convert::Type::Result(ok, err) => {
            output
                .write_all(b"Result<")
                .map_err(|_| request::Error::Encoding)?;
            render_rust_type(ok, facade, output)?;
            output
                .write_all(b", ")
                .map_err(|_| request::Error::Encoding)?;
            render_rust_type(err, facade, output)?;
            output.write_all(b">").map_err(|_| request::Error::Encoding)
        }
        convert::Type::Sequence {
            element,
            length: Some(length),
            ..
        } => {
            output
                .write_all(b"[")
                .map_err(|_| request::Error::Encoding)?;
            render_rust_type(element, facade, output)?;
            write!(output, "; {length}]").map_err(|_| request::Error::Encoding)
        }
        convert::Type::Sequence { element, .. } => {
            output
                .write_all(b"Vec<")
                .map_err(|_| request::Error::Encoding)?;
            render_rust_type(element, facade, output)?;
            output.write_all(b">").map_err(|_| request::Error::Encoding)
        }
        convert::Type::Tuple(members) => {
            output
                .write_all(b"(")
                .map_err(|_| request::Error::Encoding)?;
            for member in members {
                render_rust_type(member, facade, output)?;
                output
                    .write_all(b", ")
                    .map_err(|_| request::Error::Encoding)?;
            }
            output.write_all(b")").map_err(|_| request::Error::Encoding)
        }
        convert::Type::Parameter(_) => Err(request::Error::Drift {
            field: "unconstrained generated projection message",
        }),
    }
}

fn render_arguments_with_profile(
    signature: &crate::classify::representability::signature::Signature,
    substitutions: &[convert::Type],
    offset: usize,
    profile: &crate::profile::Proof,
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    for (index, ty) in signature.inputs().iter().enumerate() {
        render_take_profile(
            index,
            index + offset,
            substitute(ty, signature, substitutions)?,
            profile,
            output,
        )?;
    }
    Ok(())
}

fn dynamic_function_arity(
    signature: &crate::classify::representability::signature::Signature,
    functions: &[crate::surface::adapter::FunctionItem],
    mappers: &[crate::surface::adapter::EventMapper],
) -> Result<usize, request::Error> {
    let mut dynamic = 0_usize;
    let mut selected = functions.iter().peekable();
    let mut mapped = mappers.iter().peekable();
    for (position, ty) in signature.inputs().iter().enumerate() {
        let selected_here = selected
            .peek()
            .is_some_and(|function| usize::try_from(function.position()).ok() == Some(position));
        let mapped_here = mapped
            .peek()
            .is_some_and(|mapper| usize::try_from(mapper.position()).ok() == Some(position));
        match (ty, selected_here, mapped_here) {
            (ty, true, false)
                if matches!(ty, convert::Type::FunctionItem { .. })
                    || callable_parameter(ty, signature) =>
            {
                selected.next();
            }
            (convert::Type::FunctionItem { .. }, false, true) => {
                mapped.next();
            }
            (convert::Type::FunctionItem { .. }, false, false)
            | (_, true, true)
            | (_, true, false)
            | (_, false, true) => {
                return Err(request::Error::Drift {
                    field: "generated static operand position",
                });
            }
            _ => {
                dynamic = dynamic.checked_add(1).ok_or(request::Error::Limit {
                    resource: "adapter argument cardinality",
                    limit: u32::MAX as usize,
                    actual: usize::MAX,
                })?;
            }
        }
    }
    if selected.next().is_some() || mapped.next().is_some() {
        return Err(request::Error::Drift {
            field: "generated static operand cardinality",
        });
    }
    Ok(dynamic)
}

#[allow(clippy::too_many_arguments)]
/// The script functions a call's callable parameters take as slots, and
/// what each parameter takes and gives (D594).
#[derive(Clone, Copy, Default)]
struct Crossed<'a> {
    slots: &'a [crate::surface::instance::Slot],
    crossings: &'a [crate::classify::representability::callable::Call],
}

impl<'a> Crossed<'a> {
    /// The call of the slot at input `position`, when one fills it.
    fn slot_at(
        &self,
        position: usize,
        ty: &convert::Type,
    ) -> Option<&'a crate::classify::representability::callable::Call> {
        let convert::Type::Parameter(parameter) = ty else {
            return None;
        };
        self.slots
            .iter()
            .any(|slot| usize::try_from(slot.position()).ok() == Some(position))
            .then(|| {
                self.crossings
                    .iter()
                    .find(|call| call.parameter() == *parameter)
            })
            .flatten()
    }
}

/// A script function that fills a callable parameter as a slot (D594): a
/// function item `slot_{local}` the call takes, which calls the function
/// in the generation current where native code calls it, through a static
/// the adapter fills with the function each time script code passes it. The
/// item captures nothing, so it is zero-sized, and each script function
/// has its own adapter and so its own item and Rust type.
///
/// The item lends a script value it takes by reference: `&mut` as the
/// function's own place, and shared by its handle, so nothing is copied.
/// An output that borrows what it takes, as `view(&state) -> Element<'_>`
/// gives, is made in a region at that borrow's lifetime, where a script
/// string the function's native calls borrow is pinned in the lent value.
fn render_slot(
    local: usize,
    index: usize,
    call: &crate::classify::representability::callable::Call,
    signature: &crate::classify::representability::signature::Signature,
    substitutions: &[convert::Type],
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    let drift = |field| request::Error::Drift { field };
    let inputs = call
        .inputs()
        .iter()
        .map(|input| substituted(input, signature, substitutions))
        .collect::<Result<Vec<_>, _>>()?;
    let returned = substituted(call.output(), signature, substitutions)?;
    // An output that borrows the inputs, at the slot's `'x`.
    // An output that borrows the inputs, as the bound writes it or as the
    // type the instance gives a synthetic output is (`W` of iced's
    // `ViewFn`, a `Column<'a, ..>`).
    let family = if call.borrows() || carries_lifetime(&returned) {
        Some(family_of(&returned).ok_or(drift("a slot's borrowing output"))?)
    } else {
        None
    };
    let lifetime = if family.is_some() { "'x " } else { "" };
    let mut parameters = Vec::new();
    let mut values = Vec::new();
    let mut mutable = Vec::new();
    let mut restores = String::new();
    let mut pins = None;
    let mut native = false;
    let mut lends = Vec::new();
    for (position, input) in inputs.iter().enumerate() {
        let name = format!("input_{position}");
        match input {
            convert::Type::Borrowed { mutable: true, ty }
                if matches!(**ty, convert::Type::Script { .. }) =>
            {
                let rust = concrete_type(ty).ok_or(drift("a slot's input"))?;
                parameters.push(format!("{name}: &{lifetime}mut {rust}"));
                values.push(format!("{name}.0.lend_mut()"));
                mutable.push(position.to_string());
                restores.push_str(&format!(
                    "\n        {name}.0.restore(std::mem::take(&mut values[{position}]));"
                ));
            }
            convert::Type::Borrowed { mutable: false, ty }
                if matches!(**ty, convert::Type::Script { .. }) =>
            {
                let rust = concrete_type(ty).ok_or(drift("a slot's input"))?;
                parameters.push(format!("{name}: &{lifetime}{rust}"));
                values.push(format!("{name}.0.lend()"));
                pins.get_or_insert_with(|| format!("{name}.0.pins()"));
            }
            // A shared borrow of a native value, as iced's `pick_list`
            // gives `font::Family::to_string` a `&Family`: a copy, as a
            // closure's borrowed native input crosses
            // (`render_callable_closure`), so script code reads it as the
            // value it is, `Display` too.
            convert::Type::Borrowed { mutable: false, ty }
                if family.is_none()
                    && matches!(**ty, convert::Type::Nominal { .. })
                    && !is_value(ty)
                    && !carries_lifetime(ty) =>
            {
                let rust = concrete_type(ty).ok_or(drift("a slot's input"))?;
                parameters.push(format!("{name}: &{rust}"));
                let mut converted = String::new();
                render_to_dynamic(ty, &format!("Clone::clone({name})"), 0, &mut converted)?;
                native = true;
                values.push(format!(
                    "(({converted}).and_then(|value| attest_input(arguments.arena, value, witnesses.get({position}))))"
                ));
            }
            // A native value the function borrows `&mut`: lent for the
            // call alone, as a script impl's function is (D558.3).
            convert::Type::Borrowed {
                mutable: borrowed_mutably,
                ty,
            } if family.is_none()
                && matches!(**ty, convert::Type::Nominal { .. })
                && !is_value(ty)
                && !carries_lifetime(ty) =>
            {
                let rust = concrete_type(ty).ok_or(drift("a slot's input"))?;
                let borrow = if *borrowed_mutably { "&mut " } else { "&" };
                parameters.push(format!("{name}: {borrow}{rust}"));
                values.push(lent_value(position, *borrowed_mutably, &rust));
                lends.push((position, *borrowed_mutably, rust));
            }
            convert::Type::Borrowed { .. } => {
                return Err(drift(
                    "a slot's input borrows a value that isn't a script value",
                ));
            }
            input => {
                let rust = concrete_type(input).ok_or(drift("a slot's input"))?;
                parameters.push(format!("{name}: {rust}"));
                let mut converted = String::new();
                render_to_dynamic(input, &name, 0, &mut converted)?;
                // A crate value, alone or inside an `Option`, a `Result`,
                // a `Vec` or a tuple, enters the arena.
                let crate_value = holds_native(input);
                native |= crate_value;
                // A native value takes the script function's parameter's
                // witness, so script code reads it as its type, as a
                // variant's field of an imported type is read.
                if crate_value {
                    converted = format!(
                        "({converted}).and_then(|value| attest_input(arguments.arena, value, witnesses.get({position})))"
                    );
                }
                values.push(format!("({converted})"));
            }
        }
    }
    let in_arena = |body: &str| {
        format!(
            "with_arena(|arena| {{ let mut none: [&mut Dynamic; 0] = []; let arguments = &mut Arguments {{ values: &mut none, arena }}; let _ = &arguments; {body} }})"
        )
    };
    let values = if native {
        // Each converted input is a `Result`, which the arena's closure
        // gives back by `?`.
        let values = values
            .iter()
            .map(|value| {
                if value.starts_with('(') {
                    format!("{value}?")
                } else {
                    value.clone()
                }
            })
            .collect::<Vec<_>>();
        format!(
            "{}.unwrap_or_else(|error| panic!(\"rawr: a value native code gave a script function can't cross: {{error:?}}\"))",
            in_arena(&format!(
                "let witnesses = WITNESSES_{local}.read().unwrap_or_else(std::sync::PoisonError::into_inner).clone(); Ok::<_, Error>([{}])",
                values.join(", ")
            ))
        )
    } else {
        let values = values
            .iter()
            .map(|value| {
                if value.starts_with('(') {
                    format!("{value}.unwrap_or_else(|error| panic!(\"rawr: a value native code gave a script function can't cross: {{error:?}}\"))")
                } else {
                    value.clone()
                }
            })
            .collect::<Vec<_>>();
        format!("[{}]", values.join(", "))
    };
    let mutable = mutable.join(", ");
    let (generics, rust, body) = match &family {
        Some(family) => {
            let pins = pins.ok_or(drift("a slot's borrowing output with no script value lent"))?;
            (
                "<'x>",
                format!("<{family} as __rawr_internal::scope::Family>::Of<'x>"),
                format!(
                    "let mut values = {values};
        let mut region = __rawr_internal::scope::Region::anchored({pins});
        let value = __rawr_internal::scope::open_anchored(&mut region, || SLOT_{local}.invoke(&mut values, &[{mutable}]));{restores}
        let scoped = value
            .read_lock::<__rawr_internal::scope::Scoped>()
            .map(|scoped| *scoped)
            .unwrap_or_else(|| panic!(\"rawr: a script function gave native code a value that doesn't borrow what it was lent\"));
        region
            .take::<{family}>(scoped)
            .unwrap_or_else(|error| panic!(\"rawr: {{error}}\"))"
                ),
            )
        }
        None => {
            let rust = concrete_type(&returned).ok_or(drift("a slot's output"))?;
            let mut converted = String::new();
            render_from_dynamic(&returned, "value", 0, &mut converted)?;
            let native_output = holds_native(&returned);
            // A script value made from native inputs on a thread with no
            // arena is made where it is first used (`DeferProbe`).
            let deferred = if native
                && matches!(returned, convert::Type::Script { .. })
                && !inputs
                    .iter()
                    .any(|input| matches!(input, convert::Type::Borrowed { .. }))
            {
                let names = (0..inputs.len())
                    .map(|position| format!("input_{position},"))
                    .collect::<String>();
                format!(
                    "let ({names}) = if arena_access(|_| ()).is_none() {{
            let probe = DeferProbe(std::cell::Cell::new(Some(({names}))));
            if let Some(proxy) = (&probe).defer(|({names})| slot_{local}({names}).0.into_dynamic()) {{
                return <{rust}>::from_proxy(proxy);
            }}
            probe.0.take().expect(\"rawr: a slot's inputs are kept until deferred\")
        }} else {{
            ({names})
        }};
        "
                )
            } else {
                String::new()
            };
            // On a thread with no arena, as iced's executor runs
            // `Task::map`'s function and iced's `listen_with`, native
            // values move in and out through an arena of the call's own
            // (`CallArena`). A script value made of native inputs is made
            // where it is used instead (`deferred`), when it can be.
            let call_arena = if native || native_output {
                "let _arena = CallArena::enter();\n        "
            } else {
                ""
            };
            // A conversion that gives its errors back by `?`, as an
            // `Option` output's does, runs in a closure of its own.
            let converted = if native_output {
                in_arena(&converted)
            } else {
                format!("(move || {converted})()")
            };
            let invoked = format!(
                "let mut values = {values};
        let value = SLOT_{local}.invoke(&mut values, &[{mutable}]);{restores}
        value"
            );
            let (items, invoked) = lent_around(&lends, invoked);
            (
                "",
                rust,
                format!(
                    "{items}{deferred}{call_arena}let value = {{ {invoked} }};
        {converted}.unwrap_or_else(|error| panic!(\"rawr: a script function gave back a value native code can't take: {{error:?}}\"))"
                ),
            )
        }
    };
    // The witnesses of the script function's parameters, read where
    // script code passes it, which its native inputs take.
    let witnesses = if native {
        format!(
            "
    static WITNESSES_{local}: std::sync::RwLock<Vec<Option<__rawr_internal::host::runtime::Witness>>> = std::sync::RwLock::new(Vec::new());
    *WITNESSES_{local}.write().unwrap_or_else(std::sync::PoisonError::into_inner) = closure_inputs({index}, {});",
            inputs.len()
        )
    } else {
        String::new()
    };
    writeln!(
        output,
        "    static SLOT_{local}: __rawr_internal::crossing::Slot = __rawr_internal::crossing::Slot::new();
    SLOT_{local}
        .set((**arguments.values.get_mut({index}).ok_or(Error::MissingArgument)?).take())
        .map_err(|error| Error::Value(__rawr_internal::future::message(&error)))?;{witnesses}
    #[allow(unused_mut, clippy::needless_lifetimes)]
    fn slot_{local}{generics}({}) -> {rust} {{
        {body}
    }}",
        parameters.join(", ")
    )
    .map_err(|_| request::Error::Encoding)
}

#[allow(clippy::too_many_arguments)]
fn render_function_arguments(
    signature: &crate::classify::representability::signature::Signature,
    substitutions: &[convert::Type],
    functions: &[crate::surface::adapter::FunctionItem],
    mappers: &[crate::surface::adapter::EventMapper],
    profile: Option<&crate::profile::Proof>,
    call: Option<&crate::classify::representability::callable::Call>,
    crossed: Crossed<'_>,
    offset: usize,
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    let mut runtime = offset;
    let mut selected = functions.iter().peekable();
    let mut mapped = mappers.iter().peekable();
    for (position, ty) in signature.inputs().iter().enumerate() {
        let selected_here = selected
            .peek()
            .is_some_and(|function| usize::try_from(function.position()).ok() == Some(position));
        let mapped_here = mapped
            .peek()
            .is_some_and(|mapper| usize::try_from(mapper.position()).ok() == Some(position));
        match (ty, selected_here, mapped_here) {
            (convert::Type::FunctionItem { .. }, true, false) => {
                selected.next();
            }
            // A crate function filling a callable parameter a script
            // closure would, as `.style(container::rounded_box)`: the Rust
            // function itself, with no runtime argument.
            (ty, true, false) if callable_parameter(ty, signature) => {
                let function = selected.next().ok_or(request::Error::Drift {
                    field: "generated static function item cardinality",
                })?;
                write!(output, "    let argument_{position} = ")
                    .map_err(|_| request::Error::Encoding)?;
                render_function_callable(function.plan(), output)?;
                output
                    .write_all(b";\n")
                    .map_err(|_| request::Error::Encoding)?;
            }
            (convert::Type::FunctionItem { .. }, false, true) => {
                let mapper = mapped.next().ok_or(request::Error::Drift {
                    field: "generated profile mapper cardinality",
                })?;
                validate_event_projection(
                    mapper,
                    profile.ok_or(request::Error::Drift {
                        field: "generated profile mapper proof",
                    })?,
                )?;
            }
            (convert::Type::FunctionItem { .. }, false, false)
            | (_, true, true)
            | (_, true, false)
            | (_, false, true) => {
                return Err(request::Error::Drift {
                    field: "generated static operand position",
                });
            }
            _ => {
                if let Some(slot) = crossed.slot_at(position, ty) {
                    render_slot(position, runtime, slot, signature, substitutions, output)?;
                } else if let Some(call) =
                    call.filter(|call| *ty == convert::Type::Parameter(call.parameter()))
                {
                    render_take_callable(
                        position,
                        runtime,
                        call,
                        signature,
                        substitutions,
                        output,
                    )?;
                } else if let Some(call) = call.filter(|call| optional_callable(ty, call)) {
                    render_take_optional_callable(
                        position,
                        runtime,
                        call,
                        signature,
                        substitutions,
                        output,
                    )?;
                } else {
                    render_take_input(
                        position,
                        runtime,
                        ty,
                        signature,
                        substitutions,
                        profile,
                        output,
                    )?;
                }
                runtime = runtime.checked_add(1).ok_or(request::Error::Limit {
                    resource: "adapter argument cardinality",
                    limit: u32::MAX as usize,
                    actual: usize::MAX,
                })?;
            }
        }
    }
    if selected.next().is_some() || mapped.next().is_some() {
        return Err(request::Error::Drift {
            field: "generated static operand cardinality",
        });
    }
    Ok(())
}

fn function_argument_list(
    signature: &crate::classify::representability::signature::Signature,
    substitutions: &[convert::Type],
    functions: &[crate::surface::adapter::FunctionItem],
    mappers: &[crate::surface::adapter::EventMapper],
    profile: Option<&crate::profile::Proof>,
    crossed: Crossed<'_>,
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    let mut selected = functions.iter().peekable();
    let mut mapped = mappers.iter().peekable();
    for (position, ty) in signature.inputs().iter().enumerate() {
        if position != 0 {
            output
                .write_all(b", ")
                .map_err(|_| request::Error::Encoding)?;
        }
        let selected_here = selected
            .peek()
            .is_some_and(|function| usize::try_from(function.position()).ok() == Some(position));
        let mapped_here = mapped
            .peek()
            .is_some_and(|mapper| usize::try_from(mapper.position()).ok() == Some(position));
        match (ty, selected_here, mapped_here) {
            (convert::Type::FunctionItem { .. }, true, false) => {
                let function = selected.next().ok_or(request::Error::Drift {
                    field: "generated static function item cardinality",
                })?;
                render_function_callable(function.plan(), output)?;
            }
            // A crate function filling a callable parameter, which
            // `render_function_arguments` bound to its local.
            (ty, true, false) if callable_parameter(ty, signature) => {
                selected.next();
                write!(output, "argument_{position}").map_err(|_| request::Error::Encoding)?;
            }
            (convert::Type::FunctionItem { .. }, false, true) => {
                let mapper = mapped.next().ok_or(request::Error::Drift {
                    field: "generated profile mapper cardinality",
                })?;
                validate_event_projection(
                    mapper,
                    profile.ok_or(request::Error::Drift {
                        field: "generated profile mapper proof",
                    })?,
                )?;
                output
                    .write_all(b"__rawr_profile::payload")
                    .map_err(|_| request::Error::Encoding)?;
            }
            (convert::Type::FunctionItem { .. }, false, false)
            | (_, true, true)
            | (_, true, false)
            | (_, false, true) => {
                return Err(request::Error::Drift {
                    field: "generated static operand position",
                });
            }
            // A script function, as a slot (D594).
            _ if crossed.slot_at(position, ty).is_some() => {
                write!(output, "slot_{position}").map_err(|_| request::Error::Encoding)?;
            }
            // A script closure, made a Rust closure (D513.7 b), or an
            // `Option` of one.
            _ if callable_parameter(ty, signature)
                || matches!(ty, convert::Type::Option(inner) if callable_parameter(inner, signature)) =>
            {
                write!(output, "argument_{position}").map_err(|_| request::Error::Encoding)?;
            }
            _ => {
                let ty = substitute(ty, signature, substitutions)?;
                if profile.is_some_and(|proof| is_projection_target(ty, proof)) {
                    write!(output, "argument_{position}.lower(_context)")
                        .map_err(|_| request::Error::Encoding)?;
                } else if profile.is_some() && !copy_scalar(ty) {
                    write!(output, "argument_{position}.clone()")
                        .map_err(|_| request::Error::Encoding)?;
                } else {
                    write!(output, "argument_{position}").map_err(|_| request::Error::Encoding)?;
                }
            }
        }
    }
    if selected.next().is_some() || mapped.next().is_some() {
        return Err(request::Error::Drift {
            field: "generated static operand cardinality",
        });
    }
    Ok(())
}

fn validate_event_projection(
    mapper: &crate::surface::adapter::EventMapper,
    profile: &crate::profile::Proof,
) -> Result<(), request::Error> {
    let witness = mapper.projection();
    if witness.profile() != profile.identity().as_bytes()
        || witness.profile_version() != profile.version().get()
        || witness.profile_rules() != profile.rules_digest()
    {
        return Err(request::Error::Drift {
            field: "generated profile mapper identity",
        });
    }
    let mut projections = profile
        .projections()
        .iter()
        .filter(|projection| projection.selector() == witness.profile_projection());
    let projection = projections.next().ok_or(request::Error::Drift {
        field: "generated profile mapper projection",
    })?;
    if projections.next().is_some()
        || projection.target_identity() != witness.target().as_bytes()
        || profile
            .source_for_projection(projection)
            .is_none_or(|source| source.selector() != witness.profile_source())
    {
        return Err(request::Error::Drift {
            field: "generated profile mapper projection",
        });
    }
    Ok(())
}

fn render_function_callable(plan: &Plan, output: &mut Vec<u8>) -> Result<(), request::Error> {
    let Operation::Function {
        substitutions,
        constants,
        functions,
        mappers,
        container_parameters,
        associated,
        signature,
        ..
    } = plan.operation()
    else {
        return Err(request::Error::Drift {
            field: "generated static function item plan",
        });
    };
    // A generic function's turbofish names its instance's types.
    if !constants.is_empty() || !functions.is_empty() || !mappers.is_empty() {
        return Err(request::Error::Drift {
            field: "generated nested static function item",
        });
    }
    render_function_path(
        plan,
        signature,
        substitutions,
        constants,
        *container_parameters,
        *associated,
        output,
    )?;
    render_item_turbofish(
        signature,
        substitutions,
        constants,
        *container_parameters,
        output,
    )
}

fn render_take_profile(
    local: usize,
    index: usize,
    ty: &convert::Type,
    profile: &crate::profile::Proof,
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    if is_projection_target(ty, profile) {
        return writeln!(
            output,
            "    let argument_{local}: __rawr_profile_host::widget::Value = take_primitive(arguments, {index})?;"
        )
        .map_err(|_| request::Error::Encoding);
    }
    if matches!(ty, convert::Type::Builtin(convert::Builtin::InclusiveRange)) {
        return writeln!(
            output,
            "    let argument_{local}: std::ops::RangeInclusive<i64> = take_primitive(arguments, {index})?;"
        )
        .map_err(|_| request::Error::Encoding);
    }
    render_take(local, index, ty, output)
}

fn is_projection_target(ty: &convert::Type, profile: &crate::profile::Proof) -> bool {
    let convert::Type::Nominal { definition, .. } = ty else {
        return false;
    };
    profile
        .alias_targets()
        .iter()
        .any(|alias| alias.target_identity() == definition.as_bytes())
}

fn replay_argument_list(
    signature: &crate::classify::representability::signature::Signature,
    substitutions: &[convert::Type],
    profile: &crate::profile::Proof,
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    for (index, ty) in signature.inputs().iter().enumerate() {
        if index != 0 {
            output
                .write_all(b", ")
                .map_err(|_| request::Error::Encoding)?;
        }
        let ty = substitute(ty, signature, substitutions)?;
        if is_projection_target(ty, profile) {
            write!(output, "argument_{index}.lower(_context)")
                .map_err(|_| request::Error::Encoding)?;
        } else if copy_scalar(ty) {
            write!(output, "argument_{index}").map_err(|_| request::Error::Encoding)?;
        } else {
            write!(output, "argument_{index}.clone()").map_err(|_| request::Error::Encoding)?;
        }
    }
    Ok(())
}

fn render_arguments_with(
    signature: &crate::classify::representability::signature::Signature,
    substitutions: &[convert::Type],
    offset: usize,
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    for (index, ty) in signature.inputs().iter().enumerate() {
        render_take_input(
            index,
            index + offset,
            ty,
            signature,
            substitutions,
            None,
            output,
        )?;
    }
    Ok(())
}

/// [`render_take`] of an input declared `ty`, at its instance's type. An
/// input of a type parameter names that type where it is taken: an
/// argument's `impl Trait` has no turbofish to name it (E0632), as
/// Freya's `width(impl Into<Size>)` has none.
fn render_take_input(
    local: usize,
    index: usize,
    ty: &convert::Type,
    signature: &crate::classify::representability::signature::Signature,
    substitutions: &[convert::Type],
    profile: Option<&crate::profile::Proof>,
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    // `&mut Vec<T>` of scalars, or `&mut String`: a copy of Rawr's array or
    // string the call changes, which goes back after it (D558 slice 2).
    if let convert::Type::Borrowed {
        mutable: true,
        ty: element,
    } = ty
        && is_copied(element)
    {
        let mut converted = String::new();
        render_from_dynamic(element, "value", 0, &mut converted)?;
        return writeln!(
            output,
            "    let mut argument_{local}_copy = {{ let value = (**arguments.values.get_mut({index}).ok_or(Error::MissingArgument)?).take().flatten(); {converted}? }};\n    #[allow(unused_variables)]\n    let argument_{local} = &mut argument_{local}_copy;"
        )
        .map_err(|_| request::Error::Encoding);
    }
    // `&[T]` or `&(A, B)` of Rawr's array or tuple, as voronator's
    // `from_tuple` and image's `save_buffer` take: the Rust value made from
    // it, lent to the call, as a `&str` input is.
    if let convert::Type::Borrowed {
        mutable: false,
        ty: lent,
    } = ty
        && is_made_for_call(lent)
    {
        let mut converted = String::new();
        render_from_dynamic(lent, "value", 0, &mut converted)?;
        return writeln!(
            output,
            "    let argument_{local}_made = {{ let value = (**arguments.values.get_mut({index}).ok_or(Error::MissingArgument)?).take().flatten(); {converted}? }};\n    let argument_{local} = &argument_{local}_made;"
        )
        .map_err(|_| request::Error::Encoding);
    }
    if let convert::Type::Borrowed { .. } = ty {
        return render_lease(
            local,
            index,
            &substituted(ty, signature, substitutions)?,
            output,
        );
    }
    // An array of a scope frame's region's values, each moved out of it
    // into the call, as `Column::with_children` takes `column![..]`'s
    // elements.
    if REGION_ARGUMENTS.get()
        && let convert::Type::Sequence {
            element,
            owned: true,
            length: None,
        } = substituted(ty, signature, substitutions)?
        && let Some(family) = family_of(&element)
    {
        // An element made outside any region is in the arena, at
        // `'static`, which the region's lifetime shortens when the type is
        // covariant in it, as rustc's variance says. An invariant one, as
        // iced's `Text<'a>` is through `Theme::Class<'a>`, can't be: Rust
        // makes each element of a view at the view's lifetime, as Rawr
        // makes it in the region, and one made before can't join them.
        let rust = concrete_type(&element).ok_or(request::Error::Drift {
            field: "a region array's element",
        })?;
        let arena = if shortens(&element) {
            format!("item.try_cast_result::<NativeHandle>().map_err(|_| Error::TypeMismatch).and_then(|handle| arguments.arena.take::<{rust}>(handle))")
        } else {
            format!(
                "Err(Error::Value({:?}.to_owned()))",
                format!("`{rust}` is invariant in its lifetime, so one made outside the view can't join those the view makes")
            )
        };
        return writeln!(
            output,
            "    let argument_{local} = (**arguments.values.get_mut({index}).ok_or(Error::MissingArgument)?).take().into_array().map_err(|_| Error::TypeMismatch).and_then(|items| items.into_iter().map(|item| {{ let scoped = item.read_lock::<__rawr_internal::scope::Scoped>().map(|scoped| *scoped); match scoped {{ Some(scoped) => region.take::<{family}>(scoped).map_err(|error| Error::Lent(scoped.name(), error)), None => {arena} }} }}).collect::<Result<Vec<_>, Error>>())?;"
        )
        .map_err(|_| request::Error::Encoding);
    }
    // A value of a scope frame's region, moved out of it into the call
    // (D558 tier 1), as `push_html` takes a `Parser`.
    if REGION_ARGUMENTS.get()
        && let Some(family) = family_of(&substituted(ty, signature, substitutions)?)
    {
        return writeln!(
            output,
            "    let argument_{local} = {{ let scoped = scoped_value(arguments, {index}).ok_or(Error::TypeMismatch)?; region.take::<{family}>(scoped).map_err(|error| Error::Lent(scoped.name(), error))? }};"
        )
        .map_err(|_| request::Error::Encoding);
    }
    // A type parameter a `&str` fills, as a string literal fills iced's
    // `button(content: W)`: a `&str` input.
    if matches!(ty, convert::Type::Parameter(_)) && lends_str(ty, signature, substitutions) {
        return render_take(
            local,
            index,
            &convert::Type::Builtin(convert::Builtin::Str),
            output,
        );
    }
    let generic = matches!(ty, convert::Type::Parameter(_));
    // An array of a type parameter's values, as `impl IntoIterator<Item =
    // impl Into<..>>` takes, or an `Option` of one, as iced's `pick_list(
    // selected: Option<V>, ..)` takes a script value's, at the type the
    // call's instance gives them.
    let deep;
    let ty = if matches!(
        ty,
        convert::Type::Sequence { .. } | convert::Type::Option(_)
    ) || range_inclusive_element(ty).is_some()
    {
        deep = substituted(ty, signature, substitutions)?;
        &deep
    } else {
        substitute(ty, signature, substitutions)?
    };
    let mut taken = Vec::new();
    match profile {
        Some(profile) => render_take_profile(local, index, ty, profile, &mut taken)?,
        None => render_take(local, index, ty, &mut taken)?,
    }
    // A crate type of no arguments is written out too: a trait's function
    // the call reaches through one impl, as `Background::from(color)` of
    // `impl From<Color> for Background`, takes the impl's type, which
    // rustc can't infer beside the reflexive `From<Background>`.
    let plain = matches!(ty, convert::Type::Nominal { arguments, .. } if arguments.is_empty())
        && !is_value(ty)
        && !is_future(ty);
    let annotation = (generic || plain)
        .then(|| concrete_type(ty))
        .flatten()
        .filter(|annotation| !is_value(ty) && annotation != "_");
    let Some(annotation) = annotation else {
        return output
            .write_all(&taken)
            .map_err(|_| request::Error::Encoding);
    };
    let taken = String::from_utf8(taken)
        .map_err(|_| request::Error::Encoding)?
        .replacen(
            &format!("let argument_{local} = "),
            &format!("let argument_{local}: {annotation} = "),
            1,
        );
    output
        .write_all(taken.as_bytes())
        .map_err(|_| request::Error::Encoding)
}

/// A `&T` or `&mut T` input of a value in the arena, as ratatui's
/// `render_stateful_widget` takes `&mut ListState`: the value leaves its
/// slot for the call, which refuses every other use meanwhile (E0499), and
/// goes back when the lease drops, before the arena is next used (D558.5).
fn render_lease(
    local: usize,
    index: usize,
    ty: &convert::Type,
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    let convert::Type::Borrowed { mutable, ty } = ty else {
        return Err(request::Error::Drift {
            field: "a lease of other than a borrow",
        });
    };
    let rust = concrete_type(ty).ok_or(request::Error::Drift {
        field: "borrowed input type",
    })?;
    let borrow = if *mutable { "" } else { "&*" };
    if LENT_ARGUMENTS.get() && !carries_lifetime(ty) {
        // Or a value native code lent a marked function (D558.3), reached
        // through its accessor, which the call runs inside.
        let (reference, reach) = if *mutable {
            ("&mut ", "mutable")
        } else {
            ("&", "shared")
        };
        return writeln!(
            output,
            "    #[allow(unused_mut)]
    let mut argument_{local}_held: Held<{rust}>;
    let argument_{local}: {reference}{rust} = match lent_argument(lent, {index}) {{
        Some(value) => value.{reach}::<{rust}>().ok_or(Error::Lent({rust:?}, __rawr_internal::scope::Error::Mismatch))?,
        None => {{
            argument_{local}_held = {{ let handle = native_handle(arguments, {index}, false)?; arguments.arena.lease(handle)? }};
            {borrow}argument_{local}_held.get_mut()
        }}
    }};"
        )
        .map_err(|_| request::Error::Encoding);
    }
    writeln!(
        output,
        "    let mut argument_{local}_held: Held<{rust}> = {{ let handle = native_handle(arguments, {index}, false)?; arguments.arena.lease(handle)? }};\n    let argument_{local} = {borrow}argument_{local}_held.get_mut();"
    )
    .map_err(|_| request::Error::Encoding)
}

/// Whether `ty` is a callable type parameter of `signature`, as `F` of
/// `F: FnOnce() -> R` is.
fn callable_parameter(
    ty: &convert::Type,
    signature: &crate::classify::representability::signature::Signature,
) -> bool {
    let convert::Type::Parameter(index) = ty else {
        return false;
    };
    signature.parameters().iter().any(|parameter| {
        parameter.index() == *index
            && parameter.kind() == crate::classify::representability::signature::Kind::Callable
    })
}

/// A script closure where a generic item takes `F: FnOnce(..) -> R`
/// (D513.7 b), or `F: Fn(..) -> R + 'static`: a Rust closure that calls it,
/// on whichever thread calls the closure, with an evaluation of its own
/// (D490). Its inputs and output cross by value. A native value crosses
/// through the arena, which is the owner thread's: there a native call lends
/// it to the closures it calls back, as Freya's `launch` calls `fn app()`
/// (D551.6). On another thread such a closure panics with the reason.
fn render_take_callable(
    local: usize,
    index: usize,
    call: &crate::classify::representability::callable::Call,
    signature: &crate::classify::representability::signature::Signature,
    substitutions: &[convert::Type],
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    let closure = render_callable_closure(
        index,
        &format!("(**arguments.values.get_mut({index}).ok_or(Error::MissingArgument)?).take()"),
        call,
        signature,
        substitutions,
    )?;
    writeln!(output, "    let argument_{local} = {closure};").map_err(|_| request::Error::Encoding)
}

/// Whether `ty` is an `Option` of the callable parameter `call` fills, as
/// `on_toggle_maybe(f: Option<F>)` takes it.
fn optional_callable(
    ty: &convert::Type,
    call: &crate::classify::representability::callable::Call,
) -> bool {
    matches!(ty, convert::Type::Option(inner)
        if **inner == convert::Type::Parameter(call.parameter()))
}

/// An `Option` of a script closure where a generic item takes
/// `Option<F>`: `None`, or `Some` of the Rust closure that calls it, as
/// [`render_take_callable`] makes.
fn render_take_optional_callable(
    local: usize,
    index: usize,
    call: &crate::classify::representability::callable::Call,
    signature: &crate::classify::representability::signature::Signature,
    substitutions: &[convert::Type],
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    let closure = render_callable_closure(index, "inner", call, signature, substitutions)?;
    writeln!(
        output,
        "    let argument_{local} = match __rawr_internal::option::take(&mut **arguments.values.get_mut({index}).ok_or(Error::MissingArgument)?).ok_or(Error::TypeMismatch)? {{\n        None => None,\n        Some(inner) => Some({closure}),\n    }};"
    )
    .map_err(|_| request::Error::Encoding)
}

/// The block that makes a Rust closure calling the script callable
/// `source` gives, for [`render_take_callable`].
fn render_callable_closure(
    index: usize,
    source: &str,
    call: &crate::classify::representability::callable::Call,
    signature: &crate::classify::representability::signature::Signature,
    substitutions: &[convert::Type],
) -> Result<String, request::Error> {
    let by_value = |ty: &convert::Type| {
        !matches!(ty, convert::Type::Nominal { .. }) || is_value(ty) || is_duration(ty)
    };
    let inputs = call
        .inputs()
        .iter()
        .map(|input| substituted(input, signature, substitutions))
        .collect::<Result<Vec<_>, _>>()?;
    let returned = substituted(call.output(), signature, substitutions)?;
    // A native value, or a borrow of one, which crosses as a copy.
    let native = |ty: &convert::Type| match ty {
        convert::Type::Borrowed { ty, .. } => !by_value(ty),
        ty => !by_value(ty),
    };
    let native_inputs = inputs.iter().any(native);
    let native_output = !by_value(&returned) || holds_native(&returned);
    let mut parameters = Vec::new();
    let mut values = Vec::new();
    for (position, input) in inputs.iter().enumerate() {
        let rust = concrete_type(input).ok_or(request::Error::Drift {
            field: "closure input type",
        })?;
        parameters.push(format!("input_{position}: {rust}"));
        let mut converted = String::new();
        render_to_dynamic(input, &format!("input_{position}"), 0, &mut converted)?;
        // A native value takes the closure parameter's witness, so script
        // code reads it as its type, as a `match` on an imported enum does.
        if native(input) {
            converted = format!(
                "({converted}).and_then(|value| attest_input(arguments.arena, value, witnesses.get({position})))"
            );
        }
        values.push(if native_inputs {
            format!("({converted})?")
        } else {
            format!("({converted}).unwrap_or_else(|error| panic!(\"rawr: a closure's input can't cross: {{error:?}}\"))")
        });
    }
    let rust = concrete_type(&returned).ok_or(request::Error::Drift {
        field: "closure output type",
    })?;
    let mut converted = String::new();
    render_from_dynamic(&returned, "value", 0, &mut converted)?;
    // A script value native code gives the closure is checked against the
    // closure's parameter (review 2).
    let (witnesses, call) = if inputs.iter().any(is_value) {
        (
            format!(
                "\n        let witnesses = closure_inputs({index}, {});",
                inputs.len()
            ),
            "callable.call_checked(&mut values, &witnesses)",
        )
    } else if native_inputs {
        (
            format!(
                "\n        let witnesses = closure_inputs({index}, {});",
                inputs.len()
            ),
            "callable.call(&mut values)",
        )
    } else {
        (String::new(), "callable.call(&mut values)")
    };
    // Native values go in and out through the arena, with `arguments` as an
    // adapter has it.
    let in_arena = |body: &str| {
        format!(
            "with_arena(|arena| {{ let mut none: [&mut Dynamic; 0] = []; let arguments = &mut Arguments {{ values: &mut none, arena }}; let _ = &arguments; {body} }})"
        )
    };
    let values = if native_inputs {
        format!(
            "{}.unwrap_or_else(|error| panic!(\"rawr: a closure's input can't cross: {{error:?}}\"))",
            in_arena(&format!("Ok([{}])", values.join(", ")))
        )
    } else {
        format!("[{}]", values.join(", "))
    };
    let converted = if native_output {
        in_arena(&converted)
    } else {
        format!("({converted})")
    };
    // An arena of the call's own on a thread with none (`CallArena`).
    let call_arena = if native_inputs || native_output {
        "let _arena = CallArena::enter();\n            "
    } else {
        ""
    };
    Ok(format!(
        "{{\n        let callable = __rawr_internal::future::Callable::new({source})\n            .map_err(|error| Error::Value(__rawr_internal::future::message(&error)))?;{witnesses}\n        move |{}| -> {rust} {{\n            {call_arena}let mut values = {values};\n            let value = {call}.unwrap_or_else(|error| panic!(\"{{}}\", __rawr_internal::future::message(&error)));\n            {converted}.unwrap_or_else(|error| panic!(\"rawr: a closure gave back a value native code can't take: {{error:?}}\"))\n        }}\n    }}",
        parameters.join(", "),
    ))
}

fn render_fields(
    fields: &[crate::surface::adapter::Field],
    offset: usize,
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    for (index, field) in fields.iter().enumerate() {
        render_take(index, index + offset, field.ty(), output)?;
    }
    Ok(())
}

fn render_take(
    local: usize,
    index: usize,
    ty: &convert::Type,
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    // A `&'static str` input pins the string taken from Rawr for as long as
    // the runner lives, as a string literal's is.
    if matches!(ty, convert::Type::Builtin(convert::Builtin::StaticStr))
        || (PINNED_STRINGS.get() && matches!(ty, convert::Type::Builtin(convert::Builtin::Str)))
    {
        return writeln!(
            output,
            "    let argument_{local} = static_pins().pin(take_primitive::<ImmutableString>(arguments, {index})?);"
        )
        .map_err(|_| request::Error::Encoding);
    }
    // A `&str` input lends the string taken from Rawr, which a region can
    // pin by its shared handle: no text is copied.
    if matches!(ty, convert::Type::Builtin(convert::Builtin::Str)) {
        return writeln!(
            output,
            "    let argument_{local}_owned = take_primitive::<ImmutableString>(arguments, {index})?;\n    let argument_{local} = argument_{local}_owned.as_str();"
        )
        .map_err(|_| request::Error::Encoding);
    }
    let helper = match ty {
        convert::Type::Builtin(convert::Builtin::String) => "take_string",
        convert::Type::Builtin(convert::Builtin::InclusiveRange) => "take_range",
        convert::Type::Builtin(builtin) if builtin.is_narrow_integer() => "take_int",
        convert::Type::Builtin(convert::Builtin::F32) => "take_f32",
        ty if is_duration(ty) || is_facade_value(ty) => "take_primitive",
        ty if is_value(ty) => "take_value",
        // A script value where native generic code takes a Rust type: the
        // runner's proxy for its type (D594).
        convert::Type::Script { .. } => {
            let rust = concrete_type(ty).ok_or(request::Error::Drift {
                field: "a script type's proxy",
            })?;
            return writeln!(
                output,
                "    let argument_{local} = <{rust}>::from_proxy(__rawr_internal::crossing::Proxy::from_place(&mut **arguments.values.get_mut({index}).ok_or(Error::MissingArgument)?));"
            )
            .map_err(|_| request::Error::Encoding);
        }
        // A script future, run as a std future (D527).
        ty if is_future(ty) => "take_script",
        // Rawr's float or integer range, as the range of the number the
        // instance gives.
        ty if range_inclusive_element(ty).is_some() => {
            let mut converted = String::new();
            render_from_dynamic(ty, "value", 0, &mut converted)?;
            return writeln!(
                output,
                "    let argument_{local} = {{ let value = (**arguments.values.get_mut({index}).ok_or(Error::MissingArgument)?).take(); {converted} }}?;"
            )
            .map_err(|_| request::Error::Encoding);
        }
        convert::Type::Nominal { .. } | convert::Type::Parameter(_) => "take_native",
        // A slice of script values a type argument names, as an owned
        // `Vec` of their proxies (`script_slice`).
        convert::Type::Borrowed { mutable: false, ty } if let Some(element) = script_slice(ty) => {
            let owned = convert::Type::Sequence {
                element: Box::new(element.clone()),
                owned: true,
                length: None,
            };
            return render_take(local, index, &owned, output);
        }
        convert::Type::FunctionItem { .. } | convert::Type::Borrowed { .. } => {
            return Err(request::Error::Drift {
                field: "static function item entered runtime argument decoding",
            });
        }
        convert::Type::ProfileMessage { payload, .. } => {
            return render_take(local, index, payload, output);
        }
        // A Rawr array an instance takes as a `Vec`, as ratatui's
        // `List::new(rows)` does.
        convert::Type::Option(_)
        | convert::Type::Result(..)
        | convert::Type::Range(_)
        | convert::Type::Sequence { .. }
        | convert::Type::Tuple(_) => {
            // A Rawr `Option` of the converted `T`, a Rawr `Result` of the
            // converted `T` and `E`, or a float range.
            let mut converted = String::new();
            render_from_dynamic(ty, "value", 0, &mut converted)?;
            return writeln!(
                output,
                "    let argument_{local} = {{ let value = (**arguments.values.get_mut({index}).ok_or(Error::MissingArgument)?).take(); {converted} }}?;"
            )
            .map_err(|_| request::Error::Encoding);
        }
        convert::Type::Inferred => {
            return Err(request::Error::Drift {
                field: "an inferred type parameter reached a runtime value",
            })
        }
        convert::Type::Unit | convert::Type::Builtin(_) => "take_primitive",
    };
    writeln!(
        output,
        "    let argument_{local} = {helper}(arguments, {index})?;"
    )
    .map_err(|_| request::Error::Encoding)
}

fn render_give(
    ty: &convert::Type,
    value: &str,
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    match ty {
        ty if is_duration(ty) || is_facade_value(ty) => {
            writeln!(output, "    Ok(Dynamic::from({value}))")
        }
        ty if is_value(ty) => writeln!(output, "    Ok({value}.into_inner())"),
        convert::Type::Script { .. } => writeln!(output, "    Ok({value}.0.into_dynamic())"),
        // A value that carries the lifetime of the scope frame's region the
        // call runs in stays there (D558 tier 1).
        convert::Type::Nominal { .. } if IN_REGION.get() && carries_lifetime(ty) => {
            let family = family_of(ty).ok_or(request::Error::Drift {
                field: "a scoped value's family",
            })?;
            writeln!(
                output,
                "    Ok(Dynamic::from(region.insert::<{family}>({value})))"
            )
        }
        // A value whose type holds an opaque type carries the calls that
        // take it, compiled here (`opaque_table_source`), or those of the
        // value of the same type the call took.
        convert::Type::Nominal { .. } if holds_opaque(ty) => {
            let table = if OPAQUE_CONSUMING.get() {
                Some("table".to_owned())
            } else {
                OPAQUE_PRODUCING.with_borrow(|table| {
                    table.as_ref().map(|table| table.replace("__RAWR_VALUE__", value))
                })
            };
            match table {
                Some(table) => writeln!(
                    output,
                    "    let table = {table};\n    let handle = insert_native!(arguments.arena, {value})?;\n    arguments.arena.set_opaque(&handle, table);\n    Ok(Dynamic::from(handle))"
                ),
                None => writeln!(
                    output,
                    "    insert_native!(arguments.arena, {value}).map(Dynamic::from)"
                ),
            }
        }
        convert::Type::Nominal { .. } | convert::Type::Parameter(_) => {
            writeln!(
                output,
                "    insert_native!(arguments.arena, {value}).map(Dynamic::from)"
            )
        }
        convert::Type::Builtin(builtin) if builtin.is_narrow_integer() => {
            writeln!(output, "    int_into({value}).map(Dynamic::from)")
        }
        convert::Type::Builtin(convert::Builtin::F32) => {
            writeln!(output, "    Ok(Dynamic::from(f64::from({value})))")
        }
        // An output Rawr cannot hold: its `Display` text.
        convert::Type::Builtin(convert::Builtin::Text) => {
            writeln!(output, "    Ok(Dynamic::from({value}.to_string()))")
        }
        // The unit type, or a `PhantomData` field, which Rawr holds as it.
        convert::Type::Inferred => {
            return Err(request::Error::Drift {
                field: "an inferred type parameter reached a runtime value",
            })
        }
        convert::Type::Unit => {
            writeln!(output, "    let _ = {value};\n    Ok(Dynamic::from(()))")
        }
        convert::Type::Builtin(_) => {
            writeln!(output, "    Ok(Dynamic::from({value}))")
        }
        // A `&[T]` a method lends: a copy, as Rawr's array.
        convert::Type::Borrowed {
            mutable: false,
            ty: lent,
        } if matches!(**lent, convert::Type::Sequence { .. }) => {
            return render_give(lent, value, output);
        }
        // A `&str` a method lends, as url's `Url::scheme`: the text,
        // copied where the borrow ends (`lent_copy`).
        convert::Type::Borrowed {
            mutable: false,
            ty: lent,
        } if **lent == convert::Type::Builtin(convert::Builtin::Str) => {
            writeln!(output, "    Ok(Dynamic::from(String::from({value})))")
        }
        // A `&T` given to a script closure: a copy.
        convert::Type::Borrowed {
            mutable: false,
            ty: lent,
        } if matches!(
            **lent,
            convert::Type::Nominal { .. } | convert::Type::Builtin(_)
        ) && **lent != convert::Type::Builtin(convert::Builtin::Str) =>
        {
            writeln!(output, "    let copied = Clone::clone({value});")
                .map_err(|_| request::Error::Encoding)?;
            return render_give(lent, "copied", output);
        }
        convert::Type::FunctionItem { .. } | convert::Type::Borrowed { .. } => {
            return Err(request::Error::Drift {
                field: "static function item entered runtime output encoding",
            });
        }
        convert::Type::ProfileMessage { payload, .. } => {
            return render_give(payload, value, output);
        }
        convert::Type::Option(_)
        | convert::Type::Result(..)
        | convert::Type::Range(_)
        | convert::Type::Sequence { .. }
        | convert::Type::Tuple(_) => {
            let mut converted = String::new();
            render_to_dynamic(ty, value, 0, &mut converted)?;
            writeln!(output, "    {converted}")
        }
    }
    .map_err(|_| request::Error::Encoding)
}

/// A generated `Result<T, Error>` expression converting the owned `Dynamic`
/// named `value` into the Rust value of `ty`.
fn render_from_dynamic(
    ty: &convert::Type,
    value: &str,
    depth: usize,
    output: &mut String,
) -> Result<(), request::Error> {
    use std::fmt::Write as _;
    let cast = |output: &mut String, rust: &str| {
        write!(
            output,
            "{value}.try_cast_result::<{rust}>().map_err(|_| Error::TypeMismatch)"
        )
    };
    match ty {
        convert::Type::Inferred => return Err(request::Error::Drift { field: "an inferred type parameter reached a runtime value" }),
        convert::Type::Unit => cast(output, "()"),
        convert::Type::Builtin(convert::Builtin::Bool) => cast(output, "bool"),
        convert::Type::Builtin(convert::Builtin::Char) => cast(output, "char"),
        convert::Type::Builtin(convert::Builtin::F64) => cast(output, "f64"),
        convert::Type::Builtin(convert::Builtin::I64) => cast(output, "i64"),
        convert::Type::Builtin(
            builtin @ (convert::Builtin::I8
                | convert::Builtin::I16
                | convert::Builtin::I32
                | convert::Builtin::Isize
                | convert::Builtin::U8
                | convert::Builtin::U16
                | convert::Builtin::U32
                | convert::Builtin::U64
                | convert::Builtin::Usize),
        ) => write!(
            output,
            "{value}.try_cast_result::<i64>().map_err(|_| Error::TypeMismatch).and_then(int_from::<{}>)",
            builtin.rust_name().unwrap_or_default()
        ),
        convert::Type::Builtin(convert::Builtin::F32) => write!(
            output,
            "{value}.try_cast_result::<f64>().map(|value| value as f32).map_err(|_| Error::TypeMismatch)"
        ),
        convert::Type::Builtin(convert::Builtin::InclusiveRange) => write!(
            output,
            "{value}.try_cast_result::<std::ops::RangeInclusive<i64>>().map_err(|_| Error::TypeMismatch).and_then(range_from)"
        ),
        // Rawr's `RangeInclusive<f64>` as Rust's of `f32` or `f64`, and its
        // `RangeInclusive<i64>` as one of the integer the instance gives.
        ty if let Some(element) = range_inclusive_element(ty) => match element {
            convert::Type::Builtin(convert::Builtin::F64) => {
                cast(output, "std::ops::RangeInclusive<f64>")
            }
            convert::Type::Builtin(convert::Builtin::F32) => write!(
                output,
                "{value}.try_cast_result::<std::ops::RangeInclusive<f64>>().map(|range| (*range.start() as f32)..=(*range.end() as f32)).map_err(|_| Error::TypeMismatch)"
            ),
            convert::Type::Builtin(builtin)
                if *builtin == convert::Builtin::I64 || builtin.is_narrow_integer() =>
            {
                write!(
                    output,
                    "{value}.try_cast_result::<std::ops::RangeInclusive<i64>>().map_err(|_| Error::TypeMismatch).and_then(range_from::<{}>)",
                    builtin.rust_name().unwrap_or_default()
                )
            }
            _ => {
                return Err(request::Error::Drift {
                    field: "an inclusive range of a type that is not a number",
                })
            }
        },
        convert::Type::Builtin(convert::Builtin::StaticStr) => write!(
            output,
            "{value}.try_cast_result::<ImmutableString>().map(|value| static_pins().pin(value)).map_err(|_| Error::TypeMismatch)"
        ),
        convert::Type::Builtin(
            convert::Builtin::String | convert::Builtin::Str | convert::Builtin::Text,
        ) => write!(
            output,
            "{value}.try_cast_result::<ImmutableString>().map(|value| value.to_string()).map_err(|_| Error::TypeMismatch)"
        ),
        ty if let Some(time) = std_time(ty) => cast(output, time),
        ty if is_facade_value(ty) => {
            let rust = concrete_type(ty).ok_or(request::Error::Drift {
                field: "a facade value's Rust type",
            })?;
            cast(output, &rust)
        }
        ty if is_value(ty) => write!(
            output,
            "Ok::<__rawr_internal::Value, Error>(__rawr_internal::Value::from({value}))"
        ),
        convert::Type::Script { .. } => {
            let rust = concrete_type(ty).ok_or(request::Error::Drift {
                field: "a script type's proxy",
            })?;
            write!(
                output,
                "Ok::<{rust}, Error>(<{rust}>::from_proxy(__rawr_internal::crossing::Proxy::new({value})))"
            )
        }
        convert::Type::Nominal { .. } | convert::Type::Parameter(_) => write!(
            output,
            "{value}.try_cast_result::<NativeHandle>().map_err(|_| Error::TypeMismatch).and_then(|handle| arguments.arena.take(handle))"
        ),
        convert::Type::ProfileMessage { payload, .. } => {
            return render_from_dynamic(payload, value, depth, output);
        }
        convert::Type::Option(element) => {
            let inner = format!("inner_{depth}");
            let mut converted = String::new();
            render_from_dynamic(element, &inner, depth + 1, &mut converted)?;
            write!(
                output,
                "{{ let mut {value} = {value}; match __rawr_internal::option::take(&mut {value}).ok_or(Error::TypeMismatch)? {{ None => Ok(None), Some({inner}) => ({converted}).map(Some) }} }}"
            )
        }
        // Rawr's `Range<f64>`, as Rust's `Range<f32>` or `Range<f64>`, and
        // its `Range<i64>`, as a range of an integer or of chars by their
        // code points.
        convert::Type::Range(element) => match element.as_ref() {
            convert::Type::Builtin(convert::Builtin::F64) => cast(output, "std::ops::Range<f64>"),
            convert::Type::Builtin(convert::Builtin::F32) => write!(
                output,
                "{value}.try_cast_result::<std::ops::Range<f64>>().map(|range| (range.start as f32)..(range.end as f32)).map_err(|_| Error::TypeMismatch)"
            ),
            convert::Type::Builtin(convert::Builtin::I64) => cast(output, "std::ops::Range<i64>"),
            convert::Type::Builtin(builtin) if builtin.is_narrow_integer() => write!(
                output,
                "{value}.try_cast_result::<std::ops::Range<i64>>().map_err(|_| Error::TypeMismatch).and_then(|range| Ok::<_, Error>(int_from::<{rust}>(range.start)?..int_from::<{rust}>(range.end)?))",
                rust = builtin.rust_name().unwrap_or_default()
            ),
            convert::Type::Builtin(convert::Builtin::Char) => write!(
                output,
                "{value}.try_cast_result::<std::ops::Range<i64>>().map_err(|_| Error::TypeMismatch).and_then(|range| {{ let bound = |code: i64| u32::try_from(code).ok().and_then(char::from_u32).ok_or(Error::TypeMismatch); Ok::<_, Error>(bound(range.start)?..bound(range.end)?) }})"
            ),
            _ => {
                return Err(request::Error::Drift {
                    field: "a range of other than floats entered runtime argument decoding",
                });
            }
        },
        convert::Type::Result(ok, err) => {
            let inner = format!("inner_{depth}");
            let mut ok_converted = String::new();
            render_from_dynamic(ok, &inner, depth + 1, &mut ok_converted)?;
            let mut err_converted = String::new();
            render_from_dynamic(err, &inner, depth + 1, &mut err_converted)?;
            write!(
                output,
                "{{ let mut {value} = {value}; match __rawr_internal::result::take(&mut {value}).ok_or(Error::TypeMismatch)? {{ Ok({inner}) => ({ok_converted}).map(Ok), Err({inner}) => ({err_converted}).map(Err) }} }}"
            )
        }
        // A `&[u8]` a type parameter takes by value, as an item of iced's
        // `fonts(impl IntoIterator<Item = impl Into<Cow<'static, [u8]>>>)`:
        // the bytes, kept for as long as the runner lives, once for each
        // content.
        convert::Type::Borrowed { mutable: false, ty }
            if matches!(ty.as_ref(), convert::Type::Sequence { element, .. }
                if **element == convert::Type::Builtin(convert::Builtin::U8)) =>
        {
            let mut converted = String::new();
            render_from_dynamic(ty, value, depth, &mut converted)?;
            write!(output, "({converted}).map(static_bytes)")
        }
        // A slice of script values a type argument names, as an owned
        // `Vec` of their proxies (`script_slice`).
        convert::Type::Borrowed { mutable: false, ty } if let Some(element) = script_slice(ty) => {
            let owned = convert::Type::Sequence {
                element: Box::new(element.clone()),
                owned: true,
                length: None,
            };
            return render_from_dynamic(&owned, value, depth, output);
        }
        convert::Type::FunctionItem { .. } | convert::Type::Borrowed { .. } => {
            return Err(request::Error::Drift {
                field: "static function item entered runtime argument decoding",
            });
        }
        // A Rawr tuple, an array of its members, as Rust's tuple of their
        // converted values.
        convert::Type::Tuple(members) => {
            let items = format!("items_{depth}");
            write!(
                output,
                "{value}.into_array().map_err(|_| Error::TypeMismatch).and_then(|{items}| {{ if {items}.len() != {} {{ return Err(Error::TypeMismatch); }} let mut {items} = {items}.into_iter(); Ok::<_, Error>((",
                members.len()
            )
            .map_err(|_| request::Error::Encoding)?;
            for (member, ty) in members.iter().enumerate() {
                let inner = format!("inner_{depth}_{member}");
                let mut converted = String::new();
                render_from_dynamic(ty, &inner, depth + 1, &mut converted)?;
                write!(
                    output,
                    "{{ let {inner} = {items}.next().ok_or(Error::TypeMismatch)?; ({converted})? }}, "
                )
                .map_err(|_| request::Error::Encoding)?;
            }
            write!(output, ")) }})")
        }
        // A Rawr array, as the `Vec` of its converted elements, or as the
        // Rust array of its length.
        convert::Type::Sequence {
            element, length, ..
        } => {
            let inner = format!("inner_{depth}");
            let mut converted = String::new();
            render_from_dynamic(element, &inner, depth + 1, &mut converted)?;
            let fixed = match length {
                Some(length) => format!(
                    ".and_then(|items| <[_; {length}]>::try_from(items).map_err(|_| Error::TypeMismatch))"
                ),
                None => String::new(),
            };
            write!(
                output,
                "{value}.into_array().map_err(|_| Error::TypeMismatch).and_then(|items| items.into_iter().map(|{inner}| {converted}).collect::<Result<Vec<_>, Error>>()){fixed}"
            )
        }
    }
    .map_err(|_| request::Error::Encoding)
}

/// A generated `Result<Dynamic, Error>` expression converting the Rust value
/// named `value` of `ty` into its Rawr value.
fn render_to_dynamic(
    ty: &convert::Type,
    value: &str,
    depth: usize,
    output: &mut String,
) -> Result<(), request::Error> {
    use std::fmt::Write as _;
    match ty {
        ty if is_duration(ty) || is_facade_value(ty) => {
            write!(output, "Ok::<Dynamic, Error>(Dynamic::from({value}))")
        }
        ty if is_value(ty) => write!(output, "Ok::<Dynamic, Error>({value}.into_inner())"),
        convert::Type::Script { .. } => {
            write!(output, "Ok::<Dynamic, Error>({value}.0.into_dynamic())")
        }
        // A value that borrows, made in a scope frame: its region holds it
        // (D558 tier 1).
        convert::Type::Nominal { .. } if IN_REGION.get() && carries_lifetime(ty) => {
            let family = family_of(ty).ok_or(request::Error::Drift {
                field: "a scoped value's family",
            })?;
            write!(
                output,
                "Ok::<Dynamic, Error>(Dynamic::from(region.insert::<{family}>({value})))"
            )
        }
        convert::Type::Nominal { .. } | convert::Type::Parameter(_) => write!(
            output,
            "insert_native!(arguments.arena, {value}).map(Dynamic::from)"
        ),
        convert::Type::Builtin(builtin) if builtin.is_narrow_integer() => {
            write!(output, "int_into({value}).map(Dynamic::from)")
        }
        convert::Type::Builtin(convert::Builtin::F32) => {
            write!(output, "Ok::<Dynamic, Error>(Dynamic::from(f64::from({value})))")
        }
        // An output Rawr keeps as its text, as a `Cow<str>` (D558 slice 2).
        convert::Type::Builtin(convert::Builtin::Text) => {
            write!(output, "Ok::<Dynamic, Error>(Dynamic::from({value}.to_string()))")
        }
        convert::Type::Inferred => return Err(request::Error::Drift { field: "an inferred type parameter reached a runtime value" }),
        convert::Type::Unit | convert::Type::Builtin(_) => {
            write!(output, "Ok::<Dynamic, Error>(Dynamic::from({value}))")
        }
        convert::Type::ProfileMessage { payload, .. } => {
            return render_to_dynamic(payload, value, depth, output);
        }
        convert::Type::Option(element) => {
            let inner = format!("inner_{depth}");
            let mut converted = String::new();
            render_to_dynamic(element, &inner, depth + 1, &mut converted)?;
            write!(
                output,
                "match {value} {{ None => Ok(__rawr_internal::option::none()), Some({inner}) => ({converted}).map(__rawr_internal::option::some) }}"
            )
        }
        convert::Type::Range(element) => match element.as_ref() {
            convert::Type::Builtin(convert::Builtin::F64) => {
                write!(output, "Ok::<Dynamic, Error>(Dynamic::from({value}))")
            }
            convert::Type::Builtin(convert::Builtin::F32) => write!(
                output,
                "{{ let range = {value}; Ok::<Dynamic, Error>(Dynamic::from(f64::from(range.start)..f64::from(range.end))) }}"
            ),
            convert::Type::Builtin(convert::Builtin::I64) => {
                write!(output, "Ok::<Dynamic, Error>(Dynamic::from({value}))")
            }
            convert::Type::Builtin(builtin) if builtin.is_narrow_integer() => write!(
                output,
                "{{ let range = {value}; int_into(range.start).and_then(|start| int_into(range.end).map(|end| Dynamic::from(start..end))) }}"
            ),
            convert::Type::Builtin(convert::Builtin::Char) => write!(
                output,
                "{{ let range = {value}; Ok::<Dynamic, Error>(Dynamic::from(i64::from(u32::from(range.start))..i64::from(u32::from(range.end)))) }}"
            ),
            _ => {
                return Err(request::Error::Drift {
                    field: "a range of other than floats entered runtime output encoding",
                });
            }
        },
        convert::Type::Result(ok, err) => {
            let inner = format!("inner_{depth}");
            let mut ok_converted = String::new();
            render_to_dynamic(ok, &inner, depth + 1, &mut ok_converted)?;
            let mut err_converted = String::new();
            render_to_dynamic(err, &inner, depth + 1, &mut err_converted)?;
            write!(
                output,
                "match {value} {{ Ok({inner}) => ({ok_converted}).map(__rawr_internal::result::ok), Err({inner}) => ({err_converted}).map(__rawr_internal::result::err) }}"
            )
        }
        // A `Vec`'s elements are moved out, a slice's cloned.
        convert::Type::Sequence { element, owned, .. } => {
            let inner = format!("inner_{depth}");
            let mut converted = String::new();
            render_to_dynamic(element, &inner, depth + 1, &mut converted)?;
            let elements = if *owned { "into_iter()" } else { "iter().cloned()" };
            write!(
                output,
                "{value}.{elements}.map(|{inner}| {converted}).collect::<Result<Vec<Dynamic>, Error>>().map(Dynamic::from_array)"
            )
        }
        convert::Type::Tuple(members) => {
            let names = (0..members.len())
                .map(|member| format!("inner_{depth}_{member}"))
                .collect::<Vec<_>>();
            write!(
                output,
                "(|| {{ let ({},) = {value}; Ok::<Dynamic, Error>(Dynamic::from_array(vec![",
                names.join(", ")
            )
            .map_err(|_| request::Error::Encoding)?;
            for (member, name) in members.iter().zip(&names) {
                let mut converted = String::new();
                render_to_dynamic(member, name, depth + 1, &mut converted)?;
                write!(output, "({converted})?, ").map_err(|_| request::Error::Encoding)?;
            }
            write!(output, "])) }})()")
        }
        // A `&[T]` a method lends: a copy, as Rawr's array.
        convert::Type::Borrowed { mutable: false, ty: lent }
            if matches!(**lent, convert::Type::Sequence { .. }) =>
        {
            return render_to_dynamic(lent, value, depth, output);
        }
        // A `&str`: its text.
        convert::Type::Borrowed { mutable: false, ty: lent }
            if **lent == convert::Type::Builtin(convert::Builtin::Str) =>
        {
            write!(output, "Ok::<Dynamic, Error>(Dynamic::from(String::from({value})))")
        }
        // A `&T` native code gives a script closure, as pane_grid's
        // `view(pane, &state, ..)` gives `&S`: a copy.
        convert::Type::Borrowed { mutable: false, ty: lent }
            if matches!(**lent, convert::Type::Nominal { .. } | convert::Type::Builtin(_))
                && **lent != convert::Type::Builtin(convert::Builtin::Str) =>
        {
            let copied = format!("copied_{depth}");
            let mut converted = String::new();
            render_to_dynamic(lent, &copied, depth + 1, &mut converted)?;
            write!(
                output,
                "{{ let {copied} = Clone::clone({value}); {converted} }}"
            )
        }
        convert::Type::FunctionItem { .. } | convert::Type::Borrowed { .. } => {
            return Err(request::Error::Drift {
                field: "static function item entered runtime output encoding",
            });
        }
    }
    .map_err(|_| request::Error::Encoding)
}

/// A Rust future, which no crate's type names.
fn is_future(ty: &convert::Type) -> bool {
    matches!(ty, convert::Type::Nominal { definition, .. }
        if *definition == crate::surface::identity::Definition::future())
}

/// A facade value, as `iced::Length`, which crosses as its Rust value.
fn is_facade_value(ty: &convert::Type) -> bool {
    matches!(ty, convert::Type::Nominal { definition, .. }
        if FACADE_VALUES.with_borrow(|values| values.contains(definition.as_bytes())))
}

/// Whether converting `ty` takes a native value out of the arena: a crate
/// value, alone or inside a tuple, as iced's boot `(State, Task<Message>)`.
fn holds_native(ty: &convert::Type) -> bool {
    match ty {
        convert::Type::Nominal { .. } => !is_value(ty) && !is_duration(ty),
        convert::Type::Tuple(members) => members.iter().any(holds_native),
        convert::Type::Option(element) | convert::Type::Sequence { element, .. } => {
            holds_native(element)
        }
        convert::Type::Result(ok, err) => holds_native(ok) || holds_native(err),
        _ => false,
    }
}

/// Rust's `std::time::Duration`, which crosses by value.
/// Whether `ty` is std's `Duration` or `Instant`, which cross by value as
/// Rawr's own.
fn is_duration(ty: &convert::Type) -> bool {
    std_time(ty).is_some()
}

/// The Rust path of std's `Duration` or `Instant`, when `ty` is one.
fn std_time(ty: &convert::Type) -> Option<&'static str> {
    use crate::surface::identity::Definition;
    match ty {
        convert::Type::Nominal { definition, .. } if *definition == Definition::duration() => {
            Some("std::time::Duration")
        }
        convert::Type::Nominal { definition, .. } if *definition == Definition::instant() => {
            Some("std::time::Instant")
        }
        _ => None,
    }
}

/// `T` of Rust's `RangeInclusive<T>` of a float or of a type parameter,
/// which crosses by value as Rawr's float or integer range.
fn range_inclusive_element(ty: &convert::Type) -> Option<&convert::Type> {
    match ty {
        convert::Type::Nominal {
            definition,
            arguments,
        } if *definition == crate::surface::identity::Definition::range_inclusive() => {
            match arguments.as_ref() {
                [element] => Some(element),
                _ => None,
            }
        }
        _ => None,
    }
}

/// The `Output` and `Progress` of an `impl Sipper<Output, Progress>` output.
fn sipper_output(ty: &convert::Type) -> Option<(&convert::Type, &convert::Type)> {
    let convert::Type::Nominal {
        definition,
        arguments,
    } = ty
    else {
        return None;
    };
    match arguments.as_ref() {
        [output, progress] if *definition == crate::surface::identity::Definition::sipper() => {
            Some((output, progress))
        }
        _ => None,
    }
}

/// Project a Rust sipper into iced's facade `task::Sipper`. Its progress and
/// its output become Rawr values on the UI thread, never in the stream.
fn render_give_sipper(
    sipped: &convert::Type,
    progress: &convert::Type,
    value: &str,
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    let mut on_progress = String::new();
    render_to_dynamic(progress, "value", 0, &mut on_progress)?;
    let mut on_output = String::new();
    render_to_dynamic(sipped, "value", 0, &mut on_output)?;
    writeln!(
        output,
        "    Ok(__rawr_profile_host::task::sipper(\n        {value},\n        |value| -> __rawr_profile_host::task::Materialize {{ Box::new(move |witness| materialize(witness, move |arguments| {on_progress})) }},\n        |value| -> __rawr_profile_host::task::Materialize {{ Box::new(move |witness| materialize(witness, move |arguments| {on_output})) }},\n    ))"
    )
    .map_err(|_| request::Error::Encoding)
}

/// The `T` of an output that is the iced facade's `Task<T>`.
fn facade_task_output<'a>(
    ty: &'a convert::Type,
    facade: Option<&crate::surface::facade::Plan>,
    profile: &crate::profile::Proof,
) -> Option<&'a convert::Type> {
    let convert::Type::Nominal {
        definition,
        arguments,
    } = ty
    else {
        return None;
    };
    let [element] = arguments.as_ref() else {
        return None;
    };
    if *definition == crate::surface::identity::Definition::future() {
        return Some(element);
    }
    // Only a host profile has a `Task` of its own to project iced's into.
    // With the neutral one, iced's `Task` is a crate type as any other,
    // which iced takes back from `update` (D601).
    if !profile.has_generated_projections() {
        return None;
    }
    facade?
        .facades()
        .filter(|facade| facade.root() == "iced")
        .flat_map(crate::surface::facade::Facade::types)
        .any(|ty| ty.identity() == definition.as_bytes() && ty.path() == "Task")
        .then_some(element)
}

/// Project an iced `Task<T>`, or a Rust future of `T`, into the facade's
/// `Task`. Each `T` it yields becomes a Rawr value on the UI thread, never
/// in the stream.
fn render_give_task(
    element: &convert::Type,
    value: &str,
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    let mut converted = String::new();
    render_to_dynamic(element, "value", 0, &mut converted)?;
    let materialize = if holds_value(element) {
        "materialize_checked"
    } else {
        "materialize"
    };
    writeln!(
        output,
        "    let task = {{\n        #[allow(unused_imports)]\n        use __rawr_profile_host::task::{{ViaFuture as _, ViaTask as _}};\n        (&__rawr_profile_host::task::Project::new({value})).into_task()\n    }};\n    Ok(__rawr_profile_host::task::imported(task.map(|value| -> __rawr_profile_host::task::Materialize {{\n        Box::new(move |witness| {materialize}(witness, move |arguments| {converted}))\n    }})))"
    )
    .map_err(|_| request::Error::Encoding)
}

fn render_give_with(
    signature: &crate::classify::representability::signature::Signature,
    substitutions: &[convert::Type],
    value: &str,
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    let copies = copied_inputs(signature);
    if copies.is_empty() {
        return render_give(
            &substituted(signature.output(), signature, substitutions)?,
            value,
            output,
        );
    }
    // The call's value, then each `&mut` array it changed (D558 slice 2).
    let mut given = Vec::new();
    render_give(
        &substituted(signature.output(), signature, substitutions)?,
        value,
        &mut given,
    )?;
    let given = String::from_utf8(given).map_err(|_| request::Error::Encoding)?;
    let mut copied = String::new();
    for (position, element) in &copies {
        let mut converted = String::new();
        render_to_dynamic(
            element,
            &format!("argument_{position}_copy"),
            0,
            &mut converted,
        )?;
        copied.push_str(&format!(", ({converted})?"));
    }
    writeln!(
        output,
        "    let given = (|| -> Result<Dynamic, Error> {{\n{given}    }})()?;\n    Ok(Dynamic::from_array(vec![given{copied}]))"
    )
    .map_err(|_| request::Error::Encoding)
}

/// Each input `&mut Vec<T>` or `&mut String` a call takes as a copy of
/// Rawr's array or string, by position, with the type it converts as
/// (D558 slice 2).
fn copied_inputs(
    signature: &crate::classify::representability::signature::Signature,
) -> Vec<(usize, convert::Type)> {
    signature
        .inputs()
        .iter()
        .enumerate()
        .filter_map(|(position, input)| match input {
            convert::Type::Borrowed {
                mutable: true,
                ty: element,
            } if is_copied(element) => Some((position, (**element).clone())),
            _ => None,
        })
        .collect()
}

/// Whether a `&mut` input of `ty` is a copy of Rawr's value the call
/// changes and gives back: an array, as `&mut Vec<u8>`, or a string, as
/// `&mut String`.
fn is_copied(ty: &convert::Type) -> bool {
    matches!(
        ty,
        convert::Type::Sequence { .. } | convert::Type::Builtin(convert::Builtin::String)
    )
}

/// Whether a `&` input of `ty` lends a Rust value the bridge makes from
/// Rawr's array or tuple for the call: `&[T]` or `&(A, B)`.
fn is_made_for_call(ty: &convert::Type) -> bool {
    matches!(ty, convert::Type::Sequence { .. } | convert::Type::Tuple(_))
}

/// `ty` with every type parameter the signature declares replaced by its
/// substitution, at any depth: a future of `Option<T>` is a future of
/// `Option<rawr::Value>` where `T` is erased (D513.2).
fn substituted(
    ty: &convert::Type,
    signature: &crate::classify::representability::signature::Signature,
    substitutions: &[convert::Type],
) -> Result<convert::Type, request::Error> {
    let all = |types: &[convert::Type]| {
        types
            .iter()
            .map(|ty| substituted(ty, signature, substitutions))
            .collect::<Result<Box<[_]>, _>>()
    };
    let one = |ty: &convert::Type| substituted(ty, signature, substitutions).map(Box::new);
    Ok(match ty {
        convert::Type::Parameter(_) => substitute(ty, signature, substitutions)?.clone(),
        convert::Type::Nominal {
            definition,
            arguments,
        } => convert::Type::Nominal {
            definition: *definition,
            arguments: all(arguments)?,
        },
        convert::Type::Option(element) => convert::Type::Option(one(element)?),
        convert::Type::Borrowed { mutable, ty } => convert::Type::Borrowed {
            mutable: *mutable,
            ty: one(ty)?,
        },
        convert::Type::Range(element) => convert::Type::Range(one(element)?),
        convert::Type::Result(ok, err) => convert::Type::Result(one(ok)?, one(err)?),
        convert::Type::Sequence {
            element,
            owned,
            length,
        } => convert::Type::Sequence {
            element: one(element)?,
            owned: *owned,
            length: *length,
        },
        convert::Type::Tuple(members) => convert::Type::Tuple(all(members)?),
        convert::Type::Script { path, arguments } => convert::Type::Script {
            path: path.clone(),
            arguments: all(arguments)?,
        },
        convert::Type::FunctionItem { input, output } => convert::Type::FunctionItem {
            input: one(input)?,
            output: one(output)?,
        },
        convert::Type::ProfileMessage { mapper, payload } => convert::Type::ProfileMessage {
            mapper: *mapper,
            payload: one(payload)?,
        },
        convert::Type::Inferred => ty.clone(),
        convert::Type::Unit | convert::Type::Builtin(_) => ty.clone(),
    })
}

fn substitute<'a>(
    ty: &'a convert::Type,
    signature: &'a crate::classify::representability::signature::Signature,
    substitutions: &'a [convert::Type],
) -> Result<&'a convert::Type, request::Error> {
    let convert::Type::Parameter(index) = ty else {
        return Ok(ty);
    };
    // A script future's output is a `rawr::Value`, whatever type the script
    // gives it, which comes back checked (D527). The synthetic output of a
    // callable `impl Into<T>` converts from is the instance's (D551.6).
    let synthetic = crate::classify::representability::signature::FUTURE_OUTPUT;
    if index & synthetic != 0
        && signature.parameters().iter().any(|parameter| {
            parameter.index() == index & !synthetic
                && parameter.kind() == crate::classify::representability::signature::Kind::Future
        })
    {
        static VALUE: std::sync::OnceLock<convert::Type> = std::sync::OnceLock::new();
        return Ok(VALUE.get_or_init(|| convert::Type::Nominal {
            definition: crate::surface::identity::Definition::value(),
            arguments: Box::default(),
        }));
    }
    // A script closure fills a callable parameter, which has no Rust type
    // to name: its input takes the closure (`render_take_callable`).
    if signature.parameters().iter().any(|parameter| {
        parameter.index() == *index
            && parameter.kind() == crate::classify::representability::signature::Kind::Callable
    }) {
        static CLOSURE: convert::Type = convert::Type::Unit;
        return Ok(&CLOSURE);
    }
    let position = signature
        .parameters()
        .iter()
        .filter(|parameter| {
            parameter.kind() == crate::classify::representability::signature::Kind::Type
        })
        .position(|parameter| parameter.index() == *index)
        .ok_or(request::Error::Drift {
            field: "generic substitution index",
        })?;
    substitutions.get(position).ok_or(request::Error::Drift {
        field: "generic substitution index",
    })
}

fn render_constant(
    constant: &convert::Constant,
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    if constant.value().is_empty()
        || !constant
            .value()
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'+'))
    {
        return Err(request::Error::Drift {
            field: "unsafe generated const substitution",
        });
    }
    output
        .write_all(constant.value().as_bytes())
        .map_err(|_| request::Error::Encoding)
}

struct GenericPartition<'a> {
    container_parameters: &'a [crate::classify::representability::signature::Parameter],
    item_parameters: &'a [crate::classify::representability::signature::Parameter],
    container_types: &'a [convert::Type],
    item_types: &'a [convert::Type],
    container_constants: &'a [convert::Constant],
    item_constants: &'a [convert::Constant],
}

fn partition_generics<'a>(
    signature: &'a crate::classify::representability::signature::Signature,
    substitutions: &'a [convert::Type],
    constants: &'a [convert::Constant],
    container_parameters: usize,
) -> Result<GenericPartition<'a>, request::Error> {
    let (container, item) = signature
        .parameters()
        .split_at_checked(container_parameters)
        .ok_or(request::Error::Drift {
            field: "generated container parameter cardinality",
        })?;
    let container_type_count = container
        .iter()
        .filter(|parameter| {
            parameter.kind() == crate::classify::representability::signature::Kind::Type
        })
        .count();
    let container_constant_count = container
        .iter()
        .filter(|parameter| {
            parameter.kind() == crate::classify::representability::signature::Kind::Constant
        })
        .count();
    let total_type_count = signature
        .parameters()
        .iter()
        .filter(|parameter| {
            parameter.kind() == crate::classify::representability::signature::Kind::Type
        })
        .count();
    let total_constant_count = signature
        .parameters()
        .iter()
        .filter(|parameter| {
            parameter.kind() == crate::classify::representability::signature::Kind::Constant
        })
        .count();
    if total_type_count != substitutions.len() || total_constant_count != constants.len() {
        return Err(request::Error::Drift {
            field: "generated substitution cardinality",
        });
    }
    let (container_types, item_types) = substitutions.split_at(container_type_count);
    let (container_constants, item_constants) = constants.split_at(container_constant_count);
    Ok(GenericPartition {
        container_parameters: container,
        item_parameters: item,
        container_types,
        item_types,
        container_constants,
        item_constants,
    })
}

fn render_function_path(
    plan: &Plan,
    signature: &crate::classify::representability::signature::Signature,
    substitutions: &[convert::Type],
    constants: &[convert::Constant],
    container_parameters: usize,
    associated: bool,
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    if associated {
        render_container_path(
            &rust_path(plan),
            signature,
            substitutions,
            constants,
            container_parameters,
            false,
            output,
        )?;
        write!(output, "::{}", plan.name()).map_err(|_| request::Error::Encoding)
    } else if container_parameters == 0 {
        output
            .write_all(rust_path(plan).as_bytes())
            .map_err(|_| request::Error::Encoding)
    } else {
        Err(request::Error::Drift {
            field: "generated free function has container parameters",
        })
    }
}

fn render_container_path(
    path: &str,
    signature: &crate::classify::representability::signature::Signature,
    substitutions: &[convert::Type],
    constants: &[convert::Constant],
    container_parameters: usize,
    type_position: bool,
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    // A method of an impl that fixes its type's arguments is reached at
    // that instance, `quick_xml::reader::Reader<&[u8]>`.
    if let Some(instance) = signature.instance().and_then(concrete_type) {
        let instance = if type_position {
            instance
        } else {
            format!("<{instance}>")
        };
        return output
            .write_all(instance.as_bytes())
            .map_err(|_| request::Error::Encoding);
    }
    let partition = partition_generics(signature, substitutions, constants, container_parameters)?;
    // An impl that fixes some of its type's arguments, as pane_grid's
    // `Content<'a, Message, Void, W, ..>`: that instance, its own
    // parameters given their substitutions.
    if let Some(instance) = signature.instance() {
        let mut types = partition
            .container_parameters
            .iter()
            .filter(|parameter| {
                parameter.kind() == crate::classify::representability::signature::Kind::Type
            })
            .map(|parameter| parameter.index())
            .zip(partition.container_types.iter());
        let substituted =
            instance_substituted(instance, &mut types).ok_or(request::Error::Drift {
                field: "an impl's fixed instance names a parameter it does not declare",
            })?;
        let instance = concrete_type(&substituted).ok_or(request::Error::Drift {
            field: "an impl's fixed instance has no Rust name",
        })?;
        let instance = if type_position {
            instance
        } else {
            format!("<{instance}>")
        };
        return output
            .write_all(instance.as_bytes())
            .map_err(|_| request::Error::Encoding);
    }
    output
        .write_all(path.as_bytes())
        .map_err(|_| request::Error::Encoding)?;
    render_generic_arguments(
        partition.container_parameters,
        partition.container_types,
        partition.container_constants,
        if type_position { b"<" } else { b"::<" },
        output,
    )
}

/// `instance` with each parameter given its substitution from `types`, the
/// impl's parameter indexes with theirs.
fn instance_substituted<'a>(
    instance: &convert::Type,
    types: &mut (impl Iterator<Item = (u32, &'a convert::Type)> + Clone),
) -> Option<convert::Type> {
    Some(match instance {
        convert::Type::Parameter(index) => types
            .clone()
            .find(|(parameter, _)| parameter == index)
            .map(|(_, ty)| ty.clone())?,
        convert::Type::Nominal {
            definition,
            arguments,
        } => convert::Type::Nominal {
            definition: *definition,
            arguments: arguments
                .iter()
                .map(|argument| instance_substituted(argument, types))
                .collect::<Option<Box<[_]>>>()?,
        },
        convert::Type::Option(element) => {
            convert::Type::Option(Box::new(instance_substituted(element, types)?))
        }
        other => other.clone(),
    })
}

thread_local! {
    /// The Rust path of each imported type of the facade plan the runner
    /// being generated declares, by identity: an imported type argument is
    /// written out where its path is known, so a value the arena stores or
    /// finds has its exact type (`chrono::DateTime<chrono::Local>`).
    static TYPE_PATHS: std::cell::RefCell<std::collections::BTreeMap<[u8; 32], String>> =
        const { std::cell::RefCell::new(std::collections::BTreeMap::new()) };
    /// The facade's own values by the identity a script names them at
    /// (`Definition::facade`), which cross as their Rust values.
    static FACADE_VALUES: std::cell::RefCell<std::collections::BTreeSet<[u8; 32]>> =
        const { std::cell::RefCell::new(std::collections::BTreeSet::new()) };
    /// The imported types that are Copy, by identity, as the compiler reads
    /// a field of one through a shared borrow.
    static COPY_TYPES: std::cell::RefCell<std::collections::BTreeSet<[u8; 32]>> =
        const { std::cell::RefCell::new(std::collections::BTreeSet::new()) };
    /// The opaque `impl Trait` types functions give, by identity, which Rust
    /// code can't write: spelled `_`, for rustc to infer (`surface::opaque`).
    static OPAQUE_TYPES: std::cell::RefCell<std::collections::BTreeSet<[u8; 32]>> =
        const { std::cell::RefCell::new(std::collections::BTreeSet::new()) };
    /// The imported types that carry lifetimes (D558 tier 1), by identity:
    /// how many lifetime parameters each declares, and whether it is
    /// `Debug` and `Clone`.
    static SCOPED_TYPES: std::cell::RefCell<std::collections::BTreeMap<[u8; 32], Lifetimes>> =
        const { std::cell::RefCell::new(std::collections::BTreeMap::new()) };
    /// Every imported type's lifetimes, `Debug` and `Clone`, by Rust path.
    static SCOPED_PATHS: std::cell::RefCell<std::collections::BTreeMap<String, Lifetimes>> =
        const { std::cell::RefCell::new(std::collections::BTreeMap::new()) };
    /// The same, by identity.
    static TYPE_TRAITS: std::cell::RefCell<std::collections::BTreeMap<[u8; 32], Lifetimes>> =
        const { std::cell::RefCell::new(std::collections::BTreeMap::new()) };
    /// Each imported type's identity, by every Rust path it has.
    static PATH_IDENTITIES: std::cell::RefCell<std::collections::BTreeMap<String, [u8; 32]>> =
        const { std::cell::RefCell::new(std::collections::BTreeMap::new()) };
    /// The Rust path of each free imported function and constant reached
    /// through its root, by selector: a macro's `$crate` path to it is
    /// written as the root spells it.
    static ITEM_PATHS: std::cell::RefCell<std::collections::BTreeMap<[u8; 32], String>> =
        const { std::cell::RefCell::new(std::collections::BTreeMap::new()) };
    /// The scope families the adapters name, by their type at `'x`: the
    /// family's name and its type's.
    /// Each script type a generic instance names, by its path from the
    /// crate root, with its number of type arguments: the runner declares
    /// its proxy (D594).
    static PROXIES: std::cell::RefCell<std::collections::BTreeMap<String, usize>> =
        const { std::cell::RefCell::new(std::collections::BTreeMap::new()) };
    /// The name of the script's crate, which the runner's binary takes, so
    /// a proxy's `type_name` is the script type's (D594).
    static PROXY_CRATE: std::cell::RefCell<Option<String>> =
        const { std::cell::RefCell::new(None) };
    static FAMILIES: std::cell::RefCell<std::collections::BTreeMap<String, (String, Lifetimes)>> =
        const { std::cell::RefCell::new(std::collections::BTreeMap::new()) };
    /// Whether a Rust value being converted lives in a scope frame's
    /// region, the local `region`, when its type carries a lifetime.
    static IN_REGION: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// Whether an adapter's body takes its arguments that carry a lifetime
    /// from the scope frame's region `region` it runs in.
    static REGION_ARGUMENTS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// Whether an adapter's body has the arguments native code lent it as
    /// `lent` (`with_lent_arguments`).
    static LENT_ARGUMENTS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// Whether an adapter takes its `&str` arguments pinned for the
    /// runner's life, as its output borrows them (`lends_str_to_output`).
    static PINNED_STRINGS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Takes an adapter's `&str` arguments pinned for the runner's life while
/// it lives, when `pinned`.
struct PinnedStrings(bool);

impl PinnedStrings {
    fn enter(pinned: bool) -> Self {
        Self(PINNED_STRINGS.replace(pinned))
    }
}

impl Drop for PinnedStrings {
    fn drop(&mut self) {
        PINNED_STRINGS.set(self.0);
    }
}

/// Whether the call's output carries a lifetime and an input lends it a
/// `&str` through a type parameter the output doesn't name, as iced's
/// `text(impl IntoFragment<'a>) -> Text<'a>` does: the output may borrow
/// the string.
fn lends_str_to_output(
    signature: &crate::classify::representability::signature::Signature,
    substitutions: &[convert::Type],
) -> Result<bool, request::Error> {
    if !signature
        .inputs()
        .iter()
        .any(|input| lends_str(input, signature, substitutions))
    {
        return Ok(false);
    }
    let returned = substituted(signature.output(), signature, substitutions)?;
    Ok(matches!(returned, convert::Type::Nominal { .. }) && carries_lifetime(&returned))
}

/// Takes an adapter's arguments that carry a lifetime from the region its
/// body runs in, while it lives.
struct RegionArguments(bool);

impl RegionArguments {
    fn enter() -> Self {
        Self(REGION_ARGUMENTS.replace(true))
    }
}

impl Drop for RegionArguments {
    fn drop(&mut self) {
        REGION_ARGUMENTS.set(self.0);
    }
}

/// Reaches an adapter's `&T` and `&mut T` arguments in `lent` first, while
/// it lives, when `on`.
struct LentArguments(bool);

impl LentArguments {
    fn enter(on: bool) -> Self {
        Self(LENT_ARGUMENTS.replace(on))
    }
}

impl Drop for LentArguments {
    fn drop(&mut self) {
        LENT_ARGUMENTS.set(self.0);
    }
}

/// What a generated scope family needs of an imported type that carries
/// lifetimes.
#[derive(Clone, Copy)]
struct Lifetimes {
    count: u32,
    /// Covariant in each lifetime, as rustc's variance says.
    covariant: bool,
    debug: bool,
    clone: bool,
    partial_eq: bool,
}

/// Converts Rust values into a scope frame's region, the local `region`,
/// while it lives.
struct InRegion(bool);

impl InRegion {
    fn enter() -> Self {
        Self(IN_REGION.replace(true))
    }
}

impl Drop for InRegion {
    fn drop(&mut self) {
        IN_REGION.set(self.0);
    }
}

/// The lifetimes an imported type declares, when it declares any.
fn scoped_definition(definition: &crate::surface::identity::Definition) -> Option<Lifetimes> {
    SCOPED_TYPES.with_borrow(|types| types.get(definition.as_bytes()).copied())
}

/// `ty` spelled at the scope lifetime `'x`: each imported type that
/// carries lifetimes takes `'x` for each, as a scope family names it.
fn scoped_type(ty: &convert::Type) -> Option<String> {
    match ty {
        convert::Type::Nominal {
            definition,
            arguments,
        } if scoped_definition(definition).is_some() => {
            let lifetimes = scoped_definition(definition)?;
            let mut text =
                TYPE_PATHS.with_borrow(|paths| paths.get(definition.as_bytes()).cloned())?;
            let mut parts = vec!["'x".to_owned(); lifetimes.count as usize];
            for argument in arguments.iter() {
                parts.push(scoped_type(argument)?);
            }
            text.push('<');
            text.push_str(&parts.join(", "));
            text.push('>');
            Some(text)
        }
        convert::Type::Nominal {
            definition,
            arguments,
        } if !arguments.is_empty() => {
            let mut text =
                TYPE_PATHS.with_borrow(|paths| paths.get(definition.as_bytes()).cloned())?;
            let arguments = arguments
                .iter()
                .map(scoped_type)
                .collect::<Option<Vec<_>>>()?;
            text.push('<');
            text.push_str(&arguments.join(", "));
            text.push('>');
            Some(text)
        }
        convert::Type::Option(element) => Some(format!("Option<{}>", scoped_type(element)?)),
        convert::Type::Result(ok, err) => Some(format!(
            "Result<{}, {}>",
            scoped_type(ok)?,
            scoped_type(err)?
        )),
        convert::Type::Borrowed { mutable: false, ty } if script_slice(ty).is_none() => {
            Some(format!("&'x {}", borrowed_target(ty)?))
        }
        ty => concrete_type(ty),
    }
}

/// `.to_vec()` after a call whose output is a `&[T]` it lends: the copy
/// Rawr's array is made of, before the borrow ends (D558 slice 2). And
/// `.to_owned()` after one whose output is a `&str` it lends, as url's
/// `Url::scheme`: Rawr's strings can't point into native memory, so the
/// text is copied there (D568). The borrow checker keeps the receiver
/// borrowed while the `&str` is used (D572).
fn lent_copy(signature: &crate::classify::representability::signature::Signature) -> &'static str {
    match signature.output() {
        convert::Type::Borrowed { mutable: false, ty }
            if matches!(**ty, convert::Type::Sequence { .. }) =>
        {
            ".to_vec()"
        }
        convert::Type::Borrowed { mutable: false, ty }
            if **ty == convert::Type::Builtin(convert::Builtin::Str) =>
        {
            ".to_owned()"
        }
        _ => "",
    }
}

/// Whether a value of `ty` may borrow when it is made in a scope frame:
/// it carries a lifetime, or Rawr keeps it as its text, as a `Cow<str>`.
fn may_borrow(ty: &convert::Type) -> bool {
    match ty {
        convert::Type::Builtin(convert::Builtin::Text) => true,
        convert::Type::Option(element) => may_borrow(element),
        convert::Type::Result(ok, err) => may_borrow(ok) || may_borrow(err),
        ty => carries_lifetime(ty),
    }
}

/// Whether a value of `ty` borrows when it is made in a scope frame.
/// Whether a native value of `ty` at `'static` is one at any shorter
/// lifetime: its type is covariant in each lifetime it declares, as rustc's
/// variance says, and its type arguments carry none.
fn shortens(ty: &convert::Type) -> bool {
    let convert::Type::Nominal {
        definition,
        arguments,
    } = ty
    else {
        return false;
    };
    TYPE_TRAITS.with_borrow(|types| {
        types
            .get(definition.as_bytes())
            .is_some_and(|lifetimes| lifetimes.covariant)
    }) && !arguments.iter().any(carries_lifetime)
}

fn carries_lifetime(ty: &convert::Type) -> bool {
    match ty {
        convert::Type::Nominal {
            definition,
            arguments,
        } => scoped_definition(definition).is_some() || arguments.iter().any(carries_lifetime),
        convert::Type::Option(element)
        | convert::Type::Range(element)
        | convert::Type::Sequence { element, .. } => carries_lifetime(element),
        convert::Type::Result(ok, err) => carries_lifetime(ok) || carries_lifetime(err),
        convert::Type::Tuple(members) => members.iter().any(carries_lifetime),
        convert::Type::ProfileMessage { payload, .. } => carries_lifetime(payload),
        convert::Type::Borrowed { .. } => true,
        _ => false,
    }
}

/// The name of the scope family of the imported type spelled `text` at
/// `'x`, whose own path and lifetimes are `path` and `lifetimes`. The
/// family is declared in `rawr_families`.
fn family(text: String, lifetimes: Lifetimes) -> String {
    FAMILIES.with_borrow_mut(|families| {
        let next = families.len();
        families
            .entry(text)
            .or_insert_with(|| (format!("RawrFamily{next}"), lifetimes))
            .0
            .clone()
    })
}

/// The family of `ty`, an imported type that carries lifetimes.
fn family_of(ty: &convert::Type) -> Option<String> {
    let convert::Type::Nominal { definition, .. } = ty else {
        return None;
    };
    if !carries_lifetime(ty) {
        return None;
    }
    let lifetimes = TYPE_TRAITS.with_borrow(|types| types.get(definition.as_bytes()).copied())?;
    Some(family(scoped_type(ty)?, lifetimes))
}

/// The family of a method's container, spelled `container` as
/// `render_container_path` writes it, at `'x`.
fn container_family(path: &str, container: &str) -> Option<String> {
    let lifetimes = SCOPED_PATHS.with_borrow(|paths| paths.get(path).copied())?;
    // A type has one family, by the path its facade names it at, whatever
    // path the adapter reaches it by (`quick_xml::reader::Reader` is
    // `quick_xml::Reader`).
    let canonical = PATH_IDENTITIES
        .with_borrow(|identities| identities.get(path).copied())
        .and_then(|identity| TYPE_PATHS.with_borrow(|paths| paths.get(&identity).cloned()));
    let rest = container
        .strip_prefix(path)
        .or_else(|| container.strip_prefix(canonical.as_deref()?))?;
    let path = canonical.as_deref().unwrap_or(path);
    // A borrowed type argument, as `Reader<&[u8]>`'s, borrows at `'x`.
    let borrowed = rest.contains('&');
    // So does a type argument that is a scoped type, as `Column<Element<'x,
    // ..>>`'s.
    let (rest, inner) = mark_scoped_arguments(rest);
    if lifetimes.count == 0 && !borrowed && !inner {
        return None;
    }
    let rest = rest.replace("&[", "&'x [").replace("&str", "&'x str");
    let marks = vec!["'x"; lifetimes.count as usize].join(", ");
    let text = match rest.strip_prefix('<') {
        Some(arguments) if lifetimes.count != 0 => format!("{path}<{marks}, {arguments}"),
        Some(_) => format!("{path}{rest}"),
        None if rest.is_empty() => format!("{path}<{marks}>"),
        None => return None,
    };
    Some(family(text, lifetimes))
}

/// `rest`, a container's type arguments as Rust spells them, with each
/// scoped type in it given its lifetimes at `'x`, and whether there was one.
fn mark_scoped_arguments(rest: &str) -> (String, bool) {
    let scoped = SCOPED_TYPES.with_borrow(|types| {
        TYPE_PATHS.with_borrow(|paths| {
            types
                .iter()
                .filter(|(_, lifetimes)| lifetimes.count != 0)
                .filter_map(|(identity, lifetimes)| {
                    Some((paths.get(identity)?.clone(), lifetimes.count))
                })
                .collect::<Vec<_>>()
        })
    });
    let mut text = rest.to_owned();
    let mut marked = false;
    for (path, count) in scoped {
        let open = format!("{path}<");
        let mut at = 0;
        while let Some(found) = text[at..].find(&open) {
            let start = at + found;
            let boundary = text[..start]
                .chars()
                .next_back()
                .is_none_or(|before| matches!(before, '<' | ' ' | ',' | '&' | '('));
            let end = start + open.len();
            if boundary && !text[end..].starts_with('\'') {
                let marks = vec!["'x"; count as usize].join(", ");
                text.insert_str(end, &format!("{marks}, "));
                marked = true;
            }
            at = end;
        }
    }
    (text, marked)
}

/// The proxies of the script types the adapters named (D594), each at its
/// script path from the crate root, so `type_name` and `TypeId` are its
/// own. A proxy holds the script value by a shared handle; its `Clone`,
/// `Debug` and `Default` are the script type's own.
fn render_proxies(output: &mut Vec<u8>) -> Result<(), request::Error> {
    let proxies = PROXIES.with_borrow(Clone::clone);
    // Each module path's proxies, nested as the script nests them.
    let mut tree = std::collections::BTreeMap::<Vec<String>, Vec<(String, String, usize)>>::new();
    for (path, arity) in proxies {
        let mut segments = path.split("::").map(str::to_owned).collect::<Vec<_>>();
        let name = segments.pop().unwrap_or_default();
        tree.entry(segments).or_default().push((path, name, arity));
    }
    let mut open: Vec<String> = Vec::new();
    for (modules, proxies) in tree {
        while !modules.starts_with(&open) {
            open.pop();
            output
                .write_all(b"}\n")
                .map_err(|_| request::Error::Encoding)?;
        }
        for module in &modules[open.len()..] {
            writeln!(
                output,
                "pub mod {module} {{\n#[allow(unused_imports)]\nuse super::*;"
            )
            .map_err(|_| request::Error::Encoding)?;
            open.push(module.clone());
        }
        for (path, name, arity) in proxies {
            let parameters = (0..arity)
                .map(|index| format!("T{index}"))
                .collect::<Vec<_>>();
            let (generics, marker, made) = if parameters.is_empty() {
                (String::new(), String::new(), "Self(proxy)".to_owned())
            } else {
                let list = parameters.join(", ");
                (
                    format!("<{list}>"),
                    format!(", std::marker::PhantomData<fn() -> ({list},)>"),
                    "Self(proxy, std::marker::PhantomData)".to_owned(),
                )
            };
            let bounds = if parameters.is_empty() {
                String::new()
            } else {
                format!("<{}>", parameters.join(", "))
            };
            writeln!(
                output,
                "/// The script type `{path}` where native generic code takes a Rust type (D594).
pub struct {name}{generics}(pub(crate) __rawr_internal::crossing::Proxy{marker});

#[allow(dead_code)]
impl{bounds} {name}{generics} {{
    pub(crate) fn from_proxy(proxy: __rawr_internal::crossing::Proxy) -> Self {{
        {made}
    }}
}}

impl{bounds} Clone for {name}{generics} {{
    fn clone(&self) -> Self {{
        Self::from_proxy(self.0.clone())
    }}
}}

impl{bounds} std::fmt::Debug for {name}{generics} {{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {{
        std::fmt::Debug::fmt(&self.0, f)
    }}
}}

impl{bounds} Default for {name}{generics} {{
    fn default() -> Self {{
        Self::from_proxy(__rawr_internal::crossing::Proxy::default_of({path:?}))
    }}
}}

impl{bounds} PartialEq for {name}{generics} {{
    fn eq(&self, other: &Self) -> bool {{
        self.0 == other.0
    }}
}}

impl{bounds} std::fmt::Display for {name}{generics} {{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {{
        self.0.display(f)
    }}
}}
"
            )
            .map_err(|_| request::Error::Encoding)?;
        }
    }
    for _ in open {
        output
            .write_all(b"}\n")
            .map_err(|_| request::Error::Encoding)?;
    }
    Ok(())
}

/// `ty` as Rust spells it in a script impl's header and signatures, with
/// each of the impl's own type parameters by its name in `generics`.
fn impl_type(ty: &convert::Type, generics: &[Box<str>]) -> Option<String> {
    let all = |types: &[convert::Type]| {
        types
            .iter()
            .map(|ty| impl_type(ty, generics))
            .collect::<Option<Vec<_>>>()
    };
    Some(match ty {
        convert::Type::Parameter(index) => generics.get(*index as usize)?.to_string(),
        _ if !names_parameter(ty) => match ty {
            // `&str`, Rust's borrowed string, as instances write it.
            convert::Type::Borrowed { mutable: false, ty }
                if matches!(**ty, convert::Type::Builtin(convert::Builtin::Str)) =>
            {
                "&str".to_owned()
            }
            convert::Type::Borrowed { mutable, ty } => {
                let inner = concrete_type(ty)?;
                if *mutable {
                    format!("&mut {inner}")
                } else {
                    format!("&{inner}")
                }
            }
            ty => concrete_type(ty)?,
        },
        convert::Type::Script { path, arguments } => {
            // Registers the proxy, as `concrete_type` does.
            concrete_type(&convert::Type::Script {
                path: path.clone(),
                arguments: arguments.iter().map(|_| convert::Type::Unit).collect(),
            })?;
            let (_, path) = path.split_once("::")?;
            format!("crate::{path}<{}>", all(arguments)?.join(", "))
        }
        convert::Type::Nominal { arguments, .. } => {
            let bare = concrete_type(&convert::Type::Nominal {
                definition: match ty {
                    convert::Type::Nominal { definition, .. } => *definition,
                    _ => return None,
                },
                arguments: Box::default(),
            })?;
            format!("{bare}<{}>", all(arguments)?.join(", "))
        }
        convert::Type::Option(element) => format!("Option<{}>", impl_type(element, generics)?),
        convert::Type::Sequence { element, .. } => {
            format!("Vec<{}>", impl_type(element, generics)?)
        }
        convert::Type::Tuple(members) => format!("({},)", all(members)?.join(", ")),
        convert::Type::Borrowed { mutable, ty } => format!(
            "&{}{}",
            if *mutable { "mut " } else { "" },
            impl_type(ty, generics)?
        ),
        _ => return None,
    })
}

/// Whether `ty` names a type parameter at any depth.
fn names_parameter(ty: &convert::Type) -> bool {
    match ty {
        convert::Type::Parameter(_) => true,
        convert::Type::Nominal { arguments, .. } | convert::Type::Script { arguments, .. } => {
            arguments.iter().any(names_parameter)
        }
        convert::Type::Tuple(members) => members.iter().any(names_parameter),
        convert::Type::Option(inner)
        | convert::Type::Range(inner)
        | convert::Type::Borrowed { ty: inner, .. }
        | convert::Type::Sequence { element: inner, .. }
        | convert::Type::ProfileMessage { payload: inner, .. } => names_parameter(inner),
        convert::Type::Result(ok, err) => names_parameter(ok) || names_parameter(err),
        convert::Type::FunctionItem { input, output } => {
            names_parameter(input) || names_parameter(output)
        }
        convert::Type::Unit | convert::Type::Builtin(_) | convert::Type::Inferred => false,
    }
}

/// The script's impls that cross into native code with the script types
/// the adapters take (D594), each once, for the proxies: `impl canvas::
/// Program<Message> for Arc`, or `impl From<Color> for iced::Color`.
///
/// Each function calls the script's own, in the generation current where
/// native code calls it. It lends what it takes as a slot does: a script
/// value by its handle, a native value it borrows for the call alone,
/// through a lend whose accessors the runner's adapters reach it by
/// (D558.3), and a native value it owns through the arena.
fn render_impls(plans: &[Plan], output: &mut Vec<u8>) -> Result<(), request::Error> {
    let impls = plans
        .iter()
        .flat_map(Plan::impls)
        .collect::<std::collections::BTreeSet<_>>();
    for script in impls {
        render_impl(script, output)?;
    }
    Ok(())
}

fn render_impl(
    script: &crate::surface::instance::ScriptImpl,
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    let drift = |field| request::Error::Drift { field };
    let generics = script.generics();
    let owner = impl_type(script.owner(), generics).ok_or(drift("a script impl's owner"))?;
    let arguments = script
        .arguments()
        .iter()
        .map(|argument| impl_type(argument, generics))
        .collect::<Option<Vec<_>>>()
        .ok_or(drift("a script impl's trait arguments"))?;
    // Each lifetime argument is the impl's own, as `'_` writes it.
    let arguments = (0..script.lifetimes())
        .map(|_| "'_".to_owned())
        .chain(arguments)
        .collect::<Vec<_>>();
    let trait_ = if arguments.is_empty() {
        script.trait_path().to_owned()
    } else {
        format!("{}<{}>", script.trait_path(), arguments.join(", "))
    };
    let parameters = if generics.is_empty() {
        String::new()
    } else {
        format!(
            "<{}>",
            generics
                .iter()
                .map(AsRef::as_ref)
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    let mut body = String::new();
    for (name, ty) in script.associated() {
        let ty = impl_type(ty, generics).ok_or(drift("a script impl's associated type"))?;
        body.push_str(&format!("    type {name} = {ty};\n"));
    }
    for function in script.functions() {
        body.push_str(&render_impl_function(function, generics)?);
    }
    writeln!(
        output,
        "/// The script's `impl {trait_} for {owner}` (D594).
#[allow(unused_mut, unused_variables, refining_impl_trait, clippy::all)]
impl{parameters} {trait_} for {owner} {{
{body}}}
"
    )
    .map_err(|_| request::Error::Encoding)
}

/// One function of a script impl, as `render_impls` writes it.
fn render_impl_function(
    function: &crate::surface::instance::ScriptImplFunction,
    generics: &[Box<str>],
) -> Result<String, request::Error> {
    use crate::surface::instance::ImplReceiver;
    let drift = |field| request::Error::Drift { field };
    let mut parameters = Vec::new();
    let mut values = Vec::new();
    let mut mutable = Vec::new();
    let mut restores = String::new();
    let mut lends = Vec::new();
    match function.receiver() {
        ImplReceiver::None => {}
        ImplReceiver::Shared => {
            parameters.push("&self".to_owned());
            values.push("self.0.lend()".to_owned());
        }
        ImplReceiver::Mutable => {
            parameters.push("&mut self".to_owned());
            values.push("self.0.lend_mut()".to_owned());
            mutable.push("0".to_owned());
            restores.push_str("\n        self.0.restore(std::mem::take(&mut values[0]));");
        }
        ImplReceiver::Owned => {
            parameters.push("self".to_owned());
            values.push("self.0.into_dynamic()".to_owned());
        }
    }
    for (position, input) in function.inputs().iter().enumerate() {
        let name = format!("input_{position}");
        let at = values.len();
        let rust = impl_type(input, generics).ok_or(drift("a script impl function's input"))?;
        parameters.push(format!("{name}: {rust}"));
        if names_parameter(input) {
            return Err(drift(
                "a script impl function's input names a type parameter of the impl",
            ));
        }
        // The script function gives back each `&mut` it takes after its
        // value (`Callable::call_places`).
        if matches!(input, convert::Type::Borrowed { mutable: true, .. }) {
            mutable.push(at.to_string());
        }
        match input {
            convert::Type::Borrowed { mutable: true, ty }
                if matches!(**ty, convert::Type::Script { .. }) =>
            {
                values.push(format!("{name}.0.lend_mut()"));
                restores.push_str(&format!(
                    "\n        {name}.0.restore(std::mem::take(&mut values[{at}]));"
                ));
            }
            convert::Type::Borrowed { mutable: false, ty }
                if matches!(**ty, convert::Type::Script { .. }) =>
            {
                values.push(format!("{name}.0.lend()"));
            }
            convert::Type::Borrowed { ty, .. } if matches!(**ty, convert::Type::Unit) => {
                values.push("Dynamic::UNIT".to_owned());
            }
            // A `&str`: Rawr's string of its text, as a script function
            // takes a string.
            convert::Type::Borrowed { mutable: false, ty }
                if matches!(**ty, convert::Type::Builtin(convert::Builtin::Str)) =>
            {
                values.push(format!("Dynamic::from({name}.to_string())"));
            }
            // A native value the function borrows: lent for the call
            // alone, and reached by the adapters through the accessors
            // (D558.3).
            convert::Type::Borrowed {
                mutable: borrowed_mutably,
                ty,
            } if matches!(**ty, convert::Type::Nominal { .. })
                && !is_value(ty)
                && !carries_lifetime(ty) =>
            {
                let target = concrete_type(ty).ok_or(drift("a script impl function's input"))?;
                values.push(lent_value(position, *borrowed_mutably, &target));
                lends.push((position, *borrowed_mutably, target));
            }
            convert::Type::Borrowed { ty, .. } => {
                // A copy of a value Rawr holds by value, as a `&str` or a
                // `&[T]` is.
                let mut converted = String::new();
                render_to_dynamic(ty, &format!("Clone::clone({name})"), 0, &mut converted)?;
                values.push(format!(
                    "(({converted}).and_then(|value| attest_input(arguments.arena, value, witnesses.get({position}))))"
                ));
            }
            // A native value is attested as the type of the script
            // function's parameter, so script code reads it as that type.
            input => {
                let mut converted = String::new();
                render_to_dynamic(input, &name, 0, &mut converted)?;
                values.push(format!(
                    "(({converted}).and_then(|value| attest_input(arguments.arena, value, witnesses.get({position}))))"
                ));
            }
        }
    }
    let returned = function.output();
    if names_parameter(returned) {
        return Err(drift(
            "a script impl function's output names a type parameter of the impl",
        ));
    }
    let rust = impl_type(returned, generics).ok_or(drift("a script impl function's output"))?;
    let in_arena = |body: &str| {
        format!(
            "with_arena(|arena| {{ let mut none: [&mut Dynamic; 0] = []; let arguments = &mut Arguments {{ values: &mut none, arena }}; let _ = &arguments; {body} }})"
        )
    };
    let crossed = |value: &str| {
        if value.starts_with('(') {
            format!("{value}?")
        } else {
            format!("Ok::<Dynamic, Error>({value})?")
        }
    };
    // Native values go in and out through the arena.
    let count = values.len();
    let values = format!(
        "{}.unwrap_or_else(|error| panic!(\"rawr: a value native code gave a script function can't cross: {{error:?}}\"))",
        in_arena(&format!(
            "Ok::<[Dynamic; {count}], Error>([{}])",
            values.iter().map(|value| crossed(value)).collect::<Vec<_>>().join(", ")
        ))
    );
    let member = function.member();
    let mutable = mutable.join(", ");
    // An output that borrows, as a `view(&self) -> impl Widget<..> + 'a`'s
    // widgets do: made in a region, as a slot's is, anchored on the strings
    // pinned for the runner's life, since the trait may ask the widgets to
    // outlive any lifetime of the impl's, and taken out of it at
    // `'static`.
    let family = carries_lifetime(returned)
        .then(|| family_of(returned))
        .flatten();
    let (lifetime, rust, mut call) = match family {
        Some(family) => {
            (
                "",
                format!("<{family} as __rawr_internal::scope::Family>::Of<'static>"),
                format!(
                    "let witnesses = __rawr_internal::crossing::member_witnesses({member:?});
        let mut values: [Dynamic; {count}] = {values};
        let mut region = __rawr_internal::scope::Region::anchored(static_pins());
        let value = __rawr_internal::scope::open_anchored(&mut region, || __rawr_internal::crossing::call_member({member:?}, &mut values, &[{mutable}]));{restores}
        let scoped = value
            .read_lock::<__rawr_internal::scope::Scoped>()
            .map(|scoped| *scoped)
            .unwrap_or_else(|| panic!(\"rawr: a script function gave native code a value that doesn't borrow what it was lent\"));
        region
            .take::<{family}>(scoped)
            .unwrap_or_else(|error| panic!(\"rawr: {{error}}\"))"
                ),
            )
        }
        None => {
            let mut converted = String::new();
            render_from_dynamic(returned, "value", 0, &mut converted)?;
            let converted = in_arena(&converted);
            (
                "",
                rust,
                format!(
                    "let witnesses = __rawr_internal::crossing::member_witnesses({member:?});
        let mut values: [Dynamic; {count}] = {values};
        let value = __rawr_internal::crossing::call_member({member:?}, &mut values, &[{mutable}]);{restores}
        {converted}.unwrap_or_else(|error| panic!(\"rawr: a script function gave back a value native code can't take: {{error:?}}\"))"
                ),
            )
        }
    };
    // Each borrowed native value is lent around the call, innermost last.
    let (items, wrapped) = lent_around(&lends, call);
    call = wrapped;
    Ok(format!(
        "    fn {}{lifetime}({}) -> {rust} {{
{items}        {call}
    }}
",
        function.name(),
        parameters.join(", ")
    ))
}

/// The call `call` with each borrowed native value of `lends` (its input's
/// position, whether it is `&mut`, and its Rust type) lent around it,
/// innermost last, and the items that reach each one (D558.3): what a script
/// impl function and a slot write where native code lends them a value.
fn lent_around(lends: &[(usize, bool, String)], call: String) -> (String, String) {
    let mut call = call;
    let mut items = String::new();
    for (position, borrowed_mutably, target) in lends.iter().rev() {
        let lend = if *borrowed_mutably {
            "lend"
        } else {
            "lend_ref"
        };
        call = format!(
            "__rawr_internal::scope::{lend}::<Lend{position}, _>(input_{position}, |lent_{position}| {{
        {call}
        }})"
        );
        items.push_str(&format!(
            "        struct Lend{position};
        impl __rawr_internal::scope::Family for Lend{position} {{
            type Of<'x> = {target};
        }}
        fn reach_{position}(nonce: u64, visit: &mut dyn FnMut(&{target})) -> Result<(), u8> {{
            __rawr_internal::scope::with_ref::<Lend{position}, _>(__rawr_internal::scope::nonce(nonce), |value| visit(value))
                .map_err(__rawr_internal::scope::Error::code)
        }}
        static REACH_{position}: fn(u64, &mut dyn FnMut(&{target})) -> Result<(), u8> = reach_{position};
"
        ));
        if *borrowed_mutably {
            items.push_str(&format!(
                "        fn reach_mut_{position}(nonce: u64, visit: &mut dyn FnMut(&mut {target})) -> Result<(), u8> {{
            __rawr_internal::scope::with::<Lend{position}, _>(__rawr_internal::scope::nonce(nonce), |value| visit(value))
                .map_err(__rawr_internal::scope::Error::code)
        }}
        static REACH_MUT_{position}: fn(u64, &mut dyn FnMut(&mut {target})) -> Result<(), u8> = reach_mut_{position};
"
            ));
        }
    }
    (items, call)
}

/// The script value of a native value lent at `position` (`lent_around`).
fn lent_value(position: usize, borrowed_mutably: bool, target: &str) -> String {
    format!(
        "Dynamic::from(__rawr_internal::scope::Lent::new(lent_{position}, {target:?}, {}, Some(&REACH_{position} as __rawr_internal::scope::Accessor)))",
        if borrowed_mutably {
            format!("Some(&REACH_MUT_{position} as __rawr_internal::scope::Accessor)")
        } else {
            "None".to_owned()
        }
    )
}

/// The scope families the adapters named, as the `rawr_families` module.
fn render_families(output: &mut Vec<u8>) -> Result<(), request::Error> {
    output
        .write_all(
            b"#![allow(dead_code)]
use super::*;

",
        )
        .map_err(|_| request::Error::Encoding)?;
    let families = FAMILIES.with_borrow(|families| {
        families
            .iter()
            .map(|(text, (name, lifetimes))| (text.clone(), name.clone(), *lifetimes))
            .collect::<Vec<_>>()
    });
    for (text, name, lifetimes) in families {
        let debug = if lifetimes.debug {
            "\n    fn debug(value: &Self::Of<'_>) -> Option<String> {\n        Some(format!(\"{value:?}\"))\n    }"
        } else {
            ""
        };
        let clone = if lifetimes.clone {
            "\n    fn duplicate<'x>(value: &Self::Of<'x>) -> Option<Self::Of<'x>> {\n        Some(Clone::clone(value))\n    }"
        } else {
            ""
        };
        // `==` within a region, and with a value of the type at `'static`
        // the arena holds, which the region's `'x` shortens.
        let equal = if lifetimes.partial_eq {
            format!(
                "\n    fn equal<'x>(a: &Self::Of<'x>, b: &Self::Of<'x>) -> Option<bool> {{\n        Some(a == b)\n    }}\n    fn equal_native<'x>(a: &Self::Of<'x>, b: &dyn std::any::Any) -> Option<bool> {{\n        let b: &Self::Of<'x> = b.downcast_ref::<{}>()?;\n        Some(a == b)\n    }}",
                text.replace("'x", "'static")
            )
        } else {
            String::new()
        };
        let shown = text.replace("'x", "'_");
        writeln!(
            output,
            "pub(super) struct {name};\n\nimpl __rawr_internal::scope::Family for {name} {{\n    type Of<'x> = {text};\n}}\n\nimpl __rawr_internal::scope::Regional for {name} {{\n    fn name() -> &'static str {{\n        {shown:?}\n    }}{debug}{clone}{equal}\n}}\n"
        )
        .map_err(|_| request::Error::Encoding)?;
    }
    Ok(())
}

/// The type `alias` names when it passes its own parameters on, all and
/// in order, as `type Text<'a, Theme = ..> = core::Text<'a, Theme>` does.
fn passed_on(alias: &crate::surface::facade::TypeAlias) -> Option<[u8; 32]> {
    let crate::surface::facade::Type::Nominal {
        identity,
        arguments,
    } = alias.target()
    else {
        return None;
    };
    (arguments.len() == alias.parameters().len()
        && arguments.iter().zip(alias.parameters()).all(|(argument, parameter)| {
            matches!(argument, crate::surface::facade::Type::Parameter(name) if **name == *parameter)
        }))
    .then_some(*identity)
}

/// Fills `TYPE_PATHS` for one generation and empties it when dropped.
struct TypePaths;

impl TypePaths {
    /// `views` when a host profile makes view recipes (D558 tier 2); a
    /// runner with the neutral profile makes none, and an imported type
    /// that renders is an ordinary scoped type (D594).
    fn install(facade: Option<&crate::surface::facade::Plan>, views: bool) -> Self {
        view::install(facade.filter(|_| views));
        TYPE_PATHS.with_borrow_mut(|paths| {
            FACADE_VALUES.with_borrow_mut(|values| {
                paths.clear();
                values.clear();
                // A type's own spelling through its root comes before a
                // macro's `$crate` path to it, which Rust cannot write.
                let through_crate = |path: &str| path.contains(crate::api::ledger::CRATE_SEGMENT);
                let types = facade
                    .into_iter()
                    .flat_map(crate::surface::facade::Plan::facades)
                    .flat_map(|owner| owner.types().map(move |ty| (owner, ty)));
                let (plain, through): (Vec<_>, Vec<_>) =
                    types.partition(|(_, ty)| !through_crate(ty.path()));
                for (owner, ty) in &plain {
                    let path = format!("{}::{}", owner.root(), ty.path());
                    // The facade's own value at this path, as a view's
                    // builder takes `iced::Length` (D558 tier 2).
                    let facade = *crate::surface::identity::Definition::facade(&path).as_bytes();
                    values.insert(facade);
                    paths.entry(facade).or_insert_with(|| path.clone());
                    paths.entry(*ty.identity()).or_insert(path);
                }
                // A type the root reaches only through a crate's alias that
                // passes its parameters on in order, as iced's `widget::Text<
                // 'a, Theme> = core::widget::Text<'a, Theme>`: the alias
                // spells it.
                for owner in facade
                    .into_iter()
                    .flat_map(crate::surface::facade::Plan::facades)
                {
                    for alias in owner
                        .type_aliases()
                        .filter(|alias| !through_crate(alias.path()))
                    {
                        if let Some(identity) = passed_on(alias) {
                            paths
                                .entry(identity)
                                .or_insert_with(|| format!("{}::{}", owner.root(), alias.path()));
                        }
                    }
                }
                for (owner, ty) in through {
                    let path = format!("{}::{}", owner.root(), ty.path());
                    let facade = *crate::surface::identity::Definition::facade(&path).as_bytes();
                    values.insert(facade);
                    paths.entry(facade).or_insert_with(|| path.clone());
                    paths.entry(*ty.identity()).or_insert(path);
                }
            });
        });
        ITEM_PATHS.with_borrow_mut(|paths| {
            paths.clear();
            for owner in facade
                .into_iter()
                .flat_map(crate::surface::facade::Plan::facades)
            {
                for item in owner.items().filter(|item| {
                    matches!(
                        item.kind(),
                        crate::surface::facade::ItemKind::Function
                            | crate::surface::facade::ItemKind::Constant
                    ) && !item.path().contains(crate::api::ledger::CRATE_SEGMENT)
                }) {
                    paths
                        .entry(item.selector().fingerprint())
                        .or_insert_with(|| format!("{}::{}", owner.root(), item.path()));
                }
            }
        });
        OPAQUE_TYPES.with_borrow_mut(|opaque| {
            opaque.clear();
            for owner in facade
                .into_iter()
                .flat_map(crate::surface::facade::Plan::facades)
            {
                opaque.extend(
                    owner
                        .types()
                        .filter(|ty| ty.opaque())
                        .map(|ty| *ty.identity()),
                );
            }
        });
        COPY_TYPES.with_borrow_mut(|copy| {
            copy.clear();
            for owner in facade
                .into_iter()
                .flat_map(crate::surface::facade::Plan::facades)
            {
                copy.extend(
                    owner
                        .types()
                        .filter(|ty| ty.copy())
                        .map(|ty| *ty.identity()),
                );
            }
        });
        SCOPED_TYPES.with_borrow_mut(|types| {
            SCOPED_PATHS.with_borrow_mut(|paths| {
                TYPE_TRAITS.with_borrow_mut(|traits| {
                    types.clear();
                    paths.clear();
                    traits.clear();
                    for owner in facade
                        .into_iter()
                        .flat_map(crate::surface::facade::Plan::facades)
                    {
                        for ty in owner.types() {
                            let lifetimes = Lifetimes {
                                count: ty.lifetimes(),
                                covariant: ty.covariant(),
                                debug: ty.debug(),
                                clone: ty.cloneable(),
                                partial_eq: ty.partial_eq(),
                            };
                            // A view type's value is a recipe, not a scope
                            // frame's, where a host profile makes view
                            // recipes (D558 tier 2). With the neutral profile
                            // it is an ordinary scoped type (D594).
                            if ty.lifetimes() != 0 && !(views && ty.view()) {
                                types.entry(*ty.identity()).or_insert(lifetimes);
                            }
                            traits.entry(*ty.identity()).or_insert(lifetimes);
                            paths
                                .entry(format!("{}::{}", owner.root(), ty.path()))
                                .or_insert(lifetimes);
                            PATH_IDENTITIES.with_borrow_mut(|identities| {
                                identities
                                    .entry(format!("{}::{}", owner.root(), ty.path()))
                                    .or_insert(*ty.identity());
                            });
                        }
                    }
                    // An alias that passes its parameters on to a type, as
                    // iced's `widget::Text`, is that type's path too: a method
                    // reached through it finds the type's family.
                    for owner in facade
                        .into_iter()
                        .flat_map(crate::surface::facade::Plan::facades)
                    {
                        for alias in owner.type_aliases() {
                            let Some(identity) = passed_on(alias) else {
                                continue;
                            };
                            let Some(lifetimes) = traits.get(&identity).copied() else {
                                continue;
                            };
                            let path = format!("{}::{}", owner.root(), alias.path());
                            paths.entry(path.clone()).or_insert(lifetimes);
                            PATH_IDENTITIES.with_borrow_mut(|identities| {
                                identities.entry(path).or_insert(identity);
                            });
                        }
                    }
                });
            });
        });
        // A provided trait method can have arrived through a macro's
        // `$crate` while the same trait has a public path in source.
        for owner in facade
            .into_iter()
            .flat_map(crate::surface::facade::Plan::facades)
        {
            for header in owner.trait_headers() {
                let path = format!("{}::{}", owner.root(), header.path());
                PATH_IDENTITIES.with_borrow_mut(|identities| {
                    identities.entry(path.clone()).or_insert(*header.identity());
                });
                if !path.contains(crate::api::ledger::CRATE_SEGMENT) {
                    TYPE_PATHS.with_borrow_mut(|paths| {
                        paths.entry(*header.identity()).or_insert(path);
                    });
                }
            }
        }
        FAMILIES.with_borrow_mut(std::collections::BTreeMap::clear);
        PROXIES.with_borrow_mut(std::collections::BTreeMap::clear);
        PROXY_CRATE.with_borrow_mut(|named| *named = None);
        Self
    }
}

impl Drop for TypePaths {
    fn drop(&mut self) {
        TYPE_PATHS.with_borrow_mut(std::collections::BTreeMap::clear);
        COPY_TYPES.with_borrow_mut(std::collections::BTreeSet::clear);
        OPAQUE_TYPES.with_borrow_mut(std::collections::BTreeSet::clear);
        SCOPED_TYPES.with_borrow_mut(std::collections::BTreeMap::clear);
        view::clear();
        SCOPED_PATHS.with_borrow_mut(std::collections::BTreeMap::clear);
        TYPE_TRAITS.with_borrow_mut(std::collections::BTreeMap::clear);
        PATH_IDENTITIES.with_borrow_mut(std::collections::BTreeMap::clear);
        ITEM_PATHS.with_borrow_mut(std::collections::BTreeMap::clear);
        FAMILIES.with_borrow_mut(std::collections::BTreeMap::clear);
        PROXIES.with_borrow_mut(std::collections::BTreeMap::clear);
        PROXY_CRATE.with_borrow_mut(|named| *named = None);
    }
}

/// The Rust spelling of `ty`, its imported types by their facade paths, or
/// `None` when it has no exact spelling.
/// Whether `ty` names the type parameter at `index`.
fn output_names(ty: &convert::Type, index: u32) -> bool {
    match ty {
        convert::Type::Parameter(parameter) => *parameter == index,
        convert::Type::Unit | convert::Type::Builtin(_) | convert::Type::Inferred => false,
        convert::Type::Nominal { arguments, .. }
        | convert::Type::Tuple(arguments)
        | convert::Type::Script { arguments, .. } => arguments
            .iter()
            .any(|argument| output_names(argument, index)),
        convert::Type::Option(element)
        | convert::Type::Borrowed { ty: element, .. }
        | convert::Type::Range(element)
        | convert::Type::Sequence { element, .. } => output_names(element, index),
        convert::Type::Result(ok, err) => output_names(ok, index) || output_names(err, index),
        convert::Type::FunctionItem { input, output } => {
            output_names(input, index) || output_names(output, index)
        }
        convert::Type::ProfileMessage { payload, .. } => output_names(payload, index),
    }
}

/// Whether `definition` is an opaque `impl Trait` type a function gives.
fn is_opaque(definition: &crate::surface::identity::Definition) -> bool {
    OPAQUE_TYPES.with_borrow(|opaque| opaque.contains(definition.as_bytes()))
}

/// Whether `ty` holds an opaque `impl Trait` type a function gives.
fn holds_opaque(ty: &convert::Type) -> bool {
    match ty {
        convert::Type::Nominal {
            definition,
            arguments,
        } => is_opaque(definition) || arguments.iter().any(holds_opaque),
        convert::Type::Option(inner)
        | convert::Type::Borrowed { ty: inner, .. }
        | convert::Type::Range(inner)
        | convert::Type::Sequence { element: inner, .. } => holds_opaque(inner),
        convert::Type::Result(ok, err) => holds_opaque(ok) || holds_opaque(err),
        convert::Type::Tuple(members) => members.iter().any(holds_opaque),
        _ => false,
    }
}

fn concrete_type(ty: &convert::Type) -> Option<String> {
    Some(match ty {
        convert::Type::Inferred => "_".to_owned(),
        convert::Type::Unit => "()".to_owned(),
        convert::Type::Builtin(convert::Builtin::String | convert::Builtin::Text) => {
            "String".to_owned()
        }
        convert::Type::Builtin(convert::Builtin::Str) => "&str".to_owned(),
        convert::Type::Builtin(convert::Builtin::StaticStr) => "&'static str".to_owned(),
        convert::Type::Builtin(convert::Builtin::InclusiveRange) => {
            "std::ops::RangeInclusive<i64>".to_owned()
        }
        convert::Type::Builtin(builtin) => builtin.rust_name()?.to_owned(),
        convert::Type::Nominal { definition, .. }
            if *definition == crate::surface::identity::Definition::value() =>
        {
            "__rawr_internal::Value".to_owned()
        }
        ty if let Some(element) = range_inclusive_element(ty) => {
            format!("std::ops::RangeInclusive<{}>", concrete_type(element)?)
        }
        // An opaque `impl Trait` type, which rustc infers.
        convert::Type::Nominal { definition, .. } if is_opaque(definition) => "_".to_owned(),
        ty if let Some(time) = std_time(ty) => time.to_owned(),
        convert::Type::Nominal {
            definition,
            arguments,
        } => {
            let mut text =
                TYPE_PATHS.with_borrow(|paths| paths.get(definition.as_bytes()).cloned())?;
            if !arguments.is_empty() {
                let arguments = arguments
                    .iter()
                    .map(concrete_type)
                    .collect::<Option<Vec<_>>>()?;
                text.push('<');
                text.push_str(&arguments.join(", "));
                text.push('>');
            }
            text
        }
        convert::Type::Option(element) => format!("Option<{}>", concrete_type(element)?),
        convert::Type::Range(element) => format!("std::ops::Range<{}>", concrete_type(element)?),
        convert::Type::Result(ok, err) => {
            format!("Result<{}, {}>", concrete_type(ok)?, concrete_type(err)?)
        }
        convert::Type::ProfileMessage { payload, .. } => concrete_type(payload)?,
        // A script type's proxy, at the runner's crate root, which is named
        // as the script's crate is (D594).
        convert::Type::Script { path, arguments } => {
            let arguments = arguments
                .iter()
                .map(concrete_type)
                .collect::<Option<Vec<_>>>()?;
            let (krate, path) = path.split_once("::")?;
            PROXIES.with_borrow_mut(|proxies| {
                proxies.entry(path.to_owned()).or_insert(arguments.len());
            });
            PROXY_CRATE.with_borrow_mut(|named| {
                named.get_or_insert_with(|| krate.to_owned());
            });
            if arguments.is_empty() {
                format!("crate::{path}")
            } else {
                format!("crate::{path}<{}>", arguments.join(", "))
            }
        }
        convert::Type::Sequence {
            element,
            length: Some(length),
            ..
        } => format!("[{}; {length}]", concrete_type(element)?),
        convert::Type::Sequence { element, .. } => format!("Vec<{}>", concrete_type(element)?),
        convert::Type::Tuple(members) => format!(
            "({},)",
            members
                .iter()
                .map(concrete_type)
                .collect::<Option<Vec<_>>>()?
                .join(", ")
        ),
        // A type argument `&[T]` of script values (`script_slice`).
        convert::Type::Borrowed { mutable: false, ty } if let Some(element) = script_slice(ty) => {
            format!("Vec<{}>", concrete_type(element)?)
        }
        // A type argument `&[T]` or `&str` (D558 tier 1).
        convert::Type::Borrowed { mutable: false, ty } => format!("&{}", borrowed_target(ty)?),
        convert::Type::FunctionItem { .. }
        | convert::Type::Borrowed { .. }
        | convert::Type::Parameter(_) => return None,
    })
}

/// The element of a slice of script values a type argument `&[T]` borrows,
/// as iced's `pick_list(.., Language::ALL, ..)` fills `L: Borrow<[T]>`
/// with a script `&[Language]`. Rawr's array holds no Rust layout, so the
/// runner makes the proxies whichever way it passes them; it passes them
/// as an owned `Vec`, which borrows as the slice does and needs no
/// lifetime to borrow at. So is a slice of crate values, as iced's text
/// example fills it with `self.families.as_slice()` of `font::Family`:
/// Rawr's array holds each in its own arena slot, so the `Vec` holds
/// copies, as `Arena::take` clones a `Clone` value out.
fn script_slice(ty: &convert::Type) -> Option<&convert::Type> {
    match ty {
        convert::Type::Sequence {
            element,
            owned: false,
            ..
        } if matches!(
            element.as_ref(),
            convert::Type::Script { .. } | convert::Type::Nominal { .. }
        ) && !is_duration(element) =>
        {
            Some(element)
        }
        _ => None,
    }
}

/// What a type argument `&[T]` or `&str` borrows: `[T]` or `str`.
fn borrowed_target(ty: &convert::Type) -> Option<String> {
    match ty {
        convert::Type::Sequence {
            element,
            owned: false,
            ..
        } => Some(format!("[{}]", concrete_type(element)?)),
        convert::Type::Builtin(convert::Builtin::Str) => Some("str".to_owned()),
        // A closure's `&S`, as pane_grid's `view(pane, &state, ..)` gives.
        convert::Type::Nominal { .. } | convert::Type::Builtin(_) => concrete_type(ty),
        _ => None,
    }
}

fn render_item_turbofish(
    signature: &crate::classify::representability::signature::Signature,
    substitutions: &[convert::Type],
    constants: &[convert::Constant],
    container_parameters: usize,
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    let partition = partition_generics(signature, substitutions, constants, container_parameters)?;
    render_generic_arguments(
        partition.item_parameters,
        partition.item_types,
        partition.item_constants,
        b"::<",
        output,
    )
}

fn render_generic_arguments(
    parameters: &[crate::classify::representability::signature::Parameter],
    substitutions: &[convert::Type],
    constants: &[convert::Constant],
    opening: &[u8],
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    if parameters.is_empty() {
        if !substitutions.is_empty() || !constants.is_empty() {
            return Err(request::Error::Drift {
                field: "generated substitution cardinality",
            });
        }
        return Ok(());
    }
    let mut type_index = 0_usize;
    let mut constant_index = 0_usize;
    let mut written = 0_usize;
    for parameter in parameters {
        // `F::Output` of a future parameter is no parameter of the item:
        // the instance infers it (D527).
        if parameter.is_future_output() {
            type_index += 1;
            continue;
        }
        // An argument's `impl Trait` takes no generic argument (E0632).
        if parameter.name().starts_with("impl ") || parameter.is_argument_impl() {
            if parameter.kind() == crate::classify::representability::signature::Kind::Type {
                type_index += 1;
            }
            continue;
        }
        output
            .write_all(if written == 0 { opening } else { b", " })
            .map_err(|_| request::Error::Encoding)?;
        written += 1;
        match parameter.kind() {
            crate::classify::representability::signature::Kind::Type => {
                let ty = substitutions.get(type_index).ok_or(request::Error::Drift {
                    field: "generated type substitution cardinality",
                })?;
                type_index += 1;
                let concrete = match ty {
                    convert::Type::Nominal { .. }
                    | convert::Type::Script { .. }
                    | convert::Type::Option(_)
                    | convert::Type::Range(_)
                    | convert::Type::Borrowed { .. }
                    | convert::Type::Result(..) => concrete_type(ty),
                    _ => None,
                };
                output
                    .write_all(match ty {
                        convert::Type::Inferred => b"_",
                        convert::Type::Unit => b"()" as &[u8],
                        // Rust infers the range's integer type from the
                        // call's bounds, as it does for `1..=6`.
                        convert::Type::Builtin(convert::Builtin::InclusiveRange) => b"_",
                        convert::Type::Builtin(convert::Builtin::String) => b"String",
                        convert::Type::Builtin(builtin) => {
                            builtin.rust_name().unwrap_or_default().as_bytes()
                        }
                        // An imported type is written out where its path is
                        // known; Rust infers it otherwise.
                        convert::Type::Nominal { .. }
                        | convert::Type::Script { .. }
                        | convert::Type::Option(_)
                        | convert::Type::Range(_)
                        | convert::Type::Result(..)
                        | convert::Type::Sequence { .. }
                        | convert::Type::Tuple(_) => concrete.as_deref().unwrap_or("_").as_bytes(),
                        // `&[u8]` or `&str`, as `Reader<&'a [u8]>` takes
                        // (D558 tier 1).
                        convert::Type::Borrowed { .. } if concrete.is_some() => {
                            concrete.as_deref().unwrap_or("_").as_bytes()
                        }
                        convert::Type::FunctionItem { .. } | convert::Type::Borrowed { .. } => {
                            return Err(request::Error::Drift {
                                field:
                                    "static function item or borrow used as generic substitution",
                            });
                        }
                        convert::Type::ProfileMessage { payload, .. } => match payload.as_ref() {
                            convert::Type::Unit => b"()",
                            convert::Type::Builtin(builtin) if builtin.rust_name().is_some() => {
                                builtin.rust_name().unwrap_or_default().as_bytes()
                            }
                            convert::Type::Builtin(convert::Builtin::InclusiveRange) => {
                                b"std::ops::RangeInclusive<i64>"
                            }
                            convert::Type::Builtin(convert::Builtin::String) => b"String",
                            _ => {
                                return Err(request::Error::Drift {
                                    field: "non-scalar profile message payload substitution",
                                });
                            }
                        },
                        convert::Type::Parameter(_) => {
                            return Err(request::Error::Drift {
                                field: "unconstrained generated substitution",
                            });
                        }
                    })
                    .map_err(|_| request::Error::Encoding)?;
            }
            crate::classify::representability::signature::Kind::Constant => {
                let constant = constants.get(constant_index).ok_or(request::Error::Drift {
                    field: "generated const substitution cardinality",
                })?;
                constant_index += 1;
                render_constant(constant, output)?;
            }
            crate::classify::representability::signature::Kind::Callable
            | crate::classify::representability::signature::Kind::Future => {
                output
                    .write_all(b"_")
                    .map_err(|_| request::Error::Encoding)?;
            }
        }
    }
    if type_index != substitutions.len() || constant_index != constants.len() {
        return Err(request::Error::Drift {
            field: "generated substitution cardinality",
        });
    }
    if written == 0 {
        return Ok(());
    }
    output.write_all(b">").map_err(|_| request::Error::Encoding)
}

enum ProducerInputs<'a> {
    Types(&'a [convert::Type]),
    Fields(&'a [crate::surface::adapter::Field]),
    Empty,
}

impl ProducerInputs<'_> {
    fn iter(&self) -> impl Iterator<Item = &convert::Type> {
        let types = match self {
            Self::Types(types) => Some(types.iter()),
            Self::Fields(_) | Self::Empty => None,
        };
        let fields = match self {
            Self::Fields(fields) => Some(fields.iter().map(crate::surface::adapter::Field::ty)),
            Self::Types(_) | Self::Empty => None,
        };
        types
            .into_iter()
            .flatten()
            .chain(fields.into_iter().flatten())
    }
}

fn producer_inputs(operation: &Operation) -> Option<ProducerInputs<'_>> {
    let inputs = match operation {
        Operation::StructConstructor { fields, .. } | Operation::EnumVariant { fields, .. } => {
            ProducerInputs::Fields(fields)
        }
        Operation::Function { signature, .. }
            if matches!(signature.output(), convert::Type::Nominal { .. }) =>
        {
            ProducerInputs::Types(signature.inputs())
        }
        Operation::Constant {
            ty: convert::Type::Nominal { .. },
            ..
        } => ProducerInputs::Empty,
        Operation::Declaration { .. }
        | Operation::StructField { .. }
        | Operation::Function { .. }
        | Operation::Method { .. }
        | Operation::Constant { .. }
        | Operation::Builder { .. } => return None,
    };
    let primitive = inputs
        .iter()
        .all(|ty| matches!(ty, convert::Type::Unit | convert::Type::Builtin(_)));
    primitive.then_some(inputs)
}

/// The runner's own execution tests: every module's producers against every
/// module's receivers. Each module lists its own rows, so `src/main.rs` names
/// only the modules.
fn render_execution_tests<'a>(
    modules: impl Iterator<Item = &'a String> + Clone,
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    output
        .write_all(
            b"#[cfg(test)]\nmod generated_execution_tests {\n    use super::*;\n\n\
              fn invoke_one(identity: [u8; 32], handle: NativeHandle, arena: &mut Arena) -> Result<Dynamic, Error> {\n\
                  let adapter = adapters().find(|adapter| adapter.identity == identity).expect(\"generated adapter\");\n\
                  let mut receiver = Dynamic::from(handle);\n\
                  let mut values = [&mut receiver];\n\
                  let mut arguments = Arguments { values: &mut values, arena };\n\
                  (adapter.invoke)(&mut arguments)\n\
              }\n\n\
              const PRODUCERS: &[&[fn(&mut Arena) -> Result<NativeHandle, Error>]] = &[\n",
        )
        .map_err(|_| request::Error::Encoding)?;
    for module in modules.clone() {
        writeln!(output, "        {module}::PRODUCERS,").map_err(|_| request::Error::Encoding)?;
    }
    output
        .write_all(b"    ];\n\n    const ACCEPTS: &[&[([u8; 32], bool)]] = &[\n")
        .map_err(|_| request::Error::Encoding)?;
    for module in modules {
        writeln!(output, "        {module}::ACCEPTS,").map_err(|_| request::Error::Encoding)?;
    }
    output
        .write_all(
            b"    ];\n\n\
              fn assert_accepts(identity: [u8; 32], consumes: bool) {\n\
                  for producer in PRODUCERS.iter().flat_map(|producers| producers.iter()) {\n\
                      let mut arena = Arena::new([11; 32]);\n\
                      let handle = producer(&mut arena).expect(\"nominal producer\");\n\
                      match invoke_one(identity, handle.clone(), &mut arena) {\n\
                          Ok(_) => {\n\
                              if consumes {\n\
                                  assert!(matches!(invoke_one(identity, handle, &mut arena), Err(Error::StaleHandle)));\n\
                              }\n\
                              return;\n\
                          }\n\
                          Err(Error::TypeMismatch) => {}\n\
                          Err(error) => panic!(\"unexpected adapter error: {error:?}\"),\n\
                      }\n\
                  }\n\
                  panic!(\"no nominal producer matched adapter {identity:?}\");\n\
              }\n\n\
              #[test]\n\
              fn nominal_producers_and_receivers_execute_without_cloning_values() {\n\
                  assert!(PRODUCERS.iter().any(|producers| !producers.is_empty()));\n\
                  for (identity, consumes) in ACCEPTS.iter().flat_map(|accepts| accepts.iter()) {\n\
                      assert_accepts(*identity, *consumes);\n\
                  }\n\
              }\n\
              }\n\n",
        )
        .map_err(|_| request::Error::Encoding)
}

/// One adapter module's execution-test rows: its producers, the receivers it
/// asserts, and the single primitive and builder tests when it owns their plan.
fn render_module_execution_tests(
    plans: &[Plan],
    indices: &[usize],
    primitive: Option<usize>,
    builder: Option<usize>,
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    for index in indices {
        let plan = &plans[*index];
        let Some(inputs) = producer_inputs(plan.operation()) else {
            continue;
        };
        writeln!(
            output,
            "#[cfg(test)]\nfn {}(arena: &mut Arena) -> Result<NativeHandle, Error> {{",
            identity_name("producer", plan)
        )
        .map_err(|_| request::Error::Encoding)?;
        render_execution_values(&inputs.iter().collect::<Vec<_>>(), output)?;
        writeln!(
            output,
            "    let mut arguments = Arguments {{ values: &mut values, arena }};\n    let output = {}(&mut arguments)?;\n    if output.is::<NativeHandle>() {{ Ok(output.clone_cast::<NativeHandle>()) }} else {{ Err(Error::TypeMismatch) }}\n}}\n",
            adapter_name(plan)
        )
        .map_err(|_| request::Error::Encoding)?;
    }
    output
        .write_all(
            b"#[cfg(test)]\npub(super) const PRODUCERS: &[fn(&mut Arena) -> Result<NativeHandle, Error>] = &[\n",
        )
        .map_err(|_| request::Error::Encoding)?;
    for index in indices {
        let plan = &plans[*index];
        if producer_inputs(plan.operation()).is_some() {
            writeln!(output, "    {},", identity_name("producer", plan))
                .map_err(|_| request::Error::Encoding)?;
        }
    }
    output
        .write_all(b"];\n\n#[cfg(test)]\npub(super) const ACCEPTS: &[([u8; 32], bool)] = &[\n")
        .map_err(|_| request::Error::Encoding)?;
    for index in indices {
        let plan = &plans[*index];
        let consumes = match plan.operation() {
            Operation::StructField { field, .. } => !copy_scalar(field.ty()),
            Operation::Method {
                receiver,
                signature,
                ..
            } if signature.inputs().is_empty() => matches!(receiver, Receiver::Owned),
            _ => continue,
        };
        output
            .write_all(b"    (")
            .map_err(|_| request::Error::Encoding)?;
        byte_array(output, plan.identity().as_bytes())?;
        writeln!(output, ", {consumes}),").map_err(|_| request::Error::Encoding)?;
    }
    output
        .write_all(b"];\n")
        .map_err(|_| request::Error::Encoding)?;
    if let Some(index) = primitive.filter(|index| indices.contains(index)) {
        writeln!(
            output,
            "\n#[cfg(test)]\n#[test]\nfn primitive_output_executes_without_an_arena_slot() {{\n    let mut arena = Arena::new([12; 32]);\n    let mut values: [&mut Dynamic; 0] = [];\n    let mut arguments = Arguments {{ values: &mut values, arena: &mut arena }};\n    let output = {}(&mut arguments).expect(\"primitive adapter\");\n    assert!(output.is::<i64>());\n    assert!(arena.slots.is_empty());\n}}",
            adapter_name(&plans[index])
        )
        .map_err(|_| request::Error::Encoding)?;
    }
    if let Some(index) = builder.filter(|index| indices.contains(index)) {
        let inputs =
            builder_execution_inputs(plans[index].operation()).ok_or(request::Error::Drift {
                field: "generated builder execution inputs",
            })?;
        output
            .write_all(
                b"\n#[cfg(test)]\n#[test]\nfn consuming_builder_replays_in_one_native_invocation() {\n    let mut arena = Arena::new([13; 32]);\n",
            )
            .map_err(|_| request::Error::Encoding)?;
        render_execution_values(&inputs, output)?;
        writeln!(
            output,
            "    let mut arguments = Arguments {{ values: &mut values, arena: &mut arena }};\n    let output = {}(&mut arguments).expect(\"builder replay\");\n    assert!(output.is::<i64>());\n    assert!(arena.slots.is_empty());\n}}",
            adapter_name(&plans[index])
        )
        .map_err(|_| request::Error::Encoding)?;
    }
    Ok(())
}

fn render_execution_values(
    inputs: &[&convert::Type],
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    for (argument, ty) in inputs.iter().enumerate() {
        write!(output, "    let mut argument_{argument} = ")
            .map_err(|_| request::Error::Encoding)?;
        render_default(ty, output)?;
        output
            .write_all(b";\n")
            .map_err(|_| request::Error::Encoding)?;
    }
    write!(
        output,
        "    let mut values: [&mut Dynamic; {}] = [",
        inputs.len()
    )
    .map_err(|_| request::Error::Encoding)?;
    for argument in 0..inputs.len() {
        if argument != 0 {
            output
                .write_all(b", ")
                .map_err(|_| request::Error::Encoding)?;
        }
        write!(output, "&mut argument_{argument}").map_err(|_| request::Error::Encoding)?;
    }
    output
        .write_all(b"];\n")
        .map_err(|_| request::Error::Encoding)
}

fn builder_execution_inputs(operation: &Operation) -> Option<Vec<&convert::Type>> {
    let Operation::Builder { steps } = operation else {
        return None;
    };
    let count = steps.iter().try_fold(0_usize, |count, step| {
        count.checked_add(step.arguments().len())
    })?;
    let mut inputs = vec![None; count];
    for step in steps {
        let types = match step.plan().operation() {
            Operation::Function {
                signature,
                substitutions,
                ..
            }
            | Operation::Method {
                signature,
                substitutions,
                ..
            } if substitutions.is_empty() => signature.inputs(),
            Operation::StructConstructor { fields, .. } => {
                if fields.len() != step.arguments().len() {
                    return None;
                }
                for (field, position) in fields.iter().zip(step.arguments()) {
                    let slot = inputs.get_mut(*position as usize)?;
                    if slot.replace(field.ty()).is_some() {
                        return None;
                    }
                }
                continue;
            }
            _ => return None,
        };
        if types.len() != step.arguments().len() {
            return None;
        }
        for (ty, position) in types.iter().zip(step.arguments()) {
            let slot = inputs.get_mut(*position as usize)?;
            if slot.replace(ty).is_some() {
                return None;
            }
        }
    }
    let output = match steps.last()?.plan().operation() {
        Operation::Method {
            signature,
            substitutions,
            ..
        } if substitutions.is_empty() => signature.output(),
        _ => return None,
    };
    if !matches!(output, convert::Type::Builtin(convert::Builtin::I64)) {
        return None;
    }
    inputs
        .into_iter()
        .collect::<Option<Vec<_>>>()
        .filter(|types| {
            types
                .iter()
                .all(|ty| matches!(ty, convert::Type::Unit | convert::Type::Builtin(_)))
        })
}

fn render_default(ty: &convert::Type, output: &mut Vec<u8>) -> Result<(), request::Error> {
    let source = match ty {
        convert::Type::Inferred => {
            return Err(request::Error::Drift {
                field: "an inferred type parameter reached a runtime value",
            })
        }
        convert::Type::Unit => "Dynamic::from(())",
        convert::Type::Builtin(convert::Builtin::Bool) => "Dynamic::from(false)",
        convert::Type::Builtin(convert::Builtin::Char) => "Dynamic::from('\\0')",
        convert::Type::Builtin(convert::Builtin::F64) => "Dynamic::from(0.0_f64)",
        convert::Type::Builtin(convert::Builtin::I64) => "Dynamic::from(0_i64)",
        convert::Type::Builtin(
            convert::Builtin::I8
            | convert::Builtin::I16
            | convert::Builtin::I32
            | convert::Builtin::Isize
            | convert::Builtin::U8
            | convert::Builtin::U16
            | convert::Builtin::U32
            | convert::Builtin::U64
            | convert::Builtin::Usize,
        ) => "Dynamic::from(0_i64)",
        convert::Type::Builtin(convert::Builtin::F32) => "Dynamic::from(0.0_f64)",
        convert::Type::Builtin(convert::Builtin::InclusiveRange) => "Dynamic::from(0_i64..=0_i64)",
        convert::Type::Builtin(
            convert::Builtin::String
            | convert::Builtin::Str
            | convert::Builtin::StaticStr
            | convert::Builtin::Text,
        ) => "Dynamic::from(\"\")",
        convert::Type::Option(_) => "__rawr_internal::option::none()",
        convert::Type::Range(element)
            if matches!(
                element.as_ref(),
                convert::Type::Builtin(convert::Builtin::F32 | convert::Builtin::F64)
            ) =>
        {
            "Dynamic::from(0.0_f64..0.0_f64)"
        }
        convert::Type::Range(_) => "Dynamic::from(0_i64..0_i64)",
        // `Result` has no `Default`, as in Rust.
        convert::Type::Result(..)
        | convert::Type::Nominal { .. }
        | convert::Type::Script { .. }
        | convert::Type::Sequence { .. }
        | convert::Type::Tuple(_)
        | convert::Type::FunctionItem { .. }
        | convert::Type::Borrowed { .. }
        | convert::Type::Parameter(_) => {
            return Err(request::Error::Drift {
                field: "generated execution producer input",
            });
        }
        convert::Type::ProfileMessage { payload, .. } => {
            return render_default(payload, output);
        }
    };
    output
        .write_all(source.as_bytes())
        .map_err(|_| request::Error::Encoding)
}

fn argument_list(count: usize, _offset: usize, output: &mut Vec<u8>) -> Result<(), request::Error> {
    for index in 0..count {
        if index != 0 {
            output
                .write_all(b", ")
                .map_err(|_| request::Error::Encoding)?;
        }
        write!(output, "argument_{index}").map_err(|_| request::Error::Encoding)?;
    }
    Ok(())
}

fn byte_array(output: &mut Vec<u8>, bytes: &[u8]) -> Result<(), request::Error> {
    output
        .write_all(b"[")
        .map_err(|_| request::Error::Encoding)?;
    for (index, byte) in bytes.iter().enumerate() {
        if index != 0 {
            output
                .write_all(b", ")
                .map_err(|_| request::Error::Encoding)?;
        }
        write!(output, "0x{byte:02x}").map_err(|_| request::Error::Encoding)?;
    }
    output.write_all(b"]").map_err(|_| request::Error::Encoding)
}

fn signature(plan: &Plan) -> Result<[u8; 32], request::Error> {
    let mut writer = Hashing(Sha256::new());
    serde_json::to_writer(&mut writer, plan).map_err(|_| request::Error::Encoding)?;
    Ok(writer.0.finalize().into())
}

fn digest(bytes: &[u8]) -> FileDigest {
    (Sha256::digest(bytes).into(), bytes.len())
}

struct Hashing(Sha256);

impl Write for Hashing {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        projected_event_message, render_container_path, render_item_turbofish,
        LIFECYCLE_PROFILE_RUNNER_MAIN, NEUTRAL_PROFILE_SOURCE, PROFILE_RUNNER_MAIN, RUNNER_MAIN,
        RUNTIME_SOURCE,
    };
    use crate::classify::representability::convert::{Builtin, Constant, Type};
    use crate::classify::representability::signature::{Kind, Parameter, Signature};

    /// A script type in a generic item crosses as `rawr::Value`, by value,
    /// and comes back checked against the type it stands for (D513.2).
    #[test]
    fn an_erased_script_type_crosses_as_rawr_value_and_comes_back_checked() {
        use crate::surface::identity::Definition;
        let value = || Type::Nominal {
            definition: Definition::value(),
            arguments: Box::default(),
        };
        assert_eq!(
            super::concrete_type(&value()).as_deref(),
            Some("__rawr_internal::Value")
        );
        let mut take = Vec::new();
        super::render_take(0, 1, &value(), &mut take).unwrap();
        assert_eq!(
            String::from_utf8(take).unwrap(),
            "    let argument_0 = take_value(arguments, 1)?;\n"
        );
        let mut give = Vec::new();
        super::render_give(&value(), "output", &mut give).unwrap();
        assert_eq!(
            String::from_utf8(give).unwrap(),
            "    Ok(output.into_inner())\n"
        );
        // `Receiver<T>::recv` gives a future of `Option<T>`: `T` is erased at
        // any depth, and the future's output is checked when it comes back.
        let signature = Signature::new(
            Box::from([Parameter::new(0, "T".into(), Kind::Type)]),
            Box::default(),
            Type::Nominal {
                definition: Definition::future(),
                arguments: Box::from([Type::Option(Box::new(Type::Parameter(0)))]),
            },
        );
        let output = super::substituted(signature.output(), &signature, &[value()]).unwrap();
        let Type::Nominal { arguments, .. } = &output else {
            panic!("a future");
        };
        assert_eq!(arguments[0], Type::Option(Box::new(value())));
        assert!(super::holds_value(&arguments[0]));
        let mut task = Vec::new();
        super::render_give_task(&arguments[0], "output", &mut task).unwrap();
        let task = String::from_utf8(task).unwrap();
        assert!(task.contains("materialize_checked(witness"), "{task}");
        assert!(
            task.contains("Some(inner_0) => (Ok::<Dynamic, Error>(inner_0.into_inner()))"),
            "{task}"
        );
        // A native value of an erased instance is attested by its own
        // witness, not checked as a script value.
        let sender = Type::Nominal {
            definition: Definition::sipper(),
            arguments: Box::from([value()]),
        };
        assert!(!super::holds_value(&sender));
    }

    /// A script closure fills `F: FnOnce() -> R`: a Rust closure that calls
    /// it with an evaluation of its own (D513.7 b).
    #[test]
    fn a_script_closure_fills_an_fn_once_parameter() {
        use crate::classify::representability::callable::Call;
        use crate::surface::identity::Definition;
        let mut callable = Parameter::new(0, "F".into(), Kind::Type);
        callable.mark_callable();
        let signature = Signature::new(
            Box::from([callable, Parameter::new(1, "R".into(), Kind::Type)]),
            Box::from([Type::Parameter(0)]),
            Type::Unit,
        );
        assert!(super::callable_parameter(&Type::Parameter(0), &signature));
        assert!(!super::callable_parameter(&Type::Parameter(1), &signature));
        let call = Call::new(0, Box::default(), Type::Parameter(1));
        let value = Type::Nominal {
            definition: Definition::value(),
            arguments: Box::default(),
        };
        let mut take = Vec::new();
        super::render_take_callable(0, 0, &call, &signature, &[value], &mut take).unwrap();
        let take = String::from_utf8(take).unwrap();
        assert!(
            take.contains("__rawr_internal::future::Callable::new((**arguments.values.get_mut(0)"),
            "{take}"
        );
        assert!(
            take.contains("move || -> __rawr_internal::Value {"),
            "{take}"
        );
        assert!(take.contains("let mut values = [];"), "{take}");
        assert!(take.contains("callable.call(&mut values)"), "{take}");
        // A closure that gives a native value Rust can't name can't be
        // made. One it can name goes through the arena (D551.6), as the
        // erasure fixture's `WindowConfig::new(app)` runs end to end.
        let native = Call::new(
            0,
            Box::default(),
            Type::Nominal {
                definition: Definition::sipper(),
                arguments: Box::default(),
            },
        );
        assert!(super::render_take_callable(
            0,
            0,
            &native,
            &signature,
            &[Type::Unit],
            &mut Vec::new()
        )
        .is_err());
    }

    /// The runner of a program with no host profile: a Rust future is the
    /// core's future, a borrowed receiver is lent to its future, and a
    /// native value drops with its last handle (D513.3, D513.6).
    #[test]
    fn the_plain_runner_lends_receivers_and_releases_native_values() {
        let neutral = std::str::from_utf8(NEUTRAL_PROFILE_SOURCE).unwrap();
        assert!(neutral.contains("mod __rawr_profile_host {"), "{neutral}");
        assert!(
            neutral.contains("__rawr_internal::future::Future::native(move || {"),
            "{neutral}"
        );
        assert!(
            neutral.contains("__rawr_internal::future::facade::definition()"),
            "{neutral}"
        );
        assert!(
            neutral.contains("__rawr_internal::island::await_main(value.cast());"),
            "{neutral}"
        );
        let runtime = std::str::from_utf8(RUNTIME_SOURCE).unwrap();
        for expected in [
            "fn lease<T: Send + 'static>(&mut self, handle: NativeHandle) -> Result<Held<T>, Error> {",
            "impl<T: Send + 'static> Drop for Held<T> {",
            "impl<T: Clone + Send + 'static> HoldClone<T> for HoldProbe<T> {",
            "impl Drop for Release {",
            "fn release_dropped(&mut self) {",
            "if entry.generation == handle.generation && entry.leased {",
        ] {
            assert!(runtime.contains(expected), "{expected}");
        }
    }

    /// T204: `&(f64, f64)` and `&[u8]` lend the Rust value made from
    /// Rawr's tuple or array, and the attestation of a call's output reaches
    /// into a tuple inside a `Result`.
    #[test]
    fn a_tuple_or_slice_reference_lends_the_value_made_for_the_call() {
        let signature = Signature::new(Box::default(), Box::default(), Type::Unit);
        for (lent, rust) in [
            (
                Type::Tuple(Box::from([
                    Type::Builtin(Builtin::F64),
                    Type::Builtin(Builtin::F64),
                ])),
                "into_array()",
            ),
            (
                Type::Sequence {
                    element: Box::new(Type::Builtin(Builtin::U8)),
                    owned: false,
                    length: None,
                },
                "int_from::<u8>",
            ),
        ] {
            let mut take = Vec::new();
            super::render_take_input(
                2,
                2,
                &Type::Borrowed {
                    mutable: false,
                    ty: Box::new(lent),
                },
                &signature,
                &[],
                None,
                &mut take,
            )
            .unwrap();
            let take = String::from_utf8(take).unwrap();
            assert!(take.contains("let argument_2_made = {"), "{take}");
            assert!(take.contains(rust), "{take}");
            assert!(
                take.contains("let argument_2 = &argument_2_made;"),
                "{take}"
            );
        }
        let runtime = std::str::from_utf8(RUNTIME_SOURCE).unwrap();
        assert!(
            runtime.contains("attest_elements(inner, &witness, arena)?;"),
            "a tuple inside a Result is attested"
        );
    }

    #[test]
    fn a_str_input_is_lent_and_a_display_output_is_its_text() {
        let mut take = Vec::new();
        super::render_take(1, 1, &Type::Builtin(Builtin::Str), &mut take).unwrap();
        let take = String::from_utf8(take).unwrap();
        assert!(
            take.contains(
                "let argument_1_owned = take_primitive::<ImmutableString>(arguments, 1)?;"
            ),
            "{take}"
        );
        assert!(
            take.contains("let argument_1 = argument_1_owned.as_str();"),
            "{take}"
        );
        let mut give = Vec::new();
        super::render_give(&Type::Builtin(Builtin::Text), "output", &mut give).unwrap();
        let give = String::from_utf8(give).unwrap();
        assert!(give.contains("Dynamic::from(output.to_string())"), "{give}");
    }

    #[test]
    fn an_imported_copy_type_is_read_as_a_copy() {
        let nominal = |byte| Type::Nominal {
            definition: crate::surface::identity::Definition::authenticated([byte; 32]),
            arguments: Box::default(),
        };
        super::COPY_TYPES.with_borrow_mut(|copy| copy.insert([3; 32]));
        assert!(super::copy_scalar(&nominal(3)));
        assert!(super::copy_scalar(&Type::Option(Box::new(nominal(3)))));
        assert!(!super::copy_scalar(&nominal(4)));
        super::COPY_TYPES.with_borrow_mut(std::collections::BTreeSet::clear);
    }

    #[test]
    fn an_option_of_a_scalar_is_read_as_a_copy() {
        let usize = || Type::Builtin(Builtin::Usize);
        assert!(super::copy_scalar(&Type::Option(Box::new(usize()))));
        assert!(super::copy_scalar(&Type::Result(
            Box::new(usize()),
            Box::new(Type::Unit)
        )));
        assert!(!super::copy_scalar(&Type::Option(Box::new(Type::Builtin(
            Builtin::String
        )))));
    }

    #[test]
    fn runner_manifest_keeps_the_projects_profile() {
        let harness = b"[workspace]\nresolver = \"3\"\n\n[package]\nname = \"rawr-resolution-harness\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\n[lib]\npath = \"src/lib.rs\"\n\n[profile.dev.package.\"*\"]\nopt-level = 3\n\n[dependencies]\niced = \"0.14\"\n";
        let manifest = super::runner_manifest(harness, None, &[]).unwrap();
        let document = std::str::from_utf8(&manifest)
            .unwrap()
            .parse::<toml_edit::DocumentMut>()
            .unwrap();
        let dev = &document["profile"]["dev"];
        // The project's `cargo build` takes its profile as written, and so
        // does the runner: a setting Rawr adds would change every unit's hash.
        assert!(dev.get("incremental").is_none());
        assert!(dev["package"].get("rawr-generated-runner").is_none());
        assert_eq!(dev["package"]["*"]["opt-level"].as_integer(), Some(3));
        assert_eq!(
            document["package"]["name"].as_str(),
            Some("rawr-generated-runner")
        );
        assert!(document.get("lib").is_none());
        // The harness's build dependency on Rawr's own crate, which it never
        // builds, is a dependency of the runner, and its build script has none.
        let harness = b"[workspace]\n\n[package]\nname = \"rawr-resolution-harness\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\n[lib]\npath = \"src/lib.rs\"\n\n[dependencies]\nzeta = \"1\"\n\n[build-dependencies]\n__rawr_internal = { package = \"rawr\", path = \"/rawr\" }\n";
        let manifest = super::runner_manifest(harness, None, &[]).unwrap();
        let document = std::str::from_utf8(&manifest)
            .unwrap()
            .parse::<toml_edit::DocumentMut>()
            .unwrap();
        assert!(document.get("build-dependencies").is_none());
        let dependencies = document["dependencies"].as_table().unwrap();
        assert_eq!(
            dependencies
                .iter()
                .map(|(name, _)| name)
                .collect::<Vec<_>>(),
            ["__rawr_internal", "zeta"]
        );
    }

    #[test]
    fn generated_compiler_retains_one_native_account_before_initial_compilation() {
        let runtime = std::str::from_utf8(RUNTIME_SOURCE).expect("runtime source is UTF-8");
        let compile = runtime
            .split_once("fn compile_program(")
            .expect("initial compiler")
            .1
            .split_once("fn compile_live(")
            .expect("live compiler")
            .0;
        let mut remaining = compile;
        for operation in [
            "if !COMPILE_SNAPSHOT { return Ok(None); }",
            "account::Native::decode_observed(GENERATED_MANIFEST)",
            "install_compiler_facades(&mut engine, &mut native)?",
            "admit_generated_structural_retained(\n        &mut engine,\n        &facades,\n        STRUCTURAL_ROWS,\n        &mut native,",
            "engine.retain_imported_native(native)",
            "__rawr_profile::compile_initial(",
            "Ok(Some(Program { engine,",
        ] {
            remaining = remaining
                .split_once(operation)
                .unwrap_or_else(|| panic!("missing or reordered generated operation: {operation}"))
                .1;
        }
        assert_eq!(compile.matches("Native::decode_observed(").count(), 1);
        assert!(compile.contains("Compilation::Replacement(current) => current.as_ref()"));
        assert!(compile.contains(".engine.reconstruct_imported_native(GENERATED_MANIFEST)"));
        assert!(!compile.contains("Shared::new(engine)"));
        assert!(!compile.contains("admit_generated_structural("));
        for source in [RUNNER_MAIN, PROFILE_RUNNER_MAIN] {
            let source = std::str::from_utf8(source).expect("runner source is UTF-8");
            assert_eq!(source.matches("Compilation::Initial").count(), 1);
            assert_eq!(
                source.matches("Compilation::Replacement(&program)").count(),
                1
            );
            assert!(source.contains("3 if !prepared && payload.len() >= 48"));
        }
        let lifecycle =
            std::str::from_utf8(LIFECYCLE_PROFILE_RUNNER_MAIN).expect("lifecycle source is UTF-8");
        assert_eq!(lifecycle.matches("Compilation::Initial").count(), 1);
        assert!(!lifecycle.contains("Compilation::Replacement("));

        let neutral =
            std::str::from_utf8(NEUTRAL_PROFILE_SOURCE).expect("neutral profile source is UTF-8");
        assert!(neutral
            .contains("native: &mut __rawr_internal::package::import::runner::account::Native,"));
        assert!(
            neutral.contains("runner::install_retained(engine, super::IMPORTED_FACADES, native)")
        );
        assert!(!neutral.contains("runner::install("));
        assert!(
            neutral.contains("engine.compile_package_snapshot_generation_with_top_level(snapshot)")
        );
        // A runner with no host profile starts its program by calling `main`
        // bound to the imported runtime.
        assert!(neutral.contains("candidate.bind(&program.engine, runtime)"));
        assert!(neutral.contains("__rawr_internal::island::invoke_main()"));
        // A native `main` (D501) is started after the Rawr one registers it,
        // and each committed generation is published for its calls.
        assert!(neutral.contains("__rawr_internal::island::start(forward)"));
        let runner = std::str::from_utf8(RUNNER_MAIN).expect("runner source is UTF-8");
        assert!(runner.contains("20 if admitted && payload.as_slice() == key"));
        assert!(runner.contains("__rawr_profile::run(program, forward)"));
        assert!(runner.contains("__rawr_profile::publish(program)"));
        assert!(runner.contains("Ok(__RawrProtocolEvent::Call(call)) => call.answer(),"));
        // The process's main thread is kept for a native `main`, and the
        // protocol runs beside it. A marked `main` runs the protocol, and
        // then itself, on the main thread.
        let main = runner.split_once("fn main() {").expect("runner main").1;
        assert!(main.contains(
            "if marked_main(&key, &first) {\n        finish(run_protocol(stream, key, first));"
        ));
        assert!(main.contains("__rawr_internal::island::keep_main_thread()"));
        assert!(main.contains(".spawn(move || finish(run_protocol(stream, key, first)))"));
        assert!(main.contains("main_thread.serve()"));
    }

    #[test]
    fn generated_resident_exchange_uses_actual_output_and_original_installation() {
        let source =
            std::str::from_utf8(LIFECYCLE_PROFILE_RUNNER_MAIN).expect("lifecycle source is UTF-8");
        let compile = source
            .split_once("fn prepare_resident(")
            .expect("resident compiler")
            .1
            .split_once("fn resident_response(")
            .expect("resident response")
            .0;
        assert_eq!(
            compile.matches("__rawr_profile::compile_resident(").count(),
            1
        );
        assert!(!compile.contains("compile_live("));
        assert!(!compile.contains("expected"));
        let prepare = source
            .split_once("37 if pending.is_none()")
            .expect("resident request")
            .1
            .split_once("37 =>")
            .expect("invalid resident transition")
            .0;
        for operation in [
            "begun.take()",
            "wire::Attempt::decode(",
            "&payload, &identity",
            "account_result(decode_snapshot(input)",
            "send_progress(&mut stream, &key, &nonce, 3, ordinal, stage)",
            "compiled.into_parts()",
            "receipt: compiled.advance_receipt(receipt)",
            "transport.cancel_install(installation)",
            "attempt.response_header()",
        ] {
            assert!(prepare.contains(operation), "missing {operation}");
        }
        assert!(!prepare.contains("begin_install()"));
        assert!(!prepare.contains("COMPILED_SEMANTIC"));
    }

    #[test]
    fn plain_runner_compiles_once_against_its_running_engine() {
        let source = std::str::from_utf8(RUNNER_MAIN).expect("runner source is UTF-8");
        let compile = source
            .split_once("fn compile_resident(")
            .expect("resident compiler")
            .1
            .split_once("fn resident_response(")
            .expect("resident response")
            .0;
        assert_eq!(
            compile.matches("__rawr_profile::compile_resident(").count(),
            1
        );
        // No second compilation, and no new engine: the candidate and its
        // top level come from the one resident compilation.
        assert!(!compile.contains("compile_program("));
        assert!(!compile.contains("reconstruct_imported_native"));
        assert!(compile.contains("compiled.with_top_level()"));
        assert!(compile.contains("engine: resident.engine.clone()"));
        // A generation the running `main` can't take goes to full
        // preparation, which refuses it with its reason.
        assert!(compile.contains("island::refusal(running, &candidate)"));
        let serve = source
            .split_once("fn serve_hot(")
            .expect("resident transitions")
            .1
            .split_once("_ => return Ok(false)")
            .expect("other transitions")
            .0;
        let resident = serve
            .split_once("37 if pending.is_none()")
            .expect("resident request")
            .1
            .split_once("37 =>")
            .expect("invalid resident transition")
            .0;
        for operation in [
            "hot.begun.take()",
            "wire::Attempt::decode(payload, &begun.identity)",
            "account_result(decode_snapshot(input)",
            "attempt.compiled(carrier, coverage, semantic)",
            "send_frame(stream, 38, compiled.encode())",
            "attempt.response_header()",
        ] {
            assert!(resident.contains(operation), "missing {operation}");
        }
        assert!(serve.contains("replayed hot begin"));
        // Both protocol loops serve it: a native `main`'s and a marked one's.
        assert_eq!(
            source
                .matches("if serve_hot(&mut stream, tag, &payload")
                .count(),
            2
        );
        let neutral =
            std::str::from_utf8(NEUTRAL_PROFILE_SOURCE).expect("neutral profile source is UTF-8");
        assert!(neutral.contains("engine.compile_package_snapshot_resident_plain(snapshot)"));
    }

    #[test]
    #[cfg(feature = "test-fixtures")]
    fn trusted_runner_preserves_identity_rendering_limits_and_exact_ancestry() {
        use crate::surface::{Runner, Snapshot};
        use std::cell::Cell;

        struct Effectful<'a> {
            calls: &'a Cell<usize>,
            facade: &'a [u8],
        }
        impl Runner for Effectful<'_> {
            fn compile_snapshot(&self) -> bool {
                self.calls.set(self.calls.get() + 1);
                true
            }
            fn facade_plan(&self) -> Option<&[u8]> {
                self.calls.set(self.calls.get() + 1);
                Some(self.facade)
            }
        }

        let root = std::env::temp_dir().join(format!("rawr-owned-runner-{}", std::process::id()));
        std::fs::create_dir(&root).expect("exclusive owned Runner fixture");
        let root = std::fs::canonicalize(root).expect("canonical fixture root");
        let home = root.join("cargo-home");
        std::fs::create_dir(&home).expect("fixture Cargo home");
        let limits = crate::cargo::Limits::standard();
        let context = || {
            crate::context::Context::capture(
                &root,
                crate::cargo::config::Environment::from_pairs([(
                    "CARGO_HOME",
                    home.to_str().expect("Unicode fixture path"),
                )])
                .expect("fixture environment"),
                "aarch64-apple-darwin",
                &limits,
            )
            .expect("fixture context")
        };
        let classify = |context| {
            let parent = crate::node::Chain::fixture(context).expect("fixture parent");
            crate::test_fixture::graph()
                .with_node_chain_for_test(parent)
                .classify(&crate::classify::Policy::standard(), &limits)
                .expect("fixture contract")
        };
        let contract = classify(context());
        let facade = crate::surface::facade::Plan::encode_cached(&contract, &limits)
            .expect("authenticated facade");
        let calls = Cell::new(0);
        let demand = || {
            contract
                .compiler(
                    Effectful {
                        calls: &calls,
                        facade: facade.bytes(),
                    },
                    Snapshot::from_rawr_digest([0x35; 32]),
                    &limits,
                )
                .expect("fixture compiler")
                .seal()
                .expect("fixture demand")
        };
        let ordinary = demand().into_request();
        let before = demand().cached_retained_usage();
        let trusted = demand()
            .with_trusted_runner(true, &facade, &contract)
            .expect("explicit trusted selection");
        assert_eq!(
            trusted.cached_retained_usage(),
            (before.0 + 1, before.1 + facade.bytes().len())
        );
        let trusted = trusted.into_request();
        let harness = crate::input::proof::Proof::fixture_runner_harness_manifest(&root)
            .expect("fixture harness manifest");
        std::fs::write(
            harness.with_file_name("Cargo.lock"),
            b"version = 4\n\n[[package]]\nname = \"rawr-resolution-harness\"\nversion = \"0.0.0\"\n",
        )
        .expect("fixture harness lock");
        let native = crate::input::proof::Proof::fixture_runner_harness(&root)
            .expect("real fixture harness");
        let profile = crate::profile::Proof::neutral_with_chain(facade.node_chain().clone());
        trusted
            .require_runner_profile(&profile)
            .expect("exact facade in profile closure");
        let trusted_digest = super::semantic_digest(&trusted).expect("owned digest");
        let trusted_render = super::render(&trusted, &native, &profile).expect("owned render");
        assert_eq!(
            calls.get(),
            0,
            "owned path never invokes arbitrary Runner callbacks"
        );
        assert_eq!(
            trusted_digest,
            super::semantic_digest(&ordinary).expect("ordinary digest")
        );
        let ordinary_render = super::render(&ordinary, &native, &profile).expect("ordinary render");
        assert_eq!(
            calls.get(),
            4,
            "ordinary digest and rendering each retain both callbacks"
        );
        assert_eq!(trusted_render.manifest, ordinary_render.manifest);
        assert_eq!(trusted_render.lock, ordinary_render.lock);
        assert_eq!(trusted_render.source, ordinary_render.source);
        assert_eq!(trusted_render.source_map, ordinary_render.source_map);
        super::semantic_digest(&ordinary).expect("repeated ordinary digest");
        super::render(&ordinary, &native, &profile).expect("repeated ordinary rendering");
        assert_eq!(
            calls.get(),
            8,
            "ordinary callbacks remain effectful on every use"
        );
        assert_eq!(
            super::super::build_request_key(&trusted, &native, &profile).expect("owned build key"),
            super::super::build_request_key(&ordinary, &native, &profile)
                .expect("ordinary build key"),
        );

        let alternate_mode = demand()
            .with_trusted_runner(false, &facade, &contract)
            .expect("owned noncompiling mode")
            .into_request();
        assert_ne!(
            trusted_digest,
            super::semantic_digest(&alternate_mode).expect("mode identity")
        );
        assert_ne!(
            trusted_render.source,
            super::render(&alternate_mode, &native, &profile)
                .expect("mode rendering")
                .source
        );
        assert!(matches!(
            demand()
                .with_trusted_runner(true, &facade, &contract)
                .expect("first selection")
                .with_trusted_runner(true, &facade, &contract),
            Err(crate::surface::Error::ForeignRunner)
        ));
        // A distinct admitted parent in the same account prevents the two
        // different fixture graphs from aliasing the original cache recipe.
        let other_parent = facade.node_chain().clone();
        let other_contract = crate::test_fixture::support_outcomes()
            .with_node_chain_for_test(other_parent)
            .classify(&crate::classify::Policy::standard(), &limits)
            .expect("same account alternate contract");
        let other_facade = crate::surface::facade::Plan::encode_cached(&other_contract, &limits)
            .expect("same account alternate facade");
        assert!(matches!(
            demand().with_trusted_runner(true, &other_facade, &contract),
            Err(crate::surface::Error::ForeignRunner)
        ));

        let foreign = classify(context());
        assert_eq!(foreign.fingerprint(), contract.fingerprint());
        let foreign_facade =
            crate::surface::facade::Plan::encode_cached(&foreign, &limits).expect("foreign facade");
        assert!(matches!(
            demand().with_trusted_runner(true, &foreign_facade, &contract),
            Err(crate::surface::Error::ForeignRunner)
        ));
        assert!(matches!(
            demand().with_trusted_runner(true, &facade, &foreign),
            Err(crate::surface::Error::ForeignRunner)
        ));
        let foreign_profile =
            crate::profile::Proof::neutral_with_chain(foreign_facade.node_chain().clone());
        assert!(trusted.require_runner_profile(&foreign_profile).is_err());
        let missing_facade = crate::profile::Proof::neutral_with_chain(
            contract.node_chain().expect("contract chain").clone(),
        );
        assert!(trusted.require_runner_profile(&missing_facade).is_err());

        // The classified Contract limits are accounting targets. One
        // short on either axis still selects the trusted runner, charging the
        // exact facade retention; the crossing is recorded, never refused.
        let mut short = limits;
        short.classified_contract_bytes = before.1 + facade.bytes().len() - 1;
        let bounded = contract
            .compiler((), Snapshot::from_rawr_digest([0x35; 32]), &short)
            .expect("bounded compiler")
            .seal()
            .expect("bounded demand")
            .with_trusted_runner(true, &facade, &contract)
            .expect("the byte target records and admits");
        assert_eq!(
            bounded.cached_retained_usage(),
            (before.0 + 1, before.1 + facade.bytes().len())
        );
        let mut item_short = limits;
        item_short.classified_contract_items = before.0;
        let bounded = contract
            .compiler((), Snapshot::from_rawr_digest([0x35; 32]), &item_short)
            .expect("item bounded compiler")
            .seal()
            .expect("item bounded demand")
            .with_trusted_runner(true, &facade, &contract)
            .expect("the item target records and admits");
        assert_eq!(
            bounded.cached_retained_usage(),
            (before.0 + 1, before.1 + facade.bytes().len())
        );
        std::fs::remove_dir_all(root).expect("remove fixture");
    }

    /// The generated retained bytes are an accounting target recorded on
    /// the bridge account. One byte under the generated size still assembles
    /// the manifest and records the truthful figure. The files and inventory
    /// together are charged too: a target that each passes alone but their
    /// sum crosses records the combined axis; exactly at the sum is silent.
    #[test]
    fn generated_retained_bytes_past_the_json_target_assemble_and_record() {
        use crate::surface::Snapshot;

        let root =
            std::env::temp_dir().join(format!("rawr-generated-target-{}", std::process::id()));
        std::fs::create_dir(&root).expect("exclusive generated-target fixture");
        let root = std::fs::canonicalize(root).expect("canonical fixture root");
        let home = root.join("cargo-home");
        std::fs::create_dir(&home).expect("fixture Cargo home");
        let standard = crate::cargo::Limits::standard();
        let context = crate::context::Context::capture(
            &root,
            crate::cargo::config::Environment::from_pairs([(
                "CARGO_HOME",
                home.to_str().expect("Unicode fixture path"),
            )])
            .expect("fixture environment"),
            "aarch64-apple-darwin",
            &standard,
        )
        .expect("fixture context");
        let parent = crate::node::Chain::fixture(context).expect("fixture parent");
        let contract = crate::test_fixture::graph()
            .with_node_chain_for_test(parent)
            .classify(&crate::classify::Policy::standard(), &standard)
            .expect("fixture contract");
        let harness = crate::input::proof::Proof::fixture_runner_harness_manifest(&root)
            .expect("fixture harness manifest");
        std::fs::write(
            harness.with_file_name("Cargo.lock"),
            b"version = 4\n\n[[package]]\nname = \"rawr-resolution-harness\"\nversion = \"0.0.0\"\n",
        )
        .expect("fixture harness lock");
        let chain = contract.node_chain().expect("contract chain").clone();
        let mut owner = crate::supervisor::Owner::new();
        let mut assemble = |json_bytes: Option<usize>| {
            let profile = crate::profile::Proof::neutral_with_chain(chain.clone());
            let request = contract
                .compiler((), Snapshot::from_rawr_digest([0x36; 32]), &standard)
                .expect("fixture compiler")
                .seal()
                .expect("fixture demand")
                .into_request();
            let native = crate::input::proof::Proof::fixture_runner_harness(&root)
                .expect("real fixture harness");
            let product = super::render(&request, &native, &profile).expect("rendering");
            let total = product.manifest.len()
                + product.lock.len()
                + super::BUILD_SCRIPT.len()
                + product.source.len()
                + product.source_map.len();
            let actual = super::expected(&request, &product).expect("expected digests");
            let build =
                super::super::build_request_key(&request, &native, &profile).expect("build key");
            let key = super::super::GenerationRequestKey {
                request,
                native,
                profile: profile.into(),
                revision: owner.revision().expect("revision"),
                build,
            };
            let mut limits = standard;
            limits.json_bytes = json_bytes.unwrap_or(total);
            let mut account = crate::surface::account::Account::bridge();
            let manifest =
                super::assemble_accounted(key, product, actual, None, &limits, &mut account)
                    .expect("the generated manifest assembles under every JSON target");
            assert!(
                manifest.files.len() >= 4,
                "src/main.rs and any adapter modules"
            );
            (total, account.exceedances())
        };
        let (total, tiny) = assemble(Some(1));
        let combined = tiny
            .axis(super::GENERATED_COMBINED_RESOURCE)
            .expect("the combined axis records")
            .high_water as usize;
        let inventory = tiny
            .axis("resident inventory retained bytes")
            .expect("the inventory axis records")
            .high_water as usize;
        assert_eq!(combined, total + inventory);
        let (_, exact) = assemble(Some(combined));
        assert!(
            !exact.is_exceeded(),
            "exactly at the combined total is silent"
        );
        // Neither the files nor the inventory alone pass this target; only
        // their sum does, and that crossing is recorded.
        let (_, together) = assemble(Some(combined - 1));
        assert!(together.axis("generated retained bytes").is_none());
        assert!(together.axis("resident inventory retained bytes").is_none());
        let axis = *together
            .axis(super::GENERATED_COMBINED_RESOURCE)
            .expect("the combined crossing records");
        assert_eq!(
            (axis.target, axis.first_requested, axis.high_water),
            ((combined - 1) as u64, combined as u64, combined as u64)
        );
        assert_eq!(together.axes().count(), 1);
        let (_, record) = assemble(Some(total - 1));
        let axis = *record
            .axis("generated retained bytes")
            .expect("the generated byte target records its axis");
        assert_eq!(
            (axis.target, axis.first_requested, axis.high_water),
            ((total - 1) as u64, total as u64, total as u64)
        );
        std::fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    #[cfg(unix)]
    fn early_hot_requests_execute_in_the_emitted_live_protocol() {
        let source = std::str::from_utf8(LIFECYCLE_PROFILE_RUNNER_MAIN).unwrap();
        let framing = &source[..source.find("fn lifecycle()").unwrap()];
        let live = &source
            [source.find("struct PendingLive").unwrap()..source.find("fn run_protocol()").unwrap()];
        let runtime = std::str::from_utf8(RUNTIME_SOURCE).unwrap();
        let reporting = &runtime[runtime.find("fn report_accounts(").unwrap()
            ..runtime.find("fn snapshot_bytes").unwrap()];
        let root =
            std::env::temp_dir().join(format!("rawr-early-hot-protocol-{}", std::process::id()));
        std::fs::create_dir(&root).expect("exclusive protocol test directory");
        let path = root.join("main.rs");
        let executable = root.join("protocol");
        std::fs::write(
            &path,
            format!(
                "{framing}\n{reporting}\n{live}\n{}",
                include_str!("generate/live_protocol_fixture.rs")
            ),
        )
        .unwrap();
        let compiler = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
        let output = std::process::Command::new(compiler)
            .args(["--edition=2024", "-Awarnings"])
            .arg(&path)
            .arg("-o")
            .arg(&executable)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let output = std::process::Command::new(&executable).output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout)
            .contains("44 emitted live-protocol scenarios passed"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn generated_runners_stage_hot_work_until_explicit_commit_or_abort() {
        for (runner, prepare_marker) in [
            (RUNNER_MAIN, "9 if admitted && pending.is_none()"),
            (PROFILE_RUNNER_MAIN, "9 if admitted && pending.is_none()"),
            (LIFECYCLE_PROFILE_RUNNER_MAIN, "9 if pending.is_none()"),
        ] {
            let source = std::str::from_utf8(runner).expect("runner source is UTF-8");
            assert!(source.contains("&11_u32.to_be_bytes()"));
            assert!(source.contains(prepare_marker));
            assert!(source.contains("25 if payload.len() == 48"));
            assert!(source.contains("26 if payload.len() == 48"));
            assert!(source.contains("send_frame(&mut stream, 27, &payload)?"));
            let prepare = source.find(prepare_marker).unwrap();
            let commit = source.find("25 if payload.len() == 48").unwrap();
            assert!(prepare < commit);
        }

        let plain = std::str::from_utf8(RUNNER_MAIN).unwrap();
        let plain_commit = plain.find("25 if payload.len() == 48").unwrap();
        assert!(plain[..plain_commit].rfind("install_arena(next)").is_none());
        assert!(plain[plain_commit..].contains("install_arena(next)"));

        let profile = std::str::from_utf8(PROFILE_RUNNER_MAIN).unwrap();
        let profile_commit = profile.find("25 if payload.len() == 48").unwrap();
        assert!(profile[..profile_commit]
            .rfind("install_arena(next)")
            .is_none());
        assert!(profile[profile_commit..].contains("install_arena(next)"));

        let lifecycle = std::str::from_utf8(LIFECYCLE_PROFILE_RUNNER_MAIN).unwrap();
        let lifecycle_commit = lifecycle.find("25 if payload.len() == 48").unwrap();
        assert!(!lifecycle[..lifecycle_commit]
            .contains("transport.install(candidate.installation, candidate.candidate)"));
        assert!(lifecycle[lifecycle_commit..]
            .contains("transport.install(candidate.installation, candidate.candidate)"));
    }

    #[test]
    fn generated_runtime_binds_each_imported_identity_cohort_once_and_proves_totals() {
        let source = std::str::from_utf8(RUNTIME_SOURCE).expect("runtime source is UTF-8");
        assert!(source.contains("collect::<std::collections::BTreeSet<_>>()"));
        assert!(source.contains("for selector in imported_identities"));
        assert!(source.contains(
            "let proof = builder.bind_imported_type_attestation(selector, native_attestation)?"
        ));
        assert!(source.contains("installed_imported_identities != expected_imported_identities"));
        assert!(source.contains("installed_imported_rows != imported_rows"));
        assert!(source.contains("generated imported type attestation totality"));
        assert!(source.contains("builder.bind_type_attestation(path, native_attestation)?"));
        assert!(
            source.contains("ty.identity().imported().is_none() && !ty.parameters().is_empty()")
        );
        assert!(!source.contains("if let Some(identity) = ty.identity().imported()"));
        assert!(!source.contains("gallery_ui"));
    }

    #[test]
    fn projected_event_message_applies_the_exact_nominal_argument_substitution() {
        let signature = Signature::new(
            Box::from([Parameter::new(0, "M".into(), Kind::Type)]),
            Box::default(),
            Type::Nominal {
                definition: crate::surface::identity::Definition::authenticated([7; 32]),
                arguments: Box::from([Type::Parameter(0)]),
            },
        );
        let substitutions = Box::from([Type::ProfileMessage {
            mapper: [9; 16],
            payload: Box::new(Type::Builtin(Builtin::I64)),
        }]);
        let Type::Nominal { arguments, .. } = signature.output() else {
            panic!("fixture output is nominal");
        };
        let selected = projected_event_message(arguments, &signature, &substitutions, &[9; 16])
            .expect("the exact mapper-bearing substitution is selected");
        assert_eq!(selected, &substitutions[0]);
        assert!(projected_event_message(arguments, &signature, &substitutions, &[8; 16]).is_err());
    }

    #[test]
    fn places_container_and_item_substitutions_at_their_exact_rust_owners() {
        let signature = Signature::new(
            Box::from([
                Parameter::new(0, "Message".into(), Kind::Type),
                Parameter::new(1, "Output".into(), Kind::Type),
                Parameter::new(2, "COUNT".into(), Kind::Constant),
            ]),
            Box::default(),
            Type::Unit,
        );
        let substitutions = Box::from([
            Type::ProfileMessage {
                mapper: [9; 16],
                payload: Box::new(Type::Builtin(Builtin::I64)),
            },
            Type::Builtin(Builtin::Bool),
        ]);
        let constants = Box::from([Constant::literal("usize".into(), "7".into())]);

        let mut expression = Vec::new();
        render_container_path(
            "gallery_ui::Dial",
            &signature,
            &substitutions,
            &constants,
            1,
            false,
            &mut expression,
        )
        .expect("implementation-owned argument renders on the expression path");
        assert_eq!(expression, b"gallery_ui::Dial::<i64>");

        let mut ty = Vec::new();
        render_container_path(
            "gallery_ui::Dial",
            &signature,
            &substitutions,
            &constants,
            1,
            true,
            &mut ty,
        )
        .expect("implementation-owned argument renders on the receiver type");
        assert_eq!(ty, b"gallery_ui::Dial<i64>");

        let mut item = Vec::new();
        render_item_turbofish(&signature, &substitutions, &constants, 1, &mut item)
            .expect("item-owned arguments render only on the item");
        assert_eq!(item, b"::<bool, 7>");

        assert!(
            render_item_turbofish(&signature, &substitutions, &constants, 4, &mut Vec::new(),)
                .is_err()
        );
    }
}
