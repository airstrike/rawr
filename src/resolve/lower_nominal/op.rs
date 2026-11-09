//! Lowered operations: what the parser emits (`Op`), the prelude's own
//! operations, and the proven `ExecutableOp`s registered with the runtime.

use super::compare::{Comparison, Shape};
use super::format::{debug_value, debug_value_spec, pretty_debug, DebugLeaf, DebugSpec};
use super::inference::Inference;
use super::ty::{numeric_type, Int, Ty};
use super::wide::{integer_text, wide_value, Wide};
#[cfg(feature = "rust-import")]
use super::without_references;
use super::{
    cast_value, cell, char_method, collect_try, compare, counted, cow, drops, error_object,
    instantiate, int_method, keyed_query, origin, parse_text, places, sequence, slice_value,
    str_change, str_method, str_pieces, thread, time_method, vec_method, Models, Nominal,
    ParsedExpr, CHAR_METHODS, COLLECT_OPTION, COLLECT_RESULT, PRIVATE_PREFIX, SLICE_CHUNKS,
};
use crate::func::{FnCallArgs, NativeCallContext, RawrFunc};
use crate::resolve::{intrinsic, item, type_ref};
use crate::types::dynamic::Union;
use crate::types::nominal;
use crate::{
    capability, method, receiver, type_def, Dynamic, EvalAltResult, FuncRegistration,
    ImmutableString, Module, Position, RawrResult, Shared, Span, INT,
};
use std::any::TypeId;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone)]
pub(super) enum Op {
    #[cfg(feature = "rust-import")]
    Sequence {
        target: Ty,
        span: Span,
    },
    #[cfg(feature = "rust-import")]
    Imported {
        path: ImmutableString,
        selector: crate::host::type_def::ImportedSelector,
        operation: ImportedOperation,
        span: Span,
    },
    Symbolic {
        value: crate::item::Value,
        /// A lifted closure's captures, passed to the op where it is written.
        captures: usize,
    },
    Intrinsic {
        operation: Box<IntrinsicOperation>,
    },
    Projection {
        operation: Box<ProjectionOperation>,
    },
    Struct {
        definition: type_def::Definition,
        item: item::Struct,
    },
    Enum {
        definition: type_def::Definition,
        item: item::Enum,
        variant: usize,
    },
    Clone {
        definition: type_def::Definition,
    },
    Debug {
        definition: type_def::Definition,
    },
    Equal {
        definition: type_def::Definition,
    },
    Default {
        value: Dynamic,
        definition: type_def::Definition,
    },
    Validate {
        definition: type_def::Definition,
    },
    MatchSubject {
        definition: type_def::Definition,
        item: item::Enum,
    },
    /// Write what a pattern bound by `&mut` back into the place it matched.
    Prelude(PreludeOp),
    /// `collect()` whose target inference decides: `CollectString`,
    /// `CollectVec` or `CollectTry` once the function is checked.
    Collect {
        target: Ty,
    },
    /// Whether that `collect()` stops at an item: at an `Err` collecting a
    /// `Result`, at a `None` collecting an `Option` (`CollectStop`).
    CollectStop {
        target: Ty,
    },
    /// An integer operation of the kinds inference decides once the
    /// function is checked: the value's (or the target's) and an operand's.
    Kinded {
        op: PreludeOp,
        kinds: [Ty; 2],
    },
    /// A comparison or a map's or set's operation on values of type `ty`
    /// (a map's or set's key), whose shape inference settles: where it
    /// holds unsigned integers (`Shape`). `flags` are `SHAPED_*`.
    Shaped {
        op: PreludeOp,
        ty: Ty,
        flags: u8,
    },
}

/// `Op::Shaped`'s comparison is `Ord`'s (`cmp`), not `PartialOrd`'s.
pub(super) const SHAPED_TOTAL: u8 = 1;
/// A hashed collection whose key type has a hand-written `PartialEq`: a
/// key is found by its `eq`, not by its order.
pub(super) const SHAPED_HASHED: u8 = 2;

/// The runtime primitives of `Option<T>` and `Result<T, E>`;
/// every method composes them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PreludeOp {
    Some,
    None,
    IsSome,
    Unwrap,
    Expect,
    Ok,
    Err,
    IsOk,
    /// The `Ok` value, or a panic showing the `Err` value.
    UnwrapOk,
    /// The `Err` value, or a panic showing the `Ok` value.
    UnwrapErr,
    ExpectOk,
    ExpectErr,
    /// `==` on values of a generic type, by their runtime values.
    GenericEqual,
    /// `{:?}` of a value of a generic type, by its runtime value.
    GenericDebug,
    /// Explicit source Clone, distinct from cloning an authority handle.
    ValueClone,
    /// A checked sequence membership query with borrowed operands.
    SequenceContains,
    /// A number's, `bool`'s or `()`'s `Debug` under a `{:?}` spec
    /// (`DebugSpec`): `leaf(value, spec, code)` (`DebugLeaf::from_code`).
    FormatDebugLeaf,
    /// A derived `Debug` under a `{:?}` spec, which it passes to each
    /// field: `debug(value, spec)`.
    FormatSpecDebug,
    /// `a.partial_cmp(&b)` by the runtime values (`compare_value`): an
    /// `Option` of `Ordering`'s discriminant.
    Compare,
    /// The runtime type key of a value: a script type's dense definition
    /// index, a builtin's witness code (`builtin_type_key`), else `-1`.
    /// What a trait dispatcher selects the impl by.
    TypeIndex,
    /// `value as i32` of a fieldless script enum: its variant's
    /// discriminant, explicit or one past the previous variant's.
    Discriminant,
    /// `i64::from(bool)`, `f64::from(bool)`, `String::from(char)`: the
    /// prelude's conversions among builtin types.
    IntFromBool,
    FloatFromBool,
    StringFromChar,
    /// `{}`/`{:?}` of an `f64` with an optional precision (`-1` for none),
    /// Rust's text: `format_float(x, precision, debug)`.
    FormatFloat,
    /// `{:x}`, `{:X}`, `{:o}`, `{:b}` of an integer: its digits in the
    /// radix (`16`, `-16` for upper case, `8`, `2`), as Rust prints an `i64`.
    FormatRadix,
    /// `{:x}`, `{:o}`, `{:b}` and an unsigned integer's decimal digits:
    /// `format_bits(x, radix, bits)`, the integer's `bits` low bits as Rust
    /// prints its type.
    FormatBits,
    /// `{:e}`, `{:E}` of a number: `format_exp(x, precision, mode)`, `mode`
    /// `1` for upper case, `2` an `f32`, `4` an integer, `8` an unsigned one.
    FormatExp,
    /// `RangeInclusive`'s methods: `range_method(range, code, value)`, `code`
    /// `0` for `start`, `1` `end`, `2` `contains`, `3` `is_empty`.
    RangeMethod,
    /// `{:#?}`: Rust's pretty `Debug` of a one-line `Debug` text.
    PrettyDebug,
    /// `collect()` into a `String`: the items' text, concatenated.
    CollectString,
    /// `collect()` into a `Vec`: the items' array as it is.
    CollectVec,
    /// Whether `collect()` stops at its item (`ExecutableOp::Kinded`):
    /// `stop(item)`, for the target's code (`COLLECT_RESULT`,
    /// `COLLECT_OPTION`, else never).
    CollectStop,
    /// `collect()` into a `Result` or an `Option` (`ExecutableOp::Kinded`,
    /// the target's code and its payload's kind) of the items up to the
    /// first `Err` or `None`: that one, else the payloads collected into a
    /// `Vec` or a `String`.
    CollectTry,
    /// A map's and a set's operations on its sorted entries (`keyed.rs`),
    /// called as methods so the collection is taken by reference.
    MapInsert,
    MapGet,
    MapRemove,
    MapContainsKey,
    /// `map[&key]`: its value, or Rust's panic.
    MapIndex,
    /// The index of the key's entry, or `-1`.
    MapSlot,
    /// The index of the key's entry, inserting `[key, value]` if absent.
    MapSlotInsert,
    SetInsert,
    SetRemove,
    SetContains,
    /// A map of `[key, value]` items, the last value of a key kept, as
    /// repeated inserts keep it.
    MapFrom,
    /// A set of items, the first of equal items kept.
    SetFrom,
    /// `Reverse(value)` and its `.0`.
    ReverseWrap,
    ReverseInner,
    /// `Cow::Borrowed(value)`, `Cow::Owned(value)`, and a `Cow`'s payload
    /// (`cow.rs`).
    CowBorrowed,
    CowOwned,
    CowValue,
    /// `RefCell::new(value)`, a borrow of a cell (`1` mutable), a cell's or
    /// a borrow's value, a borrow's store, a borrow's release, and
    /// `into_inner`, `replace` and `clone` (`cell.rs`).
    /// An imported value's Rust `Display` text, kept by the bridge.
    DisplayText,
    CellNew,
    /// `Cell::new(value)`.
    PlainCellNew,
    /// A `Box<dyn Error>` of its `Display` and `Debug` text, and one of
    /// those texts (`error_object.rs`).
    ErrorNew,
    ErrorText,
    /// A `parse` error, whose kind its target's kind code gives, boxed as
    /// `dyn Error` by `?`.
    ErrorParse,
    /// `Mutex::new`: a cell whose borrow waits for the one held.
    MutexNew,
    CellBorrow,
    CellGet,
    CellSet,
    CellRelease,
    CellReplace,
    CellClone,
    /// `cell.update(place, rhs, apply)`: a borrow guard's place given
    /// `apply(value, rhs)` of its value, through one call of the guard's
    /// method, as rustc's `*state.write() += rhs` is (D554.7(f)).
    CellUpdate,
    /// `Rc::new`, a pointer's value (std's `Deref`), its strong and weak
    /// counts, `ptr_eq`, `downgrade`, `get_mut`, `make_mut`, `try_unwrap`,
    /// `into_inner` and `unwrap_or_clone`, and `Weak::new`, `upgrade`,
    /// its counts and `ptr_eq` (`counted.rs`).
    CountedNew,
    CountedGet,
    CountedStrong,
    CountedWeakCount,
    CountedPtrEq,
    CountedDowngrade,
    CountedGetMut,
    CountedMakeMut,
    CountedTryUnwrap,
    CountedIntoInner,
    CountedUnwrapOrClone,
    WeakNew,
    WeakUpgrade,
    WeakStrong,
    WeakWeakCount,
    WeakPtrEq,
    /// A string's `lines()`, `split_whitespace()` and `split(pattern)` (a
    /// `char` or a string): the array of its pieces.
    StrLines,
    StrSplitWhitespace,
    /// `v.join(sep)` and `v.concat()` of strings or chars: one `String`.
    Join,
    StrSplit,
    /// An arithmetic result checked against its kind (`kind_code`), with
    /// Rust's overflow panic: `narrow(value, kind, message)`; an `f32`
    /// rounded.
    Narrow,
    /// `value as T` for the kind of `kind_code`: `cast(value, kind)`.
    Cast,
    /// An integer literal past `i64` as the kind of `kind_code` keeps it:
    /// `wide(digits, kind)`, an `i128` or a `u128` boxed, a `u64`'s bits.
    WideLiteral,
    /// An arithmetic result of a `{float}` that inference made an `f32`,
    /// rounded to it: `round(value)`.
    RoundF32,
    /// A `u64` or a `usize` passed to a generic parameter, tagged as one
    /// (`UNSIGNED_TAG`): `unsigned(value)`.
    Unsigned,
    /// `(low..high).step_by(step)` of integers, which visits only the
    /// items it yields: `step_by(low, high, step, flags)`, `flags` bit 0
    /// for `..=` and bit 1 for an unsigned range.
    StepBy,
    /// `StepBy` of an `i128` or a `u128` range (bit 1 of `flags` for a
    /// `u128` one): std's own `StepBy`.
    WideStepBy,
    /// `v[low..high]` of an array or a string (by bytes, on `char`
    /// boundaries): `slice(value, low, high, inclusive)`, `high` `-1` for
    /// an open end; a copy, with Rust's panics.
    Slice,
    /// `v.get(low..high)`: `Slice`'s operands, `Some` of the copy, or
    /// `None` where `Slice` panics.
    SliceGetRange,
    /// `v.get(i)`, `first()` (`0`) and `last()` (`-1`): an `Option` of the
    /// element.
    SliceGet,
    /// `v.split_at(mid)` of an array or a string: the pair of its halves.
    SplitAt,
    /// A `char` method of `CHAR_METHODS`: `char(c, code, argument)`.
    CharMethod,
    /// `char::from_digit(n, radix)`, or `char::from_u32(n)` with radix `-1`.
    CharFrom,
    /// `e.kind()` of a `ParseIntError`, from its message: std's
    /// `IntErrorKind`.
    IntErrorKind,
    /// `c.to_uppercase()`, or `c.to_lowercase()` when its flag is false:
    /// std's `ToUppercase` or `ToLowercase`.
    CaseMapping,
    /// A `Vec` changed in place (`VEC_METHODS`), called as a method so the
    /// array is taken by reference: `vec(code, a, b, c)`.
    VecMethod,
    /// `v.capacity()`: the runtime `Vec`'s own capacity, read by reference.
    VecCapacity,
    /// What a scope frame's call changed of the `&mut` array it took at
    /// `index`, once the frame has ended (D558 slice 2): `copy(made, i)`.
    FrameCopy,
    /// Text lent at `'static` where a crate's call borrows it for its
    /// output's life, as a literal is: `lend(text)`, marked so the call
    /// pins it for the runner's life and opens no scope frame.
    StaticText,
    /// A set's `union`, `intersection`, `difference`,
    /// `symmetric_difference` (the keys, in order), `is_subset`,
    /// `is_superset` or `is_disjoint` with another, by code
    /// (`SET_ALGEBRA`): `set(other, code)`.
    SetAlgebra,
    /// A B-tree map's or set's `range`: its entries from `low` to `high`
    /// (`flags`: 1 a low bound, 2 a high bound, 4 inclusive, 8 a map).
    KeyedRange,
    /// An integer method of `INT_METHODS` or a bit operator, for its kinds
    /// (`ExecutableOp::Kinded`): `int(value, code, operand)`.
    IntMethod,
    /// A `std::time` function, method or operator of `TIME_METHODS`:
    /// `time(value, code, operand)`.
    Time,
    /// A `std::thread` or `std::sync::mpsc` function or method of
    /// `thread::METHODS`: `thread(value, code, operand)` (`thread.rs`).
    Thread,
    /// A `str` method of `STR_METHODS`: `str(text, code, a, b)`.
    StrMethod,
    /// `s.parse::<T>()` for the target kind (`ExecutableOp::Kinded`): a
    /// `Result` of the value or the std error's message.
    Parse,
    /// `print!`, `println!`, `eprint!`, `eprintln!`: `print(text, stderr)`,
    /// through the engine's output callback.
    Print,
    /// A `String` changed in place (`STR_CHANGES`), called as a method so
    /// the string is taken by reference: `string(code, a, b)`.
    StrChange,
    /// A string's pieces or bytes (`STR_PIECES`): `pieces(text, code, a, b)`.
    StrPieces,
    /// `v.chunks(n)`, `windows(n)`, `chunks_exact(n)`, `rchunks(n)` or
    /// `rchunks_exact(n)`, by its code in `SLICE_CHUNKS`: the array of its
    /// slices.
    SliceChunks,
    /// The value at a path below a place, and the value stored there,
    /// called as methods on the place (`places::path_invoke`): how a
    /// script function's `&mut` output is read and written.
    PathGet,
    PathSet,
    /// Store through a checked reference; native places write immediately.
    ReferentStore,
    /// Whether a checked argument uses direct native place transport.
    NativePlace,
    /// Whether a `Memory` swap exchanged two native places where they are,
    /// leaving nothing to store back.
    Swapped,
    /// Ownership-preserving `mem::take`, `replace` and `swap` on sequences/text.
    Memory,
    /// The parts of a value the drop glue goes on to after the value's own
    /// `Drop` (`drops.rs`): its fields, payload or elements in Rust's drop
    /// order, each that holds a value of a type with `impl Drop`.
    DropParts,
    /// `Box<dyn Trait>` of a native trait, made of a box of the program's
    /// own type that implements it: `object(value, owner, trait)`, the
    /// stand-in rawr's copy of the crate declares for the owner, holding
    /// the value's handle (`island::object`, #1264).
    NativeObject,
    /// A static declared in a body, of a native type (D862): its place,
    /// or before its block writes it, its slot: `value(key)`.
    StaticValue,
    /// Whether the static `key` has no value yet: `reserved(key)`.
    StaticReserved,
    /// Write the static `key`'s value, moved into its place, with its
    /// type's layout, `[size, pointer offsets..]` or `()` when the import
    /// driver gave none (D865): `settle(key, value, layout)`.
    StaticSettle,
}

