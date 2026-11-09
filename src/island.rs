//! Native islands at run time (D501): a program whose `main` is native Rust
//! and calls into marked modules.
//!
//! The runner links the program's shadow crate, whose trampolines stand for
//! the marked functions (`package::island`). Functions defined here with
//! fixed symbol names are what the shadow crate calls, so it never depends
//! on this crate: [`__rawr_island_start_v1`] registers the native `main`,
//! and [`__rawr_island_call_v4`] runs one call of a marked function in the
//! current generation and returns its result in the wire format of
//! `program::model::island`.
//!
//! The thread that owns the program is the process's main thread, as under
//! `cargo run`: the runner [`publish`]es the first generation there, calls
//! the program's Rawr `main` (which registers the native one) and runs the
//! native `main` there, from [`take_entry`]. A call it makes runs the marked
//! code on that same thread, so a marked function can start winit's event
//! loop, which must run on the main thread (#1119). The runner's protocol
//! goes on beside it and offers each saved generation to that thread, which
//! admits it at its next call from native code (D553.3). A call from any
//! other thread runs there too, with an evaluation of its own (D490), in
//! the generation published last.

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex, OnceLock};

use crate::program::model::island::{Natives, Reader};
use crate::reload::installation::Bootstrap;
use crate::{item, program, type_def, Dynamic, ImmutableString};

pub mod lend;
pub mod place;

/// The native `main` the program registered, for the runner to start.
static START: Mutex<Option<fn() -> i32>> = Mutex::new(None);
/// How the runner moves a native value out of the script value that holds
/// it, which [`moves`] names.
static MOVE: OnceLock<Move> = OnceLock::new();

/// The generation that calls from threads that don't own the program run
/// in, with what they look up in it.
#[cfg(feature = "sync")]
static ELSEWHERE: std::sync::RwLock<Option<(Arc<Calls>, Arc<Shared>)>> =
    std::sync::RwLock::new(None);

thread_local! {
    /// The generation native calls run in, on the thread that owns the
    /// program.
    static CURRENT: RefCell<Option<Rc<Current>>> = const { RefCell::new(None) };
    /// Each marked value native code lent whole to a running call (#1484),
    /// by the first of its parts it lent: the whole's lend.
    static WHOLES: RefCell<Vec<(crate::scope::Nonce, crate::scope::Lent)>> =
        const { RefCell::new(Vec::new()) };
}

/// How the runner keeps a native value native code gives a marked function,
/// which [`lodges`] names.
static LODGE: OnceLock<Lodge> = OnceLock::new();

/// The native value a script value holds, moved out of the runner's arena
/// as the value's last copy moves it; `None` when it holds none, and `Err`
/// when it can't be reached from this thread.
pub type Move = fn(&Dynamic) -> Option<Result<Box<dyn Any>, String>>;

/// Name how the runner moves a native value out of the script value that
/// holds it, so a marked function can give native code a value of a native
/// type, as the `iced::Error` of an `iced::Result` (#1118). Set once.
pub fn moves(moved: Move) {
    let _ = MOVE.set(moved);
}

/// The native value `value` holds, moved out for native code.
pub(crate) fn moved(value: &Dynamic) -> Option<Result<Box<dyn Any>, String>> {
    MOVE.get()?(value)
}

/// Drop what a call left of an argument given for it alone: the native
/// value it is, as a by-value `View<'a>` of borrows (#1225), or each one
/// inside its `Option`s, arrays and tuples, as an `Option<Element<'a,
/// Message>>` (#1224). One the call moved on, as into a native function
/// that took it, has nothing left to drop.
fn drop_left(value: &Dynamic) {
    if let Some(inner) = crate::option::read(value) {
        if let Some(inner) = inner {
            drop_left(&inner);
        }
        return;
    }
    if let Some(items) = value.read_lock::<crate::Array>() {
        for item in items.iter() {
            drop_left(item);
        }
        return;
    }
    if let Some(Ok(left)) = moved(value) {
        drop(left);
    }
}

/// An arena of one call's own, on a thread that doesn't own the program, as
/// a tokio worker that runs a `Task::perform`'s output closure: the native
/// values the call's marked function makes, takes and gives back live there
/// until what this gives drops.
pub type Arena = fn() -> Box<dyn Any>;

/// How the runner gives a call an arena of its own, which [`arenas`] names.
static ARENA: OnceLock<Arena> = OnceLock::new();

/// Name how the runner gives a call from a thread that doesn't own the
/// program an arena of the call's own. Set once.
pub fn arenas(arena: Arena) {
    let _ = ARENA.set(arena);
}

/// Where the native value a script value holds is, lent to native code in
/// place for as long as `pins` are kept; `None` when it holds none, and
/// `Err` when it can't be lent so long. Without pins, only a value that
/// lives as long as the program may be lent.
pub type Lend = fn(&Dynamic, Option<&crate::scope::Pins>) -> Option<Result<*const dyn Any, String>>;

/// How the runner lends a native value in place, which [`lends`] names.
static LEND: OnceLock<Lend> = OnceLock::new();

/// Name how the runner lends native code a native value a script value
/// holds, where it is (#1442). Set once.
pub fn lends(lend: Lend) {
    let _ = LEND.set(lend);
}

/// The reference a call gives native code to the native value `value` is
/// (#1442), boxed for its trampoline: a `&'static [T]` of a static slice
/// of native values, or the place of any other value, a static's or one
/// `pins` keep where it is. Without `pins` the reference is a `&'static`,
/// which only a static fills.
fn in_place(value: &Dynamic, pins: Option<&crate::scope::Pins>) -> Result<Box<dyn Any>, String> {
    // A `&'static str` is the program's own text, pinned for as long as
    // the runner lives, once per text.
    if let Ok(text) = value.flatten_clone().into_immutable_string() {
        return Ok(Box::new(lasting_text(text)));
    }
    if let Some(slice) =
        crate::place::get(value).and_then(|place| crate::place::lasting_reference(&place))
    {
        return Ok(slice);
    }
    match LEND.get().and_then(|lend| lend(value, pins)) {
        Some(Ok(place)) => Ok(Box::new(place)),
        Some(Err(error)) => Err(error),
        None => Err(format!(
            "a `{}` is no native value it can lend in place",
            value.type_name()
        )),
    }
}

/// `text`, pinned for as long as the runner lives, once per text.
fn lasting_text(text: ImmutableString) -> &'static str {
    // Per thread, since without the `sync` feature the text is an `Rc` and
    // pins of it can't be shared; the interpreter's thread is one.
    thread_local! {
        static TEXTS: &'static crate::scope::Pins =
            Box::leak(Box::new(crate::scope::Pins::constant()));
    }
    let pins: &'static crate::scope::Pins = TEXTS.with(|pins| *pins);
    pins.pin(text)
}

/// The reference a call gives native code into a part native code lent it
/// where it is (#1468), boxed for its trampoline: the place `value` reaches,
/// a part the call was lent or a field borrowed of one (`&theme.colors`, a
/// projection through the part), and the accessor of its lend, which
/// proves the place's type to the trampoline. The trampoline checks that
/// the place is in its receiver, which native code keeps borrowed for as
/// long as the reference.
fn lent_place(value: &Dynamic) -> Result<Box<dyn Any>, String> {
    let refused = || {
        format!(
            "a `{}` is no reference into what native code lent it",
            value.type_name()
        )
    };
    let found = if let Some(lent) = value.read_lock::<crate::scope::Lent>().map(|lent| *lent) {
        crate::scope::place(lent.nonce()).zip(lent.shared())
    } else if let Some(projected) = value
        .read_lock::<crate::scope::Projected>()
        .map(|projected| projected.clone())
    {
        let mut found = None;
        projected
            .open(&mut |lent| {
                found = crate::scope::place(lent.nonce()).zip(lent.shared());
                Ok(Dynamic::UNIT)
            })
            .map(drop)
            .map_err(|error| error.to_string())?;
        found
    } else {
        None
    };
    let (place, accessor) = found.ok_or_else(refused)?;
    Ok(Box::new((place.cast_const(), accessor)))
}

/// What a trampoline finds of a value's type, for the runner to keep it as
/// it keeps a value it made: its `Debug` and `Display` text, its `Clone`,
/// `PartialEq`, `Hash` and `PartialOrd`, each when the type has it.
pub type Traits = (
    Option<fn(&dyn Any) -> Option<String>>,
    Option<fn(&dyn Any) -> Option<String>>,
    Option<fn(&dyn Any) -> Option<Box<dyn Any>>>,
    Option<fn(&dyn Any, &dyn Any) -> Option<bool>>,
    Option<fn(&dyn Any) -> Option<u64>>,
    Option<fn(&dyn Any, &dyn Any) -> Option<std::cmp::Ordering>>,
);

