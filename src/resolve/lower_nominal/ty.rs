//! The lowering's types: `Ty` and the integer, float, pointer and counter
//! kinds it is made of.

#[cfg(feature = "rust-import")]
use super::ImportedFunctionProof;
use super::{
    cell, counted, cow, dyn_arguments, error_object, range_type_id, sequence, thread, Models,
    Nominal, ParseError, PreludeOp,
};
use crate::resolve::type_ref;
use crate::types::nominal;
use crate::{type_def, Dynamic, ImmutableString, INT};
use std::any::TypeId;
use std::collections::BTreeMap;
#[cfg(feature = "rust-import")]
use std::collections::BTreeSet;

#[derive(Clone, Debug, Eq)]
#[cfg_attr(feature = "rust-import", derive(serde::Deserialize, serde::Serialize))]
pub(super) struct HostTyProof {
    pub(super) definition: crate::host::type_def::Definition,
    pub(super) alias: Option<crate::host::type_def::Definition>,
    pub(super) owner: Option<crate::host::facade::Dependency>,
}

/// One imported type is one type at each of its paths, as rustc has it: a
/// crate's macros name its types through a hidden segment
/// (`events::__rawr_crate_events_<id>::stock::Stock`) beside their public
/// paths, and a value made at one path is the type a signature names at
/// the other (#893). Any other type is the definition it names.
impl PartialEq for HostTyProof {
    fn eq(&self, other: &Self) -> bool {
        match (
            self.definition.identity().imported(),
            other.definition.identity().imported(),
        ) {
            (Some(identity), Some(other_identity)) => {
                identity == other_identity && self.owner == other.owner
            }
            _ => {
                self.definition == other.definition
                    && self.alias == other.alias
                    && self.owner == other.owner
            }
        }
    }
}

#[cfg(all(test, feature = "rust-import"))]
std::thread_local! {
    static HOST_TY_PROOF_ALLOCATIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Each marked type by its mirror's identity (`Models::mirrored`).
#[cfg(feature = "rust-import")]
pub(super) type Mirrored = BTreeMap<crate::host::type_def::import::Identity, type_def::Id>;

#[cfg(feature = "rust-import")]
std::thread_local! {
    /// The mirrors of the program whose models this thread made last
    /// (`Models::new`), which every resolved type names its marked type
    /// by ([`Ty::from_resolved`], #1484).
    static MIRRORED: std::cell::RefCell<std::sync::Arc<Mirrored>> = std::cell::RefCell::new(std::sync::Arc::default());
}

/// Read every resolved type this thread lowers from now on with
/// `mirrored`'s marked types for their mirrors: a native alias's
/// `Element<'a, Message>` over the marked `Theme` names that `Theme`, as a
/// native signature does (#1484).
#[cfg(feature = "rust-import")]
pub(super) fn lower_mirrors(mirrored: std::sync::Arc<Mirrored>) {
    MIRRORED.with_borrow_mut(|current| *current = mirrored);
}

/// The marked type `definition` mirrors in the program being lowered.
#[cfg(feature = "rust-import")]
pub(super) fn mirrored(definition: &crate::host::type_def::Definition) -> Option<type_def::Id> {
    let identity = definition.identity().imported()?;
    MIRRORED.with_borrow(|mirrored| mirrored.get(identity).copied())
}

impl HostTyProof {
    pub(super) fn new(
        definition: crate::host::type_def::Definition,
        alias: Option<crate::host::type_def::Definition>,
        owner: Option<crate::host::facade::Dependency>,
    ) -> crate::Shared<Self> {
        #[cfg(all(test, feature = "rust-import"))]
        HOST_TY_PROOF_ALLOCATIONS.with(|allocations| allocations.set(allocations.get() + 1));
        crate::Shared::new(Self {
            definition,
            alias,
            owner,
        })
    }
}

#[cfg(all(test, feature = "rust-import"))]
pub(super) fn reset_host_ty_proof_allocations() {
    HOST_TY_PROOF_ALLOCATIONS.with(|allocations| allocations.set(0));
}

#[cfg(all(test, feature = "rust-import"))]
pub(super) fn host_ty_proof_allocations() -> usize {
    HOST_TY_PROOF_ALLOCATIONS.with(std::cell::Cell::get)
}

/// Rust's keyed collections (`Ty::Keyed`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "rust-import", derive(serde::Deserialize, serde::Serialize))]
pub(super) enum Keyed {
    HashMap,
    BTreeMap,
    HashSet,
    BTreeSet,
}

impl Keyed {
    pub(super) fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "HashMap" => Self::HashMap,
            "BTreeMap" => Self::BTreeMap,
            "HashSet" => Self::HashSet,
            "BTreeSet" => Self::BTreeSet,
            _ => return None,
        })
    }

    pub(super) const fn name(self) -> &'static str {
        match self {
            Self::HashMap => "HashMap",
            Self::BTreeMap => "BTreeMap",
            Self::HashSet => "HashSet",
            Self::BTreeSet => "BTreeSet",
        }
    }

    pub(super) const fn is_map(self) -> bool {
        matches!(self, Self::HashMap | Self::BTreeMap)
    }

    /// What its keys need: `Hash` and `Eq`, or `Ord`.
    pub(super) const fn hashed(self) -> bool {
        matches!(self, Self::HashMap | Self::HashSet)
    }
}

/// Rust's integer types. All but `i128` and `u128` are an `i64` at
/// runtime: the kind decides the range arithmetic is checked against and
/// what `as` truncates to, and a `u64` or a `usize` is its bits. An `i128`
/// or a `u128` is an `i64` while its value fits one and a boxed Rust `i128`
/// or `u128` past that (`wide_value`), so each value has one runtime form.
/// `Var` is a literal's `{integer}`, bound by inference
/// (`Inference::resolve`) and `i32` when nothing binds it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "rust-import", derive(serde::Deserialize, serde::Serialize))]
pub(super) enum Int {
    I8,
    I16,
    I32,
    I64,
    Isize,
    U8,
    U16,
    U32,
    U64,
    Usize,
    I128,
    U128,
    Var(usize),
}

/// The kind an unsuffixed literal of `ty` falls back to, as rustc's: an
/// `i32` or an `f64`.
pub(super) fn literal_fallback(ty: &Ty) -> Option<Ty> {
    match ty {
        Ty::Int(Int::Var(_)) => Some(Ty::Int(Int::I32)),
        Ty::Float(Float::Var(_)) => Some(Ty::Float(Float::F64)),
        _ => None,
    }
}

/// The `kind_code`s of `i128` and `u128`, past the other kinds' and the
/// floats' and casts' codes.
pub(super) const WIDE_I128: INT = 17;
pub(super) const WIDE_U128: INT = 18;

/// Added to a `Cast`'s `i128` or `u128` code: the operand is a `u64`'s or a
/// `usize`'s bits.
pub(super) const UNSIGNED_SOURCE: INT = 32;

impl Int {
    /// Each kind by name. A kind's index is its `kind_code`, but for
    /// `i128` and `u128` (`WIDE_I128`, `WIDE_U128`).
    pub(super) const KINDS: [(&'static str, Self); 12] = [
        ("i8", Self::I8),
        ("i16", Self::I16),
        ("i32", Self::I32),
        ("i64", Self::I64),
        ("isize", Self::Isize),
        ("u8", Self::U8),
        ("u16", Self::U16),
        ("u32", Self::U32),
        ("u64", Self::U64),
        ("usize", Self::Usize),
        ("i128", Self::I128),
        ("u128", Self::U128),
    ];

    /// The kind a `kind_code` names.
    pub(super) fn from_code(code: INT) -> Option<Self> {
        match code {
            WIDE_I128 => Some(Self::I128),
            WIDE_U128 => Some(Self::U128),
            0..10 => Some(Self::KINDS[code as usize].1),
            _ => None,
        }
    }

    /// `i128` or `u128`, boxed at runtime past `i64` (`wide_value`).
    pub(super) const fn wide(self) -> bool {
        matches!(self, Self::I128 | Self::U128)
    }

    pub(super) fn named(name: &str) -> Option<Self> {
        Self::KINDS
            .iter()
            .find(|(kind, _)| *kind == name)
            .map(|(_, kind)| *kind)
    }

    pub(super) fn name(self) -> &'static str {
        Self::KINDS
            .iter()
            .find(|(_, kind)| *kind == self)
            .map_or("{integer}", |(name, _)| name)
    }

    /// The values the kind holds at runtime.
    pub(super) const fn range(self) -> (INT, INT) {
        match self {
            Self::I8 => (i8::MIN as INT, i8::MAX as INT),
            Self::I16 => (i16::MIN as INT, i16::MAX as INT),
            Self::I32 => (i32::MIN as INT, i32::MAX as INT),
            Self::U8 => (0, u8::MAX as INT),
            Self::U16 => (0, u16::MAX as INT),
            Self::U32 => (0, u32::MAX as INT),
            Self::U64 | Self::Usize | Self::U128 => (0, INT::MAX),
            Self::I64 | Self::Isize | Self::I128 | Self::Var(_) => (INT::MIN, INT::MAX),
        }
    }