impl PreludeOp {
    pub(super) const fn name(self) -> &'static str {
        match self {
            Self::Some => "option.some",
            Self::None => "option.none",
            Self::IsSome => "option.is-some",
            Self::Unwrap => "option.unwrap",
            Self::Expect => "option.expect",
            Self::Ok => "result.ok",
            Self::Err => "result.err",
            Self::IsOk => "result.is-ok",
            Self::UnwrapOk => "result.unwrap",
            Self::UnwrapErr => "result.unwrap-err",
            Self::ExpectOk => "result.expect",
            Self::ExpectErr => "result.expect-err",
            Self::GenericEqual => "generic.eq",
            Self::GenericDebug => "generic.debug",
            Self::ValueClone => "generic.clone",
            Self::SequenceContains => "sequence.contains",
            Self::FormatDebugLeaf => "format.debug-leaf",
            Self::FormatSpecDebug => "format.spec-debug",
            Self::Compare => "generic.partial-cmp",
            Self::TypeIndex => "generic.type-index",
            Self::Discriminant => "enum.discriminant",
            Self::IntFromBool => "from.i64-bool",
            Self::FloatFromBool => "from.f64-bool",
            Self::StringFromChar => "from.string-char",
            Self::FormatFloat => "format.float",
            Self::FormatRadix => "format.radix",
            Self::FormatBits => "format.bits",
            Self::FormatExp => "format.exp",
            Self::RangeMethod => "range.method",
            Self::PrettyDebug => "format.pretty-debug",
            Self::CollectString => "iter.collect-string",
            Self::CollectVec => "iter.collect-vec",
            Self::CollectStop => "iter.collect-stop",
            Self::CollectTry => "iter.collect-try",
            Self::MapInsert => "map.insert",
            Self::MapGet => "map.get",
            Self::MapRemove => "map.remove",
            Self::MapContainsKey => "map.contains-key",
            Self::MapIndex => "map.index",
            Self::MapSlot => "map.slot",
            Self::MapSlotInsert => "map.slot-insert",
            Self::SetInsert => "set.insert",
            Self::SetRemove => "set.remove",
            Self::SetContains => "set.contains",
            Self::MapFrom => "map.from",
            Self::SetFrom => "set.from",
            Self::ReverseWrap => "reverse.new",
            Self::CowBorrowed => "cow.borrowed",
            Self::CowOwned => "cow.owned",
            Self::CowValue => "cow.value",
            Self::DisplayText => "format.display",
            Self::CellNew => "cell.new",
            Self::MutexNew => "cell.new_mutex",
            Self::PlainCellNew => "cell.new_plain",
            Self::ErrorNew => "error.new",
            Self::ErrorText => "error.text",
            Self::ErrorParse => "error.parse",
            Self::CellBorrow => "cell.borrow",
            Self::CellGet => "cell.get",
            Self::CellSet => "cell.set",
            Self::CellRelease => "cell.release",
            Self::CellReplace => "cell.replace",
            Self::CellClone => "cell.clone",
            Self::CellUpdate => "cell.update",
            Self::CountedNew => "counted.new",
            Self::CountedGet => "counted.get",
            Self::CountedStrong => "counted.strong_count",
            Self::CountedWeakCount => "counted.weak_count",
            Self::CountedPtrEq => "counted.ptr_eq",
            Self::CountedDowngrade => "counted.downgrade",
            Self::CountedGetMut => "counted.get_mut",
            Self::CountedMakeMut => "counted.make_mut",
            Self::CountedTryUnwrap => "counted.try_unwrap",
            Self::CountedIntoInner => "counted.into_inner",
            Self::CountedUnwrapOrClone => "counted.unwrap_or_clone",
            Self::WeakNew => "weak.new",
            Self::WeakUpgrade => "weak.upgrade",
            Self::WeakStrong => "weak.strong_count",
            Self::WeakWeakCount => "weak.weak_count",
            Self::WeakPtrEq => "weak.ptr_eq",
            Self::ReverseInner => "reverse.0",
            Self::StrLines => "str.lines",
            Self::StrSplitWhitespace => "str.split-whitespace",
            Self::Join => "slice.join",
            Self::StrSplit => "str.split",
            Self::Narrow => "number.narrow",
            Self::RoundF32 => "number.round-f32",
            Self::Unsigned => "number.unsigned",
            Self::StepBy => "range.step-by",
            Self::WideStepBy => "range.wide-step-by",
            Self::Cast => "number.cast",
            Self::WideLiteral => "number.wide-literal",
            Self::Slice => "slice.range",
            Self::SliceGetRange => "slice.get-range",
            Self::SliceGet => "slice.get",
            Self::SplitAt => "slice.split-at",
            Self::CharMethod => "char.method",
            Self::CharFrom => "char.from",
            Self::IntErrorKind => "error.int-kind",
            Self::CaseMapping => "char.case-mapping",
            Self::VecMethod => "vec.method",
            Self::VecCapacity => "vec.capacity",
            Self::FrameCopy => "scope.frame-copy",
            Self::StaticText => "scope.static-text",
            Self::SetAlgebra => "set.algebra",
            Self::KeyedRange => "keyed.range",
            Self::IntMethod => "int.method",
            Self::Time => "time.method",
            Self::Thread => "thread.method",
            Self::Parse => "str.parse",
            Self::StrMethod => "str.method",
            Self::Print => "io.print",
            Self::StrChange => "string.change",
            Self::StrPieces => "str.pieces",
            Self::SliceChunks => "slice.chunks",
            Self::PathGet => "place.get",
            Self::PathSet => "place.set",
            Self::ReferentStore => "place.store",
            Self::NativePlace => "place.native",
            Self::Swapped => "place.swapped",
            Self::Memory => "place.memory",
            Self::DropParts => "drop.parts",
            Self::NativeObject => "island.object",
            Self::StaticValue => "static.value",
            Self::StaticReserved => "static.reserved",
            Self::StaticSettle => "static.settle",
        }
    }

    /// For an operation on a map's or set's entries, called as a method on
    /// the collection by reference: whether it changes them.
    pub(super) const fn by_reference(self) -> Option<bool> {
        match self {
            Self::MapInsert
            | Self::MapRemove
            | Self::MapSlotInsert
            | Self::SetInsert
            | Self::SetRemove
            | Self::VecMethod
            | Self::StrChange
            | Self::PathSet
            | Self::ReferentStore
            | Self::CountedMakeMut
            | Self::Memory => Some(true),
            Self::MapGet
            | Self::MapContainsKey
            | Self::MapIndex
            | Self::MapSlot
            | Self::SetContains
            | Self::SetAlgebra
            | Self::VecCapacity
            | Self::KeyedRange
            | Self::PathGet
            | Self::GenericDebug
            | Self::ValueClone
            | Self::SequenceContains
            | Self::CountedGet
            | Self::CountedStrong
            | Self::CountedWeakCount
            | Self::CountedPtrEq
            | Self::CountedDowngrade
            | Self::CountedGetMut
            | Self::WeakUpgrade
            | Self::WeakStrong
            | Self::WeakWeakCount
            | Self::WeakPtrEq
            | Self::DisplayText => Some(false),
            _ => None,
        }
    }

    pub(super) fn argument_types(self) -> Vec<TypeId> {
        match self {
            Self::None => Vec::new(),
            Self::Some
            | Self::NativePlace
            | Self::Swapped
            | Self::IsSome
            | Self::Unwrap
            | Self::Ok
            | Self::Err
            | Self::IsOk
            | Self::UnwrapOk
            | Self::UnwrapErr
            | Self::GenericDebug
            | Self::ValueClone
            | Self::TypeIndex
            | Self::Discriminant
            | Self::ReverseWrap
            | Self::CowBorrowed
            | Self::CowOwned
            | Self::DisplayText
            | Self::CellNew
            | Self::PlainCellNew
            | Self::MutexNew
            | Self::CellGet
            | Self::CellRelease
            | Self::CellClone
            | Self::CountedNew
            | Self::CountedGet
            | Self::CountedStrong
            | Self::CountedWeakCount
            | Self::CountedDowngrade
            | Self::CountedGetMut
            | Self::CountedMakeMut
            | Self::CountedTryUnwrap
            | Self::CountedIntoInner
            | Self::CountedUnwrapOrClone
            | Self::WeakUpgrade
            | Self::WeakStrong
            | Self::WeakWeakCount => vec![TypeId::of::<Dynamic>()],
            Self::CountedPtrEq | Self::WeakPtrEq => {
                vec![TypeId::of::<Dynamic>(), TypeId::of::<Dynamic>()]
            }
            Self::WeakNew => Vec::new(),
            Self::DropParts
            | Self::CellBorrow
            | Self::CellSet
            | Self::CellReplace
            | Self::ErrorNew
            | Self::ErrorText
            | Self::ReferentStore => {
                vec![TypeId::of::<Dynamic>(), TypeId::of::<Dynamic>()]
            }
            Self::Memory => vec![
                TypeId::of::<Dynamic>(),
                TypeId::of::<INT>(),
                TypeId::of::<Dynamic>(),
            ],
            Self::CellUpdate => vec![
                TypeId::of::<Dynamic>(),
                TypeId::of::<Dynamic>(),
                TypeId::of::<crate::FnPtr>(),
            ],
            Self::ReverseInner => vec![TypeId::of::<std::cmp::Reverse<Dynamic>>()],
            Self::CowValue => vec![TypeId::of::<cow::Value>()],
            Self::IntFromBool | Self::FloatFromBool => vec![TypeId::of::<bool>()],
            Self::StringFromChar => vec![TypeId::of::<char>()],
            Self::FormatFloat => vec![
                TypeId::of::<crate::FLOAT>(),
                TypeId::of::<crate::INT>(),
                TypeId::of::<crate::INT>(),
            ],
            Self::FormatRadix => vec![TypeId::of::<crate::INT>(), TypeId::of::<crate::INT>()],
            Self::FormatDebugLeaf => vec![
                TypeId::of::<Dynamic>(),
                TypeId::of::<crate::Array>(),
                TypeId::of::<crate::INT>(),
            ],
            Self::FormatSpecDebug => vec![TypeId::of::<Dynamic>(), TypeId::of::<crate::Array>()],
            Self::FormatBits => vec![
                TypeId::of::<Dynamic>(),
                TypeId::of::<crate::INT>(),
                TypeId::of::<crate::INT>(),
            ],
            Self::FormatExp => vec![
                TypeId::of::<Dynamic>(),
                TypeId::of::<crate::INT>(),
                TypeId::of::<crate::INT>(),
            ],
            Self::RangeMethod => vec![
                TypeId::of::<Dynamic>(),
                TypeId::of::<crate::INT>(),
                TypeId::of::<Dynamic>(),
            ],
            Self::PrettyDebug | Self::StrLines | Self::StrSplitWhitespace | Self::StaticText => {
                vec![TypeId::of::<ImmutableString>()]
            }
            Self::StrSplit => vec![TypeId::of::<ImmutableString>(), TypeId::of::<Dynamic>()],
            Self::Join => vec![
                TypeId::of::<crate::Array>(),
                TypeId::of::<ImmutableString>(),
            ],
            Self::Narrow => vec![
                TypeId::of::<Dynamic>(),
                TypeId::of::<crate::INT>(),
                TypeId::of::<ImmutableString>(),
            ],
            Self::Cast => vec![TypeId::of::<Dynamic>(), TypeId::of::<crate::INT>()],
            Self::WideLiteral => vec![TypeId::of::<ImmutableString>(), TypeId::of::<crate::INT>()],
            Self::RoundF32 | Self::Unsigned => vec![TypeId::of::<Dynamic>()],
            Self::StepBy => vec![TypeId::of::<crate::INT>(); 4],
            Self::WideStepBy => vec![
                TypeId::of::<Dynamic>(),
                TypeId::of::<Dynamic>(),
                TypeId::of::<crate::INT>(),
                TypeId::of::<crate::INT>(),
            ],
            Self::Parse | Self::ErrorParse => vec![TypeId::of::<ImmutableString>()],
            Self::Print => vec![TypeId::of::<ImmutableString>(), TypeId::of::<bool>()],
            Self::StrChange => vec![
                TypeId::of::<Dynamic>(),
                TypeId::of::<crate::INT>(),
                TypeId::of::<Dynamic>(),
                TypeId::of::<Dynamic>(),
            ],
            Self::StrPieces => vec![
                TypeId::of::<Dynamic>(),
                TypeId::of::<crate::INT>(),
                TypeId::of::<Dynamic>(),
                TypeId::of::<Dynamic>(),
            ],
            Self::SliceChunks => vec![
                TypeId::of::<crate::Array>(),
                TypeId::of::<crate::INT>(),
                TypeId::of::<crate::INT>(),
            ],
            Self::PathGet => vec![TypeId::of::<Dynamic>(), TypeId::of::<crate::Array>()],
            Self::PathSet => vec![
                TypeId::of::<Dynamic>(),
                TypeId::of::<crate::Array>(),
                TypeId::of::<Dynamic>(),
            ],
            Self::StrMethod => vec![
                TypeId::of::<ImmutableString>(),
                TypeId::of::<crate::INT>(),
                TypeId::of::<Dynamic>(),
                TypeId::of::<Dynamic>(),
            ],
            Self::IntMethod => vec![
                TypeId::of::<crate::INT>(),
                TypeId::of::<crate::INT>(),
                TypeId::of::<Dynamic>(),
            ],
            Self::Slice | Self::SliceGetRange => vec![
                TypeId::of::<Dynamic>(),
                TypeId::of::<crate::INT>(),
                TypeId::of::<crate::INT>(),
                TypeId::of::<bool>(),
            ],
            Self::SliceGet | Self::SplitAt => {
                vec![TypeId::of::<Dynamic>(), TypeId::of::<crate::INT>()]
            }
            Self::CharFrom => vec![TypeId::of::<crate::INT>(), TypeId::of::<crate::INT>()],
            Self::IntErrorKind => vec![TypeId::of::<ImmutableString>()],
            Self::CaseMapping => vec![TypeId::of::<char>(), TypeId::of::<bool>()],
            Self::SetAlgebra => vec![
                TypeId::of::<crate::Array>(),
                TypeId::of::<crate::Array>(),
                TypeId::of::<crate::INT>(),
            ],
            Self::KeyedRange => vec![
                TypeId::of::<crate::Array>(),
                TypeId::of::<Dynamic>(),
                TypeId::of::<Dynamic>(),
                TypeId::of::<crate::INT>(),
            ],
            Self::Time | Self::Thread => vec![
                TypeId::of::<Dynamic>(),
                TypeId::of::<crate::INT>(),
                TypeId::of::<Dynamic>(),
            ],
            Self::VecCapacity => vec![TypeId::of::<Dynamic>()],
            Self::FrameCopy => vec![
                TypeId::of::<crate::scope::Opening>(),
                TypeId::of::<crate::INT>(),
            ],
            Self::VecMethod => vec![
                TypeId::of::<Dynamic>(),
                TypeId::of::<crate::INT>(),
                TypeId::of::<Dynamic>(),
                TypeId::of::<Dynamic>(),
                TypeId::of::<Dynamic>(),
            ],
            Self::CharMethod => vec![
                TypeId::of::<char>(),
                TypeId::of::<crate::INT>(),
                TypeId::of::<Dynamic>(),
            ],
            Self::CollectString
            | Self::CollectVec
            | Self::CollectTry
            | Self::MapFrom
            | Self::SetFrom => {
                vec![TypeId::of::<crate::Array>()]
            }
            Self::CollectStop => vec![TypeId::of::<Dynamic>()],
            Self::MapGet
            | Self::MapRemove
            | Self::MapContainsKey
            | Self::MapIndex
            | Self::MapSlot
            | Self::SetInsert
            | Self::SetRemove
            | Self::SetContains => vec![TypeId::of::<crate::Array>(), TypeId::of::<Dynamic>()],
            Self::MapInsert | Self::MapSlotInsert => vec![
                TypeId::of::<crate::Array>(),
                TypeId::of::<Dynamic>(),
                TypeId::of::<Dynamic>(),
            ],
            Self::GenericEqual | Self::Compare | Self::SequenceContains => {
                vec![TypeId::of::<Dynamic>(), TypeId::of::<Dynamic>()]
            }
            Self::Expect | Self::ExpectOk | Self::ExpectErr => {
                vec![TypeId::of::<Dynamic>(), TypeId::of::<ImmutableString>()]
            }
            Self::StaticValue | Self::StaticReserved => vec![TypeId::of::<ImmutableString>()],
            Self::StaticSettle => vec![
                TypeId::of::<ImmutableString>(),
                TypeId::of::<Dynamic>(),
                TypeId::of::<Dynamic>(),
            ],
            Self::NativeObject => vec![
                TypeId::of::<Dynamic>(),
                TypeId::of::<ImmutableString>(),
                TypeId::of::<ImmutableString>(),
            ],
        }
    }

    pub(super) fn invoke(
        self,
        context: Option<&NativeCallContext<'_>>,
        args: &mut FnCallArgs,
        models: &Models,
        shape: &Shape,
        flags: u8,
    ) -> RawrResult {
        let comparison = Comparison {
            models,
            context,
            total: flags & SHAPED_TOTAL != 0,
        };
        let panic = |message: String| -> RawrResult {
            Err(EvalAltResult::ErrorRuntime(message.into(), Position::NONE).into())
        };
        match self {
            Self::Some => return Ok(crate::option::some(args[0].take())),
            Self::NativePlace => return Ok(crate::place::get(args[0]).is_some().into()),
            Self::Swapped => return Ok(args[0].is_unit().into()),
            Self::None => return Ok(crate::option::none()),
            Self::Ok => return Ok(crate::result::ok(args[0].take())),
            Self::Err => return Ok(crate::result::err(args[0].take())),
            Self::SequenceContains => {
                if let Some(place) = crate::place::get(args[0]) {
                    return place.invoke(crate::place::Operation::Contains, &mut [args[1].clone()]);
                }
                let array = args[0]
                    .as_array_ref()
                    .map_err(|_| invariant("a sequence membership receiver is not a sequence"))?;
                for value in array.iter() {
                    if comparison.equal(value, args[1])? {
                        return Ok(true.into());
                    }
                }
                return Ok(false.into());
            }
            Self::GenericEqual => return Ok(comparison.equal(args[0], args[1])?.into()),
            Self::GenericDebug => {
                return match crate::place::get(args[0]) {
                    Some(place) => place.invoke(crate::place::Operation::Debug, &mut []),
                    None => Ok(debug_value(args[0], models).into()),
                }
            }
            Self::ValueClone => {
                if let Some(projected) = args[0].read_lock::<crate::scope::Projected>() {
                    return projected.cloned();
                }
                return match crate::place::get(args[0]) {
                    Some(place) => place.invoke(crate::place::Operation::Clone, &mut []),
                    None => Ok(args[0].clone()),
                };
            }
            Self::FormatDebugLeaf => {
                let spec = DebugSpec::read(args[1]);
                let leaf = DebugLeaf::from_code(args[2].as_int().unwrap_or_default());
                return Ok(ImmutableString::from(spec.leaf(args[0], leaf)).into());
            }
            Self::FormatSpecDebug => {
                let spec = DebugSpec::read(args[1]);
                return Ok(
                    ImmutableString::from(debug_value_spec(args[0], models, Some(&spec))).into(),
                );
            }
            Self::DropParts => return Ok(drops::parts(args[0], args[1], models)),
            Self::StaticValue => {
                let key = args[0].clone_cast::<ImmutableString>();
                return Ok(crate::place::statics::slot(&key).value());
            }
            Self::StaticReserved => {
                let key = args[0].clone_cast::<ImmutableString>();
                return Ok(crate::place::statics::slot(&key).reserved().into());
            }
            Self::StaticSettle => {
                let key = args[0].clone_cast::<ImmutableString>();
                let layout = static_layout(args[2]);
                crate::place::statics::slot(&key).settle(args[1].take(), layout)?;
                return Ok(Dynamic::UNIT);
            }
            #[cfg(feature = "rust-import")]
            Self::NativeObject => {
                let owner = args[1].clone_cast::<ImmutableString>();
                let trait_name = args[2].clone_cast::<ImmutableString>();
                return crate::island::object(args[0].take(), &owner, &trait_name).or_else(&panic);
            }
            // Only a native trait's object makes one, which needs the crate.
            #[cfg(not(feature = "rust-import"))]
            Self::NativeObject => return Err(invariant("a native trait object without a crate")),
            Self::Compare => {
                return Ok(match comparison.compare(args[0], args[1], shape)? {
                    Some(ordering) => crate::option::some((ordering as crate::INT).into()),
                    None => crate::option::none(),
                })
            }
            Self::IntFromBool => {
                return Ok(crate::INT::from(args[0].as_bool().unwrap_or(false)).into())
            }
            Self::FloatFromBool => {
                let value = if args[0].as_bool().unwrap_or(false) {
                    1.0
                } else {
                    0.0
                };
                return Ok(Dynamic::from_float(value));
            }
            Self::StringFromChar => {
                let value = args[0].as_char().unwrap_or_default();
                return Ok(Dynamic::from(ImmutableString::from(value.to_string())));
            }
            Self::FormatFloat => {
                let value = args[0].as_float().unwrap_or_default();
                let mode = args[2].as_int().unwrap_or_default();
                let debug = mode & 1 == 1;
                let precision = usize::try_from(args[1].as_int().unwrap_or(-1));
                let text = match (precision, mode & 2 == 2) {
                    (Ok(precision), _) => format!("{value:.precision$}"),
                    (Err(_), true) if debug => format!("{:?}", value as f32),
                    (Err(_), true) => format!("{}", value as f32),
                    (Err(_), false) if debug => format!("{value:?}"),
                    (Err(_), false) => format!("{value}"),
                };
                return Ok(Dynamic::from(ImmutableString::from(text)));
            }
            Self::FormatRadix => {
                let value = args[0].as_int().unwrap_or_default();
                let text = match args[1].as_int().unwrap_or(16) {
                    // A `u64`'s or a `usize`'s decimal digits, from its bits.
                    10 => (value as u64).to_string(),
                    2 => format!("{value:b}"),
                    8 => format!("{value:o}"),
                    -16 => format!("{value:X}"),
                    _ => format!("{value:x}"),
                };
                return Ok(Dynamic::from(ImmutableString::from(text)));
            }
            Self::FormatBits => {
                let bits = args[2].as_int().unwrap_or(64);
                // An `i128`'s or a `u128`'s 128 bits; others' bits in the
                // runtime's `i64`.
                let value = match wide_value(args[0]) {
                    Some(value) if bits == 128 => value.unsigned(),
                    _ => u128::from(args[0].as_int().unwrap_or_default() as u64),
                };
                let value = if bits >= 64 {
                    value
                } else {
                    value & ((1 << bits) - 1)
                };
                let text = match args[1].as_int().unwrap_or(16) {
                    10 => value.to_string(),
                    2 => format!("{value:b}"),
                    8 => format!("{value:o}"),
                    -16 => format!("{value:X}"),
                    _ => format!("{value:x}"),
                };
                return Ok(Dynamic::from(ImmutableString::from(text)));
            }
            Self::FormatExp => {
                let precision = usize::try_from(args[1].as_int().unwrap_or(-1)).ok();
                let mode = args[2].as_int().unwrap_or_default();
                let upper = mode & 1 == 1;
                macro_rules! exp {
                    ($value:expr) => {
                        match (precision, upper) {
                            (Some(precision), false) => format!("{:.precision$e}", $value),
                            (Some(precision), true) => format!("{:.precision$E}", $value),
                            (None, false) => format!("{:e}", $value),
                            (None, true) => format!("{:E}", $value),
                        }
                    };
                }
                let text = if mode & 4 == 4
                    && let Some(Wide::Signed(value)) =
                        wide_value(args[0]).filter(|_| args[0].as_int().is_err())
                {
                    exp!(value)
                } else if mode & 4 == 4
                    && let Some(Wide::Unsigned(value)) = wide_value(args[0])
                {
                    exp!(value)
                } else if mode & 4 == 4 {
                    let value = args[0].as_int().unwrap_or_default();
                    if mode & 8 == 8 {
                        exp!(value as u64)
                    } else {
                        exp!(value)
                    }
                } else {
                    {
                        let value = args[0].as_float().unwrap_or_default();
                        if mode & 2 == 2 {
                            exp!(value as f32)
                        } else {
                            exp!(value)
                        }
                    }
                };
                return Ok(Dynamic::from(ImmutableString::from(text)));
            }
            Self::RangeMethod => {
                let code = args[1].as_int().unwrap_or_default();
                if let Some(range) = args[0].read_lock::<std::ops::RangeInclusive<INT>>() {
                    let value = args[2].as_int().unwrap_or_default();
                    return Ok(match code {
                        0 => Dynamic::from(*range.start()),
                        1 => Dynamic::from(*range.end()),
                        2 => Dynamic::from(range.contains(&value)),
                        _ => Dynamic::from(range.is_empty()),
                    });
                }
                if let Some(range) = args[0].read_lock::<std::ops::RangeInclusive<crate::FLOAT>>() {
                    let value = args[2].as_float().unwrap_or_default();
                    return Ok(match code {
                        0 => Dynamic::from_float(*range.start()),
                        1 => Dynamic::from_float(*range.end()),
                        2 => Dynamic::from(range.contains(&value)),
                        _ => Dynamic::from(range.is_empty()),
                    });
                }
                return Err(invariant("a range method read a value that is not a range"));
            }
            Self::PrettyDebug => {
                let text = args[0].clone_cast::<ImmutableString>();
                return Ok(Dynamic::from(ImmutableString::from(pretty_debug(&text))));
            }
            Self::CollectVec => return Ok(args[0].take()),
            Self::Narrow => {
                let code = args[1].as_int().unwrap_or(3);
                if let Ok(value) = args[0].as_int()
                    && let Some(kind) = Int::from_code(code)
                {
                    let (low, high) = kind.range();
                    if !(low..=high).contains(&value) {
                        return panic(args[2].clone_cast::<ImmutableString>().to_string());
                    }
                    return Ok(args[0].take());
                }
                return cast_value(args[0], code);
            }
            Self::Cast => return cast_value(args[0], args[1].as_int().unwrap_or(3)),
            Self::WideLiteral => {
                let text = args[0].clone_cast::<ImmutableString>();
                let kind = Int::from_code(args[1].as_int().unwrap_or(3)).unwrap_or(Int::I64);
                return match integer_text(&text) {
                    Some(value) => Ok(value.store(kind)),
                    None => Err(invariant("a wide literal is not a number")),
                };
            }
            Self::RoundF32 => return cast_value(args[0], 10),
            Self::Unsigned => return Ok(compare::tag_unsigned(args[0].take(), shape)),
            Self::WideStepBy => {
                let [step, flags] = [2, 3].map(|index| args[index].as_int().unwrap_or_default());
                if step == 0 {
                    return panic("assertion failed: step != 0".into());
                }
                let step = usize::try_from(step).unwrap_or(usize::MAX);
                let (low, high) = (
                    wide_value(args[0]).unwrap_or(Wide::Signed(0)),
                    wide_value(args[1]).unwrap_or(Wide::Signed(0)),
                );
                return Ok(match (flags & 2 == 2, flags & 1 == 1) {
                    (true, true) => Dynamic::from((low.unsigned()..=high.unsigned()).step_by(step)),
                    (true, false) => Dynamic::from((low.unsigned()..high.unsigned()).step_by(step)),
                    (false, true) => Dynamic::from((low.signed()..=high.signed()).step_by(step)),
                    (false, false) => Dynamic::from((low.signed()..high.signed()).step_by(step)),
                });
            }
            Self::StepBy => {
                let [low, high, step, flags] =
                    [0, 1, 2, 3].map(|index| args[index].as_int().unwrap_or_default());
                if step == 0 {
                    return panic("assertion failed: step != 0".into());
                }
                return Ok(Dynamic::from(crate::packages::iter_basic::StepBy::new(
                    low,
                    high,
                    step as u64,
                    flags & 1 == 1,
                    flags & 2 == 2,
                )));
            }
            Self::Slice | Self::SliceGetRange | Self::SplitAt => {
                return slice_value(self, args);
            }
            Self::VecMethod => return vec_method(context, args),
            Self::PathGet | Self::PathSet => return places::path_invoke(self, args),
            Self::Memory => {
                let mode = args[1].as_int().unwrap_or(-1);
                let incoming = args[2].take();
                if let Some(place) = crate::place::get(args[0]) {
                    return match mode {
                        0 => place.invoke(crate::place::Operation::Take, &mut []),
                        1 => place.invoke(crate::place::Operation::Replace, &mut [incoming]),
                        2 if crate::place::get(&incoming).is_some() => {
                            place.invoke(crate::place::Operation::SwapValues, &mut [incoming])
                        }
                        2 => place.invoke(crate::place::Operation::Replace, &mut [incoming]),
                        _ => Err(invariant("unknown native memory operation")),
                    };
                }
                let incoming = if mode == 2
                    && let Some(other) = crate::place::get(&incoming)
                {
                    other.invoke(crate::place::Operation::Take, &mut [])?
                } else {
                    incoming
                };
                return Ok(std::mem::replace(args[0], incoming));
            }
            Self::ReferentStore => {
                let incoming = args[1].take();
                if let Some(place) = crate::place::get(args[0]) {
                    if crate::place::get(&incoming).is_some_and(|other| place.same_storage(&other))
                    {
                        return Ok(Dynamic::UNIT);
                    }
                    return place.invoke(crate::place::Operation::Store, &mut [incoming]);
                }
                *args[0] = incoming;
                return Ok(Dynamic::UNIT);
            }
            Self::VecCapacity => {
                if let Some(place) = crate::place::get(args[0]) {
                    return place.invoke(crate::place::Operation::Capacity, &mut []);
                }
                let capacity = args[0].as_array_ref().map_or(0, |array| array.capacity());
                return Ok(Dynamic::from(capacity as crate::INT));
            }
            Self::StaticText => return Ok(crate::scope::lend_static(args[0].take())),
            Self::FrameCopy => {
                let index = usize::try_from(args[1].as_int().unwrap_or(0)).unwrap_or(0);
                return Ok(args[0]
                    .read_lock::<crate::scope::Opening>()
                    .map_or(Dynamic::UNIT, |opening| opening.copies().get(index)));
            }
            Self::Time => return time_method(args),
            Self::Thread => return thread::invoke(context, args),
            Self::StrChange => return str_change(context, args),
            Self::StrPieces => return str_pieces(args),
            Self::SliceChunks => {
                let size = usize::try_from(args[1].as_int().unwrap_or(0)).unwrap_or(0);
                let code = usize::try_from(args[2].as_int().unwrap_or(0)).unwrap_or(0);
                let name = SLICE_CHUNKS.get(code).copied().unwrap_or_default();
                if size == 0 {
                    return panic(if name == "windows" {
                        "window size must be non-zero".into()
                    } else {
                        "chunk size must be non-zero".into()
                    });
                }
                let array = args[0]
                    .as_array_ref()
                    .map_err(|_| invariant("a slice is not an array at runtime"))?;
                let slice = |items: &[Dynamic]| Dynamic::from_array(items.to_vec());
                let slices: crate::Array = match name {
                    "windows" => array.windows(size).map(slice).collect(),
                    "chunks_exact" => array.chunks_exact(size).map(slice).collect(),
                    "rchunks" => array.rchunks(size).map(slice).collect(),
                    "rchunks_exact" => array.rchunks_exact(size).map(slice).collect(),
                    _ => array.chunks(size).map(slice).collect(),
                };
                return Ok(Dynamic::from_array(slices));
            }
            Self::IntMethod => return int_method([3, 3], args),
            // Kinded by the target; bare, a `Vec` target's.
            Self::CollectStop => return Ok(false.into()),
            Self::CollectTry => return Ok(args[0].take()),
            Self::Parse => return Ok(parse_text(&args[0].clone_cast::<ImmutableString>(), 3)),
            // The engine's output callback needs the call context.
            Self::Print => return Err(invariant("`print!` runs with its call context")),
            Self::StrMethod => {
                let text = args[0].clone_cast::<ImmutableString>();
                let code = usize::try_from(args[1].as_int().unwrap_or_default()).unwrap_or(0);
                return str_method(&text, code, &args[2].clone(), &args[3].clone());
            }
            Self::SetAlgebra | Self::KeyedRange => {
                return keyed_query(self, args, &Keys::new(&comparison, shape, flags, false))
            }
            Self::CharFrom => {
                let n = u32::try_from(args[0].as_int().unwrap_or(-1)).ok();
                let c = match args[1].as_int().unwrap_or(-1) {
                    -1 => n.and_then(char::from_u32),
                    radix => match (n, u32::try_from(radix)) {
                        (Some(n), Ok(radix @ 2..=36)) => char::from_digit(n, radix),
                        _ => return panic("from_digit: radix is too high (maximum 36)".into()),
                    },
                };
                return Ok(match c {
                    Some(c) => crate::option::some(c.into()),
                    None => crate::option::none(),
                });
            }
            Self::IntErrorKind => {
                let message = args[0].clone_cast::<ImmutableString>();
                return crate::std_facade::int_error_kind(&message)
                    .map(Dynamic::from)
                    .ok_or_else(|| invariant("a `ParseIntError` has std's message"));
            }
            Self::CaseMapping => {
                let c = args[0].as_char().unwrap_or_default();
                let upper = args[1].as_bool().unwrap_or_default();
                return Ok(crate::std_facade::case_mapped(c, upper));
            }
            Self::CharMethod => {
                let c = args[0].as_char().unwrap_or_default();
                let code = usize::try_from(args[1].as_int().unwrap_or_default()).unwrap_or(0);
                let name = CHAR_METHODS.get(code).map_or("", |(name, _)| name);
                return Ok(char_method(c, name, args[2]));
            }
            Self::SliceGet => {
                let array = args[0]
                    .as_array_ref()
                    .map_err(|_| invariant("a slice is not an array at runtime"))?;
                let index = args[1].as_int().unwrap_or_default();
                let index = if index == -1 {
                    array.len().checked_sub(1)
                } else {
                    usize::try_from(index).ok()
                };
                return Ok(match index.and_then(|index| array.get(index)) {
                    Some(element) => crate::option::some(element.clone()),
                    None => crate::option::none(),
                });
            }
            Self::ReverseWrap => return Ok(Dynamic::from(std::cmp::Reverse(args[0].take()))),
            Self::CowBorrowed | Self::CowOwned | Self::CowValue => return cow::run(self, args[0]),
            Self::DisplayText => {
                return match crate::place::get(args[0]) {
                    Some(place) => place.invoke(crate::place::Operation::Display, &mut []),
                    None => Ok(args[0].to_string().into()),
                }
            }
            Self::CellNew
            | Self::PlainCellNew
            | Self::MutexNew
            | Self::CellBorrow
            | Self::CellGet
            | Self::CellSet
            | Self::CellRelease
            | Self::CellReplace
            | Self::CellClone => return cell::invoke(self, args),
            Self::ErrorNew | Self::ErrorText => return error_object::invoke(self, args),
            Self::ErrorParse => {
                return Ok(error_object::parsed(
                    &args[0].clone_cast::<ImmutableString>(),
                    3,
                ));
            }
            Self::CellUpdate => return cell::update(context, args),
            Self::CountedNew
            | Self::CountedGet
            | Self::CountedStrong
            | Self::CountedWeakCount
            | Self::CountedPtrEq
            | Self::CountedDowngrade
            | Self::CountedGetMut
            | Self::CountedMakeMut
            | Self::CountedTryUnwrap
            | Self::CountedIntoInner
            | Self::CountedUnwrapOrClone
            | Self::WeakNew
            | Self::WeakUpgrade
            | Self::WeakStrong
            | Self::WeakWeakCount
            | Self::WeakPtrEq => return counted::invoke(self, args),
            Self::Join => {
                let array = args[0]
                    .as_array_ref()
                    .map_err(|_| invariant("a slice is not an array at runtime"))?;
                let separator = args[1].clone_cast::<ImmutableString>();
                let mut joined = String::new();
                for (index, piece) in array.iter().enumerate() {
                    if index > 0 {
                        joined.push_str(&separator);
                    }
                    match piece.as_char() {
                        Ok(c) => joined.push(c),
                        Err(_) => joined.push_str(&piece.clone_cast::<ImmutableString>()),
                    }
                }
                return Ok(Dynamic::from(ImmutableString::from(joined)));
            }
            Self::StrLines | Self::StrSplitWhitespace | Self::StrSplit => {
                let text = args[0].clone_cast::<ImmutableString>();
                let piece = |piece: &str| Dynamic::from(ImmutableString::from(piece));
                let pieces: crate::Array = match self {
                    Self::StrLines => text.lines().map(piece).collect(),
                    Self::StrSplitWhitespace => text.split_whitespace().map(piece).collect(),
                    _ => match args[1].as_char() {
                        Ok(pattern) => text.split(pattern).map(piece).collect(),
                        Err(_) => {
                            let pattern = args[1].clone_cast::<ImmutableString>();
                            text.split(pattern.as_str()).map(piece).collect()
                        }
                    },
                };
                return Ok(Dynamic::from_array(pieces));
            }
            Self::ReverseInner => {
                return args[0]
                    .take()
                    .try_cast::<std::cmp::Reverse<Dynamic>>()
                    .map(|reverse| reverse.0)
                    .ok_or_else(|| invariant("a `Reverse` operand is not one at runtime"));
            }
            Self::MapFrom | Self::SetFrom => {
                let mut items = args[0]
                    .take()
                    .into_array()
                    .map_err(|_| invariant("collected items are not an array at runtime"))?;
                let map = self == Self::MapFrom;
                let keys = Keys::new(&comparison, shape, flags, map);
                items.sort_by(|lhs, rhs| keys.order_entries(lhs, rhs));
                let mut entries = crate::Array::with_capacity(items.len());
                for item in items {
                    let same = match keys.hashed {
                        true => entries
                            .iter()
                            .position(|entry| keys.same_entries(entry, &item)),
                        false => entries
                            .last()
                            .filter(|last| keys.order_entries(last, &item).is_eq())
                            .map(|_| entries.len() - 1),
                    };
                    match same.and_then(|at| entries.get_mut(at)) {
                        // A hashed collection keeps the key it has and takes
                        // the last value, as repeated inserts do; a B-tree's
                        // `from_iter` keeps the last of equal keys.
                        Some(last) if keys.hashed => {
                            if map
                                && let (Ok(mut last), Ok(mut item)) = (
                                    last.as_array_mut(),
                                    item.as_array_ref().map(|pair| pair.clone()),
                                )
                            {
                                last[1] = item.pop().unwrap_or_default();
                            }
                        }
                        Some(last) => *last = item,
                        None => entries.push(item),
                    }
                }
                keys.finish()?;
                return Ok(Dynamic::from_array(entries));
            }
            Self::MapInsert
            | Self::MapGet
            | Self::MapRemove
            | Self::MapContainsKey
            | Self::MapIndex
            | Self::MapSlot
            | Self::MapSlotInsert
            | Self::SetInsert
            | Self::SetRemove
            | Self::SetContains => {
                return keyed_operation(self, args, &Keys::new(&comparison, shape, flags, true))
            }
            Self::CollectString => {
                let mut text = String::new();
                for item in args[0]
                    .read_lock::<crate::Array>()
                    .iter()
                    .flat_map(|items| items.iter())
                {
                    match item.as_char() {
                        Ok(character) => text.push(character),
                        Err(_) => text.push_str(&item.to_string()),
                    }
                }
                return Ok(Dynamic::from(ImmutableString::from(text)));
            }
            Self::Discriminant => {
                let Union::Enum(value, ..) = &args[0].0 else {
                    return Err(invariant("an enum cast operand is not an enum at runtime"));
                };
                let Some(Nominal::Enum { item, .. }) = models.by_identity.get(value.identity())
                else {
                    return Err(invariant("an enum cast operand has no enum model"));
                };
                let mut next: crate::INT = 0;
                for variant in &item.variants {
                    let discriminant = variant.discriminant.unwrap_or(next);
                    if variant.name.text == value.variant().name() {
                        return Ok(discriminant.into());
                    }
                    next = discriminant.wrapping_add(1);
                }
                return Err(invariant("an enum cast operand has an unknown variant"));
            }
            Self::TypeIndex => {
                let identity = match &args[0].0 {
                    Union::Struct(value, ..) => value.identity(),
                    Union::Enum(value, ..) => value.identity(),
                    _ => return Ok(builtin_type_key(args[0]).unwrap_or(-1).into()),
                };
                let index = models
                    .by_identity
                    .get(identity)
                    .and_then(|nominal| {
                        crate::INT::try_from(nominal.definition().id().index()).ok()
                    })
                    .unwrap_or(-1);
                return Ok(index.into());
            }
            Self::IsSome | Self::Unwrap | Self::Expect => {}
            Self::IsOk | Self::UnwrapOk | Self::UnwrapErr | Self::ExpectOk | Self::ExpectErr => {
                let result = crate::result::read(args[0])
                    .ok_or_else(|| invariant("a `Result` operand is not a result at runtime"))?;
                // Rust's messages: the expectation, then the other value's `Debug`.
                return match (self, result) {
                    (Self::IsOk, result) => Ok(result.is_ok().into()),
                    (Self::UnwrapOk | Self::ExpectOk, Ok(value))
                    | (Self::UnwrapErr | Self::ExpectErr, Err(value)) => Ok(value),
                    (Self::UnwrapOk, Err(error)) => panic(format!(
                        "called `Result::unwrap()` on an `Err` value: {}",
                        debug_value(&error, models)
                    )),
                    (Self::UnwrapErr, Ok(value)) => panic(format!(
                        "called `Result::unwrap_err()` on an `Ok` value: {}",
                        debug_value(&value, models)
                    )),
                    (_, Ok(other) | Err(other)) => panic(format!(
                        "{}: {}",
                        args[1].clone_cast::<ImmutableString>(),
                        debug_value(&other, models)
                    )),
                };
            }
        }
        // `unwrap` and `expect` consume their operand, so its value moves out.
        let option = match self {
            Self::IsSome => crate::option::read(args[0]),
            _ => crate::option::take(args[0]),
        }
        .ok_or_else(|| invariant("an `Option` operand is not an option at runtime"))?;
        match (self, option) {
            (Self::IsSome, option) => Ok(option.is_some().into()),
            (_, Some(value)) => Ok(value),
            (Self::Unwrap, None) => panic("called `Option::unwrap()` on a `None` value".into()),
            (_, None) => panic(args[1].clone_cast::<ImmutableString>().to_string()),
        }
    }
}