/// A native value native code gives a marked function by value (#1139):
/// the value, its type's name and its [`Traits`].
pub type Given = (Box<dyn Any>, &'static str, Traits);

/// The script value that holds a [`Given`] value, kept in the runner's
/// arena on this thread as a value of the type the witness proves; `Err`
/// says why it can't be.
pub type Lodge = fn(Given, Option<&crate::host::runtime::Witness>) -> Result<Dynamic, String>;

/// Name how the runner keeps a native value native code gives a marked
/// function. Set once.
pub fn lodges(lodge: Lodge) {
    let _ = LODGE.set(lodge);
}

/// The script value that holds `given`, kept by the runner as a value of
/// the type `witness` proves.
pub(crate) fn lodged(
    given: Given,
    witness: Option<&crate::host::runtime::Witness>,
) -> Result<Dynamic, String> {
    let lodge = LODGE
        .get()
        .ok_or("no runner keeps native values on this thread")?;
    lodge(given, witness)
}

/// What makes a native trait's object of a program value (#1264): given
/// the trait's path from the crate's root and the value's handle, a box of
/// the stand-in rawr's copy of the crate declares for the value's type,
/// which then owns the handle; `None` when the stand-in's impl is of
/// another trait.
pub type Maker = fn(&str, u64) -> Option<Given>;

/// The stand-ins' makers each crate registered, by their type's path.
static MAKERS: Mutex<Vec<(&'static str, Maker)>> = Mutex::new(Vec::new());

/// The program values native code holds as a native trait's object, by
/// their handle (#1264).
#[derive(Default)]
struct Kept {
    next: u64,
    values: HashMap<u64, Held>,
    /// What native code borrows of a marked value it holds through the
    /// results of calls that borrow it (#1473), as `view<'a>(&'a self) ->
    /// Element<'a, Message>` of a marked `State` gives: the program's
    /// strings the result reads and the regions the calls ran in, by the
    /// value's [`Anchor`]. They are let go when native code next takes the
    /// value `&mut`, as rustc has ended every such borrow by then. Boxed,
    /// so a region kept in them can name them while the table moves.
    borrowed: HashMap<Anchor, Box<crate::scope::Pins>>,
}

/// A marked value native code holds, as a call's header names it (#1473):
/// its address and a hash of its type's name, so a value of another type
/// at the same address, as a field at its start, is another anchor.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct Anchor {
    address: u64,
    ty: u64,
}

/// The pins of the marked value at `anchor`, made at its first borrow, while
/// a call that borrows it runs.
fn pins_at(anchor: Anchor) -> std::ptr::NonNull<crate::scope::Pins> {
    kept(|kept| std::ptr::NonNull::from(&**kept.borrowed.entry(anchor).or_default()))
}

/// Let go of what native code borrowed of the marked values at `anchors`,
/// each taken `&mut` by the call about to run (#1473).
fn release_anchors(anchors: &[Anchor]) {
    // Dropped once the table is free again.
    let released = kept(|kept| {
        anchors
            .iter()
            .filter_map(|anchor| kept.borrowed.remove(anchor))
            .collect::<Vec<_>>()
    });
    drop(released);
}

/// One program value native code holds as a native trait's object.
struct Held {
    /// The value, which each `&self` call on it shares, as a shared borrow
    /// does (#1302); `None` while a `&mut self` call has it.
    value: Option<Dynamic>,
    /// How many `&self` calls on it run.
    readers: usize,
    /// What native code borrows of the value through its `&self` calls'
    /// results (#1322, #1321): the program's strings a `&str` result reads
    /// in place, and the regions those calls ran in, which a result such as
    /// `view(&self)`'s `Element<'_, Message>` borrows. They are let go when
    /// the value is next taken `&mut`, as rustc has ended every such borrow
    /// by then (D772). Boxed, so a region kept in them can name them while
    /// the table moves.
    pins: Box<crate::scope::Pins>,
}

/// How a call has the program value it is the receiver of.
#[derive(Clone, Copy)]
enum Borrow {
    /// `&self`: shared with every other `&self` call that runs on it.
    Shared,
    /// `&mut self`: taken for the call, alone.
    Mutable,
}

#[cfg(feature = "sync")]
static KEPT: Mutex<Option<Kept>> = Mutex::new(None);

#[cfg(not(feature = "sync"))]
thread_local! {
    static KEPT: RefCell<Option<Kept>> = const { RefCell::new(None) };
}

/// `change` of the kept program values, or `None` while they are busy, as
/// they are when a value a change drops drops a marked value (#1492).
fn try_kept<R>(change: impl FnOnce(&mut Kept) -> R) -> Option<R> {
    #[cfg(feature = "sync")]
    {
        let mut kept = match KEPT.try_lock() {
            Ok(kept) => kept,
            Err(std::sync::TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) => return None,
        };
        Some(change(kept.get_or_insert_with(Kept::default)))
    }
    #[cfg(not(feature = "sync"))]
    KEPT.with(|kept| {
        let mut kept = kept.try_borrow_mut().ok()?;
        Some(change(kept.get_or_insert_with(Kept::default)))
    })
}

/// `change` of the kept program values.
fn kept<R>(change: impl FnOnce(&mut Kept) -> R) -> R {
    #[cfg(feature = "sync")]
    {
        let mut kept = KEPT
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        change(kept.get_or_insert_with(Kept::default))
    }
    #[cfg(not(feature = "sync"))]
    KEPT.with_borrow_mut(|kept| change(kept.get_or_insert_with(Kept::default)))
}

/// Register the makers of a crate's stand-ins, each by its type's path
/// from the crate's root: the shadow crate's island module calls this
/// before its first call into marked code.
#[unsafe(no_mangle)]
pub fn __rawr_island_objects_v1(makers: &[(&'static str, Maker)]) {
    MAKERS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .extend_from_slice(makers);
}

/// Let go of the program value a stand-in held: its box was dropped.
#[unsafe(no_mangle)]
pub fn __rawr_island_release_v1(handle: u64) {
    // Dropped once the table is free again.
    let released = kept(|kept| kept.values.remove(&handle));
    drop(released);
}

/// `value` kept under a handle of its own, as a native trait object's.
fn keep(value: Dynamic) -> u64 {
    kept(|kept| {
        kept.next += 1;
        kept.values.insert(
            kept.next,
            Held {
                value: Some(value),
                readers: 0,
                pins: Box::default(),
            },
        );
        kept.next
    })
}

/// `value`, of the program's type at `owner`, as the native trait object
/// `Box<dyn trait_name>`: the value is kept under a handle, and the
/// stand-in rawr's copy of the crate declares for `owner` holds it and
/// calls the impl's functions through it, in the generation of each call.
/// `trait_name` is the trait's path from the crate's root.
pub(crate) fn object(value: Dynamic, owner: &str, trait_name: &str) -> Result<Dynamic, String> {
    let makers = MAKERS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .iter()
        .filter(|(path, _)| *path == owner)
        .map(|(_, maker)| *maker)
        .collect::<Vec<_>>();
    let handle = keep(value);
    // The crate's own path of the trait, without the crate's name, which
    // the stand-in's impl compares with its own.
    let below = trait_name
        .split_once("::")
        .map_or(trait_name, |(_, below)| below);
    match makers.iter().find_map(|maker| maker(below, handle)) {
        Some(given) => lodged(given, None),
        None => {
            __rawr_island_release_v1(handle);
            Err(format!(
                "rawr: native code has no stand-in for `{owner}` as a `dyn {trait_name}`; \
                 restart `rawr run`"
            ))
        }
    }
}

/// The program value of `handle` for one call of the impl's function it
/// is the receiver of, `borrow`ed: a `&self` call shares it with any other
/// that runs, at any depth of calls back into the same object (#1302), and
/// a `&mut self` call takes it. A `&mut self` call while another call on
/// it runs is the borrow conflict rustc reports of such code.
///
/// A `&mut self` call lets go of what native code borrowed of the value
/// (`Held::pins`), once the table is free again.
fn take_kept(handle: u64, borrow: Borrow) -> Result<Dynamic, String> {
    let mut released = None;
    let taken = kept(|kept| {
        let held = kept.values.get_mut(&handle).ok_or_else(|| {
            "rawr: a native trait object's program value was used after it was dropped".to_owned()
        })?;
        match (borrow, held.value.as_mut()) {
            (Borrow::Shared, Some(value)) => {
                if !value.is_shared() {
                    *value = value.take().into_shared();
                }
                held.readers += 1;
                Ok(value.clone())
            }
            (Borrow::Mutable, Some(_)) if held.readers == 0 => {
                if !held.pins.is_empty() {
                    released = Some(std::mem::take(&mut held.pins));
                }
                Ok(held.value.take().map(Dynamic::flatten).unwrap_or_default())
            }
            (Borrow::Mutable, Some(_)) => Err(
                "error[E0502]: cannot borrow a native trait object as mutable because it is \
                 also borrowed as immutable: a `&mut self` method was called on it while a \
                 `&self` method on it runs"
                    .to_owned(),
            ),
            (Borrow::Shared, None) => Err(
                "error[E0502]: cannot borrow a native trait object as immutable because it is \
                 also borrowed as mutable: a `&self` method was called on it while a \
                 `&mut self` method on it runs"
                    .to_owned(),
            ),
            (Borrow::Mutable, None) => Err(
                "error[E0499]: cannot borrow a native trait object as mutable more than once \
                 at a time: a `&mut self` method was called on it while another on it runs"
                    .to_owned(),
            ),
        }
    });
    drop(released);
    taken
}

/// The pins of the program value of `handle` (`Held::pins`), while a
/// `&self` call on it runs.
fn pins_of(handle: u64) -> Option<std::ptr::NonNull<crate::scope::Pins>> {
    kept(|kept| {
        let held = kept.values.get(&handle)?;
        Some(std::ptr::NonNull::from(&*held.pins))
    })
}

/// What a stand-in's std trait asks of the program value it holds, as
/// the shadow crate's support numbers it (#1301).
mod op {
    pub(super) const DEBUG: u8 = 0;
    pub(super) const DISPLAY: u8 = 1;
    pub(super) const CLONE: u8 = 2;
    pub(super) const EQUAL: u8 = 3;
    pub(super) const HASH: u8 = 4;
    pub(super) const DEFAULT: u8 = 5;
    pub(super) const PARTIAL_ORDER: u8 = 6;
    pub(super) const ORDER: u8 = 7;
}

/// One question a stand-in's std trait asks (`__rawr_island_trait_v1`).
struct Asked<'a> {
    /// The marked type's path from the crate's root.
    owner: &'a str,
    op: u8,
    handle: u64,
    /// The other operand's handle, of an `eq` or a comparison.
    other: u64,
}

/// Answer what a stand-in's impl of a std trait asks of the program value
/// at `handle` (and `other`), by the impl of the type at `owner` the
/// program has, derived or written (#1301): its `Debug` or `Display` text,
/// a clone under a handle of its own, `eq`, a hash, a `Default` under a
/// handle of its own, `partial_cmp` or `cmp`. The reply starts with `0`
/// and holds the answer, or starts with `1` and holds a message to panic
/// with. The value is borrowed as a `&self` call borrows it.
#[unsafe(no_mangle)]
pub fn __rawr_island_trait_v1(owner: &str, op: u8, handle: u64, other: u64) -> Vec<u8> {
    let asked = Asked {
        owner,
        op,
        handle,
        other,
    };
    if current().is_some() {
        let _entry = crate::future::trampoline();
        if let Some(current) = current() {
            let catalog = current.bootstrap.catalog();
            let invoke = |function: &item::Value, arguments: &mut [Dynamic]| {
                catalog
                    .invoke_item(function, arguments)
                    .map_err(|error| error.to_string())
            };
            return current
                .calls
                .ask(&asked, catalog.authority().model(), &invoke)
                .unwrap_or_else(|message| failure(&message));
        }
    }
    #[cfg(feature = "sync")]
    if let Some((calls, shared)) = elsewhere() {
        let _entry = crate::future::entry();
        let invoke = |function: &item::Value, arguments: &mut [Dynamic]| {
            shared
                .authority
                .invoke_bound(function, arguments, shared.runtime.clone())
                .map_err(|error| error.to_string())
        };
        return calls
            .ask(&asked, shared.authority.model(), &invoke)
            .unwrap_or_else(|message| failure(&message));
    }
    failure(&format!("rawr: no runner serves the traits of `{owner}`"))
}

/// End the call `borrow` gave `value`, the program value of `handle`: a
/// `&mut self` call's value goes back, a `&self` call's share ends.
fn put_kept(handle: u64, borrow: Borrow, value: Dynamic) {
    // A value whose object was dropped meanwhile is dropped once the table
    // is free again.
    let released = kept(|kept| {
        let Some(held) = kept.values.get_mut(&handle) else {
            return Some(value);
        };
        match borrow {
            Borrow::Shared => {
                held.readers = held.readers.saturating_sub(1);
                Some(value)
            }
            Borrow::Mutable => {
                held.value = Some(value);
                None
            }
        }
    });
    drop(released);
}

/// Register the native `main` to start: the shadow crate's island module
/// calls this from the program's Rawr `main`.
#[unsafe(no_mangle)]
pub fn __rawr_island_start_v1(entry: fn() -> i32) {
    if let Ok(mut start) = START.lock() {
        *start = Some(entry);
    }
}

/// Run one call of a marked function on this thread: `path` names it from
/// the crate root, `arguments` holds its header and values, `given` the
/// native values it takes by value, which the runner keeps, and `lent` what
/// native code lends the call (D558.3), as ratatui's `&mut Frame<'_>`: each
/// its key, its type's name, its place, and the accessors the generated
/// runner reaches it by, a mutable one only when it is lent `&mut`. Only
/// those accessors claim the key, and each place stays borrowed while the
/// call runs.
///
/// The reply starts with `0` and holds the result and each argument the
/// call could change, each native value among them moved to `natives`, or
/// starts with `1` and holds a message to panic with.
///
/// [`__rawr_scope_claim_v1`] and [`__rawr_scope_release_v1`] are what the
/// accessors claim a lend by.
#[unsafe(no_mangle)]
pub fn __rawr_island_call_v4(
    path: &str,
    arguments: &[u8],
    given: &mut [Option<Given>],
    lent: &[(
        std::any::TypeId,
        &'static str,
        *mut (),
        Option<crate::scope::Accessor>,
        crate::scope::Accessor,
    )],
    natives: &mut Natives,
) -> Vec<u8> {
    let lent = lent.iter().map(|&lent| Loan(lent)).collect::<Vec<_>>();
    if current().is_some() {
        // The generation offered last is admitted first, so this call
        // runs in it.
        let _entry = crate::future::trampoline();
        if let Some(current) = current() {
            return current.answer(path, arguments, given, &lent, natives);
        }
    }
    #[cfg(feature = "sync")]
    if let Some((calls, shared)) = elsewhere() {
        let _entry = crate::future::entry();
        // The call's native values live in an arena of its own, which
        // ends once its reply holds what it gives back.
        let _arena = ARENA.get().map(|arena| arena());
        let invoke = |function: &item::Value, arguments: &mut [Dynamic]| {
            shared
                .authority
                .invoke_bound(function, arguments, shared.runtime.clone())
                .map_err(|error| error.to_string())
        };
        let call = Call {
            path,
            arguments,
            model: shared.authority.model(),
            invoke: &invoke,
        };
        return calls.answer(&call, given, &lent, natives);
    }
    failure(&format!("rawr: no runner serves the call of `{path}`"))
}

/// One argument a trampoline lends: its key, name, place and accessors.
#[derive(Clone, Copy)]
struct Loan(
    (
        std::any::TypeId,
        &'static str,
        *mut (),
        Option<crate::scope::Accessor>,
        crate::scope::Accessor,
    ),
);

/// Claim the lend `nonce` names under `key` for one access by a
/// trampoline's accessor, which ends it with [`__rawr_scope_release_v1`].
/// `Err` holds a [`crate::scope::Error`]'s code.
#[unsafe(no_mangle)]
pub fn __rawr_scope_claim_v1(
    nonce: u64,
    key: std::any::TypeId,
    shared: bool,
) -> Result<*mut (), u8> {
    crate::scope::claim(crate::scope::nonce(nonce), key, shared)
        .map(crate::scope::Claim::keep)
        .map_err(crate::scope::Error::code)
}

/// Let go of what native code borrowed of the marked value at `address`
/// whose type's name hashes to `ty`, which drops or moves out for good, so
/// rustc has ended every borrow of it (#1492). A drop while the kept values
/// are busy, inside a change of them, lets go of nothing.
#[unsafe(no_mangle)]
pub fn __rawr_island_unanchor_v1(address: u64, ty: u64) {
    // Dropped once the table is free again.
    let released = try_kept(|kept| kept.borrowed.remove(&Anchor { address, ty }));
    drop(released);
}

/// End an access [`__rawr_scope_claim_v1`] began.
#[unsafe(no_mangle)]
pub fn __rawr_scope_release_v1(nonce: u64, shared: bool) {
    crate::scope::end_claim(crate::scope::nonce(nonce), shared);
}

/// Make `bootstrap` the generation native calls run in, on this thread,
/// which owns the program from now on, and on every other thread. Async
/// steps run in it too (D513.3).
pub fn publish(bootstrap: Bootstrap) {
    let current = Rc::new(Current::new(bootstrap));
    crate::future::serve(current.clone());
    #[cfg(feature = "sync")]
    {
        crate::future::publish(current.shared.clone());
        *ELSEWHERE
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) =
            Some((current.calls.clone(), current.shared.clone()));
    }
    CURRENT.with(|slot| *slot.borrow_mut() = Some(current));
}

/// The future of the program's async `main`, which its runtime's entry
/// awaits (`__rawr_island_main_v1`).
#[cfg(feature = "sync")]
static MAIN: Mutex<Option<crate::future::Future>> = Mutex::new(None);

/// Hold `future`, the Rawr `main`'s, for the native entry its runtime's
/// attribute carries (D513.4). The runner then runs that entry, from
/// [`take_entry`].
#[cfg(feature = "sync")]
pub fn await_main(future: crate::future::Future) {
    if let Ok(mut main) = MAIN.lock() {
        *main = Some(future);
    }
}

/// The Rawr `main`'s future as the native entry awaits it: its status is
/// the process's, as `main`'s output gives Rust's (0, or 1 for an `Err`),
/// and 101 when it fails.
#[cfg(feature = "sync")]
#[unsafe(no_mangle)]
pub fn __rawr_island_main_v1() -> std::pin::Pin<Box<dyn std::future::Future<Output = i32> + Send>> {
    let Some(future) = MAIN.lock().ok().and_then(|mut main| main.take()) else {
        return Box::pin(async {
            eprintln!("rawr: the program's async `main` gave no future");
            101
        });
    };
    let mut script = future.into_script();
    // The entry's `block_on` polls this on the thread that owns the program
    // (D527): each poll runs the steps other threads queued, then `main`'s,
    // all in the runtime's context.
    Box::pin(std::future::poll_fn(move |cx| {
        crate::future::drive(cx.waker());
        let _entered = crate::future::enter();
        crate::future::drain();
        script.poll_output(cx).map(|output| match output {
            Ok(value) => match value.try_cast::<Result<Dynamic, Dynamic>>() {
                Some(Err(error)) => {
                    eprintln!("Error: {error:?}");
                    1
                }
                _ => 0,
            },
            Err(error) => {
                eprintln!("error: {}", crate::future::message(&error));
                101
            }
        })
    }))
}

/// Call the published generation's Rawr `fn main`. `None` when nothing is
/// published or it has no `main`.
pub fn invoke_main() -> Option<Result<Dynamic, program::Error>> {
    current()?.bootstrap.invoke_main()
}

/// Call the published generation's root function `name` with
/// `arguments`, as a call from native code into script code: the outermost
/// one first admits the generation offered last (D553.3). `None` when
/// nothing is published on this thread or its program has no such
/// function.
pub fn invoke(name: &str, arguments: &mut [Dynamic]) -> Option<Result<Dynamic, program::Error>> {
    let _entry = crate::future::entry();
    current()?.bootstrap.invoke_root(name, arguments)
}

/// Check `value`, which native code gave back where a script type was
/// erased to [`crate::Value`] (D513.2), against the exact type `witness`
/// proves, in the generation published on this thread.
pub fn validate(witness: &crate::host::runtime::Witness, value: &Dynamic) -> Result<(), String> {
    let current = current().ok_or("no generation is published on this thread")?;
    current
        .bootstrap
        .catalog()
        .validate_witness(witness, value)
        .map_err(|error| error.to_string())
}

/// `value`, a program value of a marked type the runner holds as its
/// mirror in rawr's copy of the crate, as a crate's generic takes it
/// (`Task<Message>`, #1249): written as a reply writes a marked function's
/// value, each native value it holds moved to the natives that come with
/// the bytes. `Err` names what can't cross.
pub fn mirror_out(value: &Dynamic) -> Result<(Vec<u8>, Natives), String> {
    let mut bytes = Vec::new();
    let mut natives = Natives::new();
    with_types(|model, _| model.island_write_in(value, None, &mut bytes, &mut natives, 0))?;
    Ok((bytes, natives))
}

/// The program value of a mirror native code gives back (#1249), which
/// [`mirror_out`]'s counterpart in rawr's copy of the crate wrote as
/// `bytes`, each native value it gives kept by the runner.
pub fn mirror_in(bytes: &[u8], given: &mut [Option<Given>]) -> Result<Dynamic, String> {
    with_types(|model, types| model.island_read(types, &[], given, None, &mut Reader { bytes }, 0))
}

/// Note that native code lent `value`, a marked value read from a call's
/// arguments, whole by `whole` (#1484), as well as part by part. A value
/// none of whose parts is lent in place needs no note: it crosses whole by
/// the wire format.
pub(crate) fn lent_whole(value: &Dynamic, whole: crate::scope::Lent) {
    if let Some(part) = first_lent(value, 0) {
        WHOLES.with_borrow_mut(|wholes| wholes.push((part, whole)));
    }
}

/// Whether native code lent `value` whole to a running call, as a value of
/// some type ([`lent_whole_of`]).
#[must_use]
pub fn lends_whole(value: &Dynamic) -> bool {
    // Most calls run with no whole lent, and search nothing.
    if WHOLES.with_borrow(Vec::is_empty) {
        return false;
    }
    first_lent(value, 0).is_some_and(|part| {
        WHOLES.with_borrow(|wholes| wholes.iter().any(|(first, _)| *first == part))
    })
}

/// The lend of `value`, a marked value of the mirror `T` native code lent
/// a running call whole (#1484): native code's own `&T`, which the call
/// gives back to native code that takes the value by reference, as rustc
/// reborrows it. `None` when native code lent no such `T`, or its lend has
/// ended.
#[must_use]
pub fn lent_whole_of<T: 'static>(value: &Dynamic) -> Option<crate::scope::Lent> {
    if WHOLES.with_borrow(Vec::is_empty) {
        return None;
    }
    let part = first_lent(value, 0)?;
    WHOLES.with_borrow(|wholes| {
        wholes
            .iter()
            .rev()
            .filter(|(first, _)| *first == part)
            .map(|(_, whole)| *whole)
            .find(|whole| {
                // The trampoline made this accessor for a `T`'s place.
                whole
                    .accessor::<fn(u64, &mut dyn FnMut(&T)) -> Result<(), u8>>(false)
                    .is_ok()
                    && crate::scope::place(whole.nonce()).is_some()
            })
    })
}

