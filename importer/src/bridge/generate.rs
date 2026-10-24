//! Normalized standalone runner source generation.

use std::io::{self, Write};

use sha2::{Digest as _, Sha256};
use toml_edit::{value, DocumentMut, Item};

use super::request;
use crate::classify::representability::convert;
use crate::classify::representability::ownership::Receiver;
use crate::surface::adapter::{EntryIdentity, Operation, Plan};

pub(crate) mod cached;

const MANIFEST_PATH: &str = "Cargo.toml";
const LOCK_PATH: &str = "Cargo.lock";
const BUILD_PATH: &str = "build.rs";
const SOURCE_PATH: &str = "src/main.rs";
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
struct Adapter { identity: [u8; 32], signature: [u8; 32], selector: [u8; 32], instance: [u8; 32], bound: bool, root: &'static str, witness: u8, arguments: u32, invoke: Invoke }
type Invoke = fn(&mut Arguments<'_, '_, '_>) -> Result<Dynamic, Error>;
type Project = fn(
    Box<dyn Any>,
    &__rawr_internal::host::runtime::Call<'_>,
    &__rawr_internal::host::runtime::Witness,
) -> Result<Dynamic, Box<__rawr_internal::EvalAltResult>>;
struct Arguments<'slice, 'value, 'arena> { values: &'slice mut [&'value mut Dynamic], arena: &'arena mut Arena }

#[derive(Clone, Copy)]
struct NativeHandle { epoch: [u8; 32], slot: u32, generation: u64 }
struct NativeSlot { generation: u64, value: Option<Box<dyn Any>>, debug: Option<DebugText>, display: Option<DebugText>, clone: Option<CloneValue>, relations: Relations, witness: Option<__rawr_internal::host::runtime::Witness>, project: Option<Project>, mapper: Option<[u8; 16]> }

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
            $arena.set_display(*handle, display);
            $arena.set_relations(*handle, relations);
        }
        handle
    }};
}

/// `to_string()` and `{}` of an imported value print its Rust `Display`
/// text.
impl std::fmt::Display for NativeHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = NATIVE_ARENA.with(|arena| {
            let arena = arena.try_borrow().ok()?;
            arena.as_ref()?.display(*self)
        });
        match text {
            Some(text) => f.write_str(&text),
            None => f.write_str("NativeHandle"),
        }
    }
}

/// `{:?}` of an imported value prints its Rust `Debug` text.
impl std::fmt::Debug for NativeHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = NATIVE_ARENA.with(|arena| {
            let arena = arena.try_borrow().ok()?;
            arena.as_ref()?.debug(*self)
        });
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
        }
    }
}

/// Rawr's `i64` as the Rust integer `T`, checked as `T::try_from` checks.
#[allow(dead_code)]
fn int_from<T: TryFrom<i64>>(value: i64) -> Result<T, Error> {
    T::try_from(value).map_err(|_| Error::OutOfRange {
        value: value.to_string(),
        ty: std::any::type_name::<T>(),
    })
}

/// The Rust integer `value` as Rawr's `i64`. A `u64` above `i64::MAX` is
/// an error.
#[allow(dead_code)]
fn int_into<T: TryInto<i64> + Copy + std::fmt::Display>(value: T) -> Result<i64, Error> {
    value.try_into().map_err(|_| Error::OutOfRange {
        value: value.to_string(),
        ty: "i64",
    })
}

/// Rawr's inclusive integer range with each end checked into `T`.
#[allow(dead_code)]
fn range_from<T: TryFrom<i64>>(range: std::ops::RangeInclusive<i64>) -> Result<std::ops::RangeInclusive<T>, Error> {
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
                self.slots.push(NativeSlot { generation: 1, value: None, debug: None, display: None, clone: None, relations: Relations::default(), witness: None, project: None, mapper: None });
                slot
            }
        };
        let entry = self.slots.get_mut(slot as usize).ok_or(Error::StaleHandle)?;
        if entry.value.is_some() { return Err(Error::StaleHandle); }
        entry.value = Some(Box::new(value));
        entry.debug = None;
        entry.display = None;
        entry.clone = None;
        entry.relations = Relations::default();
        entry.witness = None;
        entry.project = None;
        entry.mapper = None;
        Ok(NativeHandle { epoch: self.epoch, slot, generation: entry.generation })
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
        (entry.debug?)(entry.value.as_deref()?)
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
        (entry.display?)(entry.value.as_deref()?)
    }

    fn insert_projectable<T: 'static>(&mut self, value: T, project: Project, mapper: [u8; 16]) -> Result<NativeHandle, Error> {
        let handle = self.insert(value)?;
        let entry = self.slots.get_mut(handle.slot as usize).ok_or(Error::StaleHandle)?;
        entry.project = Some(project);
        entry.mapper = Some(mapper);
        Ok(handle)
    }

    fn index<T: 'static>(&self, handle: NativeHandle) -> Result<usize, Error> {
        if handle.epoch != self.epoch { return Err(Error::StaleHandle); }
        let index = handle.slot as usize;
        let entry = self.slots.get(index).ok_or(Error::StaleHandle)?;
        if entry.generation != handle.generation || entry.value.is_none() {
            return Err(Error::StaleHandle);
        }
        if !entry.value.as_ref().is_some_and(|value| value.is::<T>()) {
            return Err(Error::TypeMismatch);
        }
        Ok(index)
    }

    fn take<T: 'static>(&mut self, handle: NativeHandle) -> Result<T, Error> {
        let index = self.index::<T>(handle)?;
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
        (handle.epoch == self.epoch && handle.generation == entry.generation && entry.value.is_some())
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
        if handle.epoch != self.epoch || handle.generation != entry.generation || entry.value.is_none() {
            return Err(Error::StaleHandle);
        }
        entry.witness = Some(witness);
        Ok(())
    }

    fn shared<T: 'static, R>(
        &self,
        handle: NativeHandle,
        call: impl FnOnce(&T) -> R,
    ) -> Result<R, Error> {
        let index = self.index::<T>(handle)?;
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
        let index = self.index::<T>(handle)?;
        let value = self.slots[index]
            .value
            .as_mut()
            .and_then(|value| value.downcast_mut::<T>())
            .ok_or(Error::TypeMismatch)?;
        Ok(call(value))
    }
}