/// The key of a map's `[key, value]` entry or a set's entry.
pub(super) fn entry_key(entry: &Dynamic, map: bool) -> Dynamic {
    if map {
        entry
            .as_array_ref()
            .ok()
            .and_then(|pair| pair.first().cloned())
            .unwrap_or_default()
    } else {
        entry.clone()
    }
}

/// How a map or a set compares its keys: by Rust's `Ord` of their type,
/// at their shape, or by its `eq` for a hashed collection whose key type
/// has a hand-written `PartialEq`. A crate type's own `cmp` or `eq` may
/// panic; the first such error is kept and reported by `finish`.
pub(super) struct Keys<'k, 'a, 'c> {
    pub(super) comparison: &'k Comparison<'a, 'c>,
    pub(super) shape: &'k Shape,
    pub(super) hashed: bool,
    pub(super) map: bool,
    pub(super) failure: std::cell::RefCell<Option<crate::RawrError>>,
}

impl<'k, 'a, 'c> Keys<'k, 'a, 'c> {
    pub(super) fn new(
        comparison: &'k Comparison<'a, 'c>,
        shape: &'k Shape,
        flags: u8,
        map: bool,
    ) -> Self {
        Self {
            comparison,
            shape,
            hashed: flags & SHAPED_HASHED != 0,
            map,
            failure: std::cell::RefCell::new(None),
        }
    }