/// The lend of the first native part `value` holds lent in place, searched
/// field by field.
fn first_lent(value: &Dynamic, depth: usize) -> Option<crate::scope::Nonce> {
    if depth > 64 {
        return None;
    }
    let depth = depth + 1;
    match &value.0 {
        crate::types::dynamic::Union::Struct(value, ..) => value
            .fields()
            .iter()
            .find_map(|field| first_lent(field, depth)),
        crate::types::dynamic::Union::Enum(value, ..) => value
            .payload()
            .iter()
            .find_map(|item| first_lent(item, depth)),
        crate::types::dynamic::Union::Array(items, ..) => {
            items.iter().find_map(|item| first_lent(item, depth))
        }
        crate::types::dynamic::Union::Shared(cell, ..) => {
            first_lent(&*crate::func::locked_read(cell)?, depth)
        }
        crate::types::dynamic::Union::Variant(..) => {
            if let Some(lent) = value.read_lock::<crate::scope::Lent>() {
                return Some(lent.nonce());
            }
            let option = value.read_lock::<crate::option::Value>()?;
            match &*option {
                Some(inner) => first_lent(inner, depth),
                None => None,
            }
        }
        _ => None,
    }
}

/// Each text `value`, a marked value read from a call's arguments, holds,
/// added to `texts` in the order native code wrote them: field by field,
/// as the wire format writes a value.
fn texts_of(value: &Dynamic, texts: &mut Vec<ImmutableString>, depth: usize) {
    if depth > 64 {
        return;
    }
    let depth = depth + 1;
    match &value.0 {
        crate::types::dynamic::Union::Str(text, ..) => texts.push(text.clone()),
        crate::types::dynamic::Union::Struct(value, ..) => {
            for field in value.fields() {
                texts_of(field, texts, depth);
            }
        }
        crate::types::dynamic::Union::Enum(value, ..) => {
            for item in value.payload() {
                texts_of(item, texts, depth);
            }
        }
        crate::types::dynamic::Union::Array(items, ..) => {
            for item in items.iter() {
                texts_of(item, texts, depth);
            }
        }
        crate::types::dynamic::Union::Shared(cell, ..) => {
            if let Some(inner) = crate::func::locked_read(cell) {
                texts_of(&inner, texts, depth);
            }
        }
        crate::types::dynamic::Union::Variant(..) => {
            if let Some(option) = value.read_lock::<crate::option::Value>() {
                if let Some(inner) = &*option {
                    texts_of(inner, texts, depth);
                }
            } else if let Some(result) = value.read_lock::<crate::result::Value>() {
                let (Ok(inner) | Err(inner)) = &*result;
                texts_of(inner, texts, depth);
            }
        }
        _ => {}
    }
}