fn install_arena(epoch: [u8; 32]) {
    __rawr_internal::host::debug::register::<NativeHandle>();
    __rawr_internal::host::display::register::<NativeHandle>();
    NATIVE_ARENA.with(|arena| *arena.borrow_mut() = Some(Arena::new(epoch)));
}

fn with_arena<T>(call: impl FnOnce(&mut Arena) -> Result<T, Error>) -> Result<T, Error> {
    NATIVE_ARENA.with(|arena| {
        let mut arena = arena.try_borrow_mut().map_err(|_| Error::Stale)?;
        call(arena.as_mut().ok_or(Error::Stale)?)
    })
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
        if let (Some(handle), Some(witness)) = (output.read_lock::<NativeHandle>().map(|value| *value), witness) {
            arguments.arena.attest(handle, witness.clone())?;
        }
        Ok(output)
    })
    .map_err(|error| format!("{error:?}"))
}

fn take_primitive<T: 'static>(arguments: &mut Arguments<'_, '_, '_>, index: usize) -> Result<T, Error> {
    let value = arguments.values.get_mut(index).ok_or(Error::MissingArgument)?;
    (**value).take().try_cast_result::<T>().map_err(|_| Error::TypeMismatch)
}

#[allow(dead_code)]
fn take_int<T: TryFrom<i64>>(arguments: &mut Arguments<'_, '_, '_>, index: usize) -> Result<T, Error> {
    int_from(take_primitive::<i64>(arguments, index)?)
}

#[allow(dead_code)]
fn take_f32(arguments: &mut Arguments<'_, '_, '_>, index: usize) -> Result<f32, Error> {
    take_primitive::<f64>(arguments, index).map(|value| value as f32)
}