    /// Whether an `i64` result needs a check against `range`: the `i64`
    /// arithmetic already panics on its own overflow.
    pub(super) const fn checked(self) -> bool {
        !matches!(self, Self::I64 | Self::Isize | Self::Var(_))
    }

    pub(super) const fn signed(self) -> bool {
        matches!(
            self,
            Self::I8 | Self::I16 | Self::I32 | Self::I64 | Self::Isize | Self::I128 | Self::Var(_)
        )
    }

    pub(super) const fn bits(self) -> u32 {
        match self {
            Self::I8 | Self::U8 => 8,
            Self::I16 | Self::U16 => 16,
            Self::I32 | Self::U32 => 32,
            Self::I128 | Self::U128 => 128,
            _ => 64,
        }
    }

    /// Rust's bounds of the kind; a `u128`'s stop at `i128::MAX`.
    pub(super) const fn full_range(self) -> (i128, i128) {
        match self {
            Self::U64 | Self::Usize => (0, u64::MAX as i128),
            Self::I128 => (i128::MIN, i128::MAX),
            Self::U128 => (0, i128::MAX),
            _ => {
                let (low, high) = self.range();
                (low as i128, high as i128)
            }
        }
    }

    /// The unsigned kind of the same width.
    pub(super) const fn unsigned(self) -> Self {
        match self {
            Self::I8 => Self::U8,
            Self::I16 => Self::U16,
            Self::I32 => Self::U32,
            Self::I64 | Self::Var(_) => Self::U64,
            Self::Isize => Self::Usize,
            kind => kind,
        }
    }

    /// A runtime value of the kind as the number it is: a `u64`/`usize`
    /// from its bits.
    pub(super) const fn widen(self, value: INT) -> i128 {
        match self {
            Self::U64 | Self::Usize => value as u64 as i128,
            _ => value as i128,
        }
    }

    /// A number in the kind's bounds as the runtime keeps it.
    pub(super) const fn store(self, value: i128) -> INT {
        value as INT
    }

    /// A pattern of the kind's bits as its signed or unsigned number.
    pub(super) const fn sign(self, pattern: i128) -> i128 {
        let bits = self.bits();
        if self.signed() && pattern >> (bits - 1) & 1 == 1 {
            pattern - (1 << bits)
        } else {
            pattern
        }
    }
}

/// Rust's float types, an `f64` at runtime; `Var` is a literal's `{float}`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "rust-import", derive(serde::Deserialize, serde::Serialize))]
pub(super) enum Float {
    F32,
    F64,
    Var(usize),
}

impl Float {
    pub(super) fn named(name: &str) -> Option<Self> {
        match name {
            "f32" => Some(Self::F32),
            "f64" => Some(Self::F64),
            _ => None,
        }
    }

    pub(super) const fn name(self) -> &'static str {
        match self {
            Self::F32 => "f32",
            Self::F64 => "f64",
            Self::Var(_) => "{float}",
        }
    }
}

/// The numeric type a builtin name names: `usize`, `f32`, ..
pub(super) fn numeric_type(name: &str) -> Option<Ty> {
    Int::named(name)
        .map(Ty::Int)
        .or_else(|| Float::named(name).map(Ty::Float))
}

/// `std::time`'s types, as `Ty::Time` names them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "rust-import", derive(serde::Deserialize, serde::Serialize))]
pub(super) enum Time {
    Duration,
    Instant,
}

impl Time {
    pub(super) const fn name(self) -> &'static str {
        match self {
            Self::Duration => "Duration",
            Self::Instant => "Instant",
        }
    }
}

/// Which of std's cells a `Ty::Cell` is: `RefCell`, whose value is
/// borrowed, `Cell`, whose value is copied in and out, or `Mutex`, whose
/// value one thread at a time locks, as a `RefCell` borrows it mutably.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "rust-import", derive(serde::Deserialize, serde::Serialize))]
pub(super) enum CellKind {
    RefCell,
    Cell,
    Mutex,
}

impl CellKind {
    /// The cell `name` is, as `type_ref::Type::cell_element` names it.
    pub(super) fn named(name: &str) -> Self {
        match name {
            "Cell" => Self::Cell,
            "Mutex" => Self::Mutex,
            _ => Self::RefCell,
        }
    }

    pub(super) const fn name(self) -> &'static str {
        match self {
            Self::RefCell => "RefCell",
            Self::Cell => "Cell",
            Self::Mutex => "Mutex",
        }
    }

    /// The op that makes a new cell of this kind.
    pub(super) const fn new_op(self) -> PreludeOp {
        match self {
            Self::RefCell => PreludeOp::CellNew,
            Self::Cell => PreludeOp::PlainCellNew,
            Self::Mutex => PreludeOp::MutexNew,
        }
    }
}

/// The pointer a path names: `Box`, `std::rc::Rc` or `std::sync::Arc`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Pointer {
    Box,
    Rc,
    Arc,
}

impl Pointer {
    /// The pointer `name` is, as `type_ref::Type::pointer_element` names it.
    pub(super) fn named(name: &str) -> Self {
        match name {
            "Rc" => Self::Rc,
            "Arc" => Self::Arc,
            _ => Self::Box,
        }
    }

    pub(super) const fn name(self) -> &'static str {
        match self {
            Self::Box => "Box",
            Self::Rc => "Rc",
            Self::Arc => "Arc",
        }
    }

    /// The counted pointer this is: `Rc` or `Arc`, not `Box`.
    pub(super) const fn counter(self) -> Option<Counter> {
        match self {
            Self::Box => None,
            Self::Rc => Some(Counter::Rc),
            Self::Arc => Some(Counter::Arc),
        }
    }

    /// The type of this pointer to `target`.
    pub(super) fn ty(self, target: Ty) -> Ty {
        match self.counter() {
            None => Ty::Boxed(Box::new(target)),
            Some(counter) => Ty::Counted(counter, Box::new(target)),
        }
    }
}

/// Which counted pointer a `Ty::Counted` or `Ty::Weak` is: `std::rc`'s or
/// `std::sync`'s.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "rust-import", derive(serde::Deserialize, serde::Serialize))]
pub(super) enum Counter {
    Rc,
    Arc,
}

impl Counter {
    pub(super) const fn name(self) -> &'static str {
        match self {
            Self::Rc => "Rc",
            Self::Arc => "Arc",
        }
    }

    /// Rawr's builtin name for the pointer's `Weak`, as
    /// `type_ref::Type::weak_element` names it.
    pub(super) const fn weak_name(self) -> &'static str {
        match self {
            Self::Rc => "Weak",
            Self::Arc => "SyncWeak",
        }
    }

    /// The counter whose `Weak` rawr's builtin `name` is.
    pub(super) fn weak_named(name: &str) -> Self {
        if name == Self::Arc.weak_name() {
            Self::Arc
        } else {
            Self::Rc
        }
    }

    /// The std definitions of the pointer and of its `Weak`.
    pub(super) const fn std_paths(self) -> (&'static str, &'static str) {
        match self {
            Self::Rc => ("alloc::rc::Rc", "alloc::rc::Weak"),
            Self::Arc => ("alloc::sync::Arc", "alloc::sync::Weak"),
        }
    }

    /// rustc's E0594 for an assignment through the pointer, which has no
    /// `DerefMut`.
    pub(super) fn unassignable(self) -> ImmutableString {
        format!("cannot assign to data in an `{}`", self.name()).into()
    }
}

/// What Rust refuses of a place the lowering gives no place of its own.
#[derive(Clone)]
pub(super) struct Unplaced {
    /// rustc's code and message for a move out of it.
    pub(super) moved: (&'static str, String),
    /// rustc's E0594 message for an assignment to it, where it is behind a
    /// pointer with no `DerefMut`.
    pub(super) assigned: Option<ImmutableString>,
}

/// The path of a std struct rawr knows (`core::str::iter::Chars`), held
/// while the program runs. A stored one is read back as the copy the
/// process holds (`stored::text`).
type Std = &'static str;

/// Which `PartialEq` impl an `==` resolves to.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PartialEqImpl {
    /// The left operand's type's own, `PartialEq<Self>`: the right operand
    /// takes the left's type.
    Own,
    /// An impl between two types, which compares their values as they are.
    Between,
}

/// A function a `fn` pointer choice may hold: a script function or
/// closure, or a crate's function, each its own item as in Rust.
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "rust-import", derive(serde::Deserialize, serde::Serialize))]
pub(super) enum FnCandidate {
    Script(crate::item::Key),
    /// A crate's function, whose value is `()`: its type names it, as
    /// `text::warning` beside `text::default` is.
    #[cfg(feature = "rust-import")]
    Imported {
        input: Box<Ty>,
        output: Box<Ty>,
        proof: Box<ImportedFunctionProof>,
    },
}