/// Where `text` starts in `held`, when it is a part of that very text, as
/// `&self.0` is of the receiver's: by address, never by what it reads.
fn start_in(held: &str, text: &str) -> Option<usize> {
    let start = held.as_ptr().addr();
    let at = text.as_ptr().addr();
    let offset = at.checked_sub(start)?;
    (offset + text.len() <= held.len()).then_some(offset)
}

/// `read` of the published generation's model and its crate's script
/// types: on the thread that owns the program, or the one published last.
fn with_types<R>(
    read: impl FnOnce(&program::model::Model, &program::model::island::Types) -> Result<R, String>,
) -> Result<R, String> {
    if let Some(current) = current() {
        return read(
            current.bootstrap.catalog().authority().model(),
            &current.calls.types,
        );
    }
    #[cfg(feature = "sync")]
    if let Some((calls, shared)) = elsewhere() {
        return read(shared.authority.model(), &calls.types);
    }
    Err("no program is published on this thread".to_owned())
}

/// The entry the program registered for the thread that owns it to run:
/// its native `main` (D501), or the runtime entry an async `main` gave
/// (D513.4), whose `block_on` drives the program (D527).
pub fn take_entry() -> Option<fn() -> i32> {
    START.lock().ok().and_then(|mut start| start.take())
}

/// Run `body` with each of `lent` lent, outermost first, and the script
/// values that stand for them. Every lend ends before this returns.
fn lend_all<R>(
    lent: &[Loan],
    values: &mut Vec<crate::scope::Lent>,
    body: &mut dyn FnMut(&[crate::scope::Lent]) -> R,
) -> R {
    let Some((Loan((key, name, place, mutable, shared)), rest)) = lent.split_first() else {
        return body(values);
    };
    // SAFETY: the trampoline that made this call made `place` from its
    // argument's borrow for the call (`Call::lend`), on this thread, which
    // runs the call. The reply is made inside this lend, and only its
    // accessors claim `key`, reading the place as the type it is.
    unsafe {
        crate::scope::lend_raw(*key, *place, mutable.is_some(), |nonce| {
            values.push(crate::scope::Lent::new(
                nonce,
                name,
                *mutable,
                Some(*shared),
            ));
            lend_all(rest, values, body)
        })
    }
}

fn current() -> Option<Rc<Current>> {
    CURRENT.with(|slot| slot.borrow().clone())
}

fn failure(message: &str) -> Vec<u8> {
    let mut reply = Vec::with_capacity(1 + message.len());
    reply.push(1);
    reply.extend_from_slice(message.as_bytes());
    reply
}

/// One published generation, on the thread that owns the program.
struct Current {
    bootstrap: Bootstrap,
    #[cfg(feature = "sync")]
    shared: Arc<Shared>,
    calls: Arc<Calls>,
}

/// What a generation's calls from native code look up, found once: each
/// function by whether it belongs to a type, and its path, and the crate's
/// script types.
struct Calls {
    items: HashMap<(bool, String), item::Value>,
    types: program::model::island::Types,
    /// What each native value a function takes by value is attested as.
    inputs: HashMap<(bool, String), Box<[Option<program::model::island::Part>]>>,
}

/// One call from native code: the function's path and the arguments its
/// trampoline wrote, with the generation's model and how it invokes a
/// function.
struct Call<'a> {
    path: &'a str,
    arguments: &'a [u8],
    model: &'a program::model::Model,
    invoke: &'a Invoke<'a>,
}

/// How a generation invokes one of its functions with its arguments.
type Invoke<'a> = dyn Fn(&item::Value, &mut [Dynamic]) -> Result<Dynamic, String> + 'a;

impl crate::future::Host for Current {
    fn invoke(
        &self,
        function: &item::Value,
        arguments: &mut [Dynamic],
    ) -> crate::RawrResultOf<Dynamic> {
        self.bootstrap
            .catalog()
            .invoke_item(function, arguments)
            .map_err(|error| crate::future::error(error.to_string()))
    }

    #[cfg(feature = "sync")]
    fn share(&self) -> Option<crate::future::Program> {
        Some(self.shared.clone() as crate::future::Program)
    }
}

/// `authority` with `runtime` as any thread may use it, with each script
/// type's own `Clone` found once, as a test calls it.
#[cfg(all(test, feature = "sync"))]
pub(crate) fn shared(
    authority: program::Authority,
    runtime: crate::Shared<crate::host::runtime::Registry>,
) -> crate::future::Program {
    share(authority, runtime)
}

/// `authority` with `runtime` as any thread may use it, with each script
/// type's own `Clone` found once.
#[cfg(feature = "sync")]
fn share(
    authority: program::Authority,
    runtime: crate::Shared<crate::host::runtime::Registry>,
) -> Arc<Shared> {
    let methods = |name: &str| {
        authority
            .items()
            .filter(|item| item.kind() == item::Kind::Method && item.key().name() == name)
            .filter_map(|item| Some((item.key().owner()?.clone(), item.clone())))
            .collect::<HashMap<_, _>>()
    };
    let clones = methods("Clone::clone");
    let equals = methods("PartialEq::eq");
    let displays = methods("Display::fmt");
    let defaults = authority
        .items()
        .filter(|item| {
            item.kind() == item::Kind::AssociatedFunction && item.key().name() == "Default::default"
        })
        .filter_map(|item| match item.key().owner()? {
            type_def::Identity::Script(owner) => {
                let mut path = owner.modules().join("::");
                if !path.is_empty() {
                    path.push_str("::");
                }
                path.push_str(owner.name());
                // Keyed as the runner names the type (`crossing_path`).
                Some((authority.model().crossing_path(&path), item.clone()))
            }
            type_def::Identity::Host(_) => None,
        })
        .collect();
    let members = authority
        .items()
        .filter(|item| {
            matches!(
                item.kind(),
                item::Kind::Method | item::Kind::AssociatedFunction
            ) && item.key().name().contains("::")
        })
        .filter_map(|item| match item.key().owner()? {
            type_def::Identity::Script(owner) => {
                let mut path = owner.modules().join("::");
                if !path.is_empty() {
                    path.push_str("::");
                }
                path.push_str(owner.name());
                let mut path = authority.model().crossing_path(&path);
                path.push_str("::");
                path.push_str(item.key().name());
                Some((path, item.clone()))
            }
            type_def::Identity::Host(_) => None,
        })
        .collect();
    let layouts = layouts(authority.items());
    Arc::new(Shared {
        authority,
        runtime,
        clones,
        equals,
        displays,
        defaults,
        members,
        layouts,
    })
}

/// The script type of `value`, a script struct or enum.
#[cfg(feature = "sync")]
fn script_identity(value: &Dynamic) -> Option<&type_def::Identity> {
    match &value.0 {
        crate::types::dynamic::Union::Struct(value, ..) => Some(value.identity()),
        crate::types::dynamic::Union::Enum(value, ..) => Some(value.identity()),
        _ => None,
    }
}

/// The function that writes a lifted closure, by its path, or a method's
/// owner and name: its closures are named `{name}{closure#N}`. `None` for
/// any other item.
#[cfg(feature = "sync")]
fn enclosing(key: &item::Key) -> Option<String> {
    let mut path = key.to_string();
    let at = path.find("{closure#")?;
    path.truncate(at);
    Some(path)
}

/// The closures each function writes, in key order, found once for a
/// generation.
#[cfg(feature = "sync")]
fn layouts<'a>(
    items: impl Iterator<Item = &'a item::Value>,
) -> HashMap<String, Box<[item::Value]>> {
    let mut layouts = HashMap::<String, Vec<item::Value>>::new();
    for item in items {
        if let Some(enclosing) = enclosing(item.key()) {
            layouts.entry(enclosing).or_default().push(item.clone());
        }
    }
    layouts
        .into_iter()
        .map(|(enclosing, mut closures)| {
            closures.sort_by(|a, b| a.key().cmp(b.key()));
            (enclosing, closures.into_boxed_slice())
        })
        .collect()
}

/// Whether two generations' closures of one function are the same: the
/// same keys, each with the same signature and captures (D553.5). A
/// closure inserted before another renumbers it, and the other's body
/// would run a sibling's.
#[cfg(feature = "sync")]
fn same_closures(one: Option<&[item::Value]>, other: Option<&[item::Value]>) -> bool {
    match (one, other) {
        (Some(one), Some(other)) => {
            one.len() == other.len() && one.iter().zip(other).all(|(a, b)| a.same_item(b))
        }
        _ => false,
    }
}