/// An inclusive range whose integer type Rust infers from the call.
#[allow(dead_code)]
fn take_range<T: TryFrom<i64>>(arguments: &mut Arguments<'_, '_, '_>, index: usize) -> Result<std::ops::RangeInclusive<T>, Error> {
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

fn take_native<T: 'static>(arguments: &mut Arguments<'_, '_, '_>, index: usize) -> Result<T, Error> {
    let handle = native_handle(arguments, index, true)?;
    arguments.arena.take(handle)
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
            Dynamic::from(NativeHandle { epoch, slot, generation })
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

fn runtime_invoke(
    adapter: Adapter,
    call: &__rawr_internal::host::runtime::Call<'_>,
    values: &mut [&mut Dynamic],
) -> Result<Dynamic, Box<__rawr_internal::EvalAltResult>> {
    with_arena(|arena| {
        let receiver = values
            .first()
            .and_then(|value| value.read_lock::<NativeHandle>().map(|value| *value))
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
                    .ok_or(Error::Stale)?;
                pending.witness(witness).ok_or(Error::Stale)?
            }
            Err(output) => output,
        };
        if let Some(handle) = output.read_lock::<NativeHandle>().map(|value| *value) {
            let witness = match adapter.witness {
                1 => call.output_witness().ok(),
                2 => receiver
                    .as_ref()
                    .and_then(|witness| call.transform_native_witness(witness).ok()),
                _ => None,
            };
            if let Some(witness) = witness {
                arguments.arena.attest(handle, witness)?;
            }
        } else if let Ok(witness) = call.output_witness() {
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
    if let Some((inner, index)) = inner
        && let Some(handle) = inner.read_lock::<NativeHandle>().map(|value| *value)
        && let Some(witness) = witness.argument_witness(index)
    {
        arena.attest(handle, witness)?;
    } else if let Some((inner, index)) = inner_tuple(value)
        && let Some(tuple) = witness.argument_witness(index)
    {
        // A variant's fields, as its matcher gives them.
        for (member, element) in inner.iter().enumerate() {
            if let Some(handle) = element.read_lock::<NativeHandle>().map(|value| *value)
                && let Some(witness) = tuple.argument_witness(member)
            {
                arena.attest(handle, witness)?;
            }
        }
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
        match element.read_lock::<NativeHandle>().map(|value| *value) {
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
    NATIVE_ARENA.with(|arena| {
        let mut arena = arena
            .try_borrow_mut()
            .map_err(|_| runtime_error(Error::Stale))?;
        arena
            .as_mut()
            .ok_or_else(|| runtime_error(Error::Stale))?
            .project(handle, call)
    })
}

fn native_attestation(
    value: &Dynamic,
    expected: &__rawr_internal::host::runtime::Instantiation<'_>,
) -> Option<__rawr_internal::host::runtime::Attested> {
    let handle = value.read_lock::<NativeHandle>().map(|value| *value)?;
    NATIVE_ARENA.with(|arena| {
        let arena = arena.try_borrow().ok()?;
        expected.attest_native_witness(arena.as_ref()?.witness(handle))
    })
}

/// `==` of two imported values by their Rust type's `PartialEq`.
fn native_equal(lhs: &Dynamic, rhs: &Dynamic) -> Option<bool> {
    let lhs = lhs.read_lock::<NativeHandle>().map(|value| *value)?;
    let rhs = rhs.read_lock::<NativeHandle>().map(|value| *value)?;
    NATIVE_ARENA.with(|arena| arena.try_borrow().ok()?.as_ref()?.equal(lhs, rhs))
}

/// The order of two imported values that key a hashed collection.
fn native_compare(lhs: &Dynamic, rhs: &Dynamic) -> Option<std::cmp::Ordering> {
    let lhs = lhs.read_lock::<NativeHandle>().map(|value| *value)?;
    let rhs = rhs.read_lock::<NativeHandle>().map(|value| *value)?;
    NATIVE_ARENA.with(|arena| arena.try_borrow().ok()?.as_ref()?.compare(lhs, rhs))
}

fn imported_runtime(
    engine: &__rawr_internal::Engine,
    definitions: &[__rawr_internal::host::facade::Definition],
) -> Result<__rawr_internal::host::runtime::Registry, __rawr_internal::host::runtime::Error> {
    __rawr_internal::host::value::register_relations::<NativeHandle>(native_equal, native_compare);
    let mut registry = __rawr_internal::host::runtime::Registry::new();
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
    _facades: Box<[__rawr_internal::host::facade::Definition]>,
    observation: __rawr_internal::package::import::runner::account::Observation,
}

fn program_account(program: &Option<Program>) -> Option<__rawr_internal::package::import::runner::account::Snapshot> {
    program.as_ref().and_then(|program| program.observation.snapshot())
}

fn observed_account(observation: &Option<__rawr_internal::package::import::runner::account::Observation>) -> Option<__rawr_internal::package::import::runner::account::Snapshot> {
    observation.as_ref().and_then(|observation| observation.snapshot())
}

fn retire_program(program: Option<Program>) -> Option<__rawr_internal::package::import::runner::account::Observation> {
    program.map(|program| program.observation)
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
    Ok(Some(Program { engine, ast: Some(ast), candidate, application_contract, _facades: facades, observation }))
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
        assert_eq!(arena.shared::<NonClone, _>(handle, |value| value.value).expect("shared"), 2);
        arena.mutable::<NonClone, _>(handle, |value| value.value += 1).expect("mutable");
        assert!(matches!(arena.shared::<String, _>(handle, |_| ()), Err(Error::TypeMismatch)));
        assert_eq!(arena.take::<NonClone>(handle).expect("owned").value, 3);
        assert!(matches!(arena.take::<NonClone>(handle), Err(Error::StaleHandle)));
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
        assert_eq!(arena.take::<NonClone>(handle).expect("last take").value, 1);
        let next = arena.insert(NonClone { value: 2 }).expect("new slot");
        assert_ne!(next.slot, handle.slot);
    }
}
"#;
const NEUTRAL_PROFILE_SOURCE: &[u8] = br#"
mod __rawr_profile {
    #[derive(Clone)]
    pub struct ApplicationContract;

    pub fn compile_initial(
        engine: &__rawr_internal::Engine,
        snapshot: &__rawr_internal::package::snapshot::Snapshot,
        _contract: &ApplicationContract,
        _expected: &[u8; 32],
    ) -> std::io::Result<(Option<__rawr_internal::reload::Candidate>, __rawr_internal::AST)> {
        engine.compile_package_snapshot(snapshot)
            .map(|ast| (None, ast))
            .map_err(|error| std::io::Error::other(error.to_string()))
    }

    pub fn compile_generation(
        _engine: &__rawr_internal::Engine,
        _snapshot: &__rawr_internal::package::snapshot::Snapshot,
        _contract: &ApplicationContract,
        _expected: &[u8; 32],
    ) -> std::io::Result<Option<__rawr_internal::reload::Candidate>> {
        Ok(None)
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

struct PendingProgram {
    key: [u8; 32],
    nonce: [u8; 16],
    program: Option<Program>,
}

fn run_protocol() -> std::io::Result<()> {
    let socket = std::env::var_os("RAWR_PROTOCOL_SOCKET").ok_or_else(|| std::io::Error::other("protocol socket"))?;
    let capability = decode_hex(&std::env::var("RAWR_PROTOCOL_CAPABILITY").map_err(|_| std::io::Error::other("protocol capability"))?)?;
    let mut key = decode_hex(&std::env::var("RAWR_GENERATION_KEY").map_err(|_| std::io::Error::other("generation key"))?)?;
    install_arena(key);
    let mut program: Option<Program> = None;
    let mut pending: Option<PendingProgram> = None;
    let mut prepared = false;
    let mut admitted = false;
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
                let mut observation = None;
                let compiled = compile_program(&payload[112..], Compilation::Replacement(&program), &mut progress, &mut observation);
                drop(progress);
                let candidate = account_result(compiled, &mut stream, tag, &payload,
                    [program_account(&program), observed_account(&observation)])?;
                let mut response = Vec::with_capacity(48);
                response.extend_from_slice(&next);
                response.extend_from_slice(&nonce);
                pending = Some(PendingProgram { key: next, nonce, program: candidate });
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

pub(crate) struct Expected {
    aggregate: [u8; 32],
    files: Vec<(Box<str>, ([u8; 32], usize))>,
    source_map: ([u8; 32], usize),
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
    let mut entries = Hashing(Sha256::new());
    serde_json::to_writer(&mut entries, &request.entries).map_err(|_| request::Error::Encoding)?;
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
    let manifest = runner_manifest(harness.manifest())?;
    let lock = runner_lock(harness.lock())?;
    let mut sources = Vec::new();
    let mut spans = Vec::new();
    spans
        .try_reserve_exact(request.plans.len())
        .map_err(|_| request::Error::Allocation)?;
    render_source(
        &request.plans,
        request.fingerprint,
        profile,
        request.compile_snapshot(),
        request.facade_plan(),
        &mut sources,
        &mut spans,
    )?;
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
    let mut files: Vec<(Box<str>, ([u8; 32], usize))> = vec![
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

fn runner_manifest(bytes: &[u8]) -> Result<Vec<u8>, request::Error> {
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
    share_harness_units(&mut document)?;
    Ok(normalize(document.to_string().as_bytes()))
}

/// The runner is built from the harness's dependency set, for the harness's
/// target, in the same build directory: every dependency unit, the edited
/// crates included, is the one the harness build already compiled (the harness
/// builds them non-incremental). Only the runner crate itself is incremental.
fn share_harness_units(document: &mut DocumentMut) -> Result<(), request::Error> {
    let mut item = Some(document.as_item());
    for key in ["profile", "dev", "package"] {
        item = item.and_then(|item| item.get(key));
        if item.is_some_and(|item| !item.is_table()) {
            return Err(request::Error::Manifest {
                reason: "final-form harness profile is not a table".into(),
            });
        }
    }
    document["profile"]["dev"]["incremental"] = value(false);
    document["profile"]["dev"]["package"]["rawr-generated-runner"]["incremental"] = value(true);
    Ok(())
}

fn runner_lock(bytes: &[u8]) -> Result<Vec<u8>, request::Error> {
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

fn render_source(
    plans: &[Plan],
    surface: crate::surface::Fingerprint,
    profile: &crate::profile::Proof,
    compile_snapshot: bool,
    facade_plan: Option<&[u8]>,
    sources: &mut Vec<(Box<str>, Vec<u8>)>,
    spans: &mut Vec<(EntryIdentity, Box<str>, std::ops::Range<u32>)>,
) -> Result<(), request::Error> {
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
    let _type_paths = TypePaths::install(facade.as_ref());
    if let Some(plan) = &facade {
        for facade in plan.facades() {
            for path in facade.traits() {
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
            b"mod rawr_surface;\nuse rawr_surface::{COMPILED_SURFACE, COMPILE_SNAPSHOT, PROFILE_FINGERPRINT};\n#[allow(unused_imports)]\nuse rawr_surface::*;\n",
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
        writeln!(output, ", {}, {kind}),", arguments(plan)?)
            .map_err(|_| request::Error::Encoding)?;
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
                ", 1, __rawr_internal::package::import::runner::StructuralKind::Match),"
            )
            .map_err(|_| request::Error::Encoding)?;
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
                ", bound: {}, root: {:?}, witness: {}",
                plan.selector().is_some(),
                plan.export().root_alias(),
                witness_mode(plan)
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
                    ", bound: false, root: {:?}, witness: 0, arguments: 1, invoke: {} }},",
                    plan.export().root_alias(),
                    matcher_name(plan)
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
            let end = span_offset(&file)?;
            spans.push((plan.identity(), path.clone(), start..end));
        }
        render_module_execution_tests(plans, indices, primitive, builder, &mut file)?;
        sources.push((path, file));
    }
    sources.push((SURFACE_PATH.into(), stamps));
    Ok(())
}

/// The key a call of this adapter's instance carries, or zeros for an
/// item that is not generic.
fn instance_key(plan: &Plan) -> [u8; 32] {
    match plan.operation() {
        Operation::Function { substitutions, .. } | Operation::Method { substitutions, .. }
            if !substitutions.is_empty() =>
        {
            crate::surface::instance::key(substitutions)
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

fn witness_mode(plan: &Plan) -> u8 {
    match plan.operation() {
        Operation::StructConstructor { .. }
        | Operation::EnumVariant { .. }
        | Operation::Function { .. }
        | Operation::Constant { .. } => 1,
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
    writeln!(
        output,
        "    let handle = native_handle(arguments, 0, false)?;\n    #[allow(unreachable_patterns)]\n    let output = arguments.arena.shared::<{path}, _>(handle, |input| match input {{ {pattern} => Some({cloned}), _ => None }})?;\n    let Some(fields) = output else {{ return Ok(__rawr_internal::option::none()); }};"
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
    writeln!(
        output,
        "fn {}(arguments: &mut Arguments<'_, '_, '_>) -> Result<Dynamic, Error> {{",
        adapter_name(plan)
    )
    .map_err(|_| request::Error::Encoding)?;
    output
        .write_all(b"    let _ = &mut *arguments;\n")
        .map_err(|_| request::Error::Encoding)?;
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
            write!(output, "    let output = {path}").map_err(|_| request::Error::Encoding)?;
            render_constructor(*form, fields, output)?;
            output
                .write_all(b";\n    insert_native!(arguments.arena, output).map(Dynamic::from)\n")
                .map_err(|_| request::Error::Encoding)?;
        }
        Operation::StructField { field, .. } => {
            if copy_scalar(field.ty()) {
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
        } => {
            if !mappers.is_empty() && replay.is_none() {
                return Err(request::Error::Drift {
                    field: "profile event mapper invocation lacks replay projection",
                });
            }
            if replay.is_some() {
                render_function_arguments(
                    signature,
                    substitutions,
                    functions,
                    mappers,
                    0,
                    Some(profile),
                    output,
                )?;
            } else {
                render_function_arguments(
                    signature,
                    substitutions,
                    functions,
                    mappers,
                    0,
                    None,
                    output,
                )?;
            }
            if let Some(replay) = replay {
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
                output
                    .write_all(if lazy {
                        b"    let output = __rawr_profile_host::task::lazy(move || ".as_slice()
                    } else {
                        b"    let output = ".as_slice()
                    })
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
                function_argument_list(signature, substitutions, functions, mappers, None, output)?;
                output
                    .write_all(if lazy { b"));\n".as_slice() } else { b");\n".as_slice() })
                    .map_err(|_| request::Error::Encoding)?;
                let returned = substitute(signature.output(), signature, substitutions)?;
                if let Some((sipped, progress)) = sipper_output(returned) {
                    render_give_sipper(sipped, progress, "output", output)?;
                } else if let Some(element) = facade_task_output(returned, facade) {
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
        } => {
            if matches!(receiver, Receiver::Owned) && replay.is_some() {
                render_arguments_with_profile(signature, substitutions, 1, profile, output)?;
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
            } else {
                match receiver {
                    Receiver::Owned => {
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
                        write!(output, "    let output = receiver.{}", plan.name())
                            .map_err(|_| request::Error::Encoding)?;
                    }
                    Receiver::Shared | Receiver::Mutable => {
                        writeln!(
                            output,
                            "    let receiver = native_handle(arguments, 0, false)?;"
                        )
                        .map_err(|_| request::Error::Encoding)?;
                        write!(
                            output,
                            "    let output = arguments.arena.{}::<",
                            if matches!(receiver, Receiver::Mutable) {
                                "mutable"
                            } else {
                                "shared"
                            }
                        )
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
                        write!(output, ", _>(receiver, |receiver| receiver.{}", plan.name())
                            .map_err(|_| request::Error::Encoding)?;
                    }
                }
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
                if matches!(receiver, Receiver::Owned) {
                    output
                        .write_all(b");\n")
                        .map_err(|_| request::Error::Encoding)?;
                } else {
                    output
                        .write_all(b"))?;\n")
                        .map_err(|_| request::Error::Encoding)?;
                }
                // A method that takes `self` by value may give a Rust
                // future, as `Response::text` does.
                let returned = substitute(signature.output(), signature, substitutions)?;
                match facade_task_output(returned, facade) {
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
    output
        .write_all(b"}\n\n")
        .map_err(|_| request::Error::Encoding)
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
        Operation::Method { signature, .. } => {
            signature
                .inputs()
                .len()
                .checked_add(1)
                .ok_or(request::Error::Limit {
                    resource: "adapter argument cardinality",
                    limit: u32::MAX as usize,
                    actual: usize::MAX,
                })?
        }
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
                        .write_all(b"    insert_native!(arguments.arena, output).map(Dynamic::from)\n")
                        .map_err(|_| request::Error::Encoding)?;
                }
            }
            Operation::Method {
                receiver: Receiver::Owned,
                signature,
                substitutions,
                constants,
                container_parameters,
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
    path
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
            argument_list(fields.len(), 0, output)?;
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
                write!(output, "{}: argument_{index}", field.name())
                    .map_err(|_| request::Error::Encoding)?;
            }
            output
                .write_all(b" }")
                .map_err(|_| request::Error::Encoding)
        }
    }
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
        convert::Type::Unit => output
            .write_all(b"()")
            .map_err(|_| request::Error::Encoding),
        convert::Type::Builtin(convert::Builtin::InclusiveRange) => output
            .write_all(b"std::ops::RangeInclusive<i64>")
            .map_err(|_| request::Error::Encoding),
        convert::Type::Builtin(
            convert::Builtin::String | convert::Builtin::Text,
        ) => output
            .write_all(b"String")
            .map_err(|_| request::Error::Encoding),
        convert::Type::Builtin(convert::Builtin::Str) => output
            .write_all(b"&str")
            .map_err(|_| request::Error::Encoding),
        convert::Type::Builtin(builtin) => output
            .write_all(builtin.rust_name().unwrap_or_default().as_bytes())
            .map_err(|_| request::Error::Encoding),
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
        convert::Type::Option(element) => {
            output
                .write_all(b"Option<")
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
            (convert::Type::FunctionItem { .. }, true, false) => {
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

fn render_function_arguments(
    signature: &crate::classify::representability::signature::Signature,
    substitutions: &[convert::Type],
    functions: &[crate::surface::adapter::FunctionItem],
    mappers: &[crate::surface::adapter::EventMapper],
    offset: usize,
    profile: Option<&crate::profile::Proof>,
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
                let ty = substitute(ty, signature, substitutions)?;
                if let Some(profile) = profile {
                    render_take_profile(position, runtime, ty, profile, output)?;
                } else {
                    render_take(position, runtime, ty, output)?;
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
    } = plan.operation()
    else {
        return Err(request::Error::Drift {
            field: "generated static function item plan",
        });
    };
    if !substitutions.is_empty()
        || !constants.is_empty()
        || !functions.is_empty()
        || !mappers.is_empty()
    {
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
        render_take(
            index,
            index + offset,
            substitute(ty, signature, substitutions)?,
            output,
        )?;
    }
    Ok(())
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
    // A `&str` input lends the string taken from Rawr.
    if matches!(ty, convert::Type::Builtin(convert::Builtin::Str)) {
        return writeln!(
            output,
            "    let argument_{local}_owned = take_string(arguments, {index})?;\n    let argument_{local} = argument_{local}_owned.as_str();"
        )
        .map_err(|_| request::Error::Encoding);
    }
    let helper = match ty {
        convert::Type::Builtin(convert::Builtin::String) => "take_string",
        convert::Type::Builtin(convert::Builtin::InclusiveRange) => "take_range",
        convert::Type::Builtin(builtin) if builtin.is_narrow_integer() => "take_int",
        convert::Type::Builtin(convert::Builtin::F32) => "take_f32",
        ty if is_duration(ty) => "take_primitive",
        convert::Type::Nominal { .. } | convert::Type::Parameter(_) => "take_native",
        convert::Type::FunctionItem { .. } => {
            return Err(request::Error::Drift {
                field: "static function item entered runtime argument decoding",
            });
        }
        convert::Type::Sequence { .. } | convert::Type::Tuple(_) => {
            return Err(request::Error::Drift {
                field: "output-only sequence or tuple entered runtime argument decoding",
            });
        }
        convert::Type::ProfileMessage { payload, .. } => {
            return render_take(local, index, payload, output);
        }
        convert::Type::Option(_) | convert::Type::Result(..) => {
            // A Rawr `Option` of the converted `T`, or a Rawr
            // `Result` of the converted `T` and `E`.
            let mut converted = String::new();
            render_from_dynamic(ty, "value", 0, &mut converted)?;
            return writeln!(
                output,
                "    let argument_{local} = {{ let value = (**arguments.values.get_mut({index}).ok_or(Error::MissingArgument)?).take(); {converted} }}?;"
            )
            .map_err(|_| request::Error::Encoding);
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
        ty if is_duration(ty) => writeln!(output, "    Ok(Dynamic::from({value}))"),
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
        convert::Type::Unit | convert::Type::Builtin(_) => {
            writeln!(output, "    Ok(Dynamic::from({value}))")
        }
        convert::Type::FunctionItem { .. } => {
            return Err(request::Error::Drift {
                field: "static function item entered runtime output encoding",
            });
        }
        convert::Type::ProfileMessage { payload, .. } => {
            return render_give(payload, value, output);
        }
        convert::Type::Option(_)
        | convert::Type::Result(..)
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
        convert::Type::Builtin(
            convert::Builtin::String | convert::Builtin::Str | convert::Builtin::Text,
        ) => write!(
            output,
            "{value}.try_cast_result::<ImmutableString>().map(|value| value.to_string()).map_err(|_| Error::TypeMismatch)"
        ),
        ty if is_duration(ty) => cast(output, "std::time::Duration"),
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
        convert::Type::FunctionItem { .. } => {
            return Err(request::Error::Drift {
                field: "static function item entered runtime argument decoding",
            });
        }
        convert::Type::Sequence { .. } | convert::Type::Tuple(_) => {
            return Err(request::Error::Drift {
                field: "output-only sequence or tuple entered runtime argument decoding",
            });
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
        ty if is_duration(ty) => {
            write!(output, "Ok::<Dynamic, Error>(Dynamic::from({value}))")
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
        convert::Type::Sequence { element, owned } => {
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
        convert::Type::FunctionItem { .. } => {
            return Err(request::Error::Drift {
                field: "static function item entered runtime output encoding",
            });
        }
    }
    .map_err(|_| request::Error::Encoding)
}

/// Rust's `std::time::Duration`, which crosses by value.
fn is_duration(ty: &convert::Type) -> bool {
    matches!(ty, convert::Type::Nominal { definition, .. }
        if *definition == crate::surface::identity::Definition::duration())
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
    writeln!(
        output,
        "    let task = {{\n        #[allow(unused_imports)]\n        use __rawr_profile_host::task::{{ViaFuture as _, ViaTask as _}};\n        (&__rawr_profile_host::task::Project::new({value})).into_task()\n    }};\n    Ok(__rawr_profile_host::task::imported(task.map(|value| -> __rawr_profile_host::task::Materialize {{\n        Box::new(move |witness| materialize(witness, move |arguments| {converted}))\n    }})))"
    )
    .map_err(|_| request::Error::Encoding)
}

fn render_give_with(
    signature: &crate::classify::representability::signature::Signature,
    substitutions: &[convert::Type],
    value: &str,
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    render_give(
        substitute(signature.output(), signature, substitutions)?,
        value,
        output,
    )
}

fn substitute<'a>(
    ty: &'a convert::Type,
    signature: &'a crate::classify::representability::signature::Signature,
    substitutions: &'a [convert::Type],
) -> Result<&'a convert::Type, request::Error> {
    let convert::Type::Parameter(index) = ty else {
        return Ok(ty);
    };
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
    let partition = partition_generics(signature, substitutions, constants, container_parameters)?;
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

thread_local! {
    /// The Rust path of each imported type of the facade plan the runner
    /// being generated declares, by identity: an imported type argument is
    /// written out where its path is known, so a value the arena stores or
    /// finds has its exact type (`chrono::DateTime<chrono::Local>`).
    static TYPE_PATHS: std::cell::RefCell<std::collections::BTreeMap<[u8; 32], String>> =
        const { std::cell::RefCell::new(std::collections::BTreeMap::new()) };
    /// The imported types that are Copy, by identity, as the compiler reads
    /// a field of one through a shared borrow.
    static COPY_TYPES: std::cell::RefCell<std::collections::BTreeSet<[u8; 32]>> =
        const { std::cell::RefCell::new(std::collections::BTreeSet::new()) };
}

/// Fills `TYPE_PATHS` for one generation and empties it when dropped.
struct TypePaths;

impl TypePaths {
    fn install(facade: Option<&crate::surface::facade::Plan>) -> Self {
        TYPE_PATHS.with_borrow_mut(|paths| {
            paths.clear();
            for owner in facade.into_iter().flat_map(crate::surface::facade::Plan::facades) {
                for ty in owner.types() {
                    paths
                        .entry(*ty.identity())
                        .or_insert_with(|| format!("{}::{}", owner.root(), ty.path()));
                }
            }
        });
        COPY_TYPES.with_borrow_mut(|copy| {
            copy.clear();
            for owner in facade.into_iter().flat_map(crate::surface::facade::Plan::facades) {
                copy.extend(owner.types().filter(|ty| ty.copy()).map(|ty| *ty.identity()));
            }
        });
        Self
    }
}

impl Drop for TypePaths {
    fn drop(&mut self) {
        TYPE_PATHS.with_borrow_mut(std::collections::BTreeMap::clear);
        COPY_TYPES.with_borrow_mut(std::collections::BTreeSet::clear);
    }
}

/// The Rust spelling of `ty`, its imported types by their facade paths, or
/// `None` when it has no exact spelling.
fn concrete_type(ty: &convert::Type) -> Option<String> {
    Some(match ty {
        convert::Type::Unit => "()".to_owned(),
        convert::Type::Builtin(convert::Builtin::String | convert::Builtin::Text) => {
            "String".to_owned()
        }
        convert::Type::Builtin(convert::Builtin::Str) => "&str".to_owned(),
        convert::Type::Builtin(convert::Builtin::InclusiveRange) => {
            "std::ops::RangeInclusive<i64>".to_owned()
        }
        convert::Type::Builtin(builtin) => builtin.rust_name()?.to_owned(),
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
        convert::Type::Result(ok, err) => format!(
            "Result<{}, {}>",
            concrete_type(ok)?,
            concrete_type(err)?
        ),
        convert::Type::ProfileMessage { payload, .. } => concrete_type(payload)?,
        convert::Type::Sequence { element, .. } => format!("Vec<{}>", concrete_type(element)?),
        convert::Type::Tuple(members) => format!(
            "({},)",
            members
                .iter()
                .map(concrete_type)
                .collect::<Option<Vec<_>>>()?
                .join(", ")
        ),
        convert::Type::FunctionItem { .. } | convert::Type::Parameter(_) => return None,
    })
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
    output
        .write_all(opening)
        .map_err(|_| request::Error::Encoding)?;
    let mut type_index = 0_usize;
    let mut constant_index = 0_usize;
    for (index, parameter) in parameters.iter().enumerate() {
        if index != 0 {
            output
                .write_all(b", ")
                .map_err(|_| request::Error::Encoding)?;
        }
        match parameter.kind() {
            crate::classify::representability::signature::Kind::Type => {
                let ty = substitutions.get(type_index).ok_or(request::Error::Drift {
                    field: "generated type substitution cardinality",
                })?;
                type_index += 1;
                let concrete = match ty {
                    convert::Type::Nominal { .. }
                    | convert::Type::Option(_)
                    | convert::Type::Result(..) => concrete_type(ty),
                    _ => None,
                };
                output
                    .write_all(match ty {
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
                        | convert::Type::Option(_)
                        | convert::Type::Result(..)
                        | convert::Type::Sequence { .. }
                        | convert::Type::Tuple(_) => {
                            concrete.as_deref().unwrap_or("_").as_bytes()
                        }
                        convert::Type::FunctionItem { .. } => {
                            return Err(request::Error::Drift {
                                field: "static function item used as generic substitution",
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
            crate::classify::representability::signature::Kind::Callable => {
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
                      match invoke_one(identity, handle, &mut arena) {\n\
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
            convert::Builtin::String | convert::Builtin::Str | convert::Builtin::Text,
        ) => "Dynamic::from(\"\")",
        convert::Type::Option(_) => "__rawr_internal::option::none()",
        // `Result` has no `Default`, as in Rust.
        convert::Type::Result(..)
        | convert::Type::Nominal { .. }
        | convert::Type::Sequence { .. }
        | convert::Type::Tuple(_)
        | convert::Type::FunctionItem { .. }
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

fn digest(bytes: &[u8]) -> ([u8; 32], usize) {
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

    #[test]
    fn a_str_input_is_lent_and_a_display_output_is_its_text() {
        let mut take = Vec::new();
        super::render_take(1, 1, &Type::Builtin(Builtin::Str), &mut take).unwrap();
        let take = String::from_utf8(take).unwrap();
        assert!(take.contains("let argument_1_owned = take_string(arguments, 1)?;"), "{take}");
        assert!(take.contains("let argument_1 = argument_1_owned.as_str();"), "{take}");
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
        assert!(super::copy_scalar(&Type::Result(Box::new(usize()), Box::new(Type::Unit))));
        assert!(!super::copy_scalar(&Type::Option(Box::new(Type::Builtin(Builtin::String)))));
    }

    #[test]
    fn runner_manifest_makes_only_the_runner_unit_incremental() {
        let harness = b"[workspace]\nresolver = \"3\"\n\n[package]\nname = \"rawr-resolution-harness\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\n[lib]\npath = \"src/lib.rs\"\n\n[profile.dev.package.\"*\"]\nopt-level = 3\n\n[dependencies]\niced = \"0.14\"\n";
        let manifest = super::runner_manifest(harness).unwrap();
        let document = std::str::from_utf8(&manifest)
            .unwrap()
            .parse::<toml_edit::DocumentMut>()
            .unwrap();
        let dev = &document["profile"]["dev"];
        // The harness builds every dependency non-incremental; the runner's
        // dependency units are those units only when its profile agrees.
        assert_eq!(dev["incremental"].as_bool(), Some(false));
        assert_eq!(
            dev["package"]["rawr-generated-runner"]["incremental"].as_bool(),
            Some(true)
        );
        assert_eq!(dev["package"]["*"]["opt-level"].as_integer(), Some(3));
        assert_eq!(
            document["package"]["name"].as_str(),
            Some("rawr-generated-runner")
        );
        assert!(document.get("lib").is_none());
        assert!(super::runner_manifest(
            b"[workspace]\n[package]\nname = \"h\"\n[profile]\ndev = 1\n"
        )
        .is_err());
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
        assert!(neutral.contains("engine.compile_package_snapshot(snapshot)"));
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