    pub(super) fn keep<T>(&self, result: Result<T, crate::RawrError>, otherwise: T) -> T {
        result.unwrap_or_else(|error| {
            self.failure.borrow_mut().get_or_insert(error);
            otherwise
        })
    }

    /// Two keys in order.
    pub(super) fn order(&self, lhs: &Dynamic, rhs: &Dynamic) -> std::cmp::Ordering {
        let total = Comparison {
            models: self.comparison.models,
            context: self.comparison.context,
            total: true,
        };
        self.keep(total.compare(lhs, rhs, self.shape), None)
            .unwrap_or(std::cmp::Ordering::Equal)
    }

    /// Whether two keys are the same key.
    pub(super) fn same(&self, lhs: &Dynamic, rhs: &Dynamic) -> bool {
        if self.hashed {
            self.keep(self.comparison.equal(lhs, rhs), false)
        } else {
            self.order(lhs, rhs).is_eq()
        }
    }

    pub(super) fn order_entries(&self, lhs: &Dynamic, rhs: &Dynamic) -> std::cmp::Ordering {
        self.order(&entry_key(lhs, self.map), &entry_key(rhs, self.map))
    }

    pub(super) fn same_entries(&self, lhs: &Dynamic, rhs: &Dynamic) -> bool {
        self.same(&entry_key(lhs, self.map), &entry_key(rhs, self.map))
    }