/// The crate root's `fn main`, from `fn` to its closing brace, without its
/// comments and with each run of whitespace one space: an edit that only
/// moves or reflows it changes nothing. The closures it writes are part of
/// it, so an edit to one restarts too. `None` without a `fn main`.
#[cfg(feature = "sync")]
fn main_text(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let is_ident = |byte: u8| byte.is_ascii_alphanumeric() || byte == b'_';
    let start = (0..bytes.len()).find(|&at| {
        text.is_char_boundary(at)
            && text[at..].starts_with("fn")
            && (at == 0 || !is_ident(bytes[at - 1]))
            && text[at + 2..].trim_start().starts_with("main")
            && text[at + 2..].trim_start()[4..]
                .trim_start()
                .starts_with(['(', '<'])
            && bytes.get(at + 2).is_some_and(u8::is_ascii_whitespace)
    })?;
    let mut output = String::new();
    let mut depth = 0_usize;
    let mut at = start;
    let mut space = false;
    while at < bytes.len() {
        let rest = &text[at..];
        let byte = bytes[at];
        let skip = if rest.starts_with("//") {
            rest.find('\n').unwrap_or(rest.len())
        } else if let Some(comment) = rest.strip_prefix("/*") {
            comment.find("*/").map_or(rest.len(), |end| end + 4)
        } else {
            0
        };
        if skip > 0 {
            space = true;
            at += skip;
            continue;
        }
        if byte.is_ascii_whitespace() {
            space = true;
            at += 1;
            continue;
        }
        if space && !output.is_empty() {
            output.push(' ');
        }
        space = false;
        let length = if byte == b'"' || (byte == b'r' && rest[1..].starts_with(['"', '#'])) {
            // A string: its text counts as it is.
            let hashes = rest[usize::from(byte == b'r')..]
                .bytes()
                .take_while(|&b| b == b'#')
                .count();
            let open = usize::from(byte == b'r') + hashes + 1;
            if byte == b'r' || hashes > 0 {
                let close = format!("\"{}", "#".repeat(hashes));
                rest[open..]
                    .find(&close)
                    .map_or(rest.len(), |end| open + end + close.len())
            } else {
                let mut end = 1;
                while end < rest.len() && bytes[at + end] != b'"' {
                    end += if bytes[at + end] == b'\\' { 2 } else { 1 };
                }
                (end + 1).min(rest.len())
            }
        } else if byte == b'\'' && rest[1..].starts_with('\\') {
            rest[2..].find('\'').map_or(rest.len(), |end| end + 3)
        } else if byte == b'\'' && rest[1..].chars().nth(1) == Some('\'') {
            1 + rest[1..].chars().next().map_or(0, char::len_utf8) + 1
        } else {
            rest.chars().next().map_or(1, char::len_utf8)
        };
        output.push_str(&rest[..length]);
        match byte {
            b'{' => depth += 1,
            b'}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(output);
                }
            }
            _ => {}
        }
        at += length;
    }
    Some(output)
}

/// Why `candidate` can't replace `current` while a marked `main` runs
/// (D553.6), in one line for `rawr run` to restart with. `None` when it
/// swaps in: a body edit, or a new function or type.
///
/// - A script type whose fields or variants changed: rawr can't yet see
///   which script values native code stores as `rawr::Value`.
/// - A callable a marked `main`'s own frame gave native code, which no
///   longer rebinds: `main` never runs again to give a new one.
/// - A program that never called from native code into script code: no
///   entry would ever admit the new generation.
#[cfg(feature = "sync")]
#[must_use]
pub fn refusal(
    current: &crate::reload::Generation,
    candidate: &crate::reload::Candidate,
) -> Option<String> {
    let path = |key: &type_def::Key| {
        key.modules()
            .iter()
            .map(ImmutableString::as_str)
            .chain([key.name()])
            .collect::<Vec<_>>()
            .join("::")
    };
    for (identity, fingerprint) in current.schemas().iter() {
        let type_def::Identity::Script(key) = identity else {
            continue;
        };
        match candidate.schemas().get(identity) {
            Some(next) if next == fingerprint => {}
            Some(_) => return Some(format!("`{}` changed its fields", path(key))),
            None => return Some(format!("`{}` was removed", path(key))),
        }
    }
    if !crate::future::called_from_native() {
        return Some("the source changed".to_owned());
    }
    // `main` runs once: an edit to it would need it to run again.
    if main_text(current.source().entry_source().text())
        != main_text(candidate.source().entry_source().text())
    {
        return Some("`main` changed".to_owned());
    }
    let held = crate::future::held();
    if held.is_empty() {
        return None;
    }
    let items = candidate
        .items()
        .map(|item| (item.key(), item))
        .collect::<HashMap<_, _>>();
    let closures = layouts(candidate.items());
    for (function, program) in held {
        let key = function.key();
        let same = items.get(key).is_some_and(|next| next.same_item(&function))
            && enclosing(key).is_none_or(|enclosing| {
                same_closures(
                    closures.get(&enclosing).map(|closures| &**closures),
                    program.closures(key),
                )
            });
        if !same {
            let name = key.name();
            return Some(match name.find("{closure#") {
                Some(at) => format!("a closure `{}` gave native code changed", &name[..at]),
                None => format!("`{name}`, which `main` gave native code, changed"),
            });
        }
    }
    None
}

/// The published program as any thread may use it (D490): calls with an
/// evaluation of their own, checks of values native code gives, and each
/// script type's own `Clone`.
#[cfg(feature = "sync")]
struct Shared {
    authority: program::Authority,
    runtime: crate::Shared<crate::host::runtime::Registry>,
    clones: HashMap<type_def::Identity, item::Value>,
    /// Each script type's own `PartialEq::eq`.
    equals: HashMap<type_def::Identity, item::Value>,
    /// Each script type's own `Display::fmt`.
    displays: HashMap<type_def::Identity, item::Value>,
    /// Each script type's own `impl Default`, by its path (D594).
    defaults: HashMap<String, item::Value>,
    /// Each function of a script type's trait impls, by its owner's path
    /// and the member (`Arc::Program<Message>::draw`), which a runner's
    /// impl for the owner's proxy calls (D594).
    members: HashMap<String, item::Value>,
    /// The closures each function writes (D553.5).
    layouts: HashMap<String, Box<[item::Value]>>,
}

#[cfg(feature = "sync")]
impl crate::future::Remote for Shared {
    fn invoke(
        &self,
        function: &item::Value,
        arguments: &mut [Dynamic],
    ) -> crate::RawrResultOf<Dynamic> {
        self.authority
            .invoke_bound(function, arguments, self.runtime.clone())
            .map_err(|error| crate::future::error(error.to_string()))
    }

    fn validate(
        &self,
        witness: &crate::host::runtime::Witness,
        value: &Dynamic,
    ) -> Result<(), String> {
        witness
            .ty()
            .validate(value, self.authority.model(), &self.runtime)
            .map_err(|error| error.to_string())
    }

    fn clone_value(&self, value: &Dynamic) -> Option<crate::RawrResultOf<Dynamic>> {
        let clone = self.clones.get(script_identity(value)?)?;
        Some(self.invoke(clone, &mut [value.clone()]))
    }

    fn debug(&self, value: &Dynamic) -> Option<String> {
        self.authority.model().debug_text(value)
    }

    fn equal(&self, lhs: &Dynamic, rhs: &Dynamic) -> Option<crate::RawrResultOf<bool>> {
        let identity = script_identity(lhs)?;
        let Some(equal) = self.equals.get(identity) else {
            return Some(Ok(crate::host::value::structural_equal(lhs, rhs)));
        };
        Some(
            self.invoke(equal, &mut [lhs.clone(), rhs.clone()])
                .and_then(|equal| {
                    equal
                        .as_bool()
                        .map_err(|actual| crate::future::error(format!("rawr: `eq` gave {actual}")))
                }),
        )
    }

    fn display(&self, value: &Dynamic) -> Option<crate::RawrResultOf<String>> {
        let display = self.displays.get(script_identity(value)?)?;
        Some(
            self.invoke(
                display,
                &mut [value.clone(), Dynamic::from(ImmutableString::new())],
            )
            .map(|text| text.to_string()),
        )
    }

    fn default_of(&self, path: &str) -> Option<crate::RawrResultOf<Dynamic>> {
        if let Some(default) = self.defaults.get(path) {
            return Some(self.invoke(default, &mut []));
        }
        self.authority.model().default_by_path(path).map(|made| {
            made.map_err(|error| {
                crate::future::error(format!("rawr: `{path}`'s `Default`: {error}"))
            })
        })
    }

    fn member(&self, path: &str) -> Option<item::Value> {
        self.members.get(path).cloned()
    }

    fn member_witnesses(&self, path: &str) -> Vec<Option<crate::host::runtime::Witness>> {
        self.authority
            .model()
            .member_types(path)
            .unwrap_or_default()
            .iter()
            .map(|ty| ty.as_ref().map(|ty| self.runtime.witness_for(ty)))
            .collect()
    }

    fn rebinds(&self, function: &item::Value, from: &dyn crate::future::Remote) -> bool {
        let key = function.key();
        self.authority.rebind(function).is_ok()
            && (enclosing(key).is_none() || same_closures(self.closures(key), from.closures(key)))
    }

    fn closures(&self, closure: &item::Key) -> Option<&[item::Value]> {
        self.layouts
            .get(&enclosing(closure)?)
            .map(|closures| &**closures)
    }
}

impl Current {
    fn new(bootstrap: Bootstrap) -> Self {
        let authority = bootstrap.catalog().authority();
        let calls = Arc::new(Calls::new(authority, bootstrap.catalog().runtime()));
        #[cfg(feature = "sync")]
        let shared = {
            let (authority, runtime) = bootstrap.catalog().shared();
            share(authority, runtime)
        };
        Self {
            bootstrap,
            #[cfg(feature = "sync")]
            shared,
            calls,
        }
    }

    fn answer(
        &self,
        path: &str,
        arguments: &[u8],
        given: &mut [Option<Given>],
        lent: &[Loan],
        natives: &mut Natives,
    ) -> Vec<u8> {
        let catalog = self.bootstrap.catalog();
        let invoke = |function: &item::Value, arguments: &mut [Dynamic]| {
            catalog
                .invoke_item(function, arguments)
                .map_err(|error| error.to_string())
        };
        let call = Call {
            path,
            arguments,
            model: catalog.authority().model(),
            invoke: &invoke,
        };
        self.calls.answer(&call, given, lent, natives)
    }
}

/// The calls from threads that don't own the program, in the generation
/// published last.
#[cfg(feature = "sync")]
fn elsewhere() -> Option<(Arc<Calls>, Arc<Shared>)> {
    ELSEWHERE.read().ok()?.clone()
}

impl Calls {
    fn new(
        authority: &program::Authority,
        runtime: &crate::Shared<crate::host::runtime::Registry>,
    ) -> Self {
        let join = |modules: &[ImmutableString], names: &[&str]| {
            modules
                .iter()
                .map(ImmutableString::as_str)
                .chain(names.iter().copied())
                .collect::<Vec<_>>()
                .join("::")
        };
        let mut items = HashMap::new();
        let mut ambiguous = Vec::new();
        for item in authority.items() {
            if !matches!(
                item.kind(),
                item::Kind::Function | item::Kind::AssociatedFunction | item::Kind::Method
            ) {
                continue;
            }
            let key = item.key();
            let entry = match key.owner() {
                None => (false, join(key.modules(), &[key.name()])),
                // A trait's impl for a crate's or a builtin type, as `impl
                // From<&Theme> for markdown::Settings` (#1466), owned by
                // `std::` and the type's path: by `::` and the type's name,
                // as rawr's copy of the crate names it, which knows the
                // type only as the impl writes it.
                Some(type_def::Identity::Script(owner))
                    if owner.package() == "std" && owner.modules().is_empty() =>
                {
                    let name = owner.name().rsplit("::").next().unwrap_or_default();
                    let entry = (true, format!("::{name}::{}", key.name()));
                    if items.contains_key(&entry) {
                        ambiguous.push(entry);
                        continue;
                    }
                    entry
                }
                Some(type_def::Identity::Script(owner)) => {
                    (true, join(owner.modules(), &[owner.name(), key.name()]))
                }
                Some(type_def::Identity::Host(_)) => continue,
            };
            items.insert(entry, item.clone());
        }
        // Two types of one name with impls of one trait: native code's call
        // names neither.
        for entry in ambiguous {
            items.remove(&entry);
        }
        let inputs = authority
            .model()
            .island_input_parts(|ty| runtime.witness_for(ty));
        Self {
            items,
            types: authority.model().island_types(|ty| runtime.witness_for(ty)),
            inputs,
        }
    }