impl FnCandidate {
    /// The candidate a function's type names, when it names one.
    pub(super) fn of(ty: &Ty) -> Option<Self> {
        match ty {
            Ty::FnItem { key, .. } => Some(Self::Script((**key).clone())),
            #[cfg(feature = "rust-import")]
            Ty::ImportedFunctionItem {
                input,
                output,
                item: Some(proof),
            } => Some(Self::Imported {
                input: input.clone(),
                output: output.clone(),
                proof: proof.clone(),
            }),
            _ => None,
        }
    }

    /// Both name the same function, wherever each was written.
    pub(super) fn same(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Script(left), Self::Script(right)) => left == right,
            #[cfg(feature = "rust-import")]
            (Self::Imported { proof: left, .. }, Self::Imported { proof: right, .. }) => {
                left.item == right.item
                    && left.owner == right.owner
                    && left.parameters == right.parameters
            }
            #[cfg(feature = "rust-import")]
            _ => false,
        }
    }

    /// The type of the candidate's own value, which a host call takes as
    /// that exact function.
    pub(super) fn ty(&self, inputs: &[Ty], output: &Ty) -> Ty {
        match self {
            Self::Script(key) => Ty::FnItem {
                key: Box::new(key.clone()),
                inputs: inputs.into(),
                output: Box::new(output.clone()),
            },
            #[cfg(feature = "rust-import")]
            Self::Imported {
                input,
                output,
                proof,
            } => Ty::ImportedFunctionItem {
                input: input.clone(),
                output: output.clone(),
                item: Some(proof.clone()),
            },
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "rust-import", derive(serde::Deserialize, serde::Serialize))]
pub(super) enum Ty {
    Never,
    Unit,
    /// A host callable: a symbolic item taking `inputs` in order.
    Callable {
        inputs: Box<[Ty]>,
        output: Box<Ty>,
        item: Option<Box<crate::item::Value>>,
    },
    #[cfg(feature = "rust-import")]
    ImportedFunctionItem {
        input: Box<Ty>,
        output: Box<Ty>,
        item: Option<Box<ImportedFunctionProof>>,
    },
    #[cfg(feature = "rust-import")]
    ProfileMapper {
        input: Box<Ty>,
        output: Box<Ty>,
        item: Box<crate::item::Value>,
        seal: [u8; 16],
    },
    #[cfg(feature = "rust-import")]
    ProfileMessage {
        payload: Box<Ty>,
        script: Box<Ty>,
        seal: [u8; 16],
    },
    /// A closure or `fn` pointer, a function pointer at runtime.
    Fn {
        inputs: Box<[Ty]>,
        output: Box<Ty>,
    },
    /// `[T; N]`, `Vec<T>`, `[T]` or `VecDeque<T>`, as `kind` says: an
    /// array at runtime.
    Array {
        element: Box<Ty>,
        kind: sequence::Kind,
    },
    /// `(A, B, ..)`: an array at runtime.
    Tuple(Box<[Ty]>),
    /// Rust's `Option<T>`, a builtin generic enum: an
    /// `Option<Dynamic>` at runtime.
    Option(Box<Ty>),
    /// Rust's `Result<T, E>`, a builtin generic enum: a
    /// `Result<Dynamic, Dynamic>` at runtime.
    Result(Box<Ty>, Box<Ty>),
    /// A named function or non-capturing closure, which has its own type as
    /// in Rust and coerces to `fn` pointers and host callables.
    FnItem {
        key: Box<crate::item::Key>,
        inputs: Box<[Ty]>,
        output: Box<Ty>,
    },
    /// One of several named functions of one signature, chosen at runtime
    /// (`if dark { panel } else { cell }`): Rust's `fn` pointer, which keeps
    /// its candidates so a host callable can take each exact item. At
    /// runtime, `[index, fn pointer]`.
    FnChoice {
        keys: crate::Shared<Vec<FnCandidate>>,
        inputs: Box<[Ty]>,
        output: Box<Ty>,
    },
    Bool,
    Char,
    /// An integer of a kind (`i64`, `usize`, `u8`, ..), or an integer
    /// literal's kind still to be inferred: an `i64` at runtime.
    Int(Int),
    /// `f64`, `f32` (an `f64` rounded to `f32` after each operation), or a
    /// float literal's kind still to be inferred.
    Float(Float),
    String,
    /// `str`, rustc's unsized text: what `*` of a `&str` or a `String`
    /// gives, usable only as a place (`&*s`, a comparison, a method's
    /// receiver). The same text as a `String` at runtime. A reference to
    /// it is `Ty::Str`.
    UnsizedStr,
    /// `&str`: a borrowed string, the same text as a `String` at runtime.
    /// It is `Copy`, so a use never moves it. It is a distinct type from
    /// `String`, as in Rust: a `&String` becomes a `&str` only by deref
    /// coercion.
    Str,
    /// `a..=b` of integers of a kind: Rust's `RangeInclusive<T>`, a value
    /// that is also an iterator of `T`. A `RangeInclusive<i64>` at runtime.
    InclusiveRange(Int),
    /// `a..=b` of floats: Rust's `RangeInclusive<f64>`, bounds only.
    FloatRange,
    /// `a..b` of floats: Rust's `Range<T>` as a value, which an imported
    /// generic item takes as rand's `random_range` does. A `Range<f64>` at
    /// runtime.
    Range(Box<Ty>),
    /// Rust's `RangeInclusive<T>` of a crate's type, as iced's
    /// `Radians::RANGE` is: a `RangeInclusive<Dynamic>` of its two bounds
    /// at runtime.
    InclusiveOf(Box<Ty>),
    /// `std::cmp::Ordering`: an `i64` at runtime, its Rust discriminant
    /// (`Less` -1, `Equal` 0, `Greater` 1), so `==` and `<` are the integers'.
    Ordering,
    /// `std::time::Duration` or `Instant` of a program without iced (whose
    /// own `time` names them otherwise): Rust's value at runtime.
    Time(Time),
    /// `std::num::ParseIntError` and its kin, the error of `str::parse`:
    /// its message at runtime.
    ParseError(ParseError),
    /// core's `fmt::Arguments<'_>`, what `format_args!` makes: the text it
    /// formats at runtime, which `Display` and `Debug` both write as it is.
    /// A type of its own, so a crate's impl for it is the one a coercion
    /// picks (`impl Value for fmt::Arguments<'_>`, #1561).
    Arguments,
    Reference {
        mutable: bool,
        target: Box<Ty>,
    },
    Host {
        proof: crate::Shared<HostTyProof>,
        arguments: Box<[Ty]>,
    },
    /// A script struct or enum, with its type arguments when generic.
    Script(type_def::Id, Box<[Ty]>),
    /// `Box<T>`: the value itself at runtime, as Rawr values are already
    /// heap-allocated; method calls and fields auto-deref through it.
    Boxed(Box<Ty>),
    /// `Rc<T>` or `Arc<T>`: a shared, counted pointer at runtime
    /// (`counted.rs`). A clone shares the value, and its counts and
    /// address are observable. Method calls and fields auto-deref through
    /// std's `Deref`, which reads the value.
    Counted(Counter, Box<Ty>),
    /// `std::rc::Weak<T>` or `std::sync::Weak<T>`: a pointer that doesn't
    /// keep the value alive (`counted.rs`).
    Weak(Counter, Box<Ty>),
    /// `HashMap<K, V>`, `BTreeMap<K, V>`, `HashSet<K>` or `BTreeSet<K>`
    /// with its type arguments: at runtime an array sorted by key, of
    /// `[key, value]` pairs for a map and of keys for a set, so every kind
    /// iterates in key order (`keyed.rs`).
    Keyed(Keyed, Box<[Ty]>),
    /// `std::cmp::Reverse<T>`: a `std::cmp::Reverse<Dynamic>` at runtime,
    /// which `compare_value` orders backwards.
    Reverse(Box<Ty>),
    /// `std::borrow::Cow<'_, B>` of its borrowed type `B` (`str`, `[T]` or
    /// a sized `T`): a `cow::Value` at runtime.
    Cow(Box<Ty>),
    /// `std::cell::RefCell<T>` or `std::cell::Cell<T>`: a `cell::Cell` at
    /// runtime, which every copy of it shares (`cell.rs`).
    Cell(CellKind, Box<Ty>),
    /// `std::thread::JoinHandle<T>`, `std::sync::mpsc::Sender<T>` or
    /// `Receiver<T>`: std's own handle at runtime (`thread.rs`), which a
    /// move hands on and a copy never makes.
    Thread(thread::Kind, Box<Ty>),
    /// An iterator of `Item` (`impl Iterator<Item = T>`): an adaptor chain
    /// fused into one loop where it is consumed, else the array of its
    /// items at runtime. With the std struct it is, when rawr knows it
    /// (`core::str::iter::Chars` of `s.chars()`), whose own inherent
    /// methods it also has; an adaptor's chain is any iterator.
    Iter(
        Box<Ty>,
        #[cfg_attr(
            feature = "rust-import",
            serde(
                serialize_with = "crate::stored::text::serialize_optional",
                deserialize_with = "crate::stored::text::deserialize_optional"
            )
        )]
        Option<Std>,
    ),
    /// `dyn Trait`, behind `&` or `Box`: a value of some type implementing
    /// the trait, whose methods dispatch by the value's runtime type.
    /// With its associated type bindings by name (`dyn Shape<Output =
    /// i64>`), sorted.
    Dyn(ImmutableString, Box<[(ImmutableString, Ty)]>),
    /// A generic type parameter of the item being checked: opaque, with
    /// only its bounds' capabilities; any value at runtime.
    Param(ImmutableString),
    /// The associated type `name` of a type's impl of a trait
    /// (`<T as Shape>::Output`, as base, trait, name): the impl's type once
    /// the type is known, else opaque with only the associated type's bounds
    /// (Rust's projection); any value at runtime.
    Projection(Box<Ty>, ImmutableString, ImmutableString),
    Infer(usize),
    Unknown,
}