    pub(super) fn finish(&self) -> Result<(), crate::RawrError> {
        match self.failure.borrow_mut().take() {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

/// A map's or set's operation on its entries, sorted by key: the key's
/// entry is found by binary search, so a lookup is O(log n) and an insert
/// or a removal shifts the entries after it.
pub(super) fn keyed_operation(op: PreludeOp, args: &mut FnCallArgs, keys: &Keys) -> RawrResult {
    let map = !matches!(
        op,
        PreludeOp::SetInsert | PreludeOp::SetRemove | PreludeOp::SetContains
    );
    let key = args.get(1).map(|key| (**key).clone()).unwrap_or_default();
    let value = args
        .get_mut(2)
        .map(|value| value.take())
        .unwrap_or_default();
    let mut entries = args[0]
        .as_array_mut()
        .map_err(|_| invariant("a map or set is not an array at runtime"))?;
    let entry_key = |entry: &Dynamic| entry_key(entry, map);
    // A hashed key type with its own `eq`: the entry its `eq` finds, else
    // the place its order gives.
    let found = keys
        .hashed
        .then(|| {
            entries
                .iter()
                .position(|entry| keys.same(&entry_key(entry), &key))
        })
        .flatten();
    let slot = match found {
        Some(at) => Ok(at),
        None => match entries.binary_search_by(|entry| keys.order(&entry_key(entry), &key)) {
            Ok(at) if keys.hashed => Err(at),
            slot => slot,
        },
    };
    keys.finish()?;
    let value_of = |entry: &Dynamic| {
        entry
            .as_array_ref()
            .ok()
            .and_then(|pair| pair.get(1).cloned())
            .unwrap_or_default()
    };
    let index = |at: usize| Dynamic::from(at as crate::INT);
    Ok(match (op, slot) {
        (PreludeOp::MapInsert, Ok(at)) => {
            let mut pair = entries[at]
                .as_array_mut()
                .map_err(|_| invariant("a map entry is not a pair at runtime"))?;
            crate::option::some(std::mem::replace(&mut pair[1], value))
        }
        (PreludeOp::MapInsert, Err(at)) => {
            entries.insert(at, Dynamic::from_array(vec![key, value]));
            crate::option::none()
        }
        (PreludeOp::MapGet, Ok(at)) => crate::option::some(value_of(&entries[at])),
        (PreludeOp::MapRemove, Ok(at)) => crate::option::some(value_of(&entries.remove(at))),
        (PreludeOp::MapGet | PreludeOp::MapRemove, Err(_)) => crate::option::none(),
        (PreludeOp::MapIndex, Ok(at)) => value_of(&entries[at]),
        (PreludeOp::MapIndex, Err(_)) => {
            return Err(EvalAltResult::ErrorRuntime("key not found".into(), Position::NONE).into())
        }
        (PreludeOp::MapSlot | PreludeOp::MapSlotInsert, Ok(at)) => index(at),
        (PreludeOp::MapSlot, Err(_)) => Dynamic::from(-1 as crate::INT),
        (PreludeOp::MapSlotInsert, Err(at)) => {
            entries.insert(at, Dynamic::from_array(vec![key, value]));
            index(at)
        }
        (PreludeOp::SetInsert, Err(at)) => {
            entries.insert(at, key);
            true.into()
        }
        (PreludeOp::SetRemove, Ok(at)) => {
            let _ = entries.remove(at);
            true.into()
        }
        (PreludeOp::SetInsert | PreludeOp::SetRemove, _) => false.into(),
        (PreludeOp::MapContainsKey | PreludeOp::SetContains, slot) => slot.is_ok().into(),
        _ => return Err(invariant("not an operation on a map or set")),
    })
}

/// The type a method's impl is for, its parameters instantiated by
/// `substitution`: `Pair<T>` of `impl<T> Pair<T>`, `Vec<T>` of
/// `impl<T: Shape> Shape for Vec<T>`.
pub(super) fn impl_self_ty(
    definition: &method::Definition,
    substitution: &BTreeMap<ImmutableString, Ty>,
) -> Ty {
    if let Some(owner) = definition.builtin_owner() {
        return instantiate(&Ty::from_resolved(owner), substitution);
    }
    Ty::Script(
        definition.owner(),
        definition
            .owner_arguments()
            .iter()
            .map(|argument| instantiate(&Ty::from_resolved(argument), substitution))
            .collect(),
    )
}

/// The float methods without arguments, each with the runtime's function
/// that computes it as Rust does.
pub(super) const FLOAT_UNARY_METHODS: &[(&str, &str)] = &[
    ("abs", "abs"),
    ("sqrt", "sqrt"),
    ("floor", "floor"),
    ("ceil", "ceiling"),
    ("round", "round"),
    ("trunc", "int"),
    ("fract", "fraction"),
    ("is_nan", "is_nan"),
    ("is_finite", "is_finite"),
    ("is_infinite", "is_infinite"),
    ("sin", "sin"),
    ("cos", "cos"),
    ("tan", "tan"),
    ("asin", "asin"),
    ("acos", "acos"),
    ("atan", "atan"),
    ("sinh", "sinh"),
    ("cosh", "cosh"),
    ("tanh", "tanh"),
    ("asinh", "asinh"),
    ("acosh", "acosh"),
    ("atanh", "atanh"),
    ("exp", "exp"),
    ("ln", "ln"),
    ("log10", "log"),
    ("to_degrees", "to_degrees"),
    ("to_radians", "to_radians"),
];

/// The float methods of one argument of the same float, each with the
/// runtime's function that computes `function(self, other)` as Rust does.
pub(super) const FLOAT_BINARY_METHODS: &[(&str, &str)] = &[
    ("atan2", "atan"),
    ("hypot", "hypot"),
    ("log", "log"),
    ("rem_euclid", "rem_euclid"),
    ("div_euclid", "div_euclid"),
];

/// The step of Rust's method probe a receiver mode is found at: by value
/// (`self`), then by `&`, then by `&mut`.
pub(super) fn receiver_rank(mode: receiver::Mode) -> u8 {
    match mode {
        receiver::Mode::Owned => 0,
        receiver::Mode::Shared => 1,
        receiver::Mode::Mutable => 2,
    }
}

/// The step of Rust's method probe on a receiver of type `receiver` that
/// finds a method taking `mode`: by value, then `&`, then `&mut`; through a
/// reference, its own mode first, then the referent's by-value steps.
pub(super) fn probe_rank(receiver: &Ty, mode: receiver::Mode) -> u8 {
    let order = match receiver {
        Ty::Reference { mutable: false, .. } => [
            receiver::Mode::Shared,
            receiver::Mode::Owned,
            receiver::Mode::Mutable,
        ],
        Ty::Reference { mutable: true, .. } => [
            receiver::Mode::Mutable,
            receiver::Mode::Owned,
            receiver::Mode::Shared,
        ],
        _ => [
            receiver::Mode::Owned,
            receiver::Mode::Shared,
            receiver::Mode::Mutable,
        ],
    };
    order.iter().position(|step| *step == mode).unwrap_or(0) as u8
}

/// The impl-owner id of a type: a script type's definition, a builtin
/// type's witness id.
pub(super) fn owner_id(ty: &Ty) -> Option<type_def::Id> {
    match ty {
        Ty::Script(id, _) => Some(*id),
        Ty::Boxed(..) | Ty::Counted(..) | Ty::Weak(..) => None,
        Ty::Host { proof, .. } => Some(type_def::Id::foreign(&proof.definition.path())),

        ty => builtin_code(ty).map(type_def::Id::builtin),
    }
}

/// The runtime type key a dispatcher compares for an impl owner: the
/// definition index, or a builtin's witness code.
pub(super) fn owner_key(owner: type_def::Id) -> String {
    match owner.builtin_code() {
        Some(code) => code.to_string(),
        None => owner.index().to_string(),
    }
}

/// The discriminant of an `Ordering` variant, as Rust gives it.
pub(super) fn ordering_discriminant(name: &str) -> Option<crate::INT> {
    Some(match name {
        "Less" => -1,
        "Equal" => 0,
        "Greater" => 1,
        _ => return None,
    })
}

/// The builtin type a path's first segment names: `i64` of `i64::from`.
pub(super) fn builtin_path_type(name: &str) -> Option<Ty> {
    Some(match name {
        "bool" => Ty::Bool,
        "char" => Ty::Char,
        "String" => Ty::String,
        name => return numeric_type(name),
    })
}

/// The witness code of a builtin type: `T::default()`'s witness, a
/// builtin value's runtime type key and a builtin impl owner's key.
/// `Box<T>` is `T` at runtime, so it has `T`'s code.
pub(super) fn builtin_code(ty: &Ty) -> Option<crate::INT> {
    Some(match ty {
        Ty::Int(_) => -2,
        Ty::Float(_) => -3,
        Ty::String | Ty::Str => -4,
        Ty::Bool => -5,
        Ty::Char => -6,
        Ty::Option(_) => -7,
        // `Vec<T>` and `[T]` own their impls apart, as in Rust, though
        // both are an array at runtime (`builtin_type_key`).
        Ty::Array {
            kind: sequence::Kind::Vec,
            ..
        } => -8,
        Ty::Array {
            kind: sequence::Kind::Slice,
            ..
        } => -11,
        Ty::Unit => -9,
        Ty::Result(..) => -10,
        Ty::Boxed(inner) => return builtin_code(inner),
        _ => return None,
    })
}

/// The runtime type key of an `Rc` or `Arc`, which owns no impl: what a
/// std trait's dispatcher derefs it by.
pub(super) const COUNTED_KEY: crate::INT = -12;

/// The runtime type key of a builtin value: the witness code of its type
/// (`builtin_code`). One match on the value's tag, after the script-type
/// arms, so script dispatch pays nothing for it.
pub(super) fn builtin_type_key(value: &Dynamic) -> Option<crate::INT> {
    Some(match &value.0 {
        Union::Int(..) => -2,
        Union::Variant(..) if wide_value(value).is_some() => -2,
        Union::Float(..) => -3,
        Union::Str(..) => -4,
        Union::Bool(..) => -5,
        Union::Char(..) => -6,
        Union::Array(..) => -8,
        Union::Unit(..) => -9,
        Union::Variant(..) if crate::option::is(value) => -7,
        Union::Variant(..) if crate::result::is(value) => -10,
        Union::Variant(..) if value.is::<counted::Counted>() => COUNTED_KEY,
        _ => return None,
    })
}

/// Fully proven operations that may cross into sync runtime support.
#[derive(Clone)]
#[allow(
    clippy::large_enum_variant,
    reason = "intrinsic and structural proofs are most executable ops; boxing them would allocate per op"
)]
pub(super) enum ExecutableOp {
    #[cfg(feature = "rust-import")]
    Sequence(sequence::Constructor),
    #[cfg(feature = "rust-import")]
    Imported {
        proof: crate::host::structural::Proof,
    },
    /// A host operation of a body whose signature names generics inside a
    /// host type: its proof is instantiated by the witnesses it takes.
    Witnessed(Box<WitnessedOp>),
    Symbolic {
        value: crate::item::Value,
        /// A lifted closure's captures, passed to the op where it is written.
        captures: usize,
    },
    Intrinsic {
        proof: intrinsic::Proof,
    },
    Projection {
        proof: Box<crate::host::projection::Site>,
    },
    Struct {
        definition: type_def::Definition,
        item: item::Struct,
    },
    Enum {
        definition: type_def::Definition,
        item: item::Enum,
        variant: usize,
    },
    Clone {
        definition: type_def::Definition,
    },
    Debug {
        definition: type_def::Definition,
    },
    Equal {
        definition: type_def::Definition,
    },
    Default {
        value: Dynamic,
        definition: type_def::Definition,
    },
    Validate {
        definition: type_def::Definition,
    },
    MatchSubject {
        definition: type_def::Definition,
        item: item::Enum,
    },
    Prelude(PreludeOp),
    /// A prelude operation for the kinds (`kind_code`) it was checked at.
    Kinded {
        op: PreludeOp,
        kinds: [INT; 2],
    },
    /// A comparison or a keyed operation of the shape it was checked at.
    Shaped {
        op: PreludeOp,
        shape: Shape,
        flags: u8,
    },
}

#[derive(Clone)]
pub(super) struct IntrinsicOperation {
    pub(super) facade: crate::host::facade::Fingerprint,
    pub(super) owner: crate::host::facade::Dependency,
    pub(super) item: crate::host::item::Definition,
    pub(super) requirements: Box<[(ImmutableString, Ty, capability::Set)]>,
    pub(super) operands: Box<[(Ty, Span, bool)]>,
    pub(super) output: Ty,
    pub(super) span: Span,
}

#[cfg(feature = "rust-import")]
#[derive(Clone)]
pub(super) struct ImportedOperation {
    pub(super) kind: crate::host::structural::Kind,
    pub(super) operands: Box<[(Ty, Span, bool)]>,
    pub(super) output: Ty,
    pub(super) receiver: Option<crate::receiver::Mode>,
}

#[cfg(feature = "rust-import")]
impl ImportedOperation {
    pub(super) fn instance(&self, inference: &Inference) -> Box<[Ty]> {
        use crate::host::structural::Kind;
        let owner = match self.kind {
            // A generic enum's variant is made at the instance its fields
            // and use infer, as a struct is.
            Kind::Constructor | Kind::Variant => Some(&self.output),
            // A variant's matchers, of the enum value they read, as a
            // match on iced's `Key<&str>` reads it.
            Kind::Field { .. }
            | Kind::Store
            | Kind::Match
            | Kind::Test
            | Kind::Take
            | Kind::Lend
            | Kind::LendMut
            | Kind::Read { .. }
            | Kind::Borrow { .. } => self.operands.first().map(|(ty, ..)| ty),
        };
        let Some(owner) = owner else {
            return Box::default();
        };
        let resolved = inference.resolve(owner);
        match without_references(&resolved) {
            Ty::Host { arguments, .. } => arguments.clone(),
            _ => Box::default(),
        }
    }