    fn answer(
        &self,
        call: &Call<'_>,
        given: &mut [Option<Given>],
        lent: &[Loan],
        natives: &mut Natives,
    ) -> Vec<u8> {
        // What the call notes of its wholes ends with its lends.
        let wholes = WHOLES.with_borrow(Vec::len);
        let reply = lend_all(lent, &mut Vec::with_capacity(lent.len()), &mut |lent| {
            self.call(call, given, lent, natives)
        });
        WHOLES.with_borrow_mut(|noted| noted.truncate(wholes));
        reply.unwrap_or_else(|message| failure(&message))
    }

    fn call(
        &self,
        call: &Call<'_>,
        given: &mut [Option<Given>],
        lent: &[crate::scope::Lent],
        natives: &mut Natives,
    ) -> Result<Vec<u8>, String> {
        let mut scoped = Vec::new();
        let reply = self.call_with(call, given, lent, natives, &mut scoped);
        for value in &scoped {
            drop_left(value);
        }
        reply
    }

    /// [`Calls::call`], each argument given for the call alone added to
    /// `scoped` as it is read.
    fn call_with(
        &self,
        call: &Call<'_>,
        given: &mut [Option<Given>],
        lent: &[crate::scope::Lent],
        natives: &mut Natives,
        scoped: &mut Vec<Dynamic>,
    ) -> Result<Vec<u8>, String> {
        let path = call.path;
        let malformed = |error: String| format!("rawr: the call of `{path}` is malformed: {error}");
        let mut reader = Reader {
            bytes: call.arguments,
        };
        let flags = reader.byte().map_err(malformed)?;
        let owned = flags & program::model::island::OWNED != 0;
        let gives = if flags & program::model::island::BORROWS != 0 {
            Gives::Text
        } else if flags & program::model::island::IN_PLACE != 0 {
            Gives::InPlace {
                lasting: flags & program::model::island::LASTING != 0,
            }
        } else if flags & program::model::island::UNIT_RESULT != 0 {
            Gives::Unit
        } else {
            Gives::Value
        };
        let count = u32::from_le_bytes(reader.array().map_err(malformed)?) as usize;
        let mutable = u64::from_le_bytes(reader.array().map_err(malformed)?);
        let anchor = |reader: &mut Reader<'_>| -> Result<Anchor, String> {
            let address = u64::from_le_bytes(reader.array().map_err(malformed)?);
            let ty = u64::from_le_bytes(reader.array().map_err(malformed)?);
            Ok(Anchor { address, ty })
        };
        let anchored = if flags & program::model::island::ANCHORED != 0 {
            Some(anchor(&mut reader)?)
        } else {
            None
        };
        if flags & program::model::island::RELEASES != 0 {
            let released = u32::from_le_bytes(reader.array().map_err(malformed)?) as usize;
            let released = (0..released)
                .map(|_| anchor(&mut reader))
                .collect::<Result<Vec<_>, _>>()?;
            release_anchors(&released);
        }
        let (given_for_call, kept) = if flags & program::model::island::SCOPES != 0 {
            (
                u64::from_le_bytes(reader.array().map_err(malformed)?),
                u64::from_le_bytes(reader.array().map_err(malformed)?),
            )
        } else {
            (0, 0)
        };
        // A lend native code keeps for as long as the result, which only an
        // anchored call's result can, is reached in its region (#1413). A
        // call anchored on nothing keeps its lends for the call alone, in a
        // region of its own (#1491).
        let lent = lent
            .iter()
            .enumerate()
            .map(|(index, lent)| {
                if index < 64 && kept & (1 << index) != 0 && !lent.writable() {
                    lent.kept()
                } else {
                    *lent
                }
            })
            .collect::<Vec<_>>();
        let lent = lent.as_slice();
        let item = self.items.get(&(owned, path.to_owned())).ok_or_else(|| {
            format!("rawr: the running program has no function `{path}`; restart `rawr run`")
        })?;
        let model = call.model;
        let inputs = self.inputs.get(&(owned, path.to_owned()));
        let mut values = Vec::with_capacity(count);
        // A stand-in's receiver is its program value, taken for the call
        // (#1264).
        let mut held = Vec::new();
        for index in 0..count {
            if reader.bytes.first() == Some(&program::model::island::HANDLE) {
                reader.byte().map_err(malformed)?;
                let handle = u64::from_le_bytes(reader.array().map_err(malformed)?);
                let borrow = match item.receiver() {
                    Some(crate::receiver::Mode::Shared) => Borrow::Shared,
                    _ => Borrow::Mutable,
                };
                let value = match take_kept(handle, borrow) {
                    Ok(value) => value,
                    Err(message) => {
                        restore(&held, &mut values);
                        return Err(message);
                    }
                };
                held.push((index, handle, borrow));
                values.push(value);
                continue;
            }
            let part = inputs.and_then(|inputs| inputs.get(index)?.as_ref());
            match model.island_read(&self.types, lent, given, part, &mut reader, 0) {
                Ok(value) => values.push(value),
                Err(error) => {
                    restore(&held, &mut values);
                    return Err(malformed(error));
                }
            }
        }
        // An argument given for the call alone, as a by-value `View<'a>` of
        // borrows (#1225), drops once the call returns: its handle is kept
        // to reach what the call left of it.
        scoped.extend(
            values
                .iter()
                .enumerate()
                .filter(|(index, _)| *index < 64 && given_for_call & (1 << index) != 0)
                .map(|(_, value)| value.clone()),
        );
        // A function changes its first `&mut` parameter in place and copies
        // the others back out after its value, as `[value, ..copied]`; a
        // method or an associated function copies all but its receiver
        // (`resolve::lower_nominal::copied_parameters`). A lent `&mut` is
        // one of them, as `draw(frame: &mut Frame, app: &mut App)` changes
        // `frame` in place and copies `app` out, and `draw(frame: &mut
        // Frame, state: &mut ListState)` copies out the lent `state`. They
        // are found before the call, which moves its arguments out.
        let references = values
            .iter()
            .enumerate()
            .filter(|(index, value)| {
                (*index < 64 && mutable & (1 << index) != 0)
                    || value
                        .read_lock::<crate::scope::Lent>()
                        .is_some_and(|lent| lent.writable())
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        let made = Made {
            item,
            held,
            references,
            mutable,
            gives,
            own: lent.iter().map(crate::scope::Lent::nonce).collect(),
        };
        // A `&self` call on a native trait object's program value runs in a
        // region anchored on the value's pins, so what its result borrows,
        // as `view(&self)`'s `Element<'_, Message>` borrows the value's
        // strings, lives till the value is next taken `&mut` (#1321, D772).
        // A call whose result borrows a marked value native code holds, as
        // `view<'a>(&'a self, ..) -> Element<'a, Message>` borrows its
        // `State`, runs the same way, anchored on that value's pins
        // (#1473).
        let shared = made
            .held
            .iter()
            .find(|(_, _, borrow)| matches!(borrow, Borrow::Shared))
            .and_then(|&(_, handle, _)| pins_of(handle))
            .or_else(|| anchored.map(pins_at));
        let Some(pins) = shared else {
            if kept == 0 {
                return self.finish(call, made, values, None, natives);
            }
            // Its lends kept for the call alone (#1491): a crate fn's output
            // that keeps one is made in a region of the call's own, which
            // ends with it. The result borrows nothing, so it takes nothing
            // of the region.
            let pins = crate::scope::Pins::new();
            let mut region = crate::scope::Region::anchored(&pins);
            return crate::scope::open_anchored(&mut region, || {
                self.finish(call, made, values, None, natives)
            });
        };
        // SAFETY: the pins are boxed in the value's `Held`, which stays in
        // the table while the stand-in is borrowed, as it is for this call,
        // and they are let go only by a `&mut self` call (`take_kept`) or
        // the stand-in's drop, which rustc allows only once every borrow of
        // the stand-in, and so of what this call gives back, has ended. A
        // marked value's pins (`Kept::borrowed`) are let go only by a call
        // that takes the value `&mut`, which rustc allows only once every
        // borrow of it, and so of what this call gives back, has ended.
        let pins = unsafe { pins.as_ref() };
        let mut region = crate::scope::Region::anchored(pins);
        let anchored = Anchored {
            region: region.nonce(),
            pins,
        };
        let reply = crate::scope::open_anchored(&mut region, || {
            self.finish(call, made, values, Some(anchored), natives)
        });
        static SITE: crate::scope::Site = crate::scope::Site::new();
        region.keep(&SITE);
        reply
    }

    /// The rest of `call` once its arguments are read: invoke the function,
    /// give back its receivers and write its reply. A `&self` call on a
    /// native trait object runs `anchored`, and its result may borrow the
    /// value.
    fn finish(
        &self,
        call: &Call<'_>,
        made: Made<'_>,
        mut values: Vec<Dynamic>,
        anchored: Option<Anchored<'_>>,
        natives: &mut Natives,
    ) -> Result<Vec<u8>, String> {
        let Made {
            item,
            held,
            references,
            mutable,
            gives,
            own,
        } = made;
        let path = call.path;
        let model = call.model;
        // The texts a `&str` that borrows a marked receiver may be in, as
        // `&self.0` of an `Id(String)` is: each one the receiver native
        // code wrote holds, found before the call takes it.
        let mut texts = Vec::new();
        if matches!(gives, Gives::Text)
            && anchored.is_none()
            && matches!(item.kind(), item::Kind::Method)
            && let Some(receiver) = values.first()
        {
            texts_of(receiver, &mut texts, 0);
        }
        let invoked = (call.invoke)(item, &mut values);
        // The receiver changed in place, as a method's is, goes back.
        restore(&held, &mut values);
        let mut result = invoked.map_err(|error| format!("rawr: `{path}` failed: {error}"))?;
        let copied = match item.kind() {
            item::Kind::Function => references.get(1..).unwrap_or_default(),
            item::Kind::Method => references
                .iter()
                .position(|index| *index != 0)
                .map_or(&[][..], |at| &references[at..]),
            _ => &references[..],
        };
        if !copied.is_empty() {
            let returned = result
                .into_array()
                .ok()
                .filter(|returned| returned.len() == copied.len() + 1)
                .ok_or_else(|| {
                    format!("rawr: `{path}` gave back no copies of its `&mut` arguments")
                })?;
            let mut returned = returned.into_iter();
            result = returned.next().unwrap_or(Dynamic::UNIT);
            for (index, value) in copied.iter().zip(returned) {
                values[*index] = value;
            }
        }
        let mut reply = vec![0];
        let unsent = |error: String| {
            format!("rawr: `{path}` gave back a value native code can't take: {error}")
        };
        match (gives, anchored) {
            (Gives::Text, Some(Anchored { pins, .. })) => {
                // A `&str` that borrows the receiver reads the program's own
                // text where it is, pinned with the value (#1322).
                let text = result
                    .flatten_clone()
                    .into_immutable_string()
                    .map_err(|actual| unsent(format!("a `{actual}` is no `&str`")))?;
                let text = pins.pin(text);
                reply.push(program::model::island::PINNED);
                reply.extend_from_slice(&(text.as_ptr().expose_provenance() as u64).to_le_bytes());
                reply.extend_from_slice(&(text.len() as u64).to_le_bytes());
            }
            // A `&str` a method of a marked value native code holds gives
            // back: in a text native code's value holds, where native code
            // finds it in that value, or else the program's own text, pinned
            // for as long as the runner lives, once per text (#1468).
            (Gives::Text, None) => {
                let text = result
                    .flatten_clone()
                    .into_immutable_string()
                    .map_err(|actual| unsent(format!("a `{actual}` is no `&str`")))?;
                if let Some((index, start)) = texts
                    .iter()
                    .enumerate()
                    .find_map(|(index, held)| Some((index, start_in(held, &text)?)))
                {
                    reply.push(program::model::island::TEXT_AT);
                    for count in [index, start, text.len()] {
                        reply.extend_from_slice(&(count as u64).to_le_bytes());
                    }
                } else {
                    let text = lasting_text(text);
                    reply.push(program::model::island::PINNED);
                    reply.extend_from_slice(
                        &(text.as_ptr().expose_provenance() as u64).to_le_bytes(),
                    );
                    reply.extend_from_slice(&(text.len() as u64).to_le_bytes());
                }
            }
            // A reference into a part native code lent the call where it
            // is, as `&self.0.colors` of a marked `Theme` (#1468).
            (Gives::InPlace { lasting: false }, None) => {
                let lent = lent_place(&result).map_err(unsent)?;
                reply.push(program::model::island::NATIVE);
                reply.extend_from_slice(&(natives.len() as u64).to_le_bytes());
                natives.push(lent);
            }
            // A reference to a native value is the value where it is: a
            // static's, or one the receiver holds, lent for as long as
            // `&self` is borrowed (#1442).
            (Gives::InPlace { lasting }, anchored) => {
                let pins = if lasting {
                    None
                } else {
                    Some(anchored.map(|anchored| anchored.pins).ok_or_else(|| {
                        unsent("a reference it gives borrows no receiver".to_owned())
                    })?)
                };
                let lent = in_place(&result, pins).map_err(unsent)?;
                reply.push(program::model::island::NATIVE);
                reply.extend_from_slice(&(natives.len() as u64).to_le_bytes());
                natives.push(lent);
            }
            (Gives::Unit, _) => reply.push(program::model::island::UNIT),
            (Gives::Value, _) => model
                .island_write_in(
                    &result,
                    anchored.map(|anchored| anchored.region),
                    &mut reply,
                    natives,
                    0,
                )
                .map_err(unsent)?,
        }
        // Then each `&mut` argument in argument order: a copied one's copy,
        // and for one native code lent, `()` when its place is still the
        // lent one, and else the value `*place = value` gave it, which moves
        // to native code (#1139).
        for index in &references {
            let value = &values[*index];
            let copied = *index < 64 && mutable & (1 << index) != 0;
            if !copied && value.read_lock::<crate::scope::Lent>().is_some() {
                reply.push(program::model::island::UNIT);
            } else {
                model
                    .island_write_back(value, &own, &mut reply, natives)
                    .map_err(unsent)?;
            }
        }
        Ok(reply)
    }
}

impl Calls {
    /// The answer to `asked` (`__rawr_island_trait_v1`), the program's
    /// functions run by `invoke`.
    fn ask(
        &self,
        asked: &Asked<'_>,
        model: &program::model::Model,
        invoke: &Invoke<'_>,
    ) -> Result<Vec<u8>, String> {
        let owner = asked.owner;
        let member = |name: &str| self.items.get(&(true, format!("{owner}::{name}")));
        let failed = |name: &'static str| {
            move |error: String| format!("rawr: `{owner}`'s `{name}` failed: {error}")
        };
        let mut reply = vec![0];
        if asked.op == op::DEFAULT {
            let value = match member("Default::default") {
                Some(default) => invoke(default, &mut []).map_err(failed("Default::default"))?,
                None => derived_default(model, owner)?,
            };
            reply.extend_from_slice(&keep(value.flatten()).to_le_bytes());
            return Ok(reply);
        }
        let two = matches!(asked.op, op::EQUAL | op::PARTIAL_ORDER | op::ORDER);
        let value = take_kept(asked.handle, Borrow::Shared)?;
        let other = if two {
            match take_kept(asked.other, Borrow::Shared) {
                Ok(other) => Some(other),
                Err(message) => {
                    put_kept(asked.handle, Borrow::Shared, value);
                    return Err(message);
                }
            }
        } else {
            None
        };
        let answered = Self::answer_std(asked.op, &value, other.as_ref(), model, &member, invoke);
        put_kept(asked.handle, Borrow::Shared, value);
        if let Some(other) = other {
            put_kept(asked.other, Borrow::Shared, other);
        }
        let answer = answered.map_err(|error| format!("rawr: `{owner}`: {error}"))?;
        match answer {
            Answer::Text(text) => reply.extend_from_slice(text.as_bytes()),
            Answer::Value(value) => reply.extend_from_slice(&keep(value.flatten()).to_le_bytes()),
            Answer::Bool(answer) => reply.push(u8::from(answer)),
            Answer::Number(number) => reply.extend_from_slice(&number.to_le_bytes()),
            Answer::Order(ordering) => reply.push(match ordering {
                Some(std::cmp::Ordering::Less) => 0,
                Some(std::cmp::Ordering::Equal) => 1,
                Some(std::cmp::Ordering::Greater) => 2,
                None => 3,
            }),
        }
        Ok(reply)
    }