impl Ty {
    /// Any iterator of `item`, as an adaptor makes.
    pub(super) fn iter(item: Ty) -> Self {
        Self::Iter(Box::new(item), None)
    }

    /// The sequence `kind` of `element`: `Vec<T>`, `[T]`, ..
    pub(super) fn sequence(kind: sequence::Kind, element: Ty) -> Self {
        Self::Array {
            element: Box::new(element),
            kind,
        }
    }

    /// Whether this is text: a `String` or a `&str`.
    pub(super) fn is_text(&self) -> bool {
        matches!(self, Self::String | Self::Str)
    }

    #[cfg(feature = "rust-import")]
    pub(super) fn collect_dependencies(
        &self,
        dependencies: &mut BTreeSet<crate::host::facade::Dependency>,
    ) {
        match self {
            Self::Callable { inputs, output, .. } => {
                for input in inputs {
                    input.collect_dependencies(dependencies);
                }
                output.collect_dependencies(dependencies);
            }
            Self::ImportedFunctionItem { input, output, .. } => {
                input.collect_dependencies(dependencies);
                output.collect_dependencies(dependencies);
                if let Self::ImportedFunctionItem {
                    item: Some(function),
                    ..
                } = self
                {
                    dependencies.insert(function.owner.clone());
                }
            }
            Self::ProfileMapper { input, output, .. } => {
                input.collect_dependencies(dependencies);
                output.collect_dependencies(dependencies);
            }
            Self::ProfileMessage {
                payload, script, ..
            } => {
                payload.collect_dependencies(dependencies);
                script.collect_dependencies(dependencies);
            }
            Self::Reference { target, .. }
            | Self::Boxed(target)
            | Self::Counted(_, target)
            | Self::Weak(_, target)
            | Self::Iter(target, _)
            | Self::Reverse(target)
            | Self::Cow(target)
            | Self::Range(target)
            | Self::InclusiveOf(target)
            | Self::Cell(_, target)
            | Self::Thread(_, target)
            | Self::Projection(target, ..) => {
                target.collect_dependencies(dependencies);
            }
            Self::Fn { inputs, output }
            | Self::FnItem { inputs, output, .. }
            | Self::FnChoice { inputs, output, .. } => {
                for input in inputs {
                    input.collect_dependencies(dependencies);
                }
                output.collect_dependencies(dependencies);
                #[cfg(feature = "rust-import")]
                if let Self::FnChoice { keys, .. } = self {
                    for candidate in keys.iter() {
                        if let FnCandidate::Imported { proof, .. } = candidate {
                            dependencies.insert(proof.owner.clone());
                        }
                    }
                }
            }
            Self::Array { element, .. } | Self::Option(element) => {
                element.collect_dependencies(dependencies);
            }
            Self::Result(ok, err) => {
                ok.collect_dependencies(dependencies);
                err.collect_dependencies(dependencies);
            }
            Self::Tuple(members) | Self::Keyed(_, members) => {
                for member in members {
                    member.collect_dependencies(dependencies);
                }
            }
            Self::Host { proof, arguments } => {
                dependencies.extend(proof.owner.iter().cloned());
                for argument in arguments {
                    argument.collect_dependencies(dependencies);
                }
            }
            Self::Never
            | Self::Unit
            | Self::Bool
            | Self::Char
            | Self::Int(_)
            | Self::Float(_)
            | Self::String
            | Self::UnsizedStr
            | Self::Str
            | Self::InclusiveRange(_)
            | Self::FloatRange
            | Self::Ordering
            | Self::Time(_)
            | Self::ParseError(_)
            | Self::Arguments
            | Self::Script(..)
            | Self::Dyn(..)
            | Self::Param(_)
            | Self::Infer(_)
            | Self::Unknown => {}
        }
    }

    pub(super) fn from_resolved(ty: &type_ref::Type) -> Self {
        if let Some(element) = ty.option_element() {
            return Self::Option(Box::new(Self::from_resolved(element)));
        }
        if let Some((element, kind)) = sequence::Kind::of(ty) {
            return Self::sequence(kind, Self::from_resolved(element));
        }
        if let Some((pointer, element)) = ty.pointer_element() {
            return Pointer::named(pointer).ty(Self::from_resolved(element));
        }
        if let Some((weak, element)) = ty.weak_element() {
            return Self::Weak(
                Counter::weak_named(weak),
                Box::new(Self::from_resolved(element)),
            );
        }
        if let Some(trait_name) = ty.dyn_trait() {
            // `dyn Convert<i64>`: positional arguments as the bindings
            // `0`, `1`, ...
            let mut bindings = ty
                .dyn_arguments()
                .into_iter()
                .enumerate()
                .map(|(index, ty)| {
                    (
                        ImmutableString::from(index.to_string()),
                        Self::from_resolved(ty),
                    )
                })
                .chain(
                    ty.dyn_bindings()
                        .into_iter()
                        .map(|(name, ty)| (name, Self::from_resolved(ty))),
                )
                .collect::<Vec<_>>();
            bindings.sort_by(|(a, _), (b, _)| a.cmp(b));
            return Self::Dyn(trait_name.into(), bindings.into_boxed_slice());
        }
        if let Some((inputs, output)) = ty.fn_signature() {
            return Self::Fn {
                inputs: inputs.into_iter().map(Self::from_resolved).collect(),
                output: Box::new(Self::from_resolved(output)),
            };
        }
        if let Some((ok, err)) = ty.result_elements() {
            return Self::Result(
                Box::new(Self::from_resolved(ok)),
                Box::new(Self::from_resolved(err)),
            );
        }
        if let Some((item, _)) = ty.iterator_item() {
            return Self::iter(Self::from_resolved(item));
        }
        if let Some(inner) = ty.reverse_element() {
            return Self::Reverse(Box::new(Self::from_resolved(inner)));
        }
        // A header's bare `str` reads as a `&str` (`type_ref::Type::is_str`);
        // a `Cow`'s `B` is the unsized `str`.
        if let Some(borrowed) = ty.cow_element() {
            return Self::Cow(Box::new(match Self::from_resolved(borrowed) {
                Self::Str => Self::UnsizedStr,
                borrowed => borrowed,
            }));
        }
        if let Some((kind, inner)) = ty.cell_element() {
            return Self::Cell(CellKind::named(kind), Box::new(Self::from_resolved(inner)));
        }
        if let Some((name, inner)) = ty.thread_element()
            && let Some(kind) = thread::Kind::named(name)
        {
            return Self::Thread(kind, Box::new(Self::from_resolved(inner)));
        }
        if let Some((name, arguments)) = ty.keyed_arguments()
            && let Some(kind) = Keyed::from_name(name)
        {
            return Self::Keyed(
                kind,
                arguments.into_iter().map(Self::from_resolved).collect(),
            );
        }
        match ty {
            type_ref::Type::Unit(_) => Self::Unit,
            type_ref::Type::Tuple { members, .. } => {
                Self::Tuple(members.iter().map(Self::from_resolved).collect())
            }
            type_ref::Type::Reference {
                mutable, target, ..
            } => Self::Reference {
                mutable: *mutable,
                target: Box::new(Self::from_resolved(target)),
            },
            type_ref::Type::Path(_) if ty.is_str() => Self::Str,
            type_ref::Type::Path(path) => match &path.target {
                type_ref::Target::Builtin => match path.name.as_str() {
                    crate::item::type_ref::NEVER_TYPE => Self::Never,
                    "bool" => Self::Bool,
                    "char" => Self::Char,
                    "String" => Self::String,
                    "Ordering" => Self::Ordering,
                    "Duration" => Self::Time(Time::Duration),
                    "Instant" => Self::Time(Time::Instant),
                    // `RangeInclusive<f64>` and `RangeInclusive<i64>`, and
                    // one of a crate's type.
                    "RangeInclusive" => match path.arguments.first() {
                        Some(type_ref::Argument::Type(element)) => {
                            match Self::from_resolved(element) {
                                Self::Float(_) => Self::FloatRange,
                                element @ Self::Host { .. } => Self::InclusiveOf(Box::new(element)),
                                Self::Int(kind) => Self::InclusiveRange(kind),
                                _ => Self::InclusiveRange(Int::I64),
                            }
                        }
                        _ => Self::InclusiveRange(Int::I64),
                    },
                    name => ParseError::named(name)
                        .map(Self::ParseError)
                        .or_else(|| numeric_type(name))
                        .unwrap_or(Self::Unknown),
                },
                type_ref::Target::Host { definition, owner } => {
                    let arguments: Box<[Self]> = path
                        .arguments
                        .iter()
                        .filter_map(|argument| match argument {
                            type_ref::Argument::Type(ty) => Some(Self::from_resolved(ty)),
                            type_ref::Argument::Lifetime(_) => None,
                        })
                        .collect();
                    // A mirror is its marked type, wherever a resolved type
                    // names it: through a native alias, in a crate type's
                    // arguments, in an output (#1484).
                    #[cfg(feature = "rust-import")]
                    if let Some(marked) = mirrored(definition) {
                        return Self::Script(marked, arguments);
                    }
                    let alias = definition.identity().imported().map(|_| definition.clone());
                    Self::Host {
                        proof: HostTyProof::new(definition.clone(), alias, owner.clone()),
                        arguments,
                    }
                }
                type_ref::Target::Script { definition, .. } => Self::Script(
                    *definition,
                    path.arguments
                        .iter()
                        .filter_map(|argument| match argument {
                            type_ref::Argument::Type(ty) => Some(Self::from_resolved(ty)),
                            type_ref::Argument::Lifetime(_) => None,
                        })
                        .collect(),
                ),
                type_ref::Target::Parameter => Self::Param(path.name.clone()),
                type_ref::Target::Projection(base, trait_name) => Self::Projection(
                    Box::new(Self::from_resolved(base)),
                    trait_name.clone().unwrap_or_default(),
                    path.name.clone(),
                ),
                // A resolved header names an alias's target, never the alias,
                // and never a std type Rawr doesn't represent.
                type_ref::Target::Alias(_) | type_ref::Target::Std(_) => Self::Unknown,
            },
        }
    }