    pub(super) fn collect_dependencies(
        &self,
        dependencies: &mut BTreeSet<crate::host::facade::Dependency>,
    ) {
        for (operand, _, _) in &self.operands {
            operand.collect_dependencies(dependencies);
        }
        self.output.collect_dependencies(dependencies);
    }
}

#[derive(Clone)]
pub(super) struct ProjectionOperation {
    pub(super) key: crate::host::projection::Key,
    pub(super) source: Ty,
    pub(super) target: Ty,
    pub(super) span: Span,
}

/// An intrinsic or projection proof over generic parameters
/// (`intrinsic::Type::witness_parameter`), instantiated once per tuple of
/// witnesses its calls pass after their operands.
#[derive(Clone)]
pub(super) struct WitnessedOp {
    pub(super) template: ExecutableOp,
    /// The template with each parameter as `()`: what the load checks,
    /// since a binding and its attestations never depend on a script type.
    pub(super) placeholder: ExecutableOp,
    pub(super) witnesses: usize,
    pub(super) instances: Shared<crate::Locked<WitnessedInstances>>,
}

/// A witnessed op's instances, by the witnesses its calls pass.
pub(super) type WitnessedInstances = BTreeMap<Box<[crate::INT]>, Shared<WitnessedInstance>>;

pub(super) struct WitnessedInstance {
    pub(super) op: ExecutableOp,
    pub(super) requirement: Option<crate::host::runtime::Requirement>,
}

impl WitnessedOp {
    pub(super) fn new(template: ExecutableOp, witnesses: usize) -> Self {
        let placeholder = template.instantiate(&vec![intrinsic::Type::unit(); witnesses]);
        Self {
            template,
            placeholder,
            witnesses,
            instances: Shared::new(crate::Locked::new(BTreeMap::new())),
        }
    }