    /// `op` of `value` (and `other`), by the program's own `member` where it
    /// wrote one and else as the derive does.
    fn answer_std<'i>(
        op: u8,
        value: &Dynamic,
        other: Option<&Dynamic>,
        model: &program::model::Model,
        member: &dyn Fn(&str) -> Option<&'i item::Value>,
        invoke: &Invoke<'_>,
    ) -> Result<Answer, String> {
        let other = || other.cloned().unwrap_or_default();
        let call = |name: &str, arguments: &mut [Dynamic]| {
            member(name).map(|function| {
                invoke(function, arguments).map_err(|error| format!("`{name}` failed: {error}"))
            })
        };
        let ordering = |order: Dynamic| -> Option<std::cmp::Ordering> {
            let order = order.as_int().ok()?;
            Some(order.cmp(&0))
        };
        Ok(match op {
            op::DEBUG => match call(
                "Debug::fmt",
                &mut [value.clone(), ImmutableString::new().into()],
            ) {
                Some(text) => Answer::Text(text?.to_string()),
                None => Answer::Text(
                    model
                        .debug_text(&value.flatten_clone())
                        .ok_or("its `Debug` can't be written yet")?,
                ),
            },
            op::DISPLAY => {
                let text = call(
                    "Display::fmt",
                    &mut [value.clone(), ImmutableString::new().into()],
                )
                .ok_or("it has no `Display`")??;
                Answer::Text(text.to_string())
            }
            op::CLONE => match call("Clone::clone", &mut [value.clone()]) {
                Some(copy) => Answer::Value(copy?),
                None => Answer::Value(value.flatten_clone()),
            },
            op::EQUAL => match call("PartialEq::eq", &mut [value.clone(), other()]) {
                Some(equal) => Answer::Bool(
                    equal?
                        .as_bool()
                        .map_err(|actual| format!("`eq` gave {actual}"))?,
                ),
                None => Answer::Bool(crate::host::value::structural_equal(
                    &value.flatten_clone(),
                    &other().flatten_clone(),
                )),
            },
            op::HASH => {
                if member("Hash::hash").is_some() {
                    return Err("its own `Hash` can't be called from native code yet".to_owned());
                }
                let mut state = std::collections::hash_map::DefaultHasher::new();
                model.island_hash(value, &mut state)?;
                Answer::Number(std::hash::Hasher::finish(&state))
            }
            op::PARTIAL_ORDER => {
                match call("PartialOrd::partial_cmp", &mut [value.clone(), other()]) {
                    Some(order) => {
                        Answer::Order(crate::option::read(&order?).flatten().and_then(ordering))
                    }
                    None => Answer::Order(model.island_order(value, &other())),
                }
            }
            op::ORDER => match call("Ord::cmp", &mut [value.clone(), other()]) {
                Some(order) => Answer::Order(ordering(order?)),
                None => Answer::Order(model.island_order(value, &other())),
            },
            _ => {
                return Err(format!(
                    "the native code asked {op}, which rawr doesn't know"
                ))
            }
        })
    }
}

/// A call whose arguments are read (`Calls::call`): its function, the
/// stand-ins' receivers it took, its `&mut` arguments and those it copies.
struct Made<'i> {
    item: &'i item::Value,
    held: Vec<(usize, u64, Borrow)>,
    references: Vec<usize>,
    mutable: u64,
    gives: Gives,
    /// The lends of what native code lent the call, as the parts of a
    /// marked value it lends (#1469).
    own: Vec<crate::scope::Nonce>,
}

/// The region a `&self` call on a native trait object runs in, and the
/// value's pins, which what its result borrows is pinned in
/// (`Calls::call`).
#[derive(Clone, Copy)]
struct Anchored<'p> {
    region: crate::scope::Nonce,
    pins: &'p crate::scope::Pins,
}

/// How a call gives its result back, as its header's flags say.
#[derive(Clone, Copy)]
enum Gives {
    /// By the wire format, each native value in it moved.
    Value,
    /// A `&str` that borrows the receiver: the program's own text, pinned
    /// with the value (#1322).
    Text,
    /// A reference to a native value, given where the value is (#1442):
    /// a static's place, which `lasting` asks for, or else one the receiver
    /// holds, lent for as long as it is borrowed.
    InPlace { lasting: bool },
    /// `()`, whatever value the body's last statement left.
    Unit,
}

/// What a stand-in's std trait gets back (`Calls::ask`).
enum Answer {
    Text(String),
    /// A new value, kept under a handle of its own.
    Value(Dynamic),
    Bool(bool),
    Number(u64),
    Order(Option<std::cmp::Ordering>),
}

/// The derived `Default` of the script type at `owner`.
fn derived_default(model: &program::model::Model, owner: &str) -> Result<Dynamic, String> {
    #[cfg(feature = "sync")]
    {
        model
            .default_by_path(owner)
            .ok_or_else(|| format!("rawr: the running program has no type `{owner}`"))?
            .map_err(|error| format!("rawr: `{owner}`'s `Default`: {error}"))
    }
    #[cfg(not(feature = "sync"))]
    {
        let _ = model;
        Err(format!(
            "rawr: `{owner}`'s derived `Default` needs the runner"
        ))
    }
}

