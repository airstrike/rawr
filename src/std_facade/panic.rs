//! std's `panic::Location` for Rawr code: Rust's own value, which a crate
//! takes and gives as itself, as log's `__private_api::loc()` gives one
//! and its `log` takes it back (#1560).
//!
//! A crate's `#[track_caller]` function a program calls runs from the
//! runner's adapter, so the location it sees is the adapter's call. That
//! call stands for the program's own: a location crossing out of a call
//! that names the adapter's file is the program's call site
//! ([`calling`]), as rustc gives the caller's location. std makes its
//! locations only where it compiles a call, so rawr makes one for each
//! site of the program's it stands for (`at`).

#[cfg(feature = "sync")]
use std::cell::Cell;
#[cfg(feature = "sync")]
use std::collections::{HashMap, HashSet};
use std::panic::Location;
#[cfg(feature = "sync")]
use std::ptr::NonNull;
#[cfg(feature = "sync")]
use std::sync::{Mutex, PoisonError};

use super::held::{self, Held};
use crate::{capability, Dynamic, EvalAltResult, ImmutableString, Position};

/// The types the module holds.
pub(super) const HELD: [Held; 1] = [Held::Value(held::Value {
    std: "std::panic::Location",
    parameters: &[],
    lent: &[],
    also: &[],
    register: |builder, path, parameters, capabilities| {
        crate::host::value::register_ordering::<Location<'static>>();
        builder
            .value_type::<Location<'static>>(path, parameters, capabilities)
            .map(drop)
    },
    crosses: true,
    runs: &[
        capability::Kind::Debug,
        capability::Kind::Clone,
        capability::Kind::Copy,
        capability::Kind::PartialEq,
        capability::Kind::Eq,
        capability::Kind::PartialOrd,
        capability::Kind::Ord,
        capability::Kind::Send,
        capability::Kind::Static,
    ],
})];

/// A site of the program's own code, where rustc places a call's
/// location: its file as Cargo names it, its line and its column.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "rust-import", derive(serde::Deserialize, serde::Serialize))]
pub(crate) struct Site {
    file: ImmutableString,
    line: u32,
    column: u32,
}

impl Site {
    pub(crate) fn new(file: impl Into<ImmutableString>, line: u32, column: u32) -> Self {
        Self {
            file: file.into(),
            line,
            column,
        }
    }
}

#[cfg(feature = "sync")]
thread_local! {
    /// The location of the program's call running on this thread, which a
    /// location given from the adapter's call stands for.
    static CALLER: Cell<Option<Location<'static>>> = const { Cell::new(None) };
}

/// Run `call`, a call the program makes at `site`, which a location the
/// call gives from the adapter's own call stands for.
#[cfg(feature = "sync")]
pub(crate) fn calling<R>(site: Option<&Site>, call: impl FnOnce() -> R) -> R {
    let Some(caller) = site.and_then(|site| at(&site.file, site.line, site.column)) else {
        return call();
    };
    /// The caller before, back when the call ends, even by a panic.
    struct Restored(Option<Location<'static>>);
    impl Drop for Restored {
        fn drop(&mut self) {
            CALLER.set(self.0);
        }
    }
    let _restored = Restored(CALLER.replace(Some(caller)));
    call()
}

/// Run `call`: only native code a runner builds, with rawr's `sync`,
/// gives a location.
#[cfg(not(feature = "sync"))]
pub(crate) fn calling<R>(_site: Option<&Site>, call: impl FnOnce() -> R) -> R {
    call()
}

/// `location`, given from native code at `here`: the program's caller when
/// it names `here`'s file, the adapter's, and isn't one rawr made for the
/// program's own code.
#[cfg(feature = "sync")]
fn stood_for(location: Location<'static>, here: &Location<'_>) -> Location<'static> {
    if location.file() != here.file() || made(location.file()) {
        return location;
    }
    CALLER.get().unwrap_or(location)
}

/// The file names of the locations rawr made, each followed by the NUL
/// byte `Location::file_as_c_str` reads, by their text.
#[cfg(feature = "sync")]
static FILES: Mutex<Option<HashMap<Box<str>, &'static str>>> = Mutex::new(None);

/// The locations a crate borrows for good, one per location.
#[cfg(feature = "sync")]
static PLACED: Mutex<Option<HashSet<&'static Location<'static>>>> = Mutex::new(None);

/// Whether `file` is the name of a location rawr made.
#[cfg(feature = "sync")]
fn made(file: &str) -> bool {
    FILES
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .and_then(|files| files.get(file))
        .is_some_and(|kept| std::ptr::eq(*kept, file))
}

