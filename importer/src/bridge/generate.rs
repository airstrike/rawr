//! Normalized standalone runner source generation.

use std::io::{self, Write};

use sha2::{Digest as _, Sha256};
use toml_edit::{value, DocumentMut, Item};

use super::request;
use crate::classify::representability::convert;
use crate::classify::representability::ownership::Receiver;
use crate::surface::adapter::{EntryIdentity, Operation, Plan};

pub(crate) mod cached;
mod family;
mod frame;
mod place;

const MANIFEST_PATH: &str = "Cargo.toml";
const LOCK_PATH: &str = "Cargo.lock";
const BUILD_PATH: &str = "build.rs";
const SOURCE_PATH: &str = "src/main.rs";
/// The crate name of a runner binary no script type names.
const RUNNER_BINARY: &str = "rawr_generated_runner";
/// `core::ops`' operator traits, which a crate type's operator is an impl
/// of: an adapter calls the impl's method, as `receiver.sub(argument)`,
/// and `receiver.index(argument)` of `grid[i]` (#709).
const OPERATOR_TRAITS: [&str; 24] = [
    "Index",
    "IndexMut",
    "Add",
    "Sub",
    "Mul",
    "Div",
    "Rem",
    "Neg",
    "Not",
    "AddAssign",
    "SubAssign",
    "MulAssign",
    "DivAssign",
    "RemAssign",
    "BitAnd",
    "BitOr",
    "BitXor",
    "Shl",
    "Shr",
    "BitAndAssign",
    "BitOrAssign",
    "BitXorAssign",
    "ShlAssign",
    "ShrAssign",
];
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
struct Adapter { identity: [u8; 32], signature: [u8; 32], selector: [u8; 32], instance: [u8; 32], item: &'static str, bound: bool, application: bool, root: &'static str, witness: u8, arguments: u32, invoke: Invoke }
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
    /// Homes a call borrowed (`PointerLent`), back to their slots the next
    /// time the owner thread has the arena.
    static REHOMES: RefCell<Vec<(NativeHandle, Box<dyn Homed>)>> = const { RefCell::new(Vec::new()) };
    /// Values a call borrowed in a map (`MapLent`), back to their slots the
    /// next time the owner thread has the arena.
    static RELENDS: RefCell<Vec<(NativeHandle, Box<dyn Any>)>> = const { RefCell::new(Vec::new()) };
    /// The homes calls borrow now (`PointerLent`), by their slots.
    static LENT_HOMES: RefCell<Vec<(([u8; 32], u32, u64), std::rc::Rc<LentHome>)>> = const { RefCell::new(Vec::new()) };
}

/// Slots whose last handle was dropped, freed on the owner thread the next
/// time it has the arena.
static RELEASES: std::sync::Mutex<Vec<([u8; 32], u32, u64)>> = std::sync::Mutex::new(Vec::new());

/// How many entries [`RESTORES`] and [`RELEASES`] hold together, changed
/// only under the lock of the queue that changes ([`enqueue`],
/// [`dequeue`]). An arena that finds none, and nothing in this thread's own
/// [`QUEUED_HERE`], releases nothing and looks at no queue. It is a count,
/// not a mark the first arena to look clears: the entries another arena
/// waits for keep it above zero till that arena takes them, so a call
/// arena on one thread never misses its own releases because an arena on
/// another thread looked first.
static PENDING: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

thread_local! {
    /// Whether this thread's [`REHOMES`], [`RELENDS`] or [`UNLENDS`] may
    /// hold an entry.
    static QUEUED_HERE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Add `entry` to a queue every thread shares, and count it in [`PENDING`].
fn enqueue<T>(queue: &std::sync::Mutex<Vec<T>>, entry: T) {
    let mut entries = queue.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    entries.push(entry);
    PENDING.fetch_add(1, std::sync::atomic::Ordering::Release);
}

/// Take the entries `take` picks out of a queue every thread shares, and
/// uncount them in [`PENDING`].
fn dequeue<T>(queue: &std::sync::Mutex<Vec<T>>, mut take: impl FnMut(&T) -> bool) -> Vec<T> {
    let mut entries = queue.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let (taken, kept) = std::mem::take(&mut *entries)
        .into_iter()
        .partition::<Vec<_>, _>(|entry| take(entry));
    *entries = kept;
    PENDING.fetch_sub(taken.len(), std::sync::atomic::Ordering::AcqRel);
    taken
}

/// Mark [`QUEUED_HERE`] after an entry joins one of this thread's queues.
fn queued_here() {
    let _ = QUEUED_HERE.try_with(|queued| queued.set(true));
}

impl Drop for Release {
    fn drop(&mut self) {
        enqueue(&RELEASES, (self.epoch, self.slot, self.generation));
        // A handle may be dropped on any thread; the arena is the owner's.
        __rawr_internal::future::on_owner(|| {
            let _ = with_arena(|_| Ok(()));
        });
    }
}
struct NativeSlot { generation: u64, value: Option<Box<dyn Any>>, homed: Option<Box<dyn Homed>>, leased: bool, debug: Option<DebugText>, display: Option<DebugText>, clone: Option<CloneValue>, relations: Relations, witness: Option<__rawr_internal::host::runtime::Witness>, project: Option<Project>, mapper: Option<[u8; 16]>, default_of: Option<[u8; 32]>, opaque: Option<OpaqueTable>, send: Option<SendValue>, name: &'static str }

/// A native value an iced view borrows lives in the application's home
/// (D568): its slot reaches it there, and holds no value of its own.
#[allow(dead_code)]
trait Homed: Any {
    fn shared(&self, call: &mut dyn FnMut(&dyn Any)) -> Result<(), Error>;
    fn mutable(&self, call: &mut dyn FnMut(&mut dyn Any)) -> Result<(), Error>;
    fn as_any(&self) -> &dyn Any;
    /// The value, where the home holds it plainly.
    fn value(&self) -> Option<&dyn Any> {
        None
    }
    /// The value back for its slot, once nothing borrows it from the home.
    fn settle(self: Box<Self>) -> Result<Box<dyn Any>, Box<dyn Homed>>;
}

/// A native value a view's widget borrows past its call, as `qr_code(&self.
/// data)` and `pick_list(Some(&self.backend), ..)` do (D568): shared with
/// the pins of the script value the view was lent, which keep it for as
/// long as the view can borrow it. Its slot reaches it here, and takes it
/// back once those pins let it go.
#[allow(dead_code)]
struct Viewed<T>(std::rc::Rc<T>);

impl<T: 'static> Homed for Viewed<T> {
    fn shared(&self, call: &mut dyn FnMut(&dyn Any)) -> Result<(), Error> {
        call(&*self.0);
        Ok(())
    }

    /// A view still borrows it, as rustc's E0502 says of a `&mut` while a
    /// shared borrow lives.
    fn mutable(&self, _call: &mut dyn FnMut(&mut dyn Any)) -> Result<(), Error> {
        Err(Error::Home(
            "cannot borrow this value as mutable because a view still borrows it (E0502)".into(),
        ))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn value(&self) -> Option<&dyn Any> {
        Some(&*self.0)
    }

    fn settle(self: Box<Self>) -> Result<Box<dyn Any>, Box<dyn Homed>> {
        std::rc::Rc::try_unwrap(self.0)
            .map(|value| Box::new(value) as Box<dyn Any>)
            .map_err(|shared| Box::new(Viewed(shared)) as Box<dyn Homed>)
    }
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

/// The inputs of a script function or closure native code calls on a
/// thread with no arena for the native values it takes or makes, as iced's
/// executor calls `Message::Loaded` for `Task::map`. When they are `Send`,
/// the call is made on the owner thread where its value is first used
/// (`Proxy::deferred`); else it runs where it is called, by method
/// resolution on a concrete `T`.
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

/// The script value `proxy` holds, given back to script code by an adapter,
/// whose arena is borrowed. A value made where it is first used
/// (`Proxy::deferred`) is made here, on the owner thread, with the arena
/// lent to the script function that makes it, as `Task::map`'s value is
/// read by `Task::value`.
#[allow(dead_code)]
fn give_proxy(arena: &mut Arena, proxy: __rawr_internal::crossing::Proxy) -> Dynamic {
    lend(arena, || proxy.into_dynamic())
}

/// A native value out of a call's arena, with what its slot knew of its
/// type, on its way to the arena where the script value holding it is
/// first used (`script_proxy`).
struct Moved {
    slot: u32,
    generation: u64,
    value: Box<dyn Any + Send>,
    send: SendValue,
    debug: Option<DebugText>,
    display: Option<DebugText>,
    clone: Option<CloneValue>,
    relations: Relations,
    witness: Option<__rawr_internal::host::runtime::Witness>,
    project: Option<Project>,
    mapper: Option<[u8; 16]>,
    default_of: Option<[u8; 32]>,
    opaque: Option<OpaqueTable>,
    name: &'static str,
}

/// The proxy of the script value `value` that script code gives native
/// code. On a thread whose arena is a call's own (`CallArena`), as iced's
/// executor runs `keyboard::listen().filter_map(..)`'s closure, the native
/// values `value` holds there move with it, and are given slots where it
/// is first used (`Proxy::deferred`), on the owner thread, as a deferred
/// script value is made there. What holds the proxy, as the `Some` of a
/// `filter_map`, is known now.
#[allow(dead_code)]
fn script_proxy(mut value: Dynamic) -> __rawr_internal::crossing::Proxy {
    let moved = arena_access(|arena| arena.call.then(|| (arena.epoch, arena.move_out(&mut value))))
        .flatten()
        .filter(|(_, moved)| !moved.is_empty());
    let Some((epoch, moved)) = moved else {
        return __rawr_internal::crossing::Proxy::new(value);
    };
    let mut pending = Some((value, moved));
    __rawr_internal::crossing::Proxy::deferred(move || {
        let (value, released) = arena_access(|arena| {
            let (mut value, moved) = pending.take()?;
            let released = arena.move_in(&mut value, epoch, moved);
            Some((value, released))
        })
        .flatten()?;
        // The handles on the call's slots go once the arena is free.
        drop(released);
        Some(value)
    })
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

/// The `Display` text of a scope frame's value, which carries a lifetime,
/// for a type that implements it, by method resolution on a concrete `T`,
/// as `debug_probe` chooses, as jiff's `strftime` gives `Display<'f>`.
#[allow(dead_code)]
struct ShownProbe<'v, T>(&'v T);

#[allow(dead_code)]
trait ViaShown {
    fn shown_text(&self) -> Option<String>;
}

impl<T: std::fmt::Display> ViaShown for ShownProbe<'_, T> {
    fn shown_text(&self) -> Option<String> {
        Some(self.0.to_string())
    }
}

#[allow(dead_code)]
trait WithoutShown {
    fn shown_text(&self) -> Option<String>;
}

impl<T> WithoutShown for &ShownProbe<'_, T> {
    fn shown_text(&self) -> Option<String> {
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

/// A stored value of a type that implements `Send`, held as one, so it may
/// move to another thread's arena (`script_proxy`); the value itself back
/// when it is of another type.
type SendValue = fn(Box<dyn Any>) -> Result<Box<dyn Any + Send>, Box<dyn Any>>;

fn send_value<T: Send + 'static>(value: Box<dyn Any>) -> Result<Box<dyn Any + Send>, Box<dyn Any>> {
    value.downcast::<T>().map(|value| value as Box<dyn Any + Send>)
}

#[allow(dead_code)]
trait ViaSend {
    fn native_send(&self) -> Option<SendValue>;
}

impl<T: Send + 'static> ViaSend for DebugProbe<T> {
    fn native_send(&self) -> Option<SendValue> {
        Some(send_value::<T>)
    }
}

#[allow(dead_code)]
trait WithoutSend {
    fn native_send(&self) -> Option<SendValue>;
}

impl<T> WithoutSend for &DebugProbe<T> {
    fn native_send(&self) -> Option<SendValue> {
        None
    }
}

#[allow(dead_code)]
trait ViaSequenceClone<T> {
    fn native_sequence_cloning(&self) -> Option<__rawr_internal::place::sequence::Cloning<T>>;
}

impl<T: Clone> ViaSequenceClone<T> for DebugProbe<T> {
    fn native_sequence_cloning(&self) -> Option<__rawr_internal::place::sequence::Cloning<T>> {
        Some(__rawr_internal::place::sequence::cloning::<T>())
    }
}

#[allow(dead_code)]
trait WithoutSequenceClone<T> {
    fn native_sequence_cloning(&self) -> Option<__rawr_internal::place::sequence::Cloning<T>>;
}

impl<T> WithoutSequenceClone<T> for &DebugProbe<T> {
    fn native_sequence_cloning(&self) -> Option<__rawr_internal::place::sequence::Cloning<T>> {
        None
    }
}

#[allow(dead_code)]
trait ViaSequenceCopy<T> {
    fn native_sequence_copying(&self) -> Option<__rawr_internal::place::sequence::Copying<T>>;
}

impl<T: Copy> ViaSequenceCopy<T> for DebugProbe<T> {
    fn native_sequence_copying(&self) -> Option<__rawr_internal::place::sequence::Copying<T>> {
        Some(__rawr_internal::place::sequence::copying::<T>())
    }
}

#[allow(dead_code)]
trait WithoutSequenceCopy<T> {
    fn native_sequence_copying(&self) -> Option<__rawr_internal::place::sequence::Copying<T>>;
}

impl<T> WithoutSequenceCopy<T> for &DebugProbe<T> {
    fn native_sequence_copying(&self) -> Option<__rawr_internal::place::sequence::Copying<T>> {
        None
    }
}

#[allow(dead_code)]
trait ViaSequenceEqual<T> {
    fn native_sequence_comparing(&self) -> Option<__rawr_internal::place::sequence::Comparing<T>>;
}

impl<T: PartialEq> ViaSequenceEqual<T> for DebugProbe<T> {
    fn native_sequence_comparing(&self) -> Option<__rawr_internal::place::sequence::Comparing<T>> {
        Some(__rawr_internal::place::sequence::comparing::<T>())
    }
}

#[allow(dead_code)]
trait WithoutSequenceEqual<T> {
    fn native_sequence_comparing(&self) -> Option<__rawr_internal::place::sequence::Comparing<T>>;
}

impl<T> WithoutSequenceEqual<T> for &DebugProbe<T> {
    fn native_sequence_comparing(&self) -> Option<__rawr_internal::place::sequence::Comparing<T>> {
        None
    }
}

#[allow(dead_code)]
trait ViaPlaceDebug<T> {
    fn native_place_debugging(&self) -> Option<__rawr_internal::place::reading::Debugging<T>>;
}

impl<T: std::fmt::Debug> ViaPlaceDebug<T> for DebugProbe<T> {
    fn native_place_debugging(&self) -> Option<__rawr_internal::place::reading::Debugging<T>> {
        Some(__rawr_internal::place::reading::debugging::<T>())
    }
}

#[allow(dead_code)]
trait WithoutPlaceDebug<T> {
    fn native_place_debugging(&self) -> Option<__rawr_internal::place::reading::Debugging<T>>;
}

impl<T> WithoutPlaceDebug<T> for &DebugProbe<T> {
    fn native_place_debugging(&self) -> Option<__rawr_internal::place::reading::Debugging<T>> {
        None
    }
}

#[allow(dead_code)]
trait ViaPlaceDisplay<T> {
    fn native_place_displaying(&self) -> Option<__rawr_internal::place::reading::Displaying<T>>;
}

impl<T: std::fmt::Display> ViaPlaceDisplay<T> for DebugProbe<T> {
    fn native_place_displaying(&self) -> Option<__rawr_internal::place::reading::Displaying<T>> {
        Some(__rawr_internal::place::reading::displaying::<T>())
    }
}

#[allow(dead_code)]
trait WithoutPlaceDisplay<T> {
    fn native_place_displaying(&self) -> Option<__rawr_internal::place::reading::Displaying<T>>;
}

impl<T> WithoutPlaceDisplay<T> for &DebugProbe<T> {
    fn native_place_displaying(&self) -> Option<__rawr_internal::place::reading::Displaying<T>> {
        None
    }
}

#[allow(dead_code)]
trait ViaPlacePartial<T> {
    fn native_place_partial(&self) -> Option<__rawr_internal::place::reading::Partial<T>>;
}

impl<T: PartialOrd> ViaPlacePartial<T> for DebugProbe<T> {
    fn native_place_partial(&self) -> Option<__rawr_internal::place::reading::Partial<T>> {
        Some(__rawr_internal::place::reading::partial::<T>())
    }
}

#[allow(dead_code)]
trait WithoutPlacePartial<T> {
    fn native_place_partial(&self) -> Option<__rawr_internal::place::reading::Partial<T>>;
}

impl<T> WithoutPlacePartial<T> for &DebugProbe<T> {
    fn native_place_partial(&self) -> Option<__rawr_internal::place::reading::Partial<T>> {
        None
    }
}

#[allow(dead_code)]
trait ViaPlaceTotal<T> {
    fn native_place_total(&self) -> Option<__rawr_internal::place::reading::Total<T>>;
}

impl<T: Ord> ViaPlaceTotal<T> for DebugProbe<T> {
    fn native_place_total(&self) -> Option<__rawr_internal::place::reading::Total<T>> {
        Some(__rawr_internal::place::reading::total::<T>())
    }
}

#[allow(dead_code)]
trait WithoutPlaceTotal<T> {
    fn native_place_total(&self) -> Option<__rawr_internal::place::reading::Total<T>>;
}

impl<T> WithoutPlaceTotal<T> for &DebugProbe<T> {
    fn native_place_total(&self) -> Option<__rawr_internal::place::reading::Total<T>> {
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
    use std::hash::Hasher as _;
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

/// The order of two stored values of one type, for a type that
/// implements `PartialOrd`.
type OrderValues = fn(&dyn Any, &dyn Any) -> Option<std::cmp::Ordering>;

fn order_values<T: PartialOrd + 'static>(lhs: &dyn Any, rhs: &dyn Any) -> Option<std::cmp::Ordering> {
    lhs.downcast_ref::<T>()?.partial_cmp(rhs.downcast_ref::<T>()?)
}

#[allow(dead_code)]
trait ViaOrder {
    fn native_order(&self) -> Option<OrderValues>;
}

impl<T: PartialOrd + 'static> ViaOrder for DebugProbe<T> {
    fn native_order(&self) -> Option<OrderValues> {
        Some(order_values::<T>)
    }
}

#[allow(dead_code)]
trait WithoutOrder {
    fn native_order(&self) -> Option<OrderValues>;
}

impl<T> WithoutOrder for &DebugProbe<T> {
    fn native_order(&self) -> Option<OrderValues> {
        None
    }
}

/// A stored value's `PartialEq`, `Hash` and `PartialOrd`, when its type
/// has them.
#[derive(Clone, Copy, Default)]
struct Relations { equal: Option<EqualValues>, hash: Option<HashValue>, order: Option<OrderValues> }

/// Store `value` in the arena, remembering its `Debug` text and its
/// `Clone` when its type has them.
#[allow(unused_macros)]
macro_rules! insert_native {
    ($arena:expr, $value:expr) => {{
        #[allow(unused_imports)]
        use $crate::__rawr_runner::{
            ViaClone as _, ViaDebug as _, ViaDisplay as _, ViaEqual as _, ViaHash as _,
            ViaOrder as _, ViaSend as _, WithoutClone as _, WithoutDebug as _,
            WithoutDisplay as _, WithoutEqual as _, WithoutHash as _, WithoutOrder as _,
            WithoutSend as _,
        };
        let value = $value;
        let debug = (&$crate::__rawr_runner::debug_probe(&value)).native_debug();
        let display = (&$crate::__rawr_runner::debug_probe(&value)).native_display();
        let clone = (&$crate::__rawr_runner::debug_probe(&value)).native_clone();
        let send = (&$crate::__rawr_runner::debug_probe(&value)).native_send();
        let relations = $crate::__rawr_runner::Relations {
            equal: (&$crate::__rawr_runner::debug_probe(&value)).native_equal(),
            hash: (&$crate::__rawr_runner::debug_probe(&value)).native_hash(),
            order: (&$crate::__rawr_runner::debug_probe(&value)).native_order(),
        };
        let handle = $arena.insert_debug(value, debug, clone);
        if let Ok(handle) = &handle {
            $arena.set_display(handle.clone(), display);
            $arena.set_relations(handle.clone(), relations);
            $arena.set_send(handle.clone(), send);
        }
        handle
    }};
}

/// How script code reads a `$target` native code lent it through the
/// family `$family` (`scope::Reads`): its `Display` and `Debug` text and
/// its `PartialEq`, chosen as `insert_native` chooses a stored value's. The
/// type carries no lifetime, so the value is reached as `dyn Any`.
#[allow(unused_macros)]
macro_rules! lent_reads {
    ($family:ty, $target:ty) => {{
        fn reach(
            nonce: __rawr_internal::scope::Nonce,
            visit: &mut dyn FnMut(&dyn std::any::Any),
        ) -> Result<(), __rawr_internal::scope::Error> {
            __rawr_internal::scope::with_ref::<$family, _>(nonce, |value| visit(value))
        }
        fn display(value: &dyn std::any::Any) -> Option<String> {
            #[allow(unused_imports)]
            use $crate::__rawr_runner::{ViaDisplay as _, WithoutDisplay as _};
            let target = value.downcast_ref::<$target>()?;
            (&$crate::__rawr_runner::debug_probe(target)).native_display()?(value)
        }
        fn debug(value: &dyn std::any::Any) -> Option<String> {
            #[allow(unused_imports)]
            use $crate::__rawr_runner::{ViaDebug as _, WithoutDebug as _};
            let target = value.downcast_ref::<$target>()?;
            (&$crate::__rawr_runner::debug_probe(target)).native_debug()?(value)
        }
        fn equal(value: &dyn std::any::Any, other: &dyn std::any::Any) -> Option<bool> {
            #[allow(unused_imports)]
            use $crate::__rawr_runner::{ViaEqual as _, WithoutEqual as _};
            let target = value.downcast_ref::<$target>()?;
            (&$crate::__rawr_runner::debug_probe(target)).native_equal()?(value, other)
        }
        __rawr_internal::scope::Reads::new(reach, display, debug, equal)
    }};
}

/// A copy of a borrowed std value Rawr's std holds (`crossing::Std`): its
/// owned form, as `&Path` gives a `PathBuf`.
#[allow(unused_macros)]
macro_rules! std_copy {
    ($target:expr) => {{
        Ok::<_, Error>(::std::borrow::ToOwned::to_owned($target))
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
        use $crate::__rawr_runner::{ViaClone as _, WithoutClone as _};
        let target = $target;
        $crate::__rawr_runner::copied(target, (&$crate::__rawr_runner::debug_probe(target)).native_clone())
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

/// A map a call borrows whose values are crate values (#777): each value
/// moved out of its slot into the map for the call, and back after, so
/// none is copied. Its keys are rawr's strings or numbers, made for it.
#[allow(dead_code)]
struct MapLent<M: LentMap> { map: M, keys: Vec<(M::Key, NativeHandle)> }

/// std's `HashMap` and `BTreeMap`, as a [`MapLent`] gives their values
/// back.
#[allow(dead_code)]
trait LentMap: 'static {
    type Key: 'static;
    type Value: 'static;
    fn pull(&mut self, key: &Self::Key) -> Option<Self::Value>;
}

impl<K: std::hash::Hash + Eq + 'static, V: 'static> LentMap for std::collections::HashMap<K, V> {
    type Key = K;
    type Value = V;
    fn pull(&mut self, key: &K) -> Option<V> {
        self.remove(key)
    }
}

impl<K: Ord + 'static, V: 'static> LentMap for std::collections::BTreeMap<K, V> {
    type Key = K;
    type Value = V;
    fn pull(&mut self, key: &K) -> Option<V> {
        self.remove(key)
    }
}

impl<M: LentMap> Drop for MapLent<M> {
    fn drop(&mut self) {
        let keys = std::mem::take(&mut self.keys);
        // The call that borrowed it may still hold the arena: the values
        // wait for its next use.
        RELENDS.with_borrow_mut(|relends| {
            for (key, handle) in keys {
                if let Some(value) = self.map.pull(&key) {
                    relends.push((handle, Box::new(value) as Box<dyn Any>));
                }
            }
            queued_here();
        });
        let _ = with_arena(|_| Ok(()));
    }
}

/// A slice of crate values a call borrows, as `view(&items)` takes it
/// (#846): each value moved out of its slot into one `Vec` once, and not
/// copied. Each slot reaches its value there (`SliceHome`), so the values
/// are read while the slice is lent, and none is changed or moved. The
/// last of the slices that share the values gives them back to their slots
/// when it drops (`Unlend`).
#[allow(dead_code)]
struct SliceLent<T: 'static> { items: std::rc::Rc<Vec<T>>, handles: std::rc::Rc<[NativeHandle]> }

impl<T: 'static> std::ops::Deref for SliceLent<T> {
    type Target = [T];

    fn deref(&self) -> &[T] {
        &self.items
    }
}

impl<T: 'static> Drop for SliceLent<T> {
    fn drop(&mut self) {
        // Each slot's home shares the values: with none besides them, this
        // is the last slice, and the values go back.
        if std::rc::Rc::strong_count(&self.items) != self.handles.len() + 1 {
            return;
        }
        let unlend = SliceUnlend { items: self.items.clone(), handles: self.handles.clone() };
        UNLENDS.with_borrow_mut(|unlends| unlends.push(Box::new(unlend)));
        queued_here();
        let _ = with_arena(|_| Ok(()));
    }
}

/// The home of a slot whose value a lent slice holds (`SliceLent`): the
/// value is read there, and refuses to be changed while it is lent.
#[allow(dead_code)]
struct SliceHome<T> { items: std::rc::Rc<Vec<T>>, index: usize }

impl<T: 'static> Homed for SliceHome<T> {
    fn shared(&self, call: &mut dyn FnMut(&dyn Any)) -> Result<(), Error> {
        call(self.items.get(self.index).ok_or(Error::StaleHandle)?);
        Ok(())
    }

    /// A result still borrows it, as rustc's E0502 says of a `&mut` while
    /// a shared borrow lives.
    fn mutable(&self, _call: &mut dyn FnMut(&mut dyn Any)) -> Result<(), Error> {
        Err(Error::Home(
            "cannot borrow this value as mutable because a result still borrows it (E0502)".into(),
        ))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn value(&self) -> Option<&dyn Any> {
        self.items.get(self.index).map(|item| item as &dyn Any)
    }

    /// The values go back together, once nothing borrows the slice
    /// (`Unlend`).
    fn settle(self: Box<Self>) -> Result<Box<dyn Any>, Box<dyn Homed>> {
        Err(self)
    }
}

/// Values a lent slice gave back, moved into their slots the next time the
/// owner thread has the arena.
trait Unlend {
    fn unlend(self: Box<Self>, arena: &mut Arena);
}

struct SliceUnlend<T: 'static> { items: std::rc::Rc<Vec<T>>, handles: std::rc::Rc<[NativeHandle]> }

impl<T: 'static> Unlend for SliceUnlend<T> {
    fn unlend(self: Box<Self>, arena: &mut Arena) {
        let Self { items, handles } = *self;
        // The homes let the values go first; a value whose slot went
        // meanwhile drops with the slice.
        for handle in handles.iter() {
            arena.unhome_slice::<T>(handle);
        }
        let Ok(items) = std::rc::Rc::try_unwrap(items) else {
            return;
        };
        for (handle, item) in handles.iter().zip(items) {
            arena.restore(handle.clone(), Box::new(item));
        }
    }
}

thread_local! {
    /// Slices whose last borrow ended (`SliceLent`), their values back to
    /// their slots the next time the owner thread has the arena.
    static UNLENDS: RefCell<Vec<Box<dyn Unlend>>> = const { RefCell::new(Vec::new()) };
}

/// The values of `handles`, moved out of their slots into one slice, or
/// the slice they are in already, as a second `view(&items)` shares the
/// first's.
#[allow(dead_code)]
fn lend_slice<T: 'static>(arena: &mut Arena, handles: Vec<NativeHandle>) -> Result<SliceLent<T>, Error> {
    if let Some(first) = handles.first()
        && let Some(home) = arena.homed(first)
    {
        let items = home
            .as_any()
            .downcast_ref::<SliceHome<T>>()
            .map(|home| home.items.clone())
            .ok_or(Error::Leased)?;
        let same = items.len() == handles.len()
            && handles.iter().enumerate().all(|(index, handle)| {
                arena
                    .homed(handle)
                    .and_then(|home| home.as_any().downcast_ref::<SliceHome<T>>())
                    .is_some_and(|home| home.index == index && std::rc::Rc::ptr_eq(&home.items, &items))
            });
        if !same {
            return Err(Error::Leased);
        }
        return Ok(SliceLent { items, handles: handles.into() });
    }
    // Every value is there before any moves.
    for handle in &handles {
        arena.settle(handle);
        if arena.homed(handle).is_some() {
            return Err(Error::Leased);
        }
        let index = arena.index::<T>(handle)?;
        if arena.slots[index].value.is_none() {
            return Err(Error::Leased);
        }
    }
    let mut items = Vec::new();
    items.try_reserve_exact(handles.len()).map_err(|_| Error::Allocation)?;
    for handle in &handles {
        let index = arena.index::<T>(handle)?;
        let value = arena.slots[index].value.take().ok_or(Error::StaleHandle)?;
        items.push(*value.downcast::<T>().map_err(|_| Error::TypeMismatch)?);
    }
    let items = std::rc::Rc::new(items);
    for (index, handle) in handles.iter().enumerate() {
        if let Some(slot) = arena.slots.get_mut(handle.slot as usize) {
            slot.homed = Some(Box::new(SliceHome { items: items.clone(), index }));
        }
    }
    Ok(SliceLent { items, handles: handles.into() })
}

/// A crate's own `Rc` or `Arc` (#775): the home of the slot of the value
/// it points to, which a rawr `Rc` or `Arc` holds the handle of. The
/// value is read through it, as `Deref` reads it, and never copied out.
#[allow(dead_code)]
#[derive(Clone)]
struct Pointed<P>(P);

/// Rust's `Rc` and `Arc`, as a [`Pointed`] holds them.
#[allow(dead_code)]
trait Pointer: Clone + std::ops::Deref<Target = <Self as Pointer>::Value> + 'static {
    type Value: 'static;
    fn new(value: Self::Value) -> Self;
    fn strong(&self) -> usize;
    fn address(&self) -> usize;
}

impl<T: 'static> Pointer for std::rc::Rc<T> {
    type Value = T;
    fn new(value: T) -> Self {
        Self::new(value)
    }
    fn strong(&self) -> usize {
        Self::strong_count(self)
    }
    fn address(&self) -> usize {
        Self::as_ptr(self).cast::<()>() as usize
    }
}

impl<T: 'static> Pointer for std::sync::Arc<T> {
    type Value = T;
    fn new(value: T) -> Self {
        Self::new(value)
    }
    fn strong(&self) -> usize {
        Self::strong_count(self)
    }
    fn address(&self) -> usize {
        Self::as_ptr(self).cast::<()>() as usize
    }
}

impl<P: Pointer> Homed for Pointed<P> {
    fn shared(&self, call: &mut dyn FnMut(&dyn Any)) -> Result<(), Error> {
        call(&*self.0);
        Ok(())
    }

    /// Rust's `Rc` and `Arc` lend only `&T`, as rustc's E0596 says.
    fn mutable(&self, _call: &mut dyn FnMut(&mut dyn Any)) -> Result<(), Error> {
        Err(Error::Home(
            "cannot borrow data in an `Rc` or `Arc` as mutable (E0596)".into(),
        ))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn value(&self) -> Option<&dyn Any> {
        Some(&*self.0)
    }

    /// The value stays in its pointer for as long as the slot lives.
    fn settle(self: Box<Self>) -> Result<Box<dyn Any>, Box<dyn Homed>> {
        Err(self)
    }
}

/// The strong count and address of the crate's own pointer `P` whose value
/// `value`, a rawr pointer's handle, is in: what a rawr `Rc` or `Arc` that
/// stands for it reads (`crossing::Link`).
#[allow(dead_code)]
fn pointer_count<P: Pointer>(value: &Dynamic) -> Option<(usize, usize)> {
    let handle = value.read_lock::<NativeHandle>()?.clone();
    arena_access(|arena| {
        let pointed = arena.homed(&handle)?.as_any().downcast_ref::<Pointed<P>>()?;
        Some((pointed.0.strong(), pointed.0.address()))
    })
    .flatten()
}

/// The crate's own pointer a rawr `Rc` or `Arc` gives native code by value
/// (#775): the one its value is in, made now if this is its first
/// crossing. rawr's last pointer moves it out of its slot, as Rust moves
/// its last pointer; another one shares it and stands for it after. All
/// of rawr's pointers share that one, so a copy for each of the others is
/// held while the call runs (`Balance`), where it counts them.
#[allow(dead_code)]
fn take_pointer<P: Pointer>(arena: &mut Arena, value: Dynamic) -> Result<(P, Balance<P>), Error> {
    let (pointee, strong, only) = __rawr_internal::crossing::pointee(&value).ok_or(Error::TypeMismatch)?;
    let handle = pointee.try_cast_result::<NativeHandle>().map_err(|_| Error::TypeMismatch)?;
    let pointer = arena.point::<P>(handle.clone())?;
    if only {
        drop(arena.unhome(&handle)?);
        return Ok((pointer, Balance(Vec::new())));
    }
    __rawr_internal::crossing::link(&value, pointer_count::<P>);
    // `value` moves into the call, and the one rawr holds stands for one
    // of those left.
    let balance = Balance::of(&pointer, strong.saturating_sub(2));
    Ok((pointer, balance))
}

/// Copies of a crate's own pointer, one for each rawr pointer but one that
/// shares it, held while a call runs so that native code counts them.
#[allow(dead_code)]
struct Balance<P>(Vec<P>);

#[allow(dead_code)]
impl<P: Pointer> Balance<P> {
    fn of(pointer: &P, count: usize) -> Self {
        Self(std::iter::repeat_with(|| pointer.clone()).take(count).collect())
    }
}

/// A crate's own pointer out of its slot while calls borrow it, with the
/// copies that stand for rawr's other pointers to it (`Balance`). Every
/// borrow of it in one call shares it, as `same(&a, &b)` of two of rawr's
/// pointers to one allocation borrows two.
#[allow(dead_code)]
struct LentHome { homed: Box<dyn Homed>, balance: Box<dyn Any> }

/// A borrow of a crate's own pointer for one call (#775): its home, out of
/// its slot until the last borrow of it ends, so the borrow adds no count.
#[allow(dead_code)]
struct PointerLent<P: Pointer> { lent: Option<std::rc::Rc<LentHome>>, handle: NativeHandle, pointer: std::marker::PhantomData<P> }

#[allow(dead_code)]
impl<P: Pointer> PointerLent<P> {
    fn get(&self) -> Option<&P> {
        self.lent.as_ref()?.homed.as_any().downcast_ref::<Pointed<P>>().map(|pointed| &pointed.0)
    }
}

impl<P: Pointer> Drop for PointerLent<P> {
    fn drop(&mut self) {
        let Some(lent) = self.lent.take() else {
            return;
        };
        let key = (self.handle.epoch, self.handle.slot, self.handle.generation);
        let last = LENT_HOMES.with_borrow_mut(|homes| {
            let at = homes.iter().position(|(held, _)| *held == key)?;
            (std::rc::Rc::strong_count(&lent) == 2).then(|| homes.remove(at))
        });
        drop(last);
        if let Ok(lent) = std::rc::Rc::try_unwrap(lent) {
            // The call that borrowed it may still hold the arena: the home
            // waits for its next use.
            REHOMES.with_borrow_mut(|rehomes| rehomes.push((self.handle.clone(), lent.homed)));
            queued_here();
            let _ = with_arena(|_| Ok(()));
        }
    }
}

/// Lend the crate's own pointer a rawr `Rc` or `Arc` stands for to a call
/// that takes `&Rc<T>` or `&Arc<T>`, made now if this is its first
/// crossing.
#[allow(dead_code)]
fn lend_pointer<P: Pointer>(arena: &mut Arena, value: &Dynamic) -> Result<PointerLent<P>, Error> {
    let (pointee, strong, _) = __rawr_internal::crossing::pointee(value).ok_or(Error::TypeMismatch)?;
    let handle = pointee.try_cast_result::<NativeHandle>().map_err(|_| Error::TypeMismatch)?;
    let key = (handle.epoch, handle.slot, handle.generation);
    let shared = LENT_HOMES.with_borrow(|homes| {
        homes.iter().find(|(held, _)| *held == key).map(|(_, lent)| lent.clone())
    });
    if let Some(lent) = shared {
        return Ok(PointerLent { lent: Some(lent), handle, pointer: std::marker::PhantomData });
    }
    let pointer = arena.point::<P>(handle.clone())?;
    __rawr_internal::crossing::link(value, pointer_count::<P>);
    let balance = Balance::of(&pointer, strong.saturating_sub(1));
    drop(pointer);
    let homed = arena.unhome(&handle)?;
    let lent = std::rc::Rc::new(LentHome { homed, balance: Box::new(balance) });
    LENT_HOMES.with_borrow_mut(|homes| homes.push((key, lent.clone())));
    Ok(PointerLent { lent: Some(lent), handle, pointer: std::marker::PhantomData })
}

/// A rawr `Rc` or `Arc` standing for the crate's own `pointer`, whose value
/// it reads in place: a slot `pointer` is the home of.
#[allow(dead_code)]
fn give_pointed<P: Pointer>(
    arena: &mut Arena,
    pointer: P,
    debug: Option<DebugText>,
    display: Option<DebugText>,
    clone: Option<CloneValue>,
    relations: Relations,
) -> Result<Dynamic, Error> {
    let handle = arena.occupy(None, Some(Box::new(Pointed(pointer))), std::any::type_name::<P::Value>())?;
    if let Some(entry) = arena.slots.get_mut(handle.slot as usize) {
        entry.debug = debug;
        entry.display = display;
        entry.clone = clone;
        entry.relations = relations;
    }
    Ok(__rawr_internal::crossing::counted(Dynamic::from(handle), pointer_count::<P>))
}

/// [`give_pointed`] with the `Debug`, `Display`, `Clone`, `PartialEq`,
/// `Hash` and `PartialOrd` of the value the pointer holds, when its type
/// has them.
#[allow(unused_macros)]
macro_rules! give_pointer {
    ($arena:expr, $pointer:expr) => {{
        #[allow(unused_imports)]
        use $crate::__rawr_runner::{
            ViaClone as _, ViaDebug as _, ViaDisplay as _, ViaEqual as _, ViaHash as _,
            ViaOrder as _, WithoutClone as _, WithoutDebug as _, WithoutDisplay as _,
            WithoutEqual as _, WithoutHash as _, WithoutOrder as _,
        };
        let pointer = $pointer;
        let debug = (&$crate::__rawr_runner::debug_probe(&*pointer)).native_debug();
        let display = (&$crate::__rawr_runner::debug_probe(&*pointer)).native_display();
        let clone = (&$crate::__rawr_runner::debug_probe(&*pointer)).native_clone();
        let relations = $crate::__rawr_runner::Relations {
            equal: (&$crate::__rawr_runner::debug_probe(&*pointer)).native_equal(),
            hash: (&$crate::__rawr_runner::debug_probe(&*pointer)).native_hash(),
            order: (&$crate::__rawr_runner::debug_probe(&*pointer)).native_order(),
        };
        $crate::__rawr_runner::give_pointed($arena, pointer, debug, display, clone, relations)
    }};
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
/// A thread's native values, by slot. `call` marks a call's own
/// (`CallArena`).
struct Arena { epoch: [u8; 32], slots: Vec<NativeSlot>, free: Vec<u32>, call: bool }

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
/// values, as `Result::ok` does, needs nothing of the owner's arena. A
/// value of a `Send` type that a script value it gives holds moves with
/// that value (`script_proxy`). Another value the call keeps past its end
/// can't reach the owner's arena, so the call fails there and says so.
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
                *arena = Some(Arena { call: true, ..Arena::new(epoch) });
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
        dequeue(&RELEASES, |(released, ..)| *released == epoch);
        if let Some(kept) = kept
            && !std::thread::panicking()
        {
            panic!(
                "rawr: a script function that native code called on another thread kept a native value{kept} past the call; a value can't move from that thread to the program's"
            );
        }
    }
}

/// `call` with this thread's arena: the one in its cell, or else the one a
/// native call running on this thread lends. `None` when neither is free.
/// A thread that ends has no arena: a handle its last values drop finds none.
fn arena_access<R>(call: impl FnOnce(&mut Arena) -> R) -> Option<R> {
    NATIVE_ARENA
        .try_with(|cell| match cell.try_borrow_mut() {
            Ok(mut arena) => Some(call(arena.as_mut()?)),
            Err(_) => __rawr_internal::island::lend::with(call),
        })
        .ok()
        .flatten()
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
    /// A borrow guard's target, or a variant's field a match binds, read
    /// by value where its type has no `Clone`, by the type's name.
    NotClone(&'static str),
    /// A value native code lent the call (D558.3), by its type's name,
    /// that can't be reached.
    Lent(&'static str, __rawr_internal::scope::Error),
    /// A value an iced view borrows, in the application's home, that
    /// can't be reached, as the home says (D568).
    Home(String),
    /// A value a field borrowed from it is reached through that can't be
    /// reached, as its reach says.
    Unreached(String),
}

impl std::fmt::Debug for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(bug) = self.bug() {
            return write!(f, "rawr bug: {bug}");
        }
        match self {
            Self::MissingArgument
            | Self::TypeMismatch
            | Self::StaleHandle
            | Self::Malformed
            | Self::Cardinality
            | Self::MissingAdapter => unreachable!("a rawr bug is written above"),
            Self::ArenaFull => f.write_str("more than 1000000 values of crate types are alive at once"),
            Self::Allocation => f.write_str("memory allocation failed"),
            Self::Stale => f.write_str("Stale"),
            Self::OutOfRange { value, ty } => write!(f, "{value} is out of range for `{ty}`"),
            Self::Leased => f.write_str(
                "cannot borrow this value: a future or a borrow guard that borrows it mutably is still running (E0499)",
            ),
            Self::Value(reason) => write!(f, "a value native code gave back is not its type: {reason}"),
            Self::NotClone(ty) => write!(
                f,
                "cannot move out of a value behind a shared reference: `{ty}` doesn't implement `Copy` or `Clone` (E0507)"
            ),
            Self::Lent(ty, error) => write!(f, "`{ty}`: {error}"),
            Self::Home(reason) | Self::Unreached(reason) => f.write_str(reason),
        }
    }
}

impl Error {
    /// What only a rawr bug causes: the program's types, argument counts
    /// and moves are checked before any adapter runs (D664.1).
    fn bug(&self) -> Option<&'static str> {
        Some(match self {
            Self::MissingArgument => "an adapter ran without one of its arguments",
            Self::TypeMismatch => "a value reached an adapter as a type its signature doesn't take",
            Self::StaleHandle => "a crate value's handle was used after its value was released",
            Self::Malformed => "an adapter invocation's payload is malformed",
            Self::Cardinality => "an adapter ran with a number of arguments it doesn't take",
            Self::MissingAdapter => "a call names an adapter the generated table doesn't have",
            _ => return None,
        })
    }

    /// The error as the program sees it. One that only a rawr bug causes
    /// panics, naming the bug.
    fn reported(self) -> Self {
        if let Some(bug) = self.bug() {
            panic!("rawr bug: {bug}");
        }
        self
    }

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
            Self::Unreached(_) => 16,
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
        Self { epoch, slots: Vec::new(), free: Vec::new(), call: false }
    }

    fn insert<T: 'static>(&mut self, value: T) -> Result<NativeHandle, Error> {
        self.occupy(Some(Box::new(value)), None, std::any::type_name::<T>())
    }

    /// A slot holding `value`, or reaching it in `homed`, named `name`.
    fn occupy(&mut self, value: Option<Box<dyn Any>>, homed: Option<Box<dyn Homed>>, name: &'static str) -> Result<NativeHandle, Error> {
        let slot = match self.free.pop() {
            Some(slot) => slot,
            None => {
                if self.slots.len() >= 1_000_000 { return Err(Error::ArenaFull); }
                self.slots.try_reserve_exact(1).map_err(|_| Error::Allocation)?;
                let slot = u32::try_from(self.slots.len()).map_err(|_| Error::ArenaFull)?;
                self.slots.push(NativeSlot { generation: 1, value: None, homed: None, leased: false, debug: None, display: None, clone: None, relations: Relations::default(), witness: None, project: None, mapper: None, default_of: None, opaque: None, send: None, name: "" });
                slot
            }
        };
        let entry = self.slots.get_mut(slot as usize).ok_or(Error::StaleHandle)?;
        if entry.value.is_some() || entry.homed.is_some() { return Err(Error::StaleHandle); }
        entry.value = value;
        entry.homed = homed;
        entry.leased = false;
        entry.debug = None;
        // A tool reading a value without `Debug` names its type (D675.3).
        entry.name = name;
        entry.display = None;
        entry.clone = None;
        entry.relations = Relations::default();
        entry.witness = None;
        entry.project = None;
        entry.mapper = None;
        entry.default_of = None;
        entry.opaque = None;
        entry.send = None;
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

    /// The type of the value at `handle` when it has no `Debug`.
    fn undebuggable(&self, handle: &NativeHandle) -> Option<&'static str> {
        let entry = self.slots.get(handle.slot as usize)?;
        (handle.epoch == self.epoch && handle.generation == entry.generation && entry.debug.is_none())
            .then_some(entry.name)
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

    #[allow(dead_code)]
    fn set_send(&mut self, handle: NativeHandle, send: Option<SendValue>) {
        if let Some(entry) = self.slots.get_mut(handle.slot as usize) {
            entry.send = send;
        }
    }

    /// The values `value`'s handles hold in this call's arena, out of their
    /// slots to move with it (`script_proxy`): each of a `Send` type, held
    /// by value. Another stays, as its thread can't give it away.
    fn move_out(&mut self, value: &mut Dynamic) -> Vec<Moved> {
        let mut moved = Vec::<Moved>::new();
        value.deep_scan(|held| {
            let Some((slot, generation)) = held
                .read_lock::<NativeHandle>()
                .filter(|handle| handle.epoch == self.epoch)
                .map(|handle| (handle.slot, handle.generation))
            else {
                return;
            };
            if moved.iter().any(|value| value.slot == slot) {
                return;
            }
            let Some(entry) = self.slots.get_mut(slot as usize) else {
                return;
            };
            let Some(send) = entry.send.filter(|_| entry.generation == generation && !entry.leased) else {
                return;
            };
            let Some(stored) = entry.value.take() else {
                return;
            };
            match send(stored) {
                Ok(stored) => moved.push(Moved {
                    slot,
                    generation,
                    value: stored,
                    send,
                    debug: entry.debug,
                    display: entry.display,
                    clone: entry.clone,
                    relations: entry.relations,
                    witness: entry.witness.clone(),
                    project: entry.project,
                    mapper: entry.mapper,
                    default_of: entry.default_of,
                    opaque: entry.opaque,
                    name: entry.name,
                }),
                Err(stored) => entry.value = Some(stored),
            }
        });
        moved
    }

    /// `moved`, which `value` held in the call arena `epoch`, in slots of
    /// this arena, and `value`'s handles on them. The handles it held
    /// before are given back, to drop once the arena is free.
    fn move_in(&mut self, value: &mut Dynamic, epoch: [u8; 32], moved: Vec<Moved>) -> Vec<NativeHandle> {
        let mut given = Vec::with_capacity(moved.len());
        for moved in moved {
            let Ok(handle) = self.occupy(Some(moved.value), None, moved.name) else {
                continue;
            };
            if let Some(entry) = self.slots.get_mut(handle.slot as usize) {
                entry.send = Some(moved.send);
                entry.debug = moved.debug;
                entry.display = moved.display;
                entry.clone = moved.clone;
                entry.relations = moved.relations;
                entry.witness = moved.witness;
                entry.project = moved.project;
                entry.mapper = moved.mapper;
                entry.default_of = moved.default_of;
                entry.opaque = moved.opaque;
            }
            given.push(((moved.slot, moved.generation), handle));
        }
        let mut released = Vec::new();
        value.deep_scan(|held| {
            let Some(mut handle) = held.write_lock::<NativeHandle>() else {
                return;
            };
            let key = (handle.slot, handle.generation);
            if handle.epoch != epoch {
                return;
            }
            if let Some((_, new)) = given.iter().find(|(old, _)| *old == key) {
                released.push(std::mem::replace(&mut *handle, new.clone()));
            }
        });
        released
    }

    /// The live value of `handle`, with its slot.
    fn live(&self, handle: NativeHandle) -> Option<(&NativeSlot, &dyn Any)> {
        let entry = self.slots.get(handle.slot as usize)?;
        if handle.epoch != self.epoch || handle.generation != entry.generation {
            return None;
        }
        let value = match entry.value.as_deref() {
            Some(value) => value,
            None => entry.homed.as_deref()?.value()?,
        };
        Some((entry, value))
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

    /// The value of `handle`, shared with the pins a view's widget borrows
    /// it through ([`Viewed`]). The value moves out of its slot once, and
    /// isn't copied.
    #[allow(dead_code)]
    fn share<T: 'static>(&mut self, handle: NativeHandle) -> Result<std::rc::Rc<T>, Error> {
        self.settle(&handle);
        if let Some(homed) = self.homed(&handle) {
            return homed
                .as_any()
                .downcast_ref::<Viewed<T>>()
                .map(|viewed| viewed.0.clone())
                .ok_or(Error::Leased);
        }
        let index = self.index::<T>(&handle)?;
        let value = self.slots[index].value.take().ok_or(Error::StaleHandle)?;
        let value = value.downcast::<T>().map_err(|_| Error::TypeMismatch)?;
        let shared = std::rc::Rc::new(*value);
        self.slots[index].homed = Some(Box::new(Viewed(shared.clone())));
        Ok(shared)
    }

    /// The crate's own `Rc` or `Arc` the value of `handle` is in (#775): its
    /// home, where the value moves the first time it crosses as one, and
    /// isn't copied.
    #[allow(dead_code)]
    fn point<P: Pointer>(&mut self, handle: NativeHandle) -> Result<P, Error> {
        self.home::<P::Value, Pointed<P>>(handle, |value| Ok(Pointed(P::new(value))))
            .map(|pointed| pointed.0)
    }

    /// The value of `handle`, out of its slot while a call borrows it in a
    /// map (`MapLent`); the slot refuses every use meanwhile.
    #[allow(dead_code)]
    fn lend_out<T: 'static>(&mut self, handle: NativeHandle) -> Result<T, Error> {
        self.settle(&handle);
        if self.homed(&handle).is_some() {
            return Err(Error::Leased);
        }
        let index = self.index::<T>(&handle)?;
        let value = self.slots[index].value.take().ok_or(Error::StaleHandle)?;
        self.slots[index].leased = true;
        value.downcast::<T>().map(|value| *value).map_err(|_| Error::TypeMismatch)
    }

    /// The home of `handle`'s slot, out of it while a call borrows it
    /// (`PointerLent`); the slot refuses every use meanwhile.
    #[allow(dead_code)]
    fn unhome(&mut self, handle: &NativeHandle) -> Result<Box<dyn Homed>, Error> {
        if handle.epoch != self.epoch { return Err(Error::StaleHandle); }
        let entry = self.slots.get_mut(handle.slot as usize).ok_or(Error::StaleHandle)?;
        if entry.generation != handle.generation { return Err(Error::StaleHandle); }
        let homed = entry.homed.take().ok_or(Error::Leased)?;
        entry.leased = true;
        Ok(homed)
    }

    /// A home back in the slot it was lent from, unless its last handle
    /// went meanwhile.
    fn rehome(&mut self, handle: NativeHandle, homed: Box<dyn Homed>) {
        if handle.epoch != self.epoch { return; }
        if let Some(entry) = self.slots.get_mut(handle.slot as usize)
            && entry.generation == handle.generation
            && entry.leased
            && entry.homed.is_none()
        {
            entry.homed = Some(homed);
            entry.leased = false;
        }
    }

    /// The slot of `handle` out of the home a lent slice gave it, waiting
    /// for its value (`SliceUnlend`), unless its last handle went meanwhile.
    fn unhome_slice<T: 'static>(&mut self, handle: &NativeHandle) {
        if handle.epoch != self.epoch { return; }
        if let Some(entry) = self.slots.get_mut(handle.slot as usize)
            && entry.generation == handle.generation
            && entry.homed.as_ref().is_some_and(|home| home.as_any().is::<SliceHome<T>>())
        {
            entry.homed = None;
            entry.leased = true;
        }
    }

    /// Give the slot of `handle` its value back from its home once nothing
    /// borrows it there.
    fn settle(&mut self, handle: &NativeHandle) {
        if handle.epoch != self.epoch {
            return;
        }
        let Some(entry) = self.slots.get_mut(handle.slot as usize) else {
            return;
        };
        if entry.generation != handle.generation {
            return;
        }
        let Some(homed) = entry.homed.take() else {
            return;
        };
        match homed.settle() {
            Ok(value) => entry.value = Some(value),
            Err(homed) => entry.homed = Some(homed),
        }
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
        // A type with `PartialOrd` keys a B-tree collection in its own
        // order; values it can't order key a hashed one in their hash's.
        if let Some(ordering) = left.relations.order.and_then(|order| order(lhs, rhs)) {
            return Some(ordering);
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
        self.index_where(handle, |value| value.is::<T>())
    }

    fn index_where(&self, handle: &NativeHandle, is: impl Fn(&dyn Any) -> bool) -> Result<usize, Error> {
        if handle.epoch != self.epoch { return Err(Error::StaleHandle); }
        let index = handle.slot as usize;
        let entry = self.slots.get(index).ok_or(Error::StaleHandle)?;
        if entry.generation == handle.generation && entry.leased {
            return Err(Error::Leased);
        }
        if entry.generation != handle.generation || entry.value.is_none() {
            return Err(Error::StaleHandle);
        }
        if !entry.value.as_deref().is_some_and(is) {
            return Err(Error::TypeMismatch);
        }
        Ok(index)
    }

    fn take<T: 'static>(&mut self, handle: NativeHandle) -> Result<T, Error> {
        self.take_where(&handle, |value| value.is::<T>())?
            .downcast::<T>()
            .map(|value| *value)
            .map_err(|_| Error::TypeMismatch)
    }

    /// The value `handle` names, of whatever type it is: a marked
    /// function's result native code takes (#1118).
    fn take_any(&mut self, handle: &NativeHandle) -> Result<Box<dyn Any>, Error> {
        self.take_where(handle, |_| true)
    }

    /// The value `handle` names when `is` holds for it: moved out by the
    /// last copy of the handle, and else copied out by its `Clone`.
    fn take_where(&mut self, handle: &NativeHandle, is: impl Fn(&dyn Any) -> bool) -> Result<Box<dyn Any>, Error> {
        self.settle(handle);
        // A homed value is copied out by its `Clone`, as a slot's is; one
        // without it stays where the view borrows it.
        if let Some(homed) = self.homed(handle) {
            let clone = self.slots[handle.slot as usize].clone.ok_or(Error::Leased)?;
            let mut copied = None;
            homed.shared(&mut |value| copied = clone(value))?;
            return copied.filter(|value| is(value.as_ref())).ok_or(Error::TypeMismatch);
        }
        let index = self.index_where(handle, is)?;
        // A script copies a handle where it copies the value, so while
        // another copy of the handle lives, a `Clone` value is cloned out
        // and its slot stays valid for that copy. The last copy moves the
        // value out, as Rust moves it (#698): a `vec![Family::new(..)]`
        // given to a crate call clones nothing.
        let shared = handle
            .release
            .as_ref()
            .is_none_or(|release| std::sync::Arc::strong_count(release) > 1);
        if shared && let Some(clone) = self.slots[index].clone {
            let value = self.slots[index].value.as_deref().ok_or(Error::StaleHandle)?;
            return clone(value).ok_or(Error::TypeMismatch);
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
        Ok(value)
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
        self.settle(&handle);
        if self.homed(&handle).is_some() {
            return Err(Error::Leased);
        }
        let index = self.index::<T>(&handle)?;
        let value = self.slots[index].value.take().ok_or(Error::StaleHandle)?;
        self.slots[index].leased = true;
        let value = value.downcast::<T>().map(|value| *value).map_err(|_| Error::TypeMismatch)?;
        Ok(Held::Leased { value: Some(value), handle })
    }

    /// Lend a native place synchronously on its owner thread, without
    /// imposing the transfer bound required by an asynchronous Held value.
    fn lease_place<T: 'static>(&mut self, handle: NativeHandle) -> Result<PlaceHeld<T>, Error> {
        self.settle(&handle);
        if self.homed(&handle).is_some() { return Err(Error::Leased); }
        let index = self.index::<T>(&handle)?;
        let value = self.slots[index].value.take().ok_or(Error::StaleHandle)?;
        self.slots[index].leased = true;
        let value = value.downcast::<T>().map(|value| *value).map_err(|_| Error::TypeMismatch)?;
        Ok(PlaceHeld { value: Some(value), handle })
    }

    /// Free each slot whose last handle was dropped. Dropping a native value
    /// may drop handles it holds, which are freed in the next round.
    fn release_dropped(&mut self) {
        // Nothing queued since the last look: nothing to release.
        let here = QUEUED_HERE.try_with(|queued| queued.replace(false)).unwrap_or(false);
        if PENDING.load(std::sync::atomic::Ordering::Acquire) == 0 && !here {
            return;
        }
        // Another arena's values wait for their own arena.
        let restores = dequeue(&RESTORES, |(handle, _)| handle.epoch == self.epoch);
        for (handle, value) in restores {
            self.restore(handle, value);
        }
        // A thread that ends has nothing left to give back.
        if here {
            for (handle, homed) in REHOMES.try_with(RefCell::take).unwrap_or_default() {
                self.rehome(handle, homed);
            }
            for (handle, value) in RELENDS.try_with(RefCell::take).unwrap_or_default() {
                self.restore(handle, value);
            }
            for unlend in UNLENDS.try_with(RefCell::take).unwrap_or_default() {
                unlend.unlend(self);
            }
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
            let batch = dequeue(&RELEASES, |(epoch, ..)| *epoch == self.epoch);
            if !call {
                dequeue(&RELEASES, |(epoch, ..)| !live(epoch));
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
    fn restore(&mut self, handle: NativeHandle, value: Box<dyn Any>) {
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
        // A value leased from its slot to lend shared, as a pattern through
        // `&T` lends its fields, is read where the lend holds it, as Rust
        // reads a value while it is borrowed shared.
        if handle.epoch == self.epoch
            && let Some(nonce) = LENDERS
                .with_borrow(|lenders| lenders.get(&(handle.epoch, handle.slot, handle.generation)).copied())
        {
            return __rawr_internal::scope::with_ref::<LenderF<T>, _>(nonce, |value| call(value))
                .map_err(|error| Error::Lent(std::any::type_name::<T>(), error));
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
        self.settle(&handle);
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
        self.settle(&handle);
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

    /// The value of `handle` kept in its slot's home with the guard `reach`
    /// makes of it (#1325), as a crate method's `&mut` borrows it: a guard
    /// kept before ends here, as its borrow did. A `reach` that gives no
    /// guard, as an `Option<&mut T>` method's `None` (#1334), leaves the
    /// value in its slot, and the end it gives holds nothing.
    #[allow(dead_code)]
    fn yoke<T: 'static, U: ?Sized + 'static>(
        &mut self,
        handle: &NativeHandle,
        reach: impl for<'a> FnOnce(&'a mut T) -> Option<Box<dyn std::ops::DerefMut<Target = U> + 'a>>,
    ) -> Result<YokeEnd<U>, Error> {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        self.settle(handle);
        let index = self.index::<T>(handle)?;
        let value = self.slots[index].value.take().ok_or(Error::StaleHandle)?;
        let value = value.downcast::<T>().map_err(|_| Error::TypeMismatch)?;
        let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut yoke = __rawr_internal::island::place::Yoke::new(value, reach);
        if yoke.target().is_some() {
            self.slots[index].homed = Some(Box::new(Yoked { id, yoke: RefCell::new(yoke) }));
        } else {
            self.slots[index].value = Some(yoke.into_owner());
        }
        Ok(YokeEnd { handle: handle.clone(), id, target: std::marker::PhantomData })
    }
}

/// The native value `value` holds, moved out of this thread's arena for
/// native code that called a marked function (#1118).
fn moved(value: &Dynamic) -> Option<Result<Box<dyn Any>, String>> {
    let handle = value.read_lock::<NativeHandle>()?;
    Some(
        arena_access(|arena| arena.take_any(&handle).map_err(|error| format!("{:?}", error.reported())))
            .unwrap_or_else(|| Err("a native value can't move from another thread's arena".to_owned())),
    )
}

/// The script value that holds a native value native code gave a marked
/// function by value (#1139), kept in this thread's arena with what its
/// trampoline found of its type, as a value the runner made is.
fn lodge(
    given: __rawr_internal::island::Given,
    witness: Option<&__rawr_internal::host::runtime::Witness>,
) -> Result<Dynamic, String> {
    let (value, name, (debug, display, clone, equal, hash, order)) = given;
    let kept = arena_access(|arena| {
        let handle = arena.occupy(Some(value), None, name)?;
        if let Some(entry) = arena.slots.get_mut(handle.slot as usize) {
            entry.debug = debug;
            entry.display = display;
            entry.clone = clone;
            entry.relations = Relations { equal, hash, order };
        }
        if let Some(witness) = witness {
            arena.attest(handle.clone(), witness.clone())?;
        }
        Ok::<_, Error>(handle)
    });
    match kept {
        Some(Ok(handle)) => Ok(Dynamic::from(handle)),
        Some(Err(error)) => Err(format!("{:?}", error.reported())),
        None => Err("a native value can't move to another thread's arena".to_owned()),
    }
}

fn install_arena(epoch: [u8; 32]) {
    __rawr_internal::island::moves(moved);
    __rawr_internal::island::lodges(lodge);
    __rawr_internal::host::debug::register::<NativeHandle>();
    __rawr_internal::host::display::register::<NativeHandle>();
    __rawr_internal::host::debug::register::<__rawr_internal::scope::Scoped>();
    __rawr_internal::host::display::register::<__rawr_internal::scope::Scoped>();
    __rawr_internal::host::debug::register::<__rawr_internal::scope::Projected>();
    __rawr_internal::host::display::register::<__rawr_internal::scope::Projected>();
    __rawr_internal::host::debug::register::<__rawr_internal::scope::Lent>();
    __rawr_internal::host::display::register::<__rawr_internal::scope::Lent>();
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

/// Native place ownership uses the existing arena's owner thread and release
/// authority, including non-Send values. Its synchronous lease is always
/// restored before the source operation returns or unwinds.
struct PlaceOwner<T>(std::marker::PhantomData<fn() -> T>);

/// A value lent from its slot for a synchronous use on the owner thread,
/// as a native place's access or a call's `&mut T` input is: it never
/// leaves the thread, so it needs no `Send`, as rand's `ThreadRng` isn't.
struct PlaceHeld<T: 'static> { value: Option<T>, handle: NativeHandle }

#[allow(dead_code)]
impl<T: 'static> PlaceHeld<T> {
    fn get_mut(&mut self) -> &mut T {
        self.value.as_mut().expect("a lent value is held till it drops")
    }
}

impl<T: 'static> Drop for PlaceHeld<T> {
    fn drop(&mut self) {
        if let Some(value) = self.value.take() {
            // The call that borrowed it may still hold the arena: the value
            // waits in this thread's queue for its next use.
            RELENDS.with_borrow_mut(|relends| {
                relends.push((self.handle.clone(), Box::new(value) as Box<dyn Any>));
            });
            queued_here();
            let _ = with_arena(|_| Ok(()));
        }
    }
}

thread_local! {
    /// Native vectors script code owns, on the thread that made them, by
    /// slot. Each access moves its value out for the access alone, so no
    /// borrow of the table outlives a statement.
    static PLACE_VALUES: RefCell<(u64, std::collections::BTreeMap<u64, Box<dyn Any>>)> =
        RefCell::new((0, std::collections::BTreeMap::new()));
}

/// Ownership of a native value in this thread's place table (D651). The
/// carrier is a slot number: another thread finds nothing under it.
struct PlaceLocal;
struct PlaceSlot(u64);

impl Drop for PlaceSlot {
    fn drop(&mut self) {
        let value = PLACE_VALUES
            .try_with(|values| values.try_borrow_mut().ok().and_then(|mut values| values.1.remove(&self.0)))
            .ok()
            .flatten();
        drop(value);
    }
}

/// A value out of its slot for one access, put back however the access ends.
struct PlaceLease(u64, Option<Box<dyn Any>>);

impl Drop for PlaceLease {
    fn drop(&mut self) {
        if let Some(value) = self.1.take() {
            PLACE_VALUES.with_borrow_mut(|values| values.1.insert(self.0, value));
        }
    }
}

fn place_slot_missing() -> Box<__rawr_internal::EvalAltResult> {
    __rawr_internal::EvalAltResult::ErrorRuntime("the owned native place is borrowed, moved or on another thread".into(), __rawr_internal::Position::NONE).into()
}

impl<T: 'static> __rawr_internal::place::ownership::Owner<T> for PlaceLocal {
    type Stored = PlaceSlot;
    type Vector = PlaceLocal;
    fn store(value: T) -> Result<PlaceSlot, Box<__rawr_internal::EvalAltResult>> {
        Ok(PLACE_VALUES.with_borrow_mut(|values| {
            values.0 += 1;
            let slot = values.0;
            values.1.insert(slot, Box::new(value));
            PlaceSlot(slot)
        }))
    }
    fn access<R>(
        stored: &PlaceSlot,
        mutable: bool,
        visit: impl FnOnce(__rawr_internal::place::Access<'_, T>) -> Result<R, Box<__rawr_internal::EvalAltResult>>,
    ) -> Result<R, Box<__rawr_internal::EvalAltResult>> {
        let value = PLACE_VALUES.with_borrow_mut(|values| values.1.remove(&stored.0)).ok_or_else(place_slot_missing)?;
        let mut lease = PlaceLease(stored.0, Some(value));
        let value = lease.1.as_mut().and_then(|value| value.downcast_mut::<T>()).ok_or_else(place_slot_missing)?;
        visit(if mutable { __rawr_internal::place::Access::Mutable(value) } else { __rawr_internal::place::Access::Shared(value) })
    }
    fn take(stored: &PlaceSlot) -> Result<T, Box<__rawr_internal::EvalAltResult>> {
        let value = PLACE_VALUES.with_borrow_mut(|values| values.1.remove(&stored.0)).ok_or_else(place_slot_missing)?;
        value.downcast::<T>().map(|value| *value).map_err(|_| place_slot_missing())
    }
    fn restore(stored: &PlaceSlot, value: T) -> Result<(), Box<__rawr_internal::EvalAltResult>> {
        PLACE_VALUES.with_borrow_mut(|values| values.1.insert(stored.0, Box::new(value)));
        Ok(())
    }
}

fn place_error(error: Error) -> Box<__rawr_internal::EvalAltResult> {
    runtime_error(error)
}

impl<T: 'static> __rawr_internal::place::ownership::Owner<T> for PlaceOwner<T> {
    type Stored = NativeHandle;
    // A native vector is owned where it is made, not in the arena, which
    // a callback's return may already hold.
    type Vector = PlaceLocal;
    fn store(value: T) -> Result<NativeHandle, Box<__rawr_internal::EvalAltResult>> {
        with_arena(|arena| arena.insert(value)).map_err(place_error)
    }
    fn access<R>(
        stored: &NativeHandle,
        mutable: bool,
        visit: impl FnOnce(__rawr_internal::place::Access<'_, T>) -> Result<R, Box<__rawr_internal::EvalAltResult>>,
    ) -> Result<R, Box<__rawr_internal::EvalAltResult>> {
        let mut held = with_arena(|arena| arena.lease_place::<T>(stored.clone())).map_err(place_error)?;
        let value = held.value.as_mut().expect("native place lease retains its value until restored");
        visit(if mutable { __rawr_internal::place::Access::Mutable(value) } else { __rawr_internal::place::Access::Shared(value) })
    }
    fn take(stored: &NativeHandle) -> Result<T, Box<__rawr_internal::EvalAltResult>> {
        with_arena(|arena| arena.take(stored.clone())).map_err(place_error)
    }
    fn restore(stored: &NativeHandle, value: T) -> Result<(), Box<__rawr_internal::EvalAltResult>> {
        with_arena(|arena| { arena.restore(stored.clone(), Box::new(value)); Ok(()) }).map_err(place_error)
    }
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

    fn get(&self) -> &T {
        match self {
            Self::Cloned(value) => value,
            Self::Leased { value, .. } => value.as_ref().expect("a lent value is held until dropped"),
        }
    }
}

/// A `&U` a crate's value lends, as a `&self` method's `&io::Error`
/// borrows its receiver (#680): the receiver, leased from its slot while
/// the guard lives, and the method that reaches the `U` in it.
#[allow(dead_code)]
struct Projected<T: Send + 'static, U: 'static>(Held<T>, fn(&T) -> &U);

impl<T: Send + 'static, U: 'static> std::ops::Deref for Projected<T, U> {
    type Target = U;

    fn deref(&self) -> &U {
        (self.1)(self.0.get())
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
            enqueue(&RESTORES, (handle.clone(), Box::new(value) as Box<dyn Any + Send>));
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

/// Gives a concrete `T`'s `Default` where rustc proves `T: Default`, and
/// none otherwise, by method resolution: an alias that names one instance
/// of a generic type, as ratatui's `DefaultTerminal` names
/// `Terminal<CrosstermBackend<Stdout>>`, has its target's `Default` only
/// where the impl's bounds hold for its arguments.
#[allow(dead_code)]
struct DefaultProbe<T>(std::marker::PhantomData<T>);

#[allow(dead_code)]
impl<T> DefaultProbe<T> {
    fn new() -> Self {
        Self(std::marker::PhantomData)
    }
}

#[allow(dead_code)]
trait DefaultGiven<T> {
    fn made(&self) -> Option<fn() -> T>;
}

impl<T: Default> DefaultGiven<T> for DefaultProbe<T> {
    fn made(&self) -> Option<fn() -> T> {
        Some(T::default)
    }
}

#[allow(dead_code)]
trait DefaultAbsent<T> {
    fn made(&self) -> Option<fn() -> T>;
}

impl<T> DefaultAbsent<T> for &DefaultProbe<T> {
    fn made(&self) -> Option<fn() -> T> {
        None
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
    .map_err(|error| format!("{:?}", error.reported()))
}

/// [`materialize`] of an output that holds script values an erased generic
/// gave back: each is checked against the script type it stands for.
#[allow(dead_code)]
fn materialize_checked(
    witness: Option<&__rawr_internal::host::runtime::Witness>,
    convert: impl FnOnce(&mut Arguments<'_, '_, '_>) -> Result<Dynamic, Error>,
) -> Result<Dynamic, String> {
    let output = materialize(witness, convert)?;
    check_values(witness, &output).map_err(|error| format!("{:?}", error.reported()))?;
    Ok(output)
}

fn take_primitive<T: 'static>(arguments: &mut Arguments<'_, '_, '_>, index: usize) -> Result<T, Error> {
    let value = arguments.values.get_mut(index).ok_or(Error::MissingArgument)?;
    (**value).take().try_cast_result::<T>().map_err(|_| Error::TypeMismatch)
}

#[allow(dead_code)]
fn take_std<T: __rawr_internal::crossing::Std>(arguments: &mut Arguments<'_, '_, '_>, index: usize) -> Result<T, Error> {
    let value = arguments.values.get_mut(index).ok_or(Error::MissingArgument)?;
    T::take((**value).take()).ok_or(Error::TypeMismatch)
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

/// A `Cow<'_, str>`'s text: a program's `Cow<str>`, or the string a
/// struct's `Cow<str>` field is given.
#[allow(dead_code)]
fn take_text(arguments: &mut Arguments<'_, '_, '_>, index: usize) -> Result<String, Error> {
    let value = arguments.values.get_mut(index).ok_or(Error::MissingArgument)?;
    **value = __rawr_internal::cow::text((**value).take());
    take_string(arguments, index)
}

#[allow(dead_code)]
fn take_string(arguments: &mut Arguments<'_, '_, '_>, index: usize) -> Result<String, Error> {
    let value = (**arguments.values.get_mut(index).ok_or(Error::MissingArgument)?).take();
    if let Some(place) = __rawr_internal::place::get(&value) {
        __rawr_internal::place::take::<__rawr_internal::place::text::Text>(&place, place.witness())
            .map_err(|error| Error::Value(error.to_string()))
    } else {
        value.try_cast_result::<ImmutableString>().map(|value| value.to_string()).map_err(|_| Error::TypeMismatch)
    }
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
    take_future(arguments, index).map(__rawr_internal::future::Future::into_script)
}

/// The core's future at argument `index`: a script future, or a native
/// value whose opaque type is a future, as a program's own `impl Straw<..>`
/// is, which its table makes one of where it was made.
#[allow(dead_code)]
fn take_future(arguments: &mut Arguments<'_, '_, '_>, index: usize) -> Result<__rawr_internal::future::Future, Error> {
    let mut value = (**arguments.values.get_mut(index).ok_or(Error::MissingArgument)?).take();
    let handle = value.read_lock::<NativeHandle>().map(|handle| handle.clone());
    if let Some(handle) = handle {
        let table = arguments.arena.opaque(&handle)?;
        let mut values = [&mut value];
        value = table.future(&mut Arguments { values: &mut values, arena: arguments.arena })?;
    }
    value
        .try_cast_result::<__rawr_internal::future::Future>()
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

/// The value among the arguments at `indices`, and in the arrays there,
/// as `column![..]`'s elements are, and the options, as `column![..,
/// maybe.map(|x| text(x))]`'s `Option<Text>` is, whose scope frame's
/// region ends first: a call that takes values of several frames runs in
/// it, as `with_children([a, b])` of two `let`s does (#1329).
#[allow(dead_code)]
fn scoped_innermost(arguments: &Arguments<'_, '_, '_>, indices: &[usize]) -> Option<__rawr_internal::scope::Scoped> {
    let scoped = |item: &Dynamic| item.read_lock::<__rawr_internal::scope::Scoped>().map(|scoped| *scoped);
    let mut handles = Vec::new();
    for value in indices.iter().filter_map(|index| arguments.values.get(*index)) {
        if let Some(handle) = scoped(value) {
            handles.push(handle);
        } else if let Some(option) = value.read_lock::<__rawr_internal::option::Value>() {
            handles.extend(option.as_ref().and_then(scoped));
        } else if let Some(items) = value.read_lock::<__rawr_internal::Array>() {
            handles.extend(items.iter().filter_map(scoped));
        }
    }
    __rawr_internal::scope::innermost(handles)
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
type Reach = fn(Dynamic, bool, &mut dyn FnMut(Reached<'_>)) -> Result<(), Error>;

#[allow(dead_code)]
fn reach_lent<T: 'static>(
    value: Dynamic,
    mutable: bool,
    visit: &mut dyn FnMut(Reached<'_>),
) -> Result<(), Error> {
    // A value its slot has leased to lend shared, as a pattern through
    // `&T` lends its fields, is shared where the lend holds it; borrowing
    // it mutably meanwhile is the conflict rustc reports.
    if let Some(nonce) = lender_nonce(&value) {
        if mutable {
            return Err(Error::Leased);
        }
        return __rawr_internal::scope::with_ref::<LenderF<T>, _>(nonce, |value| visit(Reached::Shared(value)))
            .map_err(|error| Error::Lent(std::any::type_name::<T>(), error));
    }
    let lent = value.try_cast_result::<__rawr_internal::scope::Lent>().map_err(|_| Error::TypeMismatch)?;
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

/// Reach a native sequence/text place through its exact descriptor.
#[allow(dead_code)]
fn reach_place<P: __rawr_internal::place::Policy>(
    value: Dynamic,
    mutable: bool,
    visit: &mut dyn FnMut(Reached<'_>),
) -> Result<(), Error> {
    let place = __rawr_internal::place::get(&value).ok_or(Error::TypeMismatch)?;
    __rawr_internal::place::reach::<P, _>(&place, place.witness(), mutable, |value| {
        match value {
            __rawr_internal::place::Access::Shared(value) => visit(Reached::Shared(value)),
            __rawr_internal::place::Access::Mutable(value) => visit(Reached::Mutable(value)),
        }
        Ok(())
    }).map_err(|error| Error::Value(error.to_string()))
}

/// The receiver a slice a `&self` method lends borrows (#687): a value in
/// an arena slot, or one native code lent (D558.3).
#[allow(dead_code)]
#[derive(Clone)]
enum Lender {
    Handle(NativeHandle),
    Lent(__rawr_internal::scope::Lent),
    /// A field a pattern borrowed from another value (#783).
    Projected(__rawr_internal::scope::Projected),
    /// An element of a native slice, as one of a `static BLOCKS:
    /// &[Block]` a pattern reads (#1291).
    Place(__rawr_internal::place::Place),
}

#[allow(dead_code)]
impl Lender {
    /// The lender `value` names: a value in an arena slot, one native code
    /// lent, or a field borrowed from another value.
    fn of(value: &Dynamic) -> Result<Self, Error> {
        if let Some(lent) = value.read_lock::<__rawr_internal::scope::Lent>() {
            return Ok(Self::Lent(*lent));
        }
        if let Some(projected) = value.read_lock::<__rawr_internal::scope::Projected>() {
            return Ok(Self::Projected(projected.clone()));
        }
        if let Some(place) = __rawr_internal::place::get(value) {
            return Ok(Self::Place(place));
        }
        value
            .read_lock::<NativeHandle>()
            .map(|handle| Self::Handle(handle.clone()))
            .ok_or(Error::TypeMismatch)
    }
}

/// What a reach of a lender failed with, as the adapter that reached it
/// reports it.
#[allow(dead_code)]
fn unreached(error: Box<__rawr_internal::EvalAltResult>) -> Error {
    match *error {
        __rawr_internal::EvalAltResult::ErrorRuntime(message, _) => Error::Unreached(message.to_string()),
        error => Error::Unreached(error.to_string()),
    }
}

#[allow(dead_code)]
struct LenderF<T>(std::marker::PhantomData<fn() -> T>);

impl<T: 'static> __rawr_internal::scope::Family for LenderF<T> {
    type Of<'a> = T;
}

thread_local! {
    /// The arena slots a lent slice's operation has claimed on this
    /// thread, by arena, slot and generation: a nested operation on a slice the
    /// same value lends shares the claim, as Rust's shared borrows do.
    static LENDERS: RefCell<std::collections::BTreeMap<([u8; 32], u32, u64), __rawr_internal::scope::Nonce>> =
        const { RefCell::new(std::collections::BTreeMap::new()) };
}

#[allow(dead_code)]
struct Lending(([u8; 32], u32, u64));

impl Drop for Lending {
    fn drop(&mut self) {
        LENDERS.with_borrow_mut(|lenders| lenders.remove(&self.0));
    }
}

/// A native value a `&mut` of its crate method's borrows (#1325), kept in
/// its slot's home with the guard (`island::place::Yoke`): the guard's
/// place reaches the target here by the yoke's `id`. Any other use of the
/// value ends the guard first, as rustc ends the borrow at its last use,
/// and the value goes back to its slot the next time the slot settles.
#[allow(dead_code)]
struct Yoked<U: ?Sized + 'static> {
    id: u64,
    yoke: RefCell<__rawr_internal::island::place::Yoke<U>>,
}

impl<U: ?Sized + 'static> Homed for Yoked<U> {
    fn shared(&self, call: &mut dyn FnMut(&dyn Any)) -> Result<(), Error> {
        let mut yoke = self.yoke.try_borrow_mut().map_err(|_| Error::Leased)?;
        call(yoke.owner());
        Ok(())
    }

    fn mutable(&self, call: &mut dyn FnMut(&mut dyn Any)) -> Result<(), Error> {
        let mut yoke = self.yoke.try_borrow_mut().map_err(|_| Error::Leased)?;
        call(yoke.owner());
        Ok(())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn settle(self: Box<Self>) -> Result<Box<dyn Any>, Box<dyn Homed>> {
        Ok(self.yoke.into_inner().into_owner())
    }
}

/// What a guard's place holds of the guard `id` on the value of `handle`
/// ([`Arena::yoke`]): the guard ends when the last copy of the place
/// drops, as Rust drops it at the end of its statement or block.
#[allow(dead_code)]
struct YokeEnd<U: ?Sized + 'static> {
    handle: NativeHandle,
    id: u64,
    target: std::marker::PhantomData<fn() -> Box<U>>,
}

#[allow(dead_code)]
impl<U: ?Sized + 'static> YokeEnd<U> {
    /// Whether the guard lasts: the method gave one, and nothing ended it.
    fn held(&self, arena: &Arena) -> bool {
        arena
            .homed(&self.handle)
            .and_then(|homed| homed.as_any().downcast_ref::<Yoked<U>>())
            .is_some_and(|yoked| yoked.id == self.id)
    }

    /// `body` of the guard's target, while the guard lasts.
    fn with<R>(&self, arena: &Arena, body: impl FnOnce(&mut U) -> R) -> Result<R, Error> {
        let ended = || Error::Lent(std::any::type_name::<U>(), __rawr_internal::scope::Error::Expired);
        let yoked = arena
            .homed(&self.handle)
            .and_then(|homed| homed.as_any().downcast_ref::<Yoked<U>>())
            .filter(|yoked| yoked.id == self.id)
            .ok_or_else(ended)?;
        let mut yoke = yoked.yoke.try_borrow_mut().map_err(|_| Error::Leased)?;
        yoke.target().map(body).ok_or_else(ended)
    }
}

impl<U: ?Sized + 'static> Drop for YokeEnd<U> {
    fn drop(&mut self) {
        let ended = Unyoke::<U> { handle: self.handle.clone(), id: self.id, target: std::marker::PhantomData };
        UNLENDS.with_borrow_mut(|unlends| unlends.push(Box::new(ended)));
        queued_here();
        let _ = with_arena(|_| Ok(()));
    }
}

/// A guard whose place is gone, ended the next time the owner thread has
/// the arena, unless a later guard took its place.
#[allow(dead_code)]
struct Unyoke<U: ?Sized + 'static> {
    handle: NativeHandle,
    id: u64,
    target: std::marker::PhantomData<fn() -> Box<U>>,
}

impl<U: ?Sized + 'static> Unlend for Unyoke<U> {
    fn unlend(self: Box<Self>, arena: &mut Arena) {
        let current = arena
            .homed(&self.handle)
            .and_then(|homed| homed.as_any().downcast_ref::<Yoked<U>>())
            .is_some_and(|yoked| yoked.id == self.id);
        if current {
            arena.settle(&self.handle);
        }
    }
}

/// The lend that holds the value of the crate value handle `value`, while
/// its slot has leased it to lend shared.
#[allow(dead_code)]
fn lender_nonce(value: &Dynamic) -> Option<__rawr_internal::scope::Nonce> {
    let handle = value.read_lock::<NativeHandle>()?;
    LENDERS.with_borrow(|lenders| lenders.get(&(handle.epoch, handle.slot, handle.generation)).copied())
}

/// Reach the receiver `lender` names, shared, for one operation on the
/// slice it lends. A value in a slot is leased for the operation, so the
/// arena is free while native code reads the slice, and the value goes
/// back when it ends. A value an iced view's home holds is read there.
#[allow(dead_code)]
fn reach_lender<T: 'static>(
    lender: &Lender,
    visit: &mut dyn FnMut(&T) -> Result<Dynamic, Box<__rawr_internal::EvalAltResult>>,
) -> Result<Dynamic, Box<__rawr_internal::EvalAltResult>> {
    let handle = match lender {
        Lender::Projected(projected) => {
            return projected.open(&mut |lent| reach_lender(&Lender::Lent(lent), visit));
        }
        Lender::Lent(lent) => {
            let failed = |error| place_error(Error::Lent(lent.name(), error));
            let access: fn(u64, &mut dyn FnMut(&T)) -> Result<(), u8> = lent.accessor(false).map_err(failed)?;
            let mut result = None;
            access(lent.nonce().bits(), &mut |value| result = Some(visit(value)))
                .map_err(|code| failed(__rawr_internal::scope::Error::from_code(code)))?;
            return result.unwrap_or_else(|| Err(failed(__rawr_internal::scope::Error::Busy)));
        }
        Lender::Place(place) => {
            return __rawr_internal::place::reach_as::<T, _>(place, false, |value| visit(value.read()));
        }
        Lender::Handle(handle) => handle,
    };
    fn claimed<T: 'static>(
        nonce: __rawr_internal::scope::Nonce,
        visit: &mut dyn FnMut(&T) -> Result<Dynamic, Box<__rawr_internal::EvalAltResult>>,
    ) -> Result<Dynamic, Box<__rawr_internal::EvalAltResult>> {
        __rawr_internal::scope::with_ref::<LenderF<T>, _>(nonce, |value| visit(value))
            .map_err(|error| place_error(Error::Value(error.to_string())))?
    }
    let key = (handle.epoch, handle.slot, handle.generation);
    if let Some(nonce) = LENDERS.with_borrow(|lenders| lenders.get(&key).copied()) {
        return claimed(nonce, visit);
    }
    let held = with_arena(|arena| {
        if arena.homed(handle).is_some() {
            return Ok(None);
        }
        arena.lease_place::<T>(handle.clone()).map(Some)
    })
    .map_err(place_error)?;
    let Some(held) = held else {
        return with_arena(|arena| arena.shared::<T, _>(handle.clone(), |value| visit(value)))
            .map_err(place_error)?;
    };
    let value = held.value.as_ref().expect("a leased receiver holds its value until restored");
    __rawr_internal::scope::lend_ref::<LenderF<T>, _>(value, |nonce| {
        LENDERS.with_borrow_mut(|lenders| lenders.insert(key, nonce));
        let _lending = Lending(key);
        claimed(nonce, visit)
    })
}

/// Reach the value `lender` names mutably, for the scope a pattern through
/// `&mut T` lends its fields to. A value in a slot is leased for the
/// scope, so the arena is free while the fields are lent, every other use
/// of the value meanwhile is refused, as rustc refuses it, and the value
/// goes back when the scope ends. A value an iced view's home holds is
/// changed there.
#[allow(dead_code)]
fn reach_lender_mut<T: 'static>(
    lender: &Lender,
    visit: &mut dyn FnMut(&mut T) -> Result<Dynamic, Box<__rawr_internal::EvalAltResult>>,
) -> Result<Dynamic, Box<__rawr_internal::EvalAltResult>> {
    let handle = match lender {
        Lender::Projected(projected) => {
            return Err(place_error(Error::Lent(projected.name(), __rawr_internal::scope::Error::ReadOnly)));
        }
        Lender::Lent(lent) => {
            let failed = |error| place_error(Error::Lent(lent.name(), error));
            let access: fn(u64, &mut dyn FnMut(&mut T)) -> Result<(), u8> = lent.accessor(true).map_err(failed)?;
            let mut result = None;
            access(lent.nonce().bits(), &mut |value| result = Some(visit(value)))
                .map_err(|code| failed(__rawr_internal::scope::Error::from_code(code)))?;
            return result.unwrap_or_else(|| Err(failed(__rawr_internal::scope::Error::Busy)));
        }
        Lender::Place(place) => {
            return __rawr_internal::place::reach_as::<T, _>(place, true, |mut value| visit(value.write()?));
        }
        Lender::Handle(handle) => handle,
    };
    let held = with_arena(|arena| {
        if arena.homed(handle).is_some() {
            return Ok(None);
        }
        arena.lease_place::<T>(handle.clone()).map(Some)
    })
    .map_err(place_error)?;
    let Some(mut held) = held else {
        return with_arena(|arena| arena.mutable::<T, _>(handle.clone(), |value| visit(value)))
            .map_err(place_error)?;
    };
    let value = held.value.as_mut().ok_or_else(|| place_error(Error::StaleHandle))?;
    visit(value)
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
    body: &mut dyn FnMut(&mut Arguments<'_, '_, '_>, &mut LentArguments<'_>) -> Result<Dynamic, Error>,
) -> Result<Dynamic, Error> {
    let found: Vec<_> = lendable
        .iter()
        .filter_map(|&(index, mutable, reach)| {
            let value = arguments.values.get(index)?;
            (value.is::<__rawr_internal::scope::Lent>()
                || __rawr_internal::place::get(value).is_some()
                || lender_nonce(value).is_some())
                .then(|| (index, mutable, (**value).clone(), reach))
        })
        .collect();
    nest_lent(arguments, &found, &mut Vec::new(), body)
}

/// Run `body` with the `&[T]` argument at `index` reached where it is,
/// when it is a native slice place, as a marked page's `&BLOCKS[1..2]` is
/// (#1291): the call borrows the elements in place, and none moves or is
/// copied. `None` for another value, which `body` takes from the arena.
#[allow(dead_code)]
fn with_slice_argument<E: __rawr_internal::place::Policy>(
    arguments: &mut Arguments<'_, '_, '_>,
    index: usize,
    body: &mut dyn FnMut(&mut Arguments<'_, '_, '_>, Option<&[E::Value]>) -> Result<Dynamic, Error>,
) -> Result<Dynamic, Error> {
    let Some(place) = arguments.values.get(index).and_then(|value| __rawr_internal::place::get(value)) else {
        return body(arguments, None);
    };
    let Arguments { values, arena } = arguments;
    lend(arena, || {
        __rawr_internal::place::slice::reach::<E, _>(&place, place.witness(), false, |slice| {
            Ok(with_arena(|arena| body(&mut Arguments { values, arena }, Some(slice.read()))))
        })
    })
    .map_err(|error| Error::Value(error.to_string()))?
}

#[allow(dead_code)]
fn nest_lent(
    arguments: &mut Arguments<'_, '_, '_>,
    found: &[(usize, bool, Dynamic, Reach)],
    reached: &mut Vec<(usize, Option<Reached<'_>>)>,
    body: &mut dyn FnMut(&mut Arguments<'_, '_, '_>, &mut LentArguments<'_>) -> Result<Dynamic, Error>,
) -> Result<Dynamic, Error> {
    let Some(((index, mutable, value, reach), rest)) = found.split_first() else {
        return body(arguments, reached);
    };
    let mut result = None;
    reach(value.clone(), *mutable, &mut |value| {
        let mut inner: Vec<(usize, Option<Reached<'_>>)> = reached
            .iter_mut()
            .map(|(index, value)| (*index, value.as_mut().map(Reached::reborrow)))
            .collect();
        inner.push((*index, Some(value)));
        result = Some(nest_lent(arguments, rest, &mut inner, body));
    })?;
    result.unwrap_or_else(|| Err(Error::Value("native argument was not reached".into())))
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

    /// The core's future of the value at argument 0, from the entry every
    /// table ends with: one that makes it when the type is a future, and
    /// refuses it otherwise.
    #[allow(dead_code)]
    fn future(self, arguments: &mut Arguments<'_, '_, '_>) -> Result<Dynamic, Error> {
        let entry = self.0.last().ok_or(Error::TypeMismatch)?;
        entry(arguments, self)
    }
}

/// The table of `entries`, each a call that takes a value of `T`'s type.
#[allow(dead_code)]
fn opaque_table<T: 'static, const N: usize>(
    _: std::marker::PhantomData<T>,
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

/// The values of the opaque types of a script impl function's own type
/// parameters, which native code fills, as `S` and `S::Ok` of a
/// `serialize<S: Serializer>` (#593): kept for the call in a frame of the
/// runner's generic impl function, where rustc knows their types, and
/// reached through it. A call that takes one runs the entry the frame
/// compiled for it, at the frame's types.
trait Frame {
    fn call(&mut self, nonce: u64, slot: u32, entry: usize, arguments: &mut Arguments<'_, '_, '_>) -> Result<Dynamic, Error>;
}

/// A frame as its call lends it to the script function it runs.
struct FrameF;

impl __rawr_internal::scope::Family for FrameF {
    type Of<'x> = &'x mut (dyn Frame + 'x);
}

/// A value a frame keeps, as script code holds it in the arena: the lend
/// of its frame, the slot of its type and its index there.
#[derive(Clone, Copy, Debug)]
struct FrameRef { nonce: u64, slot: u32, index: usize }

/// The script value of the value at `index` in `slot` of the frame lent as
/// `nonce`, which calls that take it reach through `table`.
#[allow(dead_code)]
fn frame_handle(arena: &mut Arena, nonce: u64, slot: u32, index: usize, table: OpaqueTable) -> Result<Dynamic, Error> {
    let handle = arena.insert(FrameRef { nonce, slot, index })?;
    arena.set_opaque(&handle, table);
    Ok(Dynamic::from(handle))
}

/// The index in `slot` of the value `handle` names in the frame lent as
/// `nonce`.
#[allow(dead_code)]
fn frame_index(arena: &Arena, handle: NativeHandle, nonce: u64, slot: u32) -> Result<usize, Error> {
    let reference = arena.shared::<FrameRef, _>(handle, |reference| *reference)?;
    if reference.nonce != nonce || reference.slot != slot {
        return Err(Error::Lent("a value native code gave a generic function", __rawr_internal::scope::Error::Mismatch));
    }
    Ok(reference.index)
}

std::thread_local! {
    /// The frames lent on this thread, innermost last: each one's nonce and
    /// the slot of each opaque type it keeps, by identity.
    static FRAMES: RefCell<Vec<(u64, &'static [([u8; 32], u32)])>> = const { RefCell::new(Vec::new()) };
}

/// A frame's place among [`FRAMES`] while it is lent.
struct FrameCurrent;

impl FrameCurrent {
    #[allow(dead_code)]
    fn enter(nonce: u64, keys: &'static [([u8; 32], u32)]) -> Self {
        FRAMES.with_borrow_mut(|frames| frames.push((nonce, keys)));
        Self
    }
}

impl Drop for FrameCurrent {
    fn drop(&mut self) {
        FRAMES.with_borrow_mut(|frames| {
            frames.pop();
        });
    }
}

/// The call `entry` of the innermost frame lent on this thread that keeps
/// values of the opaque type `key`, for a call that takes none of them, as
/// `<S::Error as Error>::custom(..)` makes one (#593).
#[allow(dead_code)]
fn frame_static(key: &[u8; 32], entry: usize, arguments: &mut Arguments<'_, '_, '_>) -> Result<Dynamic, Error> {
    let found = FRAMES.with_borrow(|frames| {
        frames.iter().rev().find_map(|(nonce, keys)| {
            keys.iter().find(|(known, _)| known == key).map(|(_, slot)| (*nonce, *slot))
        })
    });
    let (nonce, slot) = found.ok_or(Error::Lent("a value native code gave a generic function", __rawr_internal::scope::Error::Expired))?;
    __rawr_internal::scope::with::<FrameF, _>(__rawr_internal::scope::nonce(nonce), |frame| {
        frame.call(nonce, slot, entry, arguments)
    })
    .map_err(|error| Error::Lent("a value native code gave a generic function", error))?
}

/// Each value `fresh` names, made before its frame was lent, as of the
/// frame lent as `nonce`.
#[allow(dead_code)]
fn frame_lent(fresh: &[NativeHandle], nonce: u64) {
    let _ = with_arena(|arena| {
        for handle in fresh {
            arena.mutable::<FrameRef, _>(handle.clone(), |reference| reference.nonce = nonce)?;
        }
        Ok(())
    });
}

/// The call `entry` of the frame that keeps the value at argument
/// `position`, at its slot's type.
#[allow(dead_code)]
fn frame_dispatch(arguments: &mut Arguments<'_, '_, '_>, position: usize, slot: u32, entry: usize) -> Result<Dynamic, Error> {
    let handle = native_handle(arguments, position, false)?;
    let reference = arguments.arena.shared::<FrameRef, _>(handle, |reference| *reference)?;
    __rawr_internal::scope::with::<FrameF, _>(__rawr_internal::scope::nonce(reference.nonce), |frame| {
        frame.call(reference.nonce, slot, entry, arguments)
    })
    .map_err(|error| Error::Lent("a value native code gave a generic function", error))?
}

/// Strings a call takes as `&'static str`, as iced's `Font::new` does,
/// pinned for as long as the runner lives.
#[allow(dead_code)]
fn static_pins() -> &'static __rawr_internal::scope::Pins {
    static PINS: std::sync::OnceLock<__rawr_internal::scope::Pins> = std::sync::OnceLock::new();
    PINS.get_or_init(__rawr_internal::scope::Pins::lasting)
}

/// Whether each argument at `positions` is text the program lends at
/// `'static`, as a string literal is (`scope::lent_static`).
#[allow(dead_code)]
fn lent_static(arguments: &Arguments<'_, '_, '_>, positions: &[usize]) -> bool {
    positions.iter().all(|position| {
        arguments
            .values
            .get(*position)
            .is_some_and(|value| __rawr_internal::scope::lent_static(value))
    })
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

/// Scalars a struct's field borrows for its lifetime, as iced's
/// `LineDash::segments: &'a [f32]`, kept as [`static_bytes`] keeps bytes.
#[allow(dead_code)]
fn static_slice<T: PartialEq + Send + Sync + 'static>(values: Vec<T>) -> &'static [T] {
    static KEPT: std::sync::Mutex<Vec<&'static (dyn std::any::Any + Send + Sync)>> =
        std::sync::Mutex::new(Vec::new());
    let mut kept = KEPT.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(found) = kept
        .iter()
        .filter_map(|kept| kept.downcast_ref::<&'static [T]>())
        .find(|kept| ***kept == *values)
    {
        return *found;
    }
    let leaked: &'static [T] = Box::leak(values.into_boxed_slice());
    kept.push(Box::leak(Box::new(leaked)));
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

/// The value at `handle`, at the type `witness` names, as an `Option` of
/// an opaque type holds it.
#[allow(dead_code)]
fn take_handle_like<T: 'static>(
    arena: &mut Arena,
    handle: NativeHandle,
    _witness: std::marker::PhantomData<T>,
) -> Result<T, Error> {
    arena.take(handle)
}

/// [`Arena::shared`] at the type `witness` names, which Rust code can't
/// write.
#[allow(dead_code)]
fn shared_like<T: 'static, R>(
    arena: &Arena,
    handle: NativeHandle,
    _witness: std::marker::PhantomData<T>,
    call: impl FnOnce(&T) -> R,
) -> Result<R, Error> {
    arena.shared(handle, call)
}

/// [`Arena::mutable`] at the type `witness` names, which Rust code can't
/// write.
#[allow(dead_code)]
fn mutable_like<T: 'static, R>(
    arena: &mut Arena,
    handle: NativeHandle,
    _witness: std::marker::PhantomData<T>,
    call: impl FnOnce(&mut T) -> R,
) -> Result<R, Error> {
    arena.mutable(handle, call)
}

/// The type of `value`, which Rust code can't write when it is opaque.
#[allow(dead_code)]
fn witness_of<T>(_: &T) -> std::marker::PhantomData<T> {
    std::marker::PhantomData
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
        output.push(<u8 as From<bool>>::from(value.try_cast_result::<bool>().map_err(|_| Error::TypeMismatch)?));
    } else if value.is::<i64>() {
        output.push(2);
        push_bytes(output, &value.try_cast_result::<i64>().map_err(|_| Error::TypeMismatch)?.to_le_bytes())?;
    } else if value.is::<f64>() {
        output.push(3);
        push_bytes(output, &value.try_cast_result::<f64>().map_err(|_| Error::TypeMismatch)?.to_bits().to_le_bytes())?;
    } else if value.is::<char>() {
        output.push(4);
        push_bytes(output, &<u32 as From<char>>::from(value.try_cast_result::<char>().map_err(|_| Error::TypeMismatch)?).to_le_bytes())?;
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

fn runtime_error(error: Error) -> Box<__rawr_internal::EvalAltResult> {
    __rawr_internal::EvalAltResult::ErrorRuntime(
        format!("{:?}", error.reported()).into(),
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
        attest_within(&value, witness, arena)?;
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

/// `value` with each native value in it attested, as [`attest_within`]
/// attests them.
#[allow(dead_code)]
fn attest_nested(
    arena: &mut Arena,
    value: Dynamic,
    witness: &__rawr_internal::host::runtime::Witness,
) -> Result<Dynamic, Error> {
    attest_within(&value, witness, arena)?;
    Ok(value)
}

/// Each native value in `value` attested with the witness of its place in
/// `witness`: `value` itself, or one inside an `Option`, a `Result`, an
/// `Rc` or an `Arc`, a tuple or an array, at any depth, as the
/// `Option<SmolStr>` among a key press's fields or the `Vec<Family>` of
/// iced's `Result<Vec<Family>, Error>` is.
fn attest_within(
    value: &Dynamic,
    witness: &__rawr_internal::host::runtime::Witness,
    arena: &mut Arena,
) -> Result<(), Error> {
    if let Some(handle) = value.read_lock::<NativeHandle>().map(|handle| handle.clone()) {
        return arena.attest(handle, witness.clone());
    }
    let inner = |at: usize| witness.argument_witness(at);
    if let Some(option) = value.read_lock::<__rawr_internal::option::Value>() {
        return match (&*option, inner(0)) {
            (Some(some), Some(witness)) => attest_within(some, &witness, arena),
            _ => Ok(()),
        };
    }
    if let Some(result) = value.read_lock::<__rawr_internal::result::Value>() {
        return match &*result {
            Ok(ok) => inner(0).map_or(Ok(()), |witness| attest_within(ok, &witness, arena)),
            Err(err) => inner(1).map_or(Ok(()), |witness| attest_within(err, &witness, arena)),
        };
    }
    if let Some((pointee, _, _)) = __rawr_internal::crossing::pointee(value) {
        // The value an `Rc` or `Arc` a crate gave points to, as its type.
        return inner(0).map_or(Ok(()), |witness| attest_within(&pointee, &witness, arena));
    }
    let Ok(elements) = value.as_array_ref() else {
        return Ok(());
    };
    for (index, element) in elements.iter().enumerate() {
        // A tuple's members have a witness each; an array's elements share
        // its argument's.
        if let Some(witness) = inner(index).or_else(|| inner(0)) {
            attest_within(element, &witness, arena)?;
        }
    }
    Ok(())
}

/// The witness of operand `index` of the running call: the exact type of
/// the program's value a `&mut String` or `&mut Vec<T>` lends, which the
/// value keeps as a native place once the call has it (#716).
#[allow(dead_code)]
fn operand_witness(index: usize) -> __rawr_internal::host::runtime::Witness {
    OPERANDS
        .with_borrow(|operands| operands.get(index).cloned().flatten())
        .expect("rawr: an imported call's operand has no type witness")
}

/// The program's own `String` or `Vec<T>` a call took `&mut` and changed,
/// given back as the native value itself, in place of Rawr's: the next
/// call lends it where it is, as rustc's borrow does, and the program
/// reads and changes it there (#716).
#[allow(dead_code)]
fn native_place<P: __rawr_internal::place::Policy>(
    value: P::Value,
    witness: &__rawr_internal::host::runtime::Witness,
) -> Result<Dynamic, Error> {
    __rawr_internal::place::owned::<P>(value, witness).map_err(|error| Error::Value(error.to_string()))
}

/// A native value a call took from the owned place `owner` and changed,
/// given back to that place, which every handle of it reaches (#716), or,
/// for a program's value, a native place of its own.
#[allow(dead_code)]
fn restored_place<P: __rawr_internal::place::Policy>(
    owner: Option<__rawr_internal::place::Place>,
    value: P::Value,
    witness: &__rawr_internal::host::runtime::Witness,
) -> Result<Dynamic, Error> {
    match owner {
        Some(place) => __rawr_internal::place::restore::<P>(&place, witness, value)
            .map(|()| Dynamic::from(place))
            .map_err(|error| Error::Value(error.to_string())),
        None => native_place::<P>(value, witness),
    }
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
        // The witness of the call's declared output: a function's, a
        // method's that takes its receiver by value, from its receiver's,
        // or a method's that borrows its receiver, as an async method's
        // future (D513.6).
        let declared = || {
            call.output_witness()
                .ok()
                .or_else(|| {
                    receiver
                        .as_ref()
                        .and_then(|witness| call.transform_native_witness(witness).ok())
                })
                .or_else(|| call.borrowed_output_witness().ok())
        };
        // A projected iced Task takes this call's output witness.
        let output = match output.try_cast_result::<__rawr_internal::host::runtime::Unwitnessed>() {
            Ok(pending) => pending.witness(declared().ok_or(Error::Stale)?).ok_or(Error::Stale)?,
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
        } else if let Some(witness) = declared() {
            // A Rust value inside an `Option` or a `Result` takes the
            // witness of its own type argument.
            attest_within(&output, &witness, arguments.arena)?;
        }
        Ok(output)
    })
    .map_err(runtime_error)
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
    // A value native code lent the script function (D558.3), with another
    // lent value or one the arena holds.
    let lent = |value: &Dynamic| value.read_lock::<__rawr_internal::scope::Lent>().map(|lent| *lent);
    match (lent(lhs), lent(rhs)) {
        (Some(lhs), Some(rhs)) => return lhs.equal(&rhs).ok().flatten(),
        (Some(lent), None) | (None, Some(lent)) => {
            let other = if lhs.is::<__rawr_internal::scope::Lent>() { rhs } else { lhs };
            let handle = other.read_lock::<NativeHandle>().map(|value| value.clone())?;
            return arena_access(|arena| arena.with_value(handle, |value| lent.equal_native(value).ok().flatten()))
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
    __rawr_internal::host::value::register_relations::<__rawr_internal::scope::Lent>(native_equal, native_compare);
    register_defaults();
    let mut registry = __rawr_internal::host::runtime::Registry::new();
    // The core's future, which a crate's async functions give in a program
    // with no host profile (D513.3), before the bindings that name it.
    registry.install(
        __rawr_internal::future::facade::runtime()
            .map_err(__rawr_internal::host::runtime::Error::profile)?,
    )?;
    let mut native = engine.begin_imported_runtime()?;
    native.authenticate_structural(definitions, STRUCTURAL_ROWS)?;
    let mut builders = definitions
        .iter()
        .cloned()
        .map(__rawr_internal::host::runtime::Builder::new)
        .collect::<Vec<_>>();
    __rawr_profile::bind_runtime(definitions, &mut builders)
        .map_err(__rawr_internal::host::runtime::Error::profile)?;
    bind_profile_projections(definitions, &mut builders)?;
    for (definition, mut builder) in definitions.iter().zip(builders) {
        builder.bind_imported_type_attestations(native_attestation)?;
        for ty in definition.types().filter(|ty| {
            ty.identity().imported().is_none() && !ty.parameters().is_empty()
        }) {
            let path = ty.path();
            if !builder.has_type_attestation(&path) {
                builder.bind_type_attestation(path, native_attestation)?;
            }
        }
        for adapter in adapters().copied().filter(|adapter| adapter.bound && adapter.root == definition.root()) {
            // Each instance of a generic item has its own adapter, which
            // a call reaches by the key of its instance's types alone.
            if adapter.instance != [0; 32] {
                builder.bind_imported_instance(adapter.selector, adapter.instance, adapter.item, move |call, values| runtime_invoke(adapter, call, values))?;
            } else if builder.accepts_imported_at(&adapter.selector, adapter.item) {
                builder.bind_imported_at(adapter.selector, adapter.item, move |call, values| runtime_invoke(adapter, call, values))?;
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
    _bindings: std::sync::Arc<[__rawr_internal::host::facade::Definition]>,
    /// Whether `main` is Rawr code, which runs on the thread that owns the
    /// program while the protocol goes on beside it.
    marked_main: bool,
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
    let domain = b"rawr.snapshot.entry/v2";
    if !input.starts_with(domain) { return Err(std::io::Error::other("snapshot entry")); }
    input = &input[domain.len()..];
    let entry = std::str::from_utf8(snapshot_bytes(&mut input, 1024 * 1024)?)
        .map_err(|_| std::io::Error::other("snapshot entry UTF-8"))?;
    // The entry package's configuration, as `rustc --print cfg` lines: the
    // features the parent's Cargo enabled and the options rustc sets.
    if input.len() < 8 { return Err(std::io::Error::other("snapshot features")); }
    let count = u64::from_le_bytes(input[..8].try_into().map_err(|_| std::io::Error::other("snapshot features"))?);
    input = &input[8..];
    if count > (input.len() / 8) as u64 { return Err(std::io::Error::other("snapshot features")); }
    let mut features = Vec::new();
    for _ in 0..count {
        features.push(std::str::from_utf8(snapshot_bytes(&mut input, 1024 * 1024)?)
            .map_err(|_| std::io::Error::other("snapshot feature UTF-8"))?);
    }
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
        .map(|snapshot| snapshot.with_features(__rawr_internal::project::cargo::features::Enabled::of_lines(features)))
        .map_err(|error| std::io::Error::other(error.to_string()))
}

/// Under `rawr mcp`, the socket it named in `RAWR_MCP_INSPECT`, where a
/// tool reads the running program's values (D675). Bound once; each engine
/// this runner compiles a generation on answers on it. A plain `rawr run`
/// names none, and no debugger is registered.
fn inspector() -> Option<&'static __rawr_internal::inspect::Inspector> {
    static INSPECTOR: std::sync::OnceLock<Option<__rawr_internal::inspect::Inspector>> =
        std::sync::OnceLock::new();
    INSPECTOR
        .get_or_init(|| {
            let socket = std::env::var_os("RAWR_MCP_INSPECT").filter(|path| !path.is_empty())?;
            // A crate value lives in the arena behind a handle: its type
            // names a value without `Debug`.
            __rawr_internal::inspect::name_undebuggable_with(|value| {
                let handle = value.read_lock::<NativeHandle>()?.clone();
                arena_access(|arena| arena.undebuggable(&handle)).flatten()
            });
            __rawr_internal::inspect::Inspector::bind(std::path::Path::new(&socket))
                .map_err(|error| eprintln!("rawr: values can't be read from this run: {error}"))
                .ok()
        })
        .as_ref()
}

/// Under `rawr mcp`, place the steps of `program`, which runs next.
fn locate(program: &Program) {
    if let Some(inspector) = inspector()
        && let Some(located) = program.candidate.as_ref().and_then(|candidate| candidate.located())
    {
        let located = std::sync::Arc::clone(located);
        inspector.locate_with(move |position| located.line(position));
        // Its breakpoints move to this generation's lines (#489).
        if let Some(ast) = &program.ast {
            inspector.compiled(ast);
        }
    }
}

enum Compilation<'a> {
    Initial,
    Replacement(&'a Option<Program>),
    /// Beside a running marked `main`, against the engine it runs.
    Live(&'a __rawr_internal::Shared<__rawr_internal::Engine>),
}

/// A new engine with this runner's generated bindings installed, which
/// keeps their native account: what every program compiles against. Its
/// programs may reach `access` (D683.4).
fn prepare_engine(
    mut native: __rawr_internal::package::import::runner::account::Native,
    access: __rawr_internal::Access,
    progress: &mut impl FnMut(u8) -> std::io::Result<()>,
) -> std::io::Result<(
    __rawr_internal::Shared<__rawr_internal::Engine>,
    Box<[__rawr_internal::host::facade::Definition]>,
    __rawr_profile::ApplicationContract,
)> {
    let mut engine = __rawr_internal::Engine::new();
    engine.set_access(access);
    // Under `rawr mcp`, before the program compiles (#526).
    if let Some(inspector) = inspector() {
        inspector.attach(&mut engine);
    }
    let (bindings, application_contract) = __rawr_profile::install_compiler_bindings(&mut engine, &mut native)?;
    progress(2)?;
    __rawr_internal::package::import::runner::admit_generated_structural_retained(
        &mut engine,
        &bindings,
        STRUCTURAL_ROWS,
        &mut native,
    )
    .map_err(|error| std::io::Error::other(error.to_string()))?;
    __rawr_internal::package::import::runner::admit_generated_applications(
        &mut engine,
        &bindings,
        rawr_applications::APPLICATIONS,
        adapters().filter(|adapter| adapter.application).map(|adapter| adapter.identity),
    )
    .map_err(|error| std::io::Error::other(error.to_string()))?;
    progress(3)?;
    let engine = engine.retain_imported_native(native)
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    Ok((engine, bindings, application_contract))
}

fn compile_program(
    input: &[u8],
    compilation: Compilation<'_>,
    progress: &mut impl FnMut(u8) -> std::io::Result<()>,
) -> std::io::Result<Option<Program>> {
    let (expected, input) = input
        .split_first_chunk::<32>()
        .ok_or_else(|| std::io::Error::other("application semantic"))?;
    let snapshot = decode_snapshot(input)?;
    progress(1)?;
    if !COMPILE_SNAPSHOT { return Ok(None); }
    let native = match compilation {
        Compilation::Initial => __rawr_internal::package::import::runner::account::Native::decode(GENERATED_MANIFEST),
        Compilation::Replacement(current) => current.as_ref()
            .ok_or_else(|| std::io::Error::other("missing original compiler ownership"))?
            .engine.reconstruct_imported_native(GENERATED_MANIFEST),
        Compilation::Live(engine) => engine.reconstruct_imported_native(GENERATED_MANIFEST),
    };
    let native = native.map_err(|error| std::io::Error::other(error.to_string()))?;
    // `rawr run` runs the program as Cargo builds it: with everything.
    let (engine, bindings, application_contract) =
        prepare_engine(native, __rawr_internal::Access::all(), &mut *progress)?;
    let (candidate, ast) = __rawr_profile::compile_initial(
        &engine,
        &snapshot,
        &application_contract,
        expected,
    )?;
    progress(4)?;
    progress(5)?;
    let marked_main = snapshot.marked_root();
    Ok(Some(Program { engine, ast: Some(ast), candidate, application_contract,
        _bindings: bindings.into(), marked_main }))
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
            // future, and a panic it raises there is its own, as cargo
            // shows it.
            Task(Box::new(move || Box::pin(make())))
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

        /// The core's future of a native value whose type is a future, as
        /// a program's own `impl Straw<..>` is, where a future is taken:
        /// its output is made a Rawr value on the thread that owns the
        /// program, and taken back as the native type the call names.
        pub fn native(task: Task<Materialize>) -> Dynamic {
            Dynamic::from(__rawr_internal::future::Future::native(move || {
                let future = (task.0)();
                Box::pin(async move {
                    let materialize = future.await;
                    Box::new(move || materialize(None).map_err(__rawr_internal::future::error))
                        as __rawr_internal::future::Finish
                })
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
        /// The running program's runtime registry, which each generation
        /// it admits binds a next generation of (D475.2, D553.4), so native
        /// values keep their witnesses.
        static RUNTIME: std::cell::RefCell<Option<__rawr_internal::host::runtime::Registry>> =
            const { std::cell::RefCell::new(None) };
        /// The program the owner thread admitted last, kept while it runs.
        static ADMITTED: std::cell::RefCell<Option<super::Program>> =
            const { std::cell::RefCell::new(None) };
    }

    fn bind(program: &mut super::Program) -> Result<__rawr_internal::reload::installation::Bootstrap, String> {
        let candidate = program.candidate.take().ok_or("generation program unavailable")?;
        let kept = RUNTIME.with_borrow(|runtime| runtime.as_ref().map(|runtime| runtime.next_generation()));
        let runtime = match kept {
            Some(runtime) => runtime,
            None => {
                let runtime = super::imported_runtime(&program.engine, &program._bindings)
                    .map_err(|error| error.to_string())?;
                RUNTIME.set(Some(runtime.next_generation()));
                runtime
            }
        };
        candidate.bind(&program.engine, runtime).map_err(|error| error.to_string())
    }

    /// Make `program`, compiled beside the running program, the generation
    /// its next calls from native code run in (D553.3). This
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
    /// `main` from its Rawr one, for this thread to run: its calls into
    /// marked code run here, as cargo's build runs them (#1119).
    pub fn run(program: &mut super::Program) -> super::Started {
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
        if let Some(entry) = __rawr_internal::island::take_entry() {
            return super::Started::Entry(entry);
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
    pub fn install_compiler_bindings(
        engine: &mut __rawr_internal::Engine,
        native: &mut __rawr_internal::package::import::runner::account::Native,
    ) -> std::io::Result<(Box<[__rawr_internal::host::facade::Definition]>, ApplicationContract)> {
        // Async code compiles against the core's future (D513.3).
        engine
            .register_host_facade(
                __rawr_internal::future::facade::definition().map_err(std::io::Error::other)?,
            )
            .map_err(std::io::Error::other)?;
        if super::IMPORTED_BINDINGS.is_empty() {
            return Ok((Box::default(), ApplicationContract));
        }
        __rawr_internal::package::import::runner::install_retained(engine, super::IMPORTED_BINDINGS, native)
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
    stream.write_all(&12_u32.to_be_bytes())?;
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
    if frame[..4] != 12_u32.to_be_bytes() { return Err(std::io::Error::other("wire version")); }
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

/// One frame from the parent, as the protocol's reader thread received it.
/// Its name is one no script type takes, since a script type's proxy is
/// declared at the crate root beside it.
type __RawrFrame = std::io::Result<(u8, Vec<u8>)>;

/// The parent's next frame.
fn next_frame(frames: &std::sync::mpsc::Receiver<__RawrFrame>) -> std::io::Result<(u8, Vec<u8>)> {
    frames
        .recv()
        .unwrap_or_else(|_| Err(std::io::Error::other("protocol reader stopped")))
}

/// What starting the program left this thread to do.
enum Started {
    /// The program ended with this status.
    Exit(i32),
    /// The entry the program registered, for this thread to run: its
    /// native `main` (D501), or an async `main`'s runtime entry (D513.4).
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
    bindings: std::sync::Arc<[__rawr_internal::host::facade::Definition]>,
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
    bindings: &'a std::sync::Arc<[__rawr_internal::host::facade::Definition]>,
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
    let (candidate, carrier, coverage) = compiled.into_parts();
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
        _bindings: resident.bindings.clone(),
        marked_main: snapshot.marked_root(),
    };
    Ok(ResidentResult::Compiled(program, None, carrier, coverage))
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
            return Err(std::io::Error::other("invalid hot preparation transition"));
        }
        37 if pending.is_none() && hot.begun.as_ref().is_some_and(|begun| begun.observed) => {
            let begun = hot.begun.take().ok_or_else(|| std::io::Error::other("missing original resident request"))?;
            let decoded = __rawr_internal::package::import::resident::wire::Attempt::decode(payload, &begun.identity)
                .map_err(|error| std::io::Error::other(error.to_string()));
            let (attempt, input) = decoded?;
            let snapshot = decode_snapshot(input)?;
            let nonce: [u8; 16] = begun.identity[32..48].try_into()
                .map_err(|_| std::io::Error::other("resident transition nonce"))?;
            send_progress(stream, key, &nonce, 3, 1, 1)?;
            let output = match &resident {
                Some(resident) => compile_resident(resident, &snapshot),
                None => Ok(ResidentResult::Miss),
            };
            let output = output?;
            send_progress(stream, key, &nonce, 3, 2, 4)?;
            let (response, diagnostic) = match output {
                ResidentResult::Compiled(program, semantic, carrier, coverage) => {
                    let compiled = attempt.compiled(carrier, coverage, semantic);
                    *pending = Some(PendingProgram { key: compiled.key(), nonce, program: Some(program) });
                    send_frame(stream, 38, compiled.encode())?;
                    return Ok(true);
                }
                ResidentResult::Miss => (39, String::new()),
                ResidentResult::Diagnostic(reason) => (40, reason),
                ResidentResult::Resource(reason) => (42, reason),
            };
            // Every other answer consumes the request it answers.
            resident_response(stream, attempt.response_header(), response, &diagnostic)?;
        }
        37 => {
            return Err(std::io::Error::other("invalid resident preparation transition"));
        }
        _ => return Ok(false),
    }
    Ok(true)
}

/// A generation the owner thread admitted: the owner said `queued` when it
/// committed (D553.3), and says `swapped` to the program and to a `rawr
/// mcp` that supervises the run once this writes it to the reload stream.
fn admitted() {
    use std::io::Write as _;
    let Some(path) = std::env::var_os("RAWR_RELOAD_EVENTS").filter(|path| !path.is_empty()) else {
        return;
    };
    if let Ok(mut stream) = std::os::unix::net::UnixStream::connect(path) {
        let _ = stream.write_all(b"{\"event\":\"swapped\"}\n");
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
    received: std::sync::mpsc::Receiver<__RawrFrame>,
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
                bindings: &running.bindings,
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
                        send_frame(&mut stream, 10, &response)?;
                        continue;
                    }
                    let mut ordinal = 0_u8;
                    let mut progress = |stage| {
                        ordinal = ordinal.checked_add(1).ok_or_else(|| std::io::Error::other("progress ordinal"))?;
                        send_progress(&mut stream, &next, &nonce, 3, ordinal, stage)
                    };
                    let compiled = compile_program(&payload[112..], Compilation::Live(&running.engine), &mut progress);
                    drop(progress);
                    let candidate = compiled?;
                    let refused = candidate
                        .as_ref()
                        .and_then(|program| program.candidate.as_ref())
                        .and_then(|next| __rawr_internal::island::refusal(&running.generation, next));
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
                        evaluate_program(&mut program)?;
                        if let (Some(running), Some(next)) = (live.as_mut(), program.candidate.as_ref()) {
                            running.engine = program.engine.clone();
                            running.generation = next.generation().clone();
                            running.bindings = program._bindings.clone();
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
    let (frames, received) = std::sync::mpsc::channel::<__RawrFrame>();
    let mut reader = stream.try_clone()?;
    std::thread::spawn(move || loop {
        let frame = receive_frame(&mut reader);
        let end = frame.is_err();
        if frames.send(frame).is_err() || end {
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
                bindings: &program._bindings,
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
                program = match compile_program(&payload[48..], Compilation::Initial, &mut progress) {
                    Ok(program) => program,
                    Err(error) => {
                        drop(progress);
                        return Err(error);
                    }
                };
                drop(progress);
                prepared = true;
                let mut response = Vec::with_capacity(48);
                response.extend_from_slice(&key);
                response.extend_from_slice(&nonce);
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
                    send_frame(&mut stream, 10, &response)?;
                    continue;
                }
                let mut ordinal = 0_u8;
                let mut progress = |stage| {
                    ordinal = ordinal.checked_add(1).ok_or_else(|| std::io::Error::other("progress ordinal"))?;
                    send_progress(&mut stream, &next, &nonce, 3, ordinal, stage)
                };
                let compiled = compile_program(&payload[112..], Compilation::Replacement(&program), &mut progress);
                drop(progress);
                let candidate = compiled?;
                let mut response = Vec::with_capacity(48);
                response.extend_from_slice(&next);
                response.extend_from_slice(&nonce);
                pending = Some(PendingProgram { key: next, nonce, program: candidate });
                hot.begun = None;
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
                    return Err(std::io::Error::other("missing hot preparation"));
                };
                if candidate.key != next || candidate.nonce != nonce {
                    return Err(std::io::Error::other("stale hot commit"));
                }
                if let Some(candidate_program) = &mut candidate.program {
                    evaluate_program(candidate_program)?;
                }
                install_arena(next);
                key = next;
                program = candidate.program;
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
                    return Err(std::io::Error::other("missing hot preparation"));
                };
                if candidate.key != next || candidate.nonce != nonce {
                    return Err(std::io::Error::other("stale hot abort"));
                }
                drop(candidate.program);
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
                // The program runs here, on the thread that owns it, for as
                // long as it runs; the protocol goes on beside it. That
                // thread is the process's main thread, so a windowing
                // crate's event loop runs as under `cargo run`, whether a
                // marked `main` or the native `main` of a native root
                // (D501, #1119) starts it. A save is offered to this thread,
                // which admits it at its next call from native code
                // (D553.3), or restarts the program when it can't take it.
                // A panic ends the program with Rust's status, 101.
                let Some(running) = program.as_mut() else {
                    exit(0);
                };
                let stream = stream.try_clone()?;
                let live = running.candidate.as_ref().map(|candidate| Live {
                    engine: running.engine.clone(),
                    generation: candidate.generation().clone(),
                    bindings: running._bindings.clone(),
                    contract: running.application_contract.clone(),
                });
                std::thread::Builder::new()
                    .name("rawr protocol".into())
                    .spawn(move || serve_entry(stream, received, key, live))?;
                let started = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    __rawr_profile::run(running)
                }));
                match started {
                    Ok(Started::Exit(status)) => exit(status),
                    // A native `main`, or an async `main`'s runtime entry
                    // (D527).
                    Ok(Started::Entry(entry)) => exit(std::panic::catch_unwind(entry).unwrap_or(101)),
                    Err(_) => exit(101),
                }
            }
            17 if admitted => match invoke_adapter(&key, &payload) {
                Ok(value) => send_frame(&mut stream, 18, &value)?,
                Err(error) => send_frame(&mut stream, 19, &invoke_rejection(&key, &error))?,
            },
            17 => send_frame(&mut stream, 19, &invoke_rejection(&key, &Error::Stale))?,
            12 if payload.as_slice() == key => {
                send_frame(&mut stream, 13, &key)?;
                return Ok(());
            }
            _ => {
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

/// This runner's generated bindings, as `rawr::universe` installs them in
/// an engine and binds their runtime (#1387). Its name is one no script
/// type takes.
struct __RawrBindings;

impl __rawr_internal::universe::Bindings for __RawrBindings {
    fn engine(
        &self,
        access: __rawr_internal::Access,
    ) -> std::io::Result<(
        __rawr_internal::Shared<__rawr_internal::Engine>,
        Box<[__rawr_internal::host::facade::Definition]>,
    )> {
        install_arena([0; 32]);
        let native = __rawr_internal::package::import::runner::account::Native::decode(GENERATED_MANIFEST)
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        let (engine, bindings, _) = prepare_engine(native, access, &mut |_| Ok(()))?;
        Ok((engine, bindings))
    }

    fn runtime(
        &self,
        engine: &__rawr_internal::Engine,
        definitions: &[__rawr_internal::host::facade::Definition],
    ) -> Result<__rawr_internal::host::runtime::Registry, __rawr_internal::host::runtime::Error> {
        imported_runtime(engine, definitions)
    }
}

/// `?play`'s entry in a universe runner (#434, #436, #515): a server on the
/// socket `RAWR_PLAY_SOCKET` names. What every snippet needs is made once,
/// here: a universe of the runner's bindings (`rawr::universe`). Each
/// snippet then runs in a fork of this process (see `__rawr_internal::play`),
/// which loads the crate root's source beside the manifest in `root` in
/// that universe, with no parent and no Cargo, and runs it once as Rust
/// runs it. The manifest is the one the runner's build compiled, at the
/// same path, so a program whose native calls the build covered finds every
/// one of them, and one that makes another fails to compile there.
fn run_play(root: std::path::PathBuf) -> ! {
    let Some(socket) = std::env::var_os(__rawr_internal::play::SOCKET) else {
        eprintln!("error: `{}` names no socket to serve on", __rawr_internal::play::SOCKET);
        exit(1);
    };
    if !COMPILE_SNAPSHOT {
        eprintln!("error: this runner compiles no program");
        exit(1);
    }
    let manifest_path = root.join("Cargo.toml");
    let entry = root.join("src").join("main.rs");
    let prepared = std::fs::read_to_string(&manifest_path)
        .map_err(|error| error.to_string())
        .and_then(|manifest| {
            // A snippet runs on a server whose files, environment and
            // process are not its own: it reaches none of them.
            __rawr_internal::universe::Universe::new(&__RawrBindings, __rawr_internal::Access::none())
                .map(|universe| (manifest, universe))
                .map_err(|error| error.to_string())
        });
    let (manifest, universe) = match prepared {
        Ok(prepared) => prepared,
        Err(error) => {
            eprintln!("{error}");
            exit(1);
        }
    };
    let served = __rawr_internal::play::serve(std::path::Path::new(&socket), |source| {
        // A fork of this process, with its own copy of the universe.
        let files = vec![(manifest_path.clone(), Box::from(manifest.as_str()))];
        match universe.program(entry.clone(), source, files) {
            Ok(program) => exit(program.run()),
            Err(error) => {
                eprintln!("{error}");
                exit(1);
            }
        }
    });
    match served {
        Ok(()) => exit(0),
        Err(error) => {
            eprintln!("error: ?play's runner stopped: {error}");
            exit(1);
        }
    }
}

/// The protocol and the interpreter run on the process's main thread, where
/// the program's `main` then runs, a marked one or a native one (D533,
/// #1119): winit's event loop must run there.
pub(super) fn main() {
    register_native_sequences();
    for adapter in adapters() {
        let _ = (adapter.identity, adapter.signature, adapter.invoke);
    }
    if UNIVERSE {
        __rawr_internal::crossing::universe();
        if let Some(root) = std::env::var_os("RAWR_PLAY_ROOT") {
            run_play(root.into());
        }
    }
    let (stream, key, first) = match open_protocol() {
        Ok(opened) => opened,
        Err(error) => finish(Err(error)),
    };
    finish(run_protocol(stream, key, first))
}
"#;
const PROFILE_RUNNER_MAIN: &[u8] = br#"
fn send_frame(stream: &mut std::os::unix::net::UnixStream, tag: u8, payload: &[u8]) -> std::io::Result<()> {
    use std::io::Write as _;
    let length = 5_usize.checked_add(payload.len()).ok_or_else(|| std::io::Error::other("frame length"))?;
    if length > 8 * 1024 * 1024 { return Err(std::io::Error::other("frame limit")); }
    stream.write_all(&(length as u32).to_be_bytes())?;
    stream.write_all(&12_u32.to_be_bytes())?;
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
    if frame[..4] != 12_u32.to_be_bytes() { return Err(std::io::Error::other("wire version")); }
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
                program = match compile_program(&payload[48..], Compilation::Initial, &mut progress) {
                    Ok(program) => program,
                    Err(error) => {
                        drop(progress);
                        return Err(error);
                    }
                };
                match profile.prepare() {
                    Ok(()) => {
                        let advanced = progress(6);
                        drop(progress);
                        advanced?;
                        prepared = true;
                        let mut response = Vec::with_capacity(48);
                        response.extend_from_slice(&key);
                        response.extend_from_slice(&nonce);
                        send_frame(&mut stream, 4, &response)?;
                    }
                    Err(reason) => {
                        drop(progress);
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
                        result?;
                        admitted = true;
                        send_frame(&mut stream, 8, &payload)?;
                    }
                    Err(reason) => {
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
                    send_frame(&mut stream, 10, &response)?;
                    continue;
                }
                let mut ordinal = 0_u8;
                let mut progress = |stage| {
                    ordinal = ordinal.checked_add(1).ok_or_else(|| std::io::Error::other("progress ordinal"))?;
                    send_progress(&mut stream, &next, &nonce, 3, ordinal, stage)
                };
                let next_receipt: [u8; 32] = payload[64..96].try_into().map_err(|_| std::io::Error::other("activation receipt"))?;
                let compiled = compile_program(&payload[112..], Compilation::Replacement(&program), &mut progress);
                drop(progress);
                let candidate = compiled?;
                let mut response = Vec::with_capacity(48);
                response.extend_from_slice(&next);
                response.extend_from_slice(&nonce);
                pending = Some(PendingProfileProgram {
                    key: next,
                    receipt: next_receipt,
                    nonce,
                    program: candidate,
                });
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
                    return Err(std::io::Error::other("missing hot preparation"));
                };
                if candidate.key != next || candidate.nonce != nonce {
                    return Err(std::io::Error::other("stale hot commit"));
                }
                if let Some(candidate_program) = &mut candidate.program {
                    evaluate_program(candidate_program)?;
                }
                install_arena(next);
                key = next;
                program = candidate.program;
                receipt = Some(candidate.receipt);
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
                    return Err(std::io::Error::other("missing hot preparation"));
                };
                if candidate.key != next || candidate.nonce != nonce {
                    return Err(std::io::Error::other("stale hot abort"));
                }
                drop(candidate.program);
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
                send_frame(&mut stream, 13, &key)?;
                return Ok(());
            }
            _ => {
                return Err(std::io::Error::other("invalid protocol transition"));
            }
        }
    }
}

pub(super) fn main() {
    register_native_sequences();
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
    stream.write_all(&12_u32.to_be_bytes())?;
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
    if frame[..4] != 12_u32.to_be_bytes() { return Err(std::io::Error::other("wire version")); }
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
                response[32] = <u8 as From<bool>>::from(transport.take_refresh());
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
                transport.cancel_install(installation)
                    .map_err(|error| std::io::Error::other(error.to_string()))?;
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
                return Err(std::io::Error::other("invalid hot preparation transition"));
            }
            37 if pending.is_none()
                && begun.as_ref().is_some_and(|(_, _, observed)| *observed) => {
                let (identity, installation, _) = begun.take()
                    .ok_or_else(|| std::io::Error::other("missing original resident request"))?;
                let decoded = __rawr_internal::package::import::resident::wire::Attempt::decode(
                    &payload, &identity,
                ).map_err(|error| std::io::Error::other(error.to_string()));
                let (attempt, input) = decoded?;
                let snapshot = decode_snapshot(input)?;
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
                let output = output?;
                let (response_tag, diagnostic) = match output {
                    ResidentResult::Compiled(compiled) => {
                        let (candidate, carrier, coverage) = compiled.into_parts();
                        let compiled = attempt.compiled(carrier, coverage, None);
                        pending = Some(PendingLive {
                            installation,
                            key: compiled.key(),
                            receipt: compiled.advance_receipt(receipt),
                            nonce,
                            candidate,
                        });
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
                transport.cancel_install(installation)
                    .map_err(|error| std::io::Error::other(error.to_string()))?;
                resident_response(&mut stream, attempt.response_header(), response_tag, &diagnostic)?;
            }
            37 => {
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
                        reject(&mut stream, "profile produced no live generation")?;
                        continue;
                    }
                    Err(error) => {
                        drop(progress);
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
                    return Err(std::io::Error::other("missing hot preparation"));
                };
                if candidate.key != next || candidate.nonce != nonce {
                    return Err(std::io::Error::other("stale hot commit"));
                }
                let installed = transport.install(candidate.installation, candidate.candidate);
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
                    return Err(std::io::Error::other("missing hot preparation"));
                };
                if candidate.key != next || candidate.nonce != nonce {
                    return Err(std::io::Error::other("stale hot abort"));
                }
                drop(candidate.candidate);
                transport.cancel_install(candidate.installation)
                    .map_err(|error| std::io::Error::other(error.to_string()))?;
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
                let refresh = transport.retire()
                    .map_err(|error| std::io::Error::other(error.to_string()))?;
                let mut response = [0_u8; 33];
                response[..32].copy_from_slice(&key);
                response[32] = <u8 as From<bool>>::from(refresh);
                send_frame(&mut stream, 13, &response)?;
                return Ok(());
            }
            12 if payload.as_slice() == key => {
                transport.exit()
                    .map_err(|error| std::io::Error::other(error.to_string()))?;
                send_frame(&mut stream, 13, &key)?;
                return Ok(());
            }
            12 => {
                return Err(std::io::Error::other("invalid live retirement transition"));
            }
            _ => {
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
                program = match compile_program(&payload[48..], Compilation::Initial, &mut progress) {
                    Ok(program) => program,
                    Err(error) => {
                        drop(progress);
                        return Err(error);
                    }
                };
                match profile.prepare() {
                    Ok(()) => {
                        let advanced = progress(6);
                        drop(progress);
                        advanced?;
                        prepared = true;
                        let mut response = Vec::with_capacity(48);
                        response.extend_from_slice(&key);
                        response.extend_from_slice(&nonce);
                        send_frame(&mut stream, 4, &response)?;
                    }
                    Err(reason) => {
                        drop(progress);
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
                        result?;
                        admitted = true;
                        send_frame(&mut stream, 8, &payload)?;
                    }
                    Err(reason) => {
                        reject(&mut stream, &reason)?;
                    }
                }
            }
            48 if admitted && payload.len() >= 32 && payload[..32] == key => {
                // Start from the State the previous runner exported. No State
                // means it could not carry over, and the window says why.
                let mut response = [0_u8; 33];
                response[..32].copy_from_slice(&key);
                response[32] = <u8 as From<bool>>::from(profile.seed_state(&payload[32..]));
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
                        reject(&mut stream, reason)?;
                        continue;
                    }
                };
                let context = (|| {
                    let program = program.as_ref().ok_or_else(|| std::io::Error::other("generation program"))?;
                    let engine = program.engine.clone();
                    let application_contract = program.application_contract.clone();
                    let receipt = receipt.ok_or_else(|| std::io::Error::other("activation receipt"))?;
                    Ok::<_, std::io::Error>((engine, application_contract, receipt))
                })();
                let (engine, application_contract, receipt) = context?;
                let mut control = stream.try_clone()?;
                let worker = std::thread::Builder::new()
                    .name("rawr-generated-protocol".to_owned())
                    .spawn(move || live_protocol(stream, key, receipt, engine, application_contract, transport));
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
                    send_progress(&mut stream, &key, &nonce, 4, 1, 10)?;
                    let mut response = Vec::with_capacity(48 + outcome.len());
                    response.extend_from_slice(&key);
                    response.extend_from_slice(&nonce);
                    response.extend_from_slice(&outcome);
                    send_frame(&mut stream, 23, &response)?;
                }
                Err(reason) => {
                    reject(&mut stream, &reason)?;
                }
            },
            9 if admitted => {
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
                send_frame(&mut stream, 13, &key)?;
                return Ok(());
            }
            _ => {
                return Err(std::io::Error::other("invalid protocol transition"));
            }
        }
    }
}

pub(super) fn main() {
    register_native_sequences();
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
) -> Result<crate::runner::Manifest<crate::runner::Generated, B>, request::Error> {
    let product = render(&key.request, &key.native, key.profile.value())?;
    let actual = expected(&key.request, &product)?;
    assemble(key, product, actual, None)
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
    .map_err(request::unwritable)?;
    digest.update(entries.0.finalize());
    Ok(digest.finalize().into())
}

fn assemble<B: crate::surface::Runner>(
    mut key: super::GenerationRequestKey<B>,
    product: Product,
    actual: Expected,
    chain: Option<crate::node::Chain>,
) -> Result<crate::runner::Manifest<crate::runner::Generated, B>, request::Error> {
    let mut contents = vec![
        (LOCK_PATH, product.lock.as_slice()),
        (MANIFEST_PATH, product.manifest.as_slice()),
        (BUILD_PATH, BUILD_SCRIPT),
    ];
    contents.extend(decode_sources(&product.source)?);
    if contents.len() != actual.files.len() {
        return Err(request::drift("generated file cardinality"));
    }
    let mut files = Vec::new();
    let mut file_records = Vec::new();
    for ((path, bytes), (expected, (digest, length))) in contents.into_iter().zip(&actual.files) {
        if path != &**expected {
            return Err(request::drift("generated file paths"));
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
    )?;
    // Each variant's matchers, after the entries the inventory pairs with
    // plans one to one.
    for plan in &key.request.plans {
        for matcher in matchers(plan) {
            entries.push(crate::runner::manifest::Entry::new(
                matcher.identity(plan),
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
        // And its lender.
        if has_lender(plan) {
            entries.push(crate::runner::manifest::Entry::new(
                lender_identity(plan),
                signature(plan)?,
                1,
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
    generated.validate(&files).unwrap_or_else(|error| {
        panic!("rawr's generated runner manifest matches its files: {error}")
    });
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
    let paths = native.path_packages();
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
        (harness.lock(), &paths, &mut hidden),
        &mut sources,
        &mut spans,
    )?;
    let lock = runner_lock(harness.lock(), &hidden)?;
    let (manifest, aliases) = runner_manifest_aliased(
        harness.manifest(),
        named.as_deref(),
        &hidden,
        harness.lock(),
    )?;
    alias_crates(&aliases, &mut sources)?;
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
        return Err(request::drift("adapter span cardinality"));
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
/// its name from a source a manifest can name: a registry, a git
/// repository, or a path, which the lock leaves out and `paths` (each path
/// package's name and directory) gives.
fn hidden_crate(lock: &[u8], paths: &[(String, String)], segment: &str) -> Option<HiddenCrate> {
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
    let source = match package.get("source").and_then(Item::as_str) {
        Some(source) => source.to_owned(),
        None => {
            let name = package.get("name").and_then(Item::as_str)?;
            let mut found = paths.iter().filter(|(package, _)| package == name);
            let (_, directory) = found.next()?;
            if found.next().is_some() {
                return None;
            }
            format!("path+file://{directory}")
        }
    };
    (source.starts_with("git+")
        || source.starts_with("path+file://")
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
            source,
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
    } else if let Some(directory) = hidden.source.strip_prefix("path+file://") {
        table.insert("path", directory.into());
    } else {
        table.insert("version", format!("={}", hidden.version).into());
    }
    table.insert("default-features", false.into());
    table
}

/// Whether the harness's `lock` reaches the path package `name` from its
/// root through path packages alone: then the package is a member of the
/// runner's workspace without a dependency of its own.
fn path_reached(lock: &[u8], name: &str) -> bool {
    let Some(document) = std::str::from_utf8(lock)
        .ok()
        .and_then(|text| text.parse::<DocumentMut>().ok())
    else {
        return false;
    };
    let Some(packages) = document.get("package").and_then(Item::as_array_of_tables) else {
        return false;
    };
    // Path packages have no source, so an edge names one by its name and,
    // when two share it, its version.
    let paths = packages
        .iter()
        .filter(|package| !package.contains_key("source"))
        .filter_map(|package| {
            let name = package.get("name").and_then(Item::as_str)?;
            let version = package.get("version").and_then(Item::as_str)?;
            let edges = package
                .get("dependencies")
                .and_then(Item::as_array)
                .map(|edges| edges.iter().filter_map(|edge| edge.as_str()).collect())
                .unwrap_or_else(Vec::new);
            Some((name, version, edges))
        })
        .collect::<Vec<_>>();
    let mut reached = paths
        .iter()
        .filter(|(package, _, _)| *package == "rawr-resolution-harness")
        .collect::<Vec<_>>();
    let mut index = 0;
    while let Some((_, _, edges)) = reached.get(index) {
        index += 1;
        for edge in edges {
            let mut parts = edge.split(' ');
            let (target, version) = (parts.next(), parts.next());
            if parts.next().is_some() {
                // `name version (source)`: not a path package.
                continue;
            }
            for path in &paths {
                if Some(path.0) == target
                    && version.is_none_or(|version| version == path.1)
                    && !reached.iter().any(|seen| std::ptr::eq(*seen, path))
                {
                    reached.push(path);
                }
            }
        }
    }
    reached.iter().any(|(package, _, _)| *package == name)
}

#[cfg(test)]
fn runner_manifest(
    bytes: &[u8],
    binary: Option<&str>,
    hidden: &[HiddenCrate],
) -> Result<Vec<u8>, request::Error> {
    runner_manifest_aliased(bytes, binary, hidden, &[]).map(|(manifest, _)| manifest)
}

/// The crate names a dependency key gives Rust code.
fn crate_ident(key: &str) -> String {
    key.replace('-', "_")
}

/// The package a dependency entry names: its `package`, or its key.
fn dependency_package<'a>(key: &'a str, item: &'a Item) -> &'a str {
    item.as_table_like()
        .and_then(|table| table.get("package"))
        .and_then(Item::as_str)
        .unwrap_or(key)
}

/// Whether the lock holds exactly one package of this name: then every
/// dependency on that name is the same package.
fn single_locked(lock: &[u8], name: &str) -> bool {
    let Some(document) = std::str::from_utf8(lock)
        .ok()
        .and_then(|text| text.parse::<DocumentMut>().ok())
    else {
        return false;
    };
    document
        .get("package")
        .and_then(Item::as_array_of_tables)
        .is_some_and(|packages| {
            packages
                .iter()
                .filter(|package| package.get("name").and_then(Item::as_str) == Some(name))
                .count()
                == 1
        })
}

/// Make each extra name the runner needs for a crate the project already
/// depends on: `extern crate` at the crate root puts the project's crate in
/// the extern prelude under that name too, as Cargo allows one name per
/// package.
fn alias_crates(
    aliases: &[(String, String)],
    sources: &mut [(Box<str>, Vec<u8>)],
) -> Result<(), request::Error> {
    if aliases.is_empty() {
        return Ok(());
    }
    let main = sources
        .iter_mut()
        .find(|(path, _)| path.as_ref() == SOURCE_PATH)
        .ok_or_else(|| request::drift("generated runner root"))?;
    let first = main
        .1
        .iter()
        .position(|byte| *byte == b'\n')
        .map_or(0, |at| at + 1);
    let mut lines = Vec::new();
    for (existing, required) in aliases {
        writeln!(
            lines,
            "#[allow(unused_extern_crates)]\nextern crate {existing} as {required};"
        )
        .map_err(request::unwritable)?;
    }
    main.1.splice(first..first, lines);
    Ok(())
}

/// Each `(crate, name)` a runner reaches a project dependency by too.
type Aliases = Vec<(String, String)>;

/// The runner's manifest, and each `(crate, name)` the runner must also
/// reach a crate by: a crate Rawr's runner needs under its own name that
/// the project already depends on under another. Cargo refuses one package
/// under two names, so the project's dependency serves both.
fn runner_manifest_aliased(
    bytes: &[u8],
    binary: Option<&str>,
    hidden: &[HiddenCrate],
    lock: &[u8],
) -> Result<(Vec<u8>, Aliases), request::Error> {
    let mut aliases = Vec::new();
    let text = std::str::from_utf8(bytes).map_err(request::misread)?;
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut document = normalized
        .parse::<DocumentMut>()
        .map_err(request::misread)?;
    if !document.contains_key("workspace") {
        return Err(request::misread(
            "final-form harness has no standalone workspace",
        ));
    }
    let package = document
        .get_mut("package")
        .and_then(Item::as_table_like_mut)
        .ok_or_else(|| request::misread("final-form harness has no package table"))?;
    package.insert("name", value("rawr-generated-runner"));
    package.insert("version", value("0.0.0"));
    package.insert("edition", value("2024"));
    package.insert("build", value(BUILD_PATH));
    document.remove("lib");
    // Rawr's own crate is a build dependency of the harness, which builds no
    // unit of it. The runner links it, and its build script must not.
    if let Some(item) = document.remove(crate::cargo::BUILD_DEPENDENCIES) {
        let build = item.into_table().map_err(|_| {
            request::misread("final-form harness build dependencies are not a table")
        })?;
        let dependencies = document
            .get_mut("dependencies")
            .and_then(Item::as_table_mut)
            .ok_or_else(|| request::misread("final-form harness has no dependencies table"))?;
        for (key, item) in build {
            let package = dependency_package(&key, &item).to_owned();
            if let Some((existing, _)) = dependencies
                .iter()
                .find(|(existing, present)| dependency_package(existing, present) == package)
                .filter(|_| single_locked(lock, &package))
            {
                if crate_ident(existing) != crate_ident(&key) {
                    aliases.push((crate_ident(existing), crate_ident(&key)));
                }
                continue;
            }
            dependencies.insert(&key, item);
        }
        dependencies.sort_values();
    }
    // Each crate a macro's `$crate` names, as Rust reaches it.
    if !hidden.is_empty() {
        let dependencies = document
            .get_mut("dependencies")
            .and_then(Item::as_table_like_mut)
            .ok_or_else(|| request::misread("final-form harness has no dependencies table"))?;
        let mut excluded = Vec::new();
        for hidden in hidden {
            // `hidden_crate` found exactly one locked package of this name,
            // so a project dependency on it is the same package.
            let existing = dependencies
                .iter()
                .find(|(key, item)| {
                    dependency_package(key, item) == hidden.package && *key != hidden.segment
                })
                .map(|(key, _)| key.to_owned());
            if let Some(existing) = existing {
                aliases.push((crate_ident(&existing), hidden.segment.clone()));
                continue;
            }
            dependencies.insert(
                &hidden.segment,
                Item::Value(toml_edit::Value::InlineTable(hidden_dependency(hidden))),
            );
            // A path dependency under the runner's root is a workspace
            // member, which Cargo's lockfile resolves with all its
            // features. One the runner reaches only through another source,
            // as the target of a `[patch]`, stays out of the workspace as
            // it is in the project's, so the runner resolves no more than
            // the project did.
            if let Some(directory) = hidden.source.strip_prefix("path+file://")
                && !path_reached(lock, &hidden.package)
            {
                excluded.push(directory.to_owned());
            }
        }
        if !excluded.is_empty() {
            let workspace = document
                .get_mut("workspace")
                .and_then(Item::as_table_like_mut)
                .ok_or_else(|| request::misread("final-form harness workspace is not a table"))?;
            let exclude = workspace
                .entry("exclude")
                .or_insert(toml_edit::value(toml_edit::Array::new()))
                .as_array_mut()
                .ok_or_else(|| request::misread("final-form harness exclude is not a list"))?;
            for directory in excluded {
                if !exclude
                    .iter()
                    .any(|present| present.as_str() == Some(&directory))
                {
                    exclude.push(directory);
                }
            }
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
    aliases.sort_unstable();
    aliases.dedup();
    Ok((normalize(document.to_string().as_bytes()), aliases))
}

fn runner_lock(bytes: &[u8], hidden: &[HiddenCrate]) -> Result<Vec<u8>, request::Error> {
    let text = std::str::from_utf8(bytes).map_err(request::misread)?;
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut document = normalized
        .parse::<DocumentMut>()
        .map_err(request::misread)?;
    let packages = document
        .get_mut("package")
        .and_then(Item::as_array_of_tables_mut)
        .ok_or_else(|| request::misread("final-form harness lock has no package array"))?;
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
                    .ok_or_else(|| {
                        request::misread("final-form harness lock root dependencies are not a list")
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
        return Err(request::misread(format!(
            "expected one final-form harness lock root, found {roots}"
        )));
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
    (lock, paths, hidden): (&[u8], &[(String, String)], &mut Vec<HiddenCrate>),
    sources: &mut Vec<(Box<str>, Vec<u8>)>,
    spans: &mut Vec<(EntryIdentity, Box<str>, std::ops::Range<u32>)>,
) -> Result<Option<String>, request::Error> {
    let mut main = Vec::new();
    let output = &mut main;
    if !profile.projections().is_empty() && facade_plan.is_none() {
        return Err(request::drift("generated profile bindings plan"));
    }
    let facade = facade_plan
        .map(|bytes| {
            crate::surface::facade::Plan::decode(bytes)
                .map_err(|_| request::drift("generated profile bindings plan"))
        })
        .transpose()?;
    output
        .write_all(
            b"static GENERATED_MANIFEST: &[u8] = include_bytes!(concat!(env!(\"OUT_DIR\"), \"/rawr-generated-manifest.bin\"));\n",
        )
        .map_err(request::unwritable)?;
    // The per-generation stamps change with every demand change; their own
    // module keeps `src/main.rs` byte-identical across such edits.
    let mut stamps = Vec::new();
    stamps
        .write_all(b"pub(super) static COMPILED_SURFACE: [u8; 32] = ")
        .map_err(request::unwritable)?;
    byte_array(&mut stamps, surface.as_bytes())?;
    stamps
        .write_all(b";\npub(super) static PROFILE_FINGERPRINT: [u8; 32] = ")
        .map_err(request::unwritable)?;
    byte_array(&mut stamps, profile.owner_fingerprint())?;
    writeln!(
        stamps,
        ";\npub(super) static COMPILE_SNAPSHOT: bool = {compile_snapshot};\n\
         #[allow(dead_code)]\npub(super) static UNIVERSE: bool = {};",
        crate::universe::enabled()
    )
    .map_err(request::unwritable)?;
    // An operator needs no import in Rust: its trait is in scope for every
    // adapter, which calls the impl's method (`receiver.sub(argument)`).
    writeln!(
        stamps,
        "#[allow(unused_imports)]\npub(super) use ::core::ops::{{{}}};",
        OPERATOR_TRAITS
            .iter()
            .map(|name| format!("{name} as _"))
            .collect::<Vec<_>>()
            .join(", ")
    )
    .map_err(request::unwritable)?;
    // std's traits outside the prelude whose members a crate type's impl
    // gives the program, as `Coords::from_str` of `"1,2".parse()` (#709)
    // and the `deref` a method probe takes through a crate type's `Deref`
    // (#762).
    writeln!(
        stamps,
        "#[allow(unused_imports)]\npub(super) use ::core::str::FromStr as _;\n\
         #[allow(unused_imports)]\npub(super) use ::core::hash::Hash as _;\n\
         #[allow(unused_imports)]\npub(super) use ::core::ops::{{Deref as _, DerefMut as _}};"
    )
    .map_err(request::unwritable)?;
    // Each trait the program names is in scope for every adapter, so a
    // method it provides resolves as it does in the program.
    let _type_paths = TypePaths::install(facade.as_ref());
    // Every adapter that reaches a region's value of an opaque type names
    // how the region holds it (`family`), so each is declared first: a
    // call on what another such call gives is declared once that one is.
    for plan in plans {
        record_bounds(plan)?;
    }
    let mut boxed = Vec::new();
    loop {
        let mut declared = false;
        for plan in plans {
            if let Some(ty) = declare_wrapper(plan)? {
                declared = true;
                if scoped_type(&ty).is_some_and(|key| family::Ops::held(&key).is_some()) {
                    boxed.push(ty);
                }
            }
        }
        if !declared {
            break;
        }
    }
    for ty in &boxed {
        declare_ops(plans, ty)?;
    }
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
                        let Some(found) = hidden_crate(lock, paths, &segment) else {
                            continue;
                        };
                        hidden.push(found);
                    }
                    writeln!(
                        stamps,
                        "#[allow(unused_imports, deprecated)]\npub(super) use ::{segment}::{rest} as _;"
                    )
                    .map_err(request::unwritable)?;
                    continue;
                }
                writeln!(
                    stamps,
                    "#[allow(unused_imports, deprecated)]\npub(super) use {}::{path} as _;",
                    facade.root()
                )
                .map_err(request::unwritable)?;
            }
        }
    }
    output
        .write_all(
            b"mod rawr_surface;\nuse rawr_surface::{COMPILED_SURFACE, COMPILE_SNAPSHOT};\n#[allow(unused_imports)]\nuse rawr_surface::*;\nmod rawr_families;\n#[allow(unused_imports)]\nuse rawr_families::*;\n",
        )
        .map_err(request::unwritable)?;
    // The facade plan is megabytes of data: rustc reads it with
    // `include_bytes!` instead of parsing it as an integer array literal.
    output
        .write_all(
            b"static IMPORTED_BINDINGS: &[u8] = include_bytes!(\"imported_bindings.bin\");\n",
        )
        .map_err(request::unwritable)?;
    let embedded = match &facade {
        Some(plan) => plan.runner_bytes().map_err(request::unwritable)?,
        None => Vec::new(),
    };
    sources.push((BINDINGS_PATH.into(), embedded));
    output
        .write_all(RUNTIME_SOURCE)
        .map_err(request::unwritable)?;
    output
        .write_all(TOP_LEVEL_SOURCE)
        .map_err(request::unwritable)?;
    let profile_source = if profile.has_generated_projections() {
        profile.generated_source()
    } else {
        NEUTRAL_PROFILE_SOURCE
    };
    output
        .write_all(profile_source)
        .map_err(request::unwritable)?;
    render_profile_projection_bindings(profile, facade.as_ref(), output)?;
    let mut modules = std::collections::BTreeMap::<String, Vec<usize>>::new();
    for (index, plan) in plans.iter().enumerate() {
        modules.entry(adapter_module(plan)).or_default().push(index);
    }
    output
        .write_all(b"\nstatic ADAPTER_MODULES: &[&[Adapter]] = &[\n")
        .map_err(request::unwritable)?;
    for module in modules.keys() {
        writeln!(output, "    {module}::ADAPTERS,").map_err(request::unwritable)?;
    }
    output
        .write_all(
            b"];\n\nfn adapters() -> impl Iterator<Item = &'static Adapter> {\n    ADAPTER_MODULES.iter().flat_map(|rows| rows.iter())\n}\n\n",
        )
        .map_err(request::unwritable)?;
    render_imported_defaults(facade.as_ref(), output)?;
    output
        .write_all(
            b"static STRUCTURAL_ROWS: &[__rawr_internal::package::import::runner::StructuralRow] = &[\n",
        )
        .map_err(request::unwritable)?;
    // Structural rows bind to installed facades; without a facade plan there
    // are none, so no row could be admitted.
    let mut written = std::collections::BTreeSet::new();
    for plan in plans.iter().filter(|_| facade_plan.is_some()) {
        let Some(kind) = structural_kind(plan) else {
            continue;
        };
        let selector = plan
            .selector()
            .ok_or_else(|| request::drift("generated structural selector"))?;
        // One type a program names at two paths, as a signature names
        // `iced::keyboard::Event` and a closure's inferred type the
        // `iced_core` one, is one type of the facade: its rows bind once,
        // at the first path (D536.1). A field's or variant's selector names
        // it within its type, so two types' `standard` fields are two rows.
        let definition = plan
            .definition()
            .ok_or_else(|| request::drift("generated structural definition"))?;
        let instance = crate::surface::instance::key(plan.structural_instance());
        if !written.insert((
            plan.export().root_alias(),
            definition,
            selector.to_owned(),
            kind.clone(),
            instance,
        )) {
            continue;
        }
        write!(
            output,
            "    __rawr_internal::package::import::runner::StructuralRow::new({:?}, {:?}, ",
            plan.export().root_alias(),
            structural_owner(plan),
        )
        .map_err(request::unwritable)?;
        byte_array(output, selector)?;
        output.write_all(b", ").map_err(request::unwritable)?;
        byte_array(output, plan.identity().as_bytes())?;
        output.write_all(b", ").map_err(request::unwritable)?;
        byte_array(output, &signature(plan)?)?;
        write!(output, ", {}, {kind})", arguments(plan)?).map_err(request::unwritable)?;
        render_structural_instance(plan, output)?;
        for matcher in matchers(plan) {
            write!(
                output,
                "    __rawr_internal::package::import::runner::StructuralRow::new({:?}, {:?}, ",
                plan.export().root_alias(),
                structural_owner(plan),
            )
            .map_err(request::unwritable)?;
            byte_array(output, selector)?;
            output.write_all(b", ").map_err(request::unwritable)?;
            byte_array(output, &matcher.identity(plan))?;
            output.write_all(b", ").map_err(request::unwritable)?;
            byte_array(output, &signature(plan)?)?;
            writeln!(
                output,
                ", 1, __rawr_internal::package::import::runner::StructuralKind::{})",
                matcher.kind()
            )
            .map_err(request::unwritable)?;
            render_structural_instance(plan, output)?;
        }
        if has_store(plan) {
            write!(
                output,
                "    __rawr_internal::package::import::runner::StructuralRow::new({:?}, {:?}, ",
                plan.export().root_alias(),
                structural_owner(plan),
            )
            .map_err(request::unwritable)?;
            byte_array(output, selector)?;
            output.write_all(b", ").map_err(request::unwritable)?;
            byte_array(output, &store_identity(plan))?;
            output.write_all(b", ").map_err(request::unwritable)?;
            byte_array(output, &signature(plan)?)?;
            writeln!(
                output,
                ", 2, __rawr_internal::package::import::runner::StructuralKind::Store)"
            )
            .map_err(request::unwritable)?;
            render_structural_instance(plan, output)?;
        }
        if has_lender(plan) {
            write!(
                output,
                "    __rawr_internal::package::import::runner::StructuralRow::new({:?}, {:?}, ",
                plan.export().root_alias(),
                structural_owner(plan),
            )
            .map_err(request::unwritable)?;
            byte_array(output, selector)?;
            output.write_all(b", ").map_err(request::unwritable)?;
            byte_array(output, &lender_identity(plan))?;
            output.write_all(b", ").map_err(request::unwritable)?;
            byte_array(output, &signature(plan)?)?;
            writeln!(
                output,
                ", 1, __rawr_internal::package::import::runner::StructuralKind::Borrow {{ field: 0 }})"
            )
            .map_err(request::unwritable)?;
            render_structural_instance(plan, output)?;
        }
    }
    output.write_all(b"];\n\n").map_err(request::unwritable)?;
    render_execution_tests(modules.keys(), output)?;
    let runner = if profile.owns_product_lifecycle() {
        LIFECYCLE_PROFILE_RUNNER_MAIN
    } else if profile.has_generated_projections() {
        PROFILE_RUNNER_MAIN
    } else {
        RUNNER_MAIN
    };
    output.write_all(runner).map_err(request::unwritable)?;
    output.write_all(b"\n").map_err(request::unwritable)?;
    for module in modules.keys() {
        writeln!(output, "mod {module};").map_err(request::unwritable)?;
    }
    output
        .write_all(b"mod rawr_applications;\n")
        .map_err(request::unwritable)?;
    output
        .write_all(b"include!(\"rawr_impls.rs\");\n")
        .map_err(request::unwritable)?;
    sources.push((SOURCE_PATH.into(), ROOT_SOURCE.to_vec()));
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
            .map_err(request::unwritable)?;
        for index in indices {
            let plan = &plans[*index];
            write!(file, "    Adapter {{ identity: ").map_err(request::unwritable)?;
            byte_array(&mut file, plan.identity().as_bytes())?;
            write!(file, ", signature: ").map_err(request::unwritable)?;
            byte_array(&mut file, &signature(plan)?)?;
            write!(file, ", selector: ").map_err(request::unwritable)?;
            byte_array(&mut file, plan.selector().unwrap_or(&[0; 32]))?;
            write!(file, ", instance: ").map_err(request::unwritable)?;
            byte_array(&mut file, &instance_key(plan))?;
            write!(
                file,
                ", item: {:?}, bound: {}, application: {}, root: {:?}, witness: {}",
                facade_item_path(plan),
                plan.selector().is_some(),
                plan.application_invocation().is_some(),
                plan.export().root_alias(),
                witness_mode(plan) | if returns_values(plan) { 0x80 } else { 0 }
            )
            .map_err(request::unwritable)?;
            writeln!(
                file,
                ", arguments: {}, invoke: {} }},",
                arguments(plan)?,
                adapter_name(plan)
            )
            .map_err(request::unwritable)?;
            for matcher in matchers(plan) {
                write!(file, "    Adapter {{ identity: ").map_err(request::unwritable)?;
                byte_array(&mut file, &matcher.identity(plan))?;
                write!(file, ", signature: ").map_err(request::unwritable)?;
                byte_array(&mut file, &signature(plan)?)?;
                write!(file, ", selector: ").map_err(request::unwritable)?;
                byte_array(&mut file, plan.selector().unwrap_or(&[0; 32]))?;
                write!(file, ", instance: ").map_err(request::unwritable)?;
                byte_array(&mut file, &[0; 32])?;
                // A matcher gives new values, which take its output's
                // witness: a field's reader gives the field's value alone.
                writeln!(
                    file,
                    ", item: \"\", bound: false, application: false, root: {:?}, witness: 1, arguments: 1, invoke: {} }},",
                    plan.export().root_alias(),
                    matcher.name(plan)
                )
                .map_err(request::unwritable)?;
            }
            if has_store(plan) {
                write!(file, "    Adapter {{ identity: ").map_err(request::unwritable)?;
                byte_array(&mut file, &store_identity(plan))?;
                write!(file, ", signature: ").map_err(request::unwritable)?;
                byte_array(&mut file, &signature(plan)?)?;
                write!(file, ", selector: ").map_err(request::unwritable)?;
                byte_array(&mut file, plan.selector().unwrap_or(&[0; 32]))?;
                write!(file, ", instance: ").map_err(request::unwritable)?;
                byte_array(&mut file, &[0; 32])?;
                writeln!(
                    file,
                    ", item: \"\", bound: false, application: false, root: {:?}, witness: 0, arguments: 2, invoke: {} }},",
                    plan.export().root_alias(),
                    store_name(plan)
                )
                .map_err(request::unwritable)?;
            }
            if has_lender(plan) {
                write!(file, "    Adapter {{ identity: ").map_err(request::unwritable)?;
                byte_array(&mut file, &lender_identity(plan))?;
                write!(file, ", signature: ").map_err(request::unwritable)?;
                byte_array(&mut file, &signature(plan)?)?;
                write!(file, ", selector: ").map_err(request::unwritable)?;
                byte_array(&mut file, plan.selector().unwrap_or(&[0; 32]))?;
                write!(file, ", instance: ").map_err(request::unwritable)?;
                byte_array(&mut file, &[0; 32])?;
                writeln!(
                    file,
                    ", item: \"\", bound: false, application: false, root: {:?}, witness: 0, arguments: 1, invoke: {} }},",
                    plan.export().root_alias(),
                    lender_name(plan)
                )
                .map_err(request::unwritable)?;
            }
        }
        file.write_all(b"];\n\n").map_err(request::unwritable)?;
        let path: Box<str> = format!("src/{module}.rs").into();
        for index in indices {
            let plan = &plans[*index];
            let start = span_offset(&file)?;
            render_adapter(plan, plans, profile, facade.as_ref(), &mut file)?;
            for matcher in matchers(plan) {
                render_matcher(plan, matcher, &mut file)?;
            }
            render_store(plan, &mut file)?;
            render_field_lender(plan, &mut file)?;
            let end = span_offset(&file)?;
            spans.push((plan.identity(), path.clone(), start..end));
        }
        render_module_execution_tests(plans, indices, primitive, builder, &mut file)?;
        sources.push((path, file));
    }
    let mut applications = Vec::new();
    render_applications(plans, &mut applications)?;
    sources.push((APPLICATIONS_PATH.into(), applications));
    // The impls name the proxies of their owners, and the families and
    // names the proxies take (`family::named`), so they come first.
    let mut impls = Vec::new();
    render_impls(plans, profile, facade.as_ref(), &mut impls)?;
    place::render(&mut impls);
    let mut families = Vec::new();
    render_families(&mut families)?;
    sources.push((FAMILIES_PATH.into(), families));
    let mut proxies = Vec::new();
    render_proxies(&mut proxies)?;
    // In path order, as the sources are framed.
    sources.push((IMPLS_PATH.into(), impls));
    sources.push((PROXIES_PATH.into(), proxies));
    sources.push((RUNNER_PATH.into(), main));
    sources.push((SURFACE_PATH.into(), stamps));
    // Each crate a type is named through (`hidden_spelling`), which the
    // runner depends on as it does a macro's `$crate`.
    for (path, source) in sources.iter() {
        if **path == *BINDINGS_PATH {
            continue;
        }
        for segment in hidden_segments(source) {
            if !hidden.iter().any(|known| known.segment == segment)
                && let Some(found) = hidden_crate(lock, paths, &segment)
            {
                hidden.push(found);
            }
        }
    }
    Ok(PROXY_CRATE.with_borrow(Clone::clone))
}

/// `path`, a root's path through a crate it reaches only as a macro's
/// `$crate` does (`__rawr_crate_<name>_<id>`), from that crate, which the
/// runner depends on under the segment's name: `::__rawr_crate_..::rest`.
fn hidden_spelling(path: &str) -> Option<String> {
    let segments = path.split("::").collect::<Vec<_>>();
    let at = segments
        .iter()
        .position(|segment| segment.starts_with(crate::api::ledger::CRATE_SEGMENT))?;
    (at + 1 < segments.len()).then(|| format!("::{}", segments[at..].join("::")))
}

/// Each `::__rawr_crate_<name>_<id>` crate a generated source names, once.
fn hidden_segments(source: &[u8]) -> std::collections::BTreeSet<String> {
    let marker = format!("::{}", crate::api::ledger::CRATE_SEGMENT);
    let mut found = std::collections::BTreeSet::new();
    let Ok(text) = std::str::from_utf8(source) else {
        return found;
    };
    for (at, _) in text.match_indices(&marker) {
        let segment = text[at + 2..]
            .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
            .next()
            .unwrap_or_default();
        found.insert(segment.to_owned());
    }
    found
}

/// The path of the facade item the adapter is for, as the runtime's facade
/// names it: from the crate's root, with a method after the type it is on,
/// as `events::widgets::Row::boxed`. A trait's method has one selector on every type,
/// so the runtime binds the instance of each type's item by this path.
fn facade_item_path(plan: &Plan) -> String {
    let mut segments = std::iter::once(plan.export().root_alias())
        .chain(plan.export().path())
        .collect::<Vec<_>>();
    if matches!(plan.operation(), Operation::Method { .. }) && segments.last() != Some(&plan.name())
    {
        segments.push(plan.name());
    }
    segments.join("::")
}

/// The key a call of this adapter's instance carries, or zeros for an
/// item that is not generic.
fn instance_key(plan: &Plan) -> [u8; 32] {
    match plan.operation() {
        Operation::Function {
            substitutions,
            constants,
            slots,
            ..
        }
        | Operation::Method {
            substitutions,
            constants,
            slots,
            ..
        } if !substitutions.is_empty() || !constants.is_empty() || !slots.is_empty() => {
            crate::surface::instance::key_with_constants(substitutions, constants, slots)
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
/// The runner itself, the crate root's module `__rawr_runner`, which holds
/// every item of the runner's own and the modules it declares: a program
/// names its types and modules as it likes, and none meets a runner item
/// at the crate root, where the proxies are (#1163).
const RUNNER_PATH: &str = "src/rawr_runner.rs";
/// The runner's crate root: the proxies and the runner's module, whose
/// name no program spells. The runner's own support is written whole, and
/// each runner uses the part its program needs, so what it leaves unused
/// is no finding.
const ROOT_SOURCE: &[u8] = b"#![forbid(unsafe_code)]\n\
include!(\"rawr_proxies.rs\");\n\
#[allow(dead_code)]\n\
#[path = \"rawr_runner.rs\"]\n\
mod __rawr_runner;\n\
\n\
fn main() {\n    __rawr_runner::main()\n}\n";
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
    let encoding = request::unwritable;
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
        let arguments = String::from_utf8(arguments).map_err(request::unwritable)?;
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
        .map_err(request::unwritable)?;
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
    let encoding = request::unwritable;
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
const BINDINGS_PATH: &str = "src/imported_bindings.bin";

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
    let drift = || request::drift("generated source framing");
    if take(&mut bytes, SOURCES_MAGIC.len()) != Some(SOURCES_MAGIC.as_slice()) {
        return Err(drift());
    }
    let mut sources = Vec::new();
    while !bytes.is_empty() {
        let path = length(&mut bytes)
            .and_then(|path| take(&mut bytes, path))
            .and_then(|path| std::str::from_utf8(path).ok())
            .ok_or_else(|| request::drift("generated source framing"))?;
        let source = length(&mut bytes)
            .and_then(|source| take(&mut bytes, source))
            .ok_or_else(|| request::drift("generated source framing"))?;
        if sources
            .last()
            .is_some_and(|(previous, _): &(&str, &[u8])| *previous >= path)
            || !(path == BINDINGS_PATH
                || path.starts_with("src/")
                    && path.ends_with(".rs")
                    && !path[4..path.len() - 3].contains(['/', '\\', '.']))
        {
            return Err(request::drift("generated source paths"));
        }
        sources.push((path, source));
    }
    if !sources.iter().any(|(path, _)| *path == SOURCE_PATH) {
        return Err(request::drift("generated source paths"));
    }
    Ok(sources)
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
        .map_err(request::unwritable)?;
    if profile.projections().is_empty() {
        output
            .write_all(b"    let _ = (definitions, builders);\n    Ok(())\n}\n")
            .map_err(request::unwritable)?;
        return Ok(());
    }
    let facade = facade.ok_or_else(|| request::drift("generated projection bindings plan"))?;
    let mut support_attestation = false;
    for projection in profile.projections() {
        let source = profile
            .source_for_projection(projection)
            .ok_or_else(|| request::drift("generated projection source site"))?;
        let mut targets = profile
            .alias_targets()
            .iter()
            .filter(|alias| alias.selector() == projection.alias_selector());
        let target = targets
            .next()
            .ok_or_else(|| request::drift("generated projection alias target"))?;
        if targets.next().is_some() {
            return Err(request::drift("generated projection alias target"));
        }
        let mut sites = facade
            .aliases()
            .filter(|site| site.selector() == target.site_selector());
        let site = sites
            .next()
            .ok_or_else(|| request::drift("generated projection alias site"))?;
        if sites.next().is_some()
            || site.root() != target.root()
            || site.path() != target.path()
            || site.export_identity() != target.export_identity()
            || site.alias_identity() != target.alias_identity()
            || site.target_identity() != target.target_identity()
        {
            return Err(request::drift("generated projection alias site"));
        }
        let runtime_source = facade.facades().any(|owner| {
            owner.root() == source.root() && owner.types().any(|ty| ty.path() == source.path())
        });
        if !runtime_source {
            if support_attestation {
                return Err(request::drift(
                    "generated support projection attestation cardinality",
                ));
            }
            support_attestation = true;
            writeln!(
                output,
                "    let owner = definitions.iter().position(|definition| definition.root() == {:?})\n        .ok_or_else(|| __rawr_internal::host::runtime::Error::profile(\"generated support projection owner\"))?;",
                target.root(),
            )
            .map_err(request::unwritable)?;
            writeln!(
                output,
                "    builders.get_mut(owner)\n        .ok_or_else(|| __rawr_internal::host::runtime::Error::profile(\"generated support projection builder\"))?\n        .bind_type_attestation({:?}, |value, expected| {{\n            native_attestation(value, expected)\n                .or_else(|| __rawr_profile::attest_support_endpoint(value, expected))\n        }})?;",
                format!("{}::{}", target.root(), target.path()),
            )
            .map_err(request::unwritable)?;
            continue;
        }
        writeln!(
            output,
            "    let owner = definitions.iter().position(|definition| definition.root() == {:?})\n        .ok_or_else(|| __rawr_internal::host::runtime::Error::profile(\"generated projection owner\"))?;",
            source.root(),
        )
        .map_err(request::unwritable)?;
        writeln!(
            output,
            "    builders.get_mut(owner)\n        .ok_or_else(|| __rawr_internal::host::runtime::Error::profile(\"generated projection builder\"))?\n        .bind_projection_with_context({:?}, {:?}, runtime_project)?;",
            format!("{}::{}", source.root(), source.path()),
            format!("{}::{}", target.root(), target.path()),
        )
        .map_err(request::unwritable)?;
    }
    output
        .write_all(b"    Ok(())\n}\n")
        .map_err(request::unwritable)
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
            instantiated(signature.output(), signature, substitutions)
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
/// Whether `signature` is a trait's method of its impl for `&str`, which
/// Rawr calls on its string (`surface::text`).
fn str_member(signature: &crate::classify::representability::signature::Signature) -> bool {
    matches!(
        signature.instance(),
        Some(convert::Type::Builtin(
            convert::Builtin::Str | convert::Builtin::StaticStr
        ))
    )
}

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
        } if str_member(signature) => 1,
        Operation::StructField { field, .. } if !shared_field(field.ty()) => 2,
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
        convert::Type::Unit | convert::Type::Never => true,
        convert::Type::Builtin(builtin) => builtin.rust_name().is_some(),
        convert::Type::Nominal { definition, .. } => {
            COPY_TYPES.with_borrow(|copy| copy.contains(definition.as_bytes()))
        }
        convert::Type::Option(inner) => copy_scalar(inner),
        convert::Type::Result(ok, err) => copy_scalar(ok) && copy_scalar(err),
        _ => false,
    }
}

/// Whether a struct's field of type `ty` is Copy: as `copy_scalar`, a
/// shared borrow, as iced's `LineDash::segments: &'a [f32]` is, and an
/// array of a length of Copy elements, as `SolidVertex2D::position: [f32;
/// 2]` is.
fn copy_field(ty: &convert::Type) -> bool {
    copy_scalar(ty)
        || matches!(ty, convert::Type::Borrowed { mutable: false, .. })
        || matches!(ty, convert::Type::Sequence { element, length: Some(_), .. } if copy_scalar(element))
}

/// Whether reading a struct's field of type `ty` leaves the struct whole:
/// a Copy field, or one Rawr holds as its own value (`plain_value`). The
/// runner's `shared_field` and the compiler's field read agree with it.
fn shared_field(ty: &convert::Type) -> bool {
    copy_field(ty) || plain_value(ty)
}

/// Whether `ty` holds no native value: a number, `bool`, `char`, `String`
/// or the unit, alone or in an `Option`, a `Result`, a `Vec`, an array or a
/// tuple. Rawr holds such a value as its own, so a field of it is read as
/// Rust reads it, in place.
fn plain_value(ty: &convert::Type) -> bool {
    match ty {
        convert::Type::Unit | convert::Type::Never => true,
        convert::Type::Builtin(builtin) => {
            builtin.rust_name().is_some() || *builtin == convert::Builtin::String
        }
        convert::Type::Option(inner) | convert::Type::Sequence { element: inner, .. } => {
            plain_value(inner)
        }
        convert::Type::Result(ok, err) => plain_value(ok) && plain_value(err),
        convert::Type::Tuple(members) => members.iter().all(plain_value),
        // One of std's types Rawr's std holds as its own, as `PathBuf`.
        convert::Type::Nominal { definition, .. } => {
            crate::surface::identity::Definition::std_clone(definition.as_bytes())
        }
        _ => false,
    }
}

/// How one of a variant's matchers gives the fields of a value of the
/// variant (`matchers`).
#[derive(Clone, Copy, Eq, PartialEq)]
enum Matcher {
    /// `Some` of the fields, copied, when the value is the variant, else
    /// `None`.
    Copy,
    /// `Some(())` when the value is the variant, else `None`: a pattern's
    /// test, which reads no field.
    Test,
    /// The fields, moved out of the value, as a by-value pattern binds
    /// them (`render_moved_fields`).
    Take,
    /// The fields, lent for the rest of the scope that binds them, as a
    /// pattern through `&T` binds them (`render_lent_fields`).
    Lend,
    /// The fields, lent mutably for the rest of the scope that binds them,
    /// as a pattern through `&mut T` or a `ref mut` binding binds them
    /// (`render_lent_fields`).
    LendMut,
    /// The field at the index, copied, as a binding of a `Copy` field or of
    /// a value Rawr holds as its own reads it.
    Read(u32),
    /// The field at the index, lent for the rest of the scope that reads
    /// it, as a pattern below the variant's tests and binds it
    /// (`render_borrowed_field`).
    Borrow(u32),
}

impl Matcher {
    /// The entry identity: the variant constructor's, tagged.
    fn identity(self, plan: &Plan) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(match self {
            Self::Copy => b"rawr-variant-matcher".as_slice(),
            Self::Test => b"rawr-variant-test",
            Self::Take => b"rawr-variant-take",
            Self::Lend => b"rawr-variant-lend",
            Self::LendMut => b"rawr-variant-lend-mut",
            Self::Read(_) => b"rawr-variant-read",
            Self::Borrow(_) => b"rawr-variant-borrow",
        });
        if let Self::Read(field) | Self::Borrow(field) = self {
            hasher.update(field.to_le_bytes());
        }
        hasher.update(plan.identity().as_bytes());
        hasher.finalize().into()
    }

    fn name(self, plan: &Plan) -> String {
        let suffix = match self {
            Self::Copy => "match".to_owned(),
            Self::Test => "test".to_owned(),
            Self::Take => "take".to_owned(),
            Self::Lend => "lend".to_owned(),
            Self::LendMut => "lend_mut".to_owned(),
            Self::Read(field) => format!("read_{field}"),
            Self::Borrow(field) => format!("borrow_{field}"),
        };
        format!("{}_{suffix}", adapter_name(plan))
    }

    /// The runner's `StructuralKind` variant.
    fn kind(self) -> String {
        match self {
            Self::Copy => "Match".to_owned(),
            Self::Test => "Test".to_owned(),
            Self::Take => "Take".to_owned(),
            Self::Lend => "Lend".to_owned(),
            Self::LendMut => "LendMut".to_owned(),
            Self::Read(field) => format!("Read {{ field: {field} }}"),
            Self::Borrow(field) => format!("Borrow {{ field: {field} }}"),
        }
    }
}

/// The matchers of a variant of a non-generic enum, or of a generic one at
/// its instance: one that copies its fields and its test, and, with
/// fields, one that moves them, one that lends them, one that lends them
/// mutably, and for each field one that copies it and one that lends it.
/// The set depends on the plan alone, so
/// the runner's manifest and its rows name the same adapters; how each
/// reaches the fields is decided where it is rendered (`render_matcher`).
fn matchers(plan: &Plan) -> Vec<Matcher> {
    let Operation::EnumVariant {
        parameters,
        fields,
        instance,
        ..
    } = plan.operation()
    else {
        return Vec::new();
    };
    // A generic enum's matchers are its instance's, as a match on iced's
    // `Key<&str>` reads it.
    if !parameters.is_empty() && instance.is_empty() {
        return Vec::new();
    }
    if fields.is_empty() {
        return vec![Matcher::Copy, Matcher::Test];
    }
    let mut matchers = vec![
        Matcher::Copy,
        Matcher::Test,
        Matcher::Take,
        Matcher::Lend,
        Matcher::LendMut,
    ];
    for field in (0..fields.len()).filter_map(|index| u32::try_from(index).ok()) {
        matchers.extend([Matcher::Read(field), Matcher::Borrow(field)]);
    }
    matchers
}

/// Whether the value of the variant `plan` builds is held where its fields
/// can be moved or lent: in an arena slot, or lent by native code, not in a
/// scope frame's region with a lifetime its fields may carry, as a generic
/// enum at an instance that borrows is, as iced's `Key<&str>`.
fn reaches_fields(plan: &Plan) -> bool {
    let path = rust_path(plan);
    !carries_lifetimes(&path) && container_family(&path, &structural_type(plan)).is_none()
}

/// Whether a variant's field of type `ty` is lent where a pattern borrows
/// it: a crate value Rawr doesn't hold as its own, reached by its type.
fn lendable(ty: &convert::Type) -> bool {
    matches!(ty, convert::Type::Nominal { .. })
        && !plain_value(ty)
        && !is_value(ty)
        && !is_duration(ty)
        && !is_facade_value(ty)
        && boxed_element(ty).is_none()
        && counted_element(ty).is_none()
        && keyed_element(ty).is_none()
        && std_value(ty).is_none()
        && place::available(ty)
        && concrete_type(ty).is_some()
}

/// The pattern of the variant `name` of `path` in `form`, binding each of
/// `fields` to `field_{index}`; with no fields, any value of the variant.
fn variant_pattern(
    path: &str,
    name: &str,
    form: crate::api::model::FieldForm,
    fields: &[crate::surface::adapter::Field],
) -> String {
    let bound = (0..fields.len()).map(|index| format!("field_{index}"));
    match form {
        crate::api::model::FieldForm::Unit => format!("{path}::{name}"),
        crate::api::model::FieldForm::Tuple if fields.is_empty() => format!("{path}::{name}(..)"),
        crate::api::model::FieldForm::Record if fields.is_empty() => {
            format!("{path}::{name} {{ .. }}")
        }
        crate::api::model::FieldForm::Tuple => {
            format!("{path}::{name}({})", bound.collect::<Vec<_>>().join(", "))
        }
        crate::api::model::FieldForm::Record => format!(
            "{path}::{name} {{ {} }}",
            fields
                .iter()
                .zip(bound)
                .map(|(field, local)| format!("{}: {local}", field.name()))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// The pattern of the variant `name` of `path` in `form`, binding the field
/// at `index` of `fields` to `field_0` and no other.
fn field_pattern(
    path: &str,
    name: &str,
    form: crate::api::model::FieldForm,
    fields: &[crate::surface::adapter::Field],
    index: usize,
) -> String {
    match (form, fields.get(index)) {
        (crate::api::model::FieldForm::Tuple, Some(_)) => {
            format!("{path}::{name}({}field_0, ..)", "_, ".repeat(index))
        }
        (crate::api::model::FieldForm::Record, Some(field)) => {
            format!("{path}::{name} {{ {}: field_0, .. }}", field.name())
        }
        _ => variant_pattern(path, name, form, &[]),
    }
}

/// The matcher `matcher` of a variant: `Some` of the variant's fields,
/// copied, or of `()` for its test, when the value the first argument
/// holds is that variant, otherwise `None`; one field copied; or the
/// variant's fields moved (`render_moved_fields`) or lent
/// (`render_lent_fields`), or one field lent (`render_borrowed_field`).
fn render_matcher(
    plan: &Plan,
    matcher: Matcher,
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    let Operation::EnumVariant {
        name, form, fields, ..
    } = plan.operation()
    else {
        return Ok(());
    };
    let lent = |field: &crate::surface::adapter::Field| lendable(field.ty());
    // Through `&T`, a slice of crate values, as `children: &'static
    // [Block]`, is lent where it is too, and a `&str` beside it is copied
    // (#1291).
    let shared =
        |field: &crate::surface::adapter::Field| lent(field) || lent_elements(field.ty()).is_some();
    let path = rust_path(plan);
    // The value's type, at a generic enum's instance.
    let ty = structural_type(plan);
    let (pattern, fields) = match matcher {
        Matcher::Copy => (variant_pattern(&path, name, *form, fields), &**fields),
        Matcher::Test => (variant_pattern(&path, name, *form, &[]), &[][..]),
        Matcher::Take if reaches_fields(plan) => {
            return render_moved_fields(plan, name, *form, fields, output);
        }
        Matcher::Lend
            if reaches_fields(plan)
                && fields.iter().any(shared)
                && fields
                    .iter()
                    .all(|field| shared_field(field.ty()) || shared(field)) =>
        {
            return render_projected_fields(plan, name, *form, fields, output);
        }
        Matcher::LendMut
            if reaches_fields(plan)
                && fields.iter().any(lent)
                && fields
                    .iter()
                    .all(|field| plain_value(field.ty()) || lent(field)) =>
        {
            return render_lent_fields(plan, name, *form, fields, true, output);
        }
        Matcher::Take | Matcher::Lend | Matcher::LendMut => {
            return render_copied_fields(plan, matcher, output);
        }
        Matcher::Read(field) | Matcher::Borrow(field)
            if reaches_fields(plan)
                && let Some(element) = fields
                    .get(field as usize)
                    .and_then(|field| lent_elements(field.ty())) =>
        {
            return render_lent_field(
                plan,
                matcher,
                name,
                *form,
                fields,
                field as usize,
                element,
                output,
            );
        }
        Matcher::Borrow(field)
            if reaches_fields(plan)
                && fields
                    .get(field as usize)
                    .is_some_and(|field| lendable(field.ty())) =>
        {
            return render_borrowed_field(plan, name, *form, fields, field, output);
        }
        Matcher::Borrow(field) => return render_read_field(plan, field, output),
        Matcher::Read(field) => {
            let index = field as usize;
            let Some(read) = fields.get(index..=index) else {
                return Err(request::drift("a variant field's index"));
            };
            (field_pattern(&path, name, *form, fields, index), read)
        }
    };
    // The copying matcher and its test give `Some` of what they read, and
    // `None` for another variant; a field's reader runs after the
    // pattern's test has chosen the variant, and gives the field itself.
    let reads = matches!(matcher, Matcher::Read(_));
    let missed = if reads {
        "Err(Error::TypeMismatch)"
    } else {
        "Ok(__rawr_internal::option::none())"
    };
    let bound = (0..fields.len())
        .map(|index| format!("field_{index}"))
        .collect::<Vec<_>>();
    // A field Rawr holds as its own value is copied, as Rust reads it in
    // place. A native field is copied by its `Clone`; one without `Clone`
    // can't leave the borrowed value, which rustc says as E0507.
    let copies = fields
        .iter()
        .zip(&bound)
        .map(|(field, local)| {
            // A type with lifetimes, as quick-xml's `Event<'a>`, has fields
            // `copied` can't take, which are `Clone` as Rust reads them.
            if shared_field(field.ty())
                || carries_lifetimes(&path)
                || container_family(&path, &ty).is_some()
            {
                format!("Clone::clone({local})")
            } else {
                format!("guard_copy!({local})?")
            }
        })
        .collect::<Vec<_>>();
    let tuple = match copies.len() {
        0 => "()".to_string(),
        1 => format!("({},)", copies[0]),
        _ => format!("({})", copies.join(", ")),
    };
    let cloned = format!("(|| -> Result<_, Error> {{ Ok({tuple}) }})()");
    writeln!(
        output,
        "fn {}(arguments: &mut Arguments<'_, '_, '_>) -> Result<Dynamic, Error> {{",
        matcher.name(plan)
    )
    .map_err(request::unwritable)?;
    // A value in a scope frame's region (D558 tier 1): its fields that
    // carry the region's lifetime stay there.
    if let Some(family) = container_family(&path, &ty) {
        let reached = family.call(family::Reach::Get, "scoped");
        let found = if reads {
            "values.into_iter().next().ok_or(Error::TypeMismatch)"
        } else {
            "Ok(__rawr_internal::option::some(if values.is_empty() { Dynamic::from(()) } else { Dynamic::from_array(values) }))"
        };
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
            let output = match region.{reached}.map_err(|error| Error::Lent(scoped.name(), error))? {{ {pattern} => Some({cloned}), _ => None }};
            let Some(fields) = output else {{ return {missed}; }};
            let fields = fields?;
            #[allow(unused_mut)]
            let mut values: Vec<Dynamic> = Vec::new();
            #[allow(clippy::let_unit_value)]
            let _ = &fields;{values}
            {found}
        }})
        .map_err(|error| Error::Lent(scoped.name(), error))?;
    }}"
        )
        .map_err(request::unwritable)?;
        // A generic enum at an instance that borrows, as iced's `Key<&str>`
        // from `Key::as_ref`, is only ever a frame's value.
        if !plan.structural_instance().is_empty() && !carries_lifetimes(&path) {
            return output
                .write_all(b"    Err(Error::TypeMismatch)\n}\n\n")
                .map_err(request::unwritable);
        }
    }
    // A value native code lent the script function that matches it, as a
    // canvas program's `update` takes `&Event` (D558.3), is reached through
    // its accessor, and an element of a native slice, as one of a `static
    // BLOCKS: &[Block]` (#1291), where the slice is; any other is in the
    // arena. A lent value of a type with lifetimes, as quick-xml's
    // `Event<'a>`, lends its fields only for the accessor's call, so they
    // can't be given out of it.
    if carries_lifetimes(&path) {
        writeln!(
            output,
            "    let handle = native_handle(arguments, 0, false)?;
    #[allow(unreachable_patterns)]
    let output = arguments.arena.shared::<{ty}, _>(handle, |input| match input {{ {pattern} => Some({cloned}), _ => None }})?;
    let Some(fields) = output else {{ return {missed}; }};
    let fields = fields?;"
        )
        .map_err(request::unwritable)?;
    } else {
        writeln!(
            output,
            "    let place = arguments.values.first().and_then(|value| __rawr_internal::place::get(value));
    #[allow(unreachable_patterns)]
    let output = match (place, lent_value(arguments, 0)) {{
        (Some(place), _) => {{
            lend(arguments.arena, || __rawr_internal::place::reach_as::<{ty}, _>(&place, false, |input| Ok(match input.read() {{ {pattern} => Some({cloned}), _ => None }})))
                .map_err(|error| Error::Value(error.to_string()))?
        }}
        (None, Some(lent)) => {{
            let access: fn(u64, &mut dyn FnMut(&{ty})) -> Result<(), u8> = lent
                .accessor(false)
                .map_err(|error| Error::Lent(lent.name(), error))?;
            let mut output = None;
            access(lent.nonce().bits(), &mut |input| {{
                output = Some(match input {{ {pattern} => Some({cloned}), _ => None }});
            }})
            .map_err(|code| Error::Lent(lent.name(), __rawr_internal::scope::Error::from_code(code)))?;
            output.ok_or(Error::Lent(lent.name(), __rawr_internal::scope::Error::Busy))?
        }}
        (None, None) => {{
            let handle = native_handle(arguments, 0, false)?;
            arguments.arena.shared::<{ty}, _>(handle, |input| match input {{ {pattern} => Some({cloned}), _ => None }})?
        }}
    }};
    let Some(fields) = output else {{ return {missed}; }};
    let fields = fields?;"
        )
        .map_err(request::unwritable)?;
    }
    if fields.is_empty() {
        output
            .write_all(b"    let () = fields;\n    Ok(__rawr_internal::option::some(Dynamic::from(())))\n}\n\n")
            .map_err(request::unwritable)?;
        return Ok(());
    }
    if let ([field], true) = (fields, reads) {
        let mut converted = String::new();
        render_to_dynamic(field.ty(), "fields.0", 0, &mut converted)?;
        return writeln!(output, "    {converted}\n}}\n").map_err(request::unwritable);
    }
    output
        .write_all(b"    let mut values = Vec::new();\n")
        .map_err(request::unwritable)?;
    for (index, field) in fields.iter().enumerate() {
        let mut converted = String::new();
        render_to_dynamic(field.ty(), &format!("fields.{index}"), 0, &mut converted)?;
        writeln!(output, "    values.push(({converted})?);").map_err(request::unwritable)?;
    }
    output
        .write_all(b"    Ok(__rawr_internal::option::some(Dynamic::from_array(values)))\n}\n\n")
        .map_err(request::unwritable)
}

/// The matcher `matcher` of a variant whose value can't give its fields up
/// where they are: a type that carries a lifetime, or a field that is
/// neither Rawr's own value nor lent by its type. It gives the fields as
/// the copying matcher reads them, as `Clone` reads a value with a
/// lifetime. The pattern's test has already chosen the variant.
fn render_copied_fields(
    plan: &Plan,
    matcher: Matcher,
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    writeln!(
        output,
        "fn {}(arguments: &mut Arguments<'_, '_, '_>) -> Result<Dynamic, Error> {{
    let mut copied = {}(arguments)?;
    __rawr_internal::option::take(&mut copied).flatten().ok_or(Error::TypeMismatch)
}}
",
        matcher.name(plan),
        Matcher::Copy.name(plan)
    )
    .map_err(request::unwritable)
}

/// The fields of the variant `name`, moved out of the value the first
/// argument holds, as a by-value pattern binds them: the value leaves its
/// slot, as rustc moves out of it. The pattern's test has already chosen
/// the variant.
fn render_moved_fields(
    plan: &Plan,
    name: &str,
    form: crate::api::model::FieldForm,
    fields: &[crate::surface::adapter::Field],
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    let path = rust_path(plan);
    let ty = structural_type(plan);
    let pattern = variant_pattern(&path, name, form, fields);
    writeln!(
        output,
        "fn {}(arguments: &mut Arguments<'_, '_, '_>) -> Result<Dynamic, Error> {{
    let input: {ty} = take_native(arguments, 0)?;
    #[allow(irrefutable_let_patterns)]
    let {pattern} = input else {{ return Err(Error::TypeMismatch); }};
    let mut values: Vec<Dynamic> = Vec::new();",
        Matcher::Take.name(plan)
    )
    .map_err(request::unwritable)?;
    for (index, field) in fields.iter().enumerate() {
        let mut converted = String::new();
        render_to_dynamic(field.ty(), &format!("field_{index}"), 0, &mut converted)?;
        writeln!(output, "    values.push(({converted})?);").map_err(request::unwritable)?;
    }
    output
        .write_all(b"    Ok(Dynamic::from_array(values))\n}\n\n")
        .map_err(request::unwritable)
}

/// The fields of the variant `name` of the value the first argument holds,
/// lent as a pattern binds them: through `&T` as `render_projected_fields`
/// borrows them, and through `&mut T` or as `ref mut` by a scope frame for
/// the rest of the scope that binds them, which, opened, lends each crate
/// value field mutably by its own type, as native code lends a value
/// (D558.3), and copies each field Rawr holds as its own value, as Rust
/// reads it in place. The value stays where it is while the frame lends
/// its fields. The pattern's test has already chosen the variant.
fn render_lent_fields(
    plan: &Plan,
    name: &str,
    form: crate::api::model::FieldForm,
    fields: &[crate::surface::adapter::Field],
    mutable: bool,
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    if !mutable {
        return render_projected_fields(plan, name, form, fields, output);
    }
    let path = rust_path(plan);
    let ty = structural_type(plan);
    let pattern = variant_pattern(&path, name, form, fields);
    let matcher = Matcher::LendMut;
    writeln!(
        output,
        "fn {}(arguments: &mut Arguments<'_, '_, '_>) -> Result<Dynamic, Error> {{",
        matcher.name(plan)
    )
    .map_err(request::unwritable)?;
    let mut copies = String::new();
    let mut lent = Vec::new();
    for (index, field) in fields.iter().enumerate() {
        let ty = field.ty();
        if !lendable(ty) {
            let mut converted = String::new();
            render_to_dynamic(ty, "copied", 0, &mut converted)?;
            copies.push_str(&format!(
                "\n            let value_{index} = {{ let copied = Clone::clone(&*field_{index}); {converted} }}.map_err(place_error)?;"
            ));
            continue;
        }
        let rust =
            concrete_type(ty).ok_or_else(|| request::drift("a lent field's concrete type"))?;
        render_field_family(index, &rust, mutable, output)?;
        lent.push((index, rust));
    }
    let values = (0..fields.len())
        .map(|index| format!("value_{index}"))
        .collect::<Vec<_>>()
        .join(", ");
    let mut body = format!("rest(Dynamic::from_array(vec![{values}]))");
    for (index, rust) in lent.iter().rev() {
        body = format!(
            "{} {{
                let value_{index} = {};
                {body}
            }})",
            lend_field(*index, mutable),
            lent_field(*index, rust, mutable),
        );
    }
    let reach = if mutable {
        "reach_lender_mut"
    } else {
        "reach_lender"
    };
    writeln!(
        output,
        "    let lender = match lent_value(arguments, 0) {{
        Some(lent) => Lender::Lent(lent),
        None => Lender::Handle(native_handle(arguments, 0, false)?),
    }};
    Ok(Dynamic::from(__rawr_internal::scope::Opening::new({ty:?}, move |rest| {{
        {reach}::<{ty}>(&lender, &mut |input| {{
            #[allow(irrefutable_let_patterns)]
            let {pattern} = input else {{ return Err(place_error(Error::TypeMismatch)); }};{copies}
            {body}
        }})
    }})))
}}
"
    )
    .map_err(request::unwritable)
}

/// The fields of the variant `name` of the value the first argument holds,
/// borrowed as a pattern through `&T` binds them (#782, #783): each crate
/// value field as a projection that reaches it through the value at each
/// use (`render_projection`), so the borrow lives past the arm as rustc's
/// does, and each field Rawr holds as its own value copied, as Rust reads
/// it in place. The pattern's test has already chosen the variant.
fn render_projected_fields(
    plan: &Plan,
    name: &str,
    form: crate::api::model::FieldForm,
    fields: &[crate::surface::adapter::Field],
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    let path = rust_path(plan);
    let ty = structural_type(plan);
    writeln!(
        output,
        "fn {}(arguments: &mut Arguments<'_, '_, '_>) -> Result<Dynamic, Error> {{
    let parent = (**arguments.values.first().ok_or(Error::MissingArgument)?).clone();
    let witness = output_witness();",
        Matcher::Lend.name(plan)
    )
    .map_err(request::unwritable)?;
    let mut values = String::new();
    let mut copied = false;
    for (index, field) in fields.iter().enumerate() {
        let ty = field.ty();
        if let Some(element) = lent_elements(ty) {
            let lent = lent_field_slice(
                plan,
                name,
                form,
                fields,
                index,
                element,
                &format!("witness.as_ref().and_then(|witness| witness.argument_witness({index}))"),
            )?;
            writeln!(output, "    let lent_{index} = {lent}?;").map_err(request::unwritable)?;
            values.push_str(&format!(
                "\n        let value_{index} = lent_{index}.clone();"
            ));
        } else if lendable(ty) {
            render_projection(plan, name, form, fields, index, output)?;
            values.push_str(&format!(
                "\n        let value_{index} = Dynamic::from(__rawr_internal::scope::Projected::new(parent.clone(), &PROJECTION_{index}, witness.as_ref().and_then(|witness| witness.argument_witness({index}))));"
            ));
        } else {
            let mut converted = String::new();
            render_to_dynamic(ty, "copied", 0, &mut converted)?;
            values.push_str(&format!(
                "\n        let value_{index} = {{ let copied = Clone::clone(&*field_{index}); {converted} }}.map_err(place_error)?;"
            ));
            copied = true;
        }
    }
    let array = (0..fields.len())
        .map(|index| format!("value_{index}"))
        .collect::<Vec<_>>()
        .join(", ");
    // Only a copy reads the value now; a projection reads it at its uses.
    if !copied {
        return writeln!(
            output,
            "    {{{values}\n        Ok(Dynamic::from_array(vec![{array}]))\n    }}\n}}\n"
        )
        .map_err(request::unwritable);
    }
    // The copies are read with the call's arena lent, as a projection of
    // the value reaches it.
    let pattern = variant_pattern(&path, name, form, fields);
    writeln!(
        output,
        "    let lender = Lender::of(&parent)?;
    lend(arguments.arena, || reach_lender::<{ty}>(&lender, &mut |input| {{
        #[allow(irrefutable_let_patterns, unused_variables)]
        let {pattern} = input else {{ return Err(place_error(Error::TypeMismatch)); }};{values}
        Ok(Dynamic::from_array(vec![{array}]))
    }}))
    .map_err(unreached)
}}
"
    )
    .map_err(request::unwritable)
}

/// The field at `index` of the variant `name` of the value `parent` holds,
/// a slice of crate values, as `children: &'static [Block]` (#1291): a
/// native place, of the witness `witness` gives, that reaches the field
/// through the value at each operation, as a projection does, so the
/// elements are read where they are and none is copied. An expression of
/// `Result<Dynamic, Error>`.
fn lent_field_slice(
    plan: &Plan,
    name: &str,
    form: crate::api::model::FieldForm,
    fields: &[crate::surface::adapter::Field],
    index: usize,
    element: &convert::Type,
    witness: &str,
) -> Result<String, request::Error> {
    let policy = place::policy(element)?;
    let pattern = field_pattern(&rust_path(plan), name, form, fields, index);
    let ty = structural_type(plan);
    Ok(format!(
        "Lender::of(&parent).and_then(|lender| {{
        let witness = ({witness}).ok_or(Error::Stale)?;
        __rawr_internal::place::slice::lent_by::<{policy}>(&witness, move |visit| {{
            reach_lender::<{ty}>(&lender, &mut |input| {{
                #[allow(irrefutable_let_patterns)]
                let {pattern} = input else {{ return Err(place_error(Error::TypeMismatch)); }};
                visit(*field_0)
            }})
        }})
        .map_err(|error| Error::Value(error.to_string()))
    }})"
    ))
}

/// The field at `index` of the variant `name` of the value the first
/// argument holds, read alone or borrowed as a pattern below the
/// variant's reads it, when it is a slice of crate values
/// (`lent_field_slice`).
#[allow(clippy::too_many_arguments)]
fn render_lent_field(
    plan: &Plan,
    matcher: Matcher,
    name: &str,
    form: crate::api::model::FieldForm,
    fields: &[crate::surface::adapter::Field],
    index: usize,
    element: &convert::Type,
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    let lent = lent_field_slice(plan, name, form, fields, index, element, "output_witness()")?;
    writeln!(
        output,
        "fn {}(arguments: &mut Arguments<'_, '_, '_>) -> Result<Dynamic, Error> {{
    let parent = (**arguments.values.first().ok_or(Error::MissingArgument)?).clone();
    {lent}
}}
",
        matcher.name(plan)
    )
    .map_err(request::unwritable)
}

/// The field at `index` of the variant `name` of the value the first
/// argument holds, borrowed as a pattern below the variant's reads it: a
/// projection, as `render_projected_fields` borrows each field. The
/// pattern's test has already chosen the variant.
fn render_borrowed_field(
    plan: &Plan,
    name: &str,
    form: crate::api::model::FieldForm,
    fields: &[crate::surface::adapter::Field],
    field: u32,
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    let index = field as usize;
    writeln!(
        output,
        "fn {}(arguments: &mut Arguments<'_, '_, '_>) -> Result<Dynamic, Error> {{
    let parent = (**arguments.values.first().ok_or(Error::MissingArgument)?).clone();",
        Matcher::Borrow(field).name(plan)
    )
    .map_err(request::unwritable)?;
    render_projection(plan, name, form, fields, index, output)?;
    writeln!(
        output,
        "    Ok(Dynamic::from(__rawr_internal::scope::Projected::new(parent, &PROJECTION_{index}, output_witness())))
}}
"
    )
    .map_err(request::unwritable)
}

/// The projection `PROJECTION_{index}` of the field at `index` of the
/// variant `name`: it reaches the value the projection holds, shared, and
/// lends the field by its own type for one use, as native code lends a
/// value (D558.3), and gives the field's `Debug` and `Display` text and a
/// clone of it.
fn render_projection(
    plan: &Plan,
    name: &str,
    form: crate::api::model::FieldForm,
    fields: &[crate::surface::adapter::Field],
    index: usize,
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    let path = rust_path(plan);
    // The value's type, at a generic enum's instance.
    let ty = structural_type(plan);
    let rust = fields
        .get(index)
        .and_then(|field| concrete_type(field.ty()))
        .ok_or_else(|| request::drift("a lent field's concrete type"))?;
    let pattern = field_pattern(&path, name, form, fields, index);
    render_field_family(index, &rust, false, output)?;
    writeln!(
        output,
        "    fn open_{index}(
        parent: &Dynamic,
        body: &mut dyn FnMut(__rawr_internal::scope::Lent) -> Result<Dynamic, Box<__rawr_internal::EvalAltResult>>,
    ) -> Result<Dynamic, Box<__rawr_internal::EvalAltResult>> {{
        reach_lender::<{ty}>(&Lender::of(parent).map_err(place_error)?, &mut |input| {{
            #[allow(irrefutable_let_patterns)]
            let {pattern} = input else {{ return Err(place_error(Error::TypeMismatch)); }};
            __rawr_internal::scope::lend_ref::<Field{index}, _>(field_0, |nonce| {{
                body(__rawr_internal::scope::Lent::new(nonce, {rust:?}, None, Some(&SHARED_{index} as __rawr_internal::scope::Accessor)))
            }})
        }})
    }}
    fn text_{index}(parent: &Dynamic, display: bool) -> Option<String> {{
        #[allow(unused_imports)]
        use crate::__rawr_runner::{{ViaDebug as _, ViaDisplay as _, WithoutDebug as _, WithoutDisplay as _}};
        let mut text = None;
        let _ = reach_lender::<{ty}>(&Lender::of(parent).ok()?, &mut |input| {{
            #[allow(irrefutable_let_patterns)]
            let {pattern} = input else {{ return Ok(Dynamic::UNIT); }};
            let format = if display {{
                (&debug_probe(field_0)).native_display()
            }} else {{
                (&debug_probe(field_0)).native_debug()
            }};
            text = format.and_then(|format| format(field_0));
            Ok(Dynamic::UNIT)
        }})
        .ok()?;
        text
    }}
    fn clone_{index}(
        parent: &Dynamic,
        witness: Option<&__rawr_internal::host::runtime::Witness>,
    ) -> Result<Dynamic, Box<__rawr_internal::EvalAltResult>> {{
        let mut copied = None;
        let _ = reach_lender::<{ty}>(&Lender::of(parent).map_err(place_error)?, &mut |input| {{
            #[allow(irrefutable_let_patterns)]
            let {pattern} = input else {{ return Err(place_error(Error::TypeMismatch)); }};
            copied = Some(guard_copy!(field_0));
            Ok(Dynamic::UNIT)
        }})?;
        let copied = copied.ok_or_else(|| place_error(Error::TypeMismatch))?.map_err(place_error)?;
        with_arena(|arena| {{
            let handle = insert_native!(arena, copied)?;
            if let Some(witness) = witness {{
                arena.attest(handle.clone(), witness.clone())?;
            }}
            Ok(Dynamic::from(handle))
        }})
        .map_err(place_error)
    }}
    static PROJECTION_{index}: __rawr_internal::scope::Projection = __rawr_internal::scope::Projection {{
        name: {rust:?},
        open: open_{index},
        text: text_{index},
        clone: clone_{index},
    }};"
    )
    .map_err(request::unwritable)
}

/// The lender of a field that is not lent by its type: the field copied,
/// as its reader copies it.
fn render_read_field(plan: &Plan, field: u32, output: &mut Vec<u8>) -> Result<(), request::Error> {
    writeln!(
        output,
        "fn {}(arguments: &mut Arguments<'_, '_, '_>) -> Result<Dynamic, Error> {{
    {}(arguments)
}}
",
        Matcher::Borrow(field).name(plan),
        Matcher::Read(field).name(plan)
    )
    .map_err(request::unwritable)
}

/// The scope family `Field{index}` of a lent field of the Rust type
/// `rust`, and the accessors its `Lent` reaches it by: shared, and, when
/// it is lent `mutable`, mutable.
fn render_field_family(
    index: usize,
    rust: &str,
    mutable: bool,
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    writeln!(
        output,
        "    struct Field{index};
    impl __rawr_internal::scope::Family for Field{index} {{ type Of<'a> = {rust}; }}
    const SHARED_{index}: fn(u64, &mut dyn FnMut(&{rust})) -> Result<(), u8> = |nonce, visit| __rawr_internal::scope::with_ref::<Field{index}, _>(__rawr_internal::scope::nonce(nonce), |value| visit(value)).map_err(__rawr_internal::scope::Error::code);"
    )
    .map_err(request::unwritable)?;
    if mutable {
        writeln!(
            output,
            "    const MUTABLE_{index}: fn(u64, &mut dyn FnMut(&mut {rust})) -> Result<(), u8> = |nonce, visit| __rawr_internal::scope::with::<Field{index}, _>(__rawr_internal::scope::nonce(nonce), |value| visit(value)).map_err(__rawr_internal::scope::Error::code);"
        )
        .map_err(request::unwritable)?;
    }
    Ok(())
}

/// The start of the lend of `field_{index}`, up to its body's opening
/// brace: `lend` when it is lent `mutable`, `lend_ref` otherwise.
fn lend_field(index: usize, mutable: bool) -> String {
    let lend = if mutable { "lend" } else { "lend_ref" };
    format!("__rawr_internal::scope::{lend}::<Field{index}, _>(field_{index}, |nonce|")
}

/// The script value of the field `lend_field` lends: a `Lent` reached by
/// the accessors `render_field_family` made.
fn lent_field(index: usize, rust: &str, mutable: bool) -> String {
    let writer = if mutable {
        format!("Some(&MUTABLE_{index} as __rawr_internal::scope::Accessor)")
    } else {
        "None".to_owned()
    };
    format!(
        "Dynamic::from(__rawr_internal::scope::Lent::new(nonce, {rust:?}, {writer}, Some(&SHARED_{index} as __rawr_internal::scope::Accessor)))"
    )
}

/// Key a structural row by the same concrete arguments as its adapter.
fn render_structural_instance(plan: &Plan, output: &mut Vec<u8>) -> Result<(), request::Error> {
    let types = plan.structural_instance();
    if !types.is_empty() {
        output
            .write_all(b".with_instance(")
            .map_err(request::unwritable)?;
        byte_array(output, &crate::surface::instance::key(types))?;
        output.write_all(b")").map_err(request::unwritable)?;
    }
    output.write_all(b",\n").map_err(request::unwritable)
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

/// A field of a struct that is not generic has a lender, which `&value.
/// field` reads it by (`render_field_lender`).
fn has_lender(plan: &Plan) -> bool {
    matches!(plan.operation(), Operation::StructField { .. })
        && plan.structural_instance().is_empty()
}

/// The entry identity of a field's lender: its reader's, tagged.
fn lender_identity(plan: &Plan) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"rawr-field-lender");
    hasher.update(plan.identity().as_bytes());
    hasher.finalize().into()
}

fn lender_name(plan: &Plan) -> String {
    format!("{}_lend", adapter_name(plan))
}

/// The field of the value the first argument holds, lent for the rest of
/// the scope that borrows it, as Rust's `&value.field` borrows it: a scope
/// frame that, opened, lends the field by its own type, as a variant's
/// field is lent (`render_borrowed_field`). The value stays where it is,
/// shared, while the frame lends the field. A field its type can't lend
/// is read by the field's reader.
fn render_field_lender(plan: &Plan, output: &mut Vec<u8>) -> Result<(), request::Error> {
    let Operation::StructField { field, .. } = plan.operation() else {
        return Ok(());
    };
    if !has_lender(plan) {
        return Ok(());
    }
    let path = structural_type(plan);
    let name = lender_name(plan);
    if !lendable(field.ty()) || container_family(&path, &path).is_some() || carries_lifetimes(&path)
    {
        return writeln!(
            output,
            "fn {name}(arguments: &mut Arguments<'_, '_, '_>) -> Result<Dynamic, Error> {{
    {}(arguments)
}}
",
            adapter_name(plan)
        )
        .map_err(request::unwritable);
    }
    let rust =
        concrete_type(field.ty()).ok_or_else(|| request::drift("a lent field's concrete type"))?;
    writeln!(
        output,
        "fn {name}(arguments: &mut Arguments<'_, '_, '_>) -> Result<Dynamic, Error> {{"
    )
    .map_err(request::unwritable)?;
    render_field_family(0, &rust, false, output)?;
    writeln!(
        output,
        "    let lender = match lent_value(arguments, 0) {{
        Some(lent) => Lender::Lent(lent),
        None => Lender::Handle(native_handle(arguments, 0, false)?),
    }};
    Ok(Dynamic::from(__rawr_internal::scope::Opening::new({path:?}, move |rest| {{
        reach_lender::<{path}>(&lender, &mut |input| {{
            let field_0 = &input.{};
            {} rest({}))
        }})
    }})))
}}
",
        field.name(),
        lend_field(0, false),
        lent_field(0, &rust, false),
    )
    .map_err(request::unwritable)
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
    .map_err(request::unwritable)?;
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
    .map_err(request::unwritable)
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
            shared_field(field.ty())
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
    output.write_all(b"[").map_err(request::unwritable)?;
    for (index, byte) in bytes.iter().enumerate() {
        if index != 0 {
            output.write_all(b", ").map_err(request::unwritable)?;
        }
        write!(output, "{byte}").map_err(request::unwritable)?;
    }
    output.write_all(b"]").map_err(request::unwritable)
}

/// `body`, a call's body that takes `lent` (`lent_inputs`), run with each
/// argument native code lent reached where it is for the call, as an
/// adapter reaches it, never moved out of its borrowed place (#913).
fn within_lent(lent: &[(usize, bool, String)], body: String) -> String {
    if lent.is_empty() {
        return body;
    }
    let reach = lent
        .iter()
        .map(|(index, mutable, rust)| format!("({index}, {mutable}, {rust} as Reach)"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "    with_lent_arguments(arguments, &[{reach}], &mut |arguments: &mut Arguments<'_, '_, '_>, lent: &mut LentArguments<'_>| -> Result<Dynamic, Error> {{\n    let _ = &mut *lent;\n{body}\n    }})"
    )
}

/// The calls that are plain enough to reach what native code lent where it
/// is (`lent_inputs`): neither a method that sends a message nor a replay.
fn lent_inputs_of(
    plan: &Plan,
    cohort: &[Plan],
    profile: &crate::profile::Proof,
    facade: Option<&crate::surface::facade::Plan>,
) -> Result<Vec<(usize, bool, String)>, request::Error> {
    if method_message(plan, cohort, profile, facade)?.is_none() && replay(plan, profile)?.is_none()
    {
        lent_inputs(plan)
    } else {
        Ok(Vec::new())
    }
}

/// The receiver of an opaque type's method in a region (`family`): its
/// family, and the call of it when the region holds it boxed, through its
/// trait's method.
struct Operand {
    family: family::Family,
    call: Option<String>,
}

/// The call of an opaque type's method `plan`, the `consumer`-th call on
/// the type, on a receiver a region holds (`family`): the value isn't in
/// the arena, where the call that made it set its table. Its type is a
/// wrapper's output, whose bounds rustc knows, or it is boxed, and the call
/// is its trait's method. Empty for another call.
fn region_operand(
    plan: &Plan,
    operand: &convert::Type,
    consumer: usize,
    lent: &[(usize, bool, String)],
) -> Result<String, request::Error> {
    let (signature, substitutions, constants, container_parameters) = match plan.operation() {
        Operation::Method {
            signature,
            substitutions,
            constants,
            container_parameters,
            ..
        }
        | Operation::Function {
            signature,
            substitutions,
            constants,
            container_parameters,
            ..
        } => (signature, substitutions, constants, *container_parameters),
        _ => return Ok(String::new()),
    };
    // The receiver a region holds boxed, by its own type, as a container's
    // method takes `Container<'x, impl Widget<..>>`.
    let boxed = first_operand(plan)?
        .and_then(|(key, receiver)| Some((family::Ops::held(&key)?, key, receiver)));
    let (receiver, operand) = match boxed {
        Some((held, key, receiver)) => {
            let lent = match receiver {
                Receiver::Owned => "receiver",
                Receiver::Shared => "&**receiver",
                Receiver::Mutable => "&mut **receiver",
            };
            let skip = usize::from(matches!(plan.operation(), Operation::Function { .. }));
            let arguments = signature.inputs().len() - skip;
            let (Some(call), Some(family)) = (
                family::Ops::call(&key, consumer, lent, arguments),
                family(held, BOXED_LIFETIMES),
            ) else {
                return Ok(String::new());
            };
            (
                receiver,
                Operand {
                    family,
                    call: Some(call),
                },
            )
        }
        None => match (plan.operation(), family_of(operand)) {
            (Operation::Method { receiver, .. }, Some(family)) if family.named().is_none() => {
                (*receiver, Operand { family, call: None })
            }
            _ => return Ok(String::new()),
        },
    };
    // In the receiver's region, where an argument that carries its lifetime
    // is too, as pane_grid's `content.title_bar(title_bar)` takes one.
    let mut body = Vec::new();
    {
        let _region = InRegion::enter();
        let _arguments = RegionArguments::enter();
        let _lent = LentArguments::enter(!lent.is_empty());
        match plan.operation() {
            Operation::Function { .. } => {
                for (position, input) in signature.inputs().iter().enumerate().skip(1) {
                    render_take_input(
                        position - 1,
                        position,
                        input,
                        signature,
                        substitutions,
                        None,
                        &mut body,
                    )?;
                }
            }
            _ => render_arguments_with(signature, substitutions, 1, &mut body)?,
        }
        render_scoped_receiver(
            plan,
            &rust_path(plan),
            receiver,
            signature,
            substitutions,
            constants,
            container_parameters,
            Some(operand),
            &mut body,
        )?;
    }
    let mut body = String::from_utf8(body).map_err(request::unwritable)?;
    body.push_str("    Err(Error::TypeMismatch)");
    let body = within_lent(lent, body);
    Ok(format!(
        "\n    if let Some(scoped) = scoped_value(arguments, 0) {{
        return __rawr_internal::scope::with::<__rawr_internal::scope::RegionF, _>(scoped.nonce(), |region| -> Result<Dynamic, Error> {{
{body}
        }})
        .map_err(|error| Error::Lent(scoped.name(), error))?;
    }}"
    ))
}

fn render_adapter(
    plan: &Plan,
    cohort: &[Plan],
    profile: &crate::profile::Proof,
    facade: Option<&crate::surface::facade::Plan>,
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    let encoding = request::unwritable;
    let name = adapter_name(plan);
    // A call that takes no value a generic impl function's frame keeps, but
    // names a type it keeps, runs in the innermost such frame (#593).
    if let Some((key, entry)) = frame::static_entry(plan, cohort)? {
        let convert::Type::Nominal { definition, .. } = &key else {
            return Err(request::drift("a frame's opaque type"));
        };
        return writeln!(
            output,
            "fn {name}(arguments: &mut Arguments<'_, '_, '_>) -> Result<Dynamic, Error> {{
    frame_static(&{:?}, {entry}, arguments)
}}
",
            definition.as_bytes()
        )
        .map_err(encoding);
    }
    // A call that takes a value whose type holds an opaque `impl Trait`
    // type runs what the call that made the value compiled for it.
    if let Some((position, key, operand)) = opaque_operand(plan)? {
        let index = opaque_consumers(cohort, &key)?
            .iter()
            .position(|consumer| std::ptr::eq(*consumer, plan))
            .ok_or_else(|| request::drift("an opaque type's consumer"))?;
        // An operand that holds the opaque type inside another, as an
        // `Option` of it, may hold no value of it at all: the table is the
        // one the call that makes such values compiled for its type.
        if !by_handle(&operand, &key)
            && let Some(producer) = opaque_producer(cohort, &key)?
        {
            return writeln!(
                output,
                "fn {name}(arguments: &mut Arguments<'_, '_, '_>) -> Result<Dynamic, Error> {{
    super::{module}::{producer}_opaque().call({index}, arguments)
}}
",
                module = adapter_module(producer),
                producer = adapter_name(producer),
            )
            .map_err(encoding);
        }
        let scoped = if position == 0 {
            let lent = lent_inputs_of(plan, cohort, profile, facade)?;
            region_operand(plan, &operand, index, &lent)?
        } else {
            String::new()
        };
        return writeln!(
            output,
            "fn {name}(arguments: &mut Arguments<'_, '_, '_>) -> Result<Dynamic, Error> {{{scoped}
    let handle = native_handle(arguments, {position}, false)?;
    let table = arguments.arena.opaque(&handle)?;
    table.call({index}, arguments)
}}
"
        )
        .map_err(encoding);
    }
    let table = opaque_table_source(plan, cohort, profile, facade)?;
    let plain = method_message(plan, cohort, profile, facade)?.is_none()
        && replay(plan, profile)?.is_none();
    let lent = if plain {
        lent_inputs(plan)?
    } else {
        Vec::new()
    };
    // The table of a type some call takes inside another type is reached
    // without a value of it (`opaque_producer`): rustc reads the type from
    // this call's body, which is never run there.
    let table = match table {
        Some(table) if witnessed(plan, cohort)? => {
            let inputs = if lent.is_empty() {
                "arguments: &mut Arguments<'_, '_, '_>"
            } else {
                "arguments: &mut Arguments<'_, '_, '_>, lent: &mut LentArguments<'_>"
            };
            writeln!(
                output,
                "pub(super) fn {name}_opaque() -> OpaqueTable {{
    let mut witness = std::marker::PhantomData;
    let _ = |{inputs}| -> Result<Dynamic, Error> {{
    let _ = &mut *arguments;"
            )
            .map_err(encoding)?;
            {
                let _witnessing = OpaqueWitnessing::enter();
                let _lent = LentArguments::enter(!lent.is_empty());
                render_adapter_body(plan, cohort, profile, facade, output)?;
            }
            writeln!(
                output,
                "    }};\n    {}\n}}\n",
                table.replace("__RAWR_WITNESS__", "witness")
            )
            .map_err(encoding)?;
            Some(format!("{name}_opaque()"))
        }
        table => table,
    };
    let _producing = OpaqueProducing::enter(table);
    let region = if plain {
        region_inputs(plan)?
    } else {
        Vec::new()
    };
    let _lent = LentArguments::enter(!lent.is_empty());
    let slices = if lent.is_empty() {
        slice_inputs(plan)?
    } else {
        Vec::new()
    };
    let _slices = SliceArguments::enter(slices.iter().map(|(index, ..)| *index).collect());
    if lent.is_empty() && slices.is_empty() {
        writeln!(
            output,
            "fn {name}(arguments: &mut Arguments<'_, '_, '_>) -> Result<Dynamic, Error> {{"
        )
        .map_err(encoding)?;
    } else if lent.is_empty() {
        // Each `&[T]` argument a native slice place fills is reached where
        // it is for the call, and the call runs inside them all (#1291).
        let names = slices
            .iter()
            .map(|(index, ..)| format!("slice_{index}"))
            .collect::<Vec<_>>()
            .join(", ");
        let parameters = slices
            .iter()
            .map(|(index, _, rust)| format!("slice_{index}: Option<&[{rust}]>"))
            .collect::<Vec<_>>()
            .join(", ");
        let mut call = format!("{name}_sliced(arguments, {names})");
        for (index, policy, rust) in slices.iter().rev() {
            call = format!(
                "with_slice_argument::<{policy}>(arguments, {index}, &mut |arguments: &mut Arguments<'_, '_, '_>, slice_{index}: Option<&[{rust}]>| {call})"
            );
        }
        writeln!(
            output,
            "fn {name}(arguments: &mut Arguments<'_, '_, '_>) -> Result<Dynamic, Error> {{
    {call}
}}

fn {name}_sliced(arguments: &mut Arguments<'_, '_, '_>, {parameters}) -> Result<Dynamic, Error> {{"
        )
        .map_err(encoding)?;
    } else {
        // Each argument native code lent (D558.3) is reached for the call
        // through its accessor, and the call runs inside them all.
        let reach = lent
            .iter()
            .map(|(index, mutable, rust)| format!("({index}, {mutable}, {rust} as Reach)"))
            .collect::<Vec<_>>()
            .join(", ");
        writeln!(
            output,
            "fn {name}(arguments: &mut Arguments<'_, '_, '_>) -> Result<Dynamic, Error> {{
    with_lent_arguments(arguments, &[{reach}], &mut {name}_lent)
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
        // carries the region's lifetime. So does a call a slot's region is
        // open around that takes none, as iced's `hover(base, None)` in a
        // view: what it makes joins what the view makes (#352).
        let indices = region
            .iter()
            .map(usize::to_string)
            .collect::<Vec<_>>()
            .join(", ");
        writeln!(
            output,
            "    if let Some((nonce, name)) = scoped_innermost(arguments, &[{indices}]).map(|scoped| (scoped.nonce(), scoped.name())).or_else(|| __rawr_internal::scope::anchor().map(|anchor| (anchor, \"a slot's region\"))) {{
        return __rawr_internal::scope::with::<__rawr_internal::scope::RegionF, _>(nonce, |region| -> Result<Dynamic, Error> {{"
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
        .map_err(|error| Error::Lent(name, error))?;
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
    // A scope frame's call takes what it borrows into the frame. A future
    // holds what it borrows after the call, as fount's `async fn load(font:
    // &Font)` does: its inputs are leased from their slots instead (D683.2).
    if signature.scope().is_some()
        || is_future(&substituted(signature.output(), signature, substitutions)?)
    {
        return Ok(Vec::new());
    }
    let mut lent = Vec::new();
    for (index, input) in runtime_inputs(signature, offset) {
        let convert::Type::Borrowed { mutable, ty } = input else {
            continue;
        };
        let ty = substituted(ty, signature, substitutions)?;
        if is_copied(&ty) && place::available(&ty) {
            lent.push((
                index,
                *mutable,
                format!("reach_place::<{}>", place::policy(&ty)?),
            ));
            continue;
        }
        if !matches!(ty, convert::Type::Nominal { .. }) || carries_lifetime(&ty) || is_value(&ty) {
            continue;
        }
        if let Some(rust) = concrete_type(&ty) {
            lent.push((index, *mutable, format!("reach_lent::<{rust}>")));
        }
    }
    Ok(lent)
}

/// The `&[T]` inputs of crate values a call takes (`lent_slice_element`),
/// which a native slice place may fill, as a marked page's `&BLOCKS[1..2]`
/// does (#1291): each one's argument index, its element's descriptor and
/// the element's Rust type.
fn slice_inputs(plan: &Plan) -> Result<Vec<(usize, String, String)>, request::Error> {
    let Some((signature, substitutions, offset)) = call_signature(plan) else {
        return Ok(Vec::new());
    };
    let mut slices = Vec::new();
    for (index, input) in runtime_inputs(signature, offset) {
        let ty = substituted(input, signature, substitutions)?;
        if let Some(element) = lent_slice_element(input, &ty)
            && place::available(element)
            && let Some(rust) = concrete_type(element)
        {
            slices.push((index, place::policy(element)?, rust));
        }
    }
    Ok(slices)
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
        // A value of a region, an array of them, as `Column::
        // with_children` takes `column![..]`'s elements, or an option of
        // one, as a crate's impl of its trait for `Option<T>` takes
        // `maybe.map(|x| text(x))`.
        let element = match &ty {
            convert::Type::Sequence {
                element,
                owned: true,
                length: None,
            }
            | convert::Type::Option(element) => element,
            ty => ty,
        };
        // One a region holds boxed is reached through its trait alone
        // (`region_operand`), as the call can't take its type.
        let boxed = scoped_type(element).is_some_and(|key| family::Ops::held(&key).is_some());
        if family_of(element).is_some() && !boxed {
            region.push(index);
        }
    }
    Ok(region)
}

thread_local! {
    /// While a generic script impl function's frame is rendered
    /// (`render_frame`): the slot of each opaque type of its own
    /// parameters, by identity, and how the code being written names the
    /// frame.
    static FRAME: std::cell::RefCell<Option<EnteredFrame>> =
        const { std::cell::RefCell::new(None) };
}

/// A frame's slots by identity and the name the code being written gives
/// the frame (`FRAME`).
type EnteredFrame = (std::collections::BTreeMap<[u8; 32], usize>, &'static str);

/// Sets `FRAME` for as long as it lives.
struct InFrame(Option<EnteredFrame>);

impl InFrame {
    fn enter(slots: &std::collections::BTreeMap<[u8; 32], usize>, name: &'static str) -> Self {
        Self(FRAME.replace(Some((slots.clone(), name))))
    }
}

impl Drop for InFrame {
    fn drop(&mut self) {
        FRAME.set(self.0.take());
    }
}

/// How the code being written names the frame that keeps values of the
/// opaque type `definition`, and its slot there (`FRAME`).
fn frame_slot(definition: &crate::surface::identity::Definition) -> Option<(&'static str, usize)> {
    FRAME.with_borrow(|frame| {
        let (slots, name) = frame.as_ref()?;
        Some((*name, *slots.get(definition.as_bytes())?))
    })
}

thread_local! {
    /// While a call that makes a value whose type holds an opaque type is
    /// rendered: the expression of the table of calls that take it.
    static OPAQUE_PRODUCING: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
    /// While a call that takes such a value is rendered inside the call
    /// that made it: a value of the same type it gives takes `table`.
    static OPAQUE_CONSUMING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// While a call in a table is rendered: the type of the value that made
    /// the table, which its `_w` names.
    static OPAQUE_WITNESSED: std::cell::RefCell<Option<convert::Type>> = const { std::cell::RefCell::new(None) };
    /// While the call that makes such a value is rendered only for rustc
    /// to read its type: the value goes to `witness`.
    static OPAQUE_WITNESSING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// While a call in a table is rendered whose value lives beside what it
    /// borrows (`lent_cell`): the method of the value's proxy it calls.
    static OPAQUE_CELL: std::cell::Cell<Option<ProxyReach>> = const { std::cell::Cell::new(None) };
}

/// Sets `OPAQUE_CELL` for as long as it lives.
struct OpaqueCell(Option<ProxyReach>);

impl OpaqueCell {
    fn enter(reach: Option<ProxyReach>) -> Self {
        Self(OPAQUE_CELL.replace(reach))
    }
}

impl Drop for OpaqueCell {
    fn drop(&mut self) {
        OPAQUE_CELL.set(self.0);
    }
}

/// How the value `plan` gives lives beside the crate values it borrows
/// (`lent_cell`), as its adapter makes it, when it does.
fn lends_cell(plan: &Plan) -> Result<Option<Owned>, request::Error> {
    let Operation::Function {
        signature,
        substitutions,
        ..
    } = plan.operation()
    else {
        return Ok(None);
    };
    let pinned = opaque_pinned_strings(signature, substitutions);
    let substitutions: &[convert::Type] = pinned.as_deref().unwrap_or(substitutions);
    if instance_opens(signature, substitutions) || !views_natives(signature, substitutions)? {
        return Ok(None);
    }
    lent_cell(signature, substitutions)
}

/// Sets `OPAQUE_WITNESSED` for as long as it lives.
struct OpaqueWitnessed(Option<convert::Type>);

impl OpaqueWitnessed {
    fn enter(ty: Option<convert::Type>) -> Self {
        Self(OPAQUE_WITNESSED.replace(ty))
    }
}

impl Drop for OpaqueWitnessed {
    fn drop(&mut self) {
        OPAQUE_WITNESSED.set(self.0.take());
    }
}

/// Sets `OPAQUE_WITNESSING` for as long as it lives.
struct OpaqueWitnessing(bool);

impl OpaqueWitnessing {
    fn enter() -> Self {
        Self(OPAQUE_WITNESSING.replace(true))
    }
}

impl Drop for OpaqueWitnessing {
    fn drop(&mut self) {
        OPAQUE_WITNESSING.set(self.0);
    }
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
/// index, with that opaque type and the operand's whole type: the
/// receiver, or an input.
fn opaque_operand(
    plan: &Plan,
) -> Result<Option<(usize, convert::Type, convert::Type)>, request::Error> {
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
                    return Ok(Some((0, opaque.clone(), instance.clone())));
                }
            }
            if let Some(opaque) = substitutions
                .iter()
                .take(*container_parameters)
                .find_map(first_opaque)
            {
                return Ok(Some((0, opaque.clone(), opaque.clone())));
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
            return Ok(Some((runtime, opaque.clone(), ty.clone())));
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
        if opaque_operand(plan)?.is_some_and(|(_, opaque, _)| same_opaque(&opaque, key)) {
            consumers.push(plan);
        }
    }
    Ok(consumers)
}

/// Whether an operand of type `operand` is the value of the opaque type
/// `opaque` at a handle, or a borrow of it, so the value carries its table.
fn by_handle(operand: &convert::Type, opaque: &convert::Type) -> bool {
    match operand {
        convert::Type::Borrowed { ty, .. } => by_handle(ty, opaque),
        operand => operand == opaque || !matches!(operand, convert::Type::Option(_)),
    }
}

/// The opaque type the value `plan` gives holds, when it does.
fn produced_opaque(plan: &Plan) -> Result<Option<convert::Type>, request::Error> {
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
    Ok(first_opaque(&returned).cloned())
}

/// The call in `cohort` whose adapter compiles the table of the opaque type
/// `key` for calls that take it inside another type: the first that gives a
/// value holding it and takes none.
fn opaque_producer<'p>(
    cohort: &'p [Plan],
    key: &convert::Type,
) -> Result<Option<&'p Plan>, request::Error> {
    for plan in cohort {
        if opaque_operand(plan)?.is_none()
            && produced_opaque(plan)?.is_some_and(|produced| same_opaque(&produced, key))
        {
            return Ok(Some(plan));
        }
    }
    Ok(None)
}

/// Whether `plan` compiles the table of the opaque type it gives for calls
/// that take that type inside another, as an `Option` of it.
fn witnessed(plan: &Plan, cohort: &[Plan]) -> Result<bool, request::Error> {
    let Some(key) = produced_opaque(plan)? else {
        return Ok(false);
    };
    if !opaque_producer(cohort, &key)?.is_some_and(|producer| std::ptr::eq(producer, plan)) {
        return Ok(false);
    }
    for consumer in opaque_consumers(cohort, &key)? {
        if opaque_operand(consumer)?
            .is_some_and(|(_, opaque, operand)| !by_handle(&operand, &opaque))
        {
            return Ok(true);
        }
    }
    Ok(false)
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
    // A value that lives beside what it borrows (`lent_cell`) is reached
    // through its proxy. Where it implements its trait through the proxy
    // (`Owned::forwards`), a call takes it by value, and what it borrows
    // moves with it (#856).
    let owned = lends_cell(plan)?;
    let cell = owned.is_some();
    let forwarded = owned.is_some_and(|owned| owned.forwards.is_some());
    let mut entries = Vec::new();
    for (index, consumer) in opaque_consumers(cohort, &key)?.into_iter().enumerate() {
        let Some((position, _, operand)) = opaque_operand(consumer)? else {
            continue;
        };
        // An operand that holds the table's opaque type, where the table's
        // type is that opaque type itself, is taken at the type that holds
        // it (`shape_witnesses`). So is a receiver whose type's arguments
        // hold it, as `MouseArea<'_, Message, Container<'_, _>>` of
        // `.on_press(..)`: `opaque_operand` gives the opaque type there.
        let receiver_holds = position == 0
            && matches!(consumer.operation(), Operation::Method { signature, .. }
                if signature.instance().is_none());
        let holding =
            same_opaque(&returned, &key) && (receiver_holds || !same_opaque(&operand, &key));
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
        let other = first_opaque(&produced).is_some_and(|produced| !same_opaque(produced, &key));
        let inner = if other {
            opaque_table_source(consumer, cohort, profile, facade)?
        } else {
            None
        };
        // Such a value's method is its proxy's (`proxy_entry`).
        let reach = if cell {
            proxy_entry(index, consumer, &key)?.map(|entry| ProxyReach {
                index,
                derives: entry.output.is_none(),
            })
        } else {
            None
        };
        let lent = lent_inputs_of(consumer, cohort, profile, facade)?;
        let body = {
            let _lent = LentArguments::enter(!lent.is_empty());
            let _producing = OpaqueProducing::enter(inner);
            let _consuming = OpaqueConsuming::enter(!other);
            let _witnessed = OpaqueWitnessed::enter(Some(returned.clone()));
            let _cell = OpaqueCell::enter(reach);
            let mut body = Vec::new();
            // What a region holds boxed is made of a receiver in the region
            // (`region_operand`), never of this value, which is in the
            // arena.
            let boxed = scoped_type(&produced).is_some_and(|key| family::Ops::held(&key).is_some());
            let region = region_inputs(consumer)?;
            if !region.is_empty() && !boxed {
                let indices = region
                    .iter()
                    .map(usize::to_string)
                    .collect::<Vec<_>>()
                    .join(", ");
                writeln!(
                    body,
                    "    if let Some(scoped) = scoped_innermost(arguments, &[{indices}]) {{
        return __rawr_internal::scope::with::<__rawr_internal::scope::RegionF, _>(scoped.nonce(), |region| -> Result<Dynamic, Error> {{"
                )
                .map_err(request::unwritable)?;
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
                .map_err(request::unwritable)?;
            }
            render_adapter_body(consumer, cohort, profile, facade, &mut body)?;
            String::from_utf8(body).map_err(request::unwritable)?
        };
        // Each argument native code lent, as the `&mut Vec<String>` a
        // program's impl of `labels` delegates to the value it holds, is
        // reached where it is for the call (#913).
        let body = within_lent(&lent, body);
        // A call that takes the value itself, where it doesn't implement
        // its trait through its proxy, can't take it from beside what it
        // borrows.
        let body = if cell
            && !forwarded
            && body.contains(&format!("take_native(arguments, {position})"))
        {
            "    Err(Error::Home(\"rawr doesn't move a result that borrows its call's values yet\".into()))".to_owned()
        } else {
            body
        };
        // A method its proxy can't hold: one that takes the opaque type, or
        // gives it inside another type, or gives a borrow of the value.
        let body = if cell && reach.is_none() && proxied(consumer, &key)? {
            "    Err(Error::Home(\"rawr doesn't call this method on a result that borrows its call's values yet\".into()))".to_owned()
        } else {
            body
        };
        // The operand is taken at the type of the value this call made.
        let body = body.replace(
            &format!("take_native(arguments, {position})"),
            &format!("take_like(arguments, {position}, _w)"),
        );
        let body = if holding {
            shape_witnesses(&body, position)
        } else {
            body
        };
        entries.push(format!(
            "|arguments: &mut Arguments<'_, '_, '_>, table: OpaqueTable, _w| -> Result<Dynamic, Error> {{\n    let _ = (&table, &_w);\n{body}\n}}"
        ));
    }
    // Last, the core's future of the value, when its type is a future, as
    // a program's own `impl Straw<..>` is: where a call takes a future,
    // as iced's `Task::future(impl Future<Output = T>)` does, the value
    // goes as that future (`take_future`).
    let future = match &returned {
        convert::Type::Nominal {
            definition,
            arguments,
        } => OPAQUE_FUTURES
            .with_borrow(|futures| futures.get(definition.as_bytes()).copied())
            .and_then(|position| arguments.get(usize::try_from(position).ok()?)),
        _ => None,
    };
    let body = match future {
        Some(element) => {
            let mut body = Vec::new();
            body.write_all(b"    let value = take_like(arguments, 0, _w)?;\n")
                .map_err(request::unwritable)?;
            render_give_task(element, "value", "native", &mut body)?;
            String::from_utf8(body).map_err(request::unwritable)?
        }
        None => "    Err(Error::TypeMismatch)".to_owned(),
    };
    entries.push(format!(
        "|arguments: &mut Arguments<'_, '_, '_>, table: OpaqueTable, _w| -> Result<Dynamic, Error> {{\n    let _ = (&table, &_w, &arguments);\n{body}\n}}"
    ));
    Ok(Some(format!(
        "{{
        static TABLE: std::sync::OnceLock<Box<[OpaqueEntry]>> = std::sync::OnceLock::new();
        OpaqueTable(TABLE.get_or_init(|| opaque_table(__RAWR_WITNESS__, [{}])))
    }}",
        entries.join(",\n")
    )))
}

/// `body` with each operand at `position` taken at a type that holds the
/// table's opaque type, as `boxed` takes the `Map<impl Widget, ..>` that
/// `map` gave, taken at that type: the witness of the opaque type, made
/// the witness of the type that holds it where `_` spells the opaque type.
fn shape_witnesses(body: &str, position: usize) -> String {
    let take = format!(" = take_like(arguments, {position}, _w)?;");
    let mut shaped = String::with_capacity(body.len());
    for line in body.split_inclusive('\n') {
        let held = line
            .trim_start()
            .strip_prefix("let ")
            .and_then(|rest| rest.split_once(": "))
            .and_then(|(_, rest)| rest.split_once(&take))
            .map(|(ty, _)| ty)
            .filter(|ty| *ty != "_");
        let Some(ty) = held else {
            shaped.push_str(line);
            continue;
        };
        let holes = opaque_holes(ty);
        let [hole] = holes[..] else {
            shaped.push_str(line);
            continue;
        };
        let (holding, lifetimes) = named_lifetimes(&format!("{}T{}", &ty[..hole], &ty[hole + 1..]));
        shaped.push_str(&line.replacen(
            &take,
            &format!(
                " = take_like(arguments, {position}, {{ fn shape<{lifetimes}T>(_: std::marker::PhantomData<T>) -> std::marker::PhantomData<{holding}> {{ std::marker::PhantomData }} shape(_w) }})?;"
            ),
            1,
        ));
    }
    shaped
}

/// The Rust type `ty`, which `concrete_type` wrote for a body, as an item
/// names it: each lifetime it leaves to inference, `'_`, a lifetime of the
/// item's own, `'l0`, `'l1` and on, and those lifetimes as the item
/// declares them, each followed by `, `. An item's lifetimes are its
/// caller's to pick, as a body's inferred ones are.
fn named_lifetimes(ty: &str) -> (String, String) {
    let mut declared = String::new();
    let named = inferred_lifetimes(ty, |index| {
        declared.push_str(&format!("'l{index}, "));
        format!("'l{index}")
    });
    (named, declared)
}

/// `ty` with each lifetime it leaves to inference, `'_`, as `name` names
/// the one at its index among them.
fn inferred_lifetimes(ty: &str, mut name: impl FnMut(usize) -> String) -> String {
    let mut named = String::with_capacity(ty.len());
    let mut rest = ty;
    let mut count = 0;
    while let Some(at) = rest.find("'_") {
        named.push_str(&rest[..at]);
        if rest[at + 2..]
            .chars()
            .next()
            .is_some_and(|next| next.is_alphanumeric() || next == '_')
        {
            named.push_str("'_");
        } else {
            named.push_str(&name(count));
            count += 1;
        }
        rest = &rest[at + 2..];
    }
    named.push_str(rest);
    named
}

/// The offsets of each `_` in the Rust type `ty` that stands for a whole
/// type, as in `Map<_, F, M>`, and not inside a name.
fn opaque_holes(ty: &str) -> Vec<usize> {
    let bytes = ty.as_bytes();
    // Neither inside a name nor a lifetime's, `'_`.
    let name = |byte: u8| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'\'';
    (0..bytes.len())
        .filter(|&at| {
            bytes[at] == b'_'
                && (at == 0 || !name(bytes[at - 1]))
                && bytes.get(at + 1).is_none_or(|&next| !name(next))
        })
        .collect()
}

/// The signature, substitutions and argument offset of a call: a
/// function's, or a method's after its receiver, whether or not it also
/// takes a script closure, as iced's `table::column(header, |row| ..)`
/// and `Cache::draw(renderer, size, |frame| ..)` do (#952, #954).
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
            ..
        } => Some((signature, substitutions, 0)),
        Operation::Method {
            signature,
            substitutions,
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
    REGIONAL_CLOSURES.with_borrow_mut(Vec::clear);
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
            .map_err(request::unwritable)?;
        }
        Operation::StructConstructor { form, fields, .. } => {
            render_fields(fields, 0, output)?;
            let ty = structural_type(plan);
            write!(output, "    let output: {ty} = {path}").map_err(request::unwritable)?;
            render_constructor(*form, fields, output)?;
            output
                .write_all(b";\n    insert_native!(arguments.arena, output).map(Dynamic::from)\n")
                .map_err(request::unwritable)?;
        }
        Operation::StructField { field, .. } => {
            let path = structural_type(plan);
            // A field of a value in a scope frame's region (D558 tier 1), as
            // quick-xml's `Attribute::value`: a copy, which stays in the
            // region when it carries the region's lifetime.
            if let Some(family) = container_family(&path, &path) {
                let reached = family.call(family::Reach::Get, "scoped");
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
            let output = Clone::clone(&region.{reached}.map_err(|error| Error::Lent(scoped.name(), error))?.{});
            {converted}
        }})
        .map_err(|error| Error::Lent(scoped.name(), error))?;
    }}",
                    field.name()
                )
                .map_err(request::unwritable)?;
            }
            if shared_field(field.ty()) && !matches!(field.ty(), convert::Type::Borrowed { .. }) {
                // A field of an element a native place names, as `family.
                // name` of an option a lent slice gives (#687): read where
                // the element is, inside one claim of the place, with the
                // arena lent so the claim can reach the slice's owner.
                let read = if copy_field(field.ty()) {
                    format!("receiver.read().{}", field.name())
                } else {
                    format!("Clone::clone(&receiver.read().{})", field.name())
                };
                writeln!(
                    output,
                    "    if let Some(place) = arguments.values.first().and_then(|value| __rawr_internal::place::get(value)) {{
        let output = lend(arguments.arena, || __rawr_internal::place::reach_as::<{path}, _>(&place, false, |receiver| Ok({read})))
            .map_err(|error| Error::Value(error.to_string()))?;
        return {{"
                )
                .map_err(request::unwritable)?;
                render_give(field.ty(), "output", output)?;
                writeln!(output, "    }};\n    }}").map_err(request::unwritable)?;
            }
            if matches!(field.ty(), convert::Type::Borrowed { mutable: false, .. }) {
                // A slice of crate values, as a page's `children: &'static
                // [Page]` (#1291): a native place that reaches the field
                // through the struct at each operation, so its elements are
                // read where they are.
                if let Some(element) = lent_elements(field.ty())
                    && !carries_lifetimes(&path)
                {
                    let policy = place::policy(element)?;
                    writeln!(
                        output,
                        "    let parent = (**arguments.values.first().ok_or(Error::MissingArgument)?).clone();
    let lender = Lender::of(&parent)?;
    let witness = output_witness().ok_or(Error::Stale)?;
    __rawr_internal::place::slice::lent_by::<{policy}>(&witness, move |visit| reach_lender::<{path}>(&lender, &mut |input| visit(input.{name})))
        .map_err(|error| Error::Value(error.to_string()))",
                        name = field.name()
                    )
                    .map_err(request::unwritable)?;
                    return Ok(());
                }
                // A field that borrows for the struct's lifetime, as iced's
                // `LineDash::segments`: Rawr's copy, made while the struct
                // is lent. A field of an element of a native slice, as a
                // page's `title: &'static str` (#1291), is read where the
                // element is.
                let mut converted = String::new();
                render_to_dynamic(field.ty(), "output", 0, &mut converted)?;
                if !carries_lifetimes(&path) {
                    writeln!(
                        output,
                        "    if let Some(place) = arguments.values.first().and_then(|value| __rawr_internal::place::get(value)) {{
        return lend(arguments.arena, || __rawr_internal::place::reach_as::<{path}, _>(&place, false, |receiver| {{ let output = receiver.read().{name}; Ok({converted}) }}))
            .map_err(|error| Error::Value(error.to_string()))?;
    }}",
                        name = field.name()
                    )
                    .map_err(request::unwritable)?;
                }
                writeln!(output, "    if let Some(lent) = lent_value(arguments, 0) {{
        let access: fn(u64, &mut dyn FnMut(&{path})) -> Result<(), u8> = lent
            .accessor(false).map_err(|error| Error::Lent(lent.name(), error))?;
        let mut copied = None;
        access(lent.nonce().bits(), &mut |receiver| {{ let output = receiver.{name}; copied = Some({converted}); }})
            .map_err(|code| Error::Lent(lent.name(), __rawr_internal::scope::Error::from_code(code)))?;
        return copied.ok_or(Error::Lent(lent.name(), __rawr_internal::scope::Error::Busy))?;
    }}
    let handle = native_handle(arguments, 0, false)?;
    arguments.arena.shared::<{path}, _>(handle, |input| {{ let output = input.{name}; {converted} }})?", name = field.name()).map_err(request::unwritable)?;
                return Ok(());
            }
            if copy_field(field.ty()) {
                let mut converted = String::new();
                render_to_dynamic(field.ty(), "output", 0, &mut converted)?;
                // A field that borrows for the struct's lifetime, as iced's
                // `Stroke::line_dash: LineDash<'a>` (#951), keeps the borrow
                // where the struct is: a lent struct lends it only for the
                // accessor's call, so it can't be given out of it, and one
                // in a scope frame's region gives it into the region.
                if !carries_lifetime(field.ty()) {
                    writeln!(output, "    if let Some(lent) = lent_value(arguments, 0) {{
        let access: fn(u64, &mut dyn FnMut(&{path})) -> Result<(), u8> = lent
            .accessor(false).map_err(|error| Error::Lent(lent.name(), error))?;
        let mut output = None;
        access(lent.nonce().bits(), &mut |receiver| output = Some(receiver.{}))
            .map_err(|code| Error::Lent(lent.name(), __rawr_internal::scope::Error::from_code(code)))?;
        let output = output.ok_or(Error::Lent(lent.name(), __rawr_internal::scope::Error::Busy))?;
        return {converted};
    }}", field.name()).map_err(request::unwritable)?;
                }
                output
                    .write_all(b"    let handle = native_handle(arguments, 0, false)?;\n")
                    .map_err(request::unwritable)?;
                writeln!(
                    output,
                    "    let output = arguments.arena.shared::<{path}, _>(handle, |input| input.{})?;",
                    field.name()
                )
                .map_err(request::unwritable)?;
            } else if plain_value(field.ty()) {
                // A field Rawr holds as its own value, as a `String`: read
                // through the value where it is, which stays whole, as Rust
                // reads `font.family` without moving `font`. A lent value of
                // a type with lifetimes, as quick-xml's `Attribute<'a>`,
                // lends its fields only for the accessor's call.
                if !carries_lifetimes(&path) {
                    writeln!(output, "    if let Some(lent) = lent_value(arguments, 0) {{
        let access: fn(u64, &mut dyn FnMut(&{path})) -> Result<(), u8> = lent
            .accessor(false).map_err(|error| Error::Lent(lent.name(), error))?;
        let mut output = None;
        access(lent.nonce().bits(), &mut |receiver| output = Some(Clone::clone(&receiver.{name})))
            .map_err(|code| Error::Lent(lent.name(), __rawr_internal::scope::Error::from_code(code)))?;
        let output = output.ok_or(Error::Lent(lent.name(), __rawr_internal::scope::Error::Busy))?;
        return {{", name = field.name()).map_err(request::unwritable)?;
                    render_give(field.ty(), "output", output)?;
                    writeln!(output, "    }};\n    }}").map_err(request::unwritable)?;
                }
                output
                    .write_all(b"    let handle = native_handle(arguments, 0, false)?;\n")
                    .map_err(request::unwritable)?;
                writeln!(
                    output,
                    "    let output = arguments.arena.shared::<{path}, _>(handle, |input| Clone::clone(&input.{}))?;",
                    field.name()
                )
                .map_err(request::unwritable)?;
            } else {
                writeln!(
                    output,
                    "    let input: {path} = take_native(arguments, 0)?;"
                )
                .map_err(request::unwritable)?;
                writeln!(output, "    let output = input.{};", field.name())
                    .map_err(request::unwritable)?;
            }
            render_give(field.ty(), "output", output)?;
        }
        Operation::EnumVariant {
            name, form, fields, ..
        } => {
            render_fields(fields, 0, output)?;
            // A generic enum's variant, at its instance's types.
            let ty = structural_type(plan);
            write!(output, "    let output: {ty} = {path}::{name}").map_err(request::unwritable)?;
            render_constructor(*form, fields, output)?;
            output
                .write_all(b";\n    insert_native!(arguments.arena, output).map(Dynamic::from)\n")
                .map_err(request::unwritable)?;
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
                return Err(request::drift(
                    "profile event mapper invocation lacks replay projection",
                ));
            }
            let crossed = Crossed { slots, crossings };
            let pinned = opaque_pinned_strings(signature, substitutions);
            let substitutions: &[convert::Type] = pinned.as_deref().unwrap_or(substitutions);
            let opens = instance_opens(signature, substitutions);
            // A `&str` the output borrows through a lifetime its type only
            // carries, as iced's `text(url)` gives a `Text<'a>` of an `impl
            // IntoFragment<'a>`: pinned in a slot's region, as an opening's
            // strings are, and out of any region for the runner's life.
            let pins = !opens && replay.is_none() && lends_str_to_output(signature, substitutions)?;
            let _pins = PinnedStrings::enter(pins);
            // A crate value the output borrows past the call, as
            // `qr_code(&self.data)` gives a `QRCode<'a>` (D568).
            let _viewed = ViewedArguments::enter(
                !opens && replay.is_none() && views_natives(signature, substitutions)?,
            );
            // An opening that returns on every path, as a call pinned for
            // the runner's life out of any region does, ends the adapter:
            // a frame written after it would never run.
            if (opens || pins)
                && replay.is_none()
                && render_anchored_opening(
                    plan,
                    signature,
                    substitutions,
                    constants,
                    functions,
                    mappers,
                    *container_parameters,
                    *associated,
                    output,
                )? == Ending::Returns
            {
                return Ok(());
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
                    .map_err(request::unwritable)?;
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
                output.write_all(b"(").map_err(request::unwritable)?;
                function_argument_list(
                    signature,
                    substitutions,
                    functions,
                    mappers,
                    Some(profile),
                    crossed,
                    output,
                )?;
                output.write_all(b"));\n").map_err(request::unwritable)?;
                render_replay_insert(replay, facade, output)?;
            } else {
                // A Rust future is made when its task first runs, on
                // iced's executor, as tokio's `sleep` needs.
                let lazy = matches!(
                    substitute(signature.output(), signature, substitutions)?,
                    convert::Type::Nominal { definition, .. }
                        if *definition == crate::surface::identity::Definition::future()
                );
                // An opaque output that borrows crate values its call takes
                // lives beside them in one value (#846).
                let owned = if VIEWED_ARGUMENTS.get() && !lazy {
                    lent_cell(signature, substitutions)?
                } else {
                    None
                };
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
                call.write_all(b"(").map_err(request::unwritable)?;
                {
                    let _cell = LentCell::enter(owned.is_some());
                    function_argument_list(
                        signature,
                        substitutions,
                        functions,
                        mappers,
                        None,
                        crossed,
                        &mut call,
                    )?;
                }
                call.write_all(b")").map_err(request::unwritable)?;
                let call = String::from_utf8(call).map_err(request::unwritable)?;
                let returned = &substituted(signature.output(), signature, substitutions)?;
                if !lazy && !lifetime_inputs(signature, substitutions)? {
                    render_anchored_output(returned, &call, output)?;
                }
                // A native call lends the arena to the script closures it
                // calls back, as Freya's `launch` calls `fn app()` (D551.6).
                let rebound = if lazy {
                    future_borrows(signature, substitutions)?
                } else {
                    String::new()
                };
                // The calls the program makes on such an output, which
                // reach it through its proxy (#849).
                let entries = match &owned {
                    Some(_) => proxy_entries(cohort, returned)?,
                    None => Vec::new(),
                };
                if lazy && !rebound.is_empty() {
                    // A future that borrows its inputs holds what lends them
                    // for its life: a leased value, or a value made for the
                    // call (D683.2).
                    writeln!(
                        output,
                        "    let output = __rawr_profile_host::task::lazy(move || async move {{\n{rebound}        {call}.await\n    }});"
                    )
                } else if lazy {
                    writeln!(
                        output,
                        "    let output = __rawr_profile_host::task::lazy(move || {call});"
                    )
                } else if let Some(owned) = &owned {
                    render_lent_cell(owned, &entries, &call, output)
                } else if VIEWED_ARGUMENTS.get() {
                    // Out of any view's region, the output is at
                    // `'static`, and so is what it borrows.
                    writeln!(
                        output,
                        "    let pins = static_pins();\n    let output = lend(arguments.arena, || {call});"
                    )
                } else {
                    writeln!(output, "    let output = lend(arguments.arena, || {call});")
                }
                .map_err(request::unwritable)?;
                if let Some(element) = future_output(returned) {
                    render_give_task(element, "output", "imported", output)?;
                } else {
                    render_give_with(signature, substitutions, "output", output)?;
                }
            }
        }
        Operation::Method {
            receiver: Receiver::Shared,
            signature,
            substitutions,
            constants,
            container_parameters,
            call: None,
            ..
        } if lent_slice(signature, substitutions).is_some() => {
            render_lent_slice(
                plan,
                &path,
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
                // methods took slots. A slot lends a native value `&mut`
                // too, as `Frame::with_save` lends its frame, and a closure
                // that captures crosses as the closure Rawr makes.
                let lends = |crossing: &crate::classify::representability::callable::Call| {
                    crossing.inputs().iter().all(|input| {
                        match instantiated(input, signature, substitutions) {
                            Some(convert::Type::Borrowed { mutable: true, ty })
                                if lent_mutably(&ty) =>
                            {
                                true
                            }
                            Some(convert::Type::Borrowed { ty, .. }) => {
                                matches!(*ty, convert::Type::Script { .. })
                            }
                            Some(_) => true,
                            None => false,
                        }
                    })
                };
                // A crate's own callable value is taken as itself either
                // way (`Slot::native`).
                let crossed = if call.is_some() && !crossings.iter().all(lends) {
                    Crossed {
                        slots,
                        crossings: &[],
                    }
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
                            .map_err(request::unwritable)?;
                    }
                }
            } else {
                render_arguments_with(signature, substitutions, 1, output)?;
            }
            if let Some(convert::Type::ProfileMessage { mapper, payload }) = continuation {
                output
                    .write_all(b"    __rawr_profile::map_projectable::<")
                    .map_err(request::unwritable)?;
                render_container_path(
                    &path,
                    signature,
                    substitutions,
                    constants,
                    *container_parameters,
                    true,
                    output,
                )?;
                output.write_all(b", ").map_err(request::unwritable)?;
                render_rust_type(
                    payload,
                    facade.ok_or_else(|| request::drift("event continuation bindings"))?,
                    output,
                )?;
                output
                    .write_all(b">(arguments, &")
                    .map_err(request::unwritable)?;
                byte_slice(output, mapper)?;
                write!(
                    output,
                    ", move |receiver, _context| receiver.{}",
                    plan.name()
                )
                .map_err(request::unwritable)?;
                render_item_turbofish(
                    signature,
                    substitutions,
                    constants,
                    *container_parameters,
                    output,
                )?;
                output.write_all(b"(").map_err(request::unwritable)?;
                replay_argument_list(signature, substitutions, profile, output)?;
                output.write_all(b"))\n").map_err(request::unwritable)?;
            } else if let (Receiver::Owned, Some(replay)) = (receiver, replay) {
                output
                    .write_all(b"    let receiver: __rawr_profile::Replay<")
                    .map_err(request::unwritable)?;
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
                    .map_err(request::unwritable)?;
                write!(
                    output,
                    "    let output = __rawr_profile::Replay::new(move |_context| receiver.run(_context).{}",
                    plan.name()
                )
                .map_err(request::unwritable)?;
                render_item_turbofish(
                    signature,
                    substitutions,
                    constants,
                    *container_parameters,
                    output,
                )?;
                output.write_all(b"(").map_err(request::unwritable)?;
                replay_argument_list(signature, substitutions, profile, output)?;
                output.write_all(b"));\n").map_err(request::unwritable)?;
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
                .map_err(request::unwritable)?;
                output
                    .write_all(b"    let held: Held<")
                    .map_err(request::unwritable)?;
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
                        .map_err(request::unwritable)?;
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
                        .map_err(request::unwritable)?;
                }
                write!(
                    output,
                    "    let output = __rawr_profile_host::task::lazy(move || async move {{
        let mut held = held;
        held.get_mut().{}",
                    plan.name()
                )
                .map_err(request::unwritable)?;
                render_item_turbofish(
                    signature,
                    substitutions,
                    constants,
                    *container_parameters,
                    output,
                )?;
                output.write_all(b"(").map_err(request::unwritable)?;
                argument_list(signature.inputs().len(), 1, output)?;
                output
                    .write_all(b").await\n    });\n")
                    .map_err(request::unwritable)?;
                let returned = &substituted(signature.output(), signature, substitutions)?;
                let element = future_output(returned)
                    .ok_or_else(|| request::drift("borrowed async method output"))?;
                render_give_task(element, "output", "imported", output)?;
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
                    // Rawr's string (`surface::text`), lent at the lifetime
                    // the impl names (#717).
                    Receiver::Owned if str_member(signature) => {
                        render_str_receiver(
                            plan,
                            signature,
                            substitutions,
                            constants,
                            *container_parameters,
                            output,
                        )?;
                        let annotation = concrete_type(&substituted(
                            signature.output(),
                            signature,
                            substitutions,
                        )?)
                        .map(|ty| format!(": {ty}"))
                        .unwrap_or_default();
                        write!(
                            output,
                            "    let output{annotation} = lend(arguments.arena, || "
                        )
                        .map_err(request::unwritable)?;
                        render_method_head(
                            plan.name(),
                            *receiver,
                            signature,
                            substitutions,
                            constants,
                            *container_parameters,
                            output,
                        )?;
                        argument_list(signature.inputs().len(), 1, output)?;
                        output.write_all(b"));\n").map_err(request::unwritable)?;
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
                            None,
                            output,
                        )?;
                        // A `&str` lent to a receiver out of any region,
                        // whose output keeps it, as ratatui's `Block::
                        // title("list")` keeps its title, is pinned for the
                        // runner's life, as a function's is
                        // (`lends_str_to_output`): the output the arena
                        // keeps borrows it at `'static`. The take's own
                        // `&str` of it is set aside, as the region's branch
                        // pins the string too.
                        let returned = substituted(signature.output(), signature, substitutions)?;
                        if carries_lifetime(&returned) {
                            for (position, input) in signature.inputs().iter().enumerate() {
                                if lends_str(input, signature, substitutions) {
                                    writeln!(
                                        output,
                                        "    let _ = argument_{position};\n    let argument_{position} = static_pins().pin(argument_{position}_owned);"
                                    )
                                    .map_err(request::unwritable)?;
                                }
                            }
                        }
                        let mut container = Vec::new();
                        render_container_path(
                            &path,
                            signature,
                            substitutions,
                            constants,
                            *container_parameters,
                            true,
                            &mut container,
                        )?;
                        let container =
                            String::from_utf8(container).map_err(request::unwritable)?;
                        // A receiver of a type that carries no lifetime, as
                        // iced's `Table` or `Space`, borrows nothing a
                        // region must keep (#352).
                        let lifetime_free = container_family(&path, &container).is_none();
                        // In a region an argument is in, a receiver made
                        // outside it is at `'static`, which joins the
                        // argument only when its type shortens, as
                        // `region_arena` takes an argument: an invariant
                        // one, as pane_grid's `Content<'a, ..>` that
                        // `title_bar` takes, is the region's or none.
                        let invariant = REGION_ARGUMENTS.get()
                            && !lifetime_free
                            && !container_shortens(&path, &container);
                        let taken = if invariant {
                            format!(
                                "Err(Error::Value({:?}.to_owned()))",
                                format!(
                                    "`{container}` is invariant in its lifetime, so one made outside the view can't join those the view makes"
                                )
                            )
                        } else {
                            "take_native(arguments, 0)".to_owned()
                        };
                        writeln!(output, "    let receiver: {container} = {taken}?;")
                            .map_err(request::unwritable)?;
                        // An implementation's parameter the call infers, as
                        // `Renderer` of iced's `Widget` for `Button`, is
                        // named by the output's type.
                        let inferred = signature.parameters().iter().any(|parameter| {
                            parameter.is_argument_impl()
                                && output_names(signature.output(), parameter.index())
                        });
                        // So is a concrete output, as that of `into` of
                        // core's `impl<T, U: From<T>> Into<U> for T`
                        // applied to a crate type through one of the
                        // crate's `From` impls, which rustc can't pick by
                        // the receiver alone.
                        let annotation = concrete_type(&returned)
                            .filter(|ty| inferred || !names_inferred(ty))
                            .map(|ty| format!(": {ty}"))
                            .unwrap_or_default();
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
                            .map_err(request::unwritable)?;
                        } else {
                            let mut call = Vec::new();
                            render_method_head(
                                plan.name(),
                                *receiver,
                                signature,
                                substitutions,
                                constants,
                                *container_parameters,
                                &mut call,
                            )?;
                            argument_list(signature.inputs().len(), 1, &mut call)?;
                            call.write_all(b")").map_err(request::unwritable)?;
                            let call = String::from_utf8(call).map_err(request::unwritable)?;
                            // A value that carries a lifetime, made while a
                            // slot's region is open from an opaque value its
                            // table takes, stays in that region, as a
                            // function's does: `Widget::boxed` of an opaque
                            // widget gives the view's element, and so does
                            // `boxed` of a widget whose type carries no
                            // lifetime. A receiver of a type that names its
                            // own lifetime keeps the one it was made at.
                            if (lifetime_free || OPAQUE_WITNESSED.with_borrow(Option::is_some))
                                && future_output(&returned).is_none()
                                && !lifetime_inputs(signature, substitutions)?
                            {
                                render_anchored_output(&returned, &call, output)?;
                            }
                            writeln!(
                                output,
                                "    let output{annotation} = lend(arguments.arena, || {call});"
                            )
                            .map_err(request::unwritable)?;
                        }
                        if plan.application_invocation().is_some() {
                            argument_list(signature.inputs().len(), 1, output)?;
                            output.write_all(b"));\n").map_err(request::unwritable)?;
                        }
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
                match future_output(returned) {
                    Some(element) if matches!(receiver, Receiver::Owned) => {
                        render_give_task(element, "output", "imported", output)?;
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
            // A static slice of crate values (#1291): a native place that
            // reaches the static at each operation, so its elements are read
            // where they are.
            if let Some(element) = lent_elements(ty) {
                let policy = place::policy(element)?;
                writeln!(
                    output,
                    "    let witness = output_witness().ok_or(Error::Stale)?;
    __rawr_internal::place::slice::lent_by::<{policy}>(&witness, |visit| visit({constant}))
        .map_err(|error| Error::Value(error.to_string()))"
                )
                .map_err(request::unwritable)?;
                return Ok(());
            }
            writeln!(output, "    let output = {constant};").map_err(request::unwritable)?;
            render_give(ty, "output", output)?;
        }
        Operation::Builder { steps } => {
            if let Some(replay) = replay {
                render_builder_inputs(steps, profile, output)?;
                output
                    .write_all(b"    let output = __rawr_profile::Replay::new(move |_context| {\n")
                    .map_err(request::unwritable)?;
                render_builder(steps, true, Some(profile), output)?;
                output
                    .write_all(b"        output\n    });\n")
                    .map_err(request::unwritable)?;
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
        convert::Type::Parameter(_) => {
            fills_with_str(input, signature, substitutions)
                && !static_str(input, signature, substitutions)
        }
        // `&F` the call fills with `str` (`instantiated`).
        convert::Type::Borrowed { mutable: false, .. } => matches!(
            instantiated(input, signature, substitutions),
            Some(convert::Type::Builtin(convert::Builtin::Str))
        ),
        _ => false,
    }
}

/// Whether `input` is a `&T` the bridge makes its `T` for from Rawr's
/// value for the call (`is_made_for_call`), as a `&String` fills jiff's
/// `strftime(&self, format: &'f F)`.
fn lends_made(
    input: &convert::Type,
    signature: &crate::classify::representability::signature::Signature,
    substitutions: &[convert::Type],
) -> bool {
    matches!(input, convert::Type::Borrowed { mutable: false, .. })
        && matches!(
            instantiated(input, signature, substitutions),
            Some(convert::Type::Borrowed { mutable: false, ty }) if is_made_for_call(&ty)
        )
}

/// Whether the instance fills the type parameter `input` with `&str`.
fn fills_with_str(
    input: &convert::Type,
    signature: &crate::classify::representability::signature::Signature,
    substitutions: &[convert::Type],
) -> bool {
    matches!(input, convert::Type::Parameter(_))
        && matches!(
            instantiated(input, signature, substitutions),
            Some(convert::Type::Borrowed { mutable: false, ty })
                if matches!(*ty, convert::Type::Builtin(convert::Builtin::Str))
        )
}

/// Whether `input` is a type parameter the instance fills with `&str` that
/// only a `&'static str` fills, as a literal fills iced's `title(impl
/// Title<State>)`, implemented for `&'static str`: the string lives for the
/// program, as a `&'static str` input's does, and the call lends it to
/// nothing shorter.
fn static_str(
    input: &convert::Type,
    signature: &crate::classify::representability::signature::Signature,
    substitutions: &[convert::Type],
) -> bool {
    let convert::Type::Parameter(index) = input else {
        return false;
    };
    signature
        .parameters()
        .iter()
        .any(|parameter| parameter.index() == *index && parameter.static_str())
        && fills_with_str(input, signature, substitutions)
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
    let encoding = request::unwritable;
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
    let call = String::from_utf8(call).map_err(request::unwritable)?;
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
            && boxed_element(&ty).is_none()
            && counted_element(&ty).is_none()
            && keyed_element(&ty).is_none()
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
    // The region's pins are declared before the output, which goes in the
    // region at the pins' borrow: locals drop in reverse, so the output,
    // whose type may carry that lifetime and drop on an error, drops first.
    writeln!(
        output,
        "    {before}let output = __rawr_internal::scope::Opening::new({name:?}, move |rest| {{{rebound}
        let pins = __rawr_internal::scope::Pins::new();
        let mut region = __rawr_internal::scope::Region::anchored(&pins);
        let made = {call};
        let value = with_arena(|arena| {{
            let mut values: [&mut Dynamic; 0] = [];
            #[allow(unused_variables)]
            let arguments = &mut Arguments {{ values: &mut values, arena }};
            {converted}
        }})
        .map_err(runtime_error)?;
        let result = __rawr_internal::scope::open_joined(&mut region, || rest(value));{back}
        result
    }}){with};
    Ok(Dynamic::from(output))"
    )
    .map_err(encoding)
}

/// The receiver of a trait's method of its impl for `&str`, Rawr's string,
/// at the lifetime rustc borrows it at (#717). An impl for `&'static str`
/// alone takes it pinned per site, as a `&'static str` input is (D694). An
/// output that doesn't borrow it takes it for the call. One that does, as
/// `Widget::_boxed("label")` gives an `Element<'a>`, takes it pinned in the
/// region of the slot the call is made in, and goes there itself; out of
/// any region the string is pinned for the runner's life, as a function's
/// `&str` its output borrows is. Pinning holds the string's shared handle,
/// so no text is copied.
fn render_str_receiver(
    plan: &Plan,
    signature: &crate::classify::representability::signature::Signature,
    substitutions: &[convert::Type],
    constants: &[convert::Constant],
    container_parameters: usize,
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    let encoding = request::unwritable;
    output
        .write_all(b"    let receiver_text = take_primitive::<ImmutableString>(arguments, 0)?;\n")
        .map_err(encoding)?;
    if signature.instance() == Some(&convert::Type::Builtin(convert::Builtin::StaticStr)) {
        return output
            .write_all(b"    let receiver: &'static str = { static PINS: __rawr_internal::scope::Pins = __rawr_internal::scope::Pins::constant(); PINS.pin(receiver_text) };\n")
            .map_err(encoding);
    }
    let returned = substituted(signature.output(), signature, substitutions)?;
    if !carries_lifetime(&returned) {
        return output
            .write_all(b"    let receiver: &str = receiver_text.as_str();\n")
            .map_err(encoding);
    }
    if copied_inputs(signature).is_empty() && !lifetime_inputs(signature, substitutions)? {
        let mut call = Vec::new();
        render_method_head(
            plan.name(),
            Receiver::Owned,
            signature,
            substitutions,
            constants,
            container_parameters,
            &mut call,
        )?;
        argument_list(signature.inputs().len(), 1, &mut call)?;
        call.write_all(b")").map_err(encoding)?;
        let call = String::from_utf8(call).map_err(request::unwritable)?;
        let converted = {
            let _region = InRegion::enter();
            let mut converted = String::new();
            render_to_dynamic(&returned, "made", 0, &mut converted)?;
            converted
        };
        if REGION_ARGUMENTS.get() {
            writeln!(
                output,
                "    if let Some(pins) = region.pins() {{
        let receiver = pins.pin(receiver_text);
        let made = {call};
        return {converted};
    }}"
            )
            .map_err(encoding)?;
        } else {
            writeln!(
                output,
                "    if let Some(anchor) = __rawr_internal::scope::anchor() {{
        return __rawr_internal::scope::with::<__rawr_internal::scope::RegionF, _>(anchor, |region| -> Result<Dynamic, Error> {{
            let pins = region.pins().ok_or(Error::Lent(\"a slot's region\", __rawr_internal::scope::Error::Mismatch))?;
            let receiver = pins.pin(receiver_text);
            let made = {call};
            {converted}
        }})
        .map_err(|error| Error::Lent(\"a slot's region\", error))?;
    }}"
            )
            .map_err(encoding)?;
        }
    }
    output
        .write_all(b"    let receiver: &'static str = static_pins().pin(receiver_text);\n")
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
) -> Result<Ending, request::Error> {
    let encoding = request::unwritable;
    // A script closure, as `table::column(header, |event| ..)` takes with
    // its `&str` header, is made by the call's own slot, outside this path.
    if !functions.is_empty()
        || !mappers.is_empty()
        || signature.parameters().iter().any(|parameter| {
            parameter.kind() == crate::classify::representability::signature::Kind::Callable
        })
    {
        return Ok(Ending::Falls);
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
    let call = String::from_utf8(call).map_err(request::unwritable)?;
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
    let taken = String::from_utf8(taken).map_err(request::unwritable)?;
    render_static_opening(
        signature,
        substitutions,
        &returned,
        &call,
        &taken,
        &pinned,
        output,
    )?;
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
        .map_err(encoding)
        .map(|()| Ending::Falls);
    }
    // Out of any region, such an input is at `'static`, which a string
    // pinned at the slot's lifetime can't join.
    if lifetime_inputs(signature, substitutions)? {
        return Ok(Ending::Falls);
    }
    // A call that would open a frame is made in the region of the frame
    // whose scope it is in, when no slot's region is open: a second `let`
    // joins the first's, so a call takes both at one lifetime, as rustc
    // infers one (#1335).
    let anchor = if instance_opens(signature, substitutions) {
        "joined"
    } else {
        "anchor"
    };
    writeln!(
        output,
        "    if let Some(anchor) = __rawr_internal::scope::{anchor}() {{
{taken}        return __rawr_internal::scope::with::<__rawr_internal::scope::RegionF, _>(anchor, |region| -> Result<Dynamic, Error> {{
            let pins = region.pins().ok_or(Error::Lent(\"a slot's region\", __rawr_internal::scope::Error::Mismatch))?;{pinned}
            let made = {call};
            {converted}
        }})
        .map_err(|error| Error::Lent(\"a slot's region\", error))?;
    }}"
    )
    .map_err(encoding)?;
    // Out of any region, as in a closure a widget calls while it lays out,
    // a type parameter a `&str` fills is a `&'static str`, as rustc types
    // the literal `button("Delete")` takes: the string is pinned for the
    // runner's life and the value is an arena value at `'static`, which may
    // leave the scope it was made in. An item whose own signature borrows
    // its inputs, as `Parser::new(&text)`, keeps its scope frame.
    if signature.scope().is_some() || !instance_opens(signature, substitutions) {
        return Ok(Ending::Falls);
    }
    let mut owned = String::new();
    render_to_dynamic(&returned, "made", 0, &mut owned)?;
    writeln!(
        output,
        "    {{
{taken}        let pins = static_pins();{pinned}
        let made = lend(arguments.arena, || {call});
        return {owned};
    }}"
    )
    .map_err(encoding)?;
    Ok(Ending::Returns)
}

/// Whether the code an adapter's opening wrote returns on every path, so
/// nothing written after it could run.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Ending {
    Falls,
    Returns,
}

/// A call whose own signature borrows its `&str` inputs for its output's
/// life, as `impl<'a> From<&'a str> for Slot<'a, M>` does, given only
/// text the program lends at `'static` (`lent_static`): a string literal,
/// as rustc types `Slot::from("Verified")` a `Slot<'static, M>`. Each
/// string is pinned for the runner's life in the site's pins, which hold
/// one string per text however often the call runs (D694), and the output
/// is a plain arena value, so no scope frame opens for it. A call that
/// copies arrays in, makes a value for a `&T`, or takes a value carrying
/// a lifetime keeps its frame.
fn render_static_opening(
    signature: &crate::classify::representability::signature::Signature,
    substitutions: &[convert::Type],
    returned: &convert::Type,
    call: &str,
    taken: &str,
    pinned: &str,
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    if REGION_ARGUMENTS.get()
        || signature.scope().is_none()
        || !copied_inputs(signature).is_empty()
        || lifetime_inputs(signature, substitutions)?
    {
        return Ok(());
    }
    let mut texts = Vec::new();
    for (position, input) in signature.inputs().iter().enumerate() {
        if lends_made(input, signature, substitutions) {
            return Ok(());
        }
        if lends_str(input, signature, substitutions) {
            texts.push(position.to_string());
        }
    }
    if texts.is_empty() {
        return Ok(());
    }
    let mut owned = String::new();
    render_to_dynamic(returned, "made", 0, &mut owned)?;
    writeln!(
        output,
        "    if lent_static(arguments, &[{texts}]) {{
{taken}        let pins: &'static __rawr_internal::scope::Pins = {{ static PINS: __rawr_internal::scope::Pins = __rawr_internal::scope::Pins::constant(); &PINS }};{pinned}
        let made = lend(arguments.arena, || {call});
        return {owned};
    }}",
        texts = texts.join(", ")
    )
    .map_err(request::unwritable)
}

/// `register_defaults`, which gives the core each imported type's own
/// `Default`, so a script type's derived `Default` builds a field of that
/// type as Rust's derive does: `Tooltip { position: Position::default(),
/// .. }`. Only types with no parameters or lifetimes, whose `Default` the
/// crate implements for the type itself, where rustc proves it for that
/// type (`DefaultProbe`).
fn render_imported_defaults(
    facade: Option<&crate::surface::facade::Plan>,
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    let encoding = request::unwritable;
    output
        .write_all(
            b"fn register_defaults() {\n    #[allow(unused_imports)]\n    use {DefaultAbsent as _, DefaultGiven as _};\n",
        )
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
                    String::from_utf8(identity).map_err(request::unwritable)?
                )
            };
            write!(
                output,
                "    if let Some(made) = (&DefaultProbe::<{path}>::new()).made() {{
        __rawr_internal::host::value::register_imported_default("
            )
            .map_err(encoding)?;
            byte_array(output, ty.identity())?;
            writeln!(
                output,
                ", move || with_arena(|arena| {{
            let mut values: [&mut Dynamic; 0] = [];
            let arguments = &mut Arguments {{ values: &mut values, arena }};
            let output = made();
            {give}
        }}).unwrap_or_else(|error| panic!(\"rawr: `{path}`'s `Default`: {{error:?}}\")));
    }}"
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
    // An opaque type's wrapper takes the inputs of a call in a region of
    // values (`render_scoped_receiver`), not this call's, and a region holds
    // a value boxed only where it is made of the region's values
    // (`declare_wrapper`).
    let Some(family) = written_family(returned) else {
        return Ok(());
    };
    let inserted = format!("insert::<{family}>(made)");
    // The crate values the output borrows are shared with the region's
    // pins, which live as long as the view's borrow (`views_natives`).
    let mut pins = if VIEWED_ARGUMENTS.get() {
        "\n            let pins = region.pins().ok_or(Error::Lent(\"a slot's region\", __rawr_internal::scope::Error::Mismatch))?;".to_owned()
    } else {
        String::new()
    };
    // A closure the call calls back, at the region's lifetime, as
    // `PaneGrid::new`'s view gives each pane's `Content<'a, ..>`.
    pins.extend(REGIONAL_CLOSURES.with_borrow_mut(std::mem::take));
    writeln!(
        output,
        "    if let Some(anchor) = __rawr_internal::scope::anchor() {{
        return __rawr_internal::scope::with::<__rawr_internal::scope::RegionF, _>(anchor, |region| -> Result<Dynamic, Error> {{{pins}
            let made = lend(arguments.arena, || {call});
            Ok::<Dynamic, Error>(Dynamic::from(region.{inserted}))
        }})
        .map_err(|error| Error::Lent(\"a slot's region\", error))?;
    }}"
    )
    .map_err(request::unwritable)
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
        // A native value, a sequence of them, as `column(iterator)`
        // takes a view's `Text`s, or an option of one.
        let native = match &ty {
            convert::Type::Sequence { element, .. } | convert::Type::Option(element) => {
                element.as_ref()
            }
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
    // The take's own borrow of a string or value the frame owns is set
    // aside: the frame borrows what it moves in.
    let mut set_aside = String::new();
    for (position, input) in signature.inputs().iter().enumerate() {
        if lends_str(input, signature, substitutions) {
            set_aside.push_str(&format!("let _ = argument_{position};\n    "));
            rebound.push_str(&format!(
                "\n        let argument_{position} = argument_{position}_owned.as_str();"
            ));
        } else if lends_made(input, signature, substitutions) {
            set_aside.push_str(&format!("let _ = argument_{position};\n    "));
            rebound.push_str(&format!(
                "\n        let argument_{position} = &argument_{position}_made;"
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
            before: set_aside,
            back: String::new(),
            with: String::new(),
        });
    }
    let mut given = Vec::new();
    for (position, element) in &copies {
        given.push(format!(
            "({})?",
            given_back(*position, element, !LENT_ARGUMENTS.get())?
        ));
    }
    Ok(FrameCopies {
        rebound,
        before: format!(
            "{set_aside}let copies = __rawr_internal::scope::Copies::new();\n    let frame_copies = copies.clone();\n    "
        ),
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
    let encoding = request::unwritable;
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
    let container = String::from_utf8(container).map_err(request::unwritable)?;
    let mut call = Vec::new();
    render_method_head(
        plan.name(),
        receiver,
        signature,
        substitutions,
        constants,
        container_parameters,
        &mut call,
    )?;
    argument_list(signature.inputs().len(), 1, &mut call)?;
    call.write_all(b")").map_err(encoding)?;
    let call = String::from_utf8(call).map_err(request::unwritable)?;
    let (access, reach) = match receiver {
        Receiver::Shared => ("shared", family::Reach::Get),
        Receiver::Mutable => ("mutable", family::Reach::GetMut),
        Receiver::Owned => {
            return Err(request::drift(
                "a scope frame's call that takes its receiver",
            ));
        }
    };
    let scoped = match container_family(path, &container) {
        Some(family) => format!(
            "Some(scoped) => __rawr_internal::scope::with::<__rawr_internal::scope::RegionF, _>(scoped.nonce(), |region| -> Result<_, Error> {{
                #[allow(unused_mut)]
                let mut receiver = region.{}.map_err(|error| Error::Lent(scoped.name(), error))?;
                Ok({call})
            }})
            .map_err(|error| Error::Lent(scoped.name(), error))
            .and_then(|made| made),",
            family.call(reach, "scoped")
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
    } = frame_copies(signature, substitutions)?;
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
            Receiver::Shared => ("with_ref", family::Reach::Get),
            _ => ("with", family::Reach::GetMut),
        };
        let scoped = match container_family(path, &container) {
            Some(family) => format!(
                "Some(scoped) => __rawr_internal::scope::{claim}::<__rawr_internal::scope::RegionF, _>(scoped.nonce(), |region| -> Result<Dynamic, Box<__rawr_internal::EvalAltResult>> {{
            #[allow(unused_mut)]
            let mut receiver = region.{}.map_err(|error| runtime_error(Error::Lent(scoped.name(), error)))?;
            let made = {call};
            {frame}
        }})
        .map_err(|error| runtime_error(Error::Lent(scoped.name(), error)))?,",
                family.call(family_reach, "scoped")
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
/// lifetime stays there. The receiver's family is `operand`'s, for an
/// opaque type's method, else its container's. Nothing for another
/// receiver type.
#[allow(clippy::too_many_arguments)]
fn render_scoped_receiver(
    plan: &Plan,
    path: &str,
    receiver: Receiver,
    signature: &crate::classify::representability::signature::Signature,
    substitutions: &[convert::Type],
    constants: &[convert::Constant],
    container_parameters: usize,
    operand: Option<Operand>,
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    let encoding = request::unwritable;
    let (family, call) = match operand {
        Some(Operand { family, call }) => (family, call),
        None => {
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
            let container = String::from_utf8(container).map_err(request::unwritable)?;
            match container_family(path, &container) {
                Some(family) => (family, None),
                None => return Ok(()),
            }
        }
    };
    // A function's first input is the receiver, and its others are the
    // arguments (`scoped_function_call`).
    let skip = usize::from(matches!(plan.operation(), Operation::Function { .. }));
    // A boxed receiver's call is its trait's method (`family::Ops`), which
    // gives its output as the region holds it.
    let (call, ops) = match call {
        Some(call) => (call, true),
        None => (
            scoped_receiver_call(
                plan,
                receiver,
                signature,
                substitutions,
                constants,
                container_parameters,
            )?,
            false,
        ),
    };
    let reach = match receiver {
        Receiver::Owned => family::Reach::Take,
        Receiver::Shared => family::Reach::Get,
        Receiver::Mutable => family::Reach::GetMut,
    };
    let reached = family.call(reach, "scoped");
    let returned = substituted(signature.output(), signature, substitutions)?;
    // An opaque output Rust code can't name is made through its wrapper
    // (`family`), whose output the region holds, and a trait method's is
    // what the region holds.
    let made = match family_of(&returned) {
        Some(family) if let Some(made) = family.made() => Some((made, family)),
        Some(family) if ops && family.named().is_some() => Some((call.clone(), family)),
        _ => None,
    };
    let (call, converted) = match made {
        Some((made, family)) => (
            made,
            format!(
                "Ok::<Dynamic, Error>(Dynamic::from(region.{}))",
                family.call(family::Reach::Insert, "output")
            ),
        ),
        None => {
            let _region = InRegion::enter();
            let mut converted = String::new();
            render_to_dynamic(&returned, "output", 0, &mut converted)?;
            // What the call changed of a `&mut` array it took goes back
            // after its value, as the arena's call gives it (#913).
            let copied = copied_back(signature, substitutions)?;
            if !copied.is_empty() {
                converted = format!(
                    "{{ let given = ({converted})?; Ok::<Dynamic, Error>(Dynamic::from_array(vec![given{copied}])) }}"
                );
            }
            (call, converted)
        }
    };
    // A `&str` the call lends the receiver, as `label(impl IntoFragment<'a>)`
    // takes one, is pinned in the region the receiver is in, at its
    // lifetime, as a slot's region pins its strings.
    let mut pinned = String::new();
    for (position, input) in signature.inputs().iter().enumerate().skip(skip) {
        let position = position - skip;
        if lends_str(input, signature, substitutions) {
            pinned.push_str(&format!(
                "\n        let argument_{position} = region.pins().ok_or(Error::Lent(scoped.name(), __rawr_internal::scope::Error::Mismatch))?.pin(argument_{position}_owned.clone());"
            ));
        }
    }
    // Out of the region, the arguments are the arena's, at `'static`: one
    // of a type invariant in its lifetime can't join a receiver of the
    // region, so a call of the two is the region branch's alone
    // (`region_inputs`), as pane_grid's `content.title_bar(title_bar)` is.
    if !REGION_ARGUMENTS.get() {
        for input in signature.inputs().iter().skip(skip) {
            let ty = substituted(input, signature, substitutions)?;
            if matches!(ty, convert::Type::Nominal { .. })
                && carries_lifetime(&ty)
                && !shortens(&ty)
            {
                return Ok(());
            }
        }
    }
    // In the region the adapter's body already runs in, which an argument
    // of the call is in too.
    if REGION_ARGUMENTS.get() {
        return writeln!(
            output,
            "    if let Some(scoped) = scoped_value(arguments, 0) {{
        #[allow(unused_mut)]
        let mut receiver = region.{reached}.map_err(|error| Error::Lent(scoped.name(), error))?;{pinned}
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
            let mut receiver = region.{reached}.map_err(|error| Error::Lent(scoped.name(), error))?;{pinned}
            let output = {call};
            {converted}
        }})
        .map_err(|error| Error::Lent(scoped.name(), error))?;
    }}"
    )
    .map_err(encoding)
}

/// The call `render_scoped_receiver` makes, of `receiver` and the
/// arguments `argument_0`, ...
fn scoped_receiver_call(
    plan: &Plan,
    receiver: Receiver,
    signature: &crate::classify::representability::signature::Signature,
    substitutions: &[convert::Type],
    constants: &[convert::Constant],
    container_parameters: usize,
) -> Result<String, request::Error> {
    let encoding = request::unwritable;
    let mut call = Vec::new();
    render_method_head(
        plan.name(),
        receiver,
        signature,
        substitutions,
        constants,
        container_parameters,
        &mut call,
    )?;
    argument_list(signature.inputs().len(), 1, &mut call)?;
    call.write_all(b")").map_err(encoding)?;
    call.write_all(lent_copy(signature).as_bytes())
        .map_err(encoding)?;
    String::from_utf8(call).map_err(request::unwritable)
}

/// Record the bounds of the opaque type `plan` gives, when it gives one
/// whose bounds Rust can write, for the shapes of the types that hold it
/// (`scoped_shape`).
fn record_bounds(plan: &Plan) -> Result<(), request::Error> {
    let (Operation::Method {
        signature,
        substitutions,
        ..
    }
    | Operation::Function {
        signature,
        substitutions,
        ..
    }) = plan.operation()
    else {
        return Ok(());
    };
    let returned = substituted(signature.output(), signature, substitutions)?;
    let convert::Type::Nominal { definition, .. } = &returned else {
        return Ok(());
    };
    if !is_opaque(definition) {
        return Ok(());
    }
    if let Some(key) = scoped_type(&returned)
        && let Some(bounds) = opaque_bounds(signature, substitutions)?
    {
        family::bound(key, bounds);
    }
    Ok(())
}

/// The value at the first runtime position of `plan`, spelled at `'x`, and
/// how the call takes it: a method's receiver, at its impl's instance or
/// its container's arguments, or a function's first input. None when it
/// has no such spelling, or the call has none.
fn first_operand(plan: &Plan) -> Result<Option<(String, Receiver)>, request::Error> {
    match plan.operation() {
        Operation::Method {
            receiver,
            signature,
            substitutions,
            container_parameters,
            ..
        } => {
            if let Some(instance) = signature.instance() {
                let instance = substituted(instance, signature, substitutions)?;
                return Ok(scoped_type(&instance).map(|key| (key, *receiver)));
            }
            // A method's path is its container's.
            let container = rust_path(plan);
            let Some(identity) =
                PATH_IDENTITIES.with_borrow(|identities| identities.get(&container).copied())
            else {
                return Ok(None);
            };
            let Some(mut text) = TYPE_PATHS.with_borrow(|paths| paths.get(&identity).cloned())
            else {
                return Ok(None);
            };
            let lifetimes = SCOPED_TYPES.with_borrow(|types| types.get(&identity).copied());
            let mut parts =
                vec!["'x".to_owned(); lifetimes.map_or(0, |known| known.count as usize)];
            for argument in substitutions.iter().take(*container_parameters) {
                let Some(spelled) = scoped_spelling(argument) else {
                    return Ok(None);
                };
                parts.push(spelled);
            }
            if lifetimes.is_some() || !parts.is_empty() {
                text = format!("{text}<{}>", parts.join(", "));
            }
            Ok(Some((at_static(&text), *receiver)))
        }
        Operation::Function {
            signature,
            substitutions,
            functions,
            ..
        } => {
            if functions.iter().any(|function| function.position() == 0) {
                return Ok(None);
            }
            let Some(input) = signature.inputs().first() else {
                return Ok(None);
            };
            Ok(match substituted(input, signature, substitutions)? {
                convert::Type::Borrowed { mutable, ty } => {
                    let receiver = if mutable {
                        Receiver::Mutable
                    } else {
                        Receiver::Shared
                    };
                    scoped_type(&ty).map(|key| (key, receiver))
                }
                input => scoped_type(&input).map(|key| (key, Receiver::Owned)),
            })
        }
        _ => Ok(None),
    }
}

/// The call of a function `plan` whose first input is `receiver` and whose
/// others are `argument_0`, ..., as a method of the trait a region holds
/// that input boxed through (`declare_ops`) makes it.
fn scoped_function_call(plan: &Plan) -> Result<String, request::Error> {
    let Operation::Function {
        signature,
        substitutions,
        constants,
        container_parameters,
        associated,
        ..
    } = plan.operation()
    else {
        return Err(request::drift("a function call on a boxed region value"));
    };
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
    let mut inputs = vec!["receiver".to_owned()];
    inputs
        .extend((1..signature.inputs().len()).map(|position| format!("argument_{}", position - 1)));
    write!(call, "({})", inputs.join(", ")).map_err(request::unwritable)?;
    String::from_utf8(call).map_err(request::unwritable)
}

/// Declare how a region holds the output of `plan` when its type holds an
/// opaque type and carries a lifetime, and the call takes a region's value,
/// and give the output's type when the declaration is new. A method on a
/// receiver of a family struct whose output is an opaque type makes it
/// through a wrapper, whose inputs are the receiver, at the type its family
/// holds, and each argument at `'x`, and whose bounds are the opaque
/// type's. Any other such output is held boxed (`family::Ops`), as no
/// wrapper over another compiles and Rust code can't name a type that
/// holds an opaque type: through a trait implemented for the output's shape
/// (`scoped_shape`). None for another call, or one whose types have no Rust
/// spelling.
fn declare_wrapper(plan: &Plan) -> Result<Option<convert::Type>, request::Error> {
    let (Operation::Method {
        signature,
        substitutions,
        ..
    }
    | Operation::Function {
        signature,
        substitutions,
        ..
    }) = plan.operation()
    else {
        return Ok(None);
    };
    let returned = substituted(signature.output(), signature, substitutions)?;
    let convert::Type::Nominal { definition, .. } = &returned else {
        return Ok(None);
    };
    if !holds_opaque(&returned) || !carries_lifetime(&returned) {
        return Ok(None);
    }
    let Some(key) = scoped_type(&returned) else {
        return Ok(None);
    };
    if family::Ops::held(&key).is_some() || family::of(&key).is_some() {
        return Ok(None);
    }
    let regional = match plan.operation() {
        Operation::Method {
            receiver: receiver @ Receiver::Owned,
            constants,
            container_parameters,
            ..
        } => {
            let path = rust_path(plan);
            let mut container = Vec::new();
            render_container_path(
                &path,
                signature,
                substitutions,
                constants,
                *container_parameters,
                true,
                &mut container,
            )?;
            let container = String::from_utf8(container).map_err(request::unwritable)?;
            let named = container_family(&path, &container)
                .as_ref()
                .and_then(family::Family::named)
                .and_then(family_type);
            let receiving = named.is_some();
            if let Some(held) = named
                && is_opaque(definition)
                && let Some(bounds) = opaque_bounds(signature, substitutions)?
            {
                return declare_given(plan, *receiver, held, key, bounds, returned);
            }
            receiving
                || first_operand(plan)?.is_some_and(|(operand, _)| {
                    family::Ops::held(&operand).is_some() || family::of(&operand).is_some()
                })
                || !region_inputs(plan)?.is_empty()
        }
        Operation::Method { .. } => false,
        _ => {
            first_operand(plan)?.is_some_and(|(operand, receiver)| {
                matches!(receiver, Receiver::Owned) && family::Ops::held(&operand).is_some()
            }) || !region_inputs(plan)?.is_empty()
        }
    };
    if !regional {
        return Ok(None);
    }
    let Some(shape) = scoped_shape(&returned) else {
        return Ok(None);
    };
    let (held, _) = family::Ops::declare(key, shape);
    family(held, BOXED_LIFETIMES);
    Ok(Some(returned))
}

/// Declare the wrapper of the method `plan`, whose output, of the opaque
/// type spelled `key` with `bounds`, is made of a receiver of a family
/// struct holding `held`, and give the output's type. None when an input
/// has no Rust spelling.
fn declare_given(
    plan: &Plan,
    receiver: Receiver,
    held: String,
    key: String,
    bounds: String,
    returned: convert::Type,
) -> Result<Option<convert::Type>, request::Error> {
    let Operation::Method {
        signature,
        substitutions,
        constants,
        container_parameters,
        ..
    } = plan.operation()
    else {
        return Ok(None);
    };
    // An input of an opaque type has no Rust spelling.
    let mut inputs = vec![("receiver".to_owned(), held)];
    for (position, input) in signature.inputs().iter().enumerate() {
        let input = substituted(input, signature, substitutions)?;
        let Some(ty) = scoped_type(&input).filter(|_| !holds_opaque(&input)) else {
            return Ok(None);
        };
        inputs.push((format!("argument_{position}"), ty));
    }
    let call = scoped_receiver_call(
        plan,
        receiver,
        signature,
        substitutions,
        constants,
        *container_parameters,
    )?;
    family::Wrapper::new(inputs, call, bounds).declare(key);
    Ok(Some(returned))
}

/// What a region's boxed value of an opaque type (`family::Ops`) is, for its
/// family struct: it carries one lifetime, in which it is invariant, and
/// has none of `Debug`, `Clone` and `PartialEq`.
const BOXED_LIFETIMES: Lifetimes = Lifetimes {
    count: 1,
    covariant: false,
    debug: false,
    clone: false,
    partial_eq: false,
};

/// Each call in `plans` on `ty`, a type holding an opaque type that a
/// region holds boxed (`family::Ops`), as a method of the trait it is held
/// through: a method on it, or a function that takes it first. Its receiver
/// is the value itself, its inputs are at `'x`, and its output is boxed in
/// turn when that is such a type too. A call whose types have no Rust
/// spelling, or whose output is made through a wrapper, has none.
fn declare_ops(plans: &[Plan], ty: &convert::Type) -> Result<(), request::Error> {
    let Some(key) = scoped_type(ty) else {
        return Ok(());
    };
    let Some(opaque) = first_opaque(ty) else {
        return Ok(());
    };
    for (consumer, plan) in opaque_consumers(plans, opaque)?.into_iter().enumerate() {
        if !matches!(opaque_operand(plan)?, Some((0, ..))) {
            continue;
        }
        let Some((operand, receiver)) = first_operand(plan)? else {
            continue;
        };
        if operand != key {
            continue;
        }
        let (signature, substitutions, skip, call) = match plan.operation() {
            Operation::Method {
                signature,
                substitutions,
                constants,
                container_parameters,
                ..
            } => (
                signature,
                substitutions,
                0,
                scoped_receiver_call(
                    plan,
                    receiver,
                    signature,
                    substitutions,
                    constants,
                    *container_parameters,
                )?,
            ),
            Operation::Function {
                signature,
                substitutions,
                ..
            } => (signature, substitutions, 1, scoped_function_call(plan)?),
            _ => continue,
        };
        // A call with an input of an opaque type, or of no Rust spelling,
        // has no method.
        let mut inputs = Vec::new();
        for (position, input) in signature.inputs().iter().enumerate().skip(skip) {
            let input = substituted(input, signature, substitutions)?;
            if let Some(rust) = scoped_type(&input).filter(|_| !holds_opaque(&input)) {
                inputs.push((format!("argument_{}", position - skip), rust));
            }
        }
        if inputs.len() + skip != signature.inputs().len() {
            continue;
        }
        let returned = substituted(signature.output(), signature, substitutions)?;
        let (output, call) = match scoped_type(&returned) {
            Some(text) if let Some(held) = family::Ops::held(&text) => {
                (held, format!("Box::new({call})"))
            }
            Some(text) if family::of(&text).is_none() && !holds_opaque(&returned) => (text, call),
            _ => continue,
        };
        family::Ops::add(
            &key,
            family::Method::new(consumer, receiver, inputs, output, call),
        );
    }
    Ok(())
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
    let encoding = request::unwritable;
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
        let mut call = Vec::new();
        render_method_head(
            plan.name(),
            receiver,
            signature,
            substitutions,
            constants,
            container_parameters,
            &mut call,
        )?;
        call
    };
    argument_list(signature.inputs().len(), 1, &mut call)?;
    call.write_all(b")").map_err(encoding)?;
    call.write_all(lent_copy(signature).as_bytes())
        .map_err(encoding)?;
    let call = String::from_utf8(call).map_err(request::unwritable)?;
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
    let container = String::from_utf8(container).map_err(request::unwritable)?;
    render_scoped_receiver(
        plan,
        path,
        receiver,
        signature,
        substitutions,
        constants,
        container_parameters,
        None,
        output,
    )?;
    let access = if mutable { "mutable" } else { "shared" };
    let borrow = if mutable { "&mut " } else { "&" };
    // A value that lives beside what it borrows (`lent_cell`) is reached
    // through its proxy, at the type `_w` names.
    if let Some(proxy) = OPAQUE_CELL.get()
        && container == "_"
    {
        let index = proxy.index;
        let mut arguments = Vec::new();
        argument_list(signature.inputs().len(), 1, &mut arguments)?;
        let arguments = String::from_utf8(arguments).map_err(request::unwritable)?;
        let reach = if proxy.derives {
            "derive"
        } else if mutable {
            "mutable"
        } else {
            "shared"
        };
        let reach = format!("receiver.{reach}(|receiver| receiver.__rawr_{index}({arguments}))");
        return writeln!(
            output,
            "    let receiver = native_handle(arguments, 0, false)?;
    let output = {access}_like(arguments.arena, receiver, _w, |receiver| {reach})?;"
        )
        .map_err(encoding);
    }
    // Any other value of an opaque type in its table, as `wrap(view(
    // &items))` gives (#856), is reached at that type itself.
    if container == "_" && OPAQUE_WITNESSED.with_borrow(Option::is_some) {
        return writeln!(
            output,
            "    let receiver = native_handle(arguments, 0, false)?;
    let output = {access}_like(arguments.arena, receiver, _w, |receiver| {call})?;"
        )
        .map_err(encoding);
    }
    // A method that calls a script closure back, as `Frame::with_save(&mut
    // self, f: impl FnOnce(&mut Frame) -> R)` does, holds its receiver out
    // of its slot and lends the arena while it runs, as a function does
    // (D551.6), so the closure's own native calls reach the arena.
    let calls_back = signature.inputs().iter().any(|ty| {
        callable_parameter(ty, signature)
            || matches!(ty, convert::Type::Option(inner) if callable_parameter(inner, signature))
    });
    let (arena_call, lent_call) = if calls_back {
        (
            format!(
                "arguments.arena.held::<{container}, _>(receiver, |arena, receiver| lend(arena, || {call}))?"
            ),
            format!("lend(arguments.arena, || {call})"),
        )
    } else {
        (
            format!("arguments.arena.{access}::<{container}, _>(receiver, |receiver| {call})?"),
            call.clone(),
        )
    };
    let mut taken = Vec::new();
    argument_list(signature.inputs().len(), 1, &mut taken)?;
    let taken = String::from_utf8(taken).map_err(request::unwritable)?;
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
                    output = Some({lent_call});
                }}
            }})
            .map_err(|code| Error::Lent(lent.name(), __rawr_internal::scope::Error::from_code(code)))?;
            output.ok_or(Error::Lent(lent.name(), __rawr_internal::scope::Error::Busy))?
        }}
        None => {{
            let receiver = native_handle(arguments, 0, false)?;
            {arena_call}
        }}
    }};"
    )
    .map_err(encoding)
}

/// The element of the `&[T]` a `&self` method that takes nothing else lends
/// from its receiver, as iced's `combo_box::State::options` does (#687),
/// when the slice is lent in place: its elements are text or crate values,
/// which a native place reaches where they are. A slice of scalars is
/// still a copy, which Rawr's array patterns match (D558 slice 2).
fn lent_slice(
    signature: &crate::classify::representability::signature::Signature,
    substitutions: &[convert::Type],
) -> Option<convert::Type> {
    if !signature.inputs().is_empty() || signature.guard().is_some() || signature.scope().is_some()
    {
        return None;
    }
    let convert::Type::Borrowed { mutable: false, ty } =
        instantiated(signature.output(), signature, substitutions)?
    else {
        return None;
    };
    let convert::Type::Sequence {
        element,
        owned: false,
        length: None,
    } = *ty
    else {
        return None;
    };
    (matches!(
        *element,
        convert::Type::Builtin(convert::Builtin::String) | convert::Type::Nominal { .. }
    ) && !is_value(&element)
        && !is_duration(&element)
        && !is_facade_value(&element)
        && place::available(&element))
    .then_some(*element)
}

/// The element of a `&[T]` of crate values Rawr reaches in place, as a
/// page's `static BLOCKS: &[Block]` or a block's `children: &'static
/// [Block]` (#1291): elements that are neither Rawr's own values nor
/// borrow anything, which a native place lends one at a time.
fn lent_elements(ty: &convert::Type) -> Option<&convert::Type> {
    let convert::Type::Borrowed { mutable: false, ty } = ty else {
        return None;
    };
    let convert::Type::Sequence {
        element,
        owned: false,
        length: None,
    } = &**ty
    else {
        return None;
    };
    (matches!(**element, convert::Type::Nominal { .. })
        && !plain_value(element)
        && !is_value(element)
        && !is_duration(element)
        && !is_facade_value(element)
        && place::available(element))
    .then_some(&**element)
}

/// A `&self` method's `&[T]` lent in place (#687): a native place that
/// reaches the slice by claiming the receiver and calling the method again
/// at each operation, so `.len()`, indexing and iteration read the native
/// elements where they are, as Rust does, and none is copied.
#[allow(clippy::too_many_arguments)]
fn render_lent_slice(
    plan: &Plan,
    path: &str,
    signature: &crate::classify::representability::signature::Signature,
    substitutions: &[convert::Type],
    constants: &[convert::Constant],
    container_parameters: usize,
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    let element = lent_slice(signature, substitutions)
        .ok_or_else(|| request::drift("a lent slice's element"))?;
    let policy = place::policy(&element)?;
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
    let container = String::from_utf8(container).map_err(request::unwritable)?;
    let mut call = Vec::new();
    render_method_head(
        plan.name(),
        Receiver::Shared,
        signature,
        substitutions,
        constants,
        container_parameters,
        &mut call,
    )?;
    call.write_all(b")").map_err(request::unwritable)?;
    let call = String::from_utf8(call).map_err(request::unwritable)?;
    writeln!(
        output,
        "    let witness = output_witness().ok_or(Error::Stale)?;
    let lender = match lent_value(arguments, 0) {{
        Some(lent) => Lender::Lent(lent),
        None => Lender::Handle(native_handle(arguments, 0, false)?),
    }};
    __rawr_internal::place::slice::lent_by::<{policy}>(&witness, move |visit| {{
        reach_lender::<{container}>(&lender, &mut |receiver| visit({call}))
    }})
    .map_err(|error| Error::Value(error.to_string()))"
    )
    .map_err(request::unwritable)
}

/// `render_guard` for a receiver in a scope frame's region (D558 tier 1),
/// as quick-xml's `reader.config_mut()` is on a `Reader<&[u8]>` its
/// `Reader::from_str` frame holds. A `Deref` guard is reached in the
/// region for its one copy. A `DerefMut` guard's method is called once, and
/// the region keeps the guard beside the receiver (`scope::Region::yoke`),
/// where each access of its place reaches it (#1333), as the arena keeps a
/// guard of its own values (`Arena::yoke`). Nothing for a receiver type
/// that carries no lifetime.
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
    let encoding = request::unwritable;
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
    let container = String::from_utf8(container).map_err(request::unwritable)?;
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
    let inputs = signature.inputs().len();
    method.write_all(b"(").map_err(encoding)?;
    argument_list(inputs, 1, &mut method)?;
    method.write_all(b")").map_err(encoding)?;
    let method = String::from_utf8(method).map_err(request::unwritable)?;
    match guard {
        // A receiver in a region lends no optional shared borrow yet.
        Guard::OptionShared => Ok(()),
        Guard::Shared => {
            let copy = copy_macro(&target);
            let mut give = Vec::new();
            render_give(&target, "output", &mut give)?;
            let give = String::from_utf8(give).map_err(request::unwritable)?;
            writeln!(
                output,
                "    if let Some(scoped) = scoped_value(arguments, 0) {{
        return __rawr_internal::scope::with::<__rawr_internal::scope::RegionF, _>(scoped.nonce(), |region| -> Result<_, Error> {{
            let receiver = region.{reached}.map_err(|error| Error::Lent(scoped.name(), error))?;
            let mut guard = {method};
            let _ = &mut guard;
            let output = {copy}!(&*guard)?;
            {give}
        }})
        .map_err(|error| Error::Lent(scoped.name(), error))
        .and_then(|output| output);
    }}",
                reached = family.call(family::Reach::GetMut, "scoped"),
            )
            .map_err(encoding)
        }
        Guard::Mutable | Guard::OptionMutable => {
            let optional = matches!(guard, Guard::OptionMutable);
            let yoke = family.call(
                family::Reach::Yoke,
                &format!("scoped, |receiver| {}", yoked_guard(&method, optional)),
            );
            writeln!(
                output,
                "    if let Some(scoped) = scoped_value(arguments, 0) {{
        let end = __rawr_internal::scope::with::<__rawr_internal::scope::RegionF, _>(scoped.nonce(), |region| region.{yoke})
            .and_then(|end| end)
            .map_err(|error| Error::Lent(scoped.name(), error))?;"
            )
            .map_err(encoding)?;
            render_yoked_place(
                &target,
                optional,
                inputs,
                "end.held()",
                &|body| {
                    format!(
                        "end.with(|guard| {body}).map_err(|error| Error::Lent(scoped.name(), error))"
                    )
                },
                output,
            )?;
            output.write_all(b"    }\n").map_err(encoding)
        }
    }
}

/// The closure body that makes the guard a yoke keeps of `call`'s output
/// (`Arena::yoke`, `scope::Region::yoke`): the guard, or for an
/// `Option<&mut T>` method (`optional`) the one its `Some` holds.
fn yoked_guard(call: &str, optional: bool) -> String {
    if optional {
        format!("match {call} {{ Some(guard) => Some(Box::new(guard)), None => None }}")
    } else {
        format!("Some(Box::new({call}))")
    }
}

/// The place of a guard a yoke keeps, as `render_guard` gives it, after
/// `let end = ..;`: `held` is whether the yoke keeps one, and `with(body)`
/// the expression that runs `body` on its target, `guard`, giving
/// `Result<_, Error>`. A read copies the target, a store assigns it, and an
/// update (`op=`, D554.7(f)) reads, has the script change the copy and
/// stores the result. An `optional` guard gives `None` when the method
/// gave none, and `Some` of the place otherwise. It returns from the
/// adapter.
fn render_yoked_place(
    target: &convert::Type,
    optional: bool,
    inputs: usize,
    held: &str,
    with: &dyn Fn(&str) -> String,
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    let encoding = request::unwritable;
    let element = if optional {
        let convert::Type::Option(element) = target else {
            return Err(request::drift("an optional borrow's output is no option"));
        };
        &**element
    } else {
        target
    };
    let copy = copy_macro(element);
    let mut given = Vec::new();
    render_give(element, "output", &mut given)?;
    let given = String::from_utf8(given).map_err(request::unwritable)?;
    // A read is attested as the guard's call's output is.
    let give = format!(
        "let read: Result<Dynamic, Error> = {{\n{given}}};\nread.and_then(|value| attest_read(&mut *arguments.arena, value, &witness))"
    );
    // The stored value is the local after the inputs.
    let mut take = Vec::new();
    render_take(inputs, 0, element, &mut take)?;
    let take = String::from_utf8(take).map_err(request::unwritable)?;
    let (witness, placed) = if optional {
        writeln!(
            output,
            "    if !{held} {{\n        return Ok(__rawr_internal::option::none());\n    }}"
        )
        .map_err(encoding)?;
        (
            "output_witness().and_then(|witness| witness.argument_witness(0))",
            "__rawr_internal::option::some(Dynamic::from(place))",
        )
    } else {
        ("output_witness()", "Dynamic::from(place)")
    };
    let read = with(&format!("{copy}!(&*guard)"));
    let store = with(&format!("*guard = argument_{inputs}"));
    writeln!(
        output,
        "    let witness = {witness};
    let place = __rawr_internal::island::place::Place::new(move |access: __rawr_internal::island::place::Access<'_>| {{
        use __rawr_internal::island::place::Access;
        let reach = |mut stored: Option<Dynamic>| {{
            with_arena(|arena| {{
                let mut values: Vec<&mut Dynamic> = stored.iter_mut().collect();
                let arguments = &mut Arguments {{ values: values.as_mut_slice(), arena }};
                if arguments.values.is_empty() {{
                    let output = {read}??;
                    {give}
                }} else {{
{take}
                    {store}?;
                    Ok(Dynamic::UNIT)
                }}
            }})
            .map_err(runtime_error)
        }};
        match access {{
            Access::Read => reach(None),
            Access::Store(value) => reach(Some(value)),
            Access::Update(apply) => {{
                let value = reach(None)?;
                reach(Some(apply(value)?))
            }}
        }}
    }});
    return Ok({placed});"
    )
    .map_err(encoding)
}

/// The Rust path of `target` when it is one of std's types Rawr's std
/// holds that has no `Clone`, as `io::Error`: a borrow of one crosses as
/// the borrow, never a copy (`render_borrowed_std`).
fn borrowed_std(target: &convert::Type) -> Option<String> {
    let convert::Type::Nominal { definition, .. } = target else {
        return None;
    };
    std_value(target)
        .filter(|_| !crate::surface::identity::Definition::std_clone(definition.as_bytes()))
}

/// A `&self` method that takes nothing else and gives a `&T` of one of
/// std's types with no `Clone` (`borrowed_std`), as a crate's `fn
/// error(&self) -> &io::Error` (#680): a script value that borrows the
/// receiver's value. Each use leases the receiver from its slot and calls
/// the method again, so the program reads the error where the receiver
/// holds it, as Rust does, and none is copied.
#[allow(clippy::too_many_arguments)]
fn render_borrowed_std(
    plan: &Plan,
    path: &str,
    rust: &str,
    signature: &crate::classify::representability::signature::Signature,
    substitutions: &[convert::Type],
    constants: &[convert::Constant],
    container_parameters: usize,
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
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
    let container = String::from_utf8(container).map_err(request::unwritable)?;
    let mut method = format!("owner.{}", plan.name()).into_bytes();
    render_item_turbofish(
        signature,
        substitutions,
        constants,
        container_parameters,
        &mut method,
    )?;
    let method = String::from_utf8(method).map_err(request::unwritable)?;
    writeln!(
        output,
        "    let receiver = native_handle(arguments, 0, false)?;
    fn project(owner: &{container}) -> &{rust} {{
        {method}()
    }}
    let borrow: __rawr_internal::crossing::Borrow<{rust}> = std::sync::Arc::new(move |arena: Option<&mut dyn Any>| {{
        let lease = |arena: &mut Arena| arena.lease::<{container}>(receiver.clone());
        let held = match arena {{
            Some(arena) => arena.downcast_mut::<Arena>().ok_or(Error::TypeMismatch).and_then(lease),
            None => with_arena(lease),
        }}
        .map_err(runtime_error)?;
        Ok(Box::new(Projected(held, project)) as __rawr_internal::crossing::Lending<{rust}>)
    }});
    <{rust} as __rawr_internal::crossing::Std>::borrowed(borrow).ok_or(Error::TypeMismatch)"
    )
    .map_err(request::unwritable)
}

/// A method whose Rust output is a borrow guard, as Freya's `State::read`
/// and `State::write` give. A `Deref` guard gives a copy of its target,
/// inside the one call that holds the receiver. A `DerefMut` guard gives a
/// place (`island::place::Place`), and an `Option<&mut T>` `Some` of one or
/// `None`: the method is called once, its guard is kept with the
/// receiver's value in the slot's home (`Arena::yoke`, #1325, #1334) or
/// beside it in its region (`render_scoped_guard`), and each access
/// reaches it there (`render_yoked_place`). The guard ends when the
/// place's last copy drops or the receiver is next reached, as rustc ends
/// the borrow. The method's inputs are taken first, so they are owned
/// values the call borrows.
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
    let encoding = request::unwritable;
    let borrow = if matches!(receiver, Receiver::Mutable) {
        "arguments.arena.mutable"
    } else {
        "arguments.arena.shared"
    };
    let target = substituted(signature.output(), signature, substitutions)?;
    if matches!(guard, Guard::Shared)
        && matches!(receiver, Receiver::Shared)
        && signature.inputs().is_empty()
        && let Some(rust) = borrowed_std(&target)
    {
        return render_borrowed_std(
            plan,
            path,
            &rust,
            signature,
            substitutions,
            constants,
            container_parameters,
            output,
        );
    }
    let copy = copy_macro(&target);
    // The method's inputs, given as they were taken: it is called once.
    let inputs = signature.inputs().len();
    let mut taken = Vec::new();
    argument_list(inputs, 1, &mut taken)?;
    let taken = String::from_utf8(taken).map_err(request::unwritable)?;
    // `arena.shared::<Receiver, _>(handle, |receiver| { let mut guard =
    // receiver.method(inputs); ` through `access`, whose closure takes
    // `taking`.
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
        write!(output, "({taken}); let _ = &mut guard; ").map_err(encoding)
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
    // A receiver native code lent (D558.3), as iced lends `draw` its
    // `&Theme`, whose `palette()` the program reads (#954): the method is
    // called through its accessor, and the copy goes out of it.
    if matches!(guard, Guard::Shared)
        && matches!(receiver, Receiver::Shared)
        && signature.inputs().is_empty()
    {
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
        let container = String::from_utf8(container).map_err(request::unwritable)?;
        let mut turbofish = Vec::new();
        render_item_turbofish(
            signature,
            substitutions,
            constants,
            container_parameters,
            &mut turbofish,
        )?;
        let turbofish = String::from_utf8(turbofish).map_err(request::unwritable)?;
        writeln!(
            output,
            "    if let Some(lent) = lent_value(arguments, 0) {{
        let access: fn(u64, &mut dyn FnMut(&{container})) -> Result<(), u8> = lent
            .accessor(false).map_err(|error| Error::Lent(lent.name(), error))?;
        let mut copied = None;
        access(lent.nonce().bits(), &mut |receiver| {{ let mut guard = receiver.{name}{turbofish}(); let _ = &mut guard; copied = Some({copy}!(&*guard)); }})
            .map_err(|code| Error::Lent(lent.name(), __rawr_internal::scope::Error::from_code(code)))?;
        let output = copied.ok_or(Error::Lent(lent.name(), __rawr_internal::scope::Error::Busy))??;
        return {{",
            name = plan.name()
        )
        .map_err(encoding)?;
        render_give(&target, "output", output)?;
        writeln!(output, "    }};\n    }}").map_err(encoding)?;
    }
    writeln!(
        output,
        "    let receiver = native_handle(arguments, 0, false)?;"
    )
    .map_err(encoding)?;
    match guard {
        Guard::Shared => {
            output.write_all(b"    let output = ").map_err(encoding)?;
            call(borrow, "receiver", "receiver", output)?;
            output
                .write_all(format!("{copy}!(&*guard) }})??;\n").as_bytes())
                .map_err(encoding)?;
            render_give(&target, "output", output)
        }
        // `Some` of a copy of the target, or `None`.
        Guard::OptionShared => {
            output.write_all(b"    let output = ").map_err(encoding)?;
            call(borrow, "receiver", "receiver", output)?;
            output
                .write_all(
                    format!("guard.map(|target| {copy}!(&*target)).transpose() }})??;\n")
                        .as_bytes(),
                )
                .map_err(encoding)?;
            render_give(&target, "output", output)
        }
        // The method is called once: its guard is kept with the receiver's
        // value (`Arena::yoke`), and each access of the place reaches it
        // there (#1325). An `Option<&mut T>` gives `Some` of the place, or
        // `None` (#1334).
        Guard::Mutable | Guard::OptionMutable => {
            let optional = matches!(guard, Guard::OptionMutable);
            output
                .write_all(b"    let end = arguments.arena.yoke::<")
                .map_err(encoding)?;
            render_container_path(
                path,
                signature,
                substitutions,
                constants,
                container_parameters,
                true,
                output,
            )?;
            let mut method = format!("receiver.{}", plan.name()).into_bytes();
            render_item_turbofish(
                signature,
                substitutions,
                constants,
                container_parameters,
                &mut method,
            )?;
            write!(method, "({taken})").map_err(encoding)?;
            let method = String::from_utf8(method).map_err(request::unwritable)?;
            writeln!(
                output,
                ", _>(&receiver, |receiver| {})?;",
                yoked_guard(&method, optional)
            )
            .map_err(encoding)?;
            render_yoked_place(
                &target,
                optional,
                inputs,
                "end.held(&*arguments.arena)",
                &|body| format!("end.with(&*arguments.arena, |guard| {body})"),
                output,
            )
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
                    return Err(request::drift(
                        "static function item entered builder replay",
                    ));
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
                write!(output, "    let {binding} = ").map_err(request::unwritable)?;
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
                output.write_all(b"(").map_err(request::unwritable)?;
                builder_function_argument_list(
                    signature.inputs(),
                    mappers,
                    step.arguments(),
                    replay,
                    profile,
                    output,
                )?;
                output.write_all(b");\n").map_err(request::unwritable)?;
                if final_step && !replay {
                    render_give_with(signature, substitutions, "output", output)?;
                }
            }
            Operation::StructConstructor { form, fields, .. } if step.is_construct() => {
                let types = fields.iter().map(crate::surface::adapter::Field::ty);
                if !replay {
                    render_builder_field_arguments(types, step.arguments(), output)?;
                }
                write!(output, "    let {binding} = {}", rust_path(step.plan()))
                    .map_err(request::unwritable)?;
                render_builder_constructor(*form, fields, step.arguments(), replay, output)?;
                output.write_all(b";\n").map_err(request::unwritable)?;
                if final_step && !replay {
                    output
                        .write_all(
                            b"    insert_native!(arguments.arena, output).map(Dynamic::from)\n",
                        )
                        .map_err(request::unwritable)?;
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
                    .map_err(request::unwritable)?;
                render_item_turbofish(
                    signature,
                    substitutions,
                    constants,
                    *container_parameters,
                    output,
                )?;
                output.write_all(b"(").map_err(request::unwritable)?;
                builder_typed_argument_list(signature.inputs(), step.arguments(), replay, output)?;
                output.write_all(b");\n").map_err(request::unwritable)?;
                if final_step && !replay {
                    render_give_with(signature, substitutions, "output", output)?;
                }
            }
            _ => {
                return Err(request::drift("consuming builder operation"));
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
                render_builder_field_arguments(
                    fields.iter().map(crate::surface::adapter::Field::ty),
                    step.arguments(),
                    output,
                )?;
            }
            _ => {
                return Err(request::drift("consuming replay input operation"));
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
                return Err(request::drift("builder profile mapper position"));
            }
            let mapper = mappers
                .next()
                .ok_or_else(|| request::drift("builder profile mapper cardinality"))?;
            validate_event_projection(
                mapper,
                profile.ok_or_else(|| request::drift("builder profile mapper proof"))?,
            )?;
            continue;
        }
        let argument = *arguments
            .next()
            .ok_or_else(|| request::drift("consuming builder argument cardinality"))?
            as usize;
        render_take(argument, argument, ty, output)?;
    }
    if arguments.next().is_some() || mappers.next().is_some() {
        return Err(request::drift("consuming builder argument cardinality"));
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
            output.write_all(b", ").map_err(request::unwritable)?;
        }
        let mapped_here = mappers
            .peek()
            .is_some_and(|mapper| usize::try_from(mapper.position()).ok() == Some(position));
        if mapped_here {
            if !matches!(ty, convert::Type::FunctionItem { .. }) {
                return Err(request::drift("builder profile mapper position"));
            }
            let mapper = mappers
                .next()
                .ok_or_else(|| request::drift("builder profile mapper cardinality"))?;
            validate_event_projection(
                mapper,
                profile.ok_or_else(|| request::drift("builder profile mapper proof"))?,
            )?;
            output
                .write_all(b"__rawr_profile::payload")
                .map_err(request::unwritable)?;
        } else {
            let argument = arguments
                .next()
                .ok_or_else(|| request::drift("consuming builder argument cardinality"))?;
            if replay && !copy_scalar(ty) {
                write!(output, "argument_{argument}.clone()").map_err(request::unwritable)?;
            } else {
                write!(output, "argument_{argument}").map_err(request::unwritable)?;
            }
        }
    }
    if arguments.next().is_some() || mappers.next().is_some() {
        return Err(request::drift("consuming builder argument cardinality"));
    }
    Ok(())
}

fn render_builder_arguments_iter<'a>(
    inputs: impl ExactSizeIterator<Item = &'a convert::Type>,
    arguments: &[u32],
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    if inputs.len() != arguments.len() {
        return Err(request::drift("consuming builder argument cardinality"));
    }
    for (ty, position) in inputs.zip(arguments) {
        let position = *position as usize;
        render_take(position, position, ty, output)?;
    }
    Ok(())
}

/// `render_builder_arguments_iter` for a struct constructor's fields
/// (`render_take_field`).
fn render_builder_field_arguments<'a>(
    fields: impl ExactSizeIterator<Item = &'a convert::Type>,
    arguments: &[u32],
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    if fields.len() != arguments.len() {
        return Err(request::drift("consuming builder argument cardinality"));
    }
    for (ty, position) in fields.zip(arguments) {
        let position = *position as usize;
        render_take_field(position, position, ty, output)?;
    }
    Ok(())
}

fn builder_argument_list(arguments: &[u32], output: &mut Vec<u8>) -> Result<(), request::Error> {
    for (index, position) in arguments.iter().enumerate() {
        if index != 0 {
            output.write_all(b", ").map_err(request::unwritable)?;
        }
        write!(output, "argument_{position}").map_err(request::unwritable)?;
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
        return Err(request::drift("consuming builder argument cardinality"));
    }
    for (index, (ty, position)) in inputs.iter().zip(arguments).enumerate() {
        if index != 0 {
            output.write_all(b", ").map_err(request::unwritable)?;
        }
        if replay && !copy_scalar(ty) {
            write!(output, "argument_{position}.clone()").map_err(request::unwritable)?;
        } else {
            write!(output, "argument_{position}").map_err(request::unwritable)?;
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
            output.write_all(b"(").map_err(request::unwritable)?;
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
            output.write_all(b")").map_err(request::unwritable)
        }
        crate::api::model::FieldForm::Record => {
            output.write_all(b" { ").map_err(request::unwritable)?;
            for (index, (field, position)) in fields.iter().zip(arguments).enumerate() {
                if index != 0 {
                    output.write_all(b", ").map_err(request::unwritable)?;
                }
                if replay && !copy_field(field.ty()) {
                    write!(output, "{}: argument_{position}.clone()", field.name())
                        .map_err(request::unwritable)?;
                } else {
                    write!(output, "{}: argument_{position}", field.name())
                        .map_err(request::unwritable)?;
                }
            }
            output.write_all(b" }").map_err(request::unwritable)
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

/// A struct expression in braces, which Rust accepts through a type alias
/// as well as the struct's own path: `Alias { 0: a }` builds a tuple
/// struct and `Alias {}` a unit one, where `Alias(a)` and `Alias` are
/// E0423, since an alias names no constructor function or constant.
fn render_constructor(
    form: crate::api::model::FieldForm,
    fields: &[crate::surface::adapter::Field],
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    match form {
        crate::api::model::FieldForm::Unit => output.write_all(b" {}").map_err(request::unwritable),
        crate::api::model::FieldForm::Tuple => {
            output.write_all(b" { ").map_err(request::unwritable)?;
            for (index, field) in fields.iter().enumerate() {
                if index != 0 {
                    output.write_all(b", ").map_err(request::unwritable)?;
                }
                write!(output, "{index}: ").map_err(request::unwritable)?;
                render_field_value(index, field, output)?;
            }
            output.write_all(b" }").map_err(request::unwritable)
        }
        crate::api::model::FieldForm::Record => {
            output.write_all(b" { ").map_err(request::unwritable)?;
            for (index, field) in fields.iter().enumerate() {
                if index != 0 {
                    output.write_all(b", ").map_err(request::unwritable)?;
                }
                write!(output, "{}: ", field.name()).map_err(request::unwritable)?;
                render_field_value(index, field, output)?;
            }
            output.write_all(b" }").map_err(request::unwritable)
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
    .map_err(request::unwritable)
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
        return Err(request::drift(
            "generated projection target argument cardinality",
        ));
    };
    let Some(source) = argument.source_index() else {
        return Ok(Some(Replay::Visual));
    };
    let source = usize::try_from(source)
        .map_err(|_| request::drift("generated projection source argument"))?;
    let message = arguments
        .get(source)
        .ok_or_else(|| request::drift("generated projection source argument"))?;
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
            return Err(request::drift("event continuation Message cardinality"));
        }
    }
    let Some(message @ convert::Type::ProfileMessage { mapper, .. }) = message else {
        return Ok(None);
    };
    let fail = || request::drift("event continuation receiver proof");
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
            return Err(request::drift("ambiguous event continuation producer"));
        }
        selected = Some(witness.projection());
    }
    if selected.is_none() {
        return Err(request::drift("missing event continuation producer"));
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
            return Err(request::drift(
                "generated event projection Message cardinality",
            ));
        }
    }
    selected.ok_or_else(|| request::drift("generated event projection Message"))
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
                return Err(request::drift(
                    "generated event projection mapper cardinality",
                ));
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
            .map_err(request::unwritable),
        Replay::Message(message) => {
            output
                .write_all(b"    __rawr_profile::insert_projectable::<_, ")
                .map_err(request::unwritable)?;
            render_rust_type(
                match message {
                    convert::Type::ProfileMessage { payload, .. } => payload,
                    message => message,
                },
                facade.ok_or_else(|| request::drift("generated projection bindings plan"))?,
                output,
            )?;
            output
                .write_all(b">(arguments.arena, output")
                .map_err(request::unwritable)?;
            if let convert::Type::ProfileMessage { mapper, .. } = message {
                output.write_all(b", &").map_err(request::unwritable)?;
                byte_slice(output, mapper)?;
            }
            output
                .write_all(b").map(Dynamic::from)\n")
                .map_err(request::unwritable)
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
                    .ok_or_else(|| request::drift("a script type's proxy"))?
                    .as_bytes(),
            )
            .map_err(request::unwritable),
        convert::Type::Inferred => output.write_all(b"_").map_err(request::unwritable),
        convert::Type::Unit | convert::Type::Never => {
            output.write_all(b"()").map_err(request::unwritable)
        }
        convert::Type::Builtin(convert::Builtin::InclusiveRange) => output
            .write_all(b"std::ops::RangeInclusive<i64>")
            .map_err(request::unwritable),
        convert::Type::Builtin(convert::Builtin::String | convert::Builtin::Text) => {
            output.write_all(b"String").map_err(request::unwritable)
        }
        convert::Type::Builtin(convert::Builtin::Str) => {
            output.write_all(b"&str").map_err(request::unwritable)
        }
        convert::Type::Builtin(builtin) => output
            .write_all(builtin.rust_name().unwrap_or_default().as_bytes())
            .map_err(request::unwritable),
        convert::Type::Nominal { definition, .. } if is_opaque(definition) => {
            output.write_all(b"_").map_err(request::unwritable)
        }
        convert::Type::Nominal {
            definition,
            arguments,
        } => {
            let ty = facade
                .facades()
                .flat_map(crate::surface::facade::Facade::types)
                .find(|ty| ty.identity() == definition.as_bytes())
                .ok_or_else(|| request::drift("generated projection message type"))?;
            output
                .write_all(ty.path().as_bytes())
                .map_err(request::unwritable)?;
            if !arguments.is_empty() {
                output.write_all(b"<").map_err(request::unwritable)?;
                for (index, argument) in arguments.iter().enumerate() {
                    if index != 0 {
                        output.write_all(b", ").map_err(request::unwritable)?;
                    }
                    render_rust_type(argument, facade, output)?;
                }
                output.write_all(b">").map_err(request::unwritable)?;
            }
            Ok(())
        }
        convert::Type::FunctionItem {
            input,
            output: result,
        } => {
            output.write_all(b"fn(").map_err(request::unwritable)?;
            render_rust_type(input, facade, output)?;
            output.write_all(b") -> ").map_err(request::unwritable)?;
            render_rust_type(result, facade, output)
        }
        convert::Type::ProfileMessage { payload, .. } => render_rust_type(payload, facade, output),
        convert::Type::Borrowed { mutable, ty } => {
            output
                .write_all(if *mutable { b"&mut " } else { b"&" })
                .map_err(request::unwritable)?;
            render_rust_type(ty, facade, output)
        }
        convert::Type::Option(element) => {
            output.write_all(b"Option<").map_err(request::unwritable)?;
            render_rust_type(element, facade, output)?;
            output.write_all(b">").map_err(request::unwritable)
        }
        convert::Type::Range(element) => {
            output
                .write_all(b"std::ops::Range<")
                .map_err(request::unwritable)?;
            render_rust_type(element, facade, output)?;
            output.write_all(b">").map_err(request::unwritable)
        }
        convert::Type::Result(ok, err) => {
            output.write_all(b"Result<").map_err(request::unwritable)?;
            render_rust_type(ok, facade, output)?;
            output.write_all(b", ").map_err(request::unwritable)?;
            render_rust_type(err, facade, output)?;
            output.write_all(b">").map_err(request::unwritable)
        }
        convert::Type::ConstArray { .. } => Err(unlengthed()),
        convert::Type::Sequence {
            element,
            length: Some(length),
            ..
        } => {
            output.write_all(b"[").map_err(request::unwritable)?;
            render_rust_type(element, facade, output)?;
            write!(output, "; {length}]").map_err(request::unwritable)
        }
        convert::Type::Sequence { element, .. } => {
            output.write_all(b"Vec<").map_err(request::unwritable)?;
            render_rust_type(element, facade, output)?;
            output.write_all(b">").map_err(request::unwritable)
        }
        convert::Type::Tuple(members) => {
            output.write_all(b"(").map_err(request::unwritable)?;
            for member in members {
                render_rust_type(member, facade, output)?;
                output.write_all(b", ").map_err(request::unwritable)?;
            }
            output.write_all(b")").map_err(request::unwritable)
        }
        convert::Type::Parameter(_) => {
            Err(request::drift("unconstrained generated projection message"))
        }
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
                return Err(request::drift("generated static operand position"));
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
        return Err(request::drift("generated static operand cardinality"));
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
    /// The call of the callable parameter at input `position` that a
    /// script closure that captures fills ([`Slot::closure`]): a Rust
    /// closure holding what it captured, as each of iced's `Task::sip`'s
    /// callables takes one.
    ///
    /// [`Slot::closure`]: crate::surface::instance::Slot::closure
    fn closure_at(
        &self,
        position: usize,
        ty: &convert::Type,
    ) -> Option<&'a crate::classify::representability::callable::Call> {
        self.find(position, ty, crate::surface::instance::Slot::is_closure)
    }

    /// The call of the slot at input `position`, when one fills it.
    fn slot_at(
        &self,
        position: usize,
        ty: &convert::Type,
    ) -> Option<&'a crate::classify::representability::callable::Call> {
        self.find(position, ty, |slot| !slot.is_closure() && !slot.is_native())
    }

    /// The call of the callable parameter at input `position` that a
    /// crate's own callable value fills ([`Slot::native`]), as the `impl
    /// Fn(B) -> O` of iced's `Function::with` does.
    ///
    /// [`Slot::native`]: crate::surface::instance::Slot::native
    fn native_at<'c>(
        &self,
        position: usize,
        ty: &convert::Type,
        call: Option<&'c crate::classify::representability::callable::Call>,
    ) -> Option<&'c crate::classify::representability::callable::Call>
    where
        'a: 'c,
    {
        let convert::Type::Parameter(parameter) = ty else {
            return None;
        };
        if !self
            .slots
            .iter()
            .any(|slot| usize::try_from(slot.position()).ok() == Some(position) && slot.is_native())
        {
            return None;
        }
        self.crossings
            .iter()
            .chain(call)
            .find(|call| call.parameter() == *parameter)
    }

    fn find(
        &self,
        position: usize,
        ty: &convert::Type,
        kind: impl Fn(&crate::surface::instance::Slot) -> bool,
    ) -> Option<&'a crate::classify::representability::callable::Call> {
        let convert::Type::Parameter(parameter) = ty else {
            return None;
        };
        self.slots
            .iter()
            .any(|slot| usize::try_from(slot.position()).ok() == Some(position) && kind(slot))
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
    let drift = request::drift;
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
        // The slot's signature names the output's type, which a wrapper's
        // family can't (`family`).
        Some(written_family(&returned).ok_or_else(|| drift("a slot's borrowing output"))?)
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
    let mut places = Vec::new();
    for (position, input) in inputs.iter().enumerate() {
        let name = format!("input_{position}");
        match input {
            convert::Type::Borrowed { mutable: true, ty }
                if matches!(**ty, convert::Type::Script { .. }) =>
            {
                let rust = concrete_type(ty).ok_or_else(|| drift("a slot's input"))?;
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
                let rust = concrete_type(ty).ok_or_else(|| drift("a slot's input"))?;
                parameters.push(format!("{name}: &{lifetime}{rust}"));
                values.push(format!("{name}.0.lend()"));
                pins.get_or_insert_with(|| format!("{name}.0.pins()"));
            }
            // The unit a program of no state lends, as iced's `run((), view)`
            // lends `view` `&()`: the script function takes the unit, which
            // holds nothing to lend or give back.
            convert::Type::Borrowed {
                mutable: borrowed_mutably,
                ty,
            } if matches!(**ty, convert::Type::Unit) => {
                let borrow = if *borrowed_mutably { "mut " } else { "" };
                parameters.push(format!("{name}: &{lifetime}{borrow}()"));
                values.push(format!("{{ let _ = {name}; Dynamic::from(()) }}"));
            }
            // A shared borrow of one of std's values Rawr's std holds as its
            // own, as `&Duration`, or of a facade's own value: Rawr's value
            // of it, as a `&str` is Rawr's string.
            convert::Type::Borrowed { mutable: false, ty }
                if family.is_none()
                    && matches!(**ty, convert::Type::Nominal { .. })
                    && (is_duration(ty) || is_facade_value(ty))
                    && !carries_lifetime(ty) =>
            {
                let rust = concrete_type(ty).ok_or_else(|| drift("a slot's input"))?;
                parameters.push(format!("{name}: &{rust}"));
                let mut converted = String::new();
                render_to_dynamic(ty, &format!("Clone::clone({name})"), 0, &mut converted)?;
                native = true;
                values.push(format!(
                    "(({converted}).and_then(|value| attest_input(arguments.arena, value, witnesses.get({position}))))"
                ));
            }
            // A native value the function borrows: lent for the call alone,
            // as a script impl's function is (D558.3), so the function reads
            // the value native code holds at each call, as elevato's
            // `update(.., building: &Building)` does, and nothing is copied
            // or asks `Clone` (#1405). A closure gives each `&mut` parameter
            // back after its value, as `Path::new(|p| ..)`'s does, so it is
            // one of the call's places. Where the output is at the slot's
            // `'x`, as `transition`'s view takes `&Animation<f32>` and gives
            // a widget of the view, the output's type is one for every
            // borrow of the input, so it borrows nothing of it, and the
            // input's borrow is a lifetime of its own.
            convert::Type::Borrowed {
                mutable: borrowed_mutably,
                ty,
            } if matches!(**ty, convert::Type::Nominal { .. })
                && !is_value(ty)
                && !carries_lifetime(ty) =>
            {
                let rust = concrete_type(ty).ok_or_else(|| drift("a slot's input"))?;
                let borrow = if *borrowed_mutably { "&mut " } else { "&" };
                parameters.push(format!("{name}: {borrow}{rust}"));
                if *borrowed_mutably {
                    mutable.push(position.to_string());
                }
                let lend = LentInput::new(position, *borrowed_mutably, rust);
                values.push(lend.value());
                lends.push(lend);
            }
            convert::Type::Borrowed {
                mutable: borrowed_mutably,
                ty,
            } if family.is_none() && is_copied(ty) && place::available(ty) => {
                let rust =
                    concrete_type(ty).ok_or_else(|| drift("a slot's native sequence input"))?;
                let borrow = if *borrowed_mutably { "&mut " } else { "&" };
                parameters.push(format!("{name}: {borrow}{rust}"));
                values.push(format!("place_{position}.clone()"));
                places.push((position, *borrowed_mutably, place::policy(ty)?));
                if *borrowed_mutably {
                    mutable.push(position.to_string());
                }
                native = true;
            }
            convert::Type::Borrowed { .. } => {
                return Err(drift(
                    "a slot's input borrows a value that isn't a script value",
                ));
            }
            input => {
                let rust = concrete_type(input).ok_or_else(|| drift("a slot's input"))?;
                parameters.push(format!("{name}: {rust}"));
                let mut converted = String::new();
                {
                    let _inputs = ScriptInputs::enter();
                    render_to_dynamic(input, &name, 0, &mut converted)?;
                }
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
    // A view lent no program value, as `view(_: &())` of iced's `run((),
    // view)`, borrows only literals and constants, which outlive every
    // call, as rustc allows: the slot's own pins hold them for as long as
    // the runner lives, one per text (`Pins::constant`). The region the
    // call opens is kept once its output is taken out, as a script value
    // native code took of the view may hold one of its values
    // (`Region::keep`, #1217).
    let mut constant_pins = String::new();
    let (generics, rust, body) = match &family {
        Some(family) => {
            let pins = pins.unwrap_or_else(|| {
                constant_pins = format!(
                    "\n    static PINS_{local}: __rawr_internal::scope::Pins = __rawr_internal::scope::Pins::constant();"
                );
                format!("&PINS_{local}")
            });
            let (items, body) = lent_around(
                &lends,
                format!(
                    "let mut values = {values};
        let mut region = __rawr_internal::scope::Region::anchored({pins});
        let value = __rawr_internal::scope::open_anchored(&mut region, || SLOT_{local}.invoke(&mut values, &[{mutable}]));{restores}
        let scoped = value
            .read_lock::<__rawr_internal::scope::Scoped>()
            .map(|scoped| *scoped)
            .unwrap_or_else(|| panic!(\"rawr: a script function gave native code a value that doesn't borrow what it was lent\"));
        let output = region
            .take::<{family}>(scoped)
            .unwrap_or_else(|error| panic!(\"rawr: {{error}}\"));
        static SITE: __rawr_internal::scope::Site = __rawr_internal::scope::Site::new();
        region.keep(&SITE);
        output"
                ),
            );
            (
                "<'x>",
                format!("<{family} as __rawr_internal::scope::Family>::Of<'x>"),
                format!("{items}{body}"),
            )
        }
        None => {
            let rust = concrete_type(&returned).ok_or_else(|| drift("a slot's output"))?;
            let mut converted = String::new();
            render_from_dynamic(&returned, "value", 0, &mut converted)?;
            let native_output = holds_native(&returned);
            // A script value made on a thread with no arena is made where
            // it is first used (`DeferProbe`): its function may make native
            // values, as `Message::Tick(Local::now())` does, which only the
            // owner's arena holds.
            let deferred = if matches!(returned, convert::Type::Script { .. })
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
            // where it is used instead (`deferred`), when it can be; one
            // the function gives in a container, as `Some(message)`, takes
            // the native values it holds with it (`script_proxy`).
            let call_arena = if native || native_output || holds_script(&returned) {
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
    let body = place::around(
        &places,
        &format!(
            "WITNESSES_{local}.read().unwrap_or_else(std::sync::PoisonError::into_inner).clone()"
        ),
        body,
    );
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
        .map_err(|error| Error::Value(__rawr_internal::future::message(&error)))?;{witnesses}{constant_pins}
    #[allow(unused_mut, clippy::needless_lifetimes)]
    fn slot_{local}{generics}({}) -> {rust} {{
        {body}
    }}",
        parameters.join(", ")
    )
    .map_err(request::unwritable)
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
                let function = selected
                    .next()
                    .ok_or_else(|| request::drift("generated static function item cardinality"))?;
                write!(output, "    let argument_{position} = ").map_err(request::unwritable)?;
                render_function_callable(function.plan(), output)?;
                output.write_all(b";\n").map_err(request::unwritable)?;
            }
            (convert::Type::FunctionItem { .. }, false, true) => {
                let mapper = mapped
                    .next()
                    .ok_or_else(|| request::drift("generated profile mapper cardinality"))?;
                validate_event_projection(
                    mapper,
                    profile.ok_or_else(|| request::drift("generated profile mapper proof"))?,
                )?;
            }
            (convert::Type::FunctionItem { .. }, false, false)
            | (_, true, true)
            | (_, true, false)
            | (_, false, true) => {
                return Err(request::drift("generated static operand position"));
            }
            _ => {
                if let Some(slot) = crossed.slot_at(position, ty) {
                    render_slot(position, runtime, slot, signature, substitutions, output)?;
                } else if let Some(native) = crossed.native_at(position, ty, call) {
                    let rust = boxed_call(native, signature, substitutions)?;
                    writeln!(
                        output,
                        "    let argument_{position} = take_native::<{rust}>(arguments, {runtime})?;"
                    )
                    .map_err(request::unwritable)?;
                } else if let Some(call) = call
                    .filter(|call| *ty == convert::Type::Parameter(call.parameter()))
                    .or_else(|| crossed.closure_at(position, ty))
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
        return Err(request::drift("generated static operand cardinality"));
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
            output.write_all(b", ").map_err(request::unwritable)?;
        }
        let selected_here = selected
            .peek()
            .is_some_and(|function| usize::try_from(function.position()).ok() == Some(position));
        let mapped_here = mapped
            .peek()
            .is_some_and(|mapper| usize::try_from(mapper.position()).ok() == Some(position));
        match (ty, selected_here, mapped_here) {
            (convert::Type::FunctionItem { .. }, true, false) => {
                let function = selected
                    .next()
                    .ok_or_else(|| request::drift("generated static function item cardinality"))?;
                render_function_callable(function.plan(), output)?;
            }
            // A crate function filling a callable parameter, which
            // `render_function_arguments` bound to its local.
            (ty, true, false) if callable_parameter(ty, signature) => {
                selected.next();
                write!(output, "argument_{position}").map_err(request::unwritable)?;
            }
            (convert::Type::FunctionItem { .. }, false, true) => {
                let mapper = mapped
                    .next()
                    .ok_or_else(|| request::drift("generated profile mapper cardinality"))?;
                validate_event_projection(
                    mapper,
                    profile.ok_or_else(|| request::drift("generated profile mapper proof"))?,
                )?;
                output
                    .write_all(b"__rawr_profile::payload")
                    .map_err(request::unwritable)?;
            }
            (convert::Type::FunctionItem { .. }, false, false)
            | (_, true, true)
            | (_, true, false)
            | (_, false, true) => {
                return Err(request::drift("generated static operand position"));
            }
            // A script function, as a slot (D594).
            _ if crossed.slot_at(position, ty).is_some() => {
                write!(output, "slot_{position}").map_err(request::unwritable)?;
            }
            // A script closure, made a Rust closure (D513.7 b), or an
            // `Option` of one.
            _ if callable_parameter(ty, signature)
                || matches!(ty, convert::Type::Option(inner) if callable_parameter(inner, signature)) =>
            {
                write!(output, "argument_{position}").map_err(request::unwritable)?;
            }
            // In the builder of the value that owns them (`LENT_CELL`), a
            // lent argument is the builder's borrow of it.
            _ if VIEWED_ARGUMENTS.get()
                && lent_slice_element(ty, &substituted(ty, signature, substitutions)?)
                    .is_some() =>
            {
                if LENT_CELL.get() {
                    write!(output, "&argument_{position}[..]")
                } else {
                    write!(
                        output,
                        "&pins.hold(std::rc::Rc::new(argument_{position}))[..]"
                    )
                }
                .map_err(request::unwritable)?;
            }
            _ if VIEWED_ARGUMENTS.get()
                && keeps_borrow(signature, position, ty)
                && viewed_native(&substituted(ty, signature, substitutions)?).is_some() =>
            {
                let option = matches!(
                    substituted(ty, signature, substitutions)?,
                    convert::Type::Option(_)
                );
                match (LENT_CELL.get(), option) {
                    (true, true) => write!(output, "argument_{position}.as_deref()"),
                    (true, false) => write!(output, "&**argument_{position}"),
                    (false, true) => {
                        write!(output, "argument_{position}.map(|value| pins.hold(value))")
                    }
                    (false, false) => write!(output, "pins.hold(argument_{position})"),
                }
                .map_err(request::unwritable)?;
            }
            _ => {
                let ty = substitute(ty, signature, substitutions)?;
                if profile.is_some_and(|proof| is_projection_target(ty, proof)) {
                    write!(output, "argument_{position}.lower(_context)")
                        .map_err(request::unwritable)?;
                } else if profile.is_some() && !copy_scalar(ty) {
                    write!(output, "argument_{position}.clone()").map_err(request::unwritable)?;
                } else {
                    write!(output, "argument_{position}").map_err(request::unwritable)?;
                }
            }
        }
    }
    if selected.next().is_some() || mapped.next().is_some() {
        return Err(request::drift("generated static operand cardinality"));
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
        return Err(request::drift("generated profile mapper identity"));
    }
    let mut projections = profile
        .projections()
        .iter()
        .filter(|projection| projection.selector() == witness.profile_projection());
    let projection = projections
        .next()
        .ok_or_else(|| request::drift("generated profile mapper projection"))?;
    if projections.next().is_some()
        || projection.target_identity() != witness.target().as_bytes()
        || profile
            .source_for_projection(projection)
            .is_none_or(|source| source.selector() != witness.profile_source())
    {
        return Err(request::drift("generated profile mapper projection"));
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
        return Err(request::drift("generated static function item plan"));
    };
    // A generic function's turbofish names its instance's types.
    if !constants.is_empty() || !functions.is_empty() || !mappers.is_empty() {
        return Err(request::drift("generated nested static function item"));
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
        .map_err(request::unwritable);
    }
    if matches!(ty, convert::Type::Builtin(convert::Builtin::InclusiveRange)) {
        return writeln!(
            output,
            "    let argument_{local}: std::ops::RangeInclusive<i64> = take_primitive(arguments, {index})?;"
        )
        .map_err(request::unwritable);
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
            output.write_all(b", ").map_err(request::unwritable)?;
        }
        let ty = substitute(ty, signature, substitutions)?;
        if is_projection_target(ty, profile) {
            write!(output, "argument_{index}.lower(_context)").map_err(request::unwritable)?;
        } else if copy_scalar(ty) {
            write!(output, "argument_{index}").map_err(request::unwritable)?;
        } else {
            write!(output, "argument_{index}.clone()").map_err(request::unwritable)?;
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
    // `&[T]` of crate values, as iced's `markdown::view(&items, ..)` takes:
    // each value moved out of its slot into one slice, and back once nothing
    // borrows it (`SliceLent`). An output that may borrow it keeps it
    // (`views_natives`): an opaque one owns it (`lent_cell`), and another
    // has the call's pins hold it. Else it goes back after the call.
    let lent = substituted(ty, signature, substitutions)?;
    if let Some(element) = lent_slice_element(ty, &lent)
        && !LENT_ARGUMENTS.get()
    {
        let item =
            concrete_type(element).ok_or_else(|| request::drift("a lent slice's element type"))?;
        let take = format!(
            "{{ let value = (**arguments.values.get_mut({index}).ok_or(Error::MissingArgument)?).take().flatten(); let handles = value.into_array().map_err(|_| Error::TypeMismatch)?.into_iter().map(|item| item.try_cast_result::<NativeHandle>().map_err(|_| Error::TypeMismatch)).collect::<Result<Vec<_>, Error>>()?; lend_slice::<{item}>(arguments.arena, handles)? }}"
        );
        // A native slice place, reached where it is for the call
        // (`with_slice_argument`, #1291).
        if !VIEWED_ARGUMENTS.get() && SLICE_ARGUMENTS.with_borrow(|slices| slices.contains(&index))
        {
            return writeln!(
                output,
                "    let argument_{local}_lent;
    let argument_{local}: &[{item}] = match slice_{index} {{
        Some(slice) => slice,
        None => {{
            argument_{local}_lent = {take};
            &argument_{local}_lent.items
        }}
    }};"
            )
            .map_err(request::unwritable);
        }
        let (name, borrow) = if VIEWED_ARGUMENTS.get() {
            (format!("argument_{local}"), String::new())
        } else {
            (
                format!("argument_{local}_lent"),
                format!("\n    let argument_{local}: &[{item}] = &argument_{local}_lent.items;"),
            )
        };
        return writeln!(output, "    let {name} = {take};{borrow}").map_err(request::unwritable);
    }
    // A borrow of a script value where native generic code takes a Rust
    // type, as `&Square` fills `name_of(shape: &impl Shape)`: a borrow of
    // its proxy, which shares the value's handle (D594, #993).
    if let convert::Type::Borrowed {
        mutable,
        ty: target,
    } = &lent
        && matches!(**target, convert::Type::Script { .. })
    {
        let rust = concrete_type(target).ok_or_else(|| request::drift("a script type's proxy"))?;
        let borrow = if *mutable { "&mut " } else { "&" };
        return writeln!(
            output,
            "    let mut argument_{local}_proxy = <{rust}>::from_proxy(__rawr_internal::crossing::Proxy::from_place(&mut **arguments.values.get_mut({index}).ok_or(Error::MissingArgument)?));\n    let argument_{local} = {borrow}argument_{local}_proxy;"
        )
        .map_err(request::unwritable);
    }
    // A crate value the output borrows past the call: the value moved into
    // an `Rc` its slot shares, which the call's pins hold (`views_natives`).
    if VIEWED_ARGUMENTS.get() && keeps_borrow(signature, local, ty) {
        let substituted = substituted(ty, signature, substitutions)?;
        if let Some(target) = viewed_native(&substituted) {
            let rust = concrete_type(target)
                .ok_or_else(|| request::drift("a viewed native input's type"))?;
            let share = format!(
                "inner.try_cast_result::<NativeHandle>().map_err(|_| Error::TypeMismatch).and_then(|handle| arguments.arena.share::<{rust}>(handle))"
            );
            return if matches!(substituted, convert::Type::Option(_)) {
                writeln!(
                    output,
                    "    let argument_{local} = {{ let mut value = (**arguments.values.get_mut({index}).ok_or(Error::MissingArgument)?).take(); match __rawr_internal::option::take(&mut value).ok_or(Error::TypeMismatch)? {{ None => None, Some(inner) => Some({share}?) }} }};"
                )
            } else {
                writeln!(
                    output,
                    "    let argument_{local} = {{ let inner = (**arguments.values.get_mut({index}).ok_or(Error::MissingArgument)?).take(); {share}? }};"
                )
            }
            .map_err(request::unwritable);
        }
    }
    // A type parameter a borrow of one of std's values fills, as `&PathBuf`
    // fills `opened<P: AsRef<Path>>(path: P)`: the value itself, lent as a
    // `&Path` input is. So is a borrow of a crate value, as `&Handle` fills
    // iced's `canvas::Image::new(handle: impl Into<Handle>)` (#954).
    if let convert::Type::Parameter(_) = ty
        && let lent @ convert::Type::Borrowed {
            mutable: false,
            ty: target,
        } = substitute(ty, signature, substitutions)?
        && (std_value(target).is_some() || matches!(**target, convert::Type::Nominal { .. }))
    {
        let lent = lent.clone();
        return render_take_input(
            local,
            index,
            &lent,
            signature,
            substitutions,
            profile,
            output,
        );
    }
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
        if LENT_ARGUMENTS.get() && place::available(element) {
            let rust = concrete_type(element)
                .ok_or_else(|| request::drift("a copied or native borrowed input's type"))?;
            // A native place is lent where it is and given back as it is.
            // A program's value the call takes the first time is moved
            // into a native value, which goes back to the program in its
            // place (#716).
            return writeln!(
                output,
                "    let argument_{local}_witness = operand_witness({index});
    let argument_{local}_place = {{ let value = &**arguments.values.get({index}).ok_or(Error::MissingArgument)?; if __rawr_internal::place::get(value).is_some() {{ value.clone() }} else {{ Dynamic::UNIT }} }};
    let mut argument_{local}_copy: Option<{rust}> = None;
    let argument_{local}: &mut {rust} = match lent_argument(lent, {index}) {{
        Some(value) => value.mutable::<{rust}>().ok_or(Error::TypeMismatch)?,
        None => {{
            let value = (**arguments.values.get_mut({index}).ok_or(Error::MissingArgument)?).take().flatten();
            argument_{local}_copy = Some({converted}?);
            argument_{local}_copy.as_mut().expect(\"the script operand was materialized\")
        }}
    }};"
            )
            .map_err(request::unwritable);
        }
        let witness = if place::available(element) {
            format!(
                "    let argument_{local}_witness = operand_witness({index});\n    let argument_{local}_owner = arguments.values.get({index}).and_then(|value| __rawr_internal::place::get(&**value));\n"
            )
        } else {
            String::new()
        };
        return writeln!(
            output,
            "{witness}    let mut argument_{local}_copy = {{ let value = (**arguments.values.get_mut({index}).ok_or(Error::MissingArgument)?).take().flatten(); {converted}? }};\n    #[allow(unused_variables)]\n    let argument_{local} = &mut argument_{local}_copy;"
        )
        .map_err(request::unwritable);
    }
    // `&&str`, as a `&"text"` an argument's `impl Trait` borrows where the
    // crate implements its trait for `&str` (`object_inputs`): a borrow of
    // the `&str` lent from Rawr's string.
    if let convert::Type::Borrowed {
        mutable: false,
        ty: lent,
    } = &lent
        && matches!(&**lent, convert::Type::Borrowed { mutable: false, ty }
            if **ty == convert::Type::Builtin(convert::Builtin::Str))
    {
        return writeln!(
            output,
            "    let argument_{local}_owned = take_primitive::<ImmutableString>(arguments, {index})?;\n    let argument_{local}_str = argument_{local}_owned.as_str();\n    let argument_{local} = &argument_{local}_str;"
        )
        .map_err(request::unwritable);
    }
    // `&'static [&'static str]`, as serde's `deserialize_struct` takes its
    // fields: Rawr's array of strings, pinned as one slice for as long as
    // the runner lives, which equal arrays share.
    if let convert::Type::Borrowed {
        mutable: false,
        ty: lent,
    } = &lent
        && matches!(&**lent, convert::Type::Sequence { element, .. }
            if **element == convert::Type::Builtin(convert::Builtin::StaticStr))
    {
        return writeln!(
            output,
            "    let argument_{local} = {{ static PINS: __rawr_internal::scope::Pins = __rawr_internal::scope::Pins::constant(); let value = (**arguments.values.get_mut({index}).ok_or(Error::MissingArgument)?).take().flatten(); let texts = value.into_array().map_err(|_| Error::TypeMismatch)?.into_iter().map(|text| text.into_immutable_string().map_err(|_| Error::TypeMismatch)).collect::<Result<Vec<_>, Error>>()?; PINS.pin_strs(texts) }};"
        )
        .map_err(request::unwritable);
    }
    // `&[T]` or `&(A, B)` of Rawr's array or tuple, as voronator's
    // `from_tuple` and image's `save_buffer` take: the Rust value made from
    // it, lent to the call, as a `&str` input is. So is one a borrowed type
    // parameter takes, as serde's `serialize_field(key, &T)` at `bool`. A
    // by-value `T` a borrow fills, as `pick(options: L)` at `&Vec<Language>`,
    // takes its value as a by-value parameter does.
    if matches!(ty, convert::Type::Borrowed { mutable: false, .. })
        && let convert::Type::Borrowed {
            mutable: false,
            ty: lent,
        } = &lent
        && is_made_for_call(lent)
    {
        let mut converted = String::new();
        render_from_dynamic(lent, "value", 0, &mut converted)?;
        if LENT_ARGUMENTS.get() && place::available(lent) {
            let rust = concrete_type(lent)
                .ok_or_else(|| request::drift("a shared native sequence input's type"))?;
            return writeln!(
                output,
                "    let argument_{local}_made: {rust};
    let argument_{local}: &{rust} = match lent_argument(lent, {index}) {{
        Some(value) => value.shared::<{rust}>().ok_or(Error::TypeMismatch)?,
        None => {{
            let value = (**arguments.values.get_mut({index}).ok_or(Error::MissingArgument)?).take().flatten();
            argument_{local}_made = {converted}?;
            &argument_{local}_made
        }}
    }};"
            )
            .map_err(request::unwritable);
        }
        return writeln!(
            output,
            "    let argument_{local}_made = {{ let value = (**arguments.values.get_mut({index}).ok_or(Error::MissingArgument)?).take().flatten(); {converted}? }};\n    let argument_{local} = &argument_{local}_made;"
        )
        .map_err(request::unwritable);
    }
    // A shared borrow of one of std's types Rawr's std holds: the value
    // itself, lent for the call.
    if let convert::Type::Borrowed {
        mutable: false,
        ty: lent,
    } = ty
        && let Some(rust) = std_value(lent)
    {
        return writeln!(
            output,
            "    let argument_{local}_lent = <{rust} as __rawr_internal::crossing::Std>::lend((**arguments.values.get_mut({index}).ok_or(Error::MissingArgument)?).take(), &mut *arguments.arena).ok_or(Error::TypeMismatch)?;\n    let argument_{local} = &*argument_{local}_lent;"
        )
        .map_err(request::unwritable);
    }
    // `&HashMap<K, V>` and the other keyed collections: std's made from
    // rawr's for the call. A map of crate values moves each value out of
    // its slot into it for the call, and back after (`MapLent`).
    if let convert::Type::Borrowed {
        mutable: false,
        ty: lent,
    } = ty
        && let Some((_, members)) = keyed_element(lent)
    {
        let rust =
            concrete_type(lent).ok_or_else(|| request::drift("a lent keyed collection's type"))?;
        if let [key, value] = members
            && lendable(value)
        {
            let item =
                concrete_type(value).ok_or_else(|| request::drift("a lent map's value type"))?;
            let mut key_converted = String::new();
            render_from_dynamic(key, "key", 1, &mut key_converted)?;
            return writeln!(
                output,
                "    let argument_{local}_lent = {{ let value = (**arguments.values.get_mut({index}).ok_or(Error::MissingArgument)?).take().flatten(); let mut lent = MapLent::<{rust}> {{ map: Default::default(), keys: Vec::new() }}; for entry in value.into_array().map_err(|_| Error::TypeMismatch)? {{ let mut pair = entry.into_array().map_err(|_| Error::TypeMismatch)?.into_iter(); let key = pair.next().ok_or(Error::TypeMismatch)?; let key = ({key_converted})?; let handle = pair.next().ok_or(Error::TypeMismatch)?.try_cast_result::<NativeHandle>().map_err(|_| Error::TypeMismatch)?; let item = arguments.arena.lend_out::<{item}>(handle.clone())?; lent.keys.push((Clone::clone(&key), handle)); lent.map.insert(key, item); }} lent }};\n    let argument_{local}: &{rust} = &argument_{local}_lent.map;"
            )
            .map_err(request::unwritable);
        }
        let mut converted = String::new();
        render_from_dynamic(lent, "value", 0, &mut converted)?;
        return writeln!(
            output,
            "    let argument_{local}_made: {rust} = {{ let value = (**arguments.values.get_mut({index}).ok_or(Error::MissingArgument)?).take().flatten(); {converted}? }};\n    let argument_{local} = &argument_{local}_made;"
        )
        .map_err(request::unwritable);
    }
    // `&Rc<T>` or `&Arc<T>` of a crate value: the crate's own pointer
    // rawr's stands for, lent from its slot for the call (#775).
    if let convert::Type::Borrowed {
        mutable: false,
        ty: lent,
    } = ty
        && counted_element(lent).is_some()
    {
        let rust =
            concrete_type(lent).ok_or_else(|| request::drift("a lent counted pointer's type"))?;
        return writeln!(
            output,
            "    let argument_{local}_lent = {{ let value = (**arguments.values.get_mut({index}).ok_or(Error::MissingArgument)?).take(); lend_pointer::<{rust}>(arguments.arena, &value)? }};\n    let argument_{local}: &{rust} = argument_{local}_lent.get().ok_or(Error::TypeMismatch)?;"
        )
        .map_err(request::unwritable);
    }
    // `&F` the call fills with `str`: the `&str` input it is (`instantiated`).
    if let convert::Type::Borrowed { .. } = ty
        && lent == convert::Type::Builtin(convert::Builtin::Str)
    {
        return render_take_input(
            local,
            index,
            &lent,
            signature,
            substitutions,
            profile,
            output,
        );
    }
    if let convert::Type::Borrowed { .. } = ty {
        let future = is_future(&substituted(signature.output(), signature, substitutions)?);
        return render_lease(
            local,
            index,
            &substituted(ty, signature, substitutions)?,
            future,
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
        && let Some(family) = family_of(&element).filter(|family| family.named().is_some())
    {
        let taken = family.call(family::Reach::Take, "scoped");
        // An element made outside any region is in the arena.
        let arena = region_arena(&element)?;
        return writeln!(
            output,
            "    let argument_{local} = (**arguments.values.get_mut({index}).ok_or(Error::MissingArgument)?).take().into_array().map_err(|_| Error::TypeMismatch).and_then(|items| items.into_iter().map(|item| {{ let scoped = item.read_lock::<__rawr_internal::scope::Scoped>().map(|scoped| *scoped); match scoped {{ Some(scoped) => region.{taken}.map_err(|error| Error::Lent(scoped.name(), error)), None => {arena} }} }}).collect::<Result<Vec<_>, Error>>())?;"
        )
        .map_err(request::unwritable);
    }
    // An option of a scope frame's region's value, moved out of it into
    // the call, as a crate's impl of its trait for `Option<T>` takes
    // `maybe.map(|x| text(x))`. One made outside any region is in the
    // arena, as an array's element is.
    if REGION_ARGUMENTS.get()
        && let convert::Type::Option(element) = substituted(ty, signature, substitutions)?
        && let Some(family) = family_of(&element).filter(|family| family.named().is_some())
    {
        let taken = family.call(family::Reach::Take, "scoped");
        let arena = region_arena(&element)?;
        return writeln!(
            output,
            "    let argument_{local} = {{ let mut value = (**arguments.values.get_mut({index}).ok_or(Error::MissingArgument)?).take(); __rawr_internal::option::take(&mut value).ok_or(Error::TypeMismatch)?.map(|item| {{ let scoped = item.read_lock::<__rawr_internal::scope::Scoped>().map(|scoped| *scoped); match scoped {{ Some(scoped) => region.{taken}.map_err(|error| Error::Lent(scoped.name(), error)), None => {arena} }} }}).transpose()? }};"
        )
        .map_err(request::unwritable);
    }
    // A value of a scope frame's region, moved out of it into the call
    // (D558 tier 1), as `push_html` takes a `Parser`. One made outside any
    // region is in the arena, as an array's element is (#952).
    if REGION_ARGUMENTS.get()
        && let region_ty = substituted(ty, signature, substitutions)?
        && let Some(family) = family_of(&region_ty)
    {
        let taken = family.call(family::Reach::Take, "scoped");
        let arena = region_arena(&region_ty)?;
        return writeln!(
            output,
            "    let argument_{local} = {{ let item = (**arguments.values.get_mut({index}).ok_or(Error::MissingArgument)?).take(); let scoped = item.read_lock::<__rawr_internal::scope::Scoped>().map(|scoped| *scoped); match scoped {{ Some(scoped) => region.{taken}.map_err(|error| Error::Lent(scoped.name(), error)), None => {arena} }}? }};"
        )
        .map_err(request::unwritable);
    }
    // What a future borrows lives for the future's life (#677): a `&str`
    // is the program's own string, pinned per site as a `&'static str` is
    // (D694), with nothing copied, and a type that holds borrows, as
    // fount's `Option<&Catalog>`, leases what it holds, recursively.
    // So is a type parameter a `&str` fills, as tokio's
    // `fs::read_to_string(path: impl AsRef<Path>)` takes one.
    let future = is_future(&substituted(signature.output(), signature, substitutions)?);
    if future && lends_str(ty, signature, substitutions) {
        return render_take(
            local,
            index,
            &convert::Type::Builtin(convert::Builtin::StaticStr),
            output,
        );
    }
    if let lending
    @ (convert::Type::Option(_) | convert::Type::Result(..) | convert::Type::Tuple(_)) =
        substituted(ty, signature, substitutions)?
        && let Some(lease) = Lease::of(&lending, future, 1)
    {
        writeln!(
            output,
            "    #[allow(unused_mut)]\n    let mut argument_{local}_held: {held} = {{ let value_1 = (**arguments.values.get_mut({index}).ok_or(Error::MissingArgument)?).take(); {take} }};",
            held = lease.held,
            take = lease.take,
        )
        .map_err(request::unwritable)?;
        // A future borrows it again inside itself (`future_borrows`), and
        // a member that moves moves there.
        if future {
            return Ok(());
        }
        let borrow = lease.borrow(&format!("argument_{local}_held"), 1);
        return writeln!(output, "    let argument_{local} = {borrow};")
            .map_err(request::unwritable);
    }
    // A type parameter only a `&'static str` fills, as a literal fills
    // iced's `title(impl Title<State>)`: a `&'static str` input.
    if static_str(ty, signature, substitutions) {
        return render_take(
            local,
            index,
            &convert::Type::Builtin(convert::Builtin::StaticStr),
            output,
        );
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
        convert::Type::Sequence { .. } | convert::Type::Option(_) | convert::Type::Nominal { .. }
    ) || range_inclusive_element(ty).is_some()
        || boxed_element(ty).is_some()
        || counted_element(ty).is_some()
        || keyed_element(ty).is_some()
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
    // A crate type is written out too, at its instance's arguments: a
    // trait's function the call reaches through one impl, as
    // `Background::from(color)` of `impl From<Color> for Background`, or
    // `point.sub(other)` of `impl Sub<Point<T>> for Point<T>` beside `impl
    // Sub<Vector<T>>`, takes the impl's type, which rustc can't infer
    // beside the other impls of the trait (T502).
    let plain = matches!(ty, convert::Type::Nominal { .. }) && !is_value(ty) && !is_future(ty);
    let annotation = (generic || plain)
        .then(|| concrete_type(ty))
        .flatten()
        .filter(|annotation| !is_value(ty) && annotation != "_");
    let Some(annotation) = annotation else {
        return output.write_all(&taken).map_err(request::unwritable);
    };
    let taken = String::from_utf8(taken)
        .map_err(request::unwritable)?
        .replacen(
            &format!("let argument_{local} = "),
            &format!("let argument_{local}: {annotation} = "),
            1,
        );
    output
        .write_all(taken.as_bytes())
        .map_err(request::unwritable)
}

/// How an input that holds borrows, as `Option<&Catalog>`, is leased for
/// the call or its future (#677): the Rust type that holds what it lends
/// (`held`), the expression that takes it from the Rawr value `value_N` at
/// depth `N` (`take`), and how a borrow of it is made again (`borrow`).
/// `None` when nothing in it borrows, or Rawr can't lend what it does.
struct Lease {
    held: String,
    take: String,
    kind: LeaseKind,
}

enum LeaseKind {
    /// A crate value leased from its slot, as `render_lease` leases one.
    Native { mutable: bool },
    /// One of std's values Rawr's std holds, as `&Path`, lent from it.
    Std,
    /// A Rust value made from Rawr's array, tuple, string or number, as
    /// `&[u8]` or `&String`, lent as `render_take_input` lends one.
    Made,
    /// A value that borrows nothing, beside ones that do, as the `String`
    /// of `Result<&Font, String>`: moved into the call once.
    Owned,
    /// A `Result` of leases: its variant's.
    Result(Box<Lease>, Box<Lease>),
    /// Rawr's string: pinned for a future, or held for a call.
    Str { pinned: bool },
    /// An `Option` of a lease: `None` lends nothing.
    Option(Box<Lease>),
    /// A tuple of leases, each lent apart.
    Tuple(Box<[Lease]>),
}

impl Lease {
    fn of(ty: &convert::Type, future: bool, depth: usize) -> Option<Self> {
        if depth == 1 && !holds_borrow(ty) {
            return None;
        }
        match ty {
            convert::Type::Borrowed { mutable, ty: lent }
                if matches!(
                    **lent,
                    convert::Type::Nominal { .. } | convert::Type::Parameter(_)
                ) && std_value(lent).is_none()
                    && counted_element(lent).is_none()
                    && keyed_element(lent).is_none()
                    && !is_made_for_call(lent) =>
            {
                let rust = concrete_type(lent)?;
                // A call holds it on the owner thread: only a future asks
                // `Send` (`render_lease`).
                let (held, lease) = if future {
                    ("Held", "lease")
                } else {
                    ("PlaceHeld", "lease_place")
                };
                Some(Self {
                    held: format!("{held}<{rust}>"),
                    take: format!(
                        "{{ let handle = value_{depth}.try_cast_result::<NativeHandle>().map_err(|_| Error::TypeMismatch)?; arguments.arena.{lease}::<{rust}>(handle)? }}"
                    ),
                    kind: LeaseKind::Native { mutable: *mutable },
                })
            }
            convert::Type::Borrowed {
                mutable: false,
                ty: lent,
            } if let Some(rust) = std_value(lent) => Some(Self {
                held: format!("<{rust} as __rawr_internal::crossing::Std>::Lent"),
                take: format!(
                    "<{rust} as __rawr_internal::crossing::Std>::lend(value_{depth}, &mut *arguments.arena).ok_or(Error::TypeMismatch)?"
                ),
                kind: LeaseKind::Std,
            }),
            // A script value, as iced's `combo_box(.., selection: Option<&T>,
            // ..)` takes the program's `&Language`, is held as its proxy, as
            // a script value fills a crate's `T` by value.
            convert::Type::Borrowed {
                mutable: false,
                ty: lent,
            } if is_made_for_call(lent) || matches!(**lent, convert::Type::Script { .. }) => {
                // A slice is held as the `Vec` made for it.
                let rust = match &**lent {
                    convert::Type::Sequence { element, .. } => {
                        format!("Vec<{}>", concrete_type(element)?)
                    }
                    lent => concrete_type(lent)?,
                };
                let mut converted = String::new();
                render_from_dynamic(lent, &format!("value_{depth}"), depth, &mut converted).ok()?;
                Some(Self {
                    held: rust,
                    take: format!("{{ let value_{depth} = value_{depth}.flatten(); ({converted})? }}"),
                    kind: LeaseKind::Made,
                })
            }
            convert::Type::Builtin(convert::Builtin::Str) if future => Some(Self {
                held: "&'static str".to_owned(),
                take: format!(
                    "{{ static PINS: __rawr_internal::scope::Pins = __rawr_internal::scope::Pins::constant(); PINS.pin(value_{depth}.try_cast_result::<ImmutableString>().map_err(|_| Error::TypeMismatch)?) }}"
                ),
                kind: LeaseKind::Str { pinned: true },
            }),
            convert::Type::Builtin(convert::Builtin::Str) => Some(Self {
                held: "ImmutableString".to_owned(),
                take: format!(
                    "value_{depth}.try_cast_result::<ImmutableString>().map_err(|_| Error::TypeMismatch)?"
                ),
                kind: LeaseKind::Str { pinned: false },
            }),
            convert::Type::Option(element) => {
                let inner = Self::of(element, future, depth + 1)?;
                let next = depth + 1;
                Some(Self {
                    held: format!("Option<{}>", inner.held),
                    take: format!(
                        "{{ let mut value_{depth} = value_{depth}; match __rawr_internal::option::take(&mut value_{depth}).ok_or(Error::TypeMismatch)? {{ None => None, Some(value_{next}) => Some({}) }} }}",
                        inner.take
                    ),
                    kind: LeaseKind::Option(Box::new(inner)),
                })
            }
            convert::Type::Result(ok, err) => {
                let next = depth + 1;
                let ok = Self::of(ok, future, next)?;
                let err = Self::of(err, future, next)?;
                Some(Self {
                    held: format!("Result<{}, {}>", ok.held, err.held),
                    take: format!(
                        "{{ let mut value_{depth} = value_{depth}; match __rawr_internal::result::take(&mut value_{depth}).ok_or(Error::TypeMismatch)? {{ Ok(value_{next}) => Ok({}), Err(value_{next}) => Err({}) }} }}",
                        ok.take, err.take
                    ),
                    kind: LeaseKind::Result(Box::new(ok), Box::new(err)),
                })
            }
            convert::Type::Tuple(members) if !members.is_empty() => {
                let next = depth + 1;
                let members = members
                    .iter()
                    .map(|member| Self::of(member, future, next))
                    .collect::<Option<Box<[_]>>>()?;
                let held = members
                    .iter()
                    .map(|member| format!("{}, ", member.held))
                    .collect::<String>();
                let taken = members
                    .iter()
                    .map(|member| {
                        format!(
                            "{{ let value_{next} = items_{depth}.next().ok_or(Error::TypeMismatch)?; {} }}, ",
                            member.take
                        )
                    })
                    .collect::<String>();
                Some(Self {
                    held: format!("({held})"),
                    take: format!(
                        "{{ let items_{depth} = value_{depth}.into_array().map_err(|_| Error::TypeMismatch)?; if items_{depth}.len() != {count} {{ return Err(Error::TypeMismatch); }} let mut items_{depth} = items_{depth}.into_iter(); ({taken}) }}",
                        count = members.len()
                    ),
                    kind: LeaseKind::Tuple(members),
                })
            }
            ty if !holds_borrow(ty) => {
                let rust = concrete_type(ty)?;
                let mut converted = String::new();
                render_from_dynamic(ty, &format!("value_{depth}"), depth, &mut converted).ok()?;
                Some(Self {
                    held: format!("Option<{rust}>"),
                    take: format!("{{ let value_{depth} = value_{depth}.flatten(); Some(({converted})?) }}"),
                    kind: LeaseKind::Owned,
                })
            }
            _ => None,
        }
    }

    /// The borrow of what `place`, a mutable place of `held` at `depth`,
    /// lends.
    fn borrow(&self, place: &str, depth: usize) -> String {
        match &self.kind {
            LeaseKind::Native { mutable: true } => format!("{place}.get_mut()"),
            LeaseKind::Native { mutable: false } => format!("&*{place}.get_mut()"),
            LeaseKind::Std => format!("&*{place}"),
            LeaseKind::Made => format!("&{place}"),
            LeaseKind::Owned => format!(
                "{place}.take().expect(\"a value that borrows nothing moves into its call once\")"
            ),
            LeaseKind::Result(ok, err) => {
                let next = depth + 1;
                format!(
                    "match {place}.as_mut() {{ Ok(held_{next}) => Ok({}), Err(held_{next}) => Err({}) }}",
                    ok.borrow(&format!("(*held_{next})"), next),
                    err.borrow(&format!("(*held_{next})"), next)
                )
            }
            LeaseKind::Str { pinned: true } => place.to_owned(),
            LeaseKind::Str { pinned: false } => format!("{place}.as_str()"),
            LeaseKind::Tuple(members) => {
                let borrows = members
                    .iter()
                    .enumerate()
                    .map(|(member, lease)| {
                        format!("{}, ", lease.borrow(&format!("{place}.{member}"), depth))
                    })
                    .collect::<String>();
                format!("({borrows})")
            }
            LeaseKind::Option(inner) => {
                let next = depth + 1;
                format!(
                    "{place}.as_mut().map(|held_{next}| {})",
                    inner.borrow(&format!("(*held_{next})"), next)
                )
            }
        }
    }
}

/// Whether `ty` holds a borrow: a `&T`, `&mut T` or `&str`, alone or in an
/// `Option`, a `Result` or a tuple.
fn holds_borrow(ty: &convert::Type) -> bool {
    match ty {
        convert::Type::Borrowed { .. } | convert::Type::Builtin(convert::Builtin::Str) => true,
        convert::Type::Option(element) => holds_borrow(element),
        convert::Type::Result(ok, err) => holds_borrow(ok) || holds_borrow(err),
        convert::Type::Tuple(members) => members.iter().any(holds_borrow),
        _ => false,
    }
}

/// The statements that move what lends each borrowed input into a future
/// and borrow it there again, as `render_take_input` bound it: a leased
/// value (`render_lease`), a value made for the call, or what a type that
/// holds borrows leases (`Lease`). Empty when no input borrows.
fn future_borrows(
    signature: &crate::classify::representability::signature::Signature,
    substitutions: &[convert::Type],
) -> Result<String, request::Error> {
    let mut rebound = String::new();
    for (local, ty) in signature.inputs().iter().enumerate() {
        let substituted = substituted(ty, signature, substitutions)?;
        if matches!(
            substituted,
            convert::Type::Option(_) | convert::Type::Result(..) | convert::Type::Tuple(_)
        ) && let Some(lease) = Lease::of(&substituted, true, 1)
        {
            let borrow = lease.borrow(&format!("argument_{local}_held"), 1);
            rebound.push_str(&format!(
                "        #[allow(unused_mut)]\n        let mut argument_{local}_held = argument_{local}_held;\n        let argument_{local} = {borrow};\n"
            ));
            continue;
        }
        let convert::Type::Borrowed { mutable, ty: lent } = substituted else {
            continue;
        };
        // A `&str` a type parameter takes is pinned, as a `&'static str`
        // is (`render_take_input`): the future copies the reference.
        if *lent == convert::Type::Builtin(convert::Builtin::Str) {
            continue;
        }
        if !mutable && is_made_for_call(&lent) {
            rebound.push_str(&format!(
                "        let argument_{local}_made = argument_{local}_made;\n        let argument_{local} = &argument_{local}_made;\n"
            ));
        } else if !mutable && std_value(&lent).is_some() {
            // One of std's values Rawr's std holds, as `&Path`: the value
            // lent from its script value, held for the future's life.
            rebound.push_str(&format!(
                "        let argument_{local}_lent = argument_{local}_lent;\n        let argument_{local} = &*argument_{local}_lent;\n"
            ));
        } else {
            let borrow = if mutable { "" } else { "&*" };
            rebound.push_str(&format!(
                "        #[allow(unused_mut)]\n        let mut argument_{local}_held = argument_{local}_held;\n        let argument_{local} = {borrow}argument_{local}_held.get_mut();\n"
            ));
        }
    }
    Ok(rebound)
}

/// A `&T` or `&mut T` input of a value in the arena, as ratatui's
/// `render_stateful_widget` takes `&mut ListState`: the value leaves its
/// slot for the call, which refuses every other use meanwhile (E0499), and
/// goes back when the lease drops, before the arena is next used (D558.5).
/// Only a `future` that may run on another thread holds it as a `Held`,
/// which asks `Send`; a call holds it on the owner thread, so a value that
/// isn't `Send`, as rand's `ThreadRng`, is lent as rustc lends it.
fn render_lease(
    local: usize,
    index: usize,
    ty: &convert::Type,
    future: bool,
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    let convert::Type::Borrowed { mutable, ty } = ty else {
        return Err(request::drift("a lease of other than a borrow"));
    };
    let rust = concrete_type(ty).ok_or_else(|| request::drift("borrowed input type"))?;
    let borrow = if *mutable { "" } else { "&*" };
    let (held, lease) = if future {
        ("Held", "lease")
    } else {
        ("PlaceHeld", "lease_place")
    };
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
    let mut argument_{local}_held: {held}<{rust}>;
    let argument_{local}: {reference}{rust} = match lent_argument(lent, {index}) {{
        Some(value) => value.{reach}::<{rust}>().ok_or(Error::Lent({rust:?}, __rawr_internal::scope::Error::Mismatch))?,
        None => {{
            argument_{local}_held = {{ let handle = native_handle(arguments, {index}, false)?; arguments.arena.{lease}(handle)? }};
            {borrow}argument_{local}_held.get_mut()
        }}
    }};"
        )
        .map_err(request::unwritable);
    }
    writeln!(
        output,
        "    let mut argument_{local}_held: {held}<{rust}> = {{ let handle = native_handle(arguments, {index}, false)?; arguments.arena.{lease}(handle)? }};\n    let argument_{local} = {borrow}argument_{local}_held.get_mut();"
    )
    .map_err(request::unwritable)
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
    let source =
        format!("(**arguments.values.get_mut({index}).ok_or(Error::MissingArgument)?).take()");
    // A closure whose output is at a lifetime of the call's own, as
    // pane_grid's `PaneGrid::new(state, |id, pane, max| Content<'a, ..>)`
    // takes: made where the call is, so in the region of a call made in
    // one it gives its output at that region's lifetime (`regional`).
    if boxed_callable(signature.output()).is_none()
        && regional(call, signature, substitutions)?.is_some()
    {
        if REGION_ARGUMENTS.get() {
            let closure = render_callable_closure(
                index,
                &source,
                call,
                signature,
                substitutions,
                Some("region.pins()"),
            )?;
            return writeln!(output, "    let argument_{local} = {closure};")
                .map_err(request::unwritable);
        }
        // Out of a region the call may still be made in a slot's
        // (`render_anchored_output`), which takes the closure again.
        writeln!(output, "    let argument_{local}_source = {source};")
            .map_err(request::unwritable)?;
        let source = format!("argument_{local}_source.clone()");
        let closure = render_callable_closure(
            index,
            &source,
            call,
            signature,
            substitutions,
            Some("region.pins()"),
        )?;
        REGIONAL_CLOSURES.with_borrow_mut(|closures| {
            closures.push(format!("\n            let argument_{local} = {closure};"));
        });
        let closure =
            render_callable_closure(index, &source, call, signature, substitutions, None)?;
        return writeln!(output, "    let argument_{local} = {closure};")
            .map_err(request::unwritable);
    }
    let closure = render_callable_closure(index, &source, call, signature, substitutions, None)?;
    // An output that names the closure's type, as iced's `Widget::map`
    // gives `Map<Self, F, Message>`: the closure boxed as the `Box<dyn
    // Fn(..) -> ..>` the classifier wrote there (T417).
    if let Some(boxed) = boxed_callable(signature.output()) {
        let rust = concrete_type(&substituted(boxed, signature, substitutions)?)
            .ok_or_else(|| request::drift("a boxed closure's type"))?;
        return writeln!(
            output,
            "    let argument_{local}: {rust} = Box::new({closure});"
        )
        .map_err(request::unwritable);
    }
    writeln!(output, "    let argument_{local} = {closure};").map_err(request::unwritable)
}

/// The boxed closure type `ty` names, as `Map<W, Box<dyn Fn(A) -> B>, A>`
/// does (T417).
fn boxed_callable(ty: &convert::Type) -> Option<&convert::Type> {
    match ty {
        convert::Type::Nominal { definition, .. }
            if *definition == crate::surface::identity::Definition::callable() =>
        {
            Some(ty)
        }
        convert::Type::Nominal { arguments, .. }
        | convert::Type::Tuple(arguments)
        | convert::Type::Script { arguments, .. } => arguments.iter().find_map(boxed_callable),
        convert::Type::Option(inner)
        | convert::Type::Range(inner)
        | convert::Type::Borrowed { ty: inner, .. }
        | convert::Type::Sequence { element: inner, .. } => boxed_callable(inner),
        convert::Type::Result(ok, err) => boxed_callable(ok).or_else(|| boxed_callable(err)),
        _ => None,
    }
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
    let closure = render_callable_closure(index, "inner", call, signature, substitutions, None)?;
    writeln!(
        output,
        "    let argument_{local} = match __rawr_internal::option::take(&mut **arguments.values.get_mut({index}).ok_or(Error::MissingArgument)?).ok_or(Error::TypeMismatch)? {{\n        None => None,\n        Some(inner) => Some({closure}),\n    }};"
    )
    .map_err(request::unwritable)
}

/// The family of the output of the closure `call` fills when it is at a
/// lifetime of the call's own, as `Content<'a, ..>` of pane_grid's
/// `PaneGrid::new(state, view: impl Fn(..) -> Content<'a, ..>)` is: the
/// closure is called while the call runs, so its output is made where the
/// call is. Not one a type parameter names, as `W` of iced's `lazy` or
/// `responsive`, whose closure-filled argument is at `'static` (D761).
fn regional(
    call: &crate::classify::representability::callable::Call,
    signature: &crate::classify::representability::signature::Signature,
    substitutions: &[convert::Type],
) -> Result<Option<String>, request::Error> {
    if !matches!(call.output(), convert::Type::Nominal { .. }) {
        return Ok(None);
    }
    let returned = substituted(call.output(), signature, substitutions)?;
    if !carries_lifetime(&returned) {
        return Ok(None);
    }
    Ok(written_family(&returned))
}

/// The block that makes a Rust closure calling the script callable
/// `source` gives, for [`render_take_callable`]. With `pins`, the pins of
/// the region its call is made in, the closure's call runs in a region of
/// its own anchored on them, as a slot's does (`render_slot`), and its
/// output comes out of it at their lifetime (`regional`).
fn render_callable_closure(
    index: usize,
    source: &str,
    call: &crate::classify::representability::callable::Call,
    signature: &crate::classify::representability::signature::Signature,
    substitutions: &[convert::Type],
    pins: Option<&str>,
) -> Result<String, request::Error> {
    let regional = match pins {
        Some(pins) => regional(call, signature, substitutions)?.map(|family| (pins, family)),
        None => None,
    };
    let by_value = |ty: &convert::Type| {
        !matches!(ty, convert::Type::Nominal { .. })
            || is_value(ty)
            || is_duration(ty)
            || boxed_element(ty).is_some()
            || counted_element(ty).is_some()
            || keyed_element(ty).is_some()
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
    // An input that holds a native value, as a `Result<Bytes, String>`
    // does, inserts it into the arena too.
    let native_inputs = inputs
        .iter()
        .any(|input| native(input) || holds_native(input));
    let native_output = !by_value(&returned) || holds_native(&returned);
    let mut parameters = Vec::new();
    let mut values = Vec::new();
    let mut places = Vec::new();
    let mut lent = Vec::new();
    let mut lends = Vec::new();
    let mut restores = String::new();
    // Whether an input takes its parameter's witness, which a lent one
    // doesn't.
    let mut attested = false;
    for (position, input) in inputs.iter().enumerate() {
        // A borrow of a script value, as `.theme(|state| ..)` takes the
        // program's `&State`: lent by its handle, as a slot lends it, and
        // `&mut` as the closure's own place, given back after the call.
        if let convert::Type::Borrowed { mutable, ty } = input
            && matches!(**ty, convert::Type::Script { .. })
        {
            let rust = concrete_type(ty).ok_or_else(|| request::drift("closure input type"))?;
            if *mutable {
                parameters.push(format!("input_{position}: &mut {rust}"));
                values.push(format!("input_{position}.0.lend_mut()"));
                lent.push(position.to_string());
                restores.push_str(&format!(
                    "\n            input_{position}.0.restore(std::mem::take(&mut values[{position}]));"
                ));
            } else {
                parameters.push(format!("input_{position}: &{rust}"));
                values.push(format!("input_{position}.0.lend()"));
            }
            continue;
        }
        // A native value the closure borrows `&mut`, as iced's
        // `renderer.with_translation(v, |renderer| ..)` lends the renderer:
        // lent for the call alone, as a slot lends it (`render_slot`), so
        // the closure changes the value itself.
        if let convert::Type::Borrowed { mutable: true, ty } = input
            && lent_mutably(ty)
        {
            let rust = concrete_type(ty).ok_or_else(|| request::drift("closure input type"))?;
            parameters.push(format!("input_{position}: &mut {rust}"));
            lent.push(position.to_string());
            let lend = LentInput::new(position, true, rust);
            values.push(lend.value());
            lends.push(lend);
            continue;
        }
        let rust = concrete_type(input).ok_or_else(|| request::drift("closure input type"))?;
        parameters.push(format!("input_{position}: {rust}"));
        if let convert::Type::Borrowed { mutable, ty } = input
            && is_copied(ty)
            && place::available(ty)
        {
            places.push((position, *mutable, place::policy(ty)?));
            values.push(format!("place_{position}.clone()"));
            continue;
        }
        let mut converted = String::new();
        {
            let _inputs = ScriptInputs::enter();
            render_to_dynamic(input, &format!("input_{position}"), 0, &mut converted)?;
        }
        // A native value takes the closure parameter's witness, so script
        // code reads it as its type, as a `match` on an imported enum does.
        // So does a value that holds one, as a slot's input does.
        if native(input) || holds_native(input) {
            attested = true;
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
    let rust = concrete_type(&returned).ok_or_else(|| request::drift("closure output type"))?;
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
    } else if attested || !places.is_empty() {
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
    let mutable = places
        .iter()
        .filter(|(_, mutable, _)| *mutable)
        .map(|(at, _, _)| at.to_string())
        .chain(lent)
        .collect::<Vec<_>>();
    let call = if mutable.is_empty() {
        call.to_owned()
    } else if inputs.iter().any(is_value) {
        format!(
            "callable.call_places_checked(&mut values, &[{}], &witnesses)",
            mutable.join(", ")
        )
    } else {
        format!(
            "callable.call_places(&mut values, &[{}])",
            mutable.join(", ")
        )
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
    // The conversion's `?` is its own, not the closure's, whose output may
    // be an `Option`.
    let converted = if native_output {
        in_arena(&converted)
    } else {
        format!("(|| {converted})()")
    };
    // An arena of the call's own on a thread with none (`CallArena`), which
    // a script value the closure gives takes its native values out of
    // (`script_proxy`).
    let call_arena = if native_inputs || native_output || holds_script(&returned) {
        "let _arena = CallArena::enter();\n            "
    } else {
        ""
    };
    let body = match &regional {
        // The values the call makes go in the closure's region, which
        // keeps the strings they borrow in the pins it is anchored on.
        Some((_, family)) => format!(
            "{call_arena}let mut values = {values};
            let mut region = match pins {{
                Some(pins) => __rawr_internal::scope::Region::anchored(pins),
                None => __rawr_internal::scope::Region::new(),
            }};
            let value = __rawr_internal::scope::open_anchored(&mut region, || {call}).unwrap_or_else(|error| panic!(\"{{}}\", __rawr_internal::future::message(&error)));{restores}
            let scoped = value
                .read_lock::<__rawr_internal::scope::Scoped>()
                .map(|scoped| *scoped)
                .unwrap_or_else(|| panic!(\"rawr: a closure gave native code a value that isn't of the region its call runs in\"));
            region
                .take::<{family}>(scoped)
                .unwrap_or_else(|error| panic!(\"rawr: {{error}}\"))"
        ),
        None => format!(
            "{call_arena}let mut values = {values};\n            let value = {call}.unwrap_or_else(|error| panic!(\"{{}}\", __rawr_internal::future::message(&error)));{restores}\n            {converted}.unwrap_or_else(|error| panic!(\"rawr: a closure gave back a value native code can't take: {{error:?}}\"))"
        ),
    };
    // A script value the closure gives on a thread with no arena, as
    // iced's executor runs `Subscription::map`'s closure, is made on the
    // owner thread where it is first used (`DeferProbe`), as a slot's is:
    // its body may make native values, which only the owner's arena holds.
    let deferred = matches!(returned, convert::Type::Script { .. })
        && regional.is_none()
        && places.is_empty()
        && lends.is_empty()
        && !inputs
            .iter()
            .any(|input| matches!(input, convert::Type::Borrowed { .. }));
    let body = if deferred {
        deferred_closure_body(inputs.len(), !witnesses.is_empty(), &rust, &body)
    } else {
        body
    };
    let body = place::around(&places, "witnesses.clone()", body);
    let (items, body) = lent_around(&lends, body);
    let body = format!("{items}{body}");
    // A regional output is at the lifetime of the pins, which rustc infers
    // from them, as no Rust code here can name it.
    let (pinned, returns) = match regional {
        Some((pins, _)) => (format!("\n        let pins = {pins};"), String::new()),
        None => (String::new(), format!(" -> {rust}")),
    };
    Ok(format!(
        "{{\n        let callable = __rawr_internal::future::Callable::new({source})\n            .map_err(|error| Error::Value(__rawr_internal::future::message(&error)))?;{witnesses}{pinned}\n        move |{}|{returns} {{\n            {body}\n        }}\n    }}",
        parameters.join(", "),
    ))
}

/// `body`, a closure's call of its script callable giving the proxy type
/// `rust`, made where its value is first used when the closure runs on a
/// thread with no arena: the callable, the witnesses when `witnessed`, and
/// the `count` inputs move to the owner thread with the call.
fn deferred_closure_body(count: usize, witnessed: bool, rust: &str, body: &str) -> String {
    let names = (0..count)
        .map(|position| format!("input_{position},"))
        .collect::<String>();
    let (taken, kept, dropped) = if witnessed {
        (
            "callable.clone(), witnesses.clone(),",
            "callable, witnesses,",
            "_, _,",
        )
    } else {
        ("callable.clone(),", "callable,", "_,")
    };
    format!(
        "let ({names}) = if arena_access(|_| ()).is_none() {{
                let probe = DeferProbe(std::cell::Cell::new(Some(({taken} {names}))));
                if let Some(proxy) = (&probe).defer(|({kept} {names})| {{
                    let value: {rust} = {{ {body} }};
                    value.0.into_dynamic()
                }}) {{
                    return <{rust}>::from_proxy(proxy);
                }}
                let ({dropped} {names}) = probe.0.take().expect(\"rawr: a closure's inputs are kept until deferred\");
                ({names})
            }} else {{
                ({names})
            }};
            {body}"
    )
}

fn render_fields(
    fields: &[crate::surface::adapter::Field],
    offset: usize,
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    for (index, field) in fields.iter().enumerate() {
        render_take_field(index, index + offset, field.ty(), output)?;
    }
    Ok(())
}

/// A constructor's argument for a field: as `render_take`, and a slice of
/// scalars or a `&str` the field borrows for its struct's lifetime, as
/// iced's `LineDash { segments: &[3.0, 6.0], .. }`, kept for as long as
/// the runner lives once for each content (`render_from_dynamic`).
fn render_take_field(
    local: usize,
    index: usize,
    ty: &convert::Type,
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    let convert::Type::Borrowed {
        mutable: false,
        ty: lent,
    } = ty
    else {
        return render_take(local, index, ty, output);
    };
    let kept = matches!(lent.as_ref(), convert::Type::Sequence { element, length: None, .. }
        if matches!(**element, convert::Type::Builtin(builtin) if builtin.rust_name().is_some()))
        || **lent == convert::Type::Builtin(convert::Builtin::Str);
    let mut converted = String::new();
    if kept {
        render_from_dynamic(ty, "value", 0, &mut converted)?;
    } else if matches!(lent.as_ref(), convert::Type::Sequence { length: None, .. }) {
        // A slice of other values, as a page's `children: &'static
        // [Block]` that a literal `&[Block::Md(".."), ..]` fills (#1283):
        // the values, moved into a slice kept for as long as the runner
        // lives, as rustc keeps a promoted literal's.
        render_from_dynamic(lent, "value", 0, &mut converted)?;
        converted = format!("({converted}).map(|values| &*Vec::leak(values))");
    } else {
        return render_take(local, index, ty, output);
    }
    writeln!(
        output,
        "    let argument_{local} = {{ let value = (**arguments.values.get_mut({index}).ok_or(Error::MissingArgument)?).take(); {converted} }}?;"
    )
    .map_err(request::unwritable)
}

fn render_take(
    local: usize,
    index: usize,
    ty: &convert::Type,
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    // A `&'static str` input is the program's own string, pinned for as
    // long as the runner lives, as a string literal is: the site's pins
    // hold one string per text, however often the call runs, as iced's
    // `Font::with_name` does every frame (`Pins::constant`). Nothing is
    // copied or leaked.
    if matches!(ty, convert::Type::Builtin(convert::Builtin::StaticStr)) {
        return writeln!(
            output,
            "    let argument_{local} = {{ static PINS: __rawr_internal::scope::Pins = __rawr_internal::scope::Pins::constant(); PINS.pin(take_primitive::<ImmutableString>(arguments, {index})?) }};"
        )
        .map_err(request::unwritable);
    }
    if PINNED_STRINGS.get() && matches!(ty, convert::Type::Builtin(convert::Builtin::Str)) {
        return writeln!(
            output,
            "    let argument_{local} = static_pins().pin(take_primitive::<ImmutableString>(arguments, {index})?);"
        )
        .map_err(request::unwritable);
    }
    // A `&str` input lends the string taken from Rawr, which a region can
    // pin by its shared handle: no text is copied.
    if matches!(ty, convert::Type::Builtin(convert::Builtin::Str)) {
        return writeln!(
            output,
            "    let argument_{local}_owned = take_primitive::<ImmutableString>(arguments, {index})?;\n    let argument_{local} = argument_{local}_owned.as_str();"
        )
        .map_err(request::unwritable);
    }
    // A `Cow<'a, str>` input, as `impl From<Cow<'a, str>> for Slot<'a, M>`
    // takes: the program's `Cow<str>`'s text, owned, at the type the call
    // names, so the impl rustc selects is that one beside the item's other
    // `From` impls (#1390).
    if matches!(ty, convert::Type::Builtin(convert::Builtin::Text)) {
        return writeln!(
            output,
            "    let argument_{local} = std::borrow::Cow::<'static, str>::Owned(take_text(arguments, {index})?);"
        )
        .map_err(request::unwritable);
    }
    let helper = match ty {
        convert::Type::ConstArray { .. } => return Err(unlengthed()),
        convert::Type::Builtin(convert::Builtin::String) => "take_string",
        convert::Type::Builtin(convert::Builtin::InclusiveRange) => "take_range",
        // Named, so an item rustc chooses by its input's type, as
        // `Pixels::from` of a `From<u32>` beside a `From<f32>`, is the one
        // the binding is.
        convert::Type::Builtin(builtin) if builtin.is_narrow_integer() => {
            let rust = builtin
                .rust_name()
                .ok_or_else(|| request::drift("a narrow integer's Rust name"))?;
            return writeln!(
                output,
                "    let argument_{local} = take_int::<{rust}>(arguments, {index})?;"
            )
            .map_err(request::unwritable);
        }
        convert::Type::Builtin(convert::Builtin::F32) => "take_f32",
        ty if let Some(rust) = std_value(ty) => {
            return writeln!(
                output,
                "    let argument_{local} = take_std::<{rust}>(arguments, {index})?;"
            )
            .map_err(request::unwritable);
        }
        ty if is_duration(ty) || is_facade_value(ty) => "take_primitive",
        ty if is_value(ty) => "take_value",
        // A script value where native generic code takes a Rust type: the
        // runner's proxy for its type (D594).
        convert::Type::Script { .. } => {
            let rust = concrete_type(ty).ok_or_else(|| request::drift("a script type's proxy"))?;
            return writeln!(
                output,
                "    let argument_{local} = <{rust}>::from_proxy(__rawr_internal::crossing::Proxy::from_place(&mut **arguments.values.get_mut({index}).ok_or(Error::MissingArgument)?));"
            )
            .map_err(request::unwritable);
        }
        // A program's iterator where native code takes one, as itertools'
        // `cartesian_product(self, other: J)` takes `Self: Iterator` and `J:
        // IntoIterator`: its step function, pulled on each `next()`, and
        // each item made the call's `T` where it is pulled.
        ty if let Some(item) = items_element(ty) => {
            let mut converted = String::new();
            render_from_dynamic(item, "value", 0, &mut converted)?;
            // A native item comes out of the arena, as a closure's native
            // output does (`render_callable_closure`).
            let native = matches!(item, convert::Type::Nominal { .. })
                && !is_value(item)
                && !is_duration(item)
                && boxed_element(item).is_none()
                && counted_element(item).is_none()
                && keyed_element(item).is_none();
            let converted = if native || holds_native(item) {
                format!(
                    "{{ let _arena = CallArena::enter(); with_arena(|arena| {{ let mut none: [&mut Dynamic; 0] = []; let arguments = &mut Arguments {{ values: &mut none, arena }}; let _ = &arguments; {converted} }}) }}"
                )
            } else {
                format!("(|| {converted})()")
            };
            return writeln!(
                output,
                "    let argument_{local} = __rawr_internal::future::Items::new((**arguments.values.get_mut({index}).ok_or(Error::MissingArgument)?).take(), |value: Dynamic| {converted}.unwrap_or_else(|error| panic!(\"rawr: an iterator gave an item native code can't take: {{error:?}}\"))).map_err(|error| Error::Value(__rawr_internal::future::message(&error)))?;"
            )
            .map_err(request::unwritable);
        }
        // A script future where native code takes a future of a type of its
        // own, as iced's `Task::future(impl Future<Output = T>)` of the
        // call's `T`: its output becomes that type on the owner thread,
        // where the program's native values are (D683.5).
        ty if let Some(element) = future_output(ty)
            && !is_value(element)
            && *element != convert::Type::Inferred =>
        {
            let mut converted = String::new();
            render_from_dynamic(element, "value", 0, &mut converted)?;
            return writeln!(
                output,
                "    let argument_{local} = take_future(arguments, {index})?.into_native(|value: Dynamic| with_arena(|arena| {{ let mut none: [&mut Dynamic; 0] = []; let arguments = &mut Arguments {{ values: &mut none, arena }}; let _ = &arguments; {converted} }}).unwrap_or_else(|error| panic!(\"rawr: an async block gave back a value native code can't take: {{error:?}}\")));"
            )
            .map_err(request::unwritable);
        }
        // A script future, run as a std future (D527).
        ty if is_future(ty) => "take_script",
        // Rawr's float or integer range, as the range of the number the
        // instance gives.
        // An `Rc` or `Arc` the call takes by value, with copies of the
        // crate's pointer for rawr's others held while it runs (`Balance`).
        ty if let Some((pointer, element)) = counted_element(ty) => {
            let element = concrete_type(element)
                .ok_or_else(|| request::drift("a counted pointer's element type"))?;
            return writeln!(
                output,
                "    let (argument_{local}, _argument_{local}_balance) = {{ let value = (**arguments.values.get_mut({index}).ok_or(Error::MissingArgument)?).take(); take_pointer::<{pointer}<{element}>>(arguments.arena, value)? }};"
            )
            .map_err(request::unwritable);
        }
        ty if range_inclusive_element(ty).is_some()
            || boxed_element(ty).is_some()
            || keyed_element(ty).is_some() =>
        {
            let mut converted = String::new();
            render_from_dynamic(ty, "value", 0, &mut converted)?;
            return writeln!(
                output,
                "    let argument_{local} = {{ let value = (**arguments.values.get_mut({index}).ok_or(Error::MissingArgument)?).take(); {converted} }}?;"
            )
            .map_err(request::unwritable);
        }
        // A value of a generic impl function's own parameter's opaque type,
        // as `p.fits(p.font())` passes a `P::Font`, is in the function's
        // frame at the call's types (`render_frame`), which needn't be
        // `'static` as the arena's are.
        convert::Type::Nominal { definition, .. }
            if let Some((frame, slot)) = frame_slot(definition) =>
        {
            return writeln!(
                output,
                "    let argument_{local} = {frame}.take_{slot}(arguments, {index})?;"
            )
            .map_err(request::unwritable);
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
        // A slice of scalars a type parameter takes, as `&[u8]` fills iced's
        // `image::Handle::from_bytes(impl Into<Bytes>)` (#954): kept for the
        // runner's life, as a struct's borrowed slice field is.
        convert::Type::Borrowed {
            mutable: false,
            ty: lent,
        } if matches!(lent.as_ref(), convert::Type::Sequence { element, length: None, .. }
                if matches!(**element, convert::Type::Builtin(builtin) if builtin.rust_name().is_some())) =>
        {
            let mut converted = String::new();
            render_from_dynamic(ty, "value", 0, &mut converted)?;
            return writeln!(
                output,
                "    let argument_{local} = {{ let value = (**arguments.values.get_mut({index}).ok_or(Error::MissingArgument)?).take(); {converted} }}?;"
            )
            .map_err(request::unwritable);
        }
        convert::Type::FunctionItem { .. } | convert::Type::Borrowed { .. } => {
            return Err(request::drift(
                "static function item entered runtime argument decoding",
            ));
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
            .map_err(request::unwritable);
        }
        convert::Type::Inferred => {
            return Err(request::drift(
                "an inferred type parameter reached a runtime value",
            ))
        }
        // Named as narrow integers are: `Sides::from(6)` beside core's
        // reflexive `From<Sides>` is rustc's `From<i64>` only when the
        // argument's type is written.
        convert::Type::Builtin(builtin) if let Some(rust) = builtin.rust_name() => {
            return writeln!(
                output,
                "    let argument_{local} = take_primitive::<{rust}>(arguments, {index})?;"
            )
            .map_err(request::unwritable);
        }
        convert::Type::Unit | convert::Type::Never | convert::Type::Builtin(_) => "take_primitive",
    };
    writeln!(
        output,
        "    let argument_{local} = {helper}(arguments, {index})?;"
    )
    .map_err(request::unwritable)
}

fn render_give(
    ty: &convert::Type,
    value: &str,
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    match ty {
    convert::Type::ConstArray { .. } => {
            return Err(unlengthed())
        }
        // A value of a generic impl function's own parameter's opaque type
        // stays in its frame (`render_frame`).
        convert::Type::Nominal { definition, .. } if let Some((frame, slot)) = frame_slot(definition) => {
            writeln!(output, "    {frame}.give_{slot}(arguments.arena, {value})")
        }
        ty if boxed_element(ty).is_some()
            || counted_element(ty).is_some()
            || keyed_element(ty).is_some()
            || joined_element(ty).is_some()
            || matches!(range_inclusive_element(ty), Some(convert::Type::Nominal { .. }))
            || matches!(range_inclusive_element(ty), Some(convert::Type::Builtin(builtin))
                if *builtin == convert::Builtin::I64 || builtin.is_narrow_integer()) =>
        {
            let mut converted = String::new();
            render_to_dynamic(ty, value, 0, &mut converted)?;
            writeln!(output, "    {converted}")
        }
        ty if std_value(ty).is_some() => writeln!(
            output,
            "    Ok(__rawr_internal::crossing::Std::give({value}))"
        ),
        ty if is_duration(ty) || is_facade_value(ty) => {
            writeln!(output, "    Ok(Dynamic::from({value}))")
        }
        ty if is_value(ty) => writeln!(output, "    Ok({value}.into_inner())"),
        convert::Type::Script { .. } => {
            writeln!(output, "    Ok(give_proxy(arguments.arena, {value}.0))")
        }
        // A value that carries the lifetime of the scope frame's region the
        // call runs in stays there (D558 tier 1), when its type has a name:
        // one that leaves an argument to rustc is held as any native value.
        convert::Type::Nominal { .. }
            if IN_REGION.get()
                && carries_lifetime(ty)
                && let Some(family) = family_of(ty).filter(|family| family.named().is_some()) =>
        {
            writeln!(
                output,
                "    Ok(Dynamic::from(region.{}))",
                family.call(family::Reach::Insert, &held_value(ty, value))
            )
        }
        // An `impl Fn(A..) -> B` is held boxed, as a call that takes a
        // callable takes it (`boxed_callable_type`).
        convert::Type::Nominal { .. } if let Some(boxed) = boxed_callable_type(ty) => {
            writeln!(
                output,
                "    let boxed: {boxed} = Box::new({value});\n    insert_native!(arguments.arena, boxed).map(Dynamic::from)"
            )
        }
        // A value whose type holds an opaque type carries the calls that
        // take it, compiled here (`opaque_table_source`), or those of the
        // value of the same type the call took.
        convert::Type::Nominal { .. } if holds_opaque(ty) && OPAQUE_WITNESSING.get() => {
            writeln!(
                output,
                "    witness = witness_of(&{value});\n    Err(Error::TypeMismatch)"
            )
        }
        convert::Type::Nominal { .. } if holds_opaque(ty) => {
            let table = if OPAQUE_CONSUMING.get() {
                Some("table".to_owned())
            } else {
                OPAQUE_PRODUCING.with_borrow(|table| {
                    table.as_ref().map(|table| {
                        table.replace("__RAWR_WITNESS__", &format!("witness_of(&{value})"))
                    })
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
            writeln!(output, "    Ok(Dynamic::from(<f64 as From<f32>>::from({value})))")
        }
        // An output Rawr cannot hold: its `Display` text.
        convert::Type::Builtin(convert::Builtin::Text) => {
            writeln!(output, "    Ok(Dynamic::from({value}.to_string()))")
        }
        // The unit type, or a `PhantomData` field, which Rawr holds as it.
        convert::Type::Inferred => {
            return Err(request::drift("an inferred type parameter reached a runtime value"))
        }
        convert::Type::Unit | convert::Type::Never => {
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
                .map_err(request::unwritable)?;
            return render_give(lent, "copied", output);
        }
        convert::Type::FunctionItem { .. } | convert::Type::Borrowed { .. } => {
            return Err(request::drift("static function item entered runtime output encoding"));
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
    .map_err(request::unwritable)
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
        convert::Type::Nominal { definition, .. } if let Some((frame, slot)) = frame_slot(definition) => write!(
            output,
            "{value}.try_cast_result::<NativeHandle>().map_err(|_| Error::TypeMismatch).and_then(|handle| {frame}.out_{slot}(arguments.arena, handle))"
        ),
        convert::Type::Inferred => return Err(request::drift("an inferred type parameter reached a runtime value")),
        convert::Type::ConstArray { .. } => {
            return Err(unlengthed())
        }
        convert::Type::Unit | convert::Type::Never => cast(output, "()"),
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
        // Rawr's `Box`, which holds its value as itself, as Rust's `Box`.
        ty if let Some(element) = boxed_element(ty) => {
            let mut converted = String::new();
            render_from_dynamic(element, value, depth, &mut converted)?;
            write!(output, "({converted}).map(Box::new)")
        }
        // Rawr's map or set, as std's of its kind, made from its entries
        // (`keyed_element`).
        ty if let Some((_, members)) = keyed_element(ty) => {
            let rust = concrete_type(ty)
                .ok_or_else(|| request::drift("a keyed collection's type"))?;
            let entry = format!("entry_{depth}");
            let converted = render_entry_from_dynamic(members, &entry, depth)?;
            write!(
                output,
                "{{ let value = {value}; value.into_array().map_err(|_| Error::TypeMismatch).and_then(|items| items.into_iter().map(|{entry}| {converted}).collect::<Result<{rust}, Error>>()) }}"
            )
        }
        // Rawr's `Rc` or `Arc` of a crate value, as the crate's own pointer
        // its value is in, made the first time it crosses (#775).
        ty if let Some((pointer, element)) = counted_element(ty) => {
            let element = concrete_type(element)
                .ok_or_else(|| request::drift("a counted pointer's element type"))?;
            write!(
                output,
                "take_pointer::<{pointer}<{element}>>(arguments.arena, {value}).map(|(pointer, _)| pointer)"
            )
        }
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
            // Rawr's range of a crate type's bounds, as iced's
            // `Radians::RANGE` gives: Rust's of the bounds it holds.
            element @ convert::Type::Nominal { .. } => {
                let mut start = String::new();
                render_from_dynamic(element, &format!("start_{depth}"), depth + 1, &mut start)?;
                let mut end = String::new();
                render_from_dynamic(element, &format!("end_{depth}"), depth + 1, &mut end)?;
                write!(
                    output,
                    "{value}.try_cast_result::<std::ops::RangeInclusive<Dynamic>>().map_err(|_| Error::TypeMismatch).and_then(|range| {{ let (start_{depth}, end_{depth}) = range.into_inner(); Ok::<_, Error>(({start})?..=({end})?) }})"
                )
            }
            _ => {
                return Err(request::drift("an inclusive range of a type that is not a number"))
            }
        },
        convert::Type::Builtin(convert::Builtin::StaticStr) => write!(
            output,
            "{value}.try_cast_result::<ImmutableString>().map(|value| static_pins().pin(value)).map_err(|_| Error::TypeMismatch)"
        ),
        convert::Type::Builtin(convert::Builtin::String) => write!(
            output,
            "{{ let value = {value}; if let Some(place) = __rawr_internal::place::get(&value) {{ __rawr_internal::place::take::<__rawr_internal::place::text::Text>(&place, place.witness()).map_err(|error| Error::Value(error.to_string())) }} else {{ value.try_cast_result::<ImmutableString>().map(|value| value.to_string()).map_err(|_| Error::TypeMismatch) }} }}"
        ),
        convert::Type::Builtin(convert::Builtin::Str | convert::Builtin::Text) => write!(
            output,
            "{value}.try_cast_result::<ImmutableString>().map(|value| value.to_string()).map_err(|_| Error::TypeMismatch)"
        ),
        ty if let Some(rust) = std_value(ty) => write!(
            output,
            "<{rust} as __rawr_internal::crossing::Std>::take({value}).ok_or(Error::TypeMismatch)"
        ),
        ty if let Some(time) = std_time(ty) => cast(output, time),
        ty if is_facade_value(ty) => {
            let rust = concrete_type(ty).ok_or_else(|| request::drift("a host value's Rust type"))?;
            cast(output, &rust)
        }
        ty if is_value(ty) => write!(
            output,
            "Ok::<__rawr_internal::Value, Error>(__rawr_internal::Value::from({value}))"
        ),
        convert::Type::Script { .. } => {
            let rust = concrete_type(ty).ok_or_else(|| request::drift("a script type's proxy"))?;
            write!(
                output,
                "Ok::<{rust}, Error>(<{rust}>::from_proxy(script_proxy({value})))"
            )
        }
        // The value a table's call takes inside another type, as an
        // `Option` of the opaque type the table's maker gave: at the type
        // its `_w` names.
        convert::Type::Nominal { .. }
            if OPAQUE_WITNESSED.with_borrow(|witnessed| witnessed.as_ref() == Some(ty)) =>
        {
            write!(
                output,
                "{value}.try_cast_result::<NativeHandle>().map_err(|_| Error::TypeMismatch).and_then(|handle| take_handle_like(arguments.arena, handle, _w))"
            )
        }
        // At the type it is, as rustc infers none for a value in an
        // `Option` (#1070); a parameter's is the context's.
        convert::Type::Nominal { .. } | convert::Type::Parameter(_) => match concrete_type(ty) {
            Some(rust) => write!(
                output,
                "{value}.try_cast_result::<NativeHandle>().map_err(|_| Error::TypeMismatch).and_then(|handle| arguments.arena.take::<{rust}>(handle))"
            ),
            None => write!(
                output,
                "{value}.try_cast_result::<NativeHandle>().map_err(|_| Error::TypeMismatch).and_then(|handle| arguments.arena.take(handle))"
            ),
        },
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
                return Err(request::drift("a range of other than floats entered runtime argument decoding"));
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
        // A slice of other scalars a struct's field borrows, as iced's
        // `LineDash { segments: &[3.0, 6.0], .. }`: kept the same way.
        convert::Type::Borrowed { mutable: false, ty }
            if matches!(ty.as_ref(), convert::Type::Sequence { element, length: None, .. }
                if matches!(**element, convert::Type::Builtin(builtin) if builtin.rust_name().is_some())) =>
        {
            let mut converted = String::new();
            render_from_dynamic(ty, value, depth, &mut converted)?;
            write!(output, "({converted}).map(static_slice)")
        }
        // A `&str` a struct's field borrows: pinned as a `&'static str` is.
        convert::Type::Borrowed { mutable: false, ty }
            if **ty == convert::Type::Builtin(convert::Builtin::Str) =>
        {
            return render_from_dynamic(
                &convert::Type::Builtin(convert::Builtin::StaticStr),
                value,
                depth,
                output,
            );
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
            return Err(request::drift("static function item entered runtime argument decoding"));
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
            .map_err(request::unwritable)?;
            for (member, ty) in members.iter().enumerate() {
                let inner = format!("inner_{depth}_{member}");
                let mut converted = String::new();
                render_from_dynamic(ty, &inner, depth + 1, &mut converted)?;
                write!(
                    output,
                    "{{ let {inner} = {items}.next().ok_or(Error::TypeMismatch)?; ({converted})? }}, "
                )
                .map_err(request::unwritable)?;
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
            let array = format!("value.into_array().map_err(|_| Error::TypeMismatch).and_then(|items| items.into_iter().map(|{inner}| {converted}).collect::<Result<Vec<_>, Error>>()){fixed}");
            if matches!(ty, convert::Type::Sequence { owned: true, length: None, .. }) && place::available(ty) {
                let policy = place::policy(ty)?;
                write!(output, "{{ let value = {value}; if let Some(place) = __rawr_internal::place::get(&value) {{ __rawr_internal::place::take::<{policy}>(&place, place.witness()).map_err(|error| Error::Value(error.to_string())) }} else {{ {array} }} }}")
            } else {
                write!(output, "{{ let value = {value}; {array} }}")
            }
        }
    }
    .map_err(request::unwritable)
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
    convert::Type::ConstArray { .. } => {
            return Err(unlengthed())
        }
        convert::Type::Nominal { definition, .. } if let Some((frame, slot)) = frame_slot(definition) => {
            write!(output, "{frame}.give_{slot}(arguments.arena, {value})")
        }
        // Rust's `Box`, as rawr's, which holds its value as itself.
        ty if let Some(element) = boxed_element(ty) => {
            return render_to_dynamic(element, &format!("(*{value})"), depth, output);
        }
        // A crate's thread, as rawr's handle of it: what it gives becomes
        // rawr's where the program joins it (#1266).
        ty if let Some(element) = joined_element(ty) => {
            let mut converted = String::new();
            render_to_dynamic(element, &format!("joined_{depth}"), depth + 1, &mut converted)?;
            write!(
                output,
                "Ok::<Dynamic, Error>(__rawr_internal::crossing::joined({value}, move |joined_{depth}| ({converted}).map_err(runtime_error)))"
            )
        }
        // std's `RangeInclusive` of a crate's type, as iced's
        // `Radians::RANGE`: rawr's range of its two bounds.
        ty if let Some(element @ convert::Type::Nominal { .. }) = range_inclusive_element(ty) => {
            let mut start = String::new();
            render_to_dynamic(element, &format!("start_{depth}"), depth + 1, &mut start)?;
            let mut end = String::new();
            render_to_dynamic(element, &format!("end_{depth}"), depth + 1, &mut end)?;
            write!(
                output,
                "{{ let (start_{depth}, end_{depth}) = {value}.into_inner(); ({start}).and_then(|start| ({end}).map(|end| Dynamic::from(start..=end))) }}"
            )
        }
        // std's `RangeInclusive` of an integer, as a type parameter it
        // filled gives it back: Rawr's `RangeInclusive<i64>`.
        ty if let Some(convert::Type::Builtin(builtin)) = range_inclusive_element(ty)
            && (*builtin == convert::Builtin::I64 || builtin.is_narrow_integer()) =>
        {
            write!(
                output,
                "{{ let (start_{depth}, end_{depth}) = {value}.into_inner(); int_into(start_{depth}).and_then(|start| int_into(end_{depth}).map(|end| Dynamic::from(start..=end))) }}"
            )
        }
        // std's map or set, as rawr's of its kind: its entries sorted by
        // key, as rawr keeps them (`keyed_element`).
        ty if let Some((kind, members)) = keyed_element(ty) => {
            let sort = if kind.starts_with("Hash") {
                if members.len() == 2 {
                    "entries.sort_by(|left, right| left.0.cmp(&right.0)); "
                } else {
                    "entries.sort(); "
                }
            } else {
                ""
            };
            let entry = match members {
                [key] => {
                    let mut converted = String::new();
                    render_to_dynamic(key, &format!("key_{depth}"), depth + 1, &mut converted)?;
                    format!("|key_{depth}| {converted}")
                }
                [key, value] => {
                    let mut key_converted = String::new();
                    render_to_dynamic(key, &format!("key_{depth}"), depth + 1, &mut key_converted)?;
                    let mut value_converted = String::new();
                    render_to_dynamic(
                        value,
                        &format!("value_{depth}"),
                        depth + 1,
                        &mut value_converted,
                    )?;
                    format!(
                        "|(key_{depth}, value_{depth})| Ok::<Dynamic, Error>(Dynamic::from_array(vec![({key_converted})?, ({value_converted})?]))"
                    )
                }
                _ => return Err(request::drift("a keyed collection of other than one or two types")),
            };
            write!(
                output,
                "{{ #[allow(unused_mut)] let mut entries = {value}.into_iter().collect::<Vec<_>>(); {sort}entries.into_iter().map({entry}).collect::<Result<Vec<Dynamic>, Error>>().map(Dynamic::from_array) }}"
            )
        }
        // The crate's own `Rc` or `Arc`, as rawr's pointer whose value is in
        // it: nothing is copied (#775).
        ty if counted_element(ty).is_some() => {
            write!(output, "give_pointer!(arguments.arena, {value})")
        }
        ty if std_value(ty).is_some() => write!(
            output,
            "Ok::<Dynamic, Error>(__rawr_internal::crossing::Std::give({value}))"
        ),
        ty if is_duration(ty) || is_facade_value(ty) => {
            write!(output, "Ok::<Dynamic, Error>(Dynamic::from({value}))")
        }
        ty if is_value(ty) => write!(output, "Ok::<Dynamic, Error>({value}.into_inner())"),
        // A value native code gives a script function has no adapter's
        // arena at hand: it is made where the function uses it.
        convert::Type::Script { .. } if SCRIPT_INPUTS.get() => {
            write!(output, "Ok::<Dynamic, Error>({value}.0.into_dynamic())")
        }
        convert::Type::Script { .. } => write!(
            output,
            "Ok::<Dynamic, Error>(give_proxy(arguments.arena, {value}.0))"
        ),
        // A value that borrows, made in a scope frame: its region holds it
        // (D558 tier 1), when its type has a name: one that leaves an
        // argument to rustc is held as any native value.
        convert::Type::Nominal { .. }
            if IN_REGION.get()
                && carries_lifetime(ty)
                && let Some(family) = family_of(ty).filter(|family| family.named().is_some()) =>
        {
            write!(
                output,
                "Ok::<Dynamic, Error>(Dynamic::from(region.{}))",
                family.call(family::Reach::Insert, &held_value(ty, value))
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
            write!(output, "Ok::<Dynamic, Error>(Dynamic::from(<f64 as From<f32>>::from({value})))")
        }
        // An output Rawr keeps as its text, as a `Cow<str>` (D558 slice 2).
        convert::Type::Builtin(convert::Builtin::Text) => {
            write!(output, "Ok::<Dynamic, Error>(Dynamic::from({value}.to_string()))")
        }
        convert::Type::Inferred => return Err(request::drift("an inferred type parameter reached a runtime value")),
        convert::Type::Unit | convert::Type::Never | convert::Type::Builtin(_) => {
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
                "{{ let range = {value}; Ok::<Dynamic, Error>(Dynamic::from(<f64 as From<f32>>::from(range.start)..<f64 as From<f32>>::from(range.end))) }}"
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
                "{{ let range = {value}; Ok::<Dynamic, Error>(Dynamic::from(<i64 as From<u32>>::from(<u32 as From<char>>::from(range.start))..<i64 as From<u32>>::from(<u32 as From<char>>::from(range.end)))) }}"
            ),
            _ => {
                return Err(request::drift("a range of other than floats entered runtime output encoding"));
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
            .map_err(request::unwritable)?;
            for (member, name) in members.iter().zip(&names) {
                let mut converted = String::new();
                render_to_dynamic(member, name, depth + 1, &mut converted)?;
                write!(output, "({converted})?, ").map_err(request::unwritable)?;
            }
            write!(output, "])) }})()")
        }
        // A `&[T]` of crate values a field borrows, as a page's `children:
        // &'static [Block]` (#1283): each element copied by its type's
        // `Clone` where it has one, and else the read is refused, as rustc
        // refuses to move out of a borrow (E0507).
        convert::Type::Borrowed { mutable: false, ty: lent }
            if let convert::Type::Sequence { element, length: None, .. } = lent.as_ref()
                && matches!(**element, convert::Type::Nominal { .. })
                && holds_native(element)
                && !carries_lifetime(element) =>
        {
            let inner = format!("inner_{depth}");
            let mut converted = String::new();
            render_to_dynamic(element, &inner, depth + 1, &mut converted)?;
            write!(
                output,
                "{value}.iter().map(|{inner}| guard_copy!({inner})).collect::<Result<Vec<_>, Error>>().and_then(|items| items.into_iter().map(|{inner}| {converted}).collect::<Result<Vec<Dynamic>, Error>>()).map(Dynamic::from_array)"
            )
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
            return Err(request::drift("static function item entered runtime output encoding"));
        }
    }
    .map_err(request::unwritable)
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
        ty if let Some(element) = boxed_element(ty) => holds_native(element),
        ty if counted_element(ty).is_some() => true,
        // What a crate's thread gives crosses by value (#1266).
        ty if joined_element(ty).is_some() => false,
        ty if let Some((_, members)) = keyed_element(ty) => members.iter().any(holds_native),
        convert::Type::Nominal { .. } => !is_value(ty) && !is_duration(ty),
        convert::Type::Tuple(members) => members.iter().any(holds_native),
        convert::Type::Option(element) | convert::Type::Sequence { element, .. } => {
            holds_native(element)
        }
        convert::Type::Result(ok, err) => holds_native(ok) || holds_native(err),
        _ => false,
    }
}

/// Whether `ty` holds a script value, which may hold native values a call
/// makes, as `Option<Message>` holds `Message::Split(Axis::Vertical)`.
fn holds_script(ty: &convert::Type) -> bool {
    match ty {
        convert::Type::Script { .. } => true,
        convert::Type::Tuple(members) => members.iter().any(holds_script),
        convert::Type::Option(element) | convert::Type::Sequence { element, .. } => {
            holds_script(element)
        }
        convert::Type::Result(ok, err) => holds_script(ok) || holds_script(err),
        _ => false,
    }
}

/// Rust's `std::time::Duration`, which crosses by value.
/// Whether `ty` is std's `Duration` or `Instant`, which cross by value as
/// Rawr's own.
fn is_duration(ty: &convert::Type) -> bool {
    std_time(ty).is_some() || std_value(ty).is_some()
}

/// The macro that copies a borrowed output of type `target`, or the
/// `Some` it holds: a std value's owned form (`std_copy`), or the target's
/// `Clone` (`guard_copy`).
fn copy_macro(target: &convert::Type) -> &'static str {
    let target = match target {
        convert::Type::Option(element) => element,
        target => target,
    };
    match target {
        convert::Type::Nominal { definition, .. }
            if crate::surface::identity::Definition::std_clone(definition.as_bytes()) =>
        {
            "std_copy"
        }
        _ => "guard_copy",
    }
}

/// The Rust path of one of std's types Rawr's std holds as its Rust value
/// (`classify::std_binding`), when `ty` is one: native code takes, gives and
/// borrows the value itself (`crossing::Std`).
fn std_value(ty: &convert::Type) -> Option<String> {
    match ty {
        convert::Type::Nominal {
            definition,
            arguments,
        } if arguments.is_empty() => {
            crate::surface::identity::Definition::std_path(definition.as_bytes())
                .map(|path| format!("std::{path}"))
        }
        _ => None,
    }
}

/// The Rust path of std's `Duration` or `Instant`, when `ty` is one, or of
/// `Infallible`, which no value crosses as.
fn std_time(ty: &convert::Type) -> Option<&'static str> {
    use crate::surface::identity::Definition;
    match ty {
        convert::Type::Nominal { definition, .. } if *definition == Definition::infallible() => {
            Some("std::convert::Infallible")
        }
        convert::Type::Nominal { definition, .. } if *definition == Definition::duration() => {
            Some("std::time::Duration")
        }
        convert::Type::Nominal { definition, .. } if *definition == Definition::instant() => {
            Some("std::time::Instant")
        }
        _ => None,
    }
}

/// `T` of Rust's `Box<T>`, which crosses as rawr's `Box`: the value it
/// holds, boxed where native code takes it (D711 step 5).
fn boxed_element(ty: &convert::Type) -> Option<&convert::Type> {
    match ty {
        convert::Type::Nominal {
            definition,
            arguments,
        } if *definition == crate::surface::identity::Definition::boxed() => {
            match arguments.as_ref() {
                [element] => Some(element),
                _ => None,
            }
        }
        _ => None,
    }
}

/// `T` of a thread's `JoinHandle<T>` a crate's function gives, which
/// crosses as rawr's thread handle: its `T` becomes rawr's where the
/// program joins it, with no call's arena (#1266).
fn joined_element(ty: &convert::Type) -> Option<&convert::Type> {
    match ty {
        convert::Type::Nominal {
            definition,
            arguments,
        } if *definition == crate::surface::identity::Definition::join_handle() => {
            match arguments.as_ref() {
                [element] => Some(element),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Rust's `Rc<T>` or `Arc<T>` of a crate's `T` (#775), when `ty` is one:
/// its Rust path and `T`. It crosses as rawr's pointer, whose value is a
/// handle on `T`'s slot, which the crate's own pointer is the home of.
fn counted_element(ty: &convert::Type) -> Option<(&'static str, &convert::Type)> {
    match ty {
        convert::Type::Nominal {
            definition,
            arguments,
        } => {
            let arc = crate::surface::identity::Definition::counter(definition.as_bytes())?;
            let [element] = arguments.as_ref() else {
                return None;
            };
            Some((if arc { "std::sync::Arc" } else { "std::rc::Rc" }, element))
        }
        _ => None,
    }
}

/// std's keyed collection (#777), when `ty` is one: its name and its key
/// (and value) types. It crosses as rawr's, an array of `[key, value]`
/// pairs or of keys sorted by key, which the crate's is made from or made
/// into: the two lay their entries out apart, so the keys, rawr's strings
/// or numbers, are made anew, and crate values move.
fn keyed_element(ty: &convert::Type) -> Option<(&'static str, &[convert::Type])> {
    match ty {
        convert::Type::Nominal {
            definition,
            arguments,
        } => Some((
            crate::surface::identity::Definition::keyed_kind(definition.as_bytes())?,
            arguments,
        )),
        _ => None,
    }
}

/// The entry of a keyed collection of `members`, as Rust's iterator gives
/// it, converted from the rawr value `entry` to a `Result` of it.
fn render_entry_from_dynamic(
    members: &[convert::Type],
    entry: &str,
    depth: usize,
) -> Result<String, request::Error> {
    match members {
        [key] => {
            let mut converted = String::new();
            render_from_dynamic(key, entry, depth + 1, &mut converted)?;
            Ok(converted)
        }
        [key, value] => {
            let (key_name, value_name) = (format!("key_{depth}"), format!("value_{depth}"));
            let mut key_converted = String::new();
            render_from_dynamic(key, &key_name, depth + 1, &mut key_converted)?;
            let mut value_converted = String::new();
            render_from_dynamic(value, &value_name, depth + 1, &mut value_converted)?;
            Ok(format!(
                "{{ let mut pair = {entry}.into_array().map_err(|_| Error::TypeMismatch)?.into_iter(); let {key_name} = pair.next().ok_or(Error::TypeMismatch)?; let {value_name} = pair.next().ok_or(Error::TypeMismatch)?; Ok::<_, Error>((({key_converted})?, ({value_converted})?)) }}"
            ))
        }
        _ => Err(request::drift(
            "a keyed collection of other than one or two types",
        )),
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

/// The item `T` of a program's iterator, `rawr::future::Items<T>`, where a
/// crate takes an iterator.
fn items_element(ty: &convert::Type) -> Option<&convert::Type> {
    match ty {
        convert::Type::Nominal {
            definition,
            arguments,
        } if *definition == crate::surface::identity::Definition::iterator() => {
            match arguments.as_ref() {
                [item] => Some(item),
                _ => None,
            }
        }
        _ => None,
    }
}

/// The `T` of an output that is a Rust future of `T`.
fn future_output(ty: &convert::Type) -> Option<&convert::Type> {
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
    (*definition == crate::surface::identity::Definition::future()).then_some(element)
}

/// Project an iced `Task<T>`, or a Rust future of `T`, into the facade's
/// `Task`. Each `T` it yields becomes a Rawr value on the UI thread, never
/// in the stream. `wrap` is `imported` for a call's output, which takes the
/// call's witness, or `native` for a value of a future's type taken where a
/// future is.
fn render_give_task(
    element: &convert::Type,
    value: &str,
    wrap: &str,
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
        "    let task = {{\n        #[allow(unused_imports)]\n        use __rawr_profile_host::task::{{ViaFuture as _, ViaTask as _}};\n        (&__rawr_profile_host::task::Project::new({value})).into_task()\n    }};\n    Ok(__rawr_profile_host::task::{wrap}(task.map(|value| -> __rawr_profile_host::task::Materialize {{\n        Box::new(move |witness| {materialize}(witness, move |arguments| {{ let _ = &arguments; {converted} }}))\n    }})))"
    )
    .map_err(request::unwritable)
}

fn render_give_with(
    signature: &crate::classify::representability::signature::Signature,
    substitutions: &[convert::Type],
    value: &str,
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    let copied = copied_back(signature, substitutions)?;
    if copied.is_empty() {
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
    let given = String::from_utf8(given).map_err(request::unwritable)?;
    writeln!(
        output,
        "    let given = (|| -> Result<Dynamic, Error> {{\n{given}    }})()?;\n    Ok(Dynamic::from_array(vec![given{copied}]))"
    )
    .map_err(request::unwritable)
}

/// What goes back to the program after a call's value, `, ..` of each
/// `&mut` array the call took and changed (D558 slice 2) and each program
/// value it took `&mut` through its proxy (#1008), in order: empty when
/// the call gives its value alone.
fn copied_back(
    signature: &crate::classify::representability::signature::Signature,
    substitutions: &[convert::Type],
) -> Result<String, request::Error> {
    let mut copies = copied_inputs(signature);
    // A program value lent `&mut` where a generic call takes `&mut T` moves
    // into the call through its proxy and goes back after the call's value,
    // with what native code changed in it (#1008).
    for (position, input) in signature.inputs().iter().enumerate() {
        if let convert::Type::Borrowed { mutable: true, ty } =
            substituted(input, signature, substitutions)?
            && matches!(*ty, convert::Type::Script { .. })
            && !copies.iter().any(|(copied, _)| *copied == position)
        {
            copies.push((position, *ty));
        }
    }
    copies.sort_by_key(|(position, _)| *position);
    let mut copied = String::new();
    for (position, element) in &copies {
        if matches!(element, convert::Type::Script { .. }) {
            copied.push_str(&format!(", argument_{position}_proxy.0.into_dynamic()"));
            continue;
        }
        let converted = given_back(*position, element, !LENT_ARGUMENTS.get())?;
        if LENT_ARGUMENTS.get() && place::available(element) {
            copied.push_str(&format!(", match argument_{position}_copy {{ Some(argument_{position}_copy) => ({converted})?, None => argument_{position}_place }}"));
        } else {
            copied.push_str(&format!(", ({converted})?"));
        }
    }
    Ok(copied)
}

/// What goes back to the program of the `&mut String` or `&mut Vec<T>` at
/// `position` a call took and changed: the native value itself, as a place
/// of the operand's exact type, which the next call lends where it is
/// (#716). An element type with no native place goes back as Rawr's value
/// (D558 slice 2).
fn given_back(
    position: usize,
    element: &convert::Type,
    owned: bool,
) -> Result<String, request::Error> {
    if place::available(element) {
        return Ok(if owned {
            format!(
                "restored_place::<{}>(argument_{position}_owner, argument_{position}_copy, &argument_{position}_witness)",
                place::policy(element)?
            )
        } else {
            format!(
                "native_place::<{}>(argument_{position}_copy, &argument_{position}_witness)",
                place::policy(element)?
            )
        });
    }
    let mut converted = String::new();
    render_to_dynamic(
        element,
        &format!("argument_{position}_copy"),
        0,
        &mut converted,
    )?;
    Ok(converted)
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

/// Whether a `&mut` input of `ty` is Rawr's value the call changes and
/// gives back: a copy of an array, as `&mut Vec<u8>`, or of a string, as
/// `&mut String`, or one of std's values Rawr's std holds as itself, as
/// `&mut PathBuf`, which the program moves into the call and gets back
/// (#680).
fn is_copied(ty: &convert::Type) -> bool {
    matches!(
        ty,
        convert::Type::Sequence { .. } | convert::Type::Builtin(convert::Builtin::String)
    ) || std_value(ty).is_some()
}

/// Whether a `&` input of `ty` lends a Rust value the bridge makes from
/// Rawr's array, tuple or string for the call: `&[T]`, `&Vec<T>`, `&(A,
/// B)` or `&String`.
fn is_made_for_call(ty: &convert::Type) -> bool {
    matches!(
        ty,
        convert::Type::Sequence { .. }
            | convert::Type::Tuple(_)
            | convert::Type::Builtin(convert::Builtin::String)
    )
        // A number, as `PartialOrd<f32>::lt` takes `&f32` (T412).
        || matches!(ty, convert::Type::Builtin(builtin)
            if !matches!(
                builtin,
                convert::Builtin::String
                    | convert::Builtin::Str
                    | convert::Builtin::StaticStr
                    | convert::Builtin::Text
                    | convert::Builtin::InclusiveRange
            ))
}

/// `ty` with every type parameter the signature declares replaced by its
/// substitution, at any depth: a future of `Option<T>` is a future of
/// `Option<rawr::Value>` where `T` is erased (D513.2).
fn substituted(
    ty: &convert::Type,
    signature: &crate::classify::representability::signature::Signature,
    substitutions: &[convert::Type],
) -> Result<convert::Type, request::Error> {
    instantiated(ty, signature, substitutions)
        .ok_or_else(|| request::drift("generic substitution index"))
}

/// `ty` substituted as [`substituted`] does, or `None` when the plan does
/// not instantiate one of its parameters, as a generic item's own
/// declaration plan does not.
fn instantiated(
    ty: &convert::Type,
    signature: &crate::classify::representability::signature::Signature,
    substitutions: &[convert::Type],
) -> Option<convert::Type> {
    let all = |types: &[convert::Type]| {
        types
            .iter()
            .map(|ty| instantiated(ty, signature, substitutions))
            .collect::<Option<Box<[_]>>>()
    };
    let one = |ty: &convert::Type| instantiated(ty, signature, substitutions).map(Box::new);
    Some(match ty {
        convert::Type::Parameter(_) => substitution(ty, signature, substitutions)?.clone(),
        convert::Type::Nominal {
            definition,
            arguments,
        } => convert::Type::Nominal {
            definition: *definition,
            arguments: all(arguments)?,
        },
        convert::Type::Option(element) => convert::Type::Option(one(element)?),
        // `&F` where the call fills `F` with `str` is the `&str` input it
        // takes, as a literal fills jiff's `strftime(&self, format: &'f F)`.
        convert::Type::Borrowed { mutable: false, ty }
            if matches!(**ty, convert::Type::Parameter(_))
                && matches!(
                    substitution(ty, signature, substitutions),
                    Some(convert::Type::Builtin(convert::Builtin::Str))
                ) =>
        {
            convert::Type::Builtin(convert::Builtin::Str)
        }
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
        convert::Type::ConstArray { element, parameter } => convert::Type::ConstArray {
            element: one(element)?,
            parameter: *parameter,
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
        convert::Type::Unit | convert::Type::Never | convert::Type::Builtin(_) => ty.clone(),
    })
}

/// A `[T; N]` of a const parameter where the bridge needs a value: an
/// instance's adapter has each at its argument's length (`adapter::at_lengths`).
fn unlengthed() -> request::Error {
    request::drift("an array of a const length reached the bridge without its length")
}

fn substitute<'a>(
    ty: &'a convert::Type,
    signature: &'a crate::classify::representability::signature::Signature,
    substitutions: &'a [convert::Type],
) -> Result<&'a convert::Type, request::Error> {
    substitution(ty, signature, substitutions)
        .ok_or_else(|| request::drift("generic substitution index"))
}

/// The type a parameter `ty` stands for, `ty` itself when it is not a
/// parameter, or `None` when the plan does not instantiate it.
fn substitution<'a>(
    ty: &'a convert::Type,
    signature: &'a crate::classify::representability::signature::Signature,
    substitutions: &'a [convert::Type],
) -> Option<&'a convert::Type> {
    let convert::Type::Parameter(index) = ty else {
        return Some(ty);
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
        return Some(VALUE.get_or_init(|| convert::Type::Nominal {
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
        return Some(&CLOSURE);
    }
    let position = signature
        .parameters()
        .iter()
        .filter(|parameter| {
            parameter.kind() == crate::classify::representability::signature::Kind::Type
        })
        .position(|parameter| parameter.index() == *index)?;
    substitutions.get(position)
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
        return Err(request::drift("unsafe generated const substitution"));
    }
    output
        .write_all(constant.value().as_bytes())
        .map_err(request::unwritable)
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
        .ok_or_else(|| request::drift("generated container parameter cardinality"))?;
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
        return Err(request::drift("generated substitution cardinality"));
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
        write!(output, "::{}", plan.name()).map_err(request::unwritable)
    } else if container_parameters == 0 {
        output
            .write_all(rust_path(plan).as_bytes())
            .map_err(request::unwritable)
    } else {
        Err(request::drift(
            "generated free function has container parameters",
        ))
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
            .map_err(request::unwritable);
    }
    let partition = partition_generics(signature, substitutions, constants, container_parameters)?;
    // An impl that fixes some of its type's arguments, as pane_grid's
    // `Content<'a, Message, Void, W, ..>`, or names its parameters in
    // another order, as iced's `Map<W, F, A>` of `impl<W, A, B, F, ..>`:
    // that instance, its own parameters given their substitutions. A
    // callable one, as `Map`'s `F: Fn(A) -> B`, is the receiver's, which
    // rustc infers, as a turbofish writes it.
    if let Some(instance) = signature.instance() {
        let mut given = partition.container_types.iter();
        let mut types = partition
            .container_parameters
            .iter()
            .filter_map(|parameter| {
                use crate::classify::representability::signature::Kind;
                match parameter.kind() {
                    Kind::Type => given.next().map(|ty| (parameter.index(), ty)),
                    Kind::Callable | Kind::Future => {
                        Some((parameter.index(), &convert::Type::Inferred))
                    }
                    Kind::Constant => None,
                }
            })
            .collect::<Vec<_>>()
            .into_iter();
        let substituted = instance_substituted(instance, &mut types).ok_or_else(|| {
            request::drift("an impl's fixed instance names a parameter it does not declare")
        })?;
        let instance = concrete_type(&substituted)
            .ok_or_else(|| request::drift("an impl's fixed instance has no Rust name"))?;
        let instance = if type_position {
            instance
        } else {
            format!("<{instance}>")
        };
        return output
            .write_all(instance.as_bytes())
            .map_err(request::unwritable);
    }
    let mut arguments = Vec::new();
    render_generic_arguments(
        partition.container_parameters,
        partition.container_types,
        partition.container_constants,
        if type_position { b"<" } else { b"::<" },
        &mut arguments,
    )?;
    output
        .write_all(path.as_bytes())
        .map_err(request::unwritable)?;
    // A type names each lifetime it declares as rustc infers it in a body,
    // `'_` (`concrete_type`); a turbofish leaves them out, as rustc allows.
    let lifetimes =
        SCOPED_PATHS.with_borrow(|paths| paths.get(path).map_or(0, |found| found.count));
    if type_position && lifetimes != 0 {
        let marks = vec!["'_"; lifetimes as usize].join(", ");
        let arguments = String::from_utf8(arguments).map_err(request::unwritable)?;
        let written = match arguments.strip_prefix('<') {
            Some(rest) => format!("<{marks}, {rest}"),
            None => format!("<{marks}>"),
        };
        return output
            .write_all(written.as_bytes())
            .map_err(request::unwritable);
    }
    output.write_all(&arguments).map_err(request::unwritable)
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
        // `impl<T, E> Task<Result<T, E>>`, as iced's `and_then` has.
        convert::Type::Result(ok, err) => convert::Type::Result(
            Box::new(instance_substituted(ok, types)?),
            Box::new(instance_substituted(err, types)?),
        ),
        convert::Type::Tuple(members) => convert::Type::Tuple(
            members
                .iter()
                .map(|member| instance_substituted(member, types))
                .collect::<Option<Box<[_]>>>()?,
        ),
        convert::Type::Sequence {
            element,
            owned,
            length,
        } => convert::Type::Sequence {
            element: Box::new(instance_substituted(element, types)?),
            owned: *owned,
            length: *length,
        },
        convert::Type::Borrowed { mutable, ty } => convert::Type::Borrowed {
            mutable: *mutable,
            ty: Box::new(instance_substituted(ty, types)?),
        },
        other => other.clone(),
    })
}

/// The positions of an opaque callable type's inputs among its arguments,
/// and of its output (`OPAQUE_CALLABLES`).
type CallShape = (Box<[u32]>, u32);

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
    /// Each with the positions of the arguments that are the associated
    /// types its bounds bind.
    static OPAQUE_TYPES: std::cell::RefCell<std::collections::BTreeMap<[u8; 32], Box<[u32]>>> =
        const { std::cell::RefCell::new(std::collections::BTreeMap::new()) };
    /// The opaque types that are futures, as `impl Straw<..>` is through
    /// `Core`, by identity: the position of the argument that is their
    /// `Future::Output`.
    static OPAQUE_FUTURES: std::cell::RefCell<std::collections::BTreeMap<[u8; 32], u32>> =
        const { std::cell::RefCell::new(std::collections::BTreeMap::new()) };
    /// The opaque types that are `impl Fn(A..) -> B`, as iced's
    /// `Function::with` gives, by identity: the positions of the arguments
    /// that are their inputs and the one that is their output. Each is held
    /// boxed (`boxed_callable_type`), a type Rust code can write.
    static OPAQUE_CALLABLES: std::cell::RefCell<std::collections::BTreeMap<[u8; 32], CallShape>> =
        const { std::cell::RefCell::new(std::collections::BTreeMap::new()) };
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
    /// The imported types with type parameters a field's boxed callable
    /// gives, by identity: the positions of those parameters, as `W` of
    /// iced's `Lazy<'a, W, D>` is at 0 (`at_static`).
    static GIVEN_ARGUMENTS: std::cell::RefCell<std::collections::BTreeMap<[u8; 32], Box<[u32]>>> =
        const { std::cell::RefCell::new(std::collections::BTreeMap::new()) };
    /// Which of each named trait's type parameters other than `Self` have
    /// a default, by the trait's identity: a path through the trait leaves
    /// one the call doesn't fix to its default, as `impl Widget<Part>`
    /// does.
    static TRAIT_DEFAULTS: std::cell::RefCell<std::collections::BTreeMap<[u8; 32], Box<[bool]>>> =
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
    /// Whether the values being converted are inputs native code gives a
    /// script function or closure, outside any adapter's `arguments`.
    static SCRIPT_INPUTS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// Whether an adapter's body takes its arguments that carry a lifetime
    /// from the scope frame's region `region` it runs in.
    static REGION_ARGUMENTS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// Whether an adapter's body has the arguments native code lent it as
    /// `lent` (`with_lent_arguments`).
    static LENT_ARGUMENTS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// The `&[T]` arguments an adapter's body has as `slice_{index}`, each a
    /// native slice place's elements or `None` (`with_slice_argument`).
    static SLICE_ARGUMENTS: std::cell::RefCell<Vec<usize>> = const { std::cell::RefCell::new(Vec::new()) };
    /// Whether an adapter takes its `&str` arguments pinned for the
    /// runner's life, as its output borrows them (`lends_str_to_output`).
    static PINNED_STRINGS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// Whether an adapter's output borrows crate values it takes by `&T`,
    /// which it shares with `pins` for as long as the output can borrow
    /// them (`views_natives`).
    static VIEWED_ARGUMENTS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// The closures an adapter's call takes whose output is at a lifetime
    /// of the call's own (`regional`), each made again in the region of a
    /// slot the call is made in (`render_anchored_output`).
    static REGIONAL_CLOSURES: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
    /// Whether an adapter's opaque output borrows the crate values it
    /// takes from the one value that owns them (`lent_cell`): each such
    /// argument is named in the builder of that value.
    static LENT_CELL: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Names an adapter's lent arguments in the builder of the value that owns
/// them (`LENT_CELL`) while it lives, when `cell`.
struct LentCell(bool);

impl LentCell {
    fn enter(cell: bool) -> Self {
        Self(LENT_CELL.replace(cell))
    }
}

impl Drop for LentCell {
    fn drop(&mut self) {
        LENT_CELL.set(self.0);
    }
}

/// How an opaque output that borrows crate values its call takes lives
/// beside them (#846), as `view<'a>(items: &'a [Item]) -> impl Show + 'a`
/// gives: one self-referential value owns what the call lends, and the
/// output borrows it there, held as a proxy of its traits (#849).
struct Owned {
    /// The traits the opaque type is known by, which bound the proxy's
    /// impl, as `events::stock::Stocked<Item = String> + 'a`.
    bounds: String,
    /// The type of each lent argument the value owns, and its name.
    owners: Vec<(String, String)>,
    /// The value's impl of its one trait through the proxy (#856), so a
    /// crate call that takes an `impl Trait` takes the value, and what it
    /// borrows moves with it, as rustc moves the borrow with the result.
    /// `None` when the type has more traits, or a method has no Rust
    /// spelling Rawr writes (`Opaque::forwards`).
    forwards: Option<Forwards>,
}

/// The traits of `returned`, an opaque output of `signature` at
/// `substitutions`, besides auto traits: each by its identity and path, at
/// its arguments, with each associated type the bounds bind, as `Item = T`.
/// `None` when `returned` is no opaque type, or a trait or argument has no
/// Rust spelling.
#[allow(clippy::type_complexity)]
fn opaque_traits<'s>(
    signature: &'s crate::classify::representability::signature::Signature,
    substitutions: &[convert::Type],
    returned: &convert::Type,
    trait_path: fn(&[u8; 32]) -> Option<String>,
) -> Result<
    Option<(
        &'s crate::classify::representability::signature::Opaque,
        Vec<([u8; 32], String, Vec<String>)>,
    )>,
    request::Error,
> {
    let convert::Type::Nominal {
        definition,
        arguments,
    } = returned
    else {
        return Ok(None);
    };
    if !is_opaque(definition) {
        return Ok(None);
    }
    let Some(opaque) = signature
        .opaques()
        .iter()
        .find(|opaque| opaque.definition() == definition.as_bytes())
    else {
        return Ok(None);
    };
    // Each trait, at its arguments.
    let mut traits = Vec::new();
    for bound in opaque.traits() {
        let convert::Type::Nominal {
            definition: id,
            arguments: bound,
        } = bound
        else {
            return Ok(None);
        };
        let Some(path) = trait_path(id.as_bytes()) else {
            return Ok(None);
        };
        let mut written = Vec::new();
        for argument in bound {
            let Some(rust) = concrete_type(&substituted(argument, signature, substitutions)?)
            else {
                return Ok(None);
            };
            written.push(rust);
        }
        traits.push((*id.as_bytes(), path, written));
    }
    // Each associated type the bounds bind, as `Item = T`, on the trait
    // that declares it, or else on the first, which reaches a supertrait's.
    // The marks of a future, an iterator or a callable are no trait's.
    let marks = [
        *crate::surface::identity::Definition::future().as_bytes(),
        *crate::surface::identity::Definition::iterator().as_bytes(),
        *crate::surface::identity::Definition::callable().as_bytes(),
    ];
    for (associated, position) in opaque.associated() {
        if marks.contains(&associated.trait_identity()) {
            continue;
        }
        // One the program can't know is left to rustc.
        let Some(rust) = usize::try_from(*position)
            .ok()
            .and_then(|position| arguments.get(position))
            .and_then(concrete_type)
            .filter(|rust| opaque_holes(rust).is_empty())
        else {
            continue;
        };
        let declares = traits
            .iter()
            .position(|(id, ..)| *id == associated.trait_identity())
            .unwrap_or(0);
        if let Some((_, _, written)) = traits.get_mut(declares) {
            written.push(format!("{} = {rust}", associated.name()));
        }
    }
    Ok(Some((opaque, traits)))
}

/// The Rust path of the trait `identity` its root names it by.
fn root_trait_path(identity: &[u8; 32]) -> Option<String> {
    TYPE_PATHS.with_borrow(|paths| paths.get(identity).cloned())
}

/// The Rust path of the trait `identity`: its root's, or else the one
/// through the crate that defines it, which the runner depends on as a
/// macro's `$crate` reaches it (`hidden_spelling`), as iced's `explain`
/// gives `impl iced_core::Widget<..>`, which no path of iced's names.
fn bound_trait_path(identity: &[u8; 32]) -> Option<String> {
    root_trait_path(identity).or_else(|| {
        PATH_IDENTITIES.with_borrow(|identities| {
            identities
                .iter()
                .filter(|(_, known)| *known == identity)
                .find_map(|(path, _)| hidden_spelling(path))
        })
    })
}

/// Each of `traits`, as `opaque_traits` gives them, as Rust writes it in a
/// bound.
fn written_bounds(traits: &[([u8; 32], String, Vec<String>)]) -> impl Iterator<Item = String> + '_ {
    traits.iter().map(|(_, path, written)| {
        if written.is_empty() {
            path.clone()
        } else {
            format!("{path}<{}>", written.join(", "))
        }
    })
}

/// The bounds of the opaque output of `signature` at `substitutions`, as
/// `Widget<crate::Message, ..>`, when Rust can write them.
fn opaque_bounds(
    signature: &crate::classify::representability::signature::Signature,
    substitutions: &[convert::Type],
) -> Result<Option<String>, request::Error> {
    let returned = substituted(signature.output(), signature, substitutions)?;
    let Some((_, traits)) = opaque_traits(signature, substitutions, &returned, bound_trait_path)?
    else {
        return Ok(None);
    };
    let bounds = written_bounds(&traits).collect::<Vec<_>>();
    Ok((!bounds.is_empty()).then(|| bounds.join(" + ")))
}

/// The value an opaque output that borrows its call's crate values lives
/// in (`Owned`), or `None` when the output isn't one, or its traits or
/// arguments have no Rust spelling.
fn lent_cell(
    signature: &crate::classify::representability::signature::Signature,
    substitutions: &[convert::Type],
) -> Result<Option<Owned>, request::Error> {
    // Only an output the classifier read as borrowing its inputs, whose
    // bounds it can write (`Signature::lends`).
    if signature.lends().is_empty() {
        return Ok(None);
    }
    let returned = substituted(signature.output(), signature, substitutions)?;
    let Some((opaque, traits)) =
        opaque_traits(signature, substitutions, &returned, root_trait_path)?
    else {
        return Ok(None);
    };
    let bounds = written_bounds(&traits)
        .chain(["'a".to_owned()])
        .collect::<Vec<_>>()
        .join(" + ");
    let mut owners = Vec::new();
    for (position, declared) in signature.inputs().iter().enumerate() {
        let input = substituted(declared, signature, substitutions)?;
        let name = format!("argument_{position}");
        let owner = if let Some(element) = lent_slice_element(declared, &input) {
            let Some(item) = concrete_type(element) else {
                return Ok(None);
            };
            format!("SliceLent<{item}>")
        } else if let Some(target) = viewed_native(&input)
            && keeps_borrow(signature, position, declared)
        {
            let Some(rust) = concrete_type(target) else {
                return Ok(None);
            };
            if matches!(input, convert::Type::Option(_)) {
                format!("Option<std::rc::Rc<{rust}>>")
            } else {
                format!("std::rc::Rc<{rust}>")
            }
        } else {
            continue;
        };
        owners.push((owner, name));
    }
    // The one trait, without arguments, the value implements through the
    // proxy (#856).
    let forwards = match (&traits[..], opaque.forwards()) {
        ([(_, path, written)], Some(forwards)) if written.is_empty() => {
            render_forwards(path, forwards)
        }
        _ => None,
    };
    Ok((!owners.is_empty()).then_some(Owned {
        bounds,
        owners,
        forwards,
    }))
}

/// How a value that lives beside what it borrows implements its opaque
/// type's one trait (#856): a method of the proxy for each of the trait's,
/// which calls the method of the value inside, and the impl of the trait
/// for `LentValue`, whose methods call the proxy's.
struct Forwards {
    /// The proxy trait's methods.
    methods: String,
    /// Their bodies in the proxy's impl for each type of the traits.
    bodies: String,
    /// `impl {path} for LentValue`.
    implementation: String,
}

/// The parts of `Forwards` for the trait at `path`, or `None` when a type
/// of one of `forwards` has no Rust spelling.
fn render_forwards(
    path: &str,
    forwards: &[crate::classify::representability::signature::Forward],
) -> Option<Forwards> {
    let mut methods = String::new();
    let mut bodies = String::new();
    let mut implementation = String::new();
    for forward in forwards {
        let name = forward.name();
        let mut parameters = String::new();
        let mut names = Vec::new();
        for (position, input) in forward.inputs().iter().enumerate() {
            parameters.push_str(&format!(", input_{position}: {}", impl_type(input, &[])?));
            names.push(format!("input_{position}"));
        }
        let names = names.join(", ");
        let output = match forward.output() {
            convert::Type::Unit => "()".to_owned(),
            ty => impl_type(ty, &[])?,
        };
        let (receiver, reach) = if forward.mutable() {
            ("&mut self", "mutable")
        } else {
            ("&self", "shared")
        };
        let head = format!("fn __rawr_forward_{name}({receiver}{parameters}) -> {output}");
        let separator = if names.is_empty() { "" } else { ", " };
        methods.push_str(&format!("        {head};\n"));
        bodies.push_str(&format!(
            "        {head} {{\n            {path}::{name}(self{separator}{names})\n        }}\n"
        ));
        implementation.push_str(&format!(
            "        fn {name}({receiver}{parameters}) -> {output} {{\n            self.{reach}(|value| value.__rawr_forward_{name}({names}))\n        }}\n"
        ));
    }
    Some(Forwards {
        methods,
        bodies,
        implementation: format!(
            "    #[allow(clippy::all)]\n    impl {path} for LentValue {{\n{implementation}    }}\n"
        ),
    })
}

/// One method of a proxy (`render_lent_cell`): a call of the program's on
/// a value of an opaque type that lives beside what it borrows, by the
/// call's index in the type's table (`opaque_consumers`).
struct ProxyEntry {
    index: usize,
    mutable: bool,
    /// The Rust type of each argument after the receiver.
    inputs: Vec<String>,
    /// The Rust type of what it gives, or `None` for a value of the opaque
    /// type itself, which is held as the first is.
    output: Option<String>,
    /// The call on `receiver`, of `argument_0`, `argument_1`, ...
    call: String,
}

/// How a call in an opaque type's table reaches a value that lives beside
/// what it borrows (`OPAQUE_CELL`): through its proxy's method `index`,
/// which gives a value of the type itself when `derives`.
#[derive(Clone, Copy)]
struct ProxyReach {
    index: usize,
    derives: bool,
}

/// Whether `plan` is a method that borrows a value of the opaque type
/// `key` as its receiver, which a value that lives beside what it borrows
/// reaches through its proxy.
fn proxied(plan: &Plan, key: &convert::Type) -> Result<bool, request::Error> {
    if !matches!(
        plan.operation(),
        Operation::Method {
            receiver: Receiver::Shared | Receiver::Mutable,
            ..
        }
    ) {
        return Ok(false);
    }
    Ok(opaque_operand(plan)?
        .is_some_and(|(position, _, operand)| position == 0 && same_opaque(&operand, key)))
}

/// The proxy method of the call `plan`, the `index`th in the table of the
/// opaque type `key`: one per instance of a generic method, its associated
/// types as the call fixes them. `None` when it isn't proxied, or when a
/// type it takes or gives has no Rust spelling, or holds the opaque type
/// other than as the whole of what it gives.
fn proxy_entry(
    index: usize,
    plan: &Plan,
    key: &convert::Type,
) -> Result<Option<ProxyEntry>, request::Error> {
    let Operation::Method {
        receiver,
        signature,
        substitutions,
        constants,
        container_parameters,
        ..
    } = plan.operation()
    else {
        return Ok(None);
    };
    if !proxied(plan, key)? || plan.application_invocation().is_some() {
        return Ok(None);
    }
    let written = |ty: &convert::Type| {
        concrete_type(ty).filter(|rust| !holds_opaque(ty) && opaque_holes(rust).is_empty())
    };
    let mut inputs = Vec::new();
    for input in signature.inputs() {
        let Some(rust) = written(&substituted(input, signature, substitutions)?) else {
            return Ok(None);
        };
        inputs.push(rust);
    }
    let returned = substituted(signature.output(), signature, substitutions)?;
    let copy = lent_copy(signature);
    let mutable = matches!(receiver, Receiver::Mutable);
    let output = match &returned {
        ty if same_opaque(ty, key) && !mutable => None,
        ty if same_opaque(ty, key) => return Ok(None),
        convert::Type::Borrowed { ty, .. } if copy == ".to_vec()" => {
            let convert::Type::Sequence { element, .. } = &**ty else {
                return Ok(None);
            };
            let Some(element) = written(element) else {
                return Ok(None);
            };
            Some(format!("Vec<{element}>"))
        }
        convert::Type::Borrowed { .. } if copy == ".to_owned()" => Some("String".to_owned()),
        // What borrows the value can't leave its cell.
        convert::Type::Borrowed { .. } => return Ok(None),
        ty => match written(ty) {
            Some(rust) => Some(rust),
            None => return Ok(None),
        },
    };
    let mut call = Vec::new();
    render_method_head(
        plan.name(),
        *receiver,
        signature,
        substitutions,
        constants,
        *container_parameters,
        &mut call,
    )?;
    argument_list(signature.inputs().len(), 1, &mut call)?;
    call.write_all(b")").map_err(request::unwritable)?;
    call.write_all(copy.as_bytes())
        .map_err(request::unwritable)?;
    Ok(Some(ProxyEntry {
        index,
        mutable,
        inputs,
        output,
        call: String::from_utf8(call).map_err(request::unwritable)?,
    }))
}

/// The proxy methods of the opaque type `key`, one for each call in
/// `cohort` that reaches a value of it through its proxy.
fn proxy_entries(cohort: &[Plan], key: &convert::Type) -> Result<Vec<ProxyEntry>, request::Error> {
    let mut entries = Vec::new();
    for (index, consumer) in opaque_consumers(cohort, key)?.into_iter().enumerate() {
        entries.extend(proxy_entry(index, consumer, key)?);
    }
    Ok(entries)
}

/// The output of `call` in the value that owns what it borrows (`Owned`):
/// the lent arguments move into it, and `call` names each by the builder's
/// borrow of it (`LENT_CELL`).
///
/// The output is held as a `dyn Proxy`: an object-safe trait of one method
/// for each call the program makes on it (`entries`), whatever its own
/// traits allow of a trait object, implemented for each type of its traits
/// by calling that type's method, so rustc checks each call as it is
/// written. A method that gives `Self` gives a value held the same way, in
/// the same cell, since it borrows what the first borrows. Each value is a
/// slot of the cell: when its handle drops, the value in it drops, and when
/// the last of them drops, the arguments drop, which give the values back
/// to their slots.
fn render_lent_cell(
    owned: &Owned,
    entries: &[ProxyEntry],
    call: &str,
    output: &mut Vec<u8>,
) -> std::io::Result<()> {
    let types = owned
        .owners
        .iter()
        .map(|(ty, _)| format!("{ty}, "))
        .collect::<String>();
    let names = owned
        .owners
        .iter()
        .map(|(_, name)| format!("{name}, "))
        .collect::<String>();
    let mut methods = String::new();
    let mut bodies = String::new();
    for entry in entries {
        let index = entry.index;
        let receiver = if entry.mutable { "&mut self" } else { "&self" };
        let inputs = entry
            .inputs
            .iter()
            .enumerate()
            .map(|(position, ty)| format!(", argument_{position}: {ty}"))
            .collect::<String>();
        let (returned, body) = match &entry.output {
            Some(ty) => (ty.as_str(), entry.call.clone()),
            None => ("LentOutput<'a>", format!("Box::new({})", entry.call)),
        };
        let head = format!("fn __rawr_{index}({receiver}{inputs}) -> {returned}");
        methods.push_str(&format!("        {head};\n"));
        bodies.push_str(&format!(
            "        {head} {{\n            let receiver = self;\n            {body}\n        }}\n"
        ));
    }
    let bounds = &owned.bounds;
    // The value's impl of its trait, through the proxy (#856).
    let forwards = match &owned.forwards {
        Some(forwards) => {
            methods.push_str(&forwards.methods);
            bodies.push_str(&forwards.bodies);
            forwards.implementation.as_str()
        }
        None => "",
    };
    writeln!(
        output,
        "    trait Proxy<'a> {{
{methods}    }}
    impl<'a, T: {bounds}> Proxy<'a> for T {{
{bodies}    }}
    type LentOutput<'a> = Box<dyn Proxy<'a> + 'a>;
    type Slot<'a> = std::rc::Rc<std::cell::RefCell<LentOutput<'a>>>;
    struct Slots<'a>(std::cell::RefCell<Vec<Option<Slot<'a>>>>);
    __rawr_internal::self_cell::self_cell!(
        struct Lent {{
            owner: ({types}),
            #[not_covariant]
            dependent: Slots,
        }}
    );
    struct LentValue {{ cell: std::rc::Rc<Lent>, slot: usize }}
    impl LentValue {{
        fn reach<R>(&self, call: impl for<'a> FnOnce(&Slots<'a>, Slot<'a>) -> R) -> R {{
            self.cell.with_dependent(|_, slots| {{
                let value = slots.0.borrow()[self.slot].clone();
                call(slots, value.expect(\"a value's slot lives as long as it\"))
            }})
        }}
        fn shared<R>(&self, call: impl for<'a> FnOnce(&(dyn Proxy<'a> + 'a)) -> R) -> R {{
            self.reach(|_, value| call(&**value.borrow()))
        }}
        fn mutable<R>(&self, call: impl for<'a> FnOnce(&mut (dyn Proxy<'a> + 'a)) -> R) -> R {{
            self.reach(|_, value| call(&mut **value.borrow_mut()))
        }}
        fn derive(&self, call: impl for<'a> FnOnce(&(dyn Proxy<'a> + 'a)) -> LentOutput<'a>) -> Self {{
            let slot = self.reach(|slots, value| {{
                let derived = Some(std::rc::Rc::new(std::cell::RefCell::new(call(&**value.borrow()))));
                let mut slots = slots.0.borrow_mut();
                match slots.iter().position(Option::is_none) {{
                    Some(free) => {{
                        slots[free] = derived;
                        free
                    }}
                    None => {{
                        slots.push(derived);
                        slots.len() - 1
                    }}
                }}
            }});
            Self {{ cell: self.cell.clone(), slot }}
        }}
    }}
    impl Drop for LentValue {{
        fn drop(&mut self) {{
            let slot = self.slot;
            self.cell.with_dependent(|_, slots| {{
                let value = slots.0.borrow_mut().get_mut(slot).and_then(Option::take);
                drop(value);
            }});
        }}
    }}
    fn slots<'a>(value: LentOutput<'a>) -> Slots<'a> {{
        Slots(std::cell::RefCell::new(vec![Some(std::rc::Rc::new(std::cell::RefCell::new(value)))]))
    }}
{forwards}    let output = lend(arguments.arena, || LentValue {{ cell: std::rc::Rc::new(Lent::new(({names}), |({names})| slots(Box::new({call})))), slot: 0 }});"
    )
}

/// Takes an adapter's `&T` arguments of crate values shared with `pins`
/// while it lives, when `viewed`.
struct ViewedArguments(bool);

impl ViewedArguments {
    fn enter(viewed: bool) -> Self {
        Self(VIEWED_ARGUMENTS.replace(viewed))
    }
}

impl Drop for ViewedArguments {
    fn drop(&mut self) {
        VIEWED_ARGUMENTS.set(self.0);
    }
}

/// The crate type of a `&T`, or of an `Option<&T>`, that a call takes: a
/// value the runner's arena holds, which its output may borrow past the
/// call, as `qr_code(&self.data)` gives a `QRCode<'a>` and `pick_list(Some(
/// &self.backend), ..)` a `PickList` whose `V` is `&Backend`.
fn viewed_native(ty: &convert::Type) -> Option<&convert::Type> {
    let ty = match ty {
        convert::Type::Option(inner) => inner.as_ref(),
        ty => ty,
    };
    match ty {
        convert::Type::Borrowed { mutable: false, ty }
            if matches!(**ty, convert::Type::Nominal { .. })
                && !is_value(ty)
                && !is_facade_value(ty)
                && !is_duration(ty)
                && !carries_lifetime(ty) =>
        {
            Some(ty)
        }
        _ => None,
    }
}

/// Whether the call's output carries a lifetime, or is opaque, and an input
/// is a `&T` of a crate value it may keep, or an `Option` of one
/// (`viewed_native`, `keeps_borrow`), or a `&[T]` of them
/// (`lent_slice_element`): the output may borrow the value for as long as
/// it lives, so the adapter shares it with the pins of the view's region,
/// or of the runner out of any region, instead of lending it for the call
/// alone (D568).
fn views_natives(
    signature: &crate::classify::representability::signature::Signature,
    substitutions: &[convert::Type],
) -> Result<bool, request::Error> {
    let returned = substituted(signature.output(), signature, substitutions)?;
    let borrows = match &returned {
        convert::Type::Nominal { definition, .. } => {
            carries_lifetime(&returned) || is_opaque(definition)
        }
        _ => false,
    };
    if !borrows {
        return Ok(false);
    }
    for (position, declared) in signature.inputs().iter().enumerate() {
        let input = substituted(declared, signature, substitutions)?;
        if (viewed_native(&input).is_some() && keeps_borrow(signature, position, declared))
            || lent_slice_element(declared, &input).is_some()
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Whether the output of a call may keep the borrow its input at
/// `position`, declared `declared`, takes: a reference at a lifetime the
/// output names, which the classifier reads as kept (`Signature::lends`),
/// or what a type parameter or an `Option` holds, as `pick_list(Some(
/// &self.backend), ..)` gives a `PickList` whose `V` is `&Backend`. A
/// reference at a lifetime the output doesn't name, as elevato's
/// `view<'a, M: 'a>(&Building) -> Element<'a, M>` takes, is borrowed for
/// the call alone, so a value native code lent a script function fills it
/// as rustc lets the borrow pass on (#1406).
fn keeps_borrow(
    signature: &crate::classify::representability::signature::Signature,
    position: usize,
    declared: &convert::Type,
) -> bool {
    !matches!(declared, convert::Type::Borrowed { .. })
        || u32::try_from(position).is_ok_and(|position| signature.lends().contains(&position))
}

/// The crate type of the elements of a `&[T]` a call takes, as iced's
/// `markdown::view(&items, ..)` does: values the runner's arena holds,
/// which the call borrows in one slice (`SliceLent`). Only a parameter
/// `declared` a reference is one: a type parameter a `&[T]` fills, as
/// `pick<L: Borrow<[T]>>(options: L)` is, takes what fills it.
fn lent_slice_element<'t>(
    declared: &convert::Type,
    ty: &'t convert::Type,
) -> Option<&'t convert::Type> {
    if !matches!(declared, convert::Type::Borrowed { .. }) {
        return None;
    }
    let convert::Type::Borrowed { mutable: false, ty } = ty else {
        return None;
    };
    match &**ty {
        convert::Type::Sequence {
            element,
            length: None,
            ..
        } if lendable(element) => Some(element),
        _ => None,
    }
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

/// Takes an adapter's `&[T]` arguments at `indices` from `slice_{index}`
/// first, while it lives (`with_slice_argument`).
struct SliceArguments(Vec<usize>);

impl SliceArguments {
    fn enter(indices: Vec<usize>) -> Self {
        Self(SLICE_ARGUMENTS.replace(indices))
    }
}

impl Drop for SliceArguments {
    fn drop(&mut self) {
        SLICE_ARGUMENTS.set(std::mem::take(&mut self.0));
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

/// Converts the inputs native code gives a script function or closure,
/// while it lives.
struct ScriptInputs(bool);

impl ScriptInputs {
    fn enter() -> Self {
        Self(SCRIPT_INPUTS.replace(true))
    }
}

impl Drop for ScriptInputs {
    fn drop(&mut self) {
        SCRIPT_INPUTS.set(self.0);
    }
}

/// The lifetimes an imported type declares, when it declares any.
fn scoped_definition(definition: &crate::surface::identity::Definition) -> Option<Lifetimes> {
    SCOPED_TYPES.with_borrow(|types| types.get(definition.as_bytes()).copied())
}

/// `ty` spelled at the scope lifetime `'x`: each imported type that
/// carries lifetimes takes `'x` for each, as a scope family names it,
/// but where a script closure gives it (`at_static`).
fn scoped_type(ty: &convert::Type) -> Option<String> {
    scoped_spelling(ty).map(|text| at_static(&text))
}

/// The shape of `ty`, a type that holds opaque types, as the impl of the
/// trait a region holds it boxed through names it (`family::Shape`): each
/// opaque type in it a parameter of the bounds the call that gives it has.
/// None when one has none the runner knows.
fn scoped_shape(ty: &convert::Type) -> Option<family::Shape> {
    let mut bounds = Vec::new();
    let mut spell = |opaque: &convert::Type| {
        let found = family::bounds(&scoped_type(opaque)?)?;
        bounds.push(found);
        Some(family::Shape::parameter(bounds.len() - 1))
    };
    let spelled = spelled_scoped(ty, &mut Some(&mut spell))?;
    Some(family::Shape::new(at_static(&spelled), bounds))
}

/// `ty` spelled with `'x` for each lifetime, for [`scoped_type`].
fn scoped_spelling(ty: &convert::Type) -> Option<String> {
    spelled_scoped(ty, &mut None)
}

/// A spelling of an opaque type in place of its own (`spelled_scoped`).
type OpaqueSpelling<'s> = &'s mut dyn FnMut(&convert::Type) -> Option<String>;

/// [`scoped_spelling`], with each opaque type `ty` holds spelled by
/// `opaque` when it is given.
fn spelled_scoped(ty: &convert::Type, opaque: &mut Option<OpaqueSpelling<'_>>) -> Option<String> {
    if let convert::Type::Nominal { definition, .. } = ty
        && is_opaque(definition)
        && let Some(spell) = opaque
    {
        return spell(ty);
    }
    match ty {
        // A boxed script closure (T417), whose inputs and output may name
        // scoped types.
        convert::Type::Nominal {
            definition,
            arguments,
        } if *definition == crate::surface::identity::Definition::callable() => {
            let (output, inputs) = arguments.split_last()?;
            let inputs = inputs
                .iter()
                .map(|ty| spelled_scoped(ty, opaque))
                .collect::<Option<Vec<_>>>()?;
            Some(format!(
                "Box<dyn Fn({}) -> {}>",
                inputs.join(", "),
                spelled_scoped(output, opaque)?
            ))
        }
        convert::Type::Nominal {
            definition,
            arguments,
        } if scoped_definition(definition).is_some() => {
            let lifetimes = scoped_definition(definition)?;
            let mut text =
                TYPE_PATHS.with_borrow(|paths| paths.get(definition.as_bytes()).cloned())?;
            let mut parts = vec!["'x".to_owned(); lifetimes.count as usize];
            for argument in arguments.iter() {
                parts.push(spelled_scoped(argument, opaque)?);
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
                .map(|ty| spelled_scoped(ty, opaque))
                .collect::<Option<Vec<_>>>()?;
            text.push('<');
            text.push_str(&arguments.join(", "));
            text.push('>');
            Some(text)
        }
        convert::Type::Option(element) => {
            Some(format!("Option<{}>", spelled_scoped(element, opaque)?))
        }
        convert::Type::Result(ok, err) => Some(format!(
            "Result<{}, {}>",
            spelled_scoped(ok, opaque)?,
            spelled_scoped(err, opaque)?
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
/// The expression that takes `item`, a native value of `ty` a region's
/// call takes, from the arena, where one made outside any region is, at
/// `'static`. The region's lifetime shortens it when the type is covariant
/// in it, as rustc's variance says. An invariant one, as iced's `Text<'a>`
/// is through `Theme::Class<'a>`, can't be: Rust makes each widget of a
/// view at the view's lifetime, as Rawr makes it in the region, and one
/// made before can't join them.
fn region_arena(ty: &convert::Type) -> Result<String, request::Error> {
    let rust = concrete_type(ty).ok_or_else(|| request::drift("a region value's type"))?;
    Ok(if shortens(ty) {
        format!(
            "item.try_cast_result::<NativeHandle>().map_err(|_| Error::TypeMismatch).and_then(|handle| arguments.arena.take::<{rust}>(handle))"
        )
    } else {
        format!(
            "Err(Error::Value({:?}.to_owned()))",
            format!(
                "`{rust}` is invariant in its lifetime, so one made outside the view can't join those the view makes"
            )
        )
    })
}

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

/// [`shortens`] of a method's container, the type at `path` spelled
/// `container`: covariant in its own lifetimes, with no other lifetime or
/// borrow among its arguments.
fn container_shortens(path: &str, container: &str) -> bool {
    let Some(lifetimes) = SCOPED_PATHS.with_borrow(|paths| paths.get(path).copied()) else {
        return true;
    };
    let rest = container.strip_prefix(path).unwrap_or(container);
    lifetimes.covariant
        && rest.matches('\'').count() <= lifetimes.count as usize
        && !rest.contains('&')
}

fn carries_lifetime(ty: &convert::Type) -> bool {
    match ty {
        // A `&str` an opaque type captures is pinned for the runner's life
        // where the value is made (`opaque_pinned_strings`), so it holds no
        // borrow.
        convert::Type::Nominal {
            definition,
            arguments,
        } if is_opaque(definition) => arguments.iter().any(|argument| {
            !matches!(argument, convert::Type::Borrowed { mutable: false, ty }
                if **ty == convert::Type::Builtin(convert::Builtin::Str))
                && carries_lifetime(argument)
        }),
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
/// family is declared in `rawr_families`. A type that leaves an argument
/// to rustc's inference, spelled `_`, has none: an associated type names
/// its type in full, so its value is held as one no region holds.
fn family(text: String, lifetimes: Lifetimes) -> Option<family::Family> {
    if names_inferred(&text) {
        return None;
    }
    Some(family::Family::Named(FAMILIES.with_borrow_mut(
        |families| {
            let next = families.len();
            families
                .entry(text)
                .or_insert_with(|| (format!("RawrFamily{next}"), lifetimes))
                .0
                .clone()
        },
    )))
}

/// The type at `'x` of the family struct `name` declares.
fn family_type(name: &str) -> Option<String> {
    FAMILIES.with_borrow(|families| {
        families
            .iter()
            .find(|(_, (known, _))| known == name)
            .map(|(text, _)| text.clone())
    })
}

/// `value`, a Rust value of `ty`, as the region holds it: boxed, when
/// that is how it holds the opaque type (`family::Ops`).
fn held_value(ty: &convert::Type, value: &str) -> String {
    if scoped_type(ty).is_some_and(|key| family::Ops::held(&key).is_some()) {
        format!("Box::new({value})")
    } else {
        value.to_owned()
    }
}

/// The family struct of `ty`, when the region holds `ty` itself, so a
/// signature can name it as `<F as Family>::Of<'x>`: a wrapper's family
/// names no type, and a boxed one holds another (`family`).
fn written_family(ty: &convert::Type) -> Option<String> {
    let name = family_of(ty)?.named()?.to_owned();
    let boxed = scoped_type(ty).is_some_and(|key| family::Ops::held(&key).is_some());
    (!boxed).then_some(name)
}

/// Whether the Rust type spelled `text` has an argument `_`.
fn names_inferred(text: &str) -> bool {
    let identifier = |character: Option<char>| {
        character.is_some_and(|character| character.is_alphanumeric() || character == '_')
    };
    // Not a lifetime rustc infers, `'_`.
    text.match_indices('_').any(|(at, _)| {
        let before = text[..at].chars().next_back();
        !identifier(before) && before != Some('\'') && !identifier(text[at + 1..].chars().next())
    })
}

/// The family of `ty`, an imported type that carries lifetimes. An opaque
/// type's is what the wrapper of the call that makes it gives, when it has
/// one (`family`). A type that holds an opaque type, as iced's
/// `Container<'a, impl Widget>`, has one only when a region holds it boxed
/// (`family::Ops`): Rust code can't name it, so a value of it made outside
/// any region is held where rustc infers its type, in the arena.
fn family_of(ty: &convert::Type) -> Option<family::Family> {
    let convert::Type::Nominal {
        definition,
        arguments,
    } = ty
    else {
        return None;
    };
    if !carries_lifetime(ty) {
        return None;
    }
    if is_opaque(definition) {
        let key = scoped_type(ty)?;
        return match family::Ops::held(&key) {
            Some(held) => family(held, BOXED_LIFETIMES),
            None => family::of(&key),
        };
    }
    if arguments.iter().any(holds_opaque) {
        let held = family::Ops::held(&scoped_type(ty)?)?;
        return family(held, BOXED_LIFETIMES);
    }
    let lifetimes = TYPE_TRAITS.with_borrow(|types| types.get(definition.as_bytes()).copied())?;
    family(scoped_type(ty)?, lifetimes)
}

/// Whether the type at `path` has lifetime parameters, which a lent value
/// of it borrows for its accessor's call only.
fn carries_lifetimes(path: &str) -> bool {
    SCOPED_PATHS.with_borrow(|paths| paths.get(path).is_some_and(|lifetimes| lifetimes.count > 0))
}

/// The family of a method's container, spelled `container` as
/// `render_container_path` writes it, at `'x`.
fn container_family(path: &str, container: &str) -> Option<family::Family> {
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
    // ..>>`'s, and each lifetime the container's spelling leaves to
    // inference, `'_` (`concrete_type`).
    let rest = inferred_lifetimes(rest, |_| "'x".to_owned());
    let written = rest.contains("'x");
    let (rest, inner) = mark_scoped_arguments(&rest);
    let inner = inner || written;
    if lifetimes.count == 0 && !borrowed && !inner {
        return None;
    }
    // Each borrow in it, `&[u8]`, `&str` or a crate value's `&Backend`, is
    // at `'x`.
    let mut marked = String::with_capacity(rest.len());
    let mut characters = rest.chars().peekable();
    while let Some(character) = characters.next() {
        marked.push(character);
        if character == '&' && characters.peek() != Some(&'\'') {
            marked.push_str("'x ");
        }
    }
    let rest = marked;
    let marks = vec!["'x"; lifetimes.count as usize].join(", ");
    let text = match rest.strip_prefix('<') {
        Some(arguments) if lifetimes.count != 0 && !arguments.starts_with('\'') => {
            format!("{path}<{marks}, {arguments}")
        }
        Some(_) => format!("{path}{rest}"),
        None if rest.is_empty() => format!("{path}<{marks}>"),
        None => return None,
    };
    family(at_static(&text), lifetimes)
}

/// `text`, a type spelled at the scope lifetime `'x`, with each type
/// argument a field's boxed callable gives at `'static`: `Lazy<'x,
/// Column<'static, ..>, u8>`, since a script closure gives its value from
/// the arena, where values are at `'static`, and no region's value can
/// fill it. A widget made in the view's region joins it as rustc infers
/// one lifetime for the whole view; one a closure makes later can't.
/// [`scoped_type`] and [`container_family`] both spell a family through
/// it, so a type has one family however it is reached.
fn at_static(text: &str) -> String {
    // Each spelling of such a type: its own path, and each a method's
    // container reaches it by (`container_family`).
    let given = GIVEN_ARGUMENTS.with_borrow(|given| {
        let named = TYPE_PATHS.with_borrow(|paths| {
            given
                .iter()
                .filter_map(|(identity, positions)| {
                    Some((paths.get(identity)?.clone(), positions.clone()))
                })
                .collect::<Vec<_>>()
        });
        let reached = PATH_IDENTITIES.with_borrow(|identities| {
            identities
                .iter()
                .filter_map(|(path, identity)| Some((path.clone(), given.get(identity)?.clone())))
                .collect::<Vec<_>>()
        });
        let mut spelled = named.into_iter().chain(reached).collect::<Vec<_>>();
        spelled.sort();
        spelled.dedup();
        spelled
            .into_iter()
            .map(|(path, positions)| (format!("{path}<"), positions))
            .collect::<Vec<_>>()
    });
    let mut text = text.to_owned();
    for (open, positions) in given {
        let mut at = 0;
        while let Some(found) = text[at..].find(&open) {
            let start = at + found;
            let end = start + open.len();
            at = end;
            let boundary = text[..start]
                .chars()
                .next_back()
                .is_none_or(|before| matches!(before, '<' | ' ' | ',' | '&' | '('));
            if !boundary {
                continue;
            }
            // Each type argument, lifetimes aside, by its range in `text`.
            let mut arguments = Vec::new();
            let mut depth = 0usize;
            let mut from = end;
            let mut previous = '\0';
            for (offset, character) in text[end..].char_indices() {
                let here = end + offset;
                match character {
                    '<' | '(' | '[' => depth += 1,
                    '>' if previous == '-' => {}
                    ',' if depth == 0 => {
                        arguments.push(from..here);
                        from = here + 1;
                    }
                    '>' | ')' | ']' if depth == 0 => {
                        arguments.push(from..here);
                        break;
                    }
                    '>' | ')' | ']' => depth -= 1,
                    _ => {}
                }
                previous = character;
            }
            let types = arguments
                .into_iter()
                .filter(|range| !text[range.clone()].trim_start().starts_with('\''))
                .collect::<Vec<_>>();
            // From the last, so the ranges before stay where they are.
            for position in positions.iter().rev() {
                let Some(range) = usize::try_from(*position)
                    .ok()
                    .and_then(|position| types.get(position))
                else {
                    continue;
                };
                let argument = text[range.clone()].replace("'x", "'static");
                text.replace_range(range.clone(), &argument);
            }
        }
    }
    text
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
            output.write_all(b"}\n").map_err(request::unwritable)?;
        }
        for module in &modules[open.len()..] {
            writeln!(
                output,
                "#[allow(non_snake_case)]\npub mod {module} {{\n#[allow(unused_imports)]\nuse super::*;"
            )
            .map_err(request::unwritable)?;
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
                    format!(", ::core::marker::PhantomData<fn() -> ({list},)>"),
                    "Self(proxy, ::core::marker::PhantomData)".to_owned(),
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

impl{bounds} ::core::clone::Clone for {name}{generics} {{
    fn clone(&self) -> Self {{
        Self::from_proxy(self.0.clone())
    }}
}}

impl{bounds} ::core::fmt::Debug for {name}{generics} {{
    fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {{
        ::core::fmt::Debug::fmt(&self.0, f)
    }}
}}

impl{bounds} ::core::default::Default for {name}{generics} {{
    fn default() -> Self {{
        Self::from_proxy(__rawr_internal::crossing::Proxy::default_of({path:?}))
    }}
}}

impl{bounds} ::core::cmp::PartialEq for {name}{generics} {{
    fn eq(&self, other: &Self) -> ::core::primitive::bool {{
        self.0 == other.0
    }}
}}

impl{bounds} ::core::fmt::Display for {name}{generics} {{
    fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {{
        self.0.display(f)
    }}
}}
"
            )
            .map_err(request::unwritable)?;
        }
    }
    for _ in open {
        output.write_all(b"}\n").map_err(request::unwritable)?;
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
            convert::Type::Borrowed { mutable: false, ty }
                if matches!(
                    &**ty,
                    convert::Type::Sequence {
                        owned: false,
                        length: None,
                        ..
                    }
                ) =>
            {
                let convert::Type::Sequence { element, .. } = &**ty else {
                    return None;
                };
                format!("&[{}]", concrete_type(element)?)
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
        | convert::Type::ConstArray { element: inner, .. }
        | convert::Type::ProfileMessage { payload: inner, .. } => names_parameter(inner),
        convert::Type::Result(ok, err) => names_parameter(ok) || names_parameter(err),
        convert::Type::FunctionItem { input, output } => {
            names_parameter(input) || names_parameter(output)
        }
        convert::Type::Unit
        | convert::Type::Never
        | convert::Type::Builtin(_)
        | convert::Type::Inferred => false,
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
fn render_impls(
    plans: &[Plan],
    profile: &crate::profile::Proof,
    facade: Option<&crate::surface::facade::Plan>,
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    let impls = plans
        .iter()
        .flat_map(Plan::impls)
        .collect::<std::collections::BTreeSet<_>>();
    let cohort = Cohort {
        plans,
        profile,
        facade,
    };
    for script in impls {
        render_impl(script, &cohort, output)?;
    }
    Ok(())
}

/// What a script impl's generic function compiles its frame's calls from
/// (`render_frame`): the runner's plans and what renders them.
struct Cohort<'a> {
    plans: &'a [Plan],
    profile: &'a crate::profile::Proof,
    facade: Option<&'a crate::surface::facade::Plan>,
}

fn render_impl(
    script: &crate::surface::instance::ScriptImpl,
    cohort: &Cohort<'_>,
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    let drift = request::drift;
    let generics = script.generics();
    let owner =
        impl_type(script.owner(), generics).ok_or_else(|| drift("a script impl's owner"))?;
    let arguments = script
        .arguments()
        .iter()
        .map(|argument| impl_type(argument, generics))
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| drift("a script impl's trait arguments"))?;
    // Each lifetime argument is the impl's own, as `'_` writes it, or by
    // its name where a generic function's bound names it (#593).
    let named = frame::named_lifetimes(script);
    let arguments = (0..script.lifetimes() as usize)
        .map(|index| {
            named
                .and_then(|names| names.get(index))
                .map_or_else(|| "'_".to_owned(), ToString::to_string)
        })
        .chain(arguments)
        .collect::<Vec<_>>();
    let trait_ = if arguments.is_empty() {
        script.trait_path().to_owned()
    } else {
        format!("{}<{}>", script.trait_path(), arguments.join(", "))
    };
    let all = named
        .unwrap_or_default()
        .iter()
        .chain(generics)
        .map(AsRef::as_ref)
        .collect::<Vec<_>>();
    let parameters = if all.is_empty() {
        String::new()
    } else {
        format!("<{}>", all.join(", "))
    };
    let mut body = String::new();
    for (name, ty) in script.associated() {
        let ty = impl_type(ty, generics).ok_or_else(|| drift("a script impl's associated type"))?;
        body.push_str(&format!("    type {name} = {ty};\n"));
    }
    let mut items = String::new();
    for function in script.functions() {
        let frame = if function.generics().is_empty() {
            None
        } else {
            let frame = frame::Frame::of(script, function, cohort)?;
            items.push_str(&frame.render(script, cohort)?);
            Some(frame)
        };
        body.push_str(&render_impl_function(function, generics, frame.as_ref())?);
    }
    writeln!(
        output,
        "/// The script's `impl {trait_} for {owner}` (D594).
#[allow(unused_mut, unused_variables, refining_impl_trait, clippy::all)]
impl{parameters} {trait_} for {owner} {{
{body}}}
{items}"
    )
    .map_err(request::unwritable)
}

/// One function of a script impl, as `render_impls` writes it.
fn render_impl_function(
    function: &crate::surface::instance::ScriptImplFunction,
    generics: &[Box<str>],
    frame: Option<&frame::Frame>,
) -> Result<String, request::Error> {
    use crate::surface::instance::ImplReceiver;
    let drift = request::drift;
    let mut parameters = Vec::new();
    let mut values = Vec::new();
    let mut mutable = Vec::new();
    let mut restores = String::new();
    let mut lends = Vec::new();
    let mut places = Vec::new();
    let mut slices = Vec::new();
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
    // A value of an opaque type the frame keeps is kept there, at any
    // depth of an input, and script code holds it by a handle into the
    // frame (`frame::Frame`).
    let _in_frame = frame.map(|frame| InFrame::enter(frame.slots(), "frame"));
    let formatter = function
        .formatter()
        .and_then(|position| usize::try_from(position).ok());
    for (position, input) in function.inputs().iter().enumerate() {
        let name = format!("input_{position}");
        let at = values.len();
        // The trait method's `&mut fmt::Formatter`: the script's body
        // writes into Rawr's string and gives back its text, as a `fmt`
        // body does, which goes into the formatter.
        if formatter == Some(position) {
            parameters.push(format!("{name}: &mut ::core::fmt::Formatter<'_>"));
            values.push("Dynamic::from(String::new())".to_owned());
            continue;
        }
        let rust = match frame {
            Some(frame) => frame.spell(input, generics),
            None => impl_type(input, generics),
        }
        .ok_or_else(|| drift("a script impl function's input"))?;
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
            // A `&[T]` native code lends: the slice itself, as a place the
            // script function reads for the call.
            convert::Type::Borrowed { mutable: false, ty }
                if matches!(
                    **ty,
                    convert::Type::Sequence {
                        owned: false,
                        length: None,
                        ..
                    }
                ) =>
            {
                let convert::Type::Sequence { element, .. } = &**ty else {
                    return Err(drift("a script impl function's slice input"));
                };
                values.push(format!("place_{position}.clone()"));
                slices.push((position, place::policy(element)?));
            }
            convert::Type::Borrowed { mutable, ty }
                if matches!(
                    **ty,
                    convert::Type::Sequence {
                        owned: true,
                        length: None,
                        ..
                    }
                ) =>
            {
                let policy = place::policy(ty)?;
                values.push(format!("place_{position}.clone()"));
                places.push((position, *mutable, policy));
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
                let target =
                    concrete_type(ty).ok_or_else(|| drift("a script impl function's input"))?;
                let lend = LentInput::new(position, *borrowed_mutably, target);
                values.push(lend.value());
                lends.push(lend);
            }
            // One whose type carries lifetimes, as iced's `update(..,
            // shell: &mut Shell<'_, Message>)` takes: lent for the call too,
            // at every lifetime its family takes, and never copied (#957).
            convert::Type::Borrowed {
                mutable: borrowed_mutably,
                ty,
            } if matches!(**ty, convert::Type::Nominal { .. })
                && !is_value(ty)
                && let Some(family) = scoped_type(ty) =>
            {
                let target =
                    concrete_type(ty).ok_or_else(|| drift("a script impl function's input"))?;
                let lend = LentInput::scoped(position, *borrowed_mutably, target, family);
                values.push(lend.value());
                lends.push(lend);
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
    let rust = match frame {
        Some(frame) => frame.spell(returned, generics),
        None => impl_type(returned, generics),
    }
    .ok_or_else(|| drift("a script impl function's output"))?;
    let in_arena = |body: &str| {
        format!(
            "with_arena(|arena| {{ let mut none: [&mut Dynamic; 0] = []; let arguments = &mut Arguments {{ values: &mut none, arena }}; let _ = &arguments; {body} }})"
        )
    };
    // A method that writes into a formatter gives `fmt::Result`: its body
    // gives back the text it wrote, which the formatter takes.
    let written = formatter.map(|position| {
        format!("Ok::<::core::fmt::Result, Error>(input_{position}.write_str(&value.to_string()))")
    });
    let rust = if written.is_some() {
        "::core::fmt::Result".to_owned()
    } else {
        rust
    };
    let output = |converted: &mut String| match &written {
        Some(written) => {
            converted.push_str(written);
            Ok(())
        }
        None => render_from_dynamic(returned, "value", 0, converted),
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
    // The member is found once per generation, with its witnesses, and
    // each call enters that generation (`crossing::Member`).
    let entered = format!(
        "static MEMBER: __rawr_internal::crossing::Member = __rawr_internal::crossing::Member::new({:?});
        let member = MEMBER.enter();",
        function.member()
    );
    let mutable = mutable.join(", ");
    // An output that borrows, as a `view(&self) -> impl Widget<..> + 'a`'s
    // widgets do: made in a region, as a slot's is, anchored on the strings
    // pinned for the runner's life, since the trait may ask the widgets to
    // outlive any lifetime of the impl's, and taken out of it at
    // `'static`.
    // The member's signature names the output's type, which a wrapper's
    // family can't (`family`).
    let family = carries_lifetime(returned)
        .then(|| written_family(returned))
        .flatten();
    // Native code may call the impl on a thread with no arena, as tokio's
    // workers run reqwest's `json()` and its `Deserialize` (D740.3): the
    // values it gives and takes move through an arena of the call's own
    // (`CallArena`).
    if let Some(frame) = frame {
        if family.is_some() {
            return Err(drift("a generic script impl function whose output borrows"));
        }
        let mut converted = String::new();
        {
            let _frame = InFrame::enter(frame.slots(), "frame");
            output(&mut converted)?;
        }
        let converted = in_arena(&converted);
        let call = format!(
            "let mut frame = {made};
        let witnesses = member.witnesses();
        let mut values: [Dynamic; {count}] = {values};
        let fresh = std::mem::take(&mut frame.fresh);
        let (value, nonce) = {{
            let mut erased: &mut (dyn Frame + '_) = &mut frame;
            __rawr_internal::scope::lend::<FrameF, _>(&mut erased, |nonce| {{
                let nonce = nonce.bits();
                frame_lent(&fresh, nonce);
                let _current = FrameCurrent::enter(nonce, {keys});
                let value = member.call(&mut values, &[{mutable}]);{restores}
                (value, nonce)
            }})
        }};
        frame.nonce = nonce;
        {converted}.unwrap_or_else(|error| panic!(\"rawr: a script function gave back a value native code can't take: {{error:?}}\"))",
            made = frame.made(),
            keys = frame.keys(),
        );
        let (items, call) = lent_around(&lends, call);
        let call = place::around(&places, "member.witnesses()", call);
        let call = place::around_slices(&slices, "member.witnesses()", call);
        return Ok(format!(
            "    fn {}{}({}) -> {rust} {{
{items}        let _arena = CallArena::enter();
        {entered}
        {call}
    }}
",
            function.name(),
            frame.function_header(),
            parameters.join(", ")
        ));
    }
    let (lifetime, rust, mut call) = match family {
        Some(family) => {
            (
                "",
                format!("<{family} as __rawr_internal::scope::Family>::Of<'static>"),
                format!(
                    "let witnesses = member.witnesses();
        let mut values: [Dynamic; {count}] = {values};
        let mut region = __rawr_internal::scope::Region::anchored(static_pins());
        let value = __rawr_internal::scope::open_anchored(&mut region, || member.call(&mut values, &[{mutable}]));{restores}
        let scoped = value
            .read_lock::<__rawr_internal::scope::Scoped>()
            .map(|scoped| *scoped)
            .unwrap_or_else(|| panic!(\"rawr: a script function gave native code a value that doesn't borrow what it was lent\"));
        let output = region
            .take::<{family}>(scoped)
            .unwrap_or_else(|error| panic!(\"rawr: {{error}}\"));
        static SITE: __rawr_internal::scope::Site = __rawr_internal::scope::Site::new();
        region.keep(&SITE);
        output"
                ),
            )
        }
        None => {
            let mut converted = String::new();
            output(&mut converted)?;
            let converted = in_arena(&converted);
            // A `&str` comes back pinned for the runner's life, so it
            // meets a trait that asks for `&'static str`, as a derive's
            // `fn name(&self) -> &'static str` does, and one that asks for
            // less.
            let rust = if matches!(
                returned,
                convert::Type::Borrowed { mutable: false, ty }
                    if matches!(**ty, convert::Type::Builtin(convert::Builtin::Str))
            ) {
                "&'static str".to_owned()
            } else {
                rust
            };
            (
                "",
                rust,
                format!(
                    "let witnesses = member.witnesses();
        let mut values: [Dynamic; {count}] = {values};
        let value = member.call(&mut values, &[{mutable}]);{restores}
        {converted}.unwrap_or_else(|error| panic!(\"rawr: a script function gave back a value native code can't take: {{error:?}}\"))"
                ),
            )
        }
    };
    // Each borrowed native value is lent around the call, innermost last.
    let (items, wrapped) = lent_around(&lends, call);
    call = wrapped;
    call = place::around(&places, "member.witnesses()", call);
    call = place::around_slices(&slices, "member.witnesses()", call);
    Ok(format!(
        "    fn {}{lifetime}({}) -> {rust} {{
{items}        let _arena = CallArena::enter();
        {entered}
        {call}
    }}
",
        function.name(),
        parameters.join(", ")
    ))
}

/// A borrowed native value native code gives a script function, lent around
/// its call (`lent_around`): the input's position, whether it is `&mut`,
/// its Rust type, that type at the scope family's `'x`, and whether script
/// code reads it through its type's traits (`scope::Reads`).
struct LentInput {
    position: usize,
    mutable: bool,
    rust: String,
    family: String,
    reads: bool,
}

impl LentInput {
    /// A value of a type without lifetimes, which its family is at every
    /// `'x`: `'static`, so its `Display`, `Debug` and `PartialEq` reach it
    /// as `dyn Any`, as the arena's values are reached.
    fn new(position: usize, mutable: bool, rust: String) -> Self {
        Self {
            position,
            mutable,
            family: rust.clone(),
            rust,
            reads: true,
        }
    }

    /// A value of a type with lifetimes, as iced's `Shell<'_, Message>`:
    /// `rust` elides them, so the accessor reaches it at every lifetime,
    /// and `family` names each `'x`.
    fn scoped(position: usize, mutable: bool, rust: String, family: String) -> Self {
        Self {
            position,
            mutable,
            rust,
            family,
            reads: false,
        }
    }

    /// The script value of the lent value, inside its lend.
    fn value(&self) -> String {
        let Self {
            position,
            mutable,
            rust,
            reads,
            ..
        } = self;
        let writer = if *mutable {
            format!("Some(&REACH_MUT_{position} as __rawr_internal::scope::Accessor)")
        } else {
            "None".to_owned()
        };
        let reads = if *reads {
            format!(".with_reads(&READS_{position})")
        } else {
            String::new()
        };
        format!(
            "Dynamic::from(__rawr_internal::scope::Lent::new(lent_{position}, {rust:?}, {writer}, Some(&REACH_{position} as __rawr_internal::scope::Accessor)){reads})"
        )
    }
}

/// The call `call` with each borrowed native value of `lends` lent around
/// it, innermost last, and the items that reach each one (D558.3): what a
/// script impl function and a slot write where native code lends them a
/// value.
fn lent_around(lends: &[LentInput], call: String) -> (String, String) {
    let mut call = call;
    let mut items = String::new();
    for LentInput {
        position,
        mutable: borrowed_mutably,
        rust: target,
        family,
        reads,
    } in lends.iter().rev()
    {
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
            type Of<'x> = {family};
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
        if *reads {
            items.push_str(&format!(
                "        static READS_{position}: __rawr_internal::scope::Reads = lent_reads!(Lend{position}, {target});
"
            ));
        }
    }
    (items, call)
}

/// Whether a `&mut` of `ty` that native code gives a script closure or slot
/// is the native value itself, lent for the call (`lent_around`), as
/// `Frame::with_save` lends its frame and iced's `with_translation` its
/// renderer.
fn lent_mutably(ty: &convert::Type) -> bool {
    matches!(ty, convert::Type::Nominal { .. })
        && !is_value(ty)
        && !is_duration(ty)
        && !carries_lifetime(ty)
}

/// The scope families the adapters named, as the `rawr_families` module.
fn render_families(output: &mut Vec<u8>) -> Result<(), request::Error> {
    output
        .write_all(
            b"#![allow(dead_code)]
use super::*;

",
        )
        .map_err(request::unwritable)?;
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
        // Its `Display` text, where its type has one.
        let display = "\n    fn display(value: &Self::Of<'_>) -> Option<String> {\n        (&ShownProbe(value)).shown_text()\n    }";
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
        // A value of an enclosing frame's region joins a call in an inner
        // one where the type is covariant in its own lifetimes and nothing
        // else among its arguments borrows (#1329).
        let shorten = if lifetimes.covariant
            && text.matches('\'').count() <= lifetimes.count as usize
            && !text.contains('&')
        {
            "\n    fn shorten<'s, 'l: 's>(value: &'s Self::Of<'l>) -> Option<&'s Self::Of<'s>> {\n        Some(value)\n    }"
        } else {
            ""
        };
        let shown = family::Ops::shown(&text)
            .unwrap_or_else(|| text.clone())
            .replace("'x", "'_");
        writeln!(
            output,
            "pub(super) struct {name};\n\nimpl __rawr_internal::scope::Family for {name} {{\n    type Of<'x> = {text};\n}}\n\nimpl __rawr_internal::scope::Regional for {name} {{\n    fn name() -> &'static str {{\n        {shown:?}\n    }}{debug}{display}{clone}{shorten}{equal}\n}}\n"
        )
        .map_err(request::unwritable)?;
    }
    let mut wrappers = String::new();
    family::render(&mut wrappers);
    output
        .write_all(wrappers.as_bytes())
        .map_err(request::unwritable)
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
    /// An imported type that renders is an ordinary scoped type (D594).
    fn install(facade: Option<&crate::surface::facade::Plan>) -> Self {
        place::clear();
        TRAIT_DEFAULTS.with_borrow_mut(std::collections::BTreeMap::clear);
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
                // A type the root reaches through no path, as iced's
                // `core::widget::Map`, is named through the crate that
                // defines it, which the runner depends on as a macro's
                // `$crate` reaches it (`HiddenCrate`, T476).
                for (owner, ty) in through {
                    let path = format!("{}::{}", owner.root(), ty.path());
                    let facade = *crate::surface::identity::Definition::facade(&path).as_bytes();
                    let spelled = hidden_spelling(&path).unwrap_or_else(|| path.clone());
                    values.insert(facade);
                    paths.entry(facade).or_insert_with(|| spelled.clone());
                    paths.entry(*ty.identity()).or_insert(spelled);
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
                opaque.extend(owner.types().filter(|ty| ty.opaque()).map(|ty| {
                    let associated = ty.associated().map(|(_, _, position)| position).collect();
                    (*ty.identity(), associated)
                }));
            }
        });
        OPAQUE_CALLABLES.with_borrow_mut(|callables| {
            callables.clear();
            let callable = *crate::surface::identity::Definition::callable().as_bytes();
            for owner in facade
                .into_iter()
                .flat_map(crate::surface::facade::Plan::facades)
            {
                callables.extend(owner.types().filter(|ty| ty.opaque()).filter_map(|ty| {
                    let marked = |name: &str| {
                        ty.associated()
                            .find(|(identity, known, _)| *identity == callable && *known == name)
                            .map(|(_, _, position)| position)
                    };
                    let output = marked("Output")?;
                    let inputs = (0..)
                        .map_while(|index| marked(&format!("Input{index}")))
                        .collect();
                    Some((*ty.identity(), (inputs, output)))
                }));
            }
        });
        OPAQUE_FUTURES.with_borrow_mut(|futures| {
            futures.clear();
            let future = *crate::surface::identity::Definition::future().as_bytes();
            for owner in facade
                .into_iter()
                .flat_map(crate::surface::facade::Plan::facades)
            {
                futures.extend(owner.types().filter(|ty| ty.opaque()).filter_map(|ty| {
                    ty.associated()
                        .find(|(identity, _, _)| *identity == future)
                        .map(|(_, _, position)| (*ty.identity(), position))
                }));
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
                            // A view type is an ordinary scoped type (D594).
                            if ty.lifetimes() != 0 {
                                types.entry(*ty.identity()).or_insert(lifetimes);
                            }
                            traits.entry(*ty.identity()).or_insert(lifetimes);
                            // A type named through its crate is found at
                            // that spelling too (`hidden_spelling`).
                            let path = format!("{}::{}", owner.root(), ty.path());
                            let spelled = hidden_spelling(&path);
                            if !ty.given().is_empty() {
                                GIVEN_ARGUMENTS.with_borrow_mut(|given| {
                                    given
                                        .entry(*ty.identity())
                                        .or_insert_with(|| ty.given().into());
                                });
                            }
                            for path in [Some(path), spelled].into_iter().flatten() {
                                paths.entry(path.clone()).or_insert(lifetimes);
                                PATH_IDENTITIES.with_borrow_mut(|identities| {
                                    identities.entry(path).or_insert(*ty.identity());
                                });
                            }
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
                TRAIT_DEFAULTS.with_borrow_mut(|defaults| {
                    defaults
                        .entry(*header.identity())
                        .or_insert_with(|| header.defaults().iter().map(Option::is_some).collect());
                });
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
        family::clear();
        PROXIES.with_borrow_mut(std::collections::BTreeMap::clear);
        PROXY_CRATE.with_borrow_mut(|named| *named = None);
        Self
    }
}

impl Drop for TypePaths {
    fn drop(&mut self) {
        place::clear();
        TYPE_PATHS.with_borrow_mut(std::collections::BTreeMap::clear);
        TRAIT_DEFAULTS.with_borrow_mut(std::collections::BTreeMap::clear);
        COPY_TYPES.with_borrow_mut(std::collections::BTreeSet::clear);
        OPAQUE_TYPES.with_borrow_mut(std::collections::BTreeMap::clear);
        OPAQUE_FUTURES.with_borrow_mut(std::collections::BTreeMap::clear);
        OPAQUE_CALLABLES.with_borrow_mut(std::collections::BTreeMap::clear);
        SCOPED_TYPES.with_borrow_mut(std::collections::BTreeMap::clear);
        SCOPED_PATHS.with_borrow_mut(std::collections::BTreeMap::clear);
        TYPE_TRAITS.with_borrow_mut(std::collections::BTreeMap::clear);
        GIVEN_ARGUMENTS.with_borrow_mut(std::collections::BTreeMap::clear);
        PATH_IDENTITIES.with_borrow_mut(std::collections::BTreeMap::clear);
        ITEM_PATHS.with_borrow_mut(std::collections::BTreeMap::clear);
        FAMILIES.with_borrow_mut(std::collections::BTreeMap::clear);
        family::clear();
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
        convert::Type::Unit
        | convert::Type::Never
        | convert::Type::Builtin(_)
        | convert::Type::Inferred => false,
        convert::Type::Nominal { arguments, .. }
        | convert::Type::Tuple(arguments)
        | convert::Type::Script { arguments, .. } => arguments
            .iter()
            .any(|argument| output_names(argument, index)),
        convert::Type::Option(element)
        | convert::Type::Borrowed { ty: element, .. }
        | convert::Type::Range(element)
        | convert::Type::Sequence { element, .. } => output_names(element, index),
        convert::Type::ConstArray { element, .. } => output_names(element, index),
        convert::Type::Result(ok, err) => output_names(ok, index) || output_names(err, index),
        convert::Type::FunctionItem { input, output } => {
            output_names(input, index) || output_names(output, index)
        }
        convert::Type::ProfileMessage { payload, .. } => output_names(payload, index),
    }
}

/// Whether `definition` is an opaque `impl Trait` type a function gives.
/// The substitutions with each `&str` an opaque output captures pinned as
/// a `&'static str`, as a string literal is in Rust: `download(url: impl
/// AsRef<str>) -> impl Steps` captures `&'static str` from a literal, and
/// no region can hold a type Rust code can't name. `None` when there is
/// none.
fn opaque_pinned_strings(
    signature: &crate::classify::representability::signature::Signature,
    substitutions: &[convert::Type],
) -> Option<Box<[convert::Type]>> {
    fn captures(ty: &convert::Type, index: u32) -> bool {
        match ty {
            convert::Type::Nominal {
                definition,
                arguments,
            } => {
                (is_opaque(definition)
                    && arguments
                        .iter()
                        .any(|argument| output_names(argument, index)))
                    || arguments.iter().any(|argument| captures(argument, index))
            }
            convert::Type::Option(inner)
            | convert::Type::Range(inner)
            | convert::Type::Borrowed { ty: inner, .. }
            | convert::Type::Sequence { element: inner, .. } => captures(inner, index),
            convert::Type::Result(ok, err) => captures(ok, index) || captures(err, index),
            convert::Type::Tuple(members) => members.iter().any(|member| captures(member, index)),
            _ => false,
        }
    }
    let borrowed_str = convert::Type::Borrowed {
        mutable: false,
        ty: Box::new(convert::Type::Builtin(convert::Builtin::Str)),
    };
    let mut pinned = None::<Box<[convert::Type]>>;
    for (position, parameter) in signature
        .parameters()
        .iter()
        .filter(|parameter| {
            parameter.kind() == crate::classify::representability::signature::Kind::Type
        })
        .enumerate()
    {
        if substitutions.get(position) == Some(&borrowed_str)
            && captures(signature.output(), parameter.index())
        {
            pinned.get_or_insert_with(|| substitutions.into())[position] =
                convert::Type::Builtin(convert::Builtin::StaticStr);
        }
    }
    pinned
}

fn is_opaque(definition: &crate::surface::identity::Definition) -> bool {
    OPAQUE_TYPES.with_borrow(|opaque| opaque.contains_key(definition.as_bytes()))
        && !OPAQUE_CALLABLES.with_borrow(|callables| callables.contains_key(definition.as_bytes()))
}

/// The Rust type a value of the opaque `impl Fn(A..) -> B` type `ty` is
/// held as: `Box<dyn Fn(A..) -> B + Send>`, which a call boxes it as where
/// it gives one, and which a call that takes a callable takes as itself.
/// `None` when `ty` is no such type, or its inputs or output have no name.
fn boxed_callable_type(ty: &convert::Type) -> Option<String> {
    let convert::Type::Nominal {
        definition,
        arguments,
    } = ty
    else {
        return None;
    };
    let (inputs, output) =
        OPAQUE_CALLABLES.with_borrow(|callables| callables.get(definition.as_bytes()).cloned())?;
    let argument = |position: u32| concrete_type(arguments.get(usize::try_from(position).ok()?)?);
    let inputs = inputs
        .iter()
        .map(|position| argument(*position))
        .collect::<Option<Vec<_>>>()?;
    Some(callable_box(&inputs, &argument(output)?))
}

/// `Box<dyn Fn(A..) -> B + Send>` of `inputs` and `output`, spelled.
fn callable_box(inputs: &[String], output: &str) -> String {
    format!("Box<dyn Fn({}) -> {output} + Send>", inputs.join(", "))
}

/// The boxed callable a call's callable parameter `call` takes where a
/// crate's own callable value fills it ([`boxed_callable_type`]).
fn boxed_call(
    call: &crate::classify::representability::callable::Call,
    signature: &crate::classify::representability::signature::Signature,
    substitutions: &[convert::Type],
) -> Result<String, request::Error> {
    let spelled = |ty: &convert::Type| {
        concrete_type(&substituted(ty, signature, substitutions)?)
            .ok_or_else(|| request::drift("a callable value's input or output"))
    };
    let inputs = call
        .inputs()
        .iter()
        .map(spelled)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(callable_box(&inputs, &spelled(call.output())?))
}

/// Whether `this` and `other` are one opaque type: the same definition at
/// the same arguments, other than the associated types its bounds bind.
/// Those follow from the rest, as rustc fixes them, and the program may
/// hold one as a value of its own, as a `Result<u32, String>` it reads
/// from `Future::Output`.
fn same_opaque(this: &convert::Type, other: &convert::Type) -> bool {
    match (this, other) {
        (
            convert::Type::Nominal {
                definition,
                arguments,
            },
            convert::Type::Nominal {
                definition: other_definition,
                arguments: other_arguments,
            },
        ) if definition == other_definition && arguments.len() == other_arguments.len() => {
            OPAQUE_TYPES.with_borrow(|opaque| {
                let associated = opaque
                    .get(definition.as_bytes())
                    .map_or(&[][..], |associated| &associated[..]);
                arguments
                    .iter()
                    .zip(other_arguments.iter())
                    .enumerate()
                    .all(|(position, (this, other))| {
                        this == other
                            || u32::try_from(position)
                                .is_ok_and(|position| associated.contains(&position))
                    })
            })
        }
        _ => this == other,
    }
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
    if let Some(boxed) = boxed_callable_type(ty) {
        return Some(boxed);
    }
    Some(match ty {
        convert::Type::Inferred => "_".to_owned(),
        convert::Type::Unit | convert::Type::Never => "()".to_owned(),
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
        ty if let Some(element) = boxed_element(ty) => format!("Box<{}>", concrete_type(element)?),
        ty if let Some((pointer, element)) = counted_element(ty) => {
            format!("{pointer}<{}>", concrete_type(element)?)
        }
        ty if let Some((kind, members)) = keyed_element(ty) => format!(
            "std::collections::{kind}<{}>",
            members
                .iter()
                .map(concrete_type)
                .collect::<Option<Vec<_>>>()?
                .join(", ")
        ),
        // An opaque `impl Trait` type, which rustc infers.
        convert::Type::Nominal { definition, .. } if is_opaque(definition) => "_".to_owned(),
        // A script closure where a signature names its type, boxed (T417).
        convert::Type::Nominal {
            definition,
            arguments,
        } if *definition == crate::surface::identity::Definition::callable() => {
            let (output, inputs) = arguments.split_last()?;
            let inputs = inputs
                .iter()
                .map(concrete_type)
                .collect::<Option<Vec<_>>>()?;
            format!(
                "Box<dyn Fn({}) -> {}>",
                inputs.join(", "),
                concrete_type(output)?
            )
        }
        // A program's iterator where a crate takes one.
        ty if let Some(item) = items_element(ty) => {
            format!("__rawr_internal::future::Items<{}>", concrete_type(item)?)
        }
        ty if let Some(rust) = std_value(ty) => rust,
        ty if let Some(time) = std_time(ty) => time.to_owned(),
        convert::Type::Nominal {
            definition,
            arguments,
        } => {
            let mut text =
                TYPE_PATHS.with_borrow(|paths| paths.get(definition.as_bytes()).cloned())?;
            // Each lifetime the type declares, as rustc names one it
            // infers in a body: `'_`. An item names them (`named_lifetimes`).
            let lifetimes = scoped_definition(definition).map_or(0, |lifetimes| lifetimes.count);
            let parts = std::iter::repeat_n(Some("'_".to_owned()), lifetimes as usize)
                .chain(arguments.iter().map(concrete_type))
                .collect::<Option<Vec<_>>>()?;
            if !parts.is_empty() {
                text.push('<');
                text.push_str(&parts.join(", "));
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
            // An opaque type in an argument has a name of the runner's.
            let arguments = arguments
                .iter()
                .map(|argument| {
                    if holds_opaque(argument) {
                        Some(family::named(scoped_type(argument)?))
                    } else {
                        concrete_type(argument)
                    }
                })
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
        convert::Type::ConstArray { .. } => return None,
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
        // A closure's `&S`, as pane_grid's `view(pane, &state, ..)` gives,
        // or `&State` of the program's own, as `.theme(|state| ..)` takes.
        convert::Type::Nominal { .. }
        | convert::Type::Builtin(_)
        | convert::Type::Script { .. } => concrete_type(ty),
        _ => None,
    }
}

/// The head of a method call on the adapter's `receiver`, through the
/// `(` before its arguments: `receiver.name::<..>(`. A method of a trait
/// impl whose parameters its type doesn't name, as `Renderer` of `impl<..,
/// Renderer> Widget<Message, Theme, Renderer> for Button<..>`, is called
/// through the trait at the arguments the call fixes, and its defaults for
/// the rest, as the `impl Widget<Part>` a value is reads them (T428):
/// `<_ as Widget<Part>>::map::<..>(receiver, `.
#[allow(clippy::too_many_arguments)]
fn render_method_head(
    name: &str,
    receiver: Receiver,
    signature: &crate::classify::representability::signature::Signature,
    substitutions: &[convert::Type],
    constants: &[convert::Constant],
    container_parameters: usize,
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    let encoding = request::unwritable;
    let Some(through) = through_trait(signature, substitutions)? else {
        write!(output, "receiver.{name}").map_err(encoding)?;
        render_item_turbofish(
            signature,
            substitutions,
            constants,
            container_parameters,
            output,
        )?;
        return output.write_all(b"(").map_err(encoding);
    };
    write!(output, "<_ as {through}>::{name}").map_err(encoding)?;
    render_item_turbofish(
        signature,
        substitutions,
        constants,
        container_parameters,
        output,
    )?;
    let lent = match receiver {
        Receiver::Owned => "receiver",
        Receiver::Shared => "&*receiver",
        Receiver::Mutable => "&mut *receiver",
    };
    let separator = if signature.inputs().is_empty() {
        ""
    } else {
        ", "
    };
    write!(output, "({lent}{separator}").map_err(encoding)
}

/// The trait a method is called through, at the arguments its call fixes
/// (`signature::Signature::through`): one the call leaves open is `_`, and
/// trailing ones the trait defaults are left to their defaults. `None` for
/// a method called on its receiver, or a trait or argument with no Rust
/// spelling.
fn through_trait(
    signature: &crate::classify::representability::signature::Signature,
    substitutions: &[convert::Type],
) -> Result<Option<String>, request::Error> {
    let Some(convert::Type::Nominal {
        definition,
        arguments,
    }) = signature.through()
    else {
        return Ok(None);
    };
    let Some(path) = TYPE_PATHS.with_borrow(|paths| paths.get(definition.as_bytes()).cloned())
    else {
        return Ok(None);
    };
    let defaults = TRAIT_DEFAULTS
        .with_borrow(|defaults| defaults.get(definition.as_bytes()).cloned())
        .unwrap_or_default();
    let mut written = Vec::new();
    for argument in arguments {
        let ty = substituted(argument, signature, substitutions)?;
        written.push(match ty {
            convert::Type::Inferred => None,
            ty => match concrete_type(&ty) {
                Some(rust) => Some(rust),
                None => return Ok(None),
            },
        });
    }
    while written.last().is_some_and(Option::is_none)
        && defaults.get(written.len() - 1).copied().unwrap_or(false)
    {
        written.pop();
    }
    if written.is_empty() {
        return Ok(Some(path));
    }
    let written = written
        .into_iter()
        .map(|argument| argument.unwrap_or_else(|| "_".to_owned()))
        .collect::<Vec<_>>();
    Ok(Some(format!("{path}<{}>", written.join(", "))))
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
            return Err(request::drift("generated substitution cardinality"));
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
            .map_err(request::unwritable)?;
        written += 1;
        match parameter.kind() {
            crate::classify::representability::signature::Kind::Type => {
                let ty = substitutions
                    .get(type_index)
                    .ok_or_else(|| request::drift("generated type substitution cardinality"))?;
                type_index += 1;
                let concrete = match ty {
                    convert::Type::Nominal { .. }
                    | convert::Type::Script { .. }
                    | convert::Type::Option(_)
                    | convert::Type::Range(_)
                    | convert::Type::Borrowed { .. }
                    | convert::Type::Result(..) => concrete_type(ty),
                    // In a type, which a scope family's declaration can't
                    // leave to inference: the `Vec` a crate's `&[T]`
                    // constant crosses as, as `pick_list(.., Backend::ALL,
                    // ..)`'s `L` is. So is a `Vec` of script values' proxies,
                    // which nothing else infers, as `next_value::<Vec<T>>`'s.
                    convert::Type::Sequence {
                        length: None,
                        element,
                        ..
                    } if opening == b"<" || matches!(**element, convert::Type::Script { .. }) => {
                        concrete_type(ty)
                    }
                    _ => None,
                };
                output
                    .write_all(match ty {
                        convert::Type::Inferred => b"_",
                        convert::Type::Unit | convert::Type::Never => b"()" as &[u8],
                        // Rust infers the range's integer type from the
                        // call's bounds, as it does for `1..=6`.
                        convert::Type::Builtin(convert::Builtin::InclusiveRange) => b"_",
                        convert::Type::Builtin(convert::Builtin::String) => b"String",
                        // `str` itself, as `F` of jiff's `strftime(&self,
                        // format: &'f F)` is where a `&str` fills `&F`.
                        convert::Type::Builtin(convert::Builtin::Str) => b"str",
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
                        | convert::Type::ConstArray { .. }
                        | convert::Type::Tuple(_) => concrete.as_deref().unwrap_or("_").as_bytes(),
                        // `&[u8]` or `&str`, as `Reader<&'a [u8]>` takes
                        // (D558 tier 1).
                        convert::Type::Borrowed { .. } if concrete.is_some() => {
                            concrete.as_deref().unwrap_or("_").as_bytes()
                        }
                        convert::Type::FunctionItem { .. } | convert::Type::Borrowed { .. } => {
                            return Err(request::drift(
                                "static function item or borrow used as generic substitution",
                            ));
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
                                return Err(request::drift(
                                    "non-scalar profile message payload substitution",
                                ));
                            }
                        },
                        convert::Type::Parameter(_) => {
                            return Err(request::drift("unconstrained generated substitution"));
                        }
                    })
                    .map_err(request::unwritable)?;
            }
            crate::classify::representability::signature::Kind::Constant => {
                let constant = constants
                    .get(constant_index)
                    .ok_or_else(|| request::drift("generated const substitution cardinality"))?;
                constant_index += 1;
                render_constant(constant, output)?;
            }
            crate::classify::representability::signature::Kind::Callable
            | crate::classify::representability::signature::Kind::Future => {
                output.write_all(b"_").map_err(request::unwritable)?;
            }
        }
    }
    if type_index != substitutions.len() || constant_index != constants.len() {
        return Err(request::drift("generated substitution cardinality"));
    }
    if written == 0 {
        return Ok(());
    }
    output.write_all(b">").map_err(request::unwritable)
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
        .map_err(request::unwritable)?;
    for module in modules.clone() {
        writeln!(output, "        {module}::PRODUCERS,").map_err(request::unwritable)?;
    }
    output
        .write_all(b"    ];\n\n    const ACCEPTS: &[&[([u8; 32], bool)]] = &[\n")
        .map_err(request::unwritable)?;
    for module in modules {
        writeln!(output, "        {module}::ACCEPTS,").map_err(request::unwritable)?;
    }
    output
        .write_all(
            b"    ];\n\n\
              fn assert_accepts(identity: [u8; 32], consumes: bool) {\n\
                  for producer in PRODUCERS.iter().flat_map(|producers| producers.iter()) {\n\
                      let mut arena = Arena::new([11; 32]);\n\
                      let handle = producer(&mut arena).expect(\"nominal producer\");\n\
                      let slot = handle.slot as usize;\n\
                      // The only copy of the handle: a consuming call moves the\n\
                      // value out of its slot, where another copy would clone it.\n\
                      match invoke_one(identity, handle, &mut arena) {\n\
                          Ok(_) => {\n\
                              if consumes {\n\
                                  assert!(arena.slots[slot].value.is_none());\n\
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
        .map_err(request::unwritable)
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
        .map_err(request::unwritable)?;
        render_execution_values(&inputs.iter().collect::<Vec<_>>(), output)?;
        writeln!(
            output,
            "    let mut arguments = Arguments {{ values: &mut values, arena }};\n    let output = {}(&mut arguments)?;\n    if output.is::<NativeHandle>() {{ Ok(output.clone_cast::<NativeHandle>()) }} else {{ Err(Error::TypeMismatch) }}\n}}\n",
            adapter_name(plan)
        )
        .map_err(request::unwritable)?;
    }
    output
        .write_all(
            b"#[cfg(test)]\npub(super) const PRODUCERS: &[fn(&mut Arena) -> Result<NativeHandle, Error>] = &[\n",
        )
        .map_err(request::unwritable)?;
    for index in indices {
        let plan = &plans[*index];
        if producer_inputs(plan.operation()).is_some() {
            writeln!(output, "    {},", identity_name("producer", plan))
                .map_err(request::unwritable)?;
        }
    }
    // A struct the tests can build from primitives alone. One whose
    // constructor takes another crate value, as `FieldSurface { leaf }`
    // does, has no producer, so its fields have no value to read.
    let producible = plans
        .iter()
        .filter(|plan| {
            matches!(plan.operation(), Operation::StructConstructor { .. })
                && producer_inputs(plan.operation()).is_some()
        })
        .map(structural_type)
        .collect::<std::collections::BTreeSet<_>>();
    output
        .write_all(b"];\n\n#[cfg(test)]\npub(super) const ACCEPTS: &[([u8; 32], bool)] = &[\n")
        .map_err(request::unwritable)?;
    for index in indices {
        let plan = &plans[*index];
        let consumes = match plan.operation() {
            Operation::StructField { field, .. } if producible.contains(&structural_type(plan)) => {
                !shared_field(field.ty())
            }
            Operation::StructField { .. } => continue,
            Operation::Method {
                receiver,
                signature,
                ..
            } if signature.inputs().is_empty() => matches!(receiver, Receiver::Owned),
            _ => continue,
        };
        output.write_all(b"    (").map_err(request::unwritable)?;
        byte_array(output, plan.identity().as_bytes())?;
        writeln!(output, ", {consumes}),").map_err(request::unwritable)?;
    }
    output.write_all(b"];\n").map_err(request::unwritable)?;
    if let Some(index) = primitive.filter(|index| indices.contains(index)) {
        writeln!(
            output,
            "\n#[cfg(test)]\n#[test]\nfn primitive_output_executes_without_an_arena_slot() {{\n    let mut arena = Arena::new([12; 32]);\n    let mut values: [&mut Dynamic; 0] = [];\n    let mut arguments = Arguments {{ values: &mut values, arena: &mut arena }};\n    let output = {}(&mut arguments).expect(\"primitive adapter\");\n    assert!(output.is::<i64>());\n    assert!(arena.slots.is_empty());\n}}",
            adapter_name(&plans[index])
        )
        .map_err(request::unwritable)?;
    }
    if let Some(index) = builder.filter(|index| indices.contains(index)) {
        let inputs = builder_execution_inputs(plans[index].operation())
            .ok_or_else(|| request::drift("generated builder execution inputs"))?;
        output
            .write_all(
                b"\n#[cfg(test)]\n#[test]\nfn consuming_builder_replays_in_one_native_invocation() {\n    let mut arena = Arena::new([13; 32]);\n",
            )
            .map_err(request::unwritable)?;
        render_execution_values(&inputs, output)?;
        writeln!(
            output,
            "    let mut arguments = Arguments {{ values: &mut values, arena: &mut arena }};\n    let output = {}(&mut arguments).expect(\"builder replay\");\n    assert!(output.is::<i64>());\n    assert!(arena.slots.is_empty());\n}}",
            adapter_name(&plans[index])
        )
        .map_err(request::unwritable)?;
    }
    Ok(())
}

fn render_execution_values(
    inputs: &[&convert::Type],
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    for (argument, ty) in inputs.iter().enumerate() {
        write!(output, "    let mut argument_{argument} = ").map_err(request::unwritable)?;
        render_default(ty, output)?;
        output.write_all(b";\n").map_err(request::unwritable)?;
    }
    write!(
        output,
        "    let mut values: [&mut Dynamic; {}] = [",
        inputs.len()
    )
    .map_err(request::unwritable)?;
    for argument in 0..inputs.len() {
        if argument != 0 {
            output.write_all(b", ").map_err(request::unwritable)?;
        }
        write!(output, "&mut argument_{argument}").map_err(request::unwritable)?;
    }
    output.write_all(b"];\n").map_err(request::unwritable)
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
            return Err(request::drift(
                "an inferred type parameter reached a runtime value",
            ))
        }
        convert::Type::Unit | convert::Type::Never => "Dynamic::from(())",
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
        | convert::Type::ConstArray { .. }
        | convert::Type::Tuple(_)
        | convert::Type::FunctionItem { .. }
        | convert::Type::Borrowed { .. }
        | convert::Type::Parameter(_) => {
            return Err(request::drift("generated execution producer input"));
        }
        convert::Type::ProfileMessage { payload, .. } => {
            return render_default(payload, output);
        }
    };
    output
        .write_all(source.as_bytes())
        .map_err(request::unwritable)
}

fn argument_list(count: usize, _offset: usize, output: &mut Vec<u8>) -> Result<(), request::Error> {
    for index in 0..count {
        if index != 0 {
            output.write_all(b", ").map_err(request::unwritable)?;
        }
        write!(output, "argument_{index}").map_err(request::unwritable)?;
    }
    Ok(())
}

fn byte_array(output: &mut Vec<u8>, bytes: &[u8]) -> Result<(), request::Error> {
    output.write_all(b"[").map_err(request::unwritable)?;
    for (index, byte) in bytes.iter().enumerate() {
        if index != 0 {
            output.write_all(b", ").map_err(request::unwritable)?;
        }
        write!(output, "0x{byte:02x}").map_err(request::unwritable)?;
    }
    output.write_all(b"]").map_err(request::unwritable)
}

fn signature(plan: &Plan) -> Result<[u8; 32], request::Error> {
    let mut writer = Hashing(Sha256::new());
    serde_json::to_writer(&mut writer, plan).map_err(request::unwritable)?;
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

    /// rawr writes the runner's harness and Cargo writes its lock, so a lock
    /// rawr can't read back is a rawr bug: it panics naming the invariant,
    /// never an "internal error" message (#538, D664).
    #[test]
    #[should_panic(expected = "cannot read back the runner's Cargo.toml or Cargo.lock: \
                    expected one final-form harness lock root, found 0")]
    fn a_runner_lock_rawr_cannot_read_back_is_a_rawr_bug() {
        let _ = super::runner_lock(
            b"version = 4\n\n[[package]]\nname = \"other\"\nversion = \"0.1.0\"\n",
            &[],
        );
    }

    /// Writing the runner's source in memory can't fail; if it did, that
    /// is a rawr bug, which panics (#538, D664).
    #[test]
    #[should_panic(expected = "cannot write the runner's generated source in memory")]
    fn a_runner_source_rawr_cannot_write_is_a_rawr_bug() {
        let _ = super::request::unwritable(std::fmt::Error);
    }

    /// A type the root reaches through no path, as iced's
    /// `core::widget::Map`, is spelled from the crate that defines it, and
    /// the runner depends on each such crate its sources name (T476).
    #[test]
    fn a_type_no_root_path_reaches_is_named_through_its_crate() {
        let segment = format!(
            "{}iced_core_18998c4171898b8e",
            crate::api::ledger::CRATE_SEGMENT
        );
        assert_eq!(
            super::hidden_spelling(&format!("iced::{segment}::widget::Map")),
            Some(format!("::{segment}::widget::Map"))
        );
        assert_eq!(super::hidden_spelling("iced::widget::Button"), None);
        assert_eq!(super::hidden_spelling(&format!("iced::{segment}")), None);
        let source = format!(
            "type Of<'x> = ::{segment}::widget::Map<iced::widget::Button<'x>, F, A>;\n\
             let m: ::{segment}::widget::Map<_, _, _> = x;"
        );
        assert_eq!(
            super::hidden_segments(source.as_bytes())
                .into_iter()
                .collect::<Vec<_>>(),
            vec![segment]
        );
    }

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
        super::render_give_task(&arguments[0], "output", "imported", &mut task).unwrap();
        let task = String::from_utf8(task).unwrap();
        assert!(task.contains("materialize_checked(witness"), "{task}");
        assert!(
            task.contains("Some(inner_0) => (Ok::<Dynamic, Error>(inner_0.into_inner()))"),
            "{task}"
        );
        // A native value of an erased instance is attested by its own
        // witness, not checked as a script value.
        let sender = Type::Nominal {
            definition: Definition::future(),
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
        // A closure that gives a native value Rust can't name, as a
        // sipper or a future, never reaches the generator: the classifier
        // refuses it (`FutureClosure`, #545), so here it is a rawr bug. One
        // it can name goes through the arena (D551.6), as the erasure
        // fixture's `WindowConfig::new(app)` runs end to end.
        let native = Call::new(
            0,
            Box::default(),
            Type::Nominal {
                definition: Definition::future(),
                arguments: Box::default(),
            },
        );
        let unnamed = std::panic::catch_unwind(|| {
            super::render_take_callable(0, 0, &native, &signature, &[Type::Unit], &mut Vec::new())
                .map_err(|_| ())
        })
        .expect_err("a closure output Rust can't name panics");
        assert_eq!(
            unnamed.downcast_ref::<String>().map(String::as_str),
            Some("the generated Rust disagrees with the crates on closure output type")
        );
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

    /// iced's changelog: `boxed` of the `Map<impl Widget, ..>` that `map`
    /// gave takes it at the type that holds the table's opaque type, and a
    /// `_` inside a name isn't one.
    #[test]
    fn an_operand_holding_the_opaque_type_is_taken_at_its_own_type() {
        assert_eq!(
            super::opaque_holes("::__rawr_crate_parts::widget::Map<_, Box<dyn Fn(u8) -> m_x>, _a>"),
            [34]
        );
        let body = "    let argument_0: Map<_, Box<dyn Fn(String) -> crate::Message>, String> = take_like(arguments, 0, _w)?;\n    let receiver: _ = take_like(arguments, 0, _w)?;\n";
        let shaped = super::shape_witnesses(body, 0);
        assert!(
            shaped.contains("= take_like(arguments, 0, { fn shape<T>(_: std::marker::PhantomData<T>) -> std::marker::PhantomData<Map<T, Box<dyn Fn(String) -> crate::Message>, String>> { std::marker::PhantomData } shape(_w) })?;"),
            "{shaped}"
        );
        assert!(
            shaped.ends_with("    let receiver: _ = take_like(arguments, 0, _w)?;\n"),
            "{shaped}"
        );
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
            runtime.contains(
                "Ok(ok) => inner(0).map_or(Ok(()), |witness| attest_within(ok, &witness, arena))"
            ),
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

    /// A path crate a macro's `$crate` names, which the project reaches only
    /// as the target of its `[patch]`, is no member of the runner's
    /// workspace: as a member, Cargo's lockfile would resolve it with every
    /// feature, which the project never resolved (?play's shell has `debug`,
    /// which pulls `bincode`). One the root reaches through path packages
    /// is a member either way.
    #[test]
    fn a_hidden_path_crate_reached_only_through_a_patch_stays_out_of_the_workspace() {
        let harness = b"[workspace]\n\n[package]\nname = \"rawr-resolution-harness\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\n[lib]\npath = \"src/lib.rs\"\n\n[patch.\"https://example.com/ui\"]\nui_shell = { path = \"/app/ui_shell\" }\n\n[dependencies]\nui = { git = \"https://example.com/ui\" }\nlocal = { path = \"/app/local\" }\n";
        let lock = b"version = 4\n\n[[package]]\nname = \"rawr-resolution-harness\"\nversion = \"0.0.0\"\ndependencies = [\n \"local\",\n \"ui\",\n]\n\n[[package]]\nname = \"local\"\nversion = \"0.1.0\"\ndependencies = [\n \"local_macros\",\n]\n\n[[package]]\nname = \"local_macros\"\nversion = \"0.1.0\"\n\n[[package]]\nname = \"ui\"\nversion = \"0.1.0\"\nsource = \"git+https://example.com/ui#0123\"\ndependencies = [\n \"ui_shell\",\n]\n\n[[package]]\nname = \"ui_shell\"\nversion = \"0.1.0\"\n";
        let hidden = |package: &str| super::HiddenCrate {
            segment: format!("__rawr_crate_{package}_0123456789abcdef"),
            package: package.to_owned(),
            version: "0.1.0".to_owned(),
            source: format!("path+file:///app/{package}"),
        };
        let (manifest, _) = super::runner_manifest_aliased(
            harness,
            None,
            &[hidden("ui_shell"), hidden("local_macros")],
            lock,
        )
        .unwrap();
        let document = std::str::from_utf8(&manifest)
            .unwrap()
            .parse::<toml_edit::DocumentMut>()
            .unwrap();
        let exclude = document["workspace"]["exclude"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|entry| entry.as_str())
            .collect::<Vec<_>>();
        assert_eq!(exclude, ["/app/ui_shell"]);
        assert_eq!(
            document["dependencies"]["__rawr_crate_ui_shell_0123456789abcdef"]["path"].as_str(),
            Some("/app/ui_shell")
        );
    }

    #[test]
    fn generated_compiler_retains_one_native_account_before_initial_compilation() {
        let runtime = std::str::from_utf8(RUNTIME_SOURCE).expect("runtime source is UTF-8");
        let (prepare, compile) = runtime
            .split_once("fn prepare_engine(")
            .expect("engine preparation")
            .1
            .split_once("fn compile_live(")
            .expect("live compiler")
            .0
            .split_once("fn compile_program(")
            .expect("initial compiler");
        let in_order = |mut remaining: &str, operations: &[&str]| {
            for operation in operations {
                remaining = remaining
                    .split_once(operation)
                    .unwrap_or_else(|| {
                        panic!("missing or reordered generated operation: {operation}")
                    })
                    .1;
            }
        };
        in_order(prepare, &[
            "install_compiler_bindings(&mut engine, &mut native)?",
            "admit_generated_structural_retained(\n        &mut engine,\n        &bindings,\n        STRUCTURAL_ROWS,\n        &mut native,",
            "engine.retain_imported_native(native)",
        ]);
        assert!(!prepare.contains("Shared::new(engine)"));
        assert!(!prepare.contains("admit_generated_structural("));
        in_order(
            compile,
            &[
                "if !COMPILE_SNAPSHOT { return Ok(None); }",
                "account::Native::decode(GENERATED_MANIFEST)",
                "prepare_engine(native, __rawr_internal::Access::all(), &mut *progress)?",
                "__rawr_profile::compile_initial(",
                "Ok(Some(Program { engine,",
            ],
        );
        assert_eq!(compile.matches("Native::decode(").count(), 1);
        assert!(compile.contains("Compilation::Replacement(current) => current.as_ref()"));
        assert!(compile.contains(".engine.reconstruct_imported_native(GENERATED_MANIFEST)"));
        assert!(!compile.contains("Shared::new(engine)"));
        assert!(!compile.contains("admit_generated_structural("));
        // The plain runner compiles initially once per process, under its
        // parent's protocol. `?play`'s entry in a universe runner prepares
        // the engine once and compiles each snippet in a fork against it.
        for (source, initial) in [(RUNNER_MAIN, 1), (PROFILE_RUNNER_MAIN, 1)] {
            let source = std::str::from_utf8(source).expect("runner source is UTF-8");
            assert_eq!(source.matches("Compilation::Initial").count(), initial);
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
            neutral.contains("runner::install_retained(engine, super::IMPORTED_BINDINGS, native)")
        );
        assert!(!neutral.contains("runner::install("));
        assert!(
            neutral.contains("engine.compile_package_snapshot_generation_with_top_level(snapshot)")
        );
        // A runner with no host profile starts its program by calling `main`
        // bound to the imported runtime.
        assert!(neutral.contains("candidate.bind(&program.engine, runtime)"));
        assert!(neutral.contains("__rawr_internal::island::invoke_main()"));
        // A native `main` (D501) is an entry the Rawr one registers, which
        // the thread that owns the program runs (#1119), and a reply's
        // native values move out of that thread's arena (#1118).
        assert!(neutral.contains(
            "if let Some(entry) = __rawr_internal::island::take_entry() {\n            return super::Started::Entry(entry);"
        ));
        let runtime = std::str::from_utf8(RUNTIME_SOURCE).expect("runtime source is UTF-8");
        assert!(runtime.contains("__rawr_internal::island::moves(moved);"));
        let runner = std::str::from_utf8(RUNNER_MAIN).expect("runner source is UTF-8");
        assert!(runner.contains("20 if admitted && payload.as_slice() == key"));
        assert!(runner.contains(".spawn(move || serve_entry(stream, received, key, live))?;"));
        assert!(runner.contains("__rawr_profile::run(running)"));
        // Every program runs its protocol, and then itself, on the main
        // thread.
        let main = runner.split_once("fn main() {").expect("runner main").1;
        assert!(main.contains("finish(run_protocol(stream, key, first))"));
        assert!(!main.contains("keep_main_thread"));
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
            "decode_snapshot(input)?",
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
            "decode_snapshot(input)?",
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
        let facade =
            crate::surface::facade::Plan::encode_cached(&contract).expect("authenticated facade");
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
        let other_facade = crate::surface::facade::Plan::encode_cached(&other_contract)
            .expect("same account alternate facade");
        assert!(matches!(
            demand().with_trusted_runner(true, &other_facade, &contract),
            Err(crate::surface::Error::ForeignRunner)
        ));

        let foreign = classify(context());
        assert_eq!(foreign.fingerprint(), contract.fingerprint());
        let foreign_facade =
            crate::surface::facade::Plan::encode_cached(&foreign).expect("foreign facade");
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
        let ancestry = |profile: &crate::profile::Proof| {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                trusted.require_runner_profile(profile).map_err(|_| ())
            }))
            .expect_err("a profile from another build panics")
            .downcast_ref::<String>()
            .cloned()
        };
        let expected =
            "the generated Rust disagrees with the crates on owned runner bindings ancestry";
        assert_eq!(ancestry(&foreign_profile).as_deref(), Some(expected));
        let missing_facade = crate::profile::Proof::neutral_with_chain(
            contract.node_chain().expect("contract chain").clone(),
        );
        assert_eq!(ancestry(&missing_facade).as_deref(), Some(expected));

        std::fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    #[cfg(unix)]
    fn early_hot_requests_execute_in_the_emitted_live_protocol() {
        let source = std::str::from_utf8(LIFECYCLE_PROFILE_RUNNER_MAIN).unwrap();
        let framing = &source[..source.find("fn lifecycle()").unwrap()];
        let live = &source
            [source.find("struct PendingLive").unwrap()..source.find("fn run_protocol()").unwrap()];
        let root =
            std::env::temp_dir().join(format!("rawr-early-hot-protocol-{}", std::process::id()));
        std::fs::create_dir(&root).expect("exclusive protocol test directory");
        let path = root.join("main.rs");
        let executable = root.join("protocol");
        std::fs::write(
            &path,
            format!(
                "{framing}\n{live}\n{}",
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
            assert!(source.contains("&12_u32.to_be_bytes()"));
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
    fn generated_runtime_binds_every_imported_type_in_one_pass() {
        let source = std::str::from_utf8(RUNTIME_SOURCE).expect("runtime source is UTF-8");
        assert!(source.contains("builder.bind_imported_type_attestations(native_attestation)?"));
        assert!(!source.contains("for selector in imported_identities"));
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
        // Another mapper's projection is a rawr bug, which panics naming
        // the invariant (#538, D664).
        let foreign = std::panic::catch_unwind(|| {
            projected_event_message(arguments, &signature, &substitutions, &[8; 16]).map(|_| ())
        })
        .expect_err("a foreign mapper panics");
        assert_eq!(
            foreign.downcast_ref::<String>().map(String::as_str),
            Some("the generated Rust disagrees with the crates on generated event projection Message")
        );
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

        // More container parameters than the signature has is a rawr bug,
        // which panics naming the invariant (#538, D664).
        let partition = std::panic::catch_unwind(|| {
            render_item_turbofish(&signature, &substitutions, &constants, 4, &mut Vec::new())
                .map_err(|_| ())
        })
        .expect_err("a foreign partition panics");
        assert_eq!(
            partition.downcast_ref::<String>().map(String::as_str),
            Some(
                "the generated Rust disagrees with the crates on generated container parameter \
                 cardinality"
            )
        );
    }

    /// An impl whose type names its parameters in another order than it
    /// declares them, as iced's `impl<W, A, B, F, ..> Widget<B, ..> for
    /// Map<W, F, A>`, is reached at its own instance, each parameter where
    /// the type names it. A callable one stands as `_`, which rustc infers
    /// from what the call takes (T425).
    #[test]
    fn a_reordered_instance_places_each_parameter_where_its_type_names_it() {
        let map = crate::surface::identity::Definition::authenticated([7; 32]);
        super::TYPE_PATHS.with_borrow_mut(|paths| {
            paths.insert(*map.as_bytes(), "gallery_ui::Map".to_owned());
        });
        let instance = Type::Nominal {
            definition: map,
            arguments: Box::from([Type::Parameter(0), Type::Parameter(3), Type::Parameter(1)]),
        };
        let typed = Signature::new(
            Box::from([
                Parameter::new(0, "W".into(), Kind::Type),
                Parameter::new(1, "A".into(), Kind::Type),
                Parameter::new(3, "F".into(), Kind::Type),
            ]),
            Box::default(),
            Type::Unit,
        )
        .with_instance(Some(instance.clone()));
        let substitutions = Box::from([
            Type::Builtin(Builtin::Bool),
            Type::Builtin(Builtin::I64),
            Type::Builtin(Builtin::String),
        ]);
        let mut ty = Vec::new();
        render_container_path(
            "gallery_ui::Map",
            &typed,
            &substitutions,
            &[],
            3,
            true,
            &mut ty,
        )
        .expect("a reordered instance renders");
        assert_eq!(ty, b"gallery_ui::Map<bool, String, i64>");

        let callable = Signature::new(
            Box::from([
                Parameter::new(0, "W".into(), Kind::Type),
                Parameter::new(1, "A".into(), Kind::Type),
                Parameter::new(3, "F".into(), Kind::Callable),
            ]),
            Box::default(),
            Type::Unit,
        )
        .with_instance(Some(instance));
        let mut expression = Vec::new();
        render_container_path(
            "gallery_ui::Map",
            &callable,
            &substitutions[..2],
            &[],
            3,
            false,
            &mut expression,
        )
        .expect("a callable parameter of an instance renders");
        assert_eq!(expression, b"<gallery_ui::Map<bool, _, i64>>");
        super::TYPE_PATHS.with_borrow_mut(std::collections::BTreeMap::clear);
    }
}