/// End each stand-in's receiver's borrow in `values` (`put_kept`).
fn restore(held: &[(usize, u64, Borrow)], values: &mut [Dynamic]) {
    for &(index, handle, borrow) in held {
        if let Some(value) = values.get_mut(index) {
            put_kept(handle, borrow, value.take());
        }
    }
}

#[cfg(all(test, feature = "sync"))]
mod tests {
    use crate::{host, Dynamic, Engine, Shared};

    /// What native code borrowed of a marked value through anchored results
    /// goes when the value drops or moves out (#1492), and a drop while the
    /// kept values are busy, inside a change of them, waits for nothing.
    #[test]
    fn a_dropped_anchor_lets_go_of_what_was_borrowed_of_it() {
        let anchor = super::Anchor {
            address: 1492,
            ty: 7,
        };
        let pins = super::pins_at(anchor);
        // SAFETY: the table keeps the pins boxed until they are let go,
        // which nothing does before this read.
        let text = unsafe { pins.as_ref() }.pin(crate::ImmutableString::from("kept"));
        assert_eq!(text, "kept");
        let held = || super::kept(|kept| kept.borrowed.contains_key(&anchor));
        super::kept(|_| super::__rawr_island_unanchor_v1(1492, 7));
        assert!(held(), "a busy table lets go of nothing");
        super::__rawr_island_unanchor_v1(1492, 7);
        assert!(!held(), "the drop let go of the pins");
    }

    /// A value native code lent a call can't go back to native code in the
    /// result or in an argument it takes back, which would keep the borrow
    /// past the call (D558.3).
    #[test]
    fn a_lent_value_given_back_is_refused_by_the_borrow_rule() {
        let engine = Shared::new(Engine::new());
        let authority = engine
            .compile_authority("pub fn keep() {}")
            .unwrap_or_else(|error| panic!("{error}"));
        let lent = crate::scope::Lent::new(crate::scope::nonce(1), "ratatui::Frame", None, None);
        for value in [
            Dynamic::from(lent),
            Dynamic::from_array(vec![Dynamic::from(1_i64), Dynamic::from(lent)]),
        ] {
            let error = authority
                .model()
                .island_write_in(&value, None, &mut Vec::new(), &mut Vec::new(), 0)
                .unwrap_err();
            assert!(
                error.contains("keeps the `ratatui::Frame` native code lent it")
                    && error.contains("(E0515, E0521)"),
                "{error}"
            );
        }
    }

    /// A result crosses as its variant and its value (#1118), and a native
    /// value in it, as the `iced::Error` of an `iced::Result`, is moved out
    /// as the runner's [`super::moves`] moves it, its tag naming its place
    /// among the reply's natives.
    #[test]
    fn a_result_crosses_and_its_native_value_moves() {
        use crate::program::model::island::{ERR, NATIVE, OK, STR};

        #[derive(Clone)]
        struct Handle(u8);
        super::moves(|value| {
            value
                .read_lock::<Handle>()
                .map(|handle| Ok(Box::new(handle.0) as Box<dyn std::any::Any>))
        });
        let engine = Shared::new(Engine::new());
        let authority = engine
            .compile_authority("pub fn keep() {}")
            .unwrap_or_else(|error| panic!("{error}"));
        let model = authority.model();
        let mut natives = Vec::new();

        let mut out = Vec::new();
        let error = crate::result::err(Dynamic::from(crate::ImmutableString::from("no")));
        model
            .island_write_in(&error, None, &mut out, &mut natives, 0)
            .unwrap();
        let mut expected = vec![ERR, STR];
        expected.extend_from_slice(&2_u64.to_le_bytes());
        expected.extend_from_slice(b"no");
        assert_eq!(out, expected);

        let mut out = Vec::new();
        let ok = crate::result::ok(Dynamic::UNIT);
        let failed = crate::result::err(Dynamic::from(Handle(7)));
        model
            .island_write_in(&ok, None, &mut out, &mut natives, 0)
            .unwrap();
        model
            .island_write_in(&failed, None, &mut out, &mut natives, 0)
            .unwrap();
        let mut expected = vec![OK, crate::program::model::island::UNIT, ERR, NATIVE];
        expected.extend_from_slice(&0_u64.to_le_bytes());
        assert_eq!(out, expected);
        let [moved] = natives.as_slice() else {
            panic!("{} natives", natives.len());
        };
        assert_eq!(moved.downcast_ref::<u8>(), Some(&7));
    }

    /// Native code that clones or prints a script value it holds as
    /// `rawr::Value` gets the script type's own `Clone` and its derived
    /// `Debug` text (review 8).
    #[test]
    fn a_value_clones_and_prints_as_its_script_type_does() {
        let engine = Shared::new(Engine::new());
        let authority = engine
            .compile_authority(
                r#"
                struct Counter { n: i64 }
                impl Clone for Counter {
                    fn clone(&self) -> Self { Counter { n: self.n + 1 } }
                }
                #[derive(Debug, Clone)]
                struct Job { id: i64, name: String, tags: Vec<String>, next: Option<Step> }
                #[derive(Debug, Clone)]
                enum Step { Wait(i64), Done { at: i64 } }
                pub fn counter() -> Counter { Counter { n: 1 } }
                pub fn job() -> Job {
                    Job { id: 7, name: "seven".to_string(), tags: vec!["a".to_string()], next: Some(Step::Done { at: 3 }) }
                }
                pub fn count(counter: Counter) -> i64 { counter.n }
                "#,
            )
            .unwrap_or_else(|error| panic!("{error}"));
        let runtime = Shared::new(host::runtime::Registry::new());
        let call = |name: &str, arguments: &mut [Dynamic]| {
            let item = authority
                .items()
                .find(|item| item.key().name() == name)
                .unwrap()
                .clone();
            authority
                .invoke_bound(&item, arguments, runtime.clone())
                .unwrap_or_else(|error| panic!("{error}"))
        };
        let counter = call("counter", &mut []);
        let job = call("job", &mut []);
        crate::future::publish(super::shared(authority.clone(), runtime.clone()));
        let copy = crate::Value::from(counter).clone();
        assert_eq!(call("count", &mut [copy.into_inner()]).as_int().unwrap(), 2);
        assert_eq!(
            format!("{:?}", crate::Value::from(job)),
            r#"Job { id: 7, name: "seven", tags: ["a"], next: Some(Done { at: 3 }) }"#
        );
    }
}

/// A native trait object's program value is shared by `&self` calls at
/// any depth, the one value and not a copy, and taken by a `&mut self`
/// call only while none runs; a `&mut self` call beside another call is
/// rustc's borrow conflict (#1302).
#[cfg(all(test, feature = "sync"))]
#[test]
fn shared_calls_share_a_kept_value_and_a_mutable_one_takes_it() {
    let handle = keep(Dynamic::from(1_i64));
    let outer = take_kept(handle, Borrow::Shared).unwrap();
    let inner = take_kept(handle, Borrow::Shared).unwrap();
    assert!(outer.is_shared() && inner.is_shared());
    assert_eq!(inner.as_int(), Ok(1));
    let conflict = take_kept(handle, Borrow::Mutable).unwrap_err();
    assert!(conflict.starts_with("error[E0502]"), "{conflict}");
    put_kept(handle, Borrow::Shared, inner);
    put_kept(handle, Borrow::Shared, outer);
    let taken = take_kept(handle, Borrow::Mutable).unwrap();
    // The share ended, so the value comes out of its cell, not copied.
    assert!(!taken.is_shared());
    let conflict = take_kept(handle, Borrow::Shared).unwrap_err();
    assert!(conflict.starts_with("error[E0502]"), "{conflict}");
    let conflict = take_kept(handle, Borrow::Mutable).unwrap_err();
    assert!(conflict.starts_with("error[E0499]"), "{conflict}");
    // A `&mut self` call's change is there at the next call.
    drop(taken);
    put_kept(handle, Borrow::Mutable, Dynamic::from(2_i64));
    assert_eq!(take_kept(handle, Borrow::Shared).unwrap().as_int(), Ok(2));
    __rawr_island_release_v1(handle);
    assert!(take_kept(handle, Borrow::Shared).is_err());
}

/// What native code borrows of a kept value through its `&self` calls is
/// pinned with the value in place, once however often it is read, and let
/// go when the value is next taken `&mut`: a page whose label is read and
/// renamed in a loop pins one string, never one per call (#1322, #1321).
#[cfg(all(test, feature = "sync"))]
#[test]
fn a_kept_value_s_pins_stay_flat_and_go_at_its_next_mutable_take() {
    let pinned = |handle: u64| {
        // SAFETY: a `&self` share of the value is held while they're read.
        unsafe { pins_of(handle).unwrap().as_ref() }
    };
    let handle = keep(Dynamic::from(ImmutableString::from("clicks")));
    for round in 0..100 {
        let label = take_kept(handle, Borrow::Shared).unwrap();
        let text = label.flatten_clone().into_immutable_string().unwrap();
        for _ in 0..10 {
            let read = pinned(handle).pin(text.clone());
            assert!(std::ptr::eq(read, text.as_str()));
        }
        assert_eq!(pinned(handle).len(), 1);
        put_kept(handle, Borrow::Shared, label);
        let renamed = take_kept(handle, Borrow::Mutable).unwrap();
        assert!(pinned(handle).is_empty());
        drop(renamed);
        let name = ImmutableString::from(format!("name {round}"));
        put_kept(handle, Borrow::Mutable, Dynamic::from(name));
    }
    __rawr_island_release_v1(handle);
}

/// A generation compiled on the protocol thread is admitted on the thread
/// that owns the program (D553.9, check 2): what a compiled program holds
/// moves between threads.
#[cfg(all(test, feature = "sync"))]
#[test]
fn a_compiled_program_moves_to_the_owner_thread() {
    fn moves<T: Send>() {}
    moves::<crate::Shared<crate::Engine>>();
    moves::<crate::AST>();
    moves::<crate::reload::Candidate>();
    moves::<std::sync::Arc<crate::reload::Generation>>();
    moves::<crate::host::facade::Definition>();
}

/// What of `main` a running program compares: its text without comments
/// or layout, strings and characters as written, up to its closing brace.
#[cfg(all(test, feature = "sync"))]
#[test]
fn main_text_ignores_layout_and_comments_and_stops_at_its_brace() {
    let one = "// rawr\nuse x::y;\n\nfn main() {\n    // a comment\n    let s = \"}\";\n    let c = '}';\n    run(|t| { t + 1 }, 40);\n}\n\nfn step() {}\n";
    let moved = "// rawr\nuse x::y;\nfn helper() -> i64 { 2 }\n\nfn  main()  {\n    let s = \"}\"; /* note */\n    let c = '}';\n    run(|t| {\n        t + 1\n    }, 40);\n}\n";
    let edited = one.replace("40", "50");
    let text = main_text(one).unwrap();
    assert!(text.starts_with("fn main() {"), "{text}");
    assert!(text.ends_with("40); }"), "{text}");
    assert_eq!(main_text(moved), main_text(one));
    assert_ne!(main_text(&edited), main_text(one));
    assert_eq!(main_text("fn domain() {}"), None);
}