/// The file name `file`, kept for good with its NUL byte; `None` for one
/// that holds a NUL, which a location's name never does.
#[cfg(feature = "sync")]
fn kept(file: &str) -> Option<&'static str> {
    if file.contains('\0') {
        return None;
    }
    let mut files = FILES.lock().unwrap_or_else(PoisonError::into_inner);
    let files = files.get_or_insert_with(HashMap::new);
    if let Some(kept) = files.get(file) {
        return Some(kept);
    }
    let terminated: &'static str = Box::leak(format!("{file}\0").into_boxed_str());
    let kept = &terminated[..file.len()];
    files.insert(file.into(), kept);
    Some(kept)
}

/// A `&'static` borrow of `location`, as a crate's `&'static Location`
/// takes it: the one kept for every equal location.
#[cfg(feature = "sync")]
fn placed(location: Location<'static>) -> &'static Location<'static> {
    let mut placed = PLACED.lock().unwrap_or_else(PoisonError::into_inner);
    let placed = placed.get_or_insert_with(HashSet::new);
    if let Some(kept) = placed.get(&location) {
        return kept;
    }
    let kept: &'static Location<'static> = Box::leak(Box::new(location));
    placed.insert(kept);
    kept
}

/// std's location's parts, in a layout of rawr's own.
#[cfg(feature = "sync")]
#[repr(C)]
struct Parts {
    file: NonNull<str>,
    line: u32,
    column: u32,
}

/// The location at `line` and `column` of `file`. std has no constructor
/// for one, so it is std's own value with these parts, laid out as a
/// location std made here lays out its own; `None` when that layout is not
/// [`Parts`]'.
#[cfg(feature = "sync")]
fn at(file: &str, line: u32, column: u32) -> Option<Location<'static>> {
    let probe = Location::caller();
    // SAFETY: `Parts` is as large as a location (`transmute` below checks
    // it at compile time), and a location's parts fill it, so this reads
    // the probe's own initialized bytes.
    let read = unsafe { std::mem::transmute_copy::<Location<'static>, Parts>(probe) };
    let same = probe.line() != probe.column()
        && std::ptr::eq(read.file.as_ptr(), probe.file())
        && read.line == probe.line()
        && read.column == probe.column();
    if !same {
        return None;
    }
    let file = kept(file)?;
    let parts = Parts {
        file: NonNull::from(file),
        line,
        column,
    };
    // SAFETY: a location's parts are at the offsets `Parts` has, as the
    // probe shows, and `file` lives for good with the NUL byte after it
    // that `Location::file_as_c_str` reads, and holds no other.
    Some(unsafe { std::mem::transmute::<Parts, Location<'static>>(parts) })
}

/// A location the program's code gives native code is its own value, and
/// one native code gives from the adapter's call is the program's call it
/// stands for.
#[cfg(feature = "sync")]
impl crate::crossing::Std for Location<'static> {
    type Lent = &'static Location<'static>;

    /// `#[track_caller]`: `Location::caller()` is the adapter's call that
    /// gives the location, in the runner's file.
    #[track_caller]
    fn give(self) -> Dynamic {
        Dynamic::from(stood_for(self, Location::caller()))
    }

    fn take(value: Dynamic) -> Option<Self> {
        value.try_cast::<Self>()
    }

    fn lend(value: Dynamic, _arena: &mut dyn std::any::Any) -> Option<Self::Lent> {
        value.try_cast::<Self>().map(placed)
    }
}

/// Register `Debug` and `Display` for the type.
pub(crate) fn register_debug() {
    crate::host::debug::register::<Location<'static>>();
    crate::host::display::register::<Location<'static>>();
}

fn read(value: &Dynamic) -> Result<Location<'static>, Box<EvalAltResult>> {
    value
        .read_lock::<Location<'static>>()
        .map(|location| *location)
        .ok_or_else(|| {
            EvalAltResult::ErrorRuntime(
                format!("expected `Location`, found `{}`", value.type_name()).into(),
                Position::NONE,
            )
            .into()
        })
}

/// Bind the type's items.
pub(super) fn bind(binds: &mut impl super::Bind) {
    type Callback = fn(&mut [&mut Dynamic]) -> Result<Dynamic, Box<EvalAltResult>>;
    let mut bind = |path: &str, callback: Callback| binds.plain(path, callback);
    bind("panic::Location::file", |args| {
        Ok(Dynamic::from(ImmutableString::from(read(args[0])?.file())))
    });
    bind("panic::Location::line", |args| {
        Ok(Dynamic::from_int(crate::INT::from(read(args[0])?.line())))
    });
    bind("panic::Location::column", |args| {
        Ok(Dynamic::from_int(crate::INT::from(read(args[0])?.column())))
    });
}