    pub(super) fn invoke(
        &self,
        context: Option<NativeCallContext<'_>>,
        args: &mut FnCallArgs,
        models: &Models,
    ) -> RawrResult {
        let Some(split) = args.len().checked_sub(self.witnesses) else {
            return Err(invariant("a witnessed host operation lost its witnesses"));
        };
        let codes = args[split..]
            .iter()
            .map(|code| {
                code.as_int()
                    .map_err(|_| invariant("a type witness is not a code"))
            })
            .collect::<Result<Box<[_]>, _>>()?;
        let known = crate::func::locked_read(&self.instances)
            .and_then(|instances| instances.get(&codes).cloned());
        let instance = match known {
            Some(instance) => instance,
            None => {
                let arguments = codes
                    .iter()
                    .map(|code| {
                        witness_type(models, *code).ok_or_else(|| {
                            invariant(
                                "a generic helper's type argument must be a script type or a \
                                 number, string, bool, char or `()`",
                            )
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let op = self.template.instantiate(&arguments);
                let requirement = op.runtime_requirement();
                let instance = Shared::new(WitnessedInstance { op, requirement });
                if let Some(mut instances) = crate::func::locked_write(&self.instances) {
                    instances.insert(codes, instance.clone());
                }
                instance
            }
        };
        instance.op.invoke(
            context,
            &mut args[..split],
            models,
            instance.requirement.as_ref(),
        )
    }
}

/// The runtime type a witness code names (`builtin_code`, or a script
/// type's index).
pub(super) fn witness_type(models: &Models, code: crate::INT) -> Option<intrinsic::Type> {
    Some(match code {
        -2 => intrinsic::Type::builtin("i64", TypeId::of::<INT>()),
        -3 => intrinsic::Type::builtin("f64", TypeId::of::<crate::FLOAT>()),
        -4 => intrinsic::Type::builtin("String", TypeId::of::<ImmutableString>()),
        -5 => intrinsic::Type::builtin("bool", TypeId::of::<bool>()),
        -6 => intrinsic::Type::builtin("char", TypeId::of::<char>()),
        -9 => intrinsic::Type::unit(),
        code => {
            let index = usize::try_from(code).ok()?;
            let id = models.by_definition.keys().find(|id| id.index() == index)?;
            script_type(models, *id, Box::default())
        }
    })
}

/// The runtime type of the script type `id` given `arguments`.
pub(super) fn script_type(
    models: &Models,
    id: type_def::Id,
    arguments: Box<[intrinsic::Type]>,
) -> intrinsic::Type {
    let model = models.definition(id);
    intrinsic::Type::script(
        model.identity(),
        model.definition().schema().clone(),
        model.definition().kind(),
        model.ty().type_id(models),
        arguments,
    )
}

pub(super) struct InstantiatedHostSignature {
    pub(super) inputs: Vec<Ty>,
    pub(super) output: Ty,
    pub(super) requirements: Box<[(ImmutableString, Ty, capability::Set)]>,
}

impl ExecutableOp {
    /// An intrinsic or projection with each generic parameter replaced by
    /// its argument.
    pub(super) fn instantiate(&self, arguments: &[intrinsic::Type]) -> Self {
        match self {
            Self::Intrinsic { proof } => Self::Intrinsic {
                proof: proof.instantiate(arguments),
            },
            Self::Projection { proof } => Self::Projection {
                proof: Box::new(crate::host::projection::Site::new(
                    proof.key().clone(),
                    proof.source().instantiate(arguments),
                    proof.target().instantiate(arguments),
                    proof.span(),
                )),
            },
            op => op.clone(),
        }
    }

    pub(super) fn dependencies(&self) -> BTreeSet<crate::host::facade::Dependency> {
        match self {
            #[cfg(feature = "rust-import")]
            Self::Sequence(constructor) => constructor.dependencies(),
            Self::Witnessed(op) => op.placeholder.dependencies(),
            #[cfg(feature = "rust-import")]
            Self::Imported { proof } => proof.dependencies(),
            Self::Symbolic { .. } => BTreeSet::new(),
            Self::Intrinsic { proof } => proof.dependencies(),
            Self::Projection { proof } => proof.dependencies(),
            Self::Struct { .. }
            | Self::Enum { .. }
            | Self::Clone { .. }
            | Self::Debug { .. }
            | Self::Equal { .. }
            | Self::Default { .. }
            | Self::Validate { .. }
            | Self::MatchSubject { .. }
            | Self::Prelude(_)
            | Self::Kinded { .. }
            | Self::Shaped { .. } => BTreeSet::new(),
        }
    }

    pub(super) fn projection_requirement(&self) -> Option<crate::host::projection::Site> {
        match self {
            Self::Witnessed(op) => op.placeholder.projection_requirement(),
            Self::Projection { proof } => Some((**proof).clone()),
            #[cfg(feature = "rust-import")]
            Self::Imported { .. } | Self::Sequence(_) => None,
            Self::Symbolic { .. }
            | Self::Intrinsic { .. }
            | Self::Struct { .. }
            | Self::Enum { .. }
            | Self::Clone { .. }
            | Self::Debug { .. }
            | Self::Equal { .. }
            | Self::Default { .. }
            | Self::Validate { .. }
            | Self::MatchSubject { .. }
            | Self::Prelude(_)
            | Self::Kinded { .. }
            | Self::Shaped { .. } => None,
        }
    }

    pub(super) fn runtime_requirement(&self) -> Option<crate::host::runtime::Requirement> {
        match self {
            Self::Witnessed(op) => op.placeholder.runtime_requirement(),
            #[cfg(feature = "rust-import")]
            Self::Imported { proof } => Some(crate::host::runtime::Requirement::structural(proof)),
            #[cfg(feature = "rust-import")]
            Self::Sequence(_) => None,
            Self::Intrinsic { proof } => Some(proof.runtime_requirement()),
            Self::Symbolic { .. }
            | Self::Projection { .. }
            | Self::Struct { .. }
            | Self::Enum { .. }
            | Self::Clone { .. }
            | Self::Debug { .. }
            | Self::Equal { .. }
            | Self::Default { .. }
            | Self::Validate { .. }
            | Self::MatchSubject { .. }
            | Self::Prelude(_)
            | Self::Kinded { .. }
            | Self::Shaped { .. } => None,
        }
    }

    pub(super) fn private_name(&self, support_id: u64) -> String {
        let (definition, operation) = match self {
            Self::Witnessed(op) => {
                return format!(
                    "{}.witnessed.{}",
                    op.template.private_name(support_id),
                    op.witnesses
                );
            }
            Self::Prelude(op) => return format!("\0rawr.{}", op.name()),
            #[cfg(feature = "rust-import")]
            Self::Sequence(constructor) => return constructor.name(),
            Self::Kinded { op, kinds } => {
                return format!("\0rawr.{}.{}.{}", op.name(), kinds[0], kinds[1]);
            }
            Self::Shaped { op, shape, flags } => {
                return format!("\0rawr.{}.shaped.{shape}.{flags}", op.name());
            }
            #[cfg(feature = "rust-import")]
            Self::Imported { proof } => {
                return format!(
                    "\0rawr.generated-structural.{}.{:016x}",
                    proof.definition().path(),
                    proof.symbol_id()
                );
            }
            Self::Symbolic { value, .. } => {
                return format!(
                    "\0rawr.symbolic.{}.{:016x}",
                    value.key().name(),
                    crate::program::schema::symbol_id(value)
                );
            }
            Self::Intrinsic { proof } => {
                return format!(
                    "\0rawr.intrinsic.{}.{:016x}",
                    proof.path(),
                    proof.symbol_id()
                );
            }
            Self::Projection { proof } => {
                return format!(
                    "\0rawr.projection.{}.{:016x}",
                    proof.key().diagnostic_name(),
                    proof.symbol_id()
                );
            }
            Self::Struct { definition, .. } => (definition, "construct".to_string()),
            Self::Enum {
                definition,
                item,
                variant,
            } => (
                definition,
                format!("variant.{}", item.variants[*variant].name.text),
            ),
            Self::Clone { definition } => (definition, "clone".to_string()),
            Self::Debug { definition } => (definition, "debug".to_string()),
            Self::Equal { definition } => (definition, "equal".to_string()),
            Self::Default { definition, .. } => (definition, "default".to_string()),
            Self::Validate { definition } => (definition, "validate".to_string()),
            Self::MatchSubject { definition, .. } => (definition, "match-subject".to_string()),
        };

        format!(
            "{PRIVATE_PREFIX}{}.{:016x}.{:016x}.{operation}",
            definition.key(),
            definition.schema().symbol_id(),
            support_id
        )
    }

    pub(super) fn argument_types(&self, models: &Models) -> Vec<TypeId> {
        match self {
            #[cfg(feature = "rust-import")]
            Self::Sequence(_) => vec![
                TypeId::of::<INT>(),
                TypeId::of::<Dynamic>(),
                TypeId::of::<INT>(),
            ],
            // A bare generic operand takes any value; the proof checks it.
            Self::Witnessed(op) => {
                let parameter = intrinsic::Type::witness_parameter(0).runtime();
                op.template
                    .argument_types(models)
                    .into_iter()
                    .map(|ty| {
                        if ty == parameter {
                            TypeId::of::<Dynamic>()
                        } else {
                            ty
                        }
                    })
                    .chain(std::iter::repeat_n(TypeId::of::<INT>(), op.witnesses))
                    .collect()
            }
            #[cfg(feature = "rust-import")]
            Self::Imported { proof } => proof
                .operands()
                .iter()
                .map(|operand| place_operand(operand.ty().runtime()))
                .collect(),
            Self::Symbolic { captures, .. } => vec![TypeId::of::<Dynamic>(); *captures],
            Self::Intrinsic { proof } => proof
                .operands()
                .iter()
                .map(|operand| place_operand(operand.ty().runtime()))
                .collect(),
            Self::Projection { proof } => vec![proof.source().runtime()],
            Self::Struct { item, .. } => item
                .fields
                .iter()
                .map(|field| Ty::from_resolved(&field.ty).type_id(models))
                .map(place_operand)
                .collect(),
            Self::Enum { item, variant, .. } => item.variants[*variant]
                .payload
                .iter()
                .map(|ty| Ty::from_resolved(ty).type_id(models))
                .map(place_operand)
                .collect(),
            Self::Clone { definition } | Self::Debug { definition } => {
                vec![models.definition(definition.id()).ty().type_id(models)]
            }
            Self::Equal { definition } => {
                let type_id = models.definition(definition.id()).ty().type_id(models);
                vec![type_id, type_id]
            }
            Self::Default { .. } => Vec::new(),
            Self::Validate { definition } => {
                vec![models.definition(definition.id()).ty().type_id(models)]
            }
            Self::MatchSubject { .. } => vec![TypeId::of::<Dynamic>()],
            // An `i128` or a `u128` past `i64` is boxed.
            Self::Kinded {
                op: PreludeOp::IntMethod,
                kinds,
            } if kinds
                .iter()
                .any(|&code| Int::from_code(code).is_some_and(Int::wide)) =>
            {
                vec![
                    TypeId::of::<Dynamic>(),
                    TypeId::of::<crate::INT>(),
                    TypeId::of::<Dynamic>(),
                ]
            }
            Self::Prelude(op) | Self::Kinded { op, .. } | Self::Shaped { op, .. } => {
                op.argument_types()
            }
        }
    }

    pub(super) fn invoke(
        &self,
        context: Option<NativeCallContext<'_>>,
        args: &mut FnCallArgs,
        models: &Models,
        requirement: Option<&crate::host::runtime::Requirement>,
    ) -> RawrResult {
        match self {
            #[cfg(feature = "rust-import")]
            Self::Sequence(constructor) => constructor.invoke(context.as_ref(), args),
            Self::Witnessed(op) => op.invoke(context, args, models),
            #[cfg(feature = "rust-import")]
            Self::Imported { proof } => {
                let Some(runtime) = context.and_then(|context| context.host_runtime()) else {
                    return Err(EvalAltResult::ErrorRuntime(
                        format!(
                            "missing generated structural binding for `{}`",
                            proof.definition().path()
                        )
                        .into(),
                        proof.span().start(),
                    )
                    .into());
                };
                proof.invoke_prepared(
                    runtime,
                    requirement
                        .ok_or_else(|| invariant("registered structural requirement missing"))?,
                    args,
                    &models.runtime,
                )
            }
            Self::Symbolic { value, .. } if args.is_empty() => Ok(Dynamic::from(value.clone())),
            Self::Symbolic { value, .. } => Ok(Dynamic::from(
                value.bind(args.iter_mut().map(|arg| arg.take()).collect()),
            )),
            Self::Intrinsic { proof } => {
                let Some(context) = context else {
                    return Err(EvalAltResult::ErrorRuntime(
                        format!("missing runtime binding for `{}`", proof.path()).into(),
                        proof.span().start(),
                    )
                    .into());
                };
                let Some(runtime) = context.host_runtime() else {
                    return Err(EvalAltResult::ErrorRuntime(
                        format!("missing runtime binding for `{}`", proof.path()).into(),
                        proof.span().start(),
                    )
                    .into());
                };
                // A facade callback compares script values as this program
                // does: a crate type by its own `eq` (`Call::equal`).
                let comparison = Comparison {
                    models,
                    context: Some(&context),
                    total: false,
                };
                let equal = |lhs: &Dynamic, rhs: &Dynamic| comparison.equal(lhs, rhs);
                proof.invoke_prepared(
                    runtime,
                    requirement
                        .ok_or_else(|| invariant("registered intrinsic requirement missing"))?,
                    context.host_invocation(),
                    args,
                    &models.runtime,
                    Some(&equal),
                    context.engine().access(),
                )
            }
            Self::Projection { proof } => {
                let Some(runtime) = context.and_then(|context| context.host_runtime()) else {
                    return Err(EvalAltResult::ErrorRuntime(
                        format!(
                            "missing runtime projection binding for `{}`",
                            proof.key().diagnostic_name()
                        )
                        .into(),
                        proof.span().start(),
                    )
                    .into());
                };
                runtime.invoke_projection(proof, args, &models.runtime)
            }
            Self::Struct {
                definition,
                item: _,
            } => {
                let fields = args.iter_mut().map(|value| value.take()).collect();
                let identity = type_def::Identity::Script(definition.key().clone());
                let proof = models
                    .runtime
                    .prove_struct(&identity, definition.schema(), fields)
                    .map_err(construction_invariant)?;
                nominal::Struct::construct(&models.runtime, &identity, definition.schema(), proof)
                    .map(Into::into)
                    .map_err(construction_invariant)
            }
            Self::Enum {
                definition,
                item,
                variant,
            } => {
                let payload = args.iter_mut().map(|value| value.take()).collect();
                let identity = type_def::Identity::Script(definition.key().clone());
                let variant = nominal::variant::Id::from_name(&item.variants[*variant].name);
                let proof = models
                    .runtime
                    .prove_enum(&identity, definition.schema(), &variant, payload)
                    .map_err(construction_invariant)?;
                nominal::Enum::construct(
                    &models.runtime,
                    &identity,
                    definition.schema(),
                    &variant,
                    proof,
                )
                .map(Into::into)
                .map_err(construction_invariant)
            }
            Self::Clone { definition } => {
                validate_dynamic(args[0], definition, models)?;
                Ok(args[0].clone())
            }
            Self::Debug { definition } => {
                validate_dynamic(args[0], definition, models)?;
                Ok(debug_value(args[0], models).into())
            }
            Self::Equal { definition } => {
                let (lhs, rhs) = args.split_at_mut(1);
                validate_dynamic(lhs[0], definition, models)?;
                validate_dynamic(rhs[0], definition, models)?;
                // A derive: field by field, each by its type's own `eq`
                // where it has one.
                let comparison = Comparison {
                    models,
                    context: context.as_ref(),
                    total: false,
                };
                Ok(comparison.equal(lhs[0], rhs[0])?.into())
            }
            Self::Default { value, definition } => {
                validate_dynamic(value, definition, models)?;
                Ok(value.clone())
            }
            Self::Validate { definition } => {
                validate_dynamic(args[0], definition, models)?;
                Ok(Dynamic::UNIT)
            }
            Self::MatchSubject {
                definition, item, ..
            } => {
                let Union::Enum(value, ..) = &args[0].0 else {
                    return Ok(args[0].take());
                };
                let expected_identity = type_def::Identity::Script(definition.key().clone());
                if value.identity() != &expected_identity {
                    return Ok(args[0].take());
                }
                if value.schema() != definition.schema() {
                    return Err(invariant(&format!(
                        "nominal enum '{}' has a stale schema while matching",
                        definition.name()
                    )));
                }
                let Some(variant) = item
                    .variants
                    .iter()
                    .find(|variant| variant.name.text == value.variant().name())
                else {
                    return Err(invariant(&format!(
                        "nominal enum '{}' contains unknown variant '{}'",
                        definition.name(),
                        value.variant().name()
                    )));
                };
                if value.payload().len() != variant.payload.len()
                    || value
                        .payload()
                        .iter()
                        .zip(variant.payload.iter())
                        .any(|(value, ty)| !matches_runtime_type(value, ty, models))
                {
                    return Err(invariant(&format!(
                        "nominal enum '{}::{}' payload no longer matches its proven shape",
                        definition.name(),
                        variant.name.text
                    )));
                }
                Ok(args[0].take())
            }
            Self::Prelude(PreludeOp::Print) => {
                let output = if args[1].as_bool().unwrap_or(false) {
                    crate::Output::Stderr
                } else {
                    crate::Output::Stdout
                };
                if let Some(write) = context
                    .as_ref()
                    .and_then(|context| context.engine().output.as_ref())
                {
                    write(output, &args[0].clone_cast::<ImmutableString>());
                }
                Ok(Dynamic::UNIT)
            }
            Self::Prelude(op) => op.invoke(context.as_ref(), args, models, &Shape::Plain, 0),
            Self::Shaped { op, shape, flags } => {
                op.invoke(context.as_ref(), args, models, shape, *flags)
            }
            Self::Kinded {
                op: PreludeOp::Parse,
                kinds,
            } => Ok(parse_text(
                &args[0].clone_cast::<ImmutableString>(),
                kinds[0],
            )),
            Self::Kinded {
                op: PreludeOp::ErrorParse,
                kinds,
            } => Ok(error_object::parsed(
                &args[0].clone_cast::<ImmutableString>(),
                kinds[0],
            )),
            Self::Kinded {
                op: PreludeOp::CollectStop,
                kinds,
            } => Ok(match kinds[0] {
                COLLECT_RESULT => crate::result::read(args[0]).is_some_and(|item| item.is_err()),
                COLLECT_OPTION => crate::option::read(args[0]).is_some_and(|item| item.is_none()),
                _ => false,
            }
            .into()),
            Self::Kinded {
                op: PreludeOp::CollectTry,
                kinds,
            } => collect_try(*kinds, args[0].take()),
            Self::Kinded { kinds, .. } => int_method(*kinds, args),
        }
    }
}

/// The `&mut` arrays and strings a host call copies in and back out (D558
/// slice 2), as quick-xml's `read_event_into(&mut buf)` takes its buffer
/// and pulldown-cmark's `push_html(&mut s, ..)` its string: the arguments
/// whose input is `&mut [T]`, `&mut Vec<T>` or `&mut String`, in order.
/// A `&mut PathBuf` moves in and back out (`moved_input`).
pub(super) fn copied_places(
    inference: &Inference,
    inputs: &[Ty],
    arguments: &[ParsedExpr],
) -> Vec<ParsedExpr> {
    inputs
        .iter()
        .zip(arguments)
        .filter(|(input, _)| copied_input(inference, input))
        .map(|(_, argument)| argument.clone())
        .collect()
}

/// Whether a host call copies an input of `ty` in and back out
/// (`copied_places`): a `&mut` array or string, or one of std's values
/// (`moved_input`).
pub(super) fn copied_input(inference: &Inference, ty: &Ty) -> bool {
    moved_input(inference, ty)
        || matches!(
            inference.resolve(ty),
            Ty::Reference { mutable: true, target }
                if matches!(*target, Ty::Array { .. } | Ty::String)
        )
}

/// Whether a host call takes an input of `ty` moved in and gives it back
/// after its value (#680): a `&mut` of one of std's types Rawr's std holds
/// as its Rust value (`crossing::Std`), as a crate's `fn f(path: &mut
/// PathBuf)` takes. The call changes the program's value itself, and none
/// is copied, as rustc's borrow copies none.
///
/// So is a `&mut` of a program value where a generic call takes `&mut T`,
/// which native code changes through the value's proxy (#1008).
pub(super) fn moved_input(inference: &Inference, ty: &Ty) -> bool {
    matches!(
        inference.resolve(ty),
        Ty::Reference { mutable: true, target }
            if matches!(&*target, Ty::Host { proof, .. }
                if proof
                    .definition
                    .path()
                    .strip_prefix(crate::std_facade::ROOT)
                    .and_then(|path| path.strip_prefix("::"))
                    .is_some_and(crate::std_facade::crosses))
                || matches!(inference.resolve(&target), Ty::Script(..))
    )
}

/// Each argument of a host call that `moved_input` takes, moved out of its
/// place for the call (`copied_call` stores it back).
pub(super) fn move_into_call(inference: &Inference, inputs: &[Ty], arguments: &mut [ParsedExpr]) {
    for (input, argument) in inputs.iter().zip(arguments.iter_mut()) {
        if moved_input(inference, input) && argument.lvalue.is_some() {
            argument.source =
                origin::Fragment::call(crate::engine::KEYWORD_MOVE, [argument.rendered_source()]);
            argument.prelude = origin::Fragment::default();
        }
    }
}

/// The dispatch type of an operand. A sequence or string may be a typed
/// native place (D651) as well as a script value, as a native vector a
/// script made and keeps in a struct's field is: a host proof validates
/// either against the operand's exact type before the binding runs, and
/// each operation's name is unique to it, so no overload is widened.
pub(super) fn place_operand(ty: TypeId) -> TypeId {
    if ty == TypeId::of::<crate::Array>() || ty == TypeId::of::<ImmutableString>() {
        TypeId::of::<Dynamic>()
    } else {
        ty
    }
}

pub(super) fn register(
    module: &mut Module,
    name: ImmutableString,
    op: ExecutableOp,
    models: &Shared<Models>,
) {
    let arg_types = op.argument_types(models);
    // The exact requirement belongs to this immutable registered operation.
    // Keep it outside the hashed compiler proof and borrow it on every call.
    let requirement = op.runtime_requirement();
    let models = models.clone();
    // A map's or set's operation takes it by reference, as a method.
    let by_reference = match &op {
        ExecutableOp::Prelude(op) | ExecutableOp::Shaped { op, .. } => op.by_reference(),
        // A facade method with a `&mut self` receiver changes the receiver
        // it is called on, and one with `&self` reads it where it is.
        ExecutableOp::Intrinsic { proof } if proof.mutates_receiver() => Some(true),
        ExecutableOp::Intrinsic { proof } if proof.borrows_receiver() => Some(false),
        // A struct's own operations read the value where it is: a `clone`
        // copies it once, as Rust's does.
        ExecutableOp::Clone { .. }
        | ExecutableOp::Debug { .. }
        | ExecutableOp::Equal { .. }
        | ExecutableOp::Validate { .. } => Some(false),
        _ => None,
    };
    // Output is a side effect: never folded or dropped.
    let op_is_print = matches!(
        op,
        ExecutableOp::Prelude(PreludeOp::Print | PreludeOp::Time)
    );
    let func: Shared<crate::func::native::FnAny> =
        Shared::new(move |context, args| op.invoke(context, args, &models, requirement.as_ref()));
    let func = match by_reference {
        Some(mutates) => RawrFunc::Method {
            func,
            has_context: true,
            is_pure: !mutates,
            is_volatile: false,
        },
        None => RawrFunc::Pure {
            func,
            has_context: true,
            is_pure: true,
            is_volatile: op_is_print,
        },
    };
    FuncRegistration::new(name)
        .in_internal_namespace()
        .set_into_module_raw(module, arg_types, func);
}

pub(super) fn invariant(message: &str) -> Box<EvalAltResult> {
    EvalAltResult::ErrorRuntime(message.into(), Position::NONE).into()
}

pub(super) fn construction_invariant(
    error: crate::program::construction::Error,
) -> Box<EvalAltResult> {
    invariant(&error.to_string())
}

pub(super) fn validate_dynamic(
    value: &Dynamic,
    definition: &type_def::Definition,
    models: &Models,
) -> Result<(), Box<EvalAltResult>> {
    let identity = type_def::Identity::Script(definition.key().clone());
    models
        .runtime
        .validate_value(value, &identity, definition.schema())
        .map_err(construction_invariant)
}

pub(super) fn matches_runtime_type(value: &Dynamic, ty: &type_ref::Type, models: &Models) -> bool {
    let type_ref::Type::Path(path) = ty else {
        return nominal::matches_type(value, ty);
    };
    let type_ref::Target::Script { definition, .. } = &path.target else {
        return nominal::matches_type(value, ty);
    };
    let definition = models.definition(*definition).definition();
    let identity = type_def::Identity::Script(definition.key().clone());
    models
        .runtime
        .validate_value(value, &identity, definition.schema())
        .is_ok()
}

#[cfg(test)]
pub(super) fn matches_runtime_model(value: &Dynamic, model: &Nominal, models: &Models) -> bool {
    let expected = model.definition();
    let identity = type_def::Identity::Script(expected.key().clone());
    models
        .runtime
        .validate_value(value, &identity, expected.schema())
        .is_ok()
}

/// A static's layout as the lowering writes it, `[size, offsets..]`.
fn static_layout(layout: &Dynamic) -> Option<(usize, Box<[usize]>)> {
    let words = layout.read_lock::<crate::Array>()?;
    let mut words = words.iter().map(|word| {
        word.as_int()
            .ok()
            .and_then(|word| usize::try_from(word).ok())
    });
    let size = words.next()??;
    let offsets = words.collect::<Option<Box<[usize]>>>()?;
    Some((size, offsets))
}