    pub(super) fn name(&self, models: &Models) -> ImmutableString {
        match self {
            Self::Never => "!".into(),
            Self::Unit => "()".into(),
            Self::Callable { inputs, output, .. } => fn_type_name(inputs, output, models).into(),
            // A crate's function item, as rustc writes one: `fn(Id) ->
            // Task<Message> {window::close}`.
            #[cfg(feature = "rust-import")]
            Self::ImportedFunctionItem {
                input,
                output,
                item,
            } => {
                let mut name = fn_type_name(parameters(input), output, models);
                if let Some(item) = item {
                    name += &format!(" {{{}}}", crate::host::path::public(&item.item.path()));
                }
                name.into()
            }
            #[cfg(feature = "rust-import")]
            Self::ProfileMapper { input, output, .. } => {
                fn_type_name(parameters(input), output, models).into()
            }
            #[cfg(feature = "rust-import")]
            Self::ProfileMessage { script, .. } => script.name(models),
            Self::Bool => "bool".into(),
            Self::Char => "char".into(),
            Self::Int(kind) => kind.name().into(),
            Self::Float(kind) => kind.name().into(),
            Self::String => "String".into(),
            Self::UnsizedStr => "str".into(),
            Self::Str => "&str".into(),
            Self::InclusiveRange(kind) => format!("RangeInclusive<{}>", kind.name()).into(),
            Self::FloatRange => "RangeInclusive<f64>".into(),
            Self::Range(element) => format!("Range<{}>", element.name(models)).into(),
            Self::InclusiveOf(element) => {
                format!("RangeInclusive<{}>", element.name(models)).into()
            }
            Self::Ordering => "Ordering".into(),
            Self::Time(time) => time.name().into(),
            Self::ParseError(error) => error.name().into(),
            Self::Arguments => "Arguments<'_>".into(),
            Self::Fn { inputs, output }
            | Self::FnItem { inputs, output, .. }
            | Self::FnChoice { inputs, output, .. } => {
                let mut name = fn_type_name(inputs, output, models);
                if let Self::FnItem { key, .. } = self {
                    name += &format!(" {{{key}}}");
                }
                name.into()
            }
            Self::Array { element, kind } => {
                let element = element.name(models);
                match kind {
                    sequence::Kind::Array(length) => format!("[{element}; {length}]"),
                    sequence::Kind::Vec => format!("Vec<{element}>"),
                    sequence::Kind::Slice => format!("[{element}]"),
                    sequence::Kind::Deque => format!("VecDeque<{element}>"),
                }
                .into()
            }
            Self::Option(element) => format!("Option<{}>", element.name(models)).into(),
            Self::Result(ok, err) => {
                format!("Result<{}, {}>", ok.name(models), err.name(models)).into()
            }
            Self::Tuple(members) => {
                let names = members
                    .iter()
                    .map(|member| member.name(models).to_string())
                    .collect::<Vec<_>>();
                if names.len() == 1 {
                    format!("({},)", names[0]).into()
                } else {
                    format!("({})", names.join(", ")).into()
                }
            }
            Self::Reference { mutable, target } => format!(
                "&{}{}",
                if *mutable { "mut " } else { "" },
                pointee_name(target, models)
            )
            .into(),
            Self::Boxed(target) => format!("Box<{}>", pointee_name(target, models)).into(),
            Self::Counted(counter, target) => {
                format!("{}<{}>", counter.name(), pointee_name(target, models)).into()
            }
            Self::Weak(_, target) => format!("Weak<{}>", target.name(models)).into(),
            // rustc names a std iterator by its struct: `Chars<'_>`.
            Self::Iter(_, Some(std)) => {
                format!("{}<'_>", std.rsplit("::").next().unwrap_or(std)).into()
            }
            Self::Iter(item, None) => format!("impl Iterator<Item = {}>", item.name(models)).into(),
            Self::Reverse(inner) => format!("Reverse<{}>", inner.name(models)).into(),
            Self::Cow(borrowed) => format!("Cow<'_, {}>", borrowed.name(models)).into(),
            Self::Cell(kind, inner) => format!("{}<{}>", kind.name(), inner.name(models)).into(),
            Self::Thread(kind, inner) => kind.name(&inner.name(models)).into(),
            Self::Keyed(kind, arguments) => format!(
                "{}<{}>",
                kind.name(),
                arguments
                    .iter()
                    .map(|argument| argument.name(models).to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
            .into(),
            Self::Dyn(trait_name, bindings) if bindings.is_empty() => {
                format!("dyn {trait_name}").into()
            }
            Self::Dyn(trait_name, bindings) => format!(
                "dyn {trait_name}<{}>",
                dyn_arguments(bindings)
                    .iter()
                    .map(|ty| ty.name(models).to_string())
                    .chain(
                        bindings
                            .iter()
                            .filter(|(name, _)| name.parse::<usize>().is_err())
                            .map(|(name, ty)| format!("{name} = {}", ty.name(models)))
                    )
                    .collect::<Vec<_>>()
                    .join(", ")
            )
            .into(),
            Self::Host { proof, arguments } if arguments.is_empty() => {
                host_type_name(proof, models)
            }
            Self::Host { proof, arguments } => format!(
                "{}<{}>",
                host_type_name(proof, models),
                arguments
                    .iter()
                    .map(|argument| argument.name(models))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
            .into(),
            Self::Script(id, arguments) if arguments.is_empty() => {
                models.definition(*id).name().into()
            }
            Self::Script(id, arguments) => format!(
                "{}<{}>",
                models.definition(*id).name(),
                arguments
                    .iter()
                    .map(|argument| argument.name(models))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
            .into(),
            Self::Param(name) => name.clone(),
            Self::Projection(base, _, name) => format!("{}::{name}", base.name(models)).into(),
            // rustc writes a type it has yet to infer as `_`. A value of any
            // type has no other name in Rust.
            Self::Infer(_) | Self::Unknown => "_".into(),
        }
    }

    /// The type of the value at runtime. An imported Rust value is a handle
    /// the generated runner names, so a field or payload holding one takes
    /// any value, as a generic one does.
    pub(super) fn type_id(&self, models: &Models) -> TypeId {
        self.native_type_id(models)
            .unwrap_or_else(TypeId::of::<Dynamic>)
    }

    pub(super) fn native_type_id(&self, models: &Models) -> Option<TypeId> {
        match self {
            Self::Never => Some(TypeId::of::<Dynamic>()),
            Self::Unit => Some(TypeId::of::<()>()),
            Self::Callable { .. } => Some(TypeId::of::<crate::item::Value>()),
            Self::Fn { .. } | Self::FnItem { .. } => Some(TypeId::of::<crate::FnPtr>()),
            Self::Array { .. }
            | Self::Tuple(_)
            | Self::FnChoice { .. }
            | Self::Iter(..)
            | Self::Keyed(..) => Some(TypeId::of::<crate::Array>()),
            Self::Reverse(_) => Some(TypeId::of::<std::cmp::Reverse<Dynamic>>()),
            Self::Cow(_) => Some(TypeId::of::<cow::Value>()),
            Self::Cell(..) => Some(TypeId::of::<cell::Cell>()),
            Self::Thread(kind, _) => match kind {
                #[cfg(feature = "sync")]
                thread::Kind::JoinHandle => Some(TypeId::of::<crate::eval::threads::Handle>()),
                #[cfg(not(feature = "sync"))]
                thread::Kind::JoinHandle => None,
                #[cfg(feature = "sync")]
                thread::Kind::ScopedJoinHandle => {
                    Some(TypeId::of::<crate::eval::threads::Handle>())
                }
                #[cfg(feature = "sync")]
                thread::Kind::Scope => Some(TypeId::of::<thread::Scope>()),
                #[cfg(not(feature = "sync"))]
                thread::Kind::ScopedJoinHandle | thread::Kind::Scope => None,
                thread::Kind::Sender => Some(TypeId::of::<thread::Sender>()),
                thread::Kind::SyncSender => Some(TypeId::of::<thread::SyncSender>()),
                thread::Kind::Receiver => Some(TypeId::of::<thread::Receiver>()),
                thread::Kind::SendError => Some(TypeId::of::<thread::SendError>()),
                thread::Kind::Panic => Some(TypeId::of::<thread::Panic>()),
                // No value is made: no lock rawr runs is poisoned.
                thread::Kind::PoisonError => Some(TypeId::of::<Dynamic>()),
            },
            Self::Option(_) => Some(TypeId::of::<crate::option::Value>()),
            Self::Result(..) => Some(TypeId::of::<crate::result::Value>()),
            #[cfg(feature = "rust-import")]
            Self::ImportedFunctionItem { .. } => None,
            #[cfg(feature = "rust-import")]
            Self::ProfileMapper { .. } => None,
            #[cfg(feature = "rust-import")]
            Self::ProfileMessage { payload, .. } => payload.native_type_id(models),
            Self::Bool => Some(TypeId::of::<bool>()),
            Self::Char => Some(TypeId::of::<char>()),
            // An `i128` or a `u128` is an `i64` or, past it, boxed: any value.
            Self::Int(kind) if kind.wide() => None,
            Self::Int(_) => Some(TypeId::of::<INT>()),
            Self::Float(_) => Some(TypeId::of::<crate::FLOAT>()),
            Self::String | Self::UnsizedStr | Self::Str => Some(TypeId::of::<ImmutableString>()),
            Self::InclusiveRange(_) => Some(TypeId::of::<std::ops::RangeInclusive<INT>>()),
            Self::FloatRange => Some(TypeId::of::<std::ops::RangeInclusive<crate::FLOAT>>()),
            Self::Range(element) => Some(range_type_id(element)),
            Self::InclusiveOf(_) => Some(TypeId::of::<std::ops::RangeInclusive<Dynamic>>()),
            Self::Ordering => Some(TypeId::of::<INT>()),
            Self::Time(Time::Duration) => Some(TypeId::of::<std::time::Duration>()),
            Self::Time(Time::Instant) => Some(TypeId::of::<crate::Instant>()),
            Self::ParseError(_) | Self::Arguments => Some(TypeId::of::<ImmutableString>()),
            Self::Reference { target, .. } | Self::Boxed(target) => target.native_type_id(models),
            Self::Counted(..) => Some(TypeId::of::<counted::Counted>()),
            Self::Weak(..) => Some(TypeId::of::<counted::Weak>()),
            Self::Host { proof, .. } => proof.definition.native_identity(),

            Self::Script(id, ..) => Some(match models.definition(*id) {
                Nominal::Struct { .. } => TypeId::of::<nominal::Struct>(),
                Nominal::Enum { .. } => TypeId::of::<nominal::Enum>(),
            }),
            // std's `dyn Error` is the error object `?` and `Box::new` make.
            Self::Dyn(name, bindings)
                if name.as_str() == "Error" && bindings.is_empty() && models.std_error_trait() =>
            {
                Some(TypeId::of::<error_object::ErrorObject>())
            }
            Self::Param(_)
            | Self::Projection(..)
            | Self::Dyn(..)
            | Self::Infer(_)
            | Self::Unknown => Some(TypeId::of::<Dynamic>()),
        }
    }

    /// Each type [`Self::native_type_id`] gives but a host type's: what a
    /// runner's kept lowering on disk may name besides its facades' linked
    /// types (#1638).
    #[cfg(feature = "rust-import")]
    pub(super) fn native_type_ids() -> impl Iterator<Item = TypeId> {
        [
            TypeId::of::<Dynamic>(),
            TypeId::of::<()>(),
            TypeId::of::<crate::item::Value>(),
            TypeId::of::<crate::FnPtr>(),
            TypeId::of::<crate::Array>(),
            TypeId::of::<std::cmp::Reverse<Dynamic>>(),
            TypeId::of::<cow::Value>(),
            TypeId::of::<cell::Cell>(),
            #[cfg(feature = "sync")]
            TypeId::of::<crate::eval::threads::Handle>(),
            #[cfg(feature = "sync")]
            TypeId::of::<thread::Scope>(),
            TypeId::of::<thread::Sender>(),
            TypeId::of::<thread::SyncSender>(),
            TypeId::of::<thread::Receiver>(),
            TypeId::of::<thread::SendError>(),
            TypeId::of::<thread::Panic>(),
            TypeId::of::<crate::option::Value>(),
            TypeId::of::<crate::result::Value>(),
            TypeId::of::<bool>(),
            TypeId::of::<char>(),
            TypeId::of::<INT>(),
            TypeId::of::<crate::FLOAT>(),
            TypeId::of::<ImmutableString>(),
            TypeId::of::<std::ops::RangeInclusive<INT>>(),
            TypeId::of::<std::ops::RangeInclusive<crate::FLOAT>>(),
            TypeId::of::<std::ops::Range<INT>>(),
            TypeId::of::<std::ops::Range<crate::FLOAT>>(),
            TypeId::of::<std::ops::RangeInclusive<Dynamic>>(),
            TypeId::of::<std::time::Duration>(),
            TypeId::of::<crate::Instant>(),
            TypeId::of::<counted::Counted>(),
            TypeId::of::<counted::Weak>(),
            TypeId::of::<nominal::Struct>(),
            TypeId::of::<nominal::Enum>(),
            TypeId::of::<error_object::ErrorObject>(),
        ]
        .into_iter()
    }

    pub(super) fn invocation(&self) -> Option<&crate::host::type_def::Definition> {
        match self {
            Self::Reference { target, .. } => target.invocation(),
            Self::Host { proof, .. }
                if proof.definition.storage() == crate::host::type_def::Storage::Invocation =>
            {
                Some(&proof.definition)
            }
            Self::Host { arguments, .. } => arguments.iter().find_map(Self::invocation),
            Self::Callable { inputs, output, .. } => inputs
                .iter()
                .find_map(Self::invocation)
                .or_else(|| output.invocation()),
            #[cfg(feature = "rust-import")]
            Self::ImportedFunctionItem { input, output, .. } => {
                input.invocation().or_else(|| output.invocation())
            }
            #[cfg(feature = "rust-import")]
            Self::ProfileMapper { input, output, .. } => {
                input.invocation().or_else(|| output.invocation())
            }
            #[cfg(feature = "rust-import")]
            Self::ProfileMessage {
                payload, script, ..
            } => payload.invocation().or_else(|| script.invocation()),
            _ => None,
        }
    }

    pub(super) fn host(&self) -> Option<&crate::host::type_def::Definition> {
        match self {
            Self::Reference { target, .. } => target.host(),
            Self::Host { proof, .. } => Some(&proof.definition),
            _ => None,
        }
    }

    pub(super) fn host_alias(&self) -> Option<&crate::host::type_def::Definition> {
        match self {
            Self::Reference { target, .. } => target.host_alias(),
            Self::Host { proof, .. } => match proof.definition.identity().imported() {
                Some(_) => proof.alias.as_ref(),
                None => Some(&proof.definition),
            },
            _ => None,
        }
    }

    #[cfg(feature = "rust-import")]
    pub(super) fn imported_alias_context(
        &self,
    ) -> Option<(
        &crate::host::type_def::import::Identity,
        &crate::host::facade::Dependency,
        Option<&crate::host::type_def::Definition>,
    )> {
        match self {
            Self::Reference { target, .. } => target.imported_alias_context(),
            Self::Host { proof, .. } => Some((
                proof.definition.identity().imported()?,
                proof.owner.as_ref()?,
                proof.alias.as_ref(),
            )),
            _ => None,
        }
    }

    pub(super) fn script(&self) -> Option<type_def::Id> {
        match self {
            Self::Reference { target, .. } => target.script(),
            Self::Script(definition, ..) => Some(*definition),
            _ => None,
        }
    }

    pub(super) fn usage(&self) -> crate::host::usage::Policy {
        match self {
            Self::Host { proof, .. } => proof.definition.usage(),
            #[cfg(feature = "rust-import")]
            Self::ImportedFunctionItem { .. } => crate::host::usage::Policy::Ordinary,
            #[cfg(feature = "rust-import")]
            Self::ProfileMapper { .. } => crate::host::usage::Policy::Ordinary,
            #[cfg(feature = "rust-import")]
            Self::ProfileMessage { .. } => crate::host::usage::Policy::Ordinary,
            Self::Never
            | Self::Unit
            | Self::Callable { .. }
            | Self::Fn { .. }
            | Self::FnItem { .. }
            | Self::FnChoice { .. }
            | Self::Array { .. }
            | Self::Tuple(_)
            | Self::Option(_)
            | Self::Result(..)
            | Self::Bool
            | Self::Char
            | Self::Int(_)
            | Self::Float(_)
            | Self::String
            | Self::UnsizedStr
            | Self::Str
            | Self::InclusiveRange(_)
            | Self::FloatRange
            | Self::Range(_)
            | Self::InclusiveOf(_)
            | Self::Ordering
            | Self::Time(_)
            | Self::ParseError(_)
            | Self::Arguments
            | Self::Script(..)
            | Self::Param(_)
            | Self::Projection(..)
            | Self::Reference { .. }
            | Self::Boxed(..)
            | Self::Counted(..)
            | Self::Weak(..)
            | Self::Iter(..)
            | Self::Keyed(..)
            | Self::Reverse(_)
            | Self::Cow(_)
            | Self::Cell(..)
            | Self::Thread(..)
            | Self::Dyn(..)
            | Self::Infer(_)
            | Self::Unknown => crate::host::usage::Policy::Ordinary,
        }
    }

    pub(super) fn is_imported_static_operand(&self) -> bool {
        #[cfg(feature = "rust-import")]
        {
            matches!(
                self,
                Self::ImportedFunctionItem { .. } | Self::ProfileMapper { .. }
            )
        }
        #[cfg(not(feature = "rust-import"))]
        {
            false
        }
    }
}

/// The name of what a box, a reference or a counted pointer points to: a
/// function value there is rustc's `dyn Fn(A) -> B`, as Rawr's one
/// function type is `Box<dyn Fn>`'s pointee far more often than a `fn`
/// pointer's box.
fn pointee_name(target: &Ty, models: &Models) -> ImmutableString {
    match target {
        Ty::Fn { inputs, output } => {
            let name = fn_type_name(inputs, output, models);
            let signature = name.strip_prefix("fn").unwrap_or(&name);
            format!("dyn Fn{signature}").into()
        }
        target => target.name(models),
    }
}

/// `fn(A, B) -> R`, leaving out `-> ()` as rustc does.
pub(super) fn fn_type_name(inputs: &[Ty], output: &Ty, models: &Models) -> String {
    let inputs = inputs
        .iter()
        .map(|input| input.name(models).to_string())
        .collect::<Vec<_>>()
        .join(", ");
    match output {
        Ty::Unit => format!("fn({inputs})"),
        output => format!("fn({inputs}) -> {}", output.name(models)),
    }
}

/// Why a call of a generic function compiled per instantiation (D629) has
/// no instance: a written type parameter its arguments and the rest of the
/// body leave open, or types no instance takes. The turbofish it suggests
/// names only the written parameters, as an argument-position `impl Trait`
/// takes none.
#[cfg(feature = "rust-import")]
pub(super) fn uninstantiated_call(
    name: &str,
    generics: &[crate::resolve::item::Generic],
    types: &[Ty],
    models: &Models,
) -> String {
    let open = generics
        .iter()
        .zip(types)
        .filter(|(generic, ty)| !generic.lifted && contains_inference(ty))
        .map(|(generic, _)| format!("`{}`", generic.name.text))
        .collect::<Vec<_>>();
    let written = super::generics::written(generics)
        .map(|generic| generic.name.text.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    match open.as_slice() {
        [] => format!(
            "calling `{name}` with the type arguments `<{}>` isn't supported yet",
            types
                .iter()
                .map(|ty| ty.name(models).to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ),
        [one] => format!(
            "the type parameter {one} of `{name}` must be known from this call's arguments or \
             its expected type; write it at the call, as in `{name}::<{written}>(..)`"
        ),
        several => format!(
            "the type parameters {} of `{name}` must be known from this call's arguments or \
             its expected type; write them at the call, as in `{name}::<{written}>(..)`",
            several.join(", ")
        ),
    }
}

/// The parameters of a crate callable whose one input is them all: a
/// tuple is several (`fn(&Theme, Status)`), `()` is none.
#[cfg(feature = "rust-import")]
pub(super) fn parameters(input: &Ty) -> &[Ty] {
    match input {
        Ty::Tuple(members) => members,
        Ty::Unit => &[],
        input => std::slice::from_ref(input),
    }
}

/// A host type's path as a message names it: the path the program wrote,
/// else the one a `use` in this source imports it by, else its shortest
/// public path (`iced::Vector`, not `iced::advanced::graphics::Vector`),
/// as rustc names a type by a visible path. A path a macro's `$crate`
/// wrote through a crate's hidden segment comes last, and reads from that
/// crate's root (`iced_core::text::Span`, `host::path::public`). Every type
/// a message names goes through here, by way of [`Ty::name`].
pub(super) fn host_type_name(proof: &HostTyProof, models: &Models) -> ImmutableString {
    use crate::host::path::{is_hidden, public};
    if let Some(alias) = proof.alias.as_ref().map(|alias| alias.path())
        && !is_hidden(&alias)
    {
        return alias.into();
    }
    #[cfg(feature = "rust-import")]
    if let Some(identity) = proof.definition.identity().imported() {
        let imported = models
            .imports
            .range((models.context, ImmutableString::new())..)
            .take_while(|((source, _), _)| *source == models.context)
            .find_map(|(_, binding)| match &binding.target {
                crate::resolve::item::Import::Type { definition, .. }
                    if definition.identity().imported() == Some(identity) =>
                {
                    Some(definition.path()).filter(|path| !is_hidden(path))
                }
                _ => None,
            });
        let shortest = || {
            models
                .host
                .imported_type_paths(identity, proof.owner.as_ref()?)?
                .min_by_key(|path| (is_hidden(path), path.matches("::").count()))
        };
        if let Some(path) = imported.or_else(shortest) {
            return public(&path).as_ref().into();
        }
    }
    #[cfg(not(feature = "rust-import"))]
    let _ = models;
    public(&proof.definition.path()).as_ref().into()
}

pub(super) fn same_head(left: &Ty, right: &Ty) -> bool {
    match (left, right) {
        (
            Ty::Host {
                proof: left_proof, ..
            },
            Ty::Host {
                proof: right_proof, ..
            },
        ) => same_host_definition(
            &left_proof.definition,
            &left_proof.owner,
            &right_proof.definition,
            &right_proof.owner,
        ),
        _ => left == right,
    }
}

pub(super) fn same_host_definition(
    left: &crate::host::type_def::Definition,
    left_owner: &Option<crate::host::facade::Dependency>,
    right: &crate::host::type_def::Definition,
    right_owner: &Option<crate::host::facade::Dependency>,
) -> bool {
    match (left.identity().imported(), right.identity().imported()) {
        (Some(left), Some(right)) => left == right,
        _ => {
            left.identity() == right.identity()
                && left.path() == right.path()
                && left_owner == right_owner
        }
    }
}

pub(super) fn same_inferred(left: &Ty, right: &Ty) -> bool {
    fn compare(
        left: &Ty,
        right: &Ty,
        forward: &mut BTreeMap<usize, usize>,
        reverse: &mut BTreeMap<usize, usize>,
    ) -> bool {
        match (left, right) {
            (Ty::Infer(left), Ty::Infer(right)) => {
                if forward.get(left).is_some_and(|mapped| mapped != right)
                    || reverse.get(right).is_some_and(|mapped| mapped != left)
                {
                    return false;
                }
                forward.insert(*left, *right);
                reverse.insert(*right, *left);
                true
            }
            (
                Ty::Host {
                    proof: left_proof,
                    arguments: left_arguments,
                },
                Ty::Host {
                    proof: right_proof,
                    arguments: right_arguments,
                },
            ) => {
                same_host_definition(
                    &left_proof.definition,
                    &left_proof.owner,
                    &right_proof.definition,
                    &right_proof.owner,
                ) && left_arguments.len() == right_arguments.len()
                    && left_arguments
                        .iter()
                        .zip(right_arguments)
                        .all(|(left, right)| compare(left, right, forward, reverse))
            }
            (
                Ty::Callable {
                    inputs: left_inputs,
                    output: left_output,
                    ..
                },
                Ty::Callable {
                    inputs: right_inputs,
                    output: right_output,
                    ..
                },
            ) => {
                left_inputs.len() == right_inputs.len()
                    && left_inputs
                        .iter()
                        .zip(right_inputs.iter())
                        .all(|(left, right)| compare(left, right, forward, reverse))
                    && compare(left_output, right_output, forward, reverse)
            }
            #[cfg(feature = "rust-import")]
            (
                Ty::ProfileMessage {
                    payload: left_payload,
                    script: left_script,
                    seal: left_seal,
                },
                Ty::ProfileMessage {
                    payload: right_payload,
                    script: right_script,
                    seal: right_seal,
                },
            ) => {
                left_seal == right_seal
                    && compare(left_payload, right_payload, forward, reverse)
                    && compare(left_script, right_script, forward, reverse)
            }
            (
                Ty::Reference {
                    mutable: left_mutable,
                    target: left_target,
                },
                Ty::Reference {
                    mutable: right_mutable,
                    target: right_target,
                },
            ) => {
                left_mutable == right_mutable
                    && compare(left_target, right_target, forward, reverse)
            }
            (Ty::Boxed(left), Ty::Boxed(right)) => compare(left, right, forward, reverse),
            (Ty::Counted(left_counter, left), Ty::Counted(right_counter, right))
            | (Ty::Weak(left_counter, left), Ty::Weak(right_counter, right)) => {
                left_counter == right_counter && compare(left, right, forward, reverse)
            }
            (Ty::Option(left), Ty::Option(right))
            | (Ty::Iter(left, _), Ty::Iter(right, _))
            | (Ty::Reverse(left), Ty::Reverse(right))
            | (Ty::Cow(left), Ty::Cow(right))
            | (Ty::Range(left), Ty::Range(right))
            | (Ty::InclusiveOf(left), Ty::InclusiveOf(right)) => {
                compare(left, right, forward, reverse)
            }
            (Ty::InclusiveRange(left), Ty::InclusiveRange(right)) => {
                compare(&Ty::Int(*left), &Ty::Int(*right), forward, reverse)
            }
            (Ty::Cell(left_kind, left), Ty::Cell(right_kind, right)) => {
                left_kind == right_kind && compare(left, right, forward, reverse)
            }
            (Ty::Thread(left_kind, left), Ty::Thread(right_kind, right)) => {
                left_kind == right_kind && compare(left, right, forward, reverse)
            }
            (Ty::Keyed(left_kind, left), Ty::Keyed(right_kind, right)) => {
                left_kind == right_kind
                    && left.len() == right.len()
                    && left
                        .iter()
                        .zip(right.iter())
                        .all(|(left, right)| compare(left, right, forward, reverse))
            }
            (Ty::Result(left_ok, left_err), Ty::Result(right_ok, right_err)) => {
                compare(left_ok, right_ok, forward, reverse)
                    && compare(left_err, right_err, forward, reverse)
            }
            _ => left == right,
        }
    }

    compare(left, right, &mut BTreeMap::new(), &mut BTreeMap::new())
}

pub(super) fn exact_symbolic_type(ty: &type_ref::Type) -> Option<Ty> {
    match ty {
        type_ref::Type::Unit(_) | type_ref::Type::Path(_) => Some(Ty::from_resolved(ty)),
        type_ref::Type::Tuple { .. } | type_ref::Type::Reference { .. } => None,
    }
}

/// A symbolic callable's output where an imported item takes it: an owned
/// value, or a tuple of owned values, as iced's `new` gives its state and
/// first task, `(Self, Task<Message>)`, which the generated runner carries
/// as the tuple it is.
pub(super) fn owned_symbolic_type(ty: &type_ref::Type) -> Option<Ty> {
    match ty {
        type_ref::Type::Tuple { members, .. }
            if members
                .iter()
                .all(|member| owned_symbolic_type(member).is_some()) =>
        {
            Some(Ty::from_resolved(ty))
        }
        ty => exact_symbolic_type(ty),
    }
}

/// A symbolic callable's input: an owned value, or a shared borrow of one as
/// in an iced style function's `&Theme`. Its type keeps the borrow, which
/// the runtime passes by value.
pub(super) fn symbolic_input_type(ty: &type_ref::Type) -> Option<Ty> {
    match ty {
        type_ref::Type::Reference {
            mutable: false,
            target,
            ..
        } => exact_symbolic_type(target).map(|target| Ty::Reference {
            mutable: false,
            target: Box::new(target),
        }),
        _ => exact_symbolic_type(ty),
    }
}

/// Whether a type still has an inference variable in it.
/// Whether `ty` holds a generic parameter of the body being checked where
/// a crate's item would take it as a type of its own, not as a script
/// value: the parameter itself, or inside a crate type, a borrow, an
/// `Option`, a `Result`, a tuple or a collection.
#[cfg(feature = "rust-import")]
pub(super) fn reaches_param(ty: &Ty) -> bool {
    match ty {
        Ty::Param(_) => true,
        Ty::Reference { target, .. }
        | Ty::Boxed(target)
        | Ty::Range(target)
        | Ty::InclusiveOf(target) => reaches_param(target),
        Ty::Array { element, .. } | Ty::Option(element) => reaches_param(element),
        Ty::Result(ok, err) => reaches_param(ok) || reaches_param(err),
        Ty::Tuple(members) | Ty::Keyed(_, members) => members.iter().any(reaches_param),
        Ty::Host { arguments, .. } => arguments.iter().any(reaches_param),
        _ => false,
    }
}

/// Whether `ty` is a script type, or holds one as a crate type's argument
/// or behind a borrow.
#[cfg(feature = "rust-import")]
pub(super) fn mentions_script(ty: &Ty) -> bool {
    match ty {
        Ty::Script(..) => true,
        Ty::Reference { target, .. } | Ty::Boxed(target) => mentions_script(target),
        Ty::Host { arguments, .. } => arguments.iter().any(mentions_script),
        _ => false,
    }
}

pub(super) fn contains_inference(ty: &Ty) -> bool {
    contains(ty, &|ty| matches!(ty, Ty::Infer(_)))
}

/// The first unknown type inside `ty`, outermost first.
#[cfg(feature = "rust-import")]
pub(super) fn first_inference(ty: &Ty) -> Option<Ty> {
    let found = std::cell::RefCell::new(None);
    contains(ty, &|inner| {
        matches!(inner, Ty::Infer(_)) && found.replace(Some(inner.clone())).is_none()
    });
    found.into_inner()
}

/// Whether `ty` holds a literal whose kind is still open: `{integer}` or
/// `{float}`, which only the end of its body settles.
pub(super) fn contains_open_literal(ty: &Ty) -> bool {
    contains(ty, &|ty| {
        matches!(ty, Ty::Int(Int::Var(_)) | Ty::Float(Float::Var(_)))
    })
}

/// Whether `ty` is itself a variable inference has not settled: an
/// unknown type, or a literal whose kind is still open.
pub(super) fn is_unsettled(ty: &Ty) -> bool {
    matches!(
        ty,
        Ty::Infer(_) | Ty::Int(Int::Var(_)) | Ty::Float(Float::Var(_))
    )
}

/// Whether `ty` holds a variable inference has not settled: an unknown
/// type, or a literal whose kind is still open.
pub(super) fn contains_unsettled(ty: &Ty) -> bool {
    contains_inference(ty) || contains_open_literal(ty)
}

/// Whether `ty` or a type inside it is one `leaf` picks.
pub(super) fn contains(ty: &Ty, leaf: &dyn Fn(&Ty) -> bool) -> bool {
    let inside = |ty: &Ty| contains(ty, leaf);
    leaf(ty)
        || match ty {
            Ty::Reference { target, .. }
            | Ty::Boxed(target)
            | Ty::Iter(target, _)
            | Ty::Reverse(target)
            | Ty::Cow(target)
            | Ty::Range(target)
            | Ty::InclusiveOf(target)
            | Ty::Cell(_, target) => inside(target),
            Ty::Array { element, .. } | Ty::Option(element) => inside(element),
            Ty::Result(ok, err) => inside(ok) || inside(err),
            Ty::Tuple(members) | Ty::Keyed(_, members) => members.iter().any(inside),
            Ty::Host { arguments, .. } => arguments.iter().any(inside),
            Ty::Callable { inputs, output, .. }
            | Ty::Fn { inputs, output }
            | Ty::FnItem { inputs, output, .. }
            | Ty::FnChoice { inputs, output, .. } => inputs.iter().any(inside) || inside(output),
            _ => false,
        }
}
