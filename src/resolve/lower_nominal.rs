//! Lower validated nominal operations onto the retained evaluator.

mod attributes;
mod binding;
mod body;
mod borrows;
mod branch;
mod builtin_method;
mod call;
mod cargo_env;
pub(crate) mod cell;
mod closure;
mod coerce;
mod collect;
mod conversion;
pub(crate) mod counted;
mod coverage;
pub(crate) mod cow;

mod dispatch;
mod drops;
mod error_object;
mod expand;
mod fmt_write;
mod format;
mod from_str;
mod generics;
mod host_call;
mod host_signature;
mod imported;
mod inference;
mod iterator;
mod keyed;
mod literal;
mod local_type;
mod location;
mod loops;
mod lvalue;
mod method_path;
mod models;
mod moves;
mod mutability;
mod names;
mod op;
mod operator;
mod origin;
mod overflow;
mod path;
mod pattern;
mod places;
mod postfix;
mod proof;
mod sequence;
use places::PlacePattern;
mod reborrow;
mod reverse;
mod statement;
mod std_impl;
mod std_method;
mod std_path;
pub(crate) mod thread;
mod time;
mod trait_call;
mod traits;
mod ty;
mod unify;
mod wide;

#[cfg(test)]
mod argument_count_tests;
#[cfg(all(test, feature = "rust-import"))]
mod collected_tests;
mod compare;
#[cfg(test)]
mod comparison_tests;
#[cfg(test)]
mod copies_tests;
#[cfg(test)]
mod derive_shape_tests;
#[cfg(test)]
mod divergence_tests;
#[cfg(test)]
mod language_tests;
#[cfg(test)]
mod macro_tests;
#[cfg(test)]
mod placeholder_tests;
#[cfg(test)]
mod privacy_tests;
#[cfg(test)]
mod rust_meaning_tests;
#[cfg(test)]
mod rustc_code_tests;
#[cfg(test)]
mod std_impl_tests;
#[cfg(test)]
mod std_method_tests;
#[cfg(test)]
mod str_tests;
#[cfg(test)]
mod trait_instance_tests;

use super::borrowck::{
    self,
    record::{Matched, ScopeEnd},
};
use super::{
    intrinsic, item, type_ref, BindingSite, Callee, Error, LoweringFailure, MoveCause, Program,
    Unresolved,
};
use crate::ast::{
    self, BinaryExpr, FlowControl, FnCallExpr, FnCallHashes, NominalPattern, NominalPayloadType,
    Stmt, NOMINAL_MATCH_SENTINEL,
};
use crate::func::{FnCallArgs, NativeCallContext};
use crate::tokenizer::{is_valid_identifier, Token};
use crate::types::{dynamic::Union, nominal};
use crate::{
    calc_fn_hash, capability, method, receiver, type_def, Dynamic, Engine, EvalAltResult,
    ImmutableString, Module, Position, RawrResult, Scope, Shared, Span, AST, INT,
};
use compare::Shape;
use coverage::{
    bool_literals, field_order, reject_empty_range, scalar_text, tuple_members, Scalar,
};
use dispatch::{
    builtin_conversions_from, builtin_operator, write_closures, write_default_dispatcher,
    write_function, write_method, write_trait_dispatchers, BUILTIN_CONVERSIONS,
};
#[cfg(feature = "rust-import")]
pub(super) use imported::imported;
#[cfg(all(test, feature = "rust-import"))]
use imported::imported_result;
#[cfg(feature = "rust-import")]
use imported::UseArguments;
#[cfg(feature = "rust-import")]
use imported::{instance_names, native_impls};
#[cfg(feature = "rust-import")]
pub(crate) use imported::{reveal, ImportedOutput};
use imported::{ImplArguments, ImplUse};
#[cfg(test)]
use inference::{inference_metrics, reset_inference_metrics};
use inference::{Checkpoint, Inference, Patch};
use keyed::keyed_query;
use lvalue::{opens_compound, Lvalue, PlaceStep, Writeback};
#[cfg(feature = "rust-import")]
use models::ScriptTraitImpl;
#[cfg(test)]
use models::{model_clones, reset_model_clones};
use models::{Models, Nominal, ScriptFunction};
#[cfg(test)]
use op::builtin_type_key;
#[cfg(feature = "rust-import")]
use op::ImportedOperation;
use op::{
    builtin_code, builtin_path_type, copied_input, copied_places, entry_key, impl_self_ty,
    move_into_call, moved_input, ordering_discriminant, owner_id, owner_key, probe_rank,
    receiver_rank, register, script_type, ExecutableOp, InstantiatedHostSignature,
    IntrinsicOperation, Keys, Op, PreludeOp, ProjectionOperation, WitnessedOp,
    FLOAT_BINARY_METHODS, FLOAT_UNARY_METHODS, SHAPED_HASHED, SHAPED_TOTAL,
};
use pattern::{PatternBinding, Prelude};
use std::any::TypeId;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fmt::Write;
use std::iter::Peekable;
use std::rc::Rc;
use std::vec::IntoIter;
#[cfg(feature = "rust-import")]
use ty::uninstantiated_call;
#[cfg(all(test, feature = "rust-import"))]
use ty::{host_ty_proof_allocations, reset_host_ty_proof_allocations};
use ty::{
    host_type_name, literal_fallback, numeric_type, same_head, same_host_definition, same_inferred,
    CellKind, Counter, Float, HostTyProof, Int, Keyed, PartialEqImpl, Pointer, Time, Ty, Unplaced,
    UNSIGNED_SOURCE, WIDE_I128, WIDE_U128,
};
use wide::{
    kind_value, wide_i128, wide_method, wide_order, wide_u128, wide_value, PendingFloat,
    PendingFloatKind, PendingWide, Wide,
};
pub(crate) use wide::{wide_operator, wide_range};

const SENTINEL: &str = "__rawr_nominal_op_";
const METHOD_SENTINEL: &str = "__rawr_nominal_method_";
const FUNCTION_SENTINEL: &str = "__rawr_nominal_function_";
/// A rewrite's target for a script function's own name where the lowered
/// text calls something else by it: the call reaches only the runtime's
/// functions (`rewrite_call`).
const NATIVE_ONLY: &str = "\0rawr.native-only";
const DISPATCH_SENTINEL: &str = "__rawr_nominal_dispatch_";
/// The program's dispatcher of `Debug::fmt`, present when it has a
/// hand-written `Debug` impl; it falls back to [`DEBUG_GENERIC`].
pub(crate) const DEBUG_DISPATCHER: &str = "__rawr_nominal_dispatch_Debug__fmt";
/// The `Debug` text of any value: a derive's, a builtin's or a crate
/// type's. Present when the program formats with `Debug` or a debugger is
/// registered while it compiles.
pub(crate) const DEBUG_GENERIC: &str = "\0rawr.generic.debug";
/// The generated function from a type witness to that type's default.
const DEFAULT_DISPATCHER: &str = "__rawr_nominal_default";
const CLOSURE_SENTINEL: &str = "__rawr_nominal_closure_";
/// The last parameter of an async body's step function: `()` to start, or
/// the frame it resumes from.
const ASYNC_RESUME: &str = "__rawr_resume";
/// The core's function an async body's step function is passed to, where
/// no host's `Task` runs futures.
const CORE_ASYNC: &str = "rawr::future::script";
const PRIVATE_PREFIX: &str = "\0rawr.nominal.";
/// Why a crate call can't take the program's function that builds its
/// message, as `Message::Changed` given to iced's `on_input`, here.
#[cfg(feature = "rust-import")]
const UNSUPPORTED_MESSAGE_FUNCTION: &str =
    "passing the program's function that builds its message is not supported here in Rawr yet";

/// A lifetime's name as Rust writes it, with its `'`.
#[cfg(feature = "rust-import")]
fn lifetime_text(name: &ImmutableString) -> Box<str> {
    if name.starts_with('\'') {
        name.as_str().into()
    } else {
        format!("'{name}").into()
    }
}

/// `ty`, a type a script impl's header or signature writes, as an
/// instance's surface type, with each of the impl's own parameters
/// `generics` a [`Type::Parameter`] at its index.
#[cfg(feature = "rust-import")]
fn impl_surface_type(
    models: &Models,
    ty: &Ty,
    generics: &[ImmutableString],
) -> Option<rawr_import::classify::representability::convert::Type> {
    use rawr_import::classify::representability::convert::Type;
    let all = |types: &[Ty]| {
        types
            .iter()
            .map(|ty| impl_surface_type(models, ty, generics))
            .collect::<Option<Box<[_]>>>()
    };
    Some(match ty {
        Ty::Param(name) => Type::Parameter(
            u32::try_from(generics.iter().position(|generic| generic == name)?).ok()?,
        ),
        Ty::Unit => Type::Unit,
        Ty::Reference { mutable, target } => Type::Borrowed {
            mutable: *mutable,
            ty: Box::new(impl_surface_type(models, target, generics)?),
        },
        Ty::Option(element) => {
            Type::Option(Box::new(impl_surface_type(models, element, generics)?))
        }
        Ty::Result(ok, err) => Type::Result(
            Box::new(impl_surface_type(models, ok, generics)?),
            Box::new(impl_surface_type(models, err, generics)?),
        ),
        Ty::Array {
            element,
            kind: sequence::Kind::Vec,
        } => Type::Sequence {
            element: Box::new(impl_surface_type(models, element, generics)?),
            owned: true,
            length: None,
        },
        // `[T]`, as `visit_bytes(self, v: &[u8])` takes it.
        Ty::Array {
            element,
            kind: sequence::Kind::Slice,
        } => Type::Sequence {
            element: Box::new(impl_surface_type(models, element, generics)?),
            owned: false,
            length: None,
        },
        Ty::Tuple(members) => Type::Tuple(all(members)?),
        Ty::Script(definition, arguments) => Type::Script {
            path: models.script_path(*definition),
            arguments: all(arguments)?,
        },
        Ty::Host { proof, arguments } if proof.definition.identity().imported().is_some() => {
            Type::Nominal {
                definition: rawr_import::surface::identity::Definition::authenticated(
                    *proof.definition.identity().imported()?.as_bytes(),
                ),
                arguments: all(arguments)?,
            }
        }
        ty => instance_surface_type(&Inference::default(), models, ty)?,
    })
}

#[cfg(feature = "rust-import")]
#[derive(Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct ImportedUse {
    pub(crate) source: crate::program::source::Id,
    pub(crate) path: ImmutableString,
    pub(crate) selector: rawr_import::surface::identity::Selector,
    pub(crate) types: Box<[rawr_import::classify::representability::convert::Type]>,
    /// Its const parameters' values, in declared order, as an array's
    /// length gives `N` (#1330).
    pub(crate) constants: Box<[rawr_import::classify::representability::convert::Constant]>,
    pub(crate) functions: Box<[ImportedFunctionUse]>,
    pub(crate) mappers: Box<[ImportedMapperUse]>,
    /// The item takes a script closure for its `F: FnOnce` parameter.
    pub(crate) callable: bool,
    /// The script functions that fill its callable parameters as slots
    /// (D594), as its instance keys them ([`Slot::served`]).
    ///
    /// [`Slot::served`]: rawr_import::surface::instance::Slot::served
    pub(crate) slots: Box<[rawr_import::surface::instance::Slot]>,
    /// The same slots by the functions that fill them.
    pub(crate) fillers: Box<[rawr_import::surface::instance::Slot]>,
    /// The proof-backed application this use calls, when it is one.
    pub(crate) application: Option<[u8; 32]>,
    pub(crate) span: Span,
}

#[cfg(feature = "rust-import")]
#[derive(Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct ImportedFunctionUse {
    pub(crate) position: u32,
    /// A generic function's instance, in its declared parameter order.
    pub(crate) types: Box<[rawr_import::classify::representability::convert::Type]>,
    pub(crate) path: ImmutableString,
    pub(crate) selector: rawr_import::surface::identity::Selector,
    pub(crate) span: Span,
}

#[cfg(feature = "rust-import")]
#[derive(Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct ImportedMapperUse {
    pub(crate) position: u32,
    pub(crate) seal: [u8; 16],
    pub(crate) span: Span,
}

#[cfg(feature = "rust-import")]
#[derive(Clone, Debug, Eq, PartialEq)]
struct ImportedFunctionProof {
    item: crate::host::item::Definition,
    owner: crate::host::facade::Dependency,
    span: Span,
    /// A generic function's type parameters, in its declared order, with
    /// the types the use infers for them, as `window::close::<T>` passed
    /// where `and_then` takes `Fn(Id) -> Task<A>` takes `A`.
    parameters: Box<[(ImmutableString, Ty)]>,
}

#[cfg(feature = "rust-import")]
pub(crate) fn imported_range(source: &str, span: Span) -> Option<(u32, u32)> {
    let start = u32::try_from(location::byte_at(source, span.start())?).ok()?;
    // A span of tokens a macro expansion made from one written token can run
    // past the written one's line, as a report's label reads it: the use is
    // where they were written.
    let end = location::byte_at(source, span.end())
        .and_then(|end| u32::try_from(end).ok())
        .unwrap_or(start);
    // A use a macro's expansion makes may end at the macro's own token, at
    // its call, before a token the caller passed in: it is the use's start.
    Some((start, end.max(start)))
}

/// Rust import paths in one call's lexical scope, each by its segments.
#[cfg(feature = "rust-import")]
type ScopePaths = Box<[Box<[Box<str>]>]>;

#[cfg(feature = "rust-import")]
fn imported_surface_type(
    inference: &Inference,
    models: &Models,
    ty: &Ty,
    operation: Span,
    imported: &mut ImportedContext<'_>,
) -> Result<rawr_import::classify::representability::convert::Type, Error> {
    use rawr_import::classify::representability::convert::{Builtin, Type};
    Ok(match inference.resolved_ref(ty) {
        Ty::Unit => Type::Unit,
        Ty::Bool => Type::Builtin(Builtin::Bool),
        Ty::Char => Type::Builtin(Builtin::Char),
        // An unbound literal is `i32` or `f64`, as in Rust.
        Ty::Float(Float::F32) => Type::Builtin(Builtin::F32),
        Ty::Float(_) => Type::Builtin(Builtin::F64),
        Ty::Int(kind) => Type::Builtin(match kind {
            Int::I8 => Builtin::I8,
            Int::I16 => Builtin::I16,
            Int::I32 | Int::Var(_) => Builtin::I32,
            Int::I64 => Builtin::I64,
            Int::Isize => Builtin::Isize,
            Int::U8 => Builtin::U8,
            Int::U16 => Builtin::U16,
            Int::U32 => Builtin::U32,
            Int::U64 => Builtin::U64,
            Int::Usize => Builtin::Usize,
            Int::I128 | Int::U128 => {
                return Err(unsupported_type_argument(
                    if matches!(kind, Int::I128) {
                        "i128"
                    } else {
                        "u128"
                    }
                    .into(),
                    operation,
                ));
            }
        }),
        // `a..=b` of `i64`, or of literals rustc types from the call, is
        // Rawr's own range. One of another integer is a range of it, as
        // rustc fills a type parameter with it.
        Ty::InclusiveRange(kind) => match inference.resolve(&Ty::Int(*kind)) {
            Ty::Int(Int::I64 | Int::Var(_)) => Type::Builtin(Builtin::InclusiveRange),
            element => {
                imported.retain_checked(Some(1), Some(std::mem::size_of::<Type>()))?;
                Type::Nominal {
                    definition: rawr_import::surface::identity::Definition::range_inclusive(),
                    arguments: Box::new([imported_surface_type(
                        inference, models, &element, operation, imported,
                    )?]),
                }
            }
        },
        Ty::InclusiveOf(element) => {
            imported.retain_checked(Some(1), Some(std::mem::size_of::<Type>()))?;
            Type::Nominal {
                definition: rawr_import::surface::identity::Definition::range_inclusive(),
                arguments: Box::new([imported_surface_type(
                    inference, models, element, operation, imported,
                )?]),
            }
        }
        Ty::Range(element) => {
            imported.retain_checked(Some(1), Some(std::mem::size_of::<Type>()))?;
            Type::Range(Box::new(imported_surface_type(
                inference, models, element, operation, imported,
            )?))
        }
        Ty::String => Type::Builtin(Builtin::String),
        // A `&str`, as a string literal is: a crate's type parameter it
        // fills is `&'static str` in Rust, not `String`.
        Ty::Str => {
            imported.retain_checked(Some(1), Some(std::mem::size_of::<Type>()))?;
            Type::Borrowed {
                mutable: false,
                ty: Box::new(Type::Builtin(Builtin::Str)),
            }
        }
        // `str` itself, as `F` of a `&F` a `&str` fills.
        Ty::UnsizedStr => Type::Builtin(Builtin::Str),
        // A script closure a crate value holds, as iced's `Map<W, F, A>`
        // from `Widget::map` does: the `Box<dyn Fn(..) -> ..>` the bridge
        // boxed it as (T417).
        Ty::Callable { inputs, output, .. }
        | Ty::Fn { inputs, output }
        | Ty::FnItem { inputs, output, .. }
        | Ty::FnChoice { inputs, output, .. } => {
            let mut converted = imported.entries(inputs.len() + 1)?;
            for input in inputs.iter() {
                converted.push(imported_surface_type(
                    inference, models, input, operation, imported,
                )?);
            }
            converted.push(imported_surface_type(
                inference, models, output, operation, imported,
            )?);
            Type::Nominal {
                definition: rawr_import::surface::identity::Definition::callable(),
                arguments: converted.into_boxed_slice(),
            }
        }
        Ty::ProfileMessage { payload, seal, .. } => {
            // The mapper is inline in the outer Type; its payload owns one
            // complete boxed Type in addition to any nested allocations.
            imported.retain_checked(Some(1), Some(std::mem::size_of::<Type>()))?;
            Type::ProfileMessage {
                mapper: *seal,
                payload: Box::new(imported_surface_type(
                    inference, models, payload, operation, imported,
                )?),
            }
        }
        Ty::Host { proof, arguments } => {
            let definition = host_surface_definition(&proof.definition)
                .ok_or_else(|| unsupported_type_argument(ty.name(models), operation))?;
            let mut converted = imported.entries(arguments.len())?;
            for argument in arguments.iter() {
                converted.push(imported_surface_type(
                    inference, models, argument, operation, imported,
                )?);
            }
            Type::Nominal {
                definition,
                arguments: converted.into_boxed_slice(),
            }
        }
        // A program's iterator a crate value holds, as itertools' `Product`
        // holds the two `cartesian_product` took: the native iterator that
        // pulls from it (`future::Items`).
        Ty::Iter(item, _) => Type::Nominal {
            definition: rawr_import::surface::identity::Definition::iterator(),
            arguments: Box::new([imported_surface_type(
                inference, models, item, operation, imported,
            )?]),
        },
        // A `Box<T>`: alloc's, which a crate's signature names (D711
        // step 5).
        Ty::Boxed(inner) => Type::Nominal {
            definition: rawr_import::surface::identity::Definition::boxed(),
            arguments: Box::new([imported_surface_type(
                inference, models, inner, operation, imported,
            )?]),
        },
        // A map or set, which crosses as std's of its kind (#777).
        Ty::Keyed(kind, members) => {
            let mut converted = imported.entries(members.len())?;
            for member in members.iter() {
                converted.push(imported_surface_type(
                    inference, models, member, operation, imported,
                )?);
            }
            Type::Nominal {
                definition: rawr_import::surface::identity::Definition::keyed(kind.name()),
                arguments: converted.into_boxed_slice(),
            }
        }
        // An `Rc<T>` or `Arc<T>` of a crate's type, which stands for the
        // crate's own pointer (#775).
        Ty::Counted(counter, inner) => Type::Nominal {
            definition: rawr_import::surface::identity::Definition::counted(
                *counter == Counter::Arc,
            ),
            arguments: Box::new([imported_surface_type(
                inference, models, inner, operation, imported,
            )?]),
        },
        Ty::Time(Time::Duration) => Type::Nominal {
            definition: rawr_import::surface::identity::Definition::duration(),
            arguments: Box::default(),
        },
        Ty::Time(Time::Instant) => Type::Nominal {
            definition: rawr_import::surface::identity::Definition::instant(),
            arguments: Box::default(),
        },
        // std's `Infallible`, iced's `Never`: the message of a view that
        // sends none, as `impl Widget<Never>` fixes it.
        Ty::ParseError(ParseError::Infallible) => Type::Nominal {
            definition: rawr_import::surface::identity::Definition::infallible(),
            arguments: Box::default(),
        },
        // A type argument `&[u8]` or `&str`, as `Reader<&'a [u8]>` names
        // (D558 tier 1).
        Ty::Reference { mutable: false, .. }
            if let Some(converted @ Type::Borrowed { .. }) =
                instance_surface_type(inference, models, ty) =>
        {
            imported.retain_checked(Some(2), Some(2 * std::mem::size_of::<Type>()))?;
            converted
        }
        // A tuple of Rust's own types is that tuple, as Freya's
        // `background((35, 35, 35))` takes `(u8, u8, u8)`, and so is one
        // holding a crate value (`instance_surface_type`).
        Ty::Tuple(members)
            if instance_surface_type(inference, models, ty)
                .is_some_and(|ty| !is_erased(&ty) || holds_crate_value(&ty)) =>
        {
            let mut converted = imported.entries(members.len())?;
            for member in members.iter() {
                converted.push(imported_surface_type(
                    inference, models, member, operation, imported,
                )?);
            }
            Type::Tuple(converted.into_boxed_slice())
        }
        // An array of Rust's own types is a `Vec` of them, as ratatui's
        // `List::new(rows)` takes `impl IntoIterator<Item: Into<ListItem>>`,
        // and so is one of script values (`instance_surface_type`).
        Ty::Array { element, .. }
            if instance_surface_type(inference, models, ty)
                .is_some_and(|ty| matches!(ty, Type::Sequence { .. })) =>
        {
            imported.retain_checked(Some(1), Some(std::mem::size_of::<Type>()))?;
            Type::Sequence {
                element: Box::new(imported_surface_type(
                    inference, models, element, operation, imported,
                )?),
                owned: true,
                length: None,
            }
        }
        // A type parameter no script value fixes, as iced's `Theme` that the
        // view's widget gives: rustc infers it where the bridge calls the
        // item (D594).
        Ty::Infer(_) => Type::Inferred,
        // A shared borrow of a script value, as `Canvas::new(self)` takes
        // `&Arc` for its `P: Program`: the proxy shares the value's handle,
        // so it is the borrow, and the impls the program writes for the
        // script type are the proxy's (D594).
        Ty::Reference {
            mutable: false,
            target,
        } if matches!(inference.resolved_ref(target), Ty::Script(..)) => {
            imported_surface_type(inference, models, target, operation, imported)?
        }
        // A script type is its proxy in the runner, named as the script
        // names it, so Rust's bounds, `TypeId` and `type_name` are its own
        // (D594).
        Ty::Script(definition, arguments) => {
            let mut converted = imported.entries(arguments.len())?;
            for argument in arguments.iter() {
                converted.push(imported_surface_type(
                    inference, models, argument, operation, imported,
                )?);
            }
            Type::Script {
                path: models.script_path(*definition),
                arguments: converted.into_boxed_slice(),
            }
        }
        // An `Option` or a `Result` holding a crate value is Rust's own
        // (`instance_surface_type`).
        Ty::Option(element)
            if instance_surface_type(inference, models, ty)
                .is_some_and(|ty| matches!(ty, Type::Option(_))) =>
        {
            imported.retain_checked(Some(1), Some(std::mem::size_of::<Type>()))?;
            Type::Option(Box::new(imported_surface_type(
                inference, models, element, operation, imported,
            )?))
        }
        Ty::Result(ok, err)
            if instance_surface_type(inference, models, ty)
                .is_some_and(|ty| matches!(ty, Type::Result(..))) =>
        {
            imported.retain_checked(Some(2), Some(2 * std::mem::size_of::<Type>()))?;
            Type::Result(
                Box::new(imported_surface_type(
                    inference, models, ok, operation, imported,
                )?),
                Box::new(imported_surface_type(
                    inference, models, err, operation, imported,
                )?),
            )
        }
        ty if erased(ty) => Type::Nominal {
            definition: rawr_import::surface::identity::Definition::value(),
            arguments: Box::default(),
        },
        // A type still unknown here is one rustc can't infer either.
        Ty::Unknown => {
            return Err(Error::InvalidHostOperation {
                item: "_".into(),
                reason: "type annotations needed: a crate's type argument is unknown (E0282)"
                    .into(),
                operation,
            });
        }
        other => return Err(unsupported_type_argument(other.name(models), operation)),
    })
}

/// A host type's identity in the runner: an imported crate's own, or one
/// of std's types Rawr's std holds as its Rust value, as `PathBuf` is
/// std's native `PathBuf` (D683.3).
#[cfg(feature = "rust-import")]
fn host_surface_definition(
    definition: &crate::host::type_def::Definition,
) -> Option<rawr_import::surface::identity::Definition> {
    use rawr_import::surface::identity::Definition;
    if let Some(identity) = definition.identity().imported() {
        return Some(Definition::authenticated(*identity.as_bytes()));
    }
    let path = definition.path();
    let std = Definition::std(
        path.strip_prefix(crate::std_facade::ROOT)?
            .strip_prefix("::")?,
    );
    Definition::std_path(std.as_bytes())
        .is_some()
        .then_some(std)
}

/// A type the program gives a crate's generic item that Rawr can't pass
/// to Rust yet, named as the program wrote it.
#[cfg(feature = "rust-import")]
fn unsupported_type_argument(ty: ImmutableString, operation: Span) -> Error {
    Error::InvalidHostOperation {
        reason: unsupported_type_argument_reason(&ty),
        item: ty,
        operation,
    }
}

/// Why a crate's generic item can't take `ty` as its type argument.
#[cfg(feature = "rust-import")]
fn unsupported_type_argument_reason(ty: &str) -> ImmutableString {
    format!("`{ty}` is not supported in Rawr yet as a crate's type argument").into()
}

/// A value of type `ty` that the crate item `item` takes or gives, which
/// Rawr can't pass to Rust yet.
fn unpassable(ty: ImmutableString, item: &str, operation: Span) -> Error {
    Error::InvalidHostOperation {
        item: item.into(),
        reason: format!("a `{ty}` passed to or from it is not supported in Rawr yet").into(),
        operation,
    }
}

/// A method named without its call, as rustc says it (E0615).
fn method_without_call(member: &str) -> ImmutableString {
    format!("attempted to take value of method `{member}`; call it with `{member}(..)` (E0615)")
        .into()
}

/// A crate item whose signature names a type, `ty` when it has a path, that
/// the program's build doesn't reach, at the `operation` that uses the item.
/// Overload selection tries each form and passes over one that fails this
/// way.
fn unnamed_signature_type(item: &str, operation: Span, ty: Option<&str>) -> Error {
    Error::InvalidHostOperation {
        item: item.into(),
        reason: match ty {
            Some(ty) => format!("its signature names `{ty}`, which is not supported in Rawr yet"),
            None => "its signature names a type that is not supported in Rawr yet".into(),
        }
        .into(),
        operation,
    }
}

/// What the program does with a crate's struct or enum, in Rust's words.
#[cfg(feature = "rust-import")]
const fn structural_use(kind: crate::host::structural::Kind) -> &'static str {
    use crate::host::structural::Kind;
    match kind {
        Kind::Constructor => "constructing it",
        Kind::Field { .. } => "reading this field",
        Kind::Variant => "constructing this variant",
        Kind::Match
        | Kind::Test
        | Kind::Take
        | Kind::Lend
        | Kind::LendMut
        | Kind::Read { .. }
        | Kind::Borrow { .. } => "matching on this variant",
        Kind::Store => "assigning to this field",
    }
}

/// A type the crate fixes, as the associated type `<S as Future>::Output`
/// that the type filling `S` binds: Rust's own `Option`, `Result`, tuple or
/// `Vec` of what it holds, never `rawr::Value`, since native code gives or
/// takes it at that type. Anything else is as `imported_surface_type`
/// converts it.
#[cfg(feature = "rust-import")]
fn fixed_surface_type(
    inference: &Inference,
    models: &Models,
    ty: &Ty,
    operation: Span,
    imported: &mut ImportedContext<'_>,
) -> Result<rawr_import::classify::representability::convert::Type, Error> {
    use rawr_import::classify::representability::convert::Type;
    Ok(match inference.resolved_ref(ty) {
        Ty::Option(element) => {
            imported.retain_checked(Some(1), Some(std::mem::size_of::<Type>()))?;
            Type::Option(Box::new(fixed_surface_type(
                inference, models, element, operation, imported,
            )?))
        }
        Ty::Result(ok, err) => {
            imported.retain_checked(Some(2), Some(2 * std::mem::size_of::<Type>()))?;
            Type::Result(
                Box::new(fixed_surface_type(
                    inference, models, ok, operation, imported,
                )?),
                Box::new(fixed_surface_type(
                    inference, models, err, operation, imported,
                )?),
            )
        }
        Ty::Tuple(members) if !members.is_empty() => {
            let mut converted = imported.entries(members.len())?;
            for member in members.iter() {
                converted.push(fixed_surface_type(
                    inference, models, member, operation, imported,
                )?);
            }
            Type::Tuple(converted.into_boxed_slice())
        }
        Ty::Array { element, .. } => {
            imported.retain_checked(Some(1), Some(std::mem::size_of::<Type>()))?;
            Type::Sequence {
                element: Box::new(fixed_surface_type(
                    inference, models, element, operation, imported,
                )?),
                owned: true,
                length: None,
            }
        }
        _ => imported_surface_type(inference, models, ty, operation, imported)?,
    })
}

/// An instance type as `fixed_surface_type` converts it, without
/// allocation accounting: the key of a call's instance whose parameter an
/// associated type reads.
#[cfg(feature = "rust-import")]
fn fixed_instance_surface_type(
    inference: &Inference,
    models: &Models,
    ty: &Ty,
) -> Option<rawr_import::classify::representability::convert::Type> {
    use rawr_import::classify::representability::convert::Type;
    let fixed = |ty| fixed_instance_surface_type(inference, models, ty);
    Some(match inference.resolved_ref(ty) {
        Ty::Option(element) => Type::Option(Box::new(fixed(element)?)),
        Ty::Result(ok, err) => Type::Result(Box::new(fixed(ok)?), Box::new(fixed(err)?)),
        Ty::Tuple(members) if !members.is_empty() => {
            Type::Tuple(members.iter().map(fixed).collect::<Option<_>>()?)
        }
        Ty::Array { element, .. } => Type::Sequence {
            element: Box::new(fixed(element)?),
            owned: true,
            length: None,
        },
        _ => instance_surface_type(inference, models, ty)?,
    })
}

/// The requirements that are an imported item's instance, in the order its
/// instance names them: the item's declared order, or the requirements' own
/// when a declared parameter has none. Its const lengths are its constants,
/// never its types (#1330).
#[cfg(feature = "rust-import")]
fn instance_parameters<'r>(
    item: &crate::host::item::Definition,
    requirements: &'r [(ImmutableString, Ty, capability::Set)],
) -> Vec<&'r (ImmutableString, Ty, capability::Set)> {
    let lengths = item
        .signature()
        .declared_length_parameters()
        .collect::<Vec<_>>();
    let typed = requirements
        .iter()
        .filter(|(name, _, _)| !lengths.contains(name))
        .collect::<Vec<_>>();
    let ordered = item
        .signature()
        .declared_type_parameters()
        .filter_map(|name| {
            requirements
                .iter()
                .find(|(candidate, _, _)| *candidate == name)
        })
        .collect::<Vec<_>>();
    if ordered.len() == typed.len() {
        ordered
    } else {
        typed
    }
}

/// Whether an associated type a relation reads names `parameter`, as
/// `S::Output` of the type filling `S` does: that type is the crate's,
/// Rust's own (`fixed_surface_type`).
#[cfg(feature = "rust-import")]
fn fixed_parameter(item: &crate::host::item::Definition, parameter: &str) -> bool {
    item.signature().relations().iter().any(|relation| {
        relation.associated().is_some()
            && matches!(relation.arguments(),
                [crate::host::signature::Pattern::Parameter(name)] if name == parameter)
    })
}

/// A type argument of a crate's generic item that keys none of its
/// instances, in rustc's words: one it can't infer, or one Rawr can't pass
/// to Rust yet.
#[cfg(feature = "rust-import")]
fn keyless_type_argument(
    inference: &Inference,
    models: &Models,
    item: &crate::host::item::Definition,
    parameter: &str,
    ty: &Ty,
    operation: Span,
) -> Error {
    let reason = match inference.resolve(ty) {
        Ty::Unknown => "type annotations needed: a crate's type argument is unknown (E0282)".into(),
        ty => unsupported_type_argument_reason(&ty.name(models)),
    };
    Error::InvalidHostOperation {
        item: format!("{}'s `{parameter}`", item.path()).into(),
        reason,
        operation,
    }
}

/// A const argument of a crate's generic item rustc can't infer either.
#[cfg(feature = "rust-import")]
fn unknown_const_argument(item: &crate::host::item::Definition, operation: Span) -> Error {
    Error::InvalidHostOperation {
        item: item.path().into(),
        reason: "type annotations needed: a const argument of it is unknown (E0282)".into(),
        operation,
    }
}

/// A value type Rust can't name in the runner, as a script struct or a
/// tuple holding one: a generic Rust item takes it as `rawr::Value`
/// (D513.2), which carries it as it is.
#[cfg(feature = "rust-import")]
fn erased(ty: &Ty) -> bool {
    matches!(
        ty,
        Ty::Script(..)
            | Ty::Array { .. }
            | Ty::Tuple(_)
            | Ty::Option(_)
            | Ty::Result(..)
            | Ty::Keyed(..)
            | Ty::Boxed(..)
            | Ty::Counted(..)
            | Ty::Weak(..)
            | Ty::Reverse(_)
            | Ty::Cell(..)
            | Ty::Ordering
            | Ty::Time(Time::Instant)
    )
}

/// An instance's array as the Rust array of `length`, where a bound's case
/// fixes it (`related_array_length`).
#[cfg(feature = "rust-import")]
fn fix_array_length(
    surface: &mut rawr_import::classify::representability::convert::Type,
    length: Option<u32>,
) {
    if let rawr_import::classify::representability::convert::Type::Sequence {
        owned: true,
        length: fixed @ None,
        ..
    } = surface
    {
        *fixed = length;
    }
}

/// The const parameter whose length an input `[T; N]` or `&[T; N]` takes.
#[cfg(feature = "rust-import")]
fn length_parameter(ty: &crate::host::type_ref::Type) -> Option<&ImmutableString> {
    match ty.shape() {
        crate::host::type_ref::Shape::Array {
            sequence: crate::host::type_ref::Sequence::Length(name),
            ..
        } => Some(name),
        crate::host::type_ref::Shape::Reference { target, .. } => length_parameter(target),
        _ => None,
    }
}

/// The length of an argument's array, or of the array it borrows.
#[cfg(feature = "rust-import")]
fn array_length(ty: &Ty) -> Option<usize> {
    match ty {
        Ty::Array {
            kind: sequence::Kind::Array(length),
            ..
        } => Some(*length),
        Ty::Reference { target, .. } => array_length(target),
        _ => None,
    }
}

/// `T` of a crate's `RangeInclusive<T>` of a type parameter.
fn range_inclusive_element(
    ty: &crate::host::type_ref::Type,
) -> Option<&crate::host::type_ref::Type> {
    match ty.shape() {
        crate::host::type_ref::Shape::RangeInclusive(element) => Some(element),
        _ => None,
    }
}

/// An instance type as `imported_surface_type` converts it, without
/// allocation accounting: the key of a call's instance.
#[cfg(feature = "rust-import")]
fn instance_surface_type(
    inference: &Inference,
    models: &Models,
    ty: &Ty,
) -> Option<rawr_import::classify::representability::convert::Type> {
    use rawr_import::classify::representability::convert::{Builtin, Type};
    Some(match inference.resolved_ref(ty) {
        Ty::Unit => Type::Unit,
        Ty::Bool => Type::Builtin(Builtin::Bool),
        Ty::Char => Type::Builtin(Builtin::Char),
        Ty::Float(Float::F32) => Type::Builtin(Builtin::F32),
        Ty::Float(_) => Type::Builtin(Builtin::F64),
        Ty::Int(kind) => Type::Builtin(match kind {
            Int::I8 => Builtin::I8,
            Int::I16 => Builtin::I16,
            Int::I32 | Int::Var(_) => Builtin::I32,
            Int::I64 => Builtin::I64,
            Int::Isize => Builtin::Isize,
            Int::U8 => Builtin::U8,
            Int::U16 => Builtin::U16,
            Int::U32 => Builtin::U32,
            Int::U64 => Builtin::U64,
            Int::Usize => Builtin::Usize,
            Int::I128 | Int::U128 => return None,
        }),
        Ty::InclusiveRange(kind) => match inference.resolve(&Ty::Int(*kind)) {
            Ty::Int(Int::I64 | Int::Var(_)) => Type::Builtin(Builtin::InclusiveRange),
            element => Type::Nominal {
                definition: rawr_import::surface::identity::Definition::range_inclusive(),
                arguments: Box::new([instance_surface_type(inference, models, &element)?]),
            },
        },
        Ty::InclusiveOf(element) => Type::Nominal {
            definition: rawr_import::surface::identity::Definition::range_inclusive(),
            arguments: Box::new([instance_surface_type(inference, models, element)?]),
        },
        Ty::Range(element) => {
            Type::Range(Box::new(instance_surface_type(inference, models, element)?))
        }
        Ty::String => Type::Builtin(Builtin::String),
        Ty::Str => Type::Borrowed {
            mutable: false,
            ty: Box::new(Type::Builtin(Builtin::Str)),
        },
        // The unsized `str` a literal fills `F` of `&'f F` with, as jiff's
        // `strftime` takes it: the key of the `strftime::<str>` adapter, as
        // `imported_surface_type` names it, never the item's other
        // instances' shared binding.
        Ty::UnsizedStr => Type::Builtin(Builtin::Str),
        Ty::ProfileMessage { payload, seal, .. } => Type::ProfileMessage {
            mapper: *seal,
            payload: Box::new(instance_surface_type(inference, models, payload)?),
        },
        Ty::Host { proof, arguments } => Type::Nominal {
            definition: host_surface_definition(&proof.definition)?,
            arguments: arguments
                .iter()
                .map(|argument| instance_surface_type(inference, models, argument))
                .collect::<Option<_>>()?,
        },
        Ty::Boxed(inner) => Type::Nominal {
            definition: rawr_import::surface::identity::Definition::boxed(),
            arguments: Box::new([instance_surface_type(inference, models, inner)?]),
        },
        // A program's iterator a crate value holds (`future::Items`).
        Ty::Iter(item, _) => Type::Nominal {
            definition: rawr_import::surface::identity::Definition::iterator(),
            arguments: Box::new([instance_surface_type(inference, models, item)?]),
        },
        Ty::Counted(counter, inner) => Type::Nominal {
            definition: rawr_import::surface::identity::Definition::counted(
                *counter == Counter::Arc,
            ),
            arguments: Box::new([instance_surface_type(inference, models, inner)?]),
        },
        Ty::Keyed(kind, members) => Type::Nominal {
            definition: rawr_import::surface::identity::Definition::keyed(kind.name()),
            arguments: members
                .iter()
                .map(|member| instance_surface_type(inference, models, member))
                .collect::<Option<_>>()?,
        },
        Ty::Time(Time::Duration) => Type::Nominal {
            definition: rawr_import::surface::identity::Definition::duration(),
            arguments: Box::default(),
        },
        Ty::Time(Time::Instant) => Type::Nominal {
            definition: rawr_import::surface::identity::Definition::instant(),
            arguments: Box::default(),
        },
        Ty::ParseError(ParseError::Infallible) => Type::Nominal {
            definition: rawr_import::surface::identity::Definition::infallible(),
            arguments: Box::default(),
        },
        // A script closure a crate value holds, as `Map<W, F, A>` does: the
        // boxed closure `imported_surface_type` names, so a call on the
        // value keys the adapter generated for it (T417).
        Ty::Callable { inputs, output, .. }
        | Ty::Fn { inputs, output }
        | Ty::FnItem { inputs, output, .. }
        | Ty::FnChoice { inputs, output, .. } => Type::Nominal {
            definition: rawr_import::surface::identity::Definition::callable(),
            arguments: inputs
                .iter()
                .chain([&**output])
                .map(|ty| instance_surface_type(inference, models, ty))
                .collect::<Option<_>>()?,
        },
        // A shared borrow of a script value is its proxy, which shares it.
        Ty::Reference {
            mutable: false,
            target,
        } if matches!(inference.resolved_ref(target), Ty::Script(..)) => {
            instance_surface_type(inference, models, target)?
        }
        // A type argument `&[u8]` or `&str`, as `Reader<&'a [u8]>` names
        // (D558 tier 1).
        Ty::Reference {
            mutable: false,
            target,
        } => Type::Borrowed {
            mutable: false,
            ty: Box::new(match inference.resolve(target) {
                Ty::Array { element, .. } => Type::Sequence {
                    element: Box::new(instance_surface_type(inference, models, &element)?),
                    owned: false,
                    length: None,
                },
                Ty::Str | Ty::String => Type::Builtin(Builtin::Str),
                // A crate value the program holds, as `Some(&self.backend)`
                // fills iced's `pick_list`'s `V: Borrow<T>`.
                target @ Ty::Host { .. } => instance_surface_type(inference, models, &target)?,
                _ => return None,
            }),
        },
        // A tuple of Rust's own types is that tuple. So is one that holds
        // a crate value beside script values, as iced's `new` gives
        // `(Self, Task<Message>)`: native code takes the crate value as
        // itself and each script value as its proxy, where a `rawr::Value`
        // would carry a handle it can't use.
        Ty::Tuple(members)
            if let Some(converted) = members
                .iter()
                .map(|member| instance_surface_type(inference, models, member))
                .collect::<Option<Box<[_]>>>()
                .filter(|converted| {
                    !converted.iter().any(is_erased) || converted.iter().any(holds_crate_value)
                }) =>
        {
            Type::Tuple(converted)
        }
        // An `Option` or a `Result` that holds a crate value is Rust's own,
        // as iced's `font::list()` gives `Task<Result<Vec<Family>, Error>>`
        // and `.map(Result::ok)` takes it: native code holds the crate
        // value as itself.
        Ty::Option(element)
            if let Some(element) =
                instance_surface_type(inference, models, element).filter(holds_crate_value) =>
        {
            Type::Option(Box::new(element))
        }
        Ty::Result(ok, err)
            if let (Some(ok), Some(err)) = (
                instance_surface_type(inference, models, ok),
                instance_surface_type(inference, models, err),
            ) && (holds_crate_value(&ok) || holds_crate_value(&err)) =>
        {
            Type::Result(Box::new(ok), Box::new(err))
        }
        // An array of Rust's own types, or of a script type's proxies as a
        // slice of them is, is a `Vec` of them, as serde's
        // `serialize_field` takes `&Vec<Task>` for its `T: Serialize`.
        Ty::Array { element, .. }
            if let Some(element) =
                instance_surface_type(inference, models, element).filter(|element| {
                    !is_erased(element) || matches!(element, Type::Script { .. })
                }) =>
        {
            Type::Sequence {
                element: Box::new(element),
                owned: true,
                length: None,
            }
        }
        Ty::Infer(_) => Type::Inferred,
        Ty::Script(definition, arguments) => Type::Script {
            path: models.script_path(*definition),
            arguments: arguments
                .iter()
                .map(|argument| instance_surface_type(inference, models, argument))
                .collect::<Option<_>>()?,
        },
        ty if erased(ty) => Type::Nominal {
            definition: rawr_import::surface::identity::Definition::value(),
            arguments: Box::default(),
        },
        _ => return None,
    })
}

/// A `Range<T>` value's runtime type: a `Range<f64>` of floats, else a
/// `Range<i64>` of integers or chars' code points.
fn range_type_id(element: &Ty) -> TypeId {
    if matches!(element, Ty::Float(_)) {
        TypeId::of::<std::ops::Range<crate::FLOAT>>()
    } else {
        TypeId::of::<std::ops::Range<INT>>()
    }
}

/// Whether Rust holds some part of `ty` as a bare script value. Unlike
/// [`is_erased`], a script type counts as its proxy, a Rust type of its own.
#[cfg(feature = "rust-import")]
fn holds_value(ty: &rawr_import::classify::representability::convert::Type) -> bool {
    use rawr_import::classify::representability::convert::Type;
    match ty {
        Type::Nominal {
            definition,
            arguments,
        } => {
            *definition == rawr_import::surface::identity::Definition::value()
                || arguments.iter().any(holds_value)
        }
        Type::Script { arguments, .. } | Type::Tuple(arguments) => {
            arguments.iter().any(holds_value)
        }
        Type::Option(element)
        | Type::Borrowed { ty: element, .. }
        | Type::Range(element)
        | Type::Sequence { element, .. }
        | Type::ConstArray { element, .. } => holds_value(element),
        Type::Result(ok, err) => holds_value(ok) || holds_value(err),
        Type::ProfileMessage { payload, .. } => holds_value(payload),
        Type::Inferred
        | Type::Unit
        | Type::Never
        | Type::Builtin(_)
        | Type::FunctionItem { .. }
        | Type::Parameter(_) => false,
    }
}

/// Whether an instance type holds a crate's own value, which native code
/// holds as itself and a `rawr::Value` could only carry as a handle.
#[cfg(feature = "rust-import")]
fn holds_crate_value(ty: &rawr_import::classify::representability::convert::Type) -> bool {
    use rawr_import::classify::representability::convert::Type;
    match ty {
        Type::Nominal { definition, .. } => {
            *definition != rawr_import::surface::identity::Definition::value()
        }
        Type::Tuple(members) => members.iter().any(holds_crate_value),
        Type::Option(element)
        | Type::Sequence { element, .. }
        | Type::ConstArray { element, .. } => holds_crate_value(element),
        Type::Result(ok, err) => holds_crate_value(ok) || holds_crate_value(err),
        Type::Script { .. }
        | Type::Borrowed { .. }
        | Type::Range(_)
        | Type::ProfileMessage { .. }
        | Type::Inferred
        | Type::Unit
        | Type::Never
        | Type::Builtin(_)
        | Type::FunctionItem { .. }
        | Type::Parameter(_) => false,
    }
}

/// Whether an instance type holds a value Rust takes as `rawr::Value`.
#[cfg(feature = "rust-import")]
fn is_erased(ty: &rawr_import::classify::representability::convert::Type) -> bool {
    use rawr_import::classify::representability::convert::Type;
    match ty {
        Type::Nominal {
            definition,
            arguments,
        } => {
            *definition == rawr_import::surface::identity::Definition::value()
                || arguments.iter().any(is_erased)
        }
        Type::Tuple(members) => members.iter().any(is_erased),
        // A script type's proxy holds a script value (D594).
        Type::Script { .. } => true,
        Type::Option(element)
        | Type::Borrowed { ty: element, .. }
        | Type::Range(element)
        | Type::Sequence { element, .. }
        | Type::ConstArray { element, .. } => is_erased(element),
        Type::Result(ok, err) => is_erased(ok) || is_erased(err),
        Type::ProfileMessage { payload, .. } => is_erased(payload),
        Type::Inferred => false,
        Type::Unit
        | Type::Never
        | Type::Builtin(_)
        | Type::FunctionItem { .. }
        | Type::Parameter(_) => false,
    }
}

pub(super) fn lower(
    engine: &Engine,
    program: &Program,
    runtime_model: &crate::program::model::Model,
    scope: Option<&Scope>,
    optimization_level: crate::OptimizationLevel,
) -> Result<Output, LoweringFailure> {
    let models = Models::new(
        program,
        runtime_model.clone(),
        engine.host_registry.share(),
        engine,
    );
    lower_with_context(
        engine,
        program,
        models,
        scope,
        optimization_level,
        #[cfg(feature = "rust-import")]
        None,
    )
}

/// Lower executable support and retain its exact imported uses in the same pass.
#[cfg(feature = "rust-import")]
pub(crate) fn with_uses<A: crate::host::ImportAllocation>(
    engine: &Engine,
    program: &Program,
    runtime_model: &crate::program::model::Model,
    allocation: &mut A,
    scope: Option<&Scope>,
    optimization_level: crate::OptimizationLevel,
) -> Result<(Output, ImportedOutput), super::ImportedFailure> {
    let mut models = Models::new(
        program,
        runtime_model.clone(),
        engine.host_registry.share(),
        engine,
    );
    models._imported_owner = allocation.support_owner();
    let mut uses = Vec::new();
    let mut dependencies = BTreeSet::new();
    let mut allocation_failure = None;
    let impls = native_impls(program, &models)?.into_boxed_slice();
    let lowered = lower_with_context(
        engine,
        program,
        models,
        scope,
        optimization_level,
        Some(ImportedContext {
            native: program.native.as_deref(),
            allocation,
            failure: &mut allocation_failure,
            source: program.source.entry(),
            uses: &mut uses,
            dependencies: &mut dependencies,
        }),
    );
    if let Some(error) = allocation_failure {
        return Err(super::ImportedFailure::Surface(error));
    }
    let lowered = lowered.map_err(super::ImportedFailure::Lowering)?;
    uses.sort_unstable();
    uses.dedup();
    Ok((
        lowered,
        ImportedOutput {
            uses: uses.into_boxed_slice(),
            impls,
        },
    ))
}

/// A function's or a method's body, which `lower_with_context` lowers.
#[derive(Clone, Copy)]
enum Body<'a> {
    Function(&'a item::Shell, &'a item::Function),
    Method(&'a method::Definition),
}

impl Body<'_> {
    /// Where the program writes the body: its source, then its place there.
    fn written_at(&self) -> (usize, Position) {
        match self {
            Self::Function(shell, definition) => {
                (shell.source.index(), definition.body.span.start())
            }
            Self::Method(definition) => (definition.source().index(), definition.body().start()),
        }
    }
}

fn lower_with_context(
    engine: &Engine,
    program: &Program,
    models: Models,
    scope: Option<&Scope>,
    optimization_level: crate::OptimizationLevel,
    #[cfg(feature = "rust-import")] mut imported: Option<ImportedContext<'_>>,
) -> Result<Output, LoweringFailure> {
    // The name the program gave each generated function's receiver, which
    // its body reads as `this`: a debugger reads it by that name (#534).
    let mut receivers: BTreeMap<ImmutableString, ImmutableString> = BTreeMap::new();
    // Where each free function's `&mut` parameter, its receiver, is written
    // among its parameters, for a host call that passes it there.
    let mut written_receivers: BTreeMap<ImmutableString, usize> = BTreeMap::new();
    let mut symbol_rewrites: BTreeMap<ImmutableString, ImmutableString> = program
        .methods
        .iter()
        .chain(&program.builtin_methods)
        .map(|definition| {
            (
                method_name(definition.id()).into(),
                private_method_name(definition).into(),
            )
        })
        .collect();
    // A script's functions keep their names, for the host to call. The
    // lowered text calls them by a sentinel, so a name the lowering writes
    // for std (`abs(x)` of `x.abs()`, `v.push(x)`) is never one of them,
    // and that name reaches only the runtime's own function, as rustc
    // resolves a method before any free function.
    let aggregate = program.source.is_aggregate();
    symbol_rewrites.extend(program.items.iter().flat_map(|shell| {
        let item::Kind::Function(function) = &shell.kind else {
            return Vec::new();
        };
        if aggregate {
            let key = shell
                .key
                .as_ref()
                .expect("resolved functions retain stable item keys");
            let private = (function_name(shell.id).into(), private_function_name(key));
            // A root function a single file's program also names by its own
            // name (`Executable::into_one_file`) is never a std call's.
            if key.modules().is_empty() && key.owner().is_none() {
                return vec![private, (function.name.text.clone(), NATIVE_ONLY.into())];
            }
            return vec![private];
        }
        vec![
            (function_name(shell.id).into(), function.name.text.clone()),
            (function.name.text.clone(), NATIVE_ONLY.into()),
        ]
    }));
    let mut generated = location::Generated::new();
    let mut closures = Vec::new();
    let mut locations = Vec::new();
    let mut ops = Vec::new();
    let mut patterns = Vec::new();

    // `type Output: Clone;`: each impl's associated type has its trait's
    // bounds, and its owner the trait's supertraits, as Rust checks at the
    // impl.
    for shell in program.items.iter() {
        let item::Kind::Impl(implementation) = &shell.kind else {
            continue;
        };
        let supertraits = match &implementation.trait_ {
            Some(item::TraitImpl::User { name, .. }) => {
                models.traits.get(&name.text).is_some_and(|(_, declared)| {
                    !declared.supertraits.1.is_empty()
                        || declared.supertraits.0.iter().next().is_some()
                })
            }
            _ => false,
        };
        if implementation.associated.is_empty() && !supertraits {
            continue;
        }
        let mut parser = Parser::new_with_offset(
            engine,
            "",
            models.for_source(shell.source),
            0,
            0,
            #[cfg(feature = "rust-import")]
            None,
        );
        parser
            .check_associated_bounds(implementation)
            .map_err(|error| LoweringFailure::original(shell.source, error.into()))?;
    }

    // Bodies are checked in the order the program writes them, so the
    // first error reported is the first rustc reports.
    let mut bodies = program
        .items
        .iter()
        .filter_map(|shell| match &shell.kind {
            item::Kind::Function(definition) => Some(Body::Function(shell, definition)),
            _ => None,
        })
        .chain(
            program
                .methods
                .iter()
                .chain(&program.builtin_methods)
                .map(Body::Method),
        )
        .collect::<Vec<_>>();
    bodies.sort_by_key(Body::written_at);
    for body in bodies {
        match body {
            Body::Function(shell, definition) => {
                // A template compiled per instantiation is lowered as its
                // instances only (D629).
                if shell
                    .key
                    .as_ref()
                    .is_some_and(|key| program.instances.templates.contains_key(key))
                {
                    continue;
                }
                let body_source = body_source(
                    definition
                        .body
                        .text
                        .as_deref()
                        .unwrap_or_else(|| program.source.text_at(shell.source)),
                    &definition.body,
                );
                let mut parser = Parser::for_function(
                    engine,
                    &body_source,
                    models.for_source(shell.source),
                    definition,
                    shell
                        .key
                        .clone()
                        .expect("resolved functions retain stable item keys"),
                    ops.len(),
                    patterns.len(),
                    #[cfg(feature = "rust-import")]
                    imported
                        .as_mut()
                        .map(|context| context.for_source(shell.source)),
                );
                let lowered = parser
                    .parse_function_body(definition)
                    .map_err(|error| LoweringFailure::original(shell.source, error.into()))?;
                let generated_name = function_name(shell.id);
                if let Some(index) = this_parameter(definition) {
                    receivers.insert(
                        generated_name.as_str().into(),
                        definition.parameters[index].name.text.clone(),
                    );
                    written_receivers.insert(generated_name.as_str().into(), index);
                }
                let body = write_function(
                    &mut generated,
                    shell.source,
                    &generated_name,
                    definition,
                    &lowered,
                    &models,
                );
                retain_locations(
                    &mut locations,
                    body,
                    Some(
                        shell
                            .key
                            .as_ref()
                            .expect("resolved functions retain stable item keys"),
                    ),
                    shell.source,
                    lowered.locations,
                );
                ops.extend(lowered.ops);
                patterns.extend(lowered.patterns);
                write_closures(
                    &mut generated,
                    &mut locations,
                    shell.source,
                    lowered.closures,
                    &mut closures,
                    &mut symbol_rewrites,
                );
            }
            Body::Method(definition) => {
                // A method compiled per instantiation is lowered as its instances
                // only (D629).
                if program.instances.templates.contains_key(definition.key()) {
                    continue;
                }
                let body_source = source_span(
                    definition
                        .body_text()
                        .unwrap_or_else(|| program.source.text_at(definition.source())),
                    definition.body(),
                );
                let mut parser = Parser::for_method(
                    engine,
                    &body_source,
                    models.for_source(definition.source()),
                    definition,
                    ops.len(),
                    patterns.len(),
                    #[cfg(feature = "rust-import")]
                    imported
                        .as_mut()
                        .map(|context| context.for_source(definition.source())),
                )
                .map_err(|error| LoweringFailure::original(definition.source(), error.into()))?;
                let lowered = parser.parse_method_body(definition).map_err(|error| {
                    LoweringFailure::original(definition.source(), error.into())
                })?;
                if definition.receiver().is_some() {
                    receivers.insert(method_name(definition.id()).into(), "self".into());
                }
                let body = write_method(&mut generated, definition, &lowered, &models);
                retain_locations(
                    &mut locations,
                    body,
                    Some(definition.key()),
                    definition.source(),
                    lowered.locations,
                );
                ops.extend(lowered.ops);
                patterns.extend(lowered.patterns);
                write_closures(
                    &mut generated,
                    &mut locations,
                    definition.source(),
                    lowered.closures,
                    &mut closures,
                    &mut symbol_rewrites,
                );
            }
        }
    }

    write_trait_dispatchers(&mut generated, &models, &mut ops);
    write_default_dispatcher(&mut generated, program, &models, &mut ops);

    let script = program
        .scripts
        .iter()
        .find(|script| script.source == program.source.entry())
        .expect("resolved single-source programs retain their root script");
    let mut parser = Parser::new_with_offset(
        engine,
        &script.text,
        models.for_source(script.source),
        ops.len(),
        patterns.len(),
        #[cfg(feature = "rust-import")]
        imported
            .as_mut()
            .map(|context| context.for_source(script.source)),
    );
    if let Some(scope) = scope {
        parser.bind_scope(scope);
    }
    let lowered = parser
        .parse()
        .map_err(|error| LoweringFailure::original(script.source, error.into()))?;
    let body = generated.body(
        script.source,
        source_full_span(program.source.text_at(script.source)),
        &lowered.source,
        &lowered.locations,
    );
    // The script's statements: a runtime error in them is reported at
    // the script's own text.
    retain_locations(&mut locations, body, None, script.source, lowered.locations);
    ops.extend(lowered.ops);
    patterns.extend(lowered.patterns);

    let mut ast = match engine.compile_legacy_script_with_scope_raw(
        scope,
        generated.as_str(),
        optimization_level,
    ) {
        Ok(ast) => ast,
        Err(error) => {
            return Err(match generated.translate(error.primary_span()) {
                Ok(translated) => {
                    LoweringFailure::generated(translated.source, translated.span, error)
                }
                Err(origin) => LoweringFailure::unmapped(
                    format!("cannot retain compiler diagnostic origin: {origin}"),
                    error,
                ),
            });
        }
    };
    let mut module = Module::new();
    let mut rewrites = symbol_rewrites;
    let support_id = models.support_id();
    let models = Shared::new(models);
    let dependencies = ops
        .iter()
        .flat_map(ExecutableOp::dependencies)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let runtime_requirements = ops
        .iter()
        .filter_map(ExecutableOp::runtime_requirement)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let projection_requirements = ops
        .iter()
        .filter_map(ExecutableOp::projection_requirement)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let nominal_patterns: BTreeMap<ImmutableString, NominalPattern> = patterns
        .into_iter()
        .enumerate()
        .map(|(index, pattern)| (pattern_name(index).into(), pattern))
        .collect();

    for (index, op) in ops.into_iter().enumerate() {
        let sentinel: ImmutableString = format!("{SENTINEL}{index}").into();
        let private: ImmutableString = op.private_name(support_id).into();
        rewrites.insert(sentinel, private.clone());
        register(&mut module, private, op, &models);
    }
    // A debugger reads any value's `Debug` text through it (#488). A
    // program that formats with `Debug` registered it above; one that
    // doesn't carries it only while a debugger is registered (#526).
    debug_assert_eq!(
        format!("\0rawr.{}", PreludeOp::GenericDebug.name()),
        DEBUG_GENERIC
    );
    debug_assert_eq!(trait_dispatcher_name("Debug", "fmt"), DEBUG_DISPATCHER);
    #[cfg(feature = "debugging")]
    if engine.is_debugger_registered() {
        register(
            &mut module,
            DEBUG_GENERIC.into(),
            ExecutableOp::Prelude(PreludeOp::GenericDebug),
            &models,
        );
    }

    rewrite_statements(ast.statements_mut(), &rewrites, &nominal_patterns);
    #[cfg(feature = "debugging")]
    let named = engine.is_debugger_registered();
    #[cfg(not(feature = "debugging"))]
    let named = false;
    ast.for_each_fn_def_mut(|function| {
        rewrite_statements(function.body.statements_mut(), &rewrites, &nominal_patterns);
        if named {
            function.this_name = receivers.get(&function.name).cloned();
        }
        function.this_parameter = written_receivers.get(&function.name).copied();
        if let Some(name) = rewrites.get(&function.name)
            && name.as_str() != NATIVE_ONLY
        {
            function.name = name.clone();
        }
    });
    // Each async body becomes a state machine, once its names are final.
    let mut unsupported = None;
    ast.for_each_fn_def_mut(|function| {
        if unsupported.is_none() {
            unsupported = crate::eval::machine::lower(function).err();
        }
    });
    if let Some(crate::eval::machine::Unsupported(what, position)) = unsupported {
        let error = crate::ParseError(
            Box::new(crate::ParseErrorType::BadInput(
                crate::LexError::ImproperSymbol(
                    "await".into(),
                    format!("{what} is not supported in Rawr yet"),
                ),
            )),
            position,
            crate::types::parse_error::Rendered::default(),
        );
        return Err(match generated.translate(Span::new(position, position)) {
            Ok(translated) => LoweringFailure::generated(translated.source, translated.span, error),
            Err(origin) => LoweringFailure::unmapped(
                format!("cannot retain compiler diagnostic origin: {origin}"),
                error,
            ),
        });
    }
    ast.reindex_script_functions();
    ast.add_support(module);
    let locations = resolve_locations(generated.as_str(), locations);
    Ok(Output {
        ast,
        locations: locations.into_boxed_slice(),
        dependencies,
        runtime_requirements,
        projection_requirements,
        closures: closures.into_boxed_slice(),
    })
}

pub(crate) struct Output {
    pub(crate) ast: AST,
    pub(crate) locations: Box<[Location]>,
    pub(crate) dependencies: Box<[crate::host::facade::Dependency]>,
    pub(crate) runtime_requirements: Box<[crate::host::runtime::Requirement]>,
    pub(crate) projection_requirements: Box<[crate::host::projection::Site]>,
    pub(crate) closures: Box<[closure::Callable]>,
}

/// Compiler-created evidence translating one generated callback range.
pub(crate) struct Location {
    pub(crate) callable: Option<crate::item::Key>,
    pub(crate) source: crate::program::source::Id,
    pub(crate) generated: Span,
    pub(crate) original: Span,
    pub(crate) nesting: usize,
}

/// The index of a free function's `&mut` parameter, which is its receiver
/// `this` at runtime: the function changes the caller's place in place,
/// and a call `f(&mut v, x)` is the method call `v.f(x)`.
/// `source` lexed as checked Rawr reads Rust, through its EOF marker.
pub(crate) fn lex_rust(engine: &Engine, source: &str) -> Vec<(Token, Position)> {
    let inputs = [source];
    let (mut stream, control) = engine.lex_raw(&inputs, None);
    control.borrow_mut().rust = true;
    let mut tokens = Vec::new();
    loop {
        let token = stream
            .next()
            .expect("token streams always produce their EOF marker");
        let is_eof = token.0 == Token::EOF;
        let token = match token {
            // Rust has no power operator: binary `**` is `*` then a
            // deref (`a * *b`), and `**=` is `*` then `*=`. The untyped
            // script lexer keeps `**`.
            (Token::PowerOf | Token::PowerOfAssign, position) => {
                let second = if token.0 == Token::PowerOf {
                    Token::Multiply
                } else {
                    Token::MultiplyAssign
                };
                let mut next = position;
                next.advance();
                tokens.push((Token::Multiply, position));
                (second, next)
            }
            token => token,
        };
        tokens.push(token);
        if is_eof {
            break;
        }
    }
    tokens
}

/// A free function's first `&mut` parameter, which a call passes as its
/// receiver's place. Not its `&mut fmt::Formatter`, whose text the body
/// gives back (`wrap_formatter`).
/// `names` with each of `from`'s names also in `to`, where `to` names
/// nothing of its own by that name.
fn inherit_names<V: Clone>(
    names: &mut BTreeMap<(crate::program::source::Id, ImmutableString), V>,
    from: crate::program::source::Id,
    to: crate::program::source::Id,
) {
    let inherited = names
        .iter()
        .filter(|((source, _), _)| *source == from)
        .map(|((_, name), value)| (name.clone(), value.clone()))
        .collect::<Vec<_>>();
    for (name, value) in inherited {
        names.entry((to, name)).or_insert(value);
    }
}

pub(crate) fn this_parameter(definition: &item::Function) -> Option<usize> {
    definition.parameters.iter().position(|parameter| {
        matches!(
            parameter.ty,
            type_ref::Type::Reference { mutable: true, .. }
        ) && !super::direct_mutable_invocation(&parameter.ty)
            && definition.formatter.as_ref() != Some(&parameter.name.text)
    })
}

/// The `&mut` parameters a call copies in and back out: a free
/// function's after its receiver (`this_parameter`), an inherent method's
/// or associated function's all. Rust's borrow rules keep them apart, so
/// the copies change the caller's places as the references would. Not the
/// `formatter`, whose text the body gives back (`wrap_formatter`).
fn copied_parameters(
    parameters: &[item::Parameter],
    receiver: Option<usize>,
    formatter: Option<&ImmutableString>,
) -> Vec<usize> {
    parameters
        .iter()
        .enumerate()
        .filter(|(index, parameter)| {
            Some(*index) != receiver
                && matches!(
                    parameter.ty,
                    type_ref::Type::Reference { mutable: true, .. }
                )
                && !super::direct_mutable_invocation(&parameter.ty)
                && formatter != Some(&parameter.name.text)
        })
        .map(|(index, _)| index)
        .collect()
}

/// `copied_parameters` of a method, a trait's too (every impl of a trait
/// function copies the same parameters, so a dispatcher passes the copies
/// through); not the formatter of a `Debug` or `Display` `fmt`, nor any
/// method's `&mut fmt::Formatter`, whose text the body gives back.
fn method_copied(definition: &method::Definition, _models: &Models) -> Vec<usize> {
    if definition
        .trait_name()
        .is_some_and(|name| matches!(name.as_str(), "Debug" | "Display"))
    {
        return Vec::new();
    }
    copied_parameters(definition.parameters(), None, definition.formatter())
}

fn retain_locations(
    retained: &mut Vec<location::Pending>,
    body_offset: usize,
    callable: Option<&crate::item::Key>,
    source: crate::program::source::Id,
    locations: Vec<location::Output>,
) {
    retained.extend(locations.into_iter().filter_map(|location| {
        let start = body_offset.checked_add(location.generated.start)?;
        let end = body_offset
            .checked_add(location.generated.end)?
            .checked_sub(1)?;
        Some(location::Pending {
            callable: callable.cloned(),
            source,
            generated: start..end.saturating_add(1),
            original: location.original,
            nesting: location.nesting,
        })
    }));
}

fn resolve_locations(generated: &str, locations: Vec<location::Pending>) -> Vec<Location> {
    let mut lines = vec![0];
    lines.extend(
        generated
            .char_indices()
            .filter_map(|(index, character)| (character == '\n').then_some(index + 1)),
    );
    locations
        .into_iter()
        .filter_map(|location| {
            let end = location.generated.end.checked_sub(1)?;
            Some(Location {
                callable: location.callable,
                source: location.source,
                generated: Span::new(
                    position_at(generated, &lines, location.generated.start),
                    position_at(generated, &lines, end),
                ),
                original: location.original,
                nesting: location.nesting,
            })
        })
        .collect()
}

fn position_at(source: &str, lines: &[usize], byte: usize) -> Position {
    let line = lines
        .partition_point(|start| *start <= byte)
        .saturating_sub(1);
    let column = source[lines[line]..byte].chars().count();
    Position::new(
        u16::try_from(line.saturating_add(1)).unwrap_or(u16::MAX),
        u16::try_from(column.saturating_add(1)).unwrap_or(u16::MAX),
    )
}

fn method_name(id: method::Id) -> String {
    format!("{METHOD_SENTINEL}{}", id.index())
}

fn function_name(id: crate::item::Id) -> String {
    format!("{FUNCTION_SENTINEL}{}", id.index())
}

pub(crate) fn private_function_name(key: &crate::item::Key) -> ImmutableString {
    format!("{PRIVATE_FUNCTION}{key}").into()
}

const PRIVATE_FUNCTION: &str = "\0rawr.function.";

/// A function's name as a user reads it: a private function's path in its
/// crate (`main::geometry::area`), and any other name as it is.
pub(crate) fn function_display_name(name: &str) -> &str {
    name.strip_prefix(PRIVATE_FUNCTION).unwrap_or(name)
}

fn private_method_name(definition: &method::Definition) -> String {
    definition.symbol().into()
}

/// A path naming a free value and free functions, as a Rust `fn` item: the
/// value where it is not called, the functions where it is.
/// A facade path may name a macro and a value independently, as in Rust:
/// `column![..]` takes the macro and `column(..)` the function. A path in
/// one namespace keeps its items, so a wrong call syntax still reaches the
/// diagnostic that names the right one.
fn macro_or_value(
    items: Vec<crate::host::item::Definition>,
    bang: bool,
) -> Vec<crate::host::item::Definition> {
    let is_macro = |item: &crate::host::item::Definition| item.kind().is_macro();
    if items.iter().all(is_macro) || !items.iter().any(is_macro) {
        return items;
    }
    items
        .into_iter()
        .filter(|item| is_macro(item) == bang)
        .collect()
}

fn value_or_call(
    items: Vec<crate::host::item::Definition>,
    called: bool,
) -> Vec<crate::host::item::Definition> {
    let value = |item: &crate::host::item::Definition| {
        matches!(item.kind(), crate::host::item::Kind::Value)
    };
    // A tuple variant's constructor, beside the reader a pattern uses.
    let items = if items
        .iter()
        .any(|item| item.kind() == crate::host::item::Kind::AssociatedFunction)
    {
        items
            .into_iter()
            .filter(|item| item.kind() != crate::host::item::Kind::Method)
            .collect()
    } else {
        items
    };
    if items.iter().all(value) || !items.iter().any(value) {
        return items;
    }
    items
        .into_iter()
        .filter(|item| value(item) != called)
        .collect()
}

/// A facade field may share its name with builder methods, as iced's
/// `style.border` and `style.border(..)`: the field where it is read, the
/// builders where it is called.
fn field_or_builder(
    items: Vec<crate::host::item::Definition>,
    field: bool,
) -> Vec<crate::host::item::Definition> {
    let reader = |item: &crate::host::item::Definition| {
        item.receiver() == Some(receiver::Mode::Shared) && item.signature().parameters().is_empty()
    };
    if items.iter().all(reader) || !items.iter().any(reader) {
        return items;
    }
    items
        .into_iter()
        .filter(|item| reader(item) == field)
        .collect()
}

fn pattern_name(index: usize) -> String {
    format!("{NOMINAL_MATCH_SENTINEL}{index}")
}

/// The path of each trait object type the program's sources write, by its
/// segments: `["std", "fmt", "Debug"]` of `dyn std::fmt::Debug`. A source
/// with no `dyn` is never lexed.
fn trait_objects(engine: &Engine, source: &crate::program::Source) -> Vec<Vec<String>> {
    let mut objects = Vec::new();
    for text in source.texts().filter(|text| text.contains("dyn")) {
        let tokens = lex_rust(engine, text);
        let mut rest = tokens.iter().map(|(token, _)| token).peekable();
        while let Some(token) = rest.next() {
            if !matches!(token, Token::Identifier(name) | Token::Reserved(name)
                if name.as_str() == "dyn")
            {
                continue;
            }
            let mut segments = Vec::new();
            while let Some(Token::Identifier(name) | Token::Reserved(name)) = rest.peek() {
                segments.push(name.to_string());
                rest.next();
                if rest
                    .next_if(|token| matches!(token, Token::DoubleColon))
                    .is_none()
                {
                    break;
                }
            }
            if !segments.is_empty() {
                objects.push(segments);
            }
        }
    }
    objects
}

fn source_span(source: &str, selected: Span) -> String {
    let mut output = String::with_capacity(source.len());
    let mut line = 1_u16;
    let mut column = 0_u16;

    for character in source.chars() {
        if character == '\n' {
            output.push(character);
            line = line.saturating_add(1);
            column = 0;
            continue;
        }
        column = column.saturating_add(1);
        let position = Position::new(line, column);
        output.push(
            if position >= selected.start() && position < selected.end() {
                character
            } else {
                ' '
            },
        );
    }

    output
}

/// A function's body as `source_span` selects it, with its elided source
/// blank.
fn body_source(source: &str, body: &item::Body) -> String {
    let selected = source_span(source, body.span);
    let Some(elide) = body.elide else {
        return selected;
    };
    let mut output = String::with_capacity(selected.len());
    let mut line = 1_u16;
    let mut column = 0_u16;
    for character in selected.chars() {
        if character == '\n' {
            output.push(character);
            line = line.saturating_add(1);
            column = 0;
            continue;
        }
        column = column.saturating_add(1);
        let position = Position::new(line, column);
        output.push(if position >= elide.start() && position < elide.end() {
            ' '
        } else {
            character
        });
    }
    output
}

/// Whether a library function's numeric `parameter` takes a `source`
/// number: an integer or a float, as Rust's literal settles.
fn host_number_accepts(parameter: &crate::host::type_ref::Type, source: &Ty) -> bool {
    use crate::host::type_ref::{Builtin, Shape};
    match parameter.shape() {
        Shape::Builtin(Builtin::I64) => matches!(source, Ty::Int(_)),
        Shape::Builtin(Builtin::F64 | Builtin::F32) => matches!(source, Ty::Float(_)),
        _ => false,
    }
}

/// Whether `path` names a runtime's entry macro: a crate's `main`, as
/// `#[tokio::main]` and `#[async_std::main]` are.
pub(super) fn runtime_entry(path: &crate::item::type_ref::Path) -> bool {
    path.segments.len() > 1
        && path
            .segments
            .last()
            .is_some_and(|segment| segment.name.text == "main")
}

/// A `RefCell` of `value` (`resolve::cell_new`).
pub(crate) fn cell_new(value: Dynamic) -> Dynamic {
    cell::new(value)
}

/// Whether the `RefCell` `value` is borrowed (`resolve::cell_held`).
pub(crate) fn cell_held(value: &Dynamic) -> Option<bool> {
    cell::held(value)
}

/// A mutable borrow of the `RefCell` `value` when nothing borrows it
/// (`resolve::cell_lock`).
pub(crate) fn cell_lock(value: &Dynamic) -> Option<Dynamic> {
    cell::try_borrow_mut(value)
}

/// Whether `value` is a `RefCell` at runtime.
pub(crate) fn is_cell(value: &Dynamic) -> bool {
    value.is::<cell::Cell>()
}

fn source_full_span(source: &str) -> Span {
    let mut line = 1_u16;
    let mut column = 0_u16;
    for character in source.chars() {
        if character == '\n' {
            line = line.saturating_add(1);
            column = 0;
        } else {
            column = column.saturating_add(1);
        }
    }
    Span::new(Position::START, Position::new(line, column))
}

fn exact_symbolic_type(ty: &type_ref::Type) -> Option<Ty> {
    match ty {
        type_ref::Type::Unit(_) | type_ref::Type::Path(_) => Some(Ty::from_resolved(ty)),
        type_ref::Type::Tuple { .. } | type_ref::Type::Reference { .. } => None,
    }
}

/// A symbolic callable's output where an imported item takes it: an owned
/// value, or a tuple of owned values, as iced's `new` gives its state and
/// first task, `(Self, Task<Message>)`, which the generated runner carries
/// as the tuple it is.
fn owned_symbolic_type(ty: &type_ref::Type) -> Option<Ty> {
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
fn symbolic_input_type(ty: &type_ref::Type) -> Option<Ty> {
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

/// A `for` loop's iterator when it holds borrows, its element, the
/// positions where the element takes what the iterator holds (pairs of the
/// element's position and the iterator's), and its pattern's bindings. The
/// element and the bindings are given a value each iteration.
type LoopElement = (
    Option<super::borrowck::Local>,
    super::borrowck::Local,
    Vec<(borrowck::Path, borrowck::Path)>,
    Vec<super::borrowck::Local>,
);

struct Lowered {
    source: String,
    locations: Vec<location::Output>,
    ops: Vec<ExecutableOp>,
    patterns: Vec<NominalPattern>,
    closures: Vec<closure::Lifted>,
}

/// Whether `member` of the imported enum `definition` is one of its
/// variants. Any other member is an associated item, as torin's
/// `Size::fill()` is beside its variant `Size::Fill`.
#[cfg(feature = "rust-import")]
fn names_variant(definition: &crate::host::type_def::Definition, member: &str) -> bool {
    matches!(
        definition.shape(),
        crate::host::type_def::Shape::Enum(variants)
            if variants.iter().any(|variant| variant.name() == member)
    )
}

fn invariant(message: &str) -> Box<EvalAltResult> {
    EvalAltResult::ErrorRuntime(message.into(), Position::NONE).into()
}

fn construction_invariant(error: crate::program::construction::Error) -> Box<EvalAltResult> {
    invariant(&error.to_string())
}

fn validate_dynamic(
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

fn matches_runtime_type(value: &Dynamic, ty: &type_ref::Type, models: &Models) -> bool {
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
fn matches_runtime_model(value: &Dynamic, model: &Nominal, models: &Models) -> bool {
    let expected = model.definition();
    let identity = type_def::Identity::Script(expected.key().clone());
    models
        .runtime
        .validate_value(value, &identity, expected.schema())
        .is_ok()
}

#[derive(Clone)]
struct Binding {
    ty: Ty,
    mutable: bool,
    available: bool,
    declaration: Span,
    kind: BindingKind,
}

#[derive(Clone)]
enum BindingKind {
    Local,
    Invocation,
    Receiver { owner: ImmutableString },
}

/// An array literal being parsed, for the frames its elements open: a
/// call that opens one runs ahead of the statement, so each element before
/// its own is bound ahead of that frame, as Rust evaluates elements in
/// order (`[icon().size(12), Slot::from("Verified")]`).
struct FrameArray {
    /// Where the literal starts.
    start: Position,
    /// How many statements' frames were open when it started: a call
    /// opening a frame of the same statement binds its elements.
    depth: usize,
    /// How many calls had been noted when it started (`calls_noted`),
    /// counted rather than placed, as a macro's tokens all sit at its
    /// call.
    noted: usize,
    /// How many calls had been noted when the element being parsed
    /// started, its index, and how many frames its statement had then.
    element: (usize, usize, usize),
    /// Each element whose frame needs the elements before it bound first,
    /// with how many frames its statement had when it started.
    bound: Vec<(usize, usize)>,
}

/// An array literal's elements, its array's repeat count and a `vec!`'s
/// count expression (`[x; 3]`, `vec![x; n]`).
type ArrayElements = (Vec<ParsedExpr>, Option<usize>, Option<ParsedExpr>);

/// A scope frame a statement's call opens (D558 tier 1): `outer` declares
/// what keeps the frame, `inner` binds and opens it ahead of the statement,
/// and `after` stores the copies it gives back once it has ended.
#[derive(Default)]
struct Frame {
    outer: origin::Fragment,
    inner: origin::Fragment,
    after: origin::Fragment,
}

#[derive(Clone)]
struct ParsedExpr {
    prelude: origin::Fragment,
    source: origin::Fragment,
    ty: Ty,
    span: Span,
    lvalue: Option<Lvalue>,
    outcome: crate::host::usage::Outcome,
    evaluation: crate::host::usage::Evaluation,
}

struct Branch {
    value: ParsedExpr,
    syntax: Span,
}

impl ParsedExpr {
    fn rendered_source(&self) -> origin::Fragment {
        if self.prelude.is_empty() {
            self.source.clone()
        } else {
            self.rendered_block()
        }
    }

    fn rendered_block(&self) -> origin::Fragment {
        let mut rendered = origin::Fragment::plain("{\n");
        rendered.push(self.prelude.clone());
        rendered.push(self.source.clone());
        rendered.push_text("\n}");
        rendered
    }

    fn retain_origin(mut self) -> Self {
        self.source.cover(self.span);
        self
    }

    /// Whether this value is a local read by its name, which a call or a
    /// `format!` can borrow where it is instead of a copy (D568).
    fn is_local(&self) -> bool {
        self.prelude.is_empty() && is_valid_identifier(self.source.as_str())
    }
}

/// An iterator chain bound to an immutable local: the local's declaration,
/// the chain, its item type, and the names its setup bound where the local
/// was bound, which a closure using the local captures.
#[derive(Clone)]
struct BoundIterator {
    declaration: Span,
    pipeline: iterator::Pipeline,
    item: Ty,
    names: Vec<String>,
}

/// A `text.into()` whose target the use site gives once inference has it:
/// a `String`, or a std type of std's `From` of text (`Cow<str>`), written
/// over the placeholder before the text's `(text)`.
struct TextInto {
    span: Span,
    source: Ty,
    target: Ty,
    placeholder: String,
}

/// The placeholder of the `text.into()` numbered `index`.
fn text_into_placeholder(index: usize) -> String {
    // One more than any helper name, for the `(` written after it.
    let head = format!("__rawr_nominal_text_into_{index:010}");
    let width = head.len() + 1;
    format!("{head:_<width$}")
}

struct PendingConversion {
    placeholder: String,
    source: Ty,
    target: Ty,
    span: Span,
    /// A facade value's `.into()`: its type's own `into` forms, chosen by
    /// their output, or the target's `from` of it.
    host: Option<HostInto>,
    /// `.try_into()`: the error type of the `Result` it gives, the target's
    /// `TryFrom::Error` (#757).
    try_from: Option<Ty>,
}

/// The `into` forms of a facade value's type, and the value's span.
/// A library type's `from` functions and the dependency that owns them.
type FromItems = (
    Vec<crate::host::item::Definition>,
    Option<crate::host::facade::Dependency>,
);

struct HostInto {
    forms: Vec<crate::host::item::Definition>,
    owner: Option<crate::host::facade::Dependency>,
    receiver: Span,
}

/// The generic parameter `ty` is, or refers to.
/// The generic parameter or opaque associated type `ty` is, or refers to:
/// a value with only its bounds' methods.
fn bounded_value(ty: &Ty) -> Option<Ty> {
    match ty {
        Ty::Param(_) | Ty::Projection(..) => Some(ty.clone()),
        Ty::Reference { target, .. } => bounded_value(target),
        _ => None,
    }
}

/// `ty` with each generic parameter in `substitution` replaced.
fn instantiate(ty: &Ty, substitution: &BTreeMap<ImmutableString, Ty>) -> Ty {
    let each = |types: &[Ty]| -> Box<[Ty]> {
        types
            .iter()
            .map(|ty| instantiate(ty, substitution))
            .collect()
    };
    match ty {
        Ty::Param(name) => substitution
            .get(name)
            .cloned()
            .unwrap_or_else(|| ty.clone()),
        Ty::Option(element) => Ty::Option(Box::new(instantiate(element, substitution))),
        Ty::Boxed(target) => Ty::Boxed(Box::new(instantiate(target, substitution))),
        Ty::Counted(counter, target) => {
            Ty::Counted(*counter, Box::new(instantiate(target, substitution)))
        }
        Ty::Weak(counter, target) => {
            Ty::Weak(*counter, Box::new(instantiate(target, substitution)))
        }
        Ty::Iter(item, std) => Ty::Iter(Box::new(instantiate(item, substitution)), *std),
        Ty::Reverse(inner) => Ty::Reverse(Box::new(instantiate(inner, substitution))),
        Ty::Cow(inner) => Ty::Cow(Box::new(instantiate(inner, substitution))),
        Ty::Range(element) => Ty::Range(Box::new(instantiate(element, substitution))),
        Ty::InclusiveOf(element) => Ty::InclusiveOf(Box::new(instantiate(element, substitution))),
        Ty::Cell(kind, inner) => Ty::Cell(*kind, Box::new(instantiate(inner, substitution))),
        Ty::Thread(kind, inner) => Ty::Thread(*kind, Box::new(instantiate(inner, substitution))),
        Ty::Keyed(kind, arguments) => Ty::Keyed(
            *kind,
            arguments
                .iter()
                .map(|argument| instantiate(argument, substitution))
                .collect(),
        ),
        Ty::Dyn(trait_name, bindings) => Ty::Dyn(
            trait_name.clone(),
            bindings
                .iter()
                .map(|(name, ty)| (name.clone(), instantiate(ty, substitution)))
                .collect(),
        ),
        Ty::Result(ok, err) => Ty::Result(
            Box::new(instantiate(ok, substitution)),
            Box::new(instantiate(err, substitution)),
        ),
        Ty::Array { element, kind } => Ty::Array {
            element: Box::new(instantiate(element, substitution)),
            kind: *kind,
        },
        Ty::Tuple(members) => Ty::Tuple(each(members)),
        Ty::Reference { mutable, target } => Ty::Reference {
            mutable: *mutable,
            target: Box::new(instantiate(target, substitution)),
        },
        Ty::Fn { inputs, output } => Ty::Fn {
            inputs: each(inputs),
            output: Box::new(instantiate(output, substitution)),
        },
        Ty::FnItem {
            key,
            inputs,
            output,
        } => Ty::FnItem {
            key: key.clone(),
            inputs: each(inputs),
            output: Box::new(instantiate(output, substitution)),
        },
        Ty::Host { proof, arguments } => Ty::Host {
            proof: proof.clone(),
            arguments: each(arguments),
        },
        Ty::Script(id, arguments) => Ty::Script(*id, each(arguments)),
        Ty::Projection(base, trait_name, name) => Ty::Projection(
            Box::new(instantiate(base, substitution)),
            trait_name.clone(),
            name.clone(),
        ),
        _ => ty.clone(),
    }
}

/// The generic parameters bounded by `Default` or by a trait with a
/// function without `self`: each takes a hidden type witness, so
/// `T::default()` and `T::new()` reach a type erased at runtime.
fn witness_generics<'a>(
    generics: &'a [item::Generic],
    models: &'a Models,
) -> impl Iterator<Item = &'a item::Generic> {
    generics.iter().filter(|generic| {
        generic.host
            || generic.bounds.contains(capability::Kind::Default)
            || generic
                .traits
                .iter()
                .any(|name| static_trait(models, &name.text))
    })
}

/// Whether a resolved type names a generic parameter.
fn mentions_parameter(ty: &type_ref::Type) -> bool {
    match ty {
        type_ref::Type::Unit(_) => false,
        type_ref::Type::Tuple { members, .. } => members.iter().any(mentions_parameter),
        type_ref::Type::Reference { target, .. } => mentions_parameter(target),
        type_ref::Type::Path(path) => {
            matches!(
                path.target,
                type_ref::Target::Parameter | type_ref::Target::Projection(..)
            ) || path.arguments.iter().any(|argument| match argument {
                type_ref::Argument::Type(ty) => mentions_parameter(ty),
                type_ref::Argument::Lifetime(_) => false,
            })
        }
    }
}

/// Tokens, each with its position.
type Tokens = Vec<(Token, Position)>;

/// A call's argument tokens split after the first argument: the first,
/// and the rest after its comma. Commas inside brackets, braces,
/// parentheses, a turbofish or a closure's parameters do not split.
/// How many arguments `tokens` gives, a trailing comma aside.
fn argument_count(tokens: &Tokens) -> usize {
    let mut rest = tokens.clone();
    let mut count = 0;
    while !rest.is_empty() {
        rest = split_first_argument(rest).1;
        count += 1;
    }
    count
}

fn split_first_argument(tokens: Tokens) -> (Tokens, Tokens) {
    let mut depth = 0usize;
    let mut angle = 0usize;
    let mut closure = false;
    let mut previous_colons = false;
    for (index, (token, _)) in tokens.iter().enumerate() {
        match token {
            Token::LeftParen | Token::LeftBracket | Token::LeftBrace | Token::MapStart => {
                depth += 1;
            }
            Token::RightParen | Token::RightBracket | Token::RightBrace => {
                depth = depth.saturating_sub(1);
            }
            Token::LessThan if previous_colons || angle > 0 => angle += 1,
            Token::GreaterThan if angle > 0 => angle -= 1,
            Token::RightShift if angle > 0 => angle = angle.saturating_sub(2),
            Token::Pipe if depth == 0 && angle == 0 => closure = !closure,
            Token::Comma if depth == 0 && angle == 0 && !closure => {
                let mut first = tokens;
                let rest = first.split_off(index + 1);
                first.pop();
                return (first, rest);
            }
            _ => {}
        }
        previous_colons = matches!(token, Token::DoubleColon);
    }
    (tokens, Vec::new())
}

/// An argument's tokens without a leading `&`: `Clone::clone(&x)` is
/// `x.clone()`, as Rust's auto-reference makes it.
/// The tokens of the Rust `text`, each at `at`, with each named hole the
/// parenthesised tokens it stands for: a call rewritten as source.
fn template_tokens(
    text: &str,
    holes: &[(&str, Vec<(Token, Position)>)],
    at: Position,
) -> Vec<(Token, Position)> {
    let engine = Engine::new_raw();
    let mut tokens = Vec::new();
    for (token, _) in lex_rust(&engine, text) {
        if token == Token::EOF {
            break;
        }
        if let Token::Identifier(name) = &token
            && let Some((_, hole)) = holes.iter().find(|(hole, _)| name.as_str() == *hole)
        {
            tokens.push((Token::LeftParen, at));
            tokens.extend(hole.iter().cloned());
            tokens.push((Token::RightParen, at));
            continue;
        }
        tokens.push((token, at));
    }
    tokens
}

/// The tokens of a receiver taken by `&self` or `&mut self` as a method
/// call's receiver: its `&` or `&mut` dropped (`Iterator::next(&mut it)`
/// is `it.next()`).
fn without_borrow(mut tokens: Vec<(Token, Position)>) -> Vec<(Token, Position)> {
    if matches!(tokens.first(), Some((Token::Ampersand, _))) && tokens.len() > 1 {
        tokens.remove(0);
        if matches!(tokens.first(), Some((Token::Mut, _))) && tokens.len() > 1 {
            tokens.remove(0);
        }
    }
    tokens
}

fn without_reference(mut tokens: Vec<(Token, Position)>) -> Vec<(Token, Position)> {
    if matches!(tokens.first(), Some((Token::Ampersand, _))) && tokens.len() > 1 {
        tokens.remove(0);
    }
    tokens
}

/// The text of a generic trait's argument list: `i64, String`.
fn instance_text(arguments: &[type_ref::Type]) -> String {
    arguments
        .iter()
        .map(crate::resolve::full_type_name)
        .collect::<Vec<_>>()
        .join(", ")
}

/// The dispatcher of `trait_name::method` for the trait's instance
/// `index` of a generic trait; `FALLBACK_INSTANCE` dispatches by the
/// receiver's type alone. Same length as `instance_placeholder`.
fn instance_dispatcher_name(trait_name: &str, method: &str, index: usize) -> String {
    format!("{DISPATCH_SENTINEL}{trait_name}__{method}__{index:08}")
}

/// A call's dispatcher before inference fixes its trait arguments.
fn instance_placeholder(trait_name: &str, method: &str, pending: usize) -> String {
    format!("{DISPATCH_SENTINEL}{trait_name}__{method}__p{pending:07}")
}

const FALLBACK_INSTANCE: usize = 99_999_999;

/// Each generic parameter's generic trait bounds' arguments:
/// `(U, Convert)` to `[[i64]]` for `U: Convert<i64>`, and to
/// `[[i64], [String]]` for `U: Convert<i64> + Convert<String>`.
fn generic_trait_arguments(
    generics: &[item::Generic],
) -> BTreeMap<(ImmutableString, ImmutableString), Vec<Box<[Ty]>>> {
    let mut all = BTreeMap::<_, Vec<Box<[Ty]>>>::new();
    for generic in generics {
        for (trait_name, arguments) in &generic.trait_arguments {
            all.entry((generic.name.text.clone(), trait_name.clone()))
                .or_default()
                .push(arguments.iter().map(Ty::from_resolved).collect());
        }
    }
    all
}

/// The positional trait arguments of a trait object, kept among its
/// bindings under the names `0`, `1`, ..: `i64` of `dyn Convert<i64>`.
fn dyn_arguments(bindings: &[(ImmutableString, Ty)]) -> Vec<Ty> {
    let mut positional = bindings
        .iter()
        .filter_map(|(name, ty)| Some((name.parse::<usize>().ok()?, ty.clone())))
        .collect::<Vec<_>>();
    positional.sort_by_key(|(index, _)| *index);
    positional.into_iter().map(|(_, ty)| ty).collect()
}

/// The supertraits of the trait `name`, transitively.
fn supertraits(models: &Models, name: &str) -> Vec<ImmutableString> {
    models
        .traits
        .get(name)
        .map(|(_, declared)| {
            declared
                .supertraits
                .1
                .iter()
                .map(|name| name.text.clone())
                .collect()
        })
        .unwrap_or_default()
}

/// Whether the trait `name` has functions without `self`, which a generic
/// parameter bounded by it calls through its type witness.
fn static_trait(models: &Models, name: &str) -> bool {
    models.traits.get(name).is_some_and(|(_, declared)| {
        declared
            .functions
            .iter()
            .any(|function| function.receiver.is_none())
    })
}

/// The generated function that runs `trait_name::function`, a function
/// without `self`, for the type a witness names.
fn static_dispatcher_name(trait_name: &str, function: &str) -> String {
    format!("{DISPATCH_SENTINEL}static_{trait_name}__{function}")
}

/// The static dispatcher of a generic trait's function without `self` for
/// the trait's instance `index`, as `instance_dispatcher_name`.
fn static_instance_dispatcher_name(trait_name: &str, function: &str, index: usize) -> String {
    format!("{DISPATCH_SENTINEL}static_{trait_name}__{function}__{index:08}")
}

/// A static call's dispatcher before inference fixes its trait arguments.
fn static_instance_placeholder(trait_name: &str, function: &str, pending: usize) -> String {
    format!("{DISPATCH_SENTINEL}static_{trait_name}__{function}__p{pending:07}")
}

/// The hidden parameter carrying the witness of the generic `name`.
fn witness_variable(name: &str) -> String {
    format!("__rawr_witness_{name}")
}

/// A placeholder long enough for any witness it becomes.
fn witness_placeholder(index: usize) -> String {
    format!("__rawr_nominal_witness_{index:024}")
}

/// Every generic parameter list an item declares.
fn item_generics(kind: &item::Kind) -> Vec<&item::Generic> {
    match kind {
        item::Kind::Struct(item) => item.generics.iter().collect(),
        item::Kind::Enum(item) => item.generics.iter().collect(),
        item::Kind::Function(item) => item.generics.iter().collect(),
        item::Kind::Impl(item) => item
            .generics
            .iter()
            .chain(
                item.functions
                    .iter()
                    .flat_map(|function| function.generics.iter()),
            )
            .collect(),
        item::Kind::Trait(item) => item
            .functions
            .iter()
            .flat_map(|function| function.generics.iter())
            .collect(),
        item::Kind::Use(_) => Vec::new(),
    }
}

/// `Iterator` and the traits whose `Item` is its own, which Rawr's iterator
/// type stands for.
fn is_iterator_trait(name: &str) -> bool {
    matches!(
        name,
        "Iterator" | "DoubleEndedIterator" | "ExactSizeIterator"
    )
}

/// `T::Output = U` of each `T: Trait<Output = U>` in `generics`.
fn generic_equalities(
    generics: &[item::Generic],
) -> BTreeMap<(ImmutableString, ImmutableString, ImmutableString), Ty> {
    generics
        .iter()
        .flat_map(|generic| {
            generic.projections.iter().map(|(trait_name, name, ty)| {
                (
                    (
                        generic.name.text.clone(),
                        trait_name.text.clone(),
                        name.text.clone(),
                    ),
                    Ty::from_resolved(ty),
                )
            })
        })
        .collect()
}

/// The user trait bounds of each generic parameter in `generics`.
/// The generic parameters a signature names inside a host type.
fn host_witnesses(generics: &[item::Generic]) -> Vec<ImmutableString> {
    generics
        .iter()
        .filter(|generic| generic.host)
        .map(|generic| generic.name.text.clone())
        .collect()
}

fn generic_traits(generics: &[item::Generic]) -> BTreeMap<ImmutableString, Box<[ImmutableString]>> {
    generics
        .iter()
        .filter(|generic| !generic.traits.is_empty())
        .map(|generic| {
            (
                generic.name.text.clone(),
                generic
                    .traits
                    .iter()
                    .map(|name| name.text.clone())
                    .collect(),
            )
        })
        .collect()
}

/// The generated function that runs `trait_name::method` for a value by its
/// runtime type.
fn trait_dispatcher_name(trait_name: &str, method: &str) -> String {
    format!("{DISPATCH_SENTINEL}{trait_name}__{method}")
}

/// The capabilities a bound set names.
fn bound_kinds(bounds: capability::Set) -> impl Iterator<Item = capability::Kind> {
    [
        capability::Kind::Default,
        capability::Kind::Debug,
        capability::Kind::Clone,
        capability::Kind::Copy,
        capability::Kind::PartialEq,
        capability::Kind::Eq,
        // An `Rc` is not `Send` (rustc's E0277).
        capability::Kind::Send,
    ]
    .into_iter()
    .filter(move |kind| bounds.contains(*kind))
}

/// A placeholder long enough for any method helper name it becomes.
fn conversion_placeholder(index: usize) -> String {
    format!("__rawr_nominal_conversion_{index:010}")
}

/// The head `dispatcher(witness, ` of `T::from(x)` for a generic target
/// `T` and its `From` instance `index`, written over a conversion
/// placeholder before the converted value's `(x))`.
fn generic_conversion_call(param: &str, index: usize) -> String {
    format!(
        "{}({}, ",
        static_instance_dispatcher_name("From", "from", index),
        witness_variable(param)
    )
}

#[cfg(feature = "rust-import")]
struct ImportedContext<'a> {
    native: Option<&'a super::native::Capture>,
    allocation: &'a mut dyn crate::host::ImportAllocation,
    failure: &'a mut Option<rawr_import::surface::Error>,
    source: crate::program::source::Id,
    uses: &'a mut Vec<ImportedUse>,
    dependencies: &'a mut BTreeSet<crate::host::facade::Dependency>,
}

#[cfg(feature = "rust-import")]
impl ImportedContext<'_> {
    fn allocation_failure(&mut self, error: rawr_import::surface::Error) -> Error {
        self.failure.replace(error);
        Error::LoweringUnavailable {
            item: "<imported allocation>".into(),
            body: Span::NONE,
        }
    }

    fn retain_checked(&mut self, items: Option<usize>, bytes: Option<usize>) -> Result<(), Error> {
        self.allocation
            .retain_checked(items, bytes)
            .map_err(|error| self.allocation_failure(error))
    }

    fn entries<T>(&mut self, count: usize) -> Result<Vec<T>, Error> {
        self.retain_checked(Some(count), count.checked_mul(std::mem::size_of::<T>()))?;
        let mut entries = Vec::new();
        entries
            .try_reserve_exact(count)
            .map_err(|_| self.allocation_failure(rawr_import::surface::Error::Allocation))?;
        Ok(entries)
    }

    fn for_source(&mut self, source: crate::program::source::Id) -> ImportedContext<'_> {
        ImportedContext {
            native: self.native,
            allocation: self.allocation,
            failure: self.failure,
            source,
            uses: self.uses,
            dependencies: self.dependencies,
        }
    }
}

/// A trait call whose generic trait's dispatcher waits for inference:
/// `(placeholder, trait, method, arguments, span, static, Self)`.
type PendingInstance = (
    String,
    ImmutableString,
    ImmutableString,
    Box<[Ty]>,
    Span,
    bool,
    Ty,
);

/// A facade pattern met below a script pattern: the hidden name its value
/// is bound to, the pattern, the value's type and the binding mode.
type HostCheck = (ImmutableString, pattern::Syntax, Ty, Option<bool>);

struct Parser<'a> {
    marker: std::marker::PhantomData<&'a ()>,
    /// The source the tokens were read from, where a diagnostic finds
    /// what a literal's value doesn't show.
    text: &'a str,
    input: Peekable<IntoIter<(Token, Position)>>,
    models: Models,
    bindings: BTreeMap<ImmutableString, Binding>,
    output: origin::Fragment,
    ops: Vec<Op>,
    patterns: Vec<NominalPattern>,
    op_offset: usize,
    pattern_offset: usize,
    last_type: Ty,
    last_expression: Option<(usize, Span)>,
    inference: Inference,
    callable: Option<crate::item::Key>,
    /// In a trait's default body copied into an impl: the owner and the
    /// trait, whose own functions `self.f()` names, as with a generic `Self`.
    default_trait: Option<(type_def::Id, ImmutableString)>,
    /// In a hand-written `Debug` or `Display` `fmt`: its formatter, a
    /// `String` the body writes into with `write!` and the call returns.
    formatter: Option<ImmutableString>,
    /// The `&mut` parameters the function copies back out to its caller
    /// (`copied_parameters`): each return gives `[value, ..parameters]`.
    copy_out: Vec<ImmutableString>,
    /// The closure depth (`closures.len()`) whose returns `copy_out` wraps.
    copy_out_depth: usize,
    /// In a method: its owner, which `Self` names in `Self::Output`.
    self_ty: Option<Ty>,
    /// In a trait impl's method: the trait, whose `Self::Output` it names.
    impl_trait: Option<ImmutableString>,
    root_expression_index: u32,
    expression_stack: Vec<Box<[u32]>>,
    expression_child_indexes: Vec<u32>,
    expression_paths: Vec<(Span, Box<[u32]>)>,
    normalize_private_calls: bool,
    symbolic_only: bool,
    /// In a pass that asks the crates for every miss at once (#662): a
    /// method an imported type lacks is kept in `recovered`, and the body
    /// goes on to its next miss (`recover_miss`).
    #[cfg(feature = "rust-import")]
    recover_misses: bool,
    #[cfg(feature = "rust-import")]
    recovered: Vec<Error>,
    /// Where a recovered call of a type Rawr doesn't know starts, with the
    /// type: the members the body calls on it are that type's misses too.
    #[cfg(feature = "rust-import")]
    recovered_types: Vec<(Position, ImmutableString)>,
    /// While revealing an opaque return type (`reveal`): the declared
    /// output with an inference variable for each `impl Trait`, which the
    /// body's tail and `return`s are checked against in its place.
    #[cfg(feature = "rust-import")]
    revealing: Option<Ty>,
    loops: Vec<LoopFrame>,
    closures: Vec<closure::Scope>,
    /// Where each moved local (by its declaration) was moved, for E0382.
    moved_at: BTreeMap<Span, Span>,
    /// The type of a capture rawr knows isn't `Send`, of each closure
    /// that has one, by the closure's span: what `std::thread::spawn`
    /// refuses (`thread.rs`).
    unsent_captures: BTreeMap<Span, Ty>,
    /// A local a closure that isn't `move` borrows, of each closure that
    /// borrows one, by the closure's span: what a `'static` bound refuses
    /// (rustc's E0373), as `std::thread::spawn`'s does.
    borrowed_captures: BTreeMap<Span, ImmutableString>,
    /// The locals `let x;` declares without a value, by their declaration,
    /// with their first assignment once one is parsed: a use before an
    /// assignment gives one a value is rustc's E0381, and one not declared
    /// `mut` takes its first assignment.
    deferred: BTreeMap<Span, Option<Span>>,
    /// Each use of a `deferred` local before it has a value, reported once
    /// the body's types are settled (`finish_uninitialized`).
    uninitialized: Vec<UninitializedUse>,
    /// Each `let`'s local, in the order the body declares them: where an
    /// inference variable nothing settles is reported (`unsettled`).
    local_types: Vec<LocalType>,
    /// The type of each local whose value never comes (`let v = loop {}`):
    /// one its uses leave open is `!` once the body is read, as rustc's
    /// fallback of a diverging type (`fall_back_diverging`).
    diverging: Vec<Ty>,
    /// Reads of a crate struct's field that leave the struct whole, by
    /// their span: a by-value use moves that field out, as Rust's partial
    /// move (`move_local`).
    shared_field_reads: BTreeSet<Span>,
    /// Places Rust can't move out of that the lowering gives no place of
    /// their own (`*r`, `*rc`, `[s][0]`), by their span: what rustc
    /// refuses of each.
    unmovable: BTreeMap<Span, Unplaced>,
    /// The closures that own a capture which is not `Copy`, so they move
    /// as values that are not `Copy` do.
    owning_closures: BTreeSet<crate::item::Key>,
    /// The closures that move out a capture: `FnOnce`, so a call moves
    /// the closure.
    once_closures: BTreeSet<crate::item::Key>,
    /// The `Copy` closures that change what they captured by value, with
    /// those captures: a copy of one has its own (`KEYWORD_COPY_CLOSURE`).
    copied_state: BTreeMap<crate::item::Key, Vec<ImmutableString>>,
    /// Calls whose value is a function a call borrows shared (`impl Fn`,
    /// `fn`), by their span, and the locals declared with one, by their
    /// declaration (T179).
    shared_call_fns: BTreeSet<Span>,
    closure_count: usize,
    lifted: Vec<closure::Lifted>,
    /// Parameter types of the host callable each call argument fills.
    closure_hints: Vec<Option<Box<[Ty]>>>,
    /// The local holding a `{:?}` spec (`DebugSpec`) that `debug_source`
    /// passes down to each leaf, as Rust's `Formatter` does.
    debug_spec: Option<String>,
    closure_hint: Option<Box<[Ty]>>,
    /// The lifetimes the `for<'a>` binders around the body type being
    /// read bind, which are elided ones (`for<'a> fn(&'a T)`).
    bound_lifetimes: Vec<ImmutableString>,
    /// The `macro_rules!` expansions made so far (`expand`).
    expansions: usize,
    /// For each expansion being parsed, how many tokens follow it: those
    /// whose tokens are not all read yet nest the next call.
    expansion_ends: Vec<usize>,
    /// The `macro_rules!` macros the blocks being parsed define so far,
    /// the innermost last.
    local_macros: Vec<(ImmutableString, Shared<crate::resolve::macro_rules::Rules>)>,
    /// The host type a host callable returns, as iced's `Element`, at each
    /// argument: a closure's body is converted into it, as Rust's `Into`.
    closure_outputs: Vec<Option<closure::Output>>,
    closure_output: Option<closure::Output>,
    /// The closure about to be parsed is an async body's step function
    /// (`parse_async_block`), with its output type when it is known.
    async_step: Option<Option<Ty>>,
    /// The closure about to be parsed is `async |..| body`: its body is an
    /// `async move` block.
    async_closure: bool,
    /// The element type an array literal about to be parsed is expected to
    /// have, so each element coerces to it (`vec![Box::new(a), Box::new(b)]`
    /// as `Vec<Box<dyn Shape>>`), as Rust's expectation does.
    array_hint: Option<Ty>,
    /// The type the expression being parsed is expected to have, with the
    /// expression depth it holds at: what `collect()` builds.
    collect_hint: Option<(Ty, usize)>,
    /// Iterator chains by the source of their iterator expression, fused by
    /// the consumer that runs them.
    pipelines: BTreeMap<String, iterator::Pipeline>,
    /// The step function a program's iterator crosses into a crate as, by
    /// the source of the iterator it stands for there (`step_iterator`):
    /// the callable the runtime binding takes.
    #[cfg(feature = "rust-import")]
    steps: BTreeMap<String, Ty>,
    /// `a..b` of integers or chars by the source of its iterator, with the
    /// value it is as Rust's `Range<T>`: what an imported generic item such
    /// as rand's `random_range` takes in its place.
    range_values: BTreeMap<String, (origin::Fragment, Ty)>,
    /// The bounds of the range parsed last, with its span: what `v[a..b]`
    /// slices by.
    last_range: Option<(ParsedExpr, Option<ParsedExpr>, bool, Span)>,
    /// A field's map an `entry` in this statement changes, as the code
    /// that copies it into a local before the statement and the code that
    /// stores it back after (`self.counts.entry(k)`).
    field_entries: Vec<(origin::Fragment, origin::Fragment)>,
    /// The engine's global modules: the native functions a call may name.
    native_modules: Vec<crate::SharedModule>,
    /// How a `Some(&mut place)` value finds its place, by its span: the
    /// statements that find it, and the test that it is `None`
    /// (`optional_place`, `borrowed_call`), for a pattern matching it
    /// (`pattern_place`).
    optional_subjects: BTreeMap<Span, (origin::Fragment, origin::Fragment)>,
    /// The place a value lends `&mut` to the parts of, by the value's
    /// span: `o.as_mut()` of an `Option` or `Result` place, whose pattern's
    /// `&mut` bindings reach the place's parts (`pattern_place`).
    lent_places: BTreeMap<Span, Lvalue>,
    /// A tuple expression's field spans, by its span: where it has an
    /// expected type, each field is coerced to its own (`coerce_expr`).
    tuple_fields: BTreeMap<Span, Vec<Span>>,
    /// The expression depth of a let chain's link being read: `&&` there
    /// ends the link (`parse_chain`).
    chain_link: Option<usize>,
    /// A closure being read changes `self`.
    self_changed: std::cell::Cell<bool>,
    /// The statement being read makes a closure that changes `self`: it
    /// binds the closure's `self` before it and writes it back after it.
    lend_self: bool,
    /// The expression depth at which `&mut place` is read: a direct
    /// argument for a function's `&mut` parameter (`this_parameter`).
    mutable_borrow: Option<usize>,
    /// Where the operand of the `&` being read starts: a crate struct's
    /// field read there, as all of the operand, is lent, not taken
    /// (`read_imported_field`).
    lent_field: Option<Position>,
    /// Where the operand of the `&mut` being read starts: a crate struct's
    /// field read there, as `&mut pixel.0`, is a place the call writes
    /// through (`read_imported_field`).
    lent_mut_field: Option<Position>,
    /// The chains bound to immutable locals, by name: each use of the
    /// local runs its chain.
    bound_iterators: BTreeMap<ImmutableString, BoundIterator>,
    /// The locals holding a `v.iter_mut()`, by name, each stepped by
    /// `next()` (`places::HeldIterMut`).
    held_iter_mut: BTreeMap<ImmutableString, places::HeldIterMut>,
    /// The `v.iter_mut()` just read as a whole `let`'s value, which the
    /// `let` keeps in `held_iter_mut`.
    pending_iter_mut: Option<places::HeldIterMut>,
    /// The locals holding a map entry's `&mut`, as `let grains =
    /// map.entry(k).or_default();`, by name: their declaration and the
    /// tokens of the entry, which each use reads again.
    entry_aliases: BTreeMap<ImmutableString, (Span, Vec<(Token, Position)>)>,
    /// The borrow of a `RefCell` the expression just read made, which a
    /// `let` keeps (`cell.rs`).
    last_cell_borrow: Option<cell::CellBorrow>,
    /// The locals bound to a `RefCell` borrow, by name: their declaration,
    /// the hidden local holding the borrow, whether it is mutable, the
    /// value's type, and whether it is an imported method's borrow guard.
    cell_guards: BTreeMap<ImmutableString, (Span, String, bool, Ty, bool)>,
    /// The second field of `t.0.1`, read with the first.
    pending_member: Option<(ImmutableString, Span)>,
    /// The elements of mutable loops by name: their declaration, their
    /// place (`v[i]`) and the local holding it (`places.rs`).
    aliases: BTreeMap<ImmutableString, (Span, Lvalue)>,
    /// The declaration of the local each of `aliases` is a place in, by
    /// the alias's declaration: a later local of its name shadows it.
    alias_roots: BTreeMap<Span, Span>,
    /// The declarations of `&mut` borrows the lowering marks mutable so a
    /// write through them is allowed, though they were not declared `mut`
    /// (`places::alias_by_itself`): a closure captures what they refer to
    /// by a unique borrow, as rustc's capture analysis has it.
    undeclared_mut: BTreeSet<Span>,
    /// The `&mut` locals given new borrows, by name: their declaration and
    /// whether they were declared `let mut` (`reborrow.rs`).
    reborrow_selectors: BTreeMap<ImmutableString, (Span, bool)>,
    /// The body's borrow facts, solved once it is read (D572, `borrows.rs`).
    flow: std::cell::RefCell<super::borrowck::record::Recorder>,
    /// Where the function's parameters are declared: a `&mut` one can be
    /// changed through, though not declared `mut`.
    parameter_declarations: BTreeSet<Span>,
    /// The loop about to be read already entered its head and pushed its
    /// frame (`while`'s condition is in it, and may jump to it).
    loop_entered: bool,
    /// The hidden local holding the borrows of the `for` loop about to be
    /// read's iterator (`for x in &v`), used at its head each iteration.
    loop_iterator: Option<super::borrowck::Local>,
    /// That iterator and the loop's element, which takes what it holds at
    /// the positions `borrows::element_positions` gives, and is given a
    /// value each iteration.
    loop_element: Option<LoopElement>,
    /// What each call keeps of its inputs, by where its `)` is, for its
    /// receiver's method call to read once its value is made.
    call_summaries: BTreeMap<Position, super::borrowck::signature::Summary>,
    /// The local whose place a `let` pattern with `ref mut` destructures.
    place_root: Option<ImmutableString>,
    /// What the pattern being resolved does to its subject's parts, for
    /// the borrow facts (`borrows.rs`).
    pattern_trace: std::cell::RefCell<Option<borrows::PatternTrace>>,
    /// The expected type of each argument of the call being parsed.
    argument_hints: Vec<Option<Ty>>,
    /// A generic function's parameter types for the call being parsed,
    /// with its generic parameters fresh: each argument read fixes what it
    /// can of them, so a closure after it has the parameter types its `Fn`
    /// bound gives it, as rustc checks the arguments in order (T377).
    generic_argument_hints: Vec<Ty>,
    /// The next call's arguments go to a std method Rawr runs, as
    /// `Option::map`, where a crate function named as a value is the
    /// closure that calls it (T327).
    script_callables: bool,
    /// The argument being parsed is one of those.
    script_callable: bool,
    /// Arguments already parsed for the next call, as for a builder chain.
    pending_arguments: Option<(Vec<ParsedExpr>, Position)>,
    /// The scope frames each statement being parsed opens (D558 tier 1):
    /// `let made = call(..);` for each call in it whose value borrows what
    /// it takes, which the statement then runs inside.
    frames: Vec<Vec<Frame>>,
    /// For each of `frames`, where its statement starts and where each
    /// call or assignment parsed in it so far starts: a frame is opened
    /// ahead of its statement, so one written before the call that opens
    /// it would run after it. Each call is kept with its count among the
    /// calls noted (`calls_noted`).
    frame_effects: Vec<(Position, Vec<(Position, usize)>)>,
    /// How many calls and assignments have been noted for
    /// `frame_effects`, which orders them where their places can't, as
    /// in a macro's expansion.
    calls_noted: usize,
    /// The array literals being parsed, innermost last: a call that opens
    /// a frame in one element is preceded by the calls of the elements
    /// before it, which are bound ahead of the frame in their order.
    frame_arrays: Vec<FrameArray>,
    /// For each `(` open, where the call it starts begins, if it is one.
    call_parens: Vec<Option<Position>>,
    /// Where the last token was, if a `(` after it calls something, and
    /// whether it is a turbofish's `>`, which only `()` after it calls.
    callee: Option<(Position, bool)>,
    temporaries: usize,
    /// `.into()` conversions, chosen once their target types are known.
    pending_conversions: Vec<PendingConversion>,
    pending_formats: Vec<format::Pending>,
    /// Each `s.parse()`'s target and error type, the error settled by the
    /// target (`settle_parse`).
    pending_parses: Vec<(Ty, Ty, Span, String)>,
    /// Each settled `parse`'s placeholder and the text written over it
    /// once its call is in the body.
    settled_parses: Vec<(String, String)>,
    /// The `parse` calls read so far, which number their placeholders.
    parse_count: usize,
    /// Each `collect()` whose target inference gives later: the target,
    /// the items' type and the call, checked once the body is
    /// (`collected_into`).
    pending_collects: Vec<(Ty, Ty, Span)>,
    /// The `text.into()` calls, whose target `Err(..)` may box as `dyn
    /// Error` (`error_object.rs`) and the use site gives.
    text_into: Vec<TextInto>,
    /// Where each `Err(error)` starts, with its error's span: a `Result<_,
    /// Box<dyn Error>>` expected of it is a `Box<dyn Error>` expected of
    /// the error.
    err_payloads: BTreeMap<Position, Span>,
    /// The generic parameters in scope and their bounds.
    generics: BTreeMap<ImmutableString, capability::Set>,
    /// Each type parameter of a dependency's trait method that native code
    /// fills, with the host path of its opaque type (#593).
    opaque: BTreeMap<ImmutableString, ImmutableString>,
    /// `::<A, B>` read after a generic function's name, for its call.
    pending_turbofish: Option<Vec<Ty>>,
    /// `::<T>` read after a trait function's name in `Trait::f::<T>(..)`,
    /// for the call of that function once its arguments are read.
    trait_turbofish: Option<Vec<Ty>>,
    /// The closures that stand for a path to a function (`Vec::len`,
    /// `Look::name`), which rustc's notes call functions.
    path_values: HashSet<crate::item::Key>,
    /// The output each closure that stands for a path to a method of the
    /// program's is written with, by the closure's key: an `impl Trait`
    /// there is what rustc knows the closure's value by
    /// (`take_opaque_arguments`).
    #[cfg(feature = "rust-import")]
    path_outputs: BTreeMap<crate::item::Key, type_ref::Type>,
    /// The crate trait each `Trait::method(receiver, ..)` call names, by
    /// the position of the method it was rewritten to (`path_method_call`):
    /// a text receiver takes the method the trait's impl for `&str` gives.
    spliced_traits: BTreeMap<Position, ImmutableString>,
    /// The turbofish of each `Trait::method::<T>(receiver, ..)` call, by
    /// the position of the method it was rewritten to (`path_method_call`),
    /// which the method's own parameters take.
    spliced_turbofish: BTreeMap<Position, Vec<Ty>>,
    /// Each method call's native receiver type, by its member's position:
    /// what a method std declares and rawr doesn't run yet is looked up
    /// on (`inherent_not_yet`).
    std_probed: BTreeMap<Position, Ty>,
    /// The turbofish of the host function call whose signature is being
    /// instantiated (`rand::random::<f64>()`).
    host_turbofish: Option<Vec<Ty>>,
    /// The trait arguments a qualified call wrote (`<C as Convert<i64>>::f`,
    /// `Convert::<i64>::f`), for the trait call it makes.
    explicit_trait_arguments: Option<Vec<Ty>>,
    /// While resolving a pattern below a `&T` it matched through: bindings
    /// are references (`Some(mutable)`), Rust's default binding mode.
    binding_mode: std::cell::Cell<Option<bool>>,
    /// A name an or-pattern binds, with its type in the first alternative
    /// and in a later one, the subject's name and the later alternative's
    /// span. The root pattern makes the two one type, as rustc does.
    or_binding_types: std::cell::RefCell<Vec<(Ty, Ty, String, Span)>>,
    /// The most elements any slice pattern names: a slice's lengths up to
    /// it are told apart for coverage (`coverage_constructors`).
    slice_arity: std::cell::Cell<usize>,
    /// Patterns on facade values met below a script pattern: the hidden
    /// name the value is bound to, the pattern and the value's type. The
    /// pattern's site lowers them to tests and bindings. Each keeps the
    /// binding mode it was met in: `Some(mutable)` below a reference.
    host_checks: std::cell::RefCell<Vec<HostCheck>>,
    /// The binding mode of the facade pattern being lowered: its bindings
    /// are references (`Some(mutable)`) when it matches through one, as
    /// rustc's default binding mode makes them.
    host_binding_mode: std::cell::Cell<Option<bool>>,
    /// A host call's one signature, whose closure arguments take their
    /// parameter types from the arguments before them, as Rust infers
    /// `pick_list(selected, options, |x| x.to_string())`'s `x`.
    deferred_hints: Option<(Box<[crate::host::item::Definition]>, Option<Ty>)>,
    /// Bounds a generic call requires of its inferred type arguments,
    /// checked once they are known.
    pending_bounds: Vec<(Ty, capability::Kind, Span)>,
    /// The user trait bounds of each generic parameter in scope.
    generic_traits: BTreeMap<ImmutableString, Box<[ImmutableString]>>,
    /// `(U, Convert)` to `[i64]` for this body's `U: Convert<i64>`.
    generic_trait_arguments: BTreeMap<(ImmutableString, ImmutableString), Vec<Box<[Ty]>>>,
    /// Trait calls whose generic trait's dispatcher waits for inference:
    /// `(placeholder, trait, method, arguments, span)`.
    /// Each also says whether its dispatcher is a static one (a function
    /// without `self`, by witness).
    pending_instances: Vec<PendingInstance>,
    /// The `{integer}` (`true`) and `{float}` literal kinds of this body,
    /// which become `i64` and `f64` where nothing else binds them.
    literal_kinds: Vec<(usize, bool)>,
    /// Each unsuffixed integer literal's kind, number and place, checked
    /// against the kind inference settles on (`overflow`).
    literal_values: Vec<(usize, INT, Span)>,
    /// In an impl of a generic trait, its parameters' arguments: `T` is
    /// `i64` in `impl Convert<i64> for C`'s methods and default bodies.
    trait_parameters: BTreeMap<ImmutableString, Ty>,
    /// Each call of a generic function compiled per instantiation (D629):
    /// its key and the type each generic parameter takes.
    #[cfg_attr(not(feature = "rust-import"), allow(dead_code))]
    instance_calls: Vec<(crate::item::Key, Vec<Ty>, Span)>,
    /// Whether this body is read only for those calls (`monomorphize`).
    #[cfg_attr(not(feature = "rust-import"), allow(dead_code))]
    collecting_instances: bool,
    /// Whether a generic parameter of this body reached a crate's item as a
    /// type of its own, or a generic compiled per instantiation: the body
    /// touches a crate's types only there, and is compiled per
    /// instantiation too (D629).
    #[cfg_attr(not(feature = "rust-import"), allow(dead_code))]
    reaches_host: bool,
    /// Each use of a script impl of a crate's trait that a crate's bound
    /// selects (D629): the impl item's index, its parameters' types, and
    /// the trait's arguments with its defaults filled in.
    #[cfg_attr(not(feature = "rust-import"), allow(dead_code))]
    impl_uses: Vec<ImplUse>,
    /// The trait arguments of the impl this method belongs to, as text.
    impl_instance: String,
    /// User trait bounds a generic use requires of its inferred types.
    pending_traits: Vec<(Ty, ImmutableString, Box<[Ty]>, Span)>,
    /// Associated type equalities a generic use requires of its inferred
    /// types: `T::Output` and the bound's type.
    pending_projections: Vec<(Ty, Ty, Span)>,
    /// Type witnesses passed for `Default`-bounded generic parameters,
    /// written over their placeholders once the types are inferred.
    /// Each also says whether a reference's witness is its referent's.
    /// Each witness placeholder with the type its parameter, named as
    /// declared, takes at the call.
    pending_witnesses: Vec<(String, ImmutableString, Ty, Span, bool)>,
    /// This body's generic parameters a signature names inside a host
    /// type, in declaration order: each host operation here takes their
    /// witnesses after its operands.
    host_witnesses: Vec<ImmutableString>,
    /// The witnessed generics a proof being proven names as parameters.
    template_params: std::cell::RefCell<Box<[ImmutableString]>>,
    /// The witness names each such host operation took, by op index.
    witnessed_ops: BTreeMap<usize, Box<[ImmutableString]>>,
    /// Arithmetic on a literal's `{integer}` whose kind inference had not
    /// settled: each placeholder, written over with the kind's code once
    /// it has (`i32` when nothing binds it, as in Rust).
    pending_int_kinds: Vec<(String, Ty)>,
    /// Integer literals past `i64` whose kind inference had not settled:
    /// each must fit the kind it settles on.
    pending_wide_literals: Vec<PendingWide>,
    /// Float literals and arithmetic of a `{float}` whose kind inference
    /// had not settled: each placeholder, written over once it has. A
    /// literal becomes its text, rounded to `f32` when that is its kind;
    /// arithmetic becomes the call that rounds to `f32`, or blanks before
    /// the parenthesized value when it is an `f64`.
    pending_float_kinds: Vec<PendingFloat>,
    /// `Default::default()` calls whose type inference fixes later:
    /// `(placeholder, type, span)`, written over with the type's default.
    pending_defaults: Vec<(String, Ty, Span)>,
    no_struct: bool,
    /// The second `>` of a `>>` closing nested generic arguments.
    split_angle: Option<Position>,
    /// Where the block-like expression statement being parsed starts: an
    /// `if` there with no `else` is a statement, whose consequent must be
    /// `()` (E0308), where elsewhere its value is wanted (E0317).
    block_statement: Option<Position>,
    /// Where each `if` with no `else` starts: its value, `()`, where another
    /// type is expected is rustc's E0317.
    else_less: BTreeSet<Position>,
    /// The declared output of the body being parsed, for `?` and `return`.
    return_ty: Option<Ty>,
    /// A function whose `&mut` output borrows its receiver: each value it
    /// returns is the path to the place below the receiver
    /// (`places::returned_place`).
    place_output: bool,
    /// Explicit `return`s parsed so far; a closure that returns early has
    /// its declared or inferred output, not its body's type.
    returns: usize,
    /// Why an explicit `return` cannot leave this body, when it cannot.
    return_blocked: Option<&'static str>,
    /// In a crate with `impl Drop`: the scopes whose locals drop where they
    /// end (`drops.rs`), innermost last.
    drop_scopes: Vec<drops::Scope>,
    /// The first of `drop_scopes` the current body or closure opened: what
    /// a `return` leaves.
    drop_base: usize,
    /// The depth of `drop_scopes` the next loop's `break` and `continue`
    /// leave to, when its iteration opened a scope for its pattern.
    loop_drop_depth: Option<usize>,
    /// The subject of the pattern being read starts with `*`: a place
    /// read through a reference, which a match does not own.
    subject_deref: bool,
    /// The temporary scopes being read (`drops.rs`), innermost last: the
    /// hidden locals holding the values made in each that nothing moves.
    temporary_scopes: Vec<Vec<String>>,
    /// The `drops::Shape` of each temporary there whose type needs more
    /// than the walk of every part.
    temporary_shapes: BTreeMap<String, String>,
    /// Expressions that read a place through a reference, which no
    /// temporary holds (`materialize`).
    borrowed: BTreeSet<Span>,
    /// Each `&` borrow of a chain of fields below a local, by the borrow's
    /// span: the local and the chain's place (`lend_argument`).
    borrowed_fields: BTreeMap<Span, (ImmutableString, origin::Fragment)>,
    /// Moves an arm made out of a local its bindings shadow, deferred to
    /// after the match (`drops::Clears::InArm`): each flag's declaration
    /// and what runs once the match ends.
    deferred_clears: Vec<(String, String)>,
    #[cfg(feature = "rust-import")]
    imported: Option<ImportedContext<'a>>,
    /// Structural uses keep their inference variables until the body's
    /// return and later uses have constrained them.
    #[cfg(feature = "rust-import")]
    pending_structural: Vec<(usize, Box<[Ty]>, Span)>,
    #[cfg(feature = "rust-import")]
    structural_defaults: Vec<(Ty, Ty, Span)>,
}

impl<'a> Parser<'a> {
    #[cfg(feature = "rust-import")]
    fn native_site(&self) -> Option<super::native::Site<'_>> {
        let imported = self.imported.as_ref()?;
        Some(imported.native?.at(imported.source))
    }

    #[cfg(feature = "rust-import")]
    fn native_miss(&mut self, proven: bool, error: Error) -> Error {
        if proven && let Some(site) = self.native_site() {
            site.record(&error);
        }
        error
    }

    fn new_with_offset(
        engine: &Engine,
        source: &'a str,
        models: Models,
        op_offset: usize,
        pattern_offset: usize,
        #[cfg(feature = "rust-import")] imported: Option<ImportedContext<'a>>,
    ) -> Self {
        let mut tokens = lex_rust(engine, source);
        if models.imports_variants() {
            tokens = expand_imported_variants(&models, tokens);
        }
        let associated = Shared::clone(&models.associated);

        Self {
            marker: std::marker::PhantomData,
            text: source,
            input: tokens.into_iter().peekable(),
            models,
            bindings: BTreeMap::new(),
            output: origin::Fragment::default(),
            ops: Vec::new(),
            patterns: Vec::new(),
            op_offset,
            pattern_offset,
            last_type: Ty::Unit,
            last_expression: None,
            inference: Inference {
                associated,
                ..Inference::default()
            },
            callable: None,
            default_trait: None,
            formatter: None,
            copy_out: Vec::new(),
            copy_out_depth: 0,
            self_ty: None,
            impl_trait: None,
            root_expression_index: 0,
            expression_stack: Vec::new(),
            expression_child_indexes: Vec::new(),
            expression_paths: Vec::new(),
            normalize_private_calls: true,
            symbolic_only: false,
            #[cfg(feature = "rust-import")]
            recover_misses: false,
            #[cfg(feature = "rust-import")]
            recovered: Vec::new(),
            #[cfg(feature = "rust-import")]
            recovered_types: Vec::new(),
            #[cfg(feature = "rust-import")]
            revealing: None,
            loops: Vec::new(),
            closures: Vec::new(),
            moved_at: BTreeMap::new(),
            unsent_captures: BTreeMap::new(),
            borrowed_captures: BTreeMap::new(),
            deferred: BTreeMap::new(),
            uninitialized: Vec::new(),
            local_types: Vec::new(),
            diverging: Vec::new(),
            shared_field_reads: BTreeSet::new(),
            unmovable: BTreeMap::new(),
            owning_closures: BTreeSet::new(),
            once_closures: BTreeSet::new(),
            copied_state: BTreeMap::new(),
            shared_call_fns: BTreeSet::new(),
            closure_count: 0,
            lifted: Vec::new(),
            closure_hints: Vec::new(),
            debug_spec: None,
            closure_hint: None,
            bound_lifetimes: Vec::new(),
            expansions: 0,
            expansion_ends: Vec::new(),
            local_macros: Vec::new(),
            closure_outputs: Vec::new(),
            closure_output: None,
            async_step: None,
            async_closure: false,
            array_hint: None,
            collect_hint: None,
            pipelines: BTreeMap::new(),
            #[cfg(feature = "rust-import")]
            steps: BTreeMap::new(),
            range_values: BTreeMap::new(),
            last_range: None,
            field_entries: Vec::new(),
            native_modules: engine.global_modules.clone(),
            optional_subjects: BTreeMap::new(),
            lent_places: BTreeMap::new(),
            tuple_fields: BTreeMap::new(),
            chain_link: None,
            self_changed: std::cell::Cell::new(false),
            lend_self: false,
            mutable_borrow: None,
            lent_field: None,
            lent_mut_field: None,
            bound_iterators: BTreeMap::new(),
            held_iter_mut: BTreeMap::new(),
            pending_iter_mut: None,
            entry_aliases: BTreeMap::new(),
            last_cell_borrow: None,
            cell_guards: BTreeMap::new(),
            pending_member: None,
            aliases: BTreeMap::new(),
            alias_roots: BTreeMap::new(),
            undeclared_mut: BTreeSet::new(),
            reborrow_selectors: BTreeMap::new(),
            flow: std::cell::RefCell::default(),
            parameter_declarations: BTreeSet::new(),
            loop_entered: false,
            loop_iterator: None,
            loop_element: None,
            call_summaries: BTreeMap::new(),
            place_root: None,
            pattern_trace: std::cell::RefCell::new(None),
            argument_hints: Vec::new(),
            generic_argument_hints: Vec::new(),
            script_callables: false,
            script_callable: false,
            pending_arguments: None,
            frames: Vec::new(),
            frame_effects: Vec::new(),
            calls_noted: 0,
            frame_arrays: Vec::new(),
            call_parens: Vec::new(),
            callee: None,
            temporaries: 0,
            pending_conversions: Vec::new(),
            pending_formats: Vec::new(),
            pending_parses: Vec::new(),
            pending_collects: Vec::new(),
            settled_parses: Vec::new(),
            parse_count: 0,
            text_into: Vec::new(),
            err_payloads: BTreeMap::new(),
            generics: BTreeMap::new(),
            opaque: BTreeMap::new(),
            pending_turbofish: None,
            trait_turbofish: None,
            path_values: HashSet::new(),
            #[cfg(feature = "rust-import")]
            path_outputs: BTreeMap::new(),
            spliced_traits: BTreeMap::new(),
            spliced_turbofish: BTreeMap::new(),
            std_probed: BTreeMap::new(),
            host_turbofish: None,
            explicit_trait_arguments: None,
            binding_mode: std::cell::Cell::new(None),
            or_binding_types: std::cell::RefCell::new(Vec::new()),
            slice_arity: std::cell::Cell::new(0),
            host_checks: std::cell::RefCell::new(Vec::new()),
            host_binding_mode: std::cell::Cell::new(None),
            deferred_hints: None,
            pending_bounds: Vec::new(),
            generic_traits: BTreeMap::new(),
            generic_trait_arguments: BTreeMap::new(),
            pending_instances: Vec::new(),
            literal_kinds: Vec::new(),
            literal_values: Vec::new(),
            trait_parameters: BTreeMap::new(),
            instance_calls: Vec::new(),
            collecting_instances: false,
            reaches_host: false,
            impl_uses: Vec::new(),
            impl_instance: String::new(),
            pending_traits: Vec::new(),
            pending_projections: Vec::new(),
            pending_witnesses: Vec::new(),
            host_witnesses: Vec::new(),
            template_params: std::cell::RefCell::new(Box::default()),
            witnessed_ops: BTreeMap::new(),
            pending_int_kinds: Vec::new(),
            pending_wide_literals: Vec::new(),
            pending_float_kinds: Vec::new(),
            pending_defaults: Vec::new(),
            no_struct: false,
            split_angle: None,
            block_statement: None,
            else_less: BTreeSet::new(),
            return_ty: None,
            place_output: false,
            returns: 0,
            return_blocked: None,
            drop_scopes: Vec::new(),
            drop_base: 0,
            loop_drop_depth: None,
            subject_deref: false,
            temporary_scopes: Vec::new(),
            temporary_shapes: BTreeMap::new(),
            borrowed: BTreeSet::new(),
            borrowed_fields: BTreeMap::new(),
            deferred_clears: Vec::new(),
            #[cfg(feature = "rust-import")]
            imported,
            #[cfg(feature = "rust-import")]
            pending_structural: Vec::new(),
            #[cfg(feature = "rust-import")]
            structural_defaults: Vec::new(),
        }
    }

    fn for_method(
        engine: &Engine,
        source: &'a str,
        models: Models,
        definition: &method::Definition,
        op_offset: usize,
        pattern_offset: usize,
        #[cfg(feature = "rust-import")] imported: Option<ImportedContext<'a>>,
    ) -> Result<Self, Error> {
        let mut parser = Self::new_with_offset(
            engine,
            source,
            models,
            op_offset,
            pattern_offset,
            #[cfg(feature = "rust-import")]
            imported,
        );
        parser.callable = Some(definition.key().clone());
        parser.default_trait = definition
            .trait_default()
            .map(|trait_name| (definition.owner(), trait_name.clone()));
        parser.impl_trait = definition.trait_name().cloned();
        let formatter = definition
            .trait_name()
            .filter(|trait_name| {
                matches!(trait_name.as_str(), "Debug" | "Display")
                    && parser.models.std_traits.contains(*trait_name)
            })
            .and_then(|_| definition.parameters().first())
            .map(|parameter| parameter.name.text.clone())
            .or_else(|| definition.formatter().cloned());
        parser.return_ty = Some(Ty::from_resolved(definition.output()));
        parser.place_output = definition.name() != "index_mut"
            && matches!(definition.receiver(), Some(receiver::Mode::Mutable))
            && parser
                .place_output(&Ty::from_resolved(definition.output()))
                .is_some();
        parser.generics = definition
            .generics()
            .iter()
            .map(|generic| (generic.name.text.clone(), generic.bounds))
            .collect();
        parser.opaque = definition.opaque().clone();
        parser.generic_traits = generic_traits(definition.generics());
        parser.host_witnesses = host_witnesses(definition.generics());
        parser.generic_trait_arguments = generic_trait_arguments(definition.generics());
        parser.impl_instance = instance_text(definition.trait_arguments());
        // `T` of `trait Convert<T>` is the impl's argument.
        if let Some(trait_name) = definition.trait_name()
            && let Some((_, declared)) = parser.models.traits.get(trait_name)
        {
            parser.trait_parameters = declared
                .generics
                .iter()
                .map(|generic| generic.name.text.clone())
                .zip(definition.trait_arguments().iter().map(Ty::from_resolved))
                .collect();
        }
        // An instance's body reads each of its template's generic
        // parameters as the type this instance gives it (D629).
        #[cfg(feature = "rust-import")]
        if let Some(types) = parser.models.instances.types.get(definition.key()).cloned() {
            for (name, ty) in types.iter() {
                parser
                    .trait_parameters
                    .insert(name.clone(), Ty::from_resolved(ty));
            }
        }
        parser.inference.equalities = generic_equalities(definition.generics());
        parser.copy_out = method_copied(definition, &parser.models)
            .into_iter()
            .map(|index| definition.parameters()[index].name.text.clone())
            .collect();

        for parameter in definition.parameters() {
            let ty = Ty::from_resolved(&parameter.ty);
            let invocation = ty.invocation().is_some();
            parser.parameter_declarations.insert(parameter.name.span);
            parser.bindings.insert(
                parameter.name.text.clone(),
                Binding {
                    mutable: matches!(ty, Ty::Reference { mutable: true, .. }),
                    available: true,
                    ty,
                    declaration: parameter.name.span,
                    kind: if invocation {
                        BindingKind::Invocation
                    } else {
                        BindingKind::Local
                    },
                },
            );
        }

        if let Some(name) = formatter {
            parser.bind_formatter(name);
        }

        parser.self_ty = Some(impl_self_ty(definition, &BTreeMap::new()));
        if let Some(mode) = definition.receiver() {
            let owner = impl_self_ty(definition, &BTreeMap::new());
            let (ty, mutable) = match mode {
                receiver::Mode::Shared => (
                    Ty::Reference {
                        mutable: false,
                        target: Box::new(owner),
                    },
                    false,
                ),
                receiver::Mode::Mutable => (
                    Ty::Reference {
                        mutable: true,
                        target: Box::new(owner),
                    },
                    true,
                ),
                receiver::Mode::Owned => (owner, definition.receiver_mutable()),
            };
            parser.bindings.insert(
                "self".into(),
                Binding {
                    ty,
                    mutable,
                    available: true,
                    declaration: definition
                        .receiver_span()
                        .expect("resolved receiver modes retain their declaration span"),
                    kind: BindingKind::Receiver {
                        owner: definition.owner_name().into(),
                    },
                },
            );
        }

        Ok(parser)
    }

    fn for_function(
        engine: &Engine,
        source: &'a str,
        models: Models,
        definition: &item::Function,
        key: crate::item::Key,
        op_offset: usize,
        pattern_offset: usize,
        #[cfg(feature = "rust-import")] imported: Option<ImportedContext<'a>>,
    ) -> Self {
        let mut parser = Self::new_with_offset(
            engine,
            source,
            models,
            op_offset,
            pattern_offset,
            #[cfg(feature = "rust-import")]
            imported,
        );
        parser.callable = Some(key);
        parser.return_ty = Some(
            definition
                .output
                .as_ref()
                .map_or(Ty::Unit, Ty::from_resolved),
        );
        parser.place_output = !definition.body.constant
            && this_parameter(definition).is_some()
            && definition
                .output
                .as_ref()
                .is_some_and(|output| parser.place_output(&Ty::from_resolved(output)).is_some());
        parser.generics = definition
            .generics
            .iter()
            .map(|generic| (generic.name.text.clone(), generic.bounds))
            .collect();
        parser.generic_traits = generic_traits(&definition.generics);
        parser.host_witnesses = host_witnesses(&definition.generics);
        parser.generic_trait_arguments = generic_trait_arguments(&definition.generics);
        parser.inference.equalities = generic_equalities(&definition.generics);
        // An instance's body reads each of its template's generic
        // parameters as the type this instance gives it (D629).
        let types = parser
            .callable
            .as_ref()
            .and_then(|key| parser.models.instances.types.get(key).cloned());
        if let Some(types) = types {
            for (name, ty) in types.iter() {
                parser
                    .trait_parameters
                    .insert(name.clone(), Ty::from_resolved(ty));
            }
        }

        let receiver = this_parameter(definition);
        parser.copy_out = copied_parameters(
            &definition.parameters,
            receiver,
            definition.formatter.as_ref(),
        )
        .into_iter()
        .map(|index| definition.parameters[index].name.text.clone())
        .collect();
        for (index, parameter) in definition.parameters.iter().enumerate() {
            let ty = Ty::from_resolved(&parameter.ty);
            // The `&mut` parameter is `this`, the caller's place.
            if Some(index) == receiver {
                parser.bindings.insert(
                    "self".into(),
                    Binding {
                        ty: ty.clone(),
                        mutable: true,
                        available: true,
                        declaration: parameter.name.span,
                        kind: BindingKind::Local,
                    },
                );
                parser.alias_element(
                    parameter.name.text.clone(),
                    parameter.name.span,
                    ty,
                    origin::Fragment::plain("self"),
                    "self".into(),
                );
                continue;
            }
            let invocation = ty.invocation().is_some();
            parser.parameter_declarations.insert(parameter.name.span);
            parser.bindings.insert(
                parameter.name.text.clone(),
                Binding {
                    mutable: matches!(ty, Ty::Reference { mutable: true, .. }),
                    available: true,
                    ty,
                    declaration: parameter.name.span,
                    kind: if invocation {
                        BindingKind::Invocation
                    } else {
                        BindingKind::Local
                    },
                },
            );
        }
        if let Some(name) = definition.formatter.clone() {
            parser.bind_formatter(name);
        }

        parser
    }

    /// The formatter `name` is the text the body writes, a mutable
    /// `String`, which the body gives back (`wrap_formatter`).
    fn bind_formatter(&mut self, name: ImmutableString) {
        if let Some(binding) = self.bindings.get_mut(name.as_str()) {
            binding.ty = Ty::String;
            binding.mutable = true;
        }
        self.formatter = Some(name);
        self.return_blocked = Some("from a `fmt` body; use its tail");
    }

    /// The variables of an embedder's `Scope`, which the script reads as
    /// locals the host declared: each has its value's type, as a value a
    /// registered function returns does, and a constant is immutable.
    fn bind_scope(&mut self, scope: &Scope) {
        use std::any::TypeId;
        for (name, constant, value) in scope.iter_raw() {
            let id = value.type_id();
            let ty = if id == TypeId::of::<crate::INT>() {
                Ty::Int(Int::I64)
            } else if id == TypeId::of::<crate::FLOAT>() {
                Ty::Float(Float::F64)
            } else if id == TypeId::of::<bool>() {
                Ty::Bool
            } else if id == TypeId::of::<char>() {
                Ty::Char
            } else if id == TypeId::of::<ImmutableString>() {
                Ty::String
            } else if id == TypeId::of::<()>() {
                Ty::Unit
            } else {
                Ty::Unknown
            };
            self.bindings.insert(
                name.into(),
                Binding {
                    ty,
                    mutable: !constant,
                    available: true,
                    declaration: Span::NONE,
                    kind: BindingKind::Local,
                },
            );
        }
    }

    /// rustc's syntax error at the next token when no expression continues
    /// with it: a name, a literal, a keyword or a `!` after an expression
    /// (`2 in y`, `1 !in v`). What rustc lists as expected depends on what
    /// the expression ends ([`Continuation`]).
    fn expected_operator(&mut self, continuation: Continuation) -> Option<Error> {
        let (token, position) = self.input.peek().map(|(token, at)| (token.clone(), *at))?;
        let stops = matches!(
            token,
            Token::Identifier(_)
                | Token::IntegerConstant(_)
                | Token::StringConstant(_)
                | Token::CharConstant(_)
                | Token::ByteConstant(_)
                | Token::Bang
                | Token::True
                | Token::False
                | Token::Fn
                | Token::Let
                | Token::Const
                | Token::In
                | Token::If
                | Token::While
                | Token::Loop
                | Token::For
                | Token::Return
        ) || {
            {
                matches!(token, Token::FloatConstant(_))
            }
        };
        if !stops || self.split_angle.is_some() {
            return None;
        }
        let (expected, label) = match continuation {
            Continuation::Path => (
                "expected one of `!`, `.`, `::`, `;`, `?`, `{`, `}`, or an operator",
                "expected one of 8 possible tokens",
            ),
            Continuation::Statement => (
                "expected one of `.`, `;`, `?`, `}`, or an operator",
                "expected one of `.`, `;`, `?`, `}`, or an operator",
            ),
            Continuation::Let => (
                "expected one of `.`, `;`, `?`, `else`, or an operator",
                "expected one of `.`, `;`, `?`, `else`, or an operator",
            ),
        };
        let found = crate::parser::item::found(&token);
        let written = found
            .trim_start_matches("reserved ")
            .trim_start_matches("keyword ")
            .trim_matches('`');
        Some(Error::Syntax {
            message: format!("{expected}, found {found}").into(),
            label: label.into(),
            help: None,
            at: Span::new(position, text_end(position, written)),
        })
    }

    /// An expression expected to have type `expected`: an array literal's
    /// elements coerce to its element type.
    fn parse_expr_expecting(&mut self, expected: Option<&Ty>) -> Result<ParsedExpr, Error> {
        if let Some(expected) = expected {
            self.array_hint = self.literal_element(expected);
        }
        let hint = expected.map(|expected| (expected.clone(), self.expression_stack.len() + 1));
        let outer = std::mem::replace(&mut self.collect_hint, hint);
        let value = self.parse_expr();
        self.collect_hint = outer;
        self.array_hint = None;
        value
    }

    /// The element type the array or `vec!` literal ahead is expected to
    /// have when the expression is expected to be `expected`, as rustc's
    /// expectation reaches it: `[..]` and `vec![..]` of a sequence, and
    /// `&[..]` of a reference to one (`&[&A, &B]` as a `&[&dyn Named]`).
    /// A `&vec![..]` reaches only a `&Vec<T>`: a `&[T]` is its deref, which
    /// carries no expectation.
    fn literal_element(&mut self, expected: &Ty) -> Option<Ty> {
        if self.split_angle.is_some() {
            return None;
        }
        let mut ahead = self.input.clone().map(|(token, _)| token);
        let mut expected = self.inference.resolve(expected);
        let mut token = ahead.next()?;
        let referenced = matches!(token, Token::Ampersand);
        if referenced {
            let Ty::Reference { target, .. } = expected else {
                return None;
            };
            expected = self.inference.resolve(&target);
            token = ahead.next()?;
            if matches!(token, Token::Mut) {
                token = ahead.next()?;
            }
        }
        let vec = match token {
            Token::LeftBracket => false,
            Token::Identifier(name) if name.as_str() == "vec" => {
                matches!(ahead.next(), Some(Token::Bang)).then_some(true)?
            }
            _ => return None,
        };
        let Ty::Array { element, kind } = expected else {
            return None;
        };
        (!(referenced && vec) || kind == sequence::Kind::Vec).then_some(*element)
    }

    /// Whether the expression ahead ends at the enclosing block's `}`: its
    /// first bracket's match is followed by it, as a body's tail literal
    /// `vec![..]` is.
    fn literal_ends_block(&mut self) -> bool {
        let mut ahead = self.input.clone().map(|(token, _)| token);
        let mut depth = 0_usize;
        while let Some(token) = ahead.next() {
            match token {
                Token::LeftBracket | Token::LeftParen | Token::LeftBrace | Token::MapStart => {
                    depth += 1;
                }
                Token::RightBracket | Token::RightParen | Token::RightBrace if depth > 1 => {
                    depth -= 1;
                }
                Token::RightBracket | Token::RightParen | Token::RightBrace if depth == 1 => {
                    return matches!(ahead.next(), Some(Token::RightBrace));
                }
                Token::RightBracket | Token::RightParen | Token::RightBrace | Token::EOF => {
                    return false;
                }
                _ => {}
            }
        }
        false
    }

    fn parse_expr(&mut self) -> Result<ParsedExpr, Error> {
        self.parse_expr_in(false)
    }

    fn peek_block_like(&mut self) -> bool {
        match self.peek() {
            Token::If
            | Token::While
            | Token::Loop
            | Token::For
            | Token::LeftBrace
            | Token::Apostrophe => true,
            Token::Reserved(keyword) => keyword.as_str() == "match",
            _ => false,
        }
    }

    /// A statement that starts with a block-like expression ends with it
    /// (`block`), as in Rust; otherwise this parses a full expression.
    fn parse_expr_in(&mut self, block: bool) -> Result<ParsedExpr, Error> {
        let path = match self.expression_stack.last() {
            Some(parent) => {
                let child = self
                    .expression_child_indexes
                    .last_mut()
                    .expect("every active expression path retains its next child index");
                let index = *child;
                *child = child
                    .checked_add(1)
                    .expect("one expression cannot contain more than u32::MAX children");
                let mut path = parent.to_vec();
                path.push(index);
                path.into_boxed_slice()
            }
            None => {
                let index = self.root_expression_index;
                self.root_expression_index = self
                    .root_expression_index
                    .checked_add(1)
                    .expect("one callable cannot contain more than u32::MAX root expressions");
                vec![index].into_boxed_slice()
            }
        };
        self.expression_stack.push(path.clone());
        self.expression_child_indexes.push(0);
        let expression = if block {
            // As in Rust, `match self { .. }.fmt(f)` and `{ .. }?` go on
            // after the block. A `(` or `[` there starts a new statement.
            self.parse_primary().and_then(|value| {
                let continues = match self.peek() {
                    Token::Period => true,
                    Token::Reserved(keyword) => keyword.as_str() == "?",
                    _ => false,
                };
                if continues {
                    self.parse_postfix_of(value)
                } else {
                    Ok(value)
                }
            })
        } else {
            self.parse_range()
        };
        self.expression_stack.pop();
        self.expression_child_indexes.pop();
        let expression = expression?;
        self.expression_paths.push((expression.span, path));
        Ok(expression)
    }

    /// `at`, a point where `name` is written, over the name, as rustc
    /// marks a local where it is declared, moved and used.
    fn name_span(&self, at: Span, name: &str) -> Span {
        let named = span(at.start(), text_end(at.start(), name));
        if at.start() == at.end() && named.text(self.text) == name {
            named
        } else {
            at
        }
    }

    /// Where `name` is in `written`, the inside of a `{...}` that starts
    /// at char `at` of a `format!` string's value, in the literal at
    /// `literal`: rustc points at it there. None for a literal the source
    /// doesn't show as written.
    fn template_name_span(
        &self,
        literal: Span,
        template: &str,
        written: &str,
        at: usize,
        name: &str,
    ) -> Option<Span> {
        let identifier = |character: char| character == '_' || character.is_alphanumeric();
        let offset = written.match_indices(name).find_map(|(offset, _)| {
            let before = written[..offset].chars().next_back();
            let after = written[offset + name.len()..].chars().next();
            (!before.is_some_and(identifier) && !after.is_some_and(identifier))
                .then(|| written[..offset].chars().count())
        })?;
        let start = at + offset;
        literal::value_span(
            literal.text(self.text),
            literal.start(),
            template,
            start..start + name.chars().count(),
        )
    }

    /// Write `text` over `placeholder`, padded to its length so source
    /// locations stay exact, in the body and its lifted closures. Whether
    /// the placeholder was written yet: a call still being read isn't.
    fn write_over(&mut self, placeholder: &str, text: &str) -> bool {
        let replacement = format!("{text:<width$}", width = placeholder.len());
        debug_assert_eq!(replacement.len(), placeholder.len());
        let mut found = self.output.replace_same_length(placeholder, &replacement);
        for closure in &mut self.lifted {
            found |= closure.body.replace_same_length(placeholder, &replacement);
        }
        found
    }

    fn parse_operand(&mut self) -> Result<ParsedExpr, Error> {
        // `&&x`, which the lexer reads as one `&&`: `& &x`.
        if matches!(self.peek(), Token::And) {
            let at = self.next().1;
            let mut second = at;
            second.advance();
            self.splice_tokens(vec![(Token::Ampersand, at), (Token::Ampersand, second)]);
        }
        if matches!(self.peek(), Token::Ampersand) {
            let start = self.next().1;
            if matches!(self.peek(), Token::Mut) {
                // `&mut place` as a function's `&mut` argument, which is
                // the call's receiver; a by-value runtime cannot keep one
                // in a local.
                // `index_mut`'s `&mut place` is its value; writes reach the
                // place where it is inlined (`indexed_place`).
                let index_mut = self.place_output
                    || matches!(&self.return_ty, Some(Ty::Reference { mutable: true, .. }));
                if !index_mut && self.mutable_borrow != Some(self.expression_stack.len()) {
                    return Err(self.unsupported(
                        "`&mut` borrow expressions other than a function's `&mut` argument",
                    ));
                }
                self.next();
                let operand_start = self.position();
                let lent = self.lent_mut_field.replace(operand_start);
                let operand = self.parse_operand();
                self.lent_mut_field = lent;
                let mut operand = operand?;
                let whole = span(start, operand.span.end());
                if operand.lvalue.is_none() {
                    // `twice(&mut |x| v.push(x))`: the closure is the
                    // borrowed temporary, and the callee calls it where it
                    // is, as its captures are where the closure reaches them.
                    if !matches!(self.inference.resolve(&operand.ty), Ty::FnItem { .. }) {
                        return Err(self.unsupported("`&mut` of a value that is not a place"));
                    }
                    self.borrow_temporary_mut(&operand, whole);
                    operand.source = operand.rendered_source().surrounded("(", ")");
                    operand.prelude = origin::Fragment::default();
                    operand.ty = Ty::Reference {
                        mutable: true,
                        target: Box::new(self.inference.resolve(&operand.ty)),
                    };
                    operand.span = whole;
                    return Ok(operand.retain_origin());
                }
                let inner = self.may_hold_borrow(&operand.ty);
                let reference = borrows::is_reference(&self.inference.resolve(&operand.ty));
                self.flow().borrow(
                    operand.span.start(),
                    operand.span,
                    whole,
                    true,
                    inner,
                    reference,
                );
                self.flow().alias(operand.span.start(), start);
                operand.ty = Ty::Reference {
                    mutable: true,
                    target: Box::new(self.inference.resolve(&operand.ty)),
                };
                operand.span = whole;
                return Ok(operand.retain_origin());
            }
            let operand_start = self.position();
            let lent = self.lent_field.replace(operand_start);
            let operand = self.parse_operand();
            self.lent_field = lent;
            let mut operand = operand?;
            let whole = span(start, operand.span.end());
            if operand.lvalue.is_some() || self.flow().is_place(operand.span) {
                let inner = self.may_hold_borrow(&operand.ty);
                let reference = borrows::is_reference(&self.inference.resolve(&operand.ty));
                self.flow().borrow(
                    operand.span.start(),
                    operand.span,
                    whole,
                    false,
                    inner,
                    reference,
                );
            } else {
                // `&String::from("a")`, `&(n + 1)`: a borrow of a temporary.
                self.borrow_temporary(&operand, whole);
            }
            self.hold_temporary(&mut operand)?;
            // `&*s` of a `String`: a `&str`.
            operand.ty = match self.inference.resolve(&operand.ty) {
                Ty::UnsizedStr => Ty::Str,
                target => Ty::Reference {
                    mutable: false,
                    target: Box::new(target),
                },
            };
            operand.span = span(start, operand.span.end());
            // `&s.a.b`: the field chain a call may lend (`lend_argument`).
            if let Some(field) = fields_below_local(&operand) {
                self.borrowed_fields.insert(operand.span, field);
            }
            operand.lvalue = None;
            return Ok(operand.retain_origin());
        }
        if matches!(self.peek(), Token::Multiply) {
            let start = self.next().1;
            let operand = self.parse_operand()?;
            // `*r` reads what the holder `r` refers to: its use's place.
            self.flow().deref(operand.span.start(), start);
            let had_place = operand.lvalue.is_some() || self.flow().is_place(operand.span);
            // `*w` of a program type is `*Deref::deref(&w)`, and
            // `*DerefMut::deref_mut(&mut w)` where it is assigned, as
            // rustc's place deref is.
            let (mut operand, dereferenced) = self.program_deref(operand)?;
            let resolved = self.inference.resolve(&operand.ty);
            let target = match resolved.clone() {
                Ty::Reference { target, .. } | Ty::Boxed(target) | Ty::Counted(_, target) => target,
                // `*s` of a `&str` is the `str` it refers to.
                Ty::Str => Box::new(Ty::UnsizedStr),
                // `*s` of a `String` or a `Cow`: its `Deref` target, from
                // std's impls.
                ty => match self.deref_target(&ty, false) {
                    Some((
                        target,
                        call @ (std_impl::DerefCall::Value | std_impl::DerefCall::Cow),
                    )) => {
                        self.apply_derefs(&mut operand, vec![call], ty)?;
                        Box::new(target)
                    }
                    _ => {
                        return Err(
                            self.unsupported("`*` on a value that is not a reference or box")
                        )
                    }
                },
            };
            // `*v = x` through a `&mut` local or place (a map's entry, an
            // `and_modify` value) writes it.
            // `*self = x` in a `&mut self` method replaces the receiver.
            let receiver = operand.lvalue.is_none()
                && self.closures.is_empty()
                && operand.source.as_str() == "self"
                && self.bindings.get("self").is_some_and(|binding| {
                    binding.mutable && matches!(binding.kind, BindingKind::Receiver { .. })
                });
            if receiver {
                operand.lvalue = Some(Lvalue::Binding {
                    binding: "self".into(),
                    ty: Ty::Unknown,
                });
            }
            let through_reference = matches!(resolved, Ty::Reference { .. });
            operand.lvalue = match (resolved, operand.lvalue.take()) {
                (Ty::Reference { mutable: true, .. }, Some(Lvalue::Binding { binding, .. })) => {
                    Some(Lvalue::Binding {
                        binding,
                        ty: (*target).clone(),
                    })
                }
                (
                    Ty::Reference { mutable: true, .. },
                    Some(Lvalue::Place { binding, place, .. }),
                ) => Some(Lvalue::Place {
                    binding,
                    ty: (*target).clone(),
                    place,
                }),
                (Ty::Reference { mutable: true, .. }, Some(Lvalue::Nested(mut nested))) => {
                    nested.ty = (*target).clone();
                    Some(Lvalue::Nested(nested))
                }
                // `*b` of a local `Box`: the box's own place, which a use
                // by value moves, with drop glue in the crate.
                (Ty::Boxed(_), Some(Lvalue::Binding { binding, .. })) if self.drops() => {
                    Some(Lvalue::Binding {
                        binding,
                        ty: (*target).clone(),
                    })
                }
                _ => None,
            };
            // `*rc` reads the value through std's `Deref`.
            if matches!(self.inference.resolve(&operand.ty), Ty::Counted(..)) {
                self.counted_value(&mut operand);
            }
            let base = (operand.span, operand.ty.clone());
            operand.ty = *target;
            operand.span = span(start, operand.span.end());
            // A move out of `*r` or `*rc` is rustc's E0507; a `Box` gives
            // its value up.
            let behind = match self.inference.resolve(&base.1) {
                _ if let Some(owner) = &dereferenced => Some(format!("dereference of `{owner}`")),
                Ty::Reference { mutable: true, .. } => {
                    Some("a value behind a mutable reference".into())
                }
                Ty::Reference { .. } => Some("a value behind a shared reference".into()),
                Ty::Counted(Counter::Rc, _) => Some("a value of an `Rc`".into()),
                Ty::Counted(Counter::Arc, _) => Some("a value of an `Arc`".into()),
                _ => None,
            };
            if let Some(behind) = behind {
                let assigned = match self.inference.resolve(&base.1) {
                    Ty::Counted(counter, _) => Some(counter.unassignable()),
                    _ => None,
                };
                self.unmovable.insert(
                    operand.span,
                    Unplaced {
                        moved: ("E0507", format!("cannot move out of {behind}")),
                        assigned,
                    },
                );
            }
            // `*x` of a `&&T` is the `&T` it refers to.
            self.part_of(
                (base.0, &base.1),
                operand.span,
                &operand.ty,
                borrows::PartOf::Deref,
            );
            if had_place {
                self.flow().mark_place(operand.span);
            }
            if through_reference && operand.lvalue.is_none() && self.drops() {
                self.borrowed.insert(operand.span);
            }
            return Ok(operand.retain_origin());
        }
        if matches!(self.peek(), Token::Minus | Token::UnaryMinus | Token::Bang) {
            self.parse_unary()
        } else {
            self.parse_postfix()
        }
    }

    fn parse_unary(&mut self) -> Result<ParsedExpr, Error> {
        let (operator, allowed) = match self.peek() {
            Token::Bang => ("!", [Ty::Bool, Ty::Int(Int::I64)]),
            _ => ("-", [Ty::Int(Int::I64), Ty::Float(Float::F64)]),
        };
        let start = self.next().1;
        let operand = self.parse_operand()?;
        let symbol = if operator == "-" { "neg" } else { "!" };
        if let Some(mut value) = self.operator_impl(symbol, &operand, None)? {
            value.span = span(start, operand.span.end());
            return Ok(value);
        }
        let mut ty = self.inference.resolve(&operand.ty);
        // `-x` and `!x` of a `&i64` or `&bool`: std's `impl Neg for &i64`
        // and kin give the value.
        if let Ty::Reference { target, .. } = &ty
            && let target @ (Ty::Int(_) | Ty::Float(_) | Ty::Bool) = self.inference.resolve(target)
        {
            ty = target;
        }
        // `!x` of an integer flips its bits.
        if operator == "!"
            && let Ty::Int(_) = ty
        {
            let expression = span(start, operand.span.end());
            let source = self.int_op("!", operand.rendered_source(), "0".into(), &ty, &ty);
            return Ok(self.operator_expr(source, ty, expression));
        }
        // `-x` on an unsigned integer is E0600.
        if !allowed_type(&allowed, &ty)
            || operator == "-" && matches!(ty, Ty::Int(kind) if !kind.signed())
        {
            return Err(Error::TypeMismatch {
                expected: allowed[0].name(&self.models),
                actual: ty.name(&self.models),
                expression: operand.span,
            });
        }
        if let Ty::Int(kind) = ty
            && kind.wide()
        {
            let expression = span(start, operand.span.end());
            let source = self.int_op("neg", operand.rendered_source(), "0".into(), &ty, &ty);
            return Ok(self.operator_expr(source, ty, expression));
        }
        let source = operand
            .rendered_source()
            .surrounded(&format!("{operator}("), ")");
        let value = self.operator_expr(source, ty, span(start, operand.span.end()));
        Ok(if operator == "-" {
            self.checked_arithmetic(value, "neg")
        } else {
            value
        })
    }

    fn parse_primary(&mut self) -> Result<ParsedExpr, Error> {
        if matches!(self.peek(), Token::LeftBrace) {
            return self.parse_block_expr();
        }
        // `unsafe { .. }` is its block: Rawr has none of the operations
        // that need it, so what it holds is checked as anywhere else.
        if matches!(self.peek(), Token::Identifier(ref name) if name.as_str() == "unsafe")
            && matches!(self.peek_second(), Token::LeftBrace)
        {
            self.next();
            return self.parse_block_expr();
        }
        let (token, position) = self.next();
        let token_span = point_span(position);

        match token {
            Token::Unit => Ok(self.literal("()", Ty::Unit, token_span)),
            Token::True => Ok(self.literal("true", Ty::Bool, token_span)),
            Token::False => Ok(self.literal("false", Ty::Bool, token_span)),
            Token::IntegerConstant(value) => {
                let (ty, float) = self.literal_kind(value, true, token_span)?;
                let text = if float {
                    format!("{value}.0")
                } else {
                    value.to_string()
                };
                Ok(self.literal(text, ty, token_span))
            }
            Token::CharConstant(value) => {
                Ok(self.literal(format!("{value:?}"), Ty::Char, token_span))
            }
            Token::StringConstant(value) => {
                // A literal is `&'static str`: it borrows nothing of the body.
                self.flow().set_origins(token_span, Vec::new());
                Ok(self.literal(format!("{value:?}"), Ty::Str, token_span))
            }
            // `b"…"`: a `&[u8; N]`, an array of its bytes at runtime.
            Token::ByteStringConstant(bytes) => Ok(self.byte_literal(&bytes, token_span)),
            Token::ByteConstant(byte) => {
                Ok(self.literal(byte.to_string(), Ty::Int(Int::U8), token_span))
            }
            Token::FloatConstant(value)
                if let Some(literal) = self.wide_literal(&value.1, token_span)? =>
            {
                Ok(literal)
            }
            Token::FloatConstant(value) => {
                let ty = self.literal_kind(0, false, token_span)?.0;
                let number = *value.0;
                let text = match &ty {
                    // An `f32` holds the nearest `f32`, as Rust's literal.
                    Ty::Float(Float::F32) => format!("{:?}", number as f32 as f64),
                    // Its kind is known once inference settles.
                    Ty::Float(Float::Var(_)) => {
                        let placeholder =
                            format!("__rawr_float_literal_{:08}", self.pending_float_kinds.len());
                        self.pending_float_kinds.push(PendingFloat {
                            placeholder: placeholder.clone(),
                            ty: ty.clone(),
                            kind: PendingFloatKind::Literal(number),
                        });
                        placeholder
                    }
                    _ => value.1.to_string(),
                };
                Ok(self.literal(text, ty, token_span))
            }
            Token::LeftParen => {
                let outer = std::mem::replace(&mut self.no_struct, false);
                let value = self.parse_parenthesized(position);
                self.no_struct = outer;
                value
            }
            Token::If => self.parse_if(token_span),
            Token::While | Token::Loop | Token::For => self.parse_loop(token, token_span, None),
            Token::Apostrophe => {
                let (label, _) = self.expect_name("loop label")?;
                self.expect(Token::Colon, "':' after a loop label")?;
                let (token, position) = self.next();
                if !matches!(token, Token::While | Token::Loop | Token::For) {
                    return Err(Error::UnsupportedNominalSyntax {
                        construct: "label on something other than a loop".into(),
                        source: point_span(position),
                    });
                }
                self.parse_loop(token, token_span, Some(label))
            }
            Token::Break | Token::Continue => self.parse_jump(token, token_span),
            Token::Return => self.parse_return(token_span),
            Token::Pipe | Token::Or => self.parse_closure(token, token_span, false),
            Token::Reserved(keyword) if keyword.as_str() == "async" => self.parse_async(token_span),
            Token::LeftBracket => self.parse_array(position, Token::RightBracket, Some(0)),
            Token::Reserved(keyword) if keyword.as_str() == "match" => self.parse_match(token_span),
            Token::Identifier(name) => self.parse_name_primary((*name).into(), token_span),
            // `::image::ImageReader::new(..)`: a leading `::` names an
            // outside crate, which no `use`, local or function shadows.
            Token::DoubleColon if matches!(self.peek(), Token::Identifier(_)) => {
                let (Token::Identifier(name), position) = self.next() else {
                    unreachable!("an identifier after `::`");
                };
                let name: ImmutableString = (*name).into();
                let name_span = point_span(position);
                if matches!(name.as_str(), "crate" | "Self")
                    || self.bindings.contains_key(&name)
                    || self.models.function(&name).is_some()
                    || !matches!(self.peek(), Token::DoubleColon)
                {
                    let reference = span(token_span.start(), name_span.end());
                    return Err(crate::resolve::not_a_crate(&name, reference));
                }
                let outer = self.models.global_head.replace(name.clone());
                let value = self.parse_name_primary(name, name_span);
                self.models.global_head = outer;
                value
            }
            // `<T as Trait>::f(..)`: a trait's function for the type `T`.
            Token::LessThan => {
                let self_ty = self.parse_local_type()?;
                self.expect(Token::As, "'as' in a qualified path")?;
                let (mut trait_name, mut trait_span) =
                    self.expect_name("trait in a qualified path")?;
                let root = trait_name.clone();
                let mut segments = 1_usize;
                // `<T as std::default::Default>`: the path's last segment.
                while matches!(self.peek(), Token::DoubleColon)
                    && matches!(self.peek_second(), Token::Identifier(_))
                {
                    self.next();
                    (trait_name, trait_span) = self.expect_name("trait in a qualified path")?;
                    segments += 1;
                }
                // A path through a crate, as `serde::de::Error`, names the
                // crate's trait, never std's of the same name.
                // `fmt::Display` after `use std::fmt;` is std's.
                let root = self.models.import(&root).map_or_else(
                    || root.to_string(),
                    |binding| {
                        binding
                            .qualified
                            .split("::")
                            .next()
                            .unwrap_or_default()
                            .to_owned()
                    },
                );
                let through_crate = segments > 1 && !crate::std_table::is_root(&root);
                let explicit = if matches!(self.peek(), Token::LessThan) {
                    Some(self.parse_trait_path_arguments()?)
                } else {
                    None
                };
                self.expect_close_angle("'>' after a qualified path's trait")?;
                self.expect(Token::DoubleColon, "'::' after a qualified path")?;
                if !through_crate
                    && !self.ufcs_trait(&trait_name)
                    && let Some(std_name) = crate::resolve::std_trait::anywhere(&trait_name)
                        .and_then(|definition| definition.rsplit("::").next())
                {
                    return self.parse_typed_std_trait_call(
                        std_name,
                        Some(self_ty),
                        explicit,
                        position,
                    );
                }
                self.parse_trait_path_call(
                    Some(self_ty),
                    trait_name,
                    trait_span,
                    explicit,
                    position,
                )
            }
            Token::Reserved(keyword)
                if keyword.as_str() == "super" && matches!(self.peek(), Token::DoubleColon) =>
            {
                self.parse_qualified("super".into(), token_span)
            }
            // `self::m::f()`: a path from this module, never the receiver.
            Token::SelfValue if matches!(self.peek(), Token::DoubleColon) => {
                self.parse_qualified("self".into(), token_span)
            }
            Token::SelfValue => {
                // A free function's `&mut` parameter is `this` too.
                let Some(binding) = self
                    .bindings
                    .get("self")
                    .filter(|binding| matches!(binding.kind, BindingKind::Receiver { .. }))
                else {
                    return Err(Error::SelfOutsideImpl {
                        reference: token_span,
                    });
                };
                let ty = binding.ty.clone();
                // A closure reads `self` through the local its outermost
                // closure binds (`parse_closure`): the runtime binds `this`
                // only in the method's own body.
                self.record_capture(&"self".into());
                let source = if self.closures.is_empty() {
                    "self"
                } else {
                    SELF_CAPTURE
                };
                Ok(ParsedExpr {
                    prelude: origin::Fragment::default(),
                    source: origin::Fragment::plain(source),
                    ty,
                    span: token_span,
                    lvalue: None,
                    outcome: crate::host::usage::Outcome::Conservative,
                    evaluation: crate::host::usage::Evaluation::Required,
                }
                .retain_origin())
            }
            // `try { .. } catch ..`: rustc reads a `try` block, which no
            // `catch` may follow.
            Token::Try if let Some(at) = self.catch_after_block() => Err(Error::Syntax {
                message: "keyword `catch` cannot follow a `try` block".into(),
                label: "".into(),
                help: Some("try using `match` on the result of the `try` block instead".into()),
                at: Span::new(at, text_end(at, "catch")),
            }),
            // `throw`, `do`, `#{ .. }` and the like: rustc's syntax error.
            token => {
                let found = crate::parser::item::found(&token);
                let written = found
                    .trim_start_matches("reserved ")
                    .trim_start_matches("keyword ")
                    .trim_matches('`');
                Err(Error::Syntax {
                    message: format!("expected expression, found {found}").into(),
                    label: "expected expression".into(),
                    help: None,
                    at: Span::new(token_span.start(), text_end(token_span.start(), written)),
                })
            }
        }
    }

    /// `{ statements; tail }`: a block whose bindings end with it and whose
    /// value is its tail, as in Rust.
    #[inline(never)]
    fn parse_block_expr(&mut self) -> Result<ParsedExpr, Error> {
        let original = self.bindings.clone();
        let outer = std::mem::replace(&mut self.no_struct, false);
        let block = self.parse_match_action();
        self.no_struct = outer;
        let inner = std::mem::replace(&mut self.bindings, original);
        for (name, binding) in &mut self.bindings {
            // A binding the block shadowed keeps its own state.
            if let Some(after) = inner.get(name)
                && after.declaration == binding.declaration
            {
                binding.available = after.available;
            }
        }
        let block = block?;
        let source = block.value.rendered_block();
        Ok(ParsedExpr {
            prelude: origin::Fragment::default(),
            source,
            ty: block.value.ty,
            span: block.syntax,
            lvalue: None,
            outcome: crate::host::usage::Outcome::Conservative,
            evaluation: crate::host::usage::Evaluation::Required,
        }
        .retain_origin())
    }

    /// `(value)`, or a tuple `(a, b, ..)`, which is an array at runtime.
    fn parse_parenthesized(&mut self, open: Position) -> Result<ParsedExpr, Error> {
        // A tuple keeps its form when a `#[cfg(..)]` leaves elements out.
        let mut left_out = false;
        while self.element_left_out()? {
            left_out = true;
        }
        let mut value = self.parse_expr()?;
        if left_out && !matches!(self.peek(), Token::Comma) {
            let end = self.expect(Token::RightParen, "')' after a tuple")?;
            self.move_local(&mut value)?;
            return Ok(self.tuple_of(open, end, vec![value]));
        }
        if !matches!(self.peek(), Token::Comma) {
            let end = self.expect(Token::RightParen, "')' after expression")?;
            let before = value.source.as_str().to_string();
            value.source = value.source.surrounded("(", ")");
            self.flow().same_origins(span(open, end), value.span);
            if let Some(unmovable) = self.unmovable.get(&value.span).cloned() {
                self.unmovable.insert(span(open, end), unmovable);
            }
            value.span = span(open, end);
            value.lvalue = None;
            value.source.cover(value.span);
            self.keep_pipeline(&before, &value);
            return Ok(value);
        }
        let mut members = vec![value];
        while matches!(self.peek(), Token::Comma) {
            self.next();
            while self.element_left_out()? {}
            if matches!(self.peek(), Token::RightParen) {
                break;
            }
            members.push(self.parse_expr()?);
        }
        let end = self.expect(Token::RightParen, "')' after a tuple")?;
        for member in &mut members {
            self.move_local(member)?;
        }
        Ok(self.tuple_of(open, end, members))
    }

    /// The tuple of `members`, which have moved into it, written from
    /// `open` to `end`.
    fn tuple_of(&mut self, open: Position, end: Position, members: Vec<ParsedExpr>) -> ParsedExpr {
        let ty = Ty::Tuple(
            members
                .iter()
                .map(|member| self.inference.resolve(&member.ty))
                .collect(),
        );
        let parts: Vec<_> = members
            .iter()
            .enumerate()
            .map(|(index, member)| {
                (
                    member,
                    borrows::Slot::At(borrowck::Path::of(&[borrowck::Step::Arg(index as u8)])),
                )
            })
            .collect();
        self.compose(span(open, end), &ty, &parts);
        self.tuple_fields.insert(
            span(open, end),
            members.iter().map(|member| member.span).collect(),
        );
        let source = origin::Fragment::join(members.iter().map(ParsedExpr::rendered_source), ", ")
            .surrounded("[", "]");
        self.operator_expr(source, ty, span(open, end))
    }

    fn parse_name_primary(
        &mut self,
        name: ImmutableString,
        name_span: Span,
    ) -> Result<ParsedExpr, Error> {
        if name.starts_with(SENTINEL)
            || name.starts_with(METHOD_SENTINEL)
            || name.starts_with(NOMINAL_MATCH_SENTINEL)
        {
            return Err(Error::UnsupportedNominalSyntax {
                construct: "compiler-private nominal intrinsic".into(),
                source: name_span,
            });
        }
        if crate::std_table::is_root(&name) && matches!(self.peek(), Token::DoubleColon) {
            self.check_std_path(&name, name_span)?;
        }
        // `S::Error::custom(..)` or `S::f(..)` of a type parameter native
        // code fills: the function of its opaque type (#593).
        #[cfg(feature = "rust-import")]
        if matches!(self.peek(), Token::DoubleColon)
            && !self.bindings.contains_key(name.as_str())
            && let Some(path) = self.opaque.get(&name).cloned()
        {
            self.next();
            let (first, first_span) = self.expect_qualified_name()?;
            let projected = rawr_import::surface::opaque::parameter_projection(&path, &first)
                .filter(|projected| self.models.host.source_type(projected).is_some());
            let (owner, member, member_span) = match projected {
                Some(projected) if matches!(self.peek(), Token::DoubleColon) => {
                    self.next();
                    let (member, member_span) = self.expect_qualified_name()?;
                    (projected, member, member_span)
                }
                _ => (path.to_string(), first, first_span),
            };
            return self.opaque_function_call(&owner, &member, member_span);
        }
        if matches!(self.peek(), Token::DoubleColon) && !self.bindings.contains_key(name.as_str()) {
            self.unbound_std_type(&name, true, name_span)?;
        }

        // `Type::<A>::member`: the path without its arguments, its value
        // of the type `Type<A>` when it is one (a constructor).
        if matches!(self.peek(), Token::DoubleColon)
            && let Some(self_ty) = self.take_path_arguments(&name, name_span)?
        {
            let value = self.parse_name_primary(name, name_span)?;
            let same_head = match (&self.inference.resolve(&value.ty), &self_ty) {
                (Ty::Script(value, _), Ty::Script(written, _)) => value == written,
                (Ty::Keyed(value, _), Ty::Keyed(written, _)) => value == written,
                // `Task::<Message>::none()`: a facade type's arguments.
                (Ty::Host { proof: value, .. }, Ty::Host { proof: written, .. }) => {
                    value.definition.path() == written.definition.path()
                }
                (Ty::Array { .. }, Ty::Array { .. })
                | (Ty::Option(_), Ty::Option(_))
                | (Ty::Result(..), Ty::Result(..))
                | (Ty::Boxed(..), Ty::Boxed(..))
                | (Ty::Counted(..), Ty::Counted(..))
                | (Ty::Weak(..), Ty::Weak(..)) => true,
                _ => false,
            };
            if same_head {
                self.require_type(&self_ty, &value.ty, value.span)?;
            }
            return Ok(value);
        }

        // The program's own macro: textual scope shadows the prelude's.
        if matches!(self.peek(), Token::Bang)
            && let Some(rules) = self.program_macro(&name, name_span.start())
        {
            let rules = rules?;
            return self.expand_program_macro(&rules, name_span);
        }
        if matches!(self.peek(), Token::Bang)
            && name.as_str() == "format"
            && self.models.import(&name).is_none()
        {
            self.next();
            return self.parse_format(name_span, false);
        }
        // `print!`, `println!`, `eprint!`, `eprintln!`.
        if matches!(self.peek(), Token::Bang)
            && let Some(stream) = match name.as_str() {
                "print" => Some((false, false)),
                "println" => Some((false, true)),
                "eprint" => Some((true, false)),
                "eprintln" => Some((true, true)),
                _ => None,
            }
            && self.models.import(&name).is_none()
        {
            return self.parse_print(stream, name_span);
        }
        if matches!(self.peek(), Token::Bang)
            && name.as_str() == "dbg"
            && self.models.import(&name).is_none()
        {
            return self.parse_dbg(name_span);
        }
        // `write!(s, ..)` and `writeln!(&mut s, ..)` into a `String`, through
        // `std::fmt::Write`: the text pushed onto the place, and `Ok(())`.
        if matches!(self.peek(), Token::Bang)
            && matches!(name.as_str(), "write" | "writeln")
            && self.models.import(&name).is_none()
            && !self.writes_into_formatter()
        {
            return self.parse_write_string(&name, name_span);
        }
        // `write!(f, ..)` and `writeln!(f, ..)` into a `fmt` body's formatter.
        if matches!(self.peek(), Token::Bang)
            && matches!(name.as_str(), "write" | "writeln")
            && let Some(formatter) = self.formatter.clone()
        {
            self.next();
            self.expect(Token::LeftParen, "'(' after `write!`")?;
            let (target, target_span) = self.expect_name("the formatter of `write!`")?;
            if target != formatter {
                return Err(Error::UnsupportedNominalSyntax {
                    construct: "`write!` into anything but the `fmt` formatter".into(),
                    source: target_span,
                });
            }
            let newline = if name.as_str() == "writeln" {
                " + \"\\n\""
            } else {
                ""
            };
            let (text, end) = if matches!(self.peek(), Token::Comma) {
                self.next();
                let text = self.parse_format(name_span, true)?;
                let end = text.span.end();
                (text.rendered_source(), end)
            } else {
                let end = self.expect(Token::RightParen, "')' after `writeln!`")?;
                (origin::Fragment::plain("\"\""), end)
            };
            let expression = span(name_span.start(), end);
            let mut source = origin::Fragment::plain(format!("{{ {formatter} += ("));
            source.push(text);
            source.push_text(format!("){newline}; "));
            let ok = self.prelude_op(PreludeOp::Ok, ["()".into()]);
            source.push(ok);
            source.push_text(" }");
            return Ok(self.operator_expr(
                source,
                Ty::Result(Box::new(Ty::Unit), Box::new(Ty::Unit)),
                expression,
            ));
        }
        if matches!(self.peek(), Token::Bang)
            && PANIC_MACROS.contains(&name.as_str())
            && self.models.import(&name).is_none()
        {
            self.next();
            return self.parse_panic_macro(&name, name_span);
        }
        if matches!(self.peek(), Token::Bang)
            && name.as_str() == "vec"
            && self.models.import(&name).is_none()
        {
            self.next();
            let (open, close) = match self.next() {
                (Token::LeftBracket, position) => (position, Token::RightBracket),
                (Token::LeftParen, position) => (position, Token::RightParen),
                (_, position) => {
                    return Err(Error::UnsupportedNominalSyntax {
                        construct: "`vec!` without `[` or `(`".into(),
                        source: point_span(position),
                    })
                }
            };
            return self.parse_array(open, close, None);
        }
        if matches!(self.peek(), Token::Bang)
            && matches!(
                name.as_str(),
                "concat"
                    | "env"
                    | "option_env"
                    | "include_str"
                    | "include_bytes"
                    | "stringify"
                    | "line"
                    | "column"
                    | "file"
                    | "module_path"
            )
            && self.models.import(&name).is_none()
        {
            self.next();
            return self.parse_compile_time_macro(&name, name_span);
        }
        if matches!(self.peek(), Token::Bang)
            && name.as_str() == "cfg"
            && self.models.import(&name).is_none()
        {
            self.next();
            let open = self.expect(Token::LeftParen, "`(` after `cfg!`")?;
            let value = self.cfg_predicate()?;
            let close = self.expect(Token::RightParen, "`)` to close `cfg!`")?;
            let _ = open;
            return Ok(self.literal(
                if value { "true" } else { "false" },
                Ty::Bool,
                span(name_span.start(), close),
            ));
        }
        // `join!`, `try_join!` and `select!` after `use tokio::join;`.
        if matches!(self.peek(), Token::Bang)
            && let Some(binding) = self.models.import(&name)
            && let Some(name) = crate::resolve::tokio_macro(&binding.qualified, &self.models.host)
        {
            return self.parse_tokio_macro(name, name_span);
        }
        if matches!(self.peek(), Token::Bang)
            && name.as_str() == "matches"
            && self.models.import(&name).is_none()
        {
            self.next();
            return self.parse_matches(name_span);
        }
        // `RefCell::new(value)` and `RefCell::default()`, and `Cell`'s.
        if let Some((skip, kind)) = self.cell_path(&name) {
            for _ in 0..skip {
                self.next();
            }
            return self.parse_cell_constructor(name_span, kind);
        }
        // `PhantomData`, also `PhantomData::<T>` or in full: a value that is
        // nothing at run time.
        if let Some(skip) = self.phantom_path(&name) {
            for _ in 0..skip {
                self.next();
            }
            let mut end = name_span.end();
            if matches!(self.peek(), Token::DoubleColon)
                && matches!(self.peek_second(), Token::LessThan)
            {
                self.next();
                self.parse_phantom_type()?;
                end = self.position();
            }
            return Ok(self.literal("()", Ty::Unit, span(name_span.start(), end)));
        }
        // `spawn(f)` and `channel()` imported from `std::thread` and
        // `std::sync::mpsc`, unless a local or the program names them.
        if !self.bindings.contains_key(name.as_str())
            && self.models.function(&name).is_none()
            && matches!(
                self.peek(),
                Token::LeftParen | Token::Unit | Token::DoubleColon
            )
            && let Some(binding) = self.models.import(&name)
        {
            match binding.qualified.as_str() {
                "std::thread::spawn" => return self.parse_thread_spawn(name_span.start()),
                "std::sync::mpsc::channel" => return self.parse_channel(name_span.start()),
                _ => {}
            }
        }
        // `std::thread::sleep(duration)`, `thread::sleep`, `sleep`.
        if let Some(skip) = self.sleep_path(&name) {
            for _ in 0..skip {
                self.next();
            }
            let (mut arguments, end) = self.parse_call_arguments("`sleep` arguments")?;
            let call = span(name_span.start(), end);
            let [duration] = arguments.as_mut_slice() else {
                return Err(Error::argument_count(
                    Callee::Function,
                    crate::resolve::Qualified("std::thread", "sleep").to_string(),
                    1,
                    arguments.len(),
                    call,
                ));
            };
            self.coerce_expr(&Ty::Time(Time::Duration), duration)?;
            let code = time_code("sleep").to_string();
            let source = self.prelude_op(
                PreludeOp::Time,
                ["()".into(), code.into(), duration.rendered_source()],
            );
            return Ok(self.operator_expr(source, Ty::Unit, call));
        }
        // `std::mem::swap(&mut a, &mut b)`, `mem::replace`, `take`.
        if let Some((function, skip)) = self.mem_path(&name) {
            for _ in 0..skip {
                self.next();
            }
            if function.starts_with("size_of") {
                return self.parse_size_of(function, name_span);
            }
            if function == "drop" {
                return self.parse_script_call("drop".into(), None, name_span);
            }
            return self.parse_mem_call(function, name_span);
        }
        // `Reverse(x)`, `std::cmp::Reverse(x)`.
        if self.reverse_path(&name) {
            return self.parse_reverse(name_span);
        }
        // `HashMap::new()`, `std::collections::BTreeSet::from(..)`.
        if let Some(kind) = self.keyed_path(&name) {
            return self.parse_keyed_constructor(kind, name_span);
        }
        if let Some(kind) = self.growable_path(&name) {
            self.next();
            let (member, member_span) = self.expect_member_name()?;
            // `Vec::len`, `Vec::is_empty(&v)`.
            let owner = Ty::sequence(kind, self.inference.fresh());
            let written = match kind {
                sequence::Kind::Deque => "VecDeque",
                _ => "Vec",
            };
            if let Some(value) =
                self.std_method_path(owner, written, &member, name_span, member_span)
            {
                return value;
            }
            let (arguments, end) = self.parse_call_arguments(&format!("`{name}` arguments"))?;
            // `Vec<T>` from `[T; N]`, or `VecDeque<T>` from a `Vec<T>` or
            // `[T; N]`, as std's `From` impls say: the same array at
            // runtime.
            if member.as_str() == "from"
                && let [value] = &arguments[..]
            {
                let call = span(name_span.start(), end);
                let ty = Ty::sequence(kind, self.inference.fresh());
                if self.std_conversion(&ty, &value.ty).is_none() {
                    let source = self.inference.resolve(&value.ty);
                    if let Some(error) = self.conversion_not_yet(&ty, &source, call) {
                        return Err(error);
                    }
                    return Err(Error::MissingCapability {
                        owner: format!("{name}<T>").into(),
                        capability: format!("From<{}>", source.name(&self.models)).into(),
                        operation: call,
                        derive: false,
                        required_by: None,
                    });
                }
                let ty = self.inference.resolve(&ty);
                return Ok(self.operator_expr(value.rendered_source(), ty, call));
            }
            let mut arguments = arguments;
            // `Vec::with_capacity(n)`: an empty `Vec` with room for `n`.
            if member.as_str() == "with_capacity"
                && let [capacity] = arguments.as_mut_slice()
            {
                self.coerce_expr(&Ty::Int(Int::Usize), capacity)?;
                let helper = self.push_op(Op::Prelude(PreludeOp::VecMethod));
                let code = vec_method_code("with_capacity");
                let mut source = origin::Fragment::plain(format!("{helper}([], {code}, "));
                source.push(capacity.rendered_source());
                source.push_text(", (), false)");
                let ty = Ty::sequence(kind, self.inference.fresh());
                let at = span(name_span.start(), end);
                #[cfg(feature = "rust-import")]
                let source =
                    self.sequence_source(&ty, at, capacity.rendered_source(), "()".into(), 0);
                return Ok(self.operator_expr(source, ty, at));
            } else if member.as_str() != "new" || !arguments.is_empty() {
                return Err(Error::UnknownMember {
                    owner: name,
                    member,
                    reference: member_span,
                });
            }
            let ty = Ty::sequence(kind, self.inference.fresh());
            let at = span(name_span.start(), end);
            #[cfg(feature = "rust-import")]
            let source = self.sequence_source(&ty, at, "0".into(), "()".into(), 0);
            #[cfg(not(feature = "rust-import"))]
            let source = origin::Fragment::plain("[]");
            return Ok(self.operator_expr(source, ty, at));
        }
        // `Rc::strong_count(&rc)`, `Arc::new(value)`, `Weak::new()`: an
        // `Rc`'s, `Arc`'s or `Weak`'s associated function (`counted.rs`).
        if matches!(self.peek(), Token::DoubleColon)
            && let Some((counter, weak)) = self
                .pointer_named(&name)
                .and_then(Pointer::counter)
                .map(|counter| (counter, false))
                .or_else(|| self.weak_named(&name).map(|counter| (counter, true)))
        {
            self.next();
            let (member, member_span) = self.expect_member_name()?;
            return self.parse_counted_path(counter, weak, member, name_span, member_span);
        }
        // `Box::new(value)`: the value itself at runtime.
        if matches!(self.peek(), Token::DoubleColon)
            && let Some(Pointer::Box) = self.pointer_named(&name)
        {
            self.next();
            let (member, member_span) = self.expect_member_name()?;
            let unknown = || Error::UnknownMember {
                owner: "Box".into(),
                member: member.clone(),
                reference: member_span,
            };
            // `.map(Box::new)`: the function as a value.
            if member.as_str() == "new" && !matches!(self.peek(), Token::LeftParen | Token::Unit) {
                let input = self.inference.fresh();
                let output = Ty::Boxed(Box::new(input.clone()));
                let parameter = self.temporary("value");
                let source = origin::Fragment::plain(format!("|{parameter}| {parameter}"));
                let ty = Ty::Fn {
                    inputs: Box::new([input]),
                    output: Box::new(output),
                };
                return Ok(self.operator_expr(
                    source,
                    ty,
                    span(name_span.start(), member_span.end()),
                ));
            }
            let (arguments, end) = self.parse_call_arguments("`Box` arguments")?;
            let [argument] = <[ParsedExpr; 1]>::try_from(arguments).map_err(|_| unknown())?;
            let call = span(name_span.start(), end);
            if member.as_str() != "new" {
                return Err(unknown());
            }
            let ty = Ty::Boxed(Box::new(self.inference.resolve(&argument.ty)));
            // A value with drop glue moves into its `Box`.
            let mut argument = argument;
            if self.needs_drop(&ty, call)? {
                self.move_local(&mut argument)?;
            }
            let source = argument.rendered_source();
            return Ok(self.operator_expr(source, ty, call));
        }

        if matches!(self.peek(), Token::Bang) {
            let Some(binding) = self.models.import(&name).cloned() else {
                let scope =
                    match self
                        .models
                        .macros
                        .lookup(&name, self.models.context, name_span.start())
                    {
                        crate::resolve::macros::Lookup::Later(definition) => {
                            crate::resolve::MacroScope::Later(definition)
                        }
                        crate::resolve::macros::Lookup::Elsewhere => {
                            crate::resolve::MacroScope::Elsewhere
                        }
                        crate::resolve::macros::Lookup::Found(_)
                        | crate::resolve::macros::Lookup::Missing => {
                            crate::resolve::MacroScope::Undefined
                        }
                    };
                return Err(Error::UnknownMacro {
                    reference: Span::new(name_span.start(), text_end(name_span.start(), &name)),
                    name,
                    scope,
                });
            };
            let (item::Import::Item {
                definitions: items,
                owner,
            }
            | item::Import::ModuleItem {
                definitions: items,
                owner,
            }) = binding.target
            else {
                return Err(Error::InvalidHostOperation {
                    item: binding.qualified,
                    reason: "it is not a macro".into(),
                    operation: name_span,
                });
            };
            let items = macro_or_value(items.into_vec(), true).into_boxed_slice();
            return self.parse_host_macro(items, owner, name_span);
        }
        self.parse_name_value(name, name_span)
    }

    /// `panic!`, `assert!`, `assert_eq!`, `assert_ne!`, their `debug_`
    /// forms, `unreachable!`, `todo!` and `unimplemented!` after the `!`,
    /// with Rust's messages. A panic throws its message.
    #[inline(never)]
    fn parse_panic_macro(&mut self, name: &str, name_span: Span) -> Result<ParsedExpr, Error> {
        let unit = matches!(self.peek(), Token::Unit);
        let (tokens, end) = self.take_argument_tokens()?;
        let call = span(name_span.start(), end);
        // The `)` that closes the arguments, which `()` starts before.
        let closing = if unit { text_end(end, "(") } else { end };
        let name = name.strip_prefix("debug_").unwrap_or(name);
        if let Some(operator) = match name {
            "assert_eq" => Some((Token::EqualsTo, "==")),
            "assert_ne" => Some((Token::NotEqualsTo, "!=")),
            _ => None,
        } {
            return self.parse_assert_compare(operator, tokens, name_span, closing);
        }
        if name == "assert" {
            // rustc's errors for a missing condition: none at all, or an
            // argument list that starts with its comma.
            if let Some((Token::Comma, comma)) = tokens.first() {
                return Err(Error::Syntax {
                    message: "expected expression, found `,`".into(),
                    label: "expected expression".into(),
                    help: None,
                    at: point_span(*comma),
                });
            }
            if tokens.is_empty() {
                return Err(Error::Syntax {
                    message: "macro requires a boolean expression as an argument".into(),
                    label: "boolean expression required".into(),
                    help: None,
                    at: span(name_span.start(), text_end(closing, ")")),
                });
            }
            let mut parts = split_arguments(tokens);
            let condition_tokens = parts.remove(0);
            let text = format!("assertion failed: {}", token_text(&condition_tokens));
            let mut spliced = condition_tokens;
            spliced.push((Token::RightParen, end));
            self.splice_tokens(spliced);
            let condition = self.parse_expr()?;
            self.expect(Token::RightParen, "')' after the condition of `assert!`")?;
            self.require_type(&Ty::Bool, &condition.ty, condition.span)?;
            let message = match self.panic_format(parts, name_span, end)? {
                Some(message) => message,
                None => origin::Fragment::plain(format!("{text:?}")),
            };
            let mut source = origin::Fragment::plain("if !(");
            source.push(condition.rendered_source());
            source.push_text(") { __rawr_throw ");
            source.push(message);
            source.push_text("; }");
            return Ok(self.operator_expr(source, Ty::Unit, call));
        }
        let prefix = match name {
            "todo" => Some("not yet implemented"),
            "unimplemented" => Some("not implemented"),
            "unreachable" => Some("internal error: entered unreachable code"),
            _ => None,
        };
        let message = match (
            self.panic_format(split_arguments(tokens), name_span, end)?,
            prefix,
        ) {
            (Some(message), None) => message,
            (Some(message), Some(prefix)) => {
                let mut text = origin::Fragment::plain(format!("{:?} + (", format!("{prefix}: ")));
                text.push(message);
                text.push_text(")");
                text
            }
            (None, prefix) => {
                origin::Fragment::plain(format!("{:?}", prefix.unwrap_or("explicit panic")))
            }
        };
        let mut source = origin::Fragment::plain("{ __rawr_throw ");
        source.push(message);
        source.push_text("; }");
        Ok(self.operator_expr(source, Ty::Never, call))
    }

    /// `dbg!(value)`: `[file:line:column] value = {value:#?}` on stderr, as
    /// std's `dbg!` writes it, and the value itself. `dbg!()` writes only
    /// where it is.
    fn parse_dbg(&mut self, name_span: Span) -> Result<ParsedExpr, Error> {
        self.next();
        let at = name_span.start();
        let file = match self.source_file() {
            Some(file) => cargo_env::file_path(&file),
            None => self.source_identity(),
        };
        let place = format!(
            "[{file}:{}:{}]",
            at.line().unwrap_or_default(),
            at.position().unwrap_or_default()
        );
        if matches!(self.peek(), Token::Unit) {
            let (_, end) = self.next();
            let text = origin::Fragment::plain(format!("{:?}", format!("{place}\n")));
            let source = self.prelude_op(PreludeOp::Print, [text, "true".into()]);
            return Ok(self.operator_expr(source, Ty::Unit, span(name_span.start(), end)));
        }
        self.expect(Token::LeftParen, "'(' after `dbg!`")?;
        let tokens = self.input.clone();
        let mut value = self.parse_expr()?;
        let stop = self.position();
        if matches!(self.peek(), Token::Comma) {
            self.next();
        }
        if !matches!(self.peek(), Token::RightParen) {
            return Err(self.unsupported("`dbg!` of more than one value"));
        }
        let end = self.expect(Token::RightParen, "')' after `dbg!`")?;
        let tokens = tokens
            .take_while(|(_, position)| *position != stop)
            .collect::<Vec<_>>();
        let key = self.models.sources.key(self.models.context.index());
        let written = self
            .models
            .sources
            .resolve(key)
            .ok()
            .and_then(|source| stringify_tokens(source.text(), &tokens, stop))
            .unwrap_or_else(|| {
                tokens
                    .iter()
                    .map(|(token, _)| token.to_string())
                    .collect::<Vec<_>>()
                    .join(" ")
            });
        self.move_local(&mut value)?;
        let held = self.temporary("dbg");
        let debug = self.debug_source(&value.ty, held.as_str().into(), value.span)?;
        let pretty = self.prelude_op(PreludeOp::PrettyDebug, [debug]);
        let mut text = origin::Fragment::plain(format!("{:?} + ", format!("{place} {written} = ")));
        text.push(pretty);
        text.push_text(" + \"\\n\"");
        let print = self.prelude_op(PreludeOp::Print, [text, "true".into()]);
        let mut source = origin::Fragment::plain(format!("{{ let {held} = "));
        source.push(value.rendered_source());
        source.push_text("; ");
        source.push(print);
        source.push_text(format!("; {held} }}"));
        Ok(self.operator_expr(source, value.ty, span(name_span.start(), end)))
    }

    /// The rest of [`Self::parse_name_primary`], kept out of its frame: a
    /// nested `column![..]` recurses through that frame once per level.
    #[inline(never)]
    fn parse_name_value(
        &mut self,
        name: ImmutableString,
        name_span: Span,
    ) -> Result<ParsedExpr, Error> {
        if name.as_str() == "move"
            && matches!(self.peek(), Token::Pipe | Token::Or)
            && !self.bindings.contains_key("move")
        {
            let token = self.next().0;
            return self.parse_closure(token, name_span, true);
        }

        // A local binding shadows an item of the same name, as in Rust.
        let local = self.bindings.contains_key(name.as_str())
            && !matches!(
                self.peek(),
                Token::DoubleColon | Token::LeftParen | Token::Unit
            );

        #[cfg(feature = "rust-import")]
        if let Some(binding) = self.models.import(&name).cloned().filter(|_| !local)
            && let item::Import::Type { definition, owner } = binding.target
            && let crate::host::type_def::Shape::Struct { form, .. } = definition.shape()
        {
            if matches!(form, crate::host::type_def::Form::Record)
                && matches!(self.peek(), Token::LeftBrace)
            {
                return self.parse_imported_struct(binding.qualified, definition, owner, name_span);
            }
            if let Some(value) = self.imported_struct_value(
                binding.qualified,
                definition,
                owner,
                &[(&name, name_span)],
                name_span,
            )? {
                return Ok(value);
            }
        }

        if matches!(self.peek(), Token::LeftBrace)
            && !self.no_struct
            && !self.bindings.contains_key(name.as_str())
        {
            return self.parse_struct(name, name_span);
        }
        // `S` of `struct S;`, `W(x)` of `struct W(T);`, `W` uncalled.
        if !self.bindings.contains_key(name.as_str())
            && !matches!(self.peek(), Token::DoubleColon)
            && let Some(model) = self.named_model(&name)
            && let Some(value) = self.parse_struct_value(model, name_span)?
        {
            return Ok(value);
        }

        // `String::new()`, `String::from(x)`, `i64::from(true)`, and a crate
        // trait's functions for a builtin type (`i64::make()`).
        if matches!(self.peek(), Token::DoubleColon)
            && let Some(builtin) = builtin_path_type(&name)
            && self.models.get(&name).is_none()
            && self.models.import(&name).is_none()
            && !self.bindings.contains_key(name.as_str())
        {
            return self.parse_builtin_path(builtin, name, name_span);
        }

        // The prelude's `Some(value)`, `None`, `Ok(value)` and `Err(error)`,
        // also as `Option::Some(value)` or `Result::Ok(value)`.
        let shadowed = |this: &Self, name: &str| {
            this.bindings.contains_key(name)
                || this.models.function(name).is_some()
                || this.models.get(name).is_some()
                || this.models.import(name).is_some()
        };
        // The prelude's `impl<T> From<T> for Option<T>`: `Option::from(x)`
        // is `Some(x)`.
        if name.as_str() == "Option"
            && matches!(self.peek(), Token::DoubleColon)
            && !shadowed(self, &name)
        {
            let mut ahead = self.input.clone();
            ahead.next();
            if let Some((Token::Identifier(member), _)) = ahead.next()
                && member.as_str() == "from"
            {
                self.next();
                let (_, member_span) = self.expect_member_name()?;
                let start = Span::new(name_span.start(), member_span.end());
                return self.parse_prelude_constructor(Prelude::Some, start);
            }
        }
        // `Cow::Borrowed(..)`, `std::borrow::Cow::Owned(..)`.
        if matches!(self.peek(), Token::DoubleColon)
            && !self.bindings.contains_key(name.as_str())
            && let Some((member, member_span)) = self.cow_member_ahead(&name)
        {
            let start = Span::new(name_span.start(), member_span.end());
            return self.parse_cow_associated(&member, start);
        }
        if matches!(name.as_str(), "Option" | "Result")
            && matches!(self.peek(), Token::DoubleColon)
            && !shadowed(self, &name)
        {
            let mut ahead = self.input.clone();
            ahead.next();
            if let Some((Token::Identifier(variant), _)) = ahead.next()
                && let Some(variant) = Prelude::from_name(&variant)
                && variant.owner() == name.as_str()
            {
                self.next();
                let (_, variant_span) = self.expect_member_name()?;
                let start = Span::new(name_span.start(), variant_span.end());
                return self.parse_prelude_constructor(variant, start);
            }
        }
        if let Some(variant) = Prelude::from_name(&name)
            && !shadowed(self, name.as_str())
        {
            return self.parse_prelude_constructor(variant, name_span);
        }

        // `first::<i64>(..)`: a generic function's type arguments.
        if matches!(self.peek(), Token::DoubleColon)
            && matches!(self.peek_second(), Token::LessThan)
            && !self.bindings.contains_key(name.as_str())
            && self
                .models
                .function(&name)
                .is_some_and(|function| !function.definition.generics.is_empty())
        {
            self.next();
            self.next();
            let mut types = vec![self.parse_local_type()?];
            while matches!(self.peek(), Token::Comma) {
                self.next();
                if matches!(self.peek(), Token::GreaterThan | Token::RightShift) {
                    break;
                }
                types.push(self.parse_local_type()?);
            }
            self.expect_close_angle("'>' after turbofish type arguments")?;
            self.pending_turbofish = Some(types);
        }
        // `T::default()` for a generic `T: Default`: the default of the type
        // its witness names.
        if matches!(self.peek(), Token::DoubleColon)
            && self.generics.contains_key(name.as_str())
            && !self.bindings.contains_key(name.as_str())
        {
            self.next();
            let param = Ty::Param(name.clone());
            // `T::name(&t)`: a method of a trait bounding `T`, called as
            // `<T as Trait>::name(&t)` is (T376).
            if let Some((Token::Identifier(member), position)) = self.input.clone().next()
                && let Some((trait_name, function)) =
                    self.generic_trait_function(&name, &member, point_span(position))?
                && function.receiver.is_some()
            {
                return self.parse_trait_path_call(
                    Some(param),
                    trait_name,
                    name_span,
                    None,
                    name_span.start(),
                );
            }
            let (member, member_span) = self.expect_member_name()?;
            // `T::new(..)`: a function without `self` of a trait bounding
            // `T`, run for the type its witness names.
            let function = self
                .generic_trait_function(&name, &member, member_span)?
                .filter(|(_, function)| function.receiver.is_none());
            // `T::SIDES`: an associated const is its value for `T`.
            let (arguments, end) = if function
                .as_ref()
                .is_some_and(|(_, function)| function.body.constant)
            {
                (Vec::new(), text_end(member_span.start(), &member))
            } else {
                self.parse_call_arguments("generic type function arguments")?
            };
            let call = span(name_span.start(), end);
            if let Some((trait_name, function)) = function {
                let witness = origin::Fragment::plain(witness_variable(&name));
                self.capture_witnesses();
                return self.call_static_trait_function(
                    param,
                    witness,
                    &trait_name,
                    &function,
                    arguments,
                    call,
                );
            }
            if member.as_str() != "default" || !arguments.is_empty() {
                return Err(Error::UnknownMember {
                    owner: name,
                    member,
                    reference: member_span,
                });
            }
            if !self.type_provides(&param, capability::Kind::Default) {
                return Err(Error::MissingCapability {
                    owner: name,
                    capability: "Default".into(),
                    operation: member_span,
                    derive: false,
                    required_by: None,
                });
            }
            self.capture_witnesses();
            let source = origin::Fragment::plain(format!(
                "{DEFAULT_DISPATCHER}({})",
                witness_variable(&name)
            ));
            return Ok(self.operator_expr(source, param, call));
        }
        // `Into::<String>::into(x)`, `From::<N>::from(x)`: a prelude
        // trait's argument written.
        if matches!(self.peek(), Token::DoubleColon)
            && self.peek_second() == Token::LessThan
            && let Some(std_name) = ["From", "Into"]
                .into_iter()
                .find(|std_name| *std_name == name.as_str())
            && !self.ufcs_trait(&name)
            && self.models.get(&name).is_none()
            && !self.bindings.contains_key(name.as_str())
        {
            self.next();
            let explicit = self.parse_trait_path_arguments()?;
            self.expect(Token::DoubleColon, "'::' after a trait's arguments")?;
            return self.parse_typed_std_trait_call(
                std_name,
                None,
                Some(explicit),
                name_span.start(),
            );
        }
        // `std::convert::Into::<String>::into(x)`: the same in full.
        if matches!(name.as_str(), "std" | "core")
            && matches!(self.peek(), Token::DoubleColon)
            && self.models.import(&name).is_none()
            && !self.bindings.contains_key(name.as_str())
        {
            let mut ahead = self.input.clone();
            let mut segments = Vec::new();
            while matches!(ahead.peek(), Some((Token::DoubleColon, _))) {
                ahead.next();
                let Some((Token::Identifier(segment), _)) = ahead.peek() else {
                    break;
                };
                segments.push(segment.to_string());
                ahead.next();
            }
            if let [module, std_name] = &segments[..]
                && module == "convert"
                && let Some(std_name) = ["From", "Into"].into_iter().find(|name| name == std_name)
                && matches!(ahead.peek(), Some((Token::LessThan, _)))
                && !self.ufcs_trait(std_name)
            {
                for _ in 0..5 {
                    self.next();
                }
                let explicit = self.parse_trait_path_arguments()?;
                self.expect(Token::DoubleColon, "'::' after a trait's arguments")?;
                return self.parse_typed_std_trait_call(
                    std_name,
                    None,
                    Some(explicit),
                    name_span.start(),
                );
            }
        }
        // `Duration::from_millis(5)`, `std::time::Instant::now()`.
        if matches!(self.peek(), Token::DoubleColon)
            && !self.bindings.contains_key(name.as_str())
            && let Some((time, member, skip, end)) = self.std_time_member(&name)
        {
            for _ in 0..skip {
                self.next();
            }
            return self.parse_time_function(time, member, Span::new(name_span.start(), end));
        }
        // `Ordering::Less`, `std::cmp::Ordering::Greater`: its discriminant.
        if matches!(self.peek(), Token::DoubleColon)
            && !self.bindings.contains_key(name.as_str())
            && let Some((value, skip, end)) = self.ordering_variant(&name)
        {
            for _ in 0..skip {
                self.next();
            }
            return Ok(self.literal(
                format!("({value})"),
                Ty::Ordering,
                Span::new(name_span.start(), end),
            ));
        }
        // `.map(Result::ok)`, `.and_then(Option::as_ref)`: a method of
        // `Option` or `Result` named as a value is the closure that calls it.
        // `str::trim`, `Option::is_some(&x)`.
        if matches!(name.as_str(), "Option" | "Result" | "str")
            && self.models.get(&name).is_none()
            && self.models.import(&name).is_none()
            && !self.bindings.contains_key(name.as_str())
            && let Some(member) = self.std_method_member()
        {
            let owner = match name.as_str() {
                "Option" => Ty::Option(Box::new(self.inference.fresh())),
                "Result" => Ty::Result(
                    Box::new(self.inference.fresh()),
                    Box::new(self.inference.fresh()),
                ),
                _ => Ty::Str,
            };
            if std_method::path_function(&owner, &member).is_some() {
                self.next();
                let (member, member_span) = self.expect_member_name()?;
                if let Some(value) =
                    self.std_method_path(owner, &name, &member, name_span, member_span)
                {
                    return value;
                }
            }
        }
        // `Default::default()`, `From::from(x)`, `std::convert::Into::into(x)`:
        // a std or prelude trait's function called by path.
        if matches!(self.peek(), Token::DoubleColon)
            && self.models.get(&name).is_none()
            && self.models.import(&name).is_none_or(|binding| {
                binding.qualified.starts_with("std::") || binding.qualified.starts_with("core::")
            })
            && !self.bindings.contains_key(name.as_str())
            && !self.generics.contains_key(name.as_str())
            && let Some((trait_name, member, skip)) = self.std_trait_path(&name)
        {
            for _ in 0..skip {
                self.next();
            }
            return self.parse_std_trait_call(trait_name, member, name_span.start());
        }
        // `Shape::area(&c)`, `Convert::<i64>::convert(&c)`: a crate trait's
        // function called by path.
        if matches!(self.peek(), Token::DoubleColon)
            && self.ufcs_trait(&name)
            && self.models.get(&name).is_none()
            && self.models.import(&name).is_none()
            && !self.bindings.contains_key(name.as_str())
        {
            self.next();
            let explicit = if matches!(self.peek(), Token::LessThan) {
                let arguments = self.parse_trait_path_arguments()?;
                self.expect(Token::DoubleColon, "'::' after a trait's arguments")?;
                Some(arguments)
            } else {
                None
            };
            return self.parse_trait_path_call(None, name, name_span, explicit, name_span.start());
        }
        if matches!(self.peek(), Token::DoubleColon) {
            return self.parse_qualified(name, name_span);
        }

        if matches!(self.peek(), Token::LeftParen | Token::Unit) {
            if self.bindings.contains_key(name.as_str()) {
                return self.parse_local_call(name, name_span);
            }
            // `min(a, b)` and `max(a, b)` imported from `std::cmp`.
            if let Some(binding) = self.models.import(&name)
                && let Some(member) = ["min", "max"].into_iter().find(|member| {
                    [
                        format!("std::cmp::{member}"),
                        format!("core::cmp::{member}"),
                    ]
                    .contains(&binding.qualified.to_string())
                })
            {
                return self.parse_std_trait_call("cmp", member, name_span.start());
            }
            if let Some(binding) = self.models.import(&name).cloned() {
                if let item::Import::Item {
                    definitions: items,
                    owner,
                }
                | item::Import::ModuleItem {
                    definitions: items,
                    owner,
                } = binding.target
                {
                    let items = value_or_call(macro_or_value(items.into_vec(), false), true)
                        .into_boxed_slice();
                    return self.parse_host_function(items, owner, name_span);
                }
                // `Degrees(90.0)`: a facade tuple struct's constructor, which
                // the facade declares as the type's `from`.
                if let item::Import::Type { definition, .. } = &binding.target
                    && definition.identity().imported().is_none()
                    && self
                        .models
                        .host_items(&format!("{}::from", definition.path()))
                        .is_some()
                {
                    let at = name_span.start();
                    self.splice_tokens(vec![
                        (Token::DoubleColon, at),
                        (Token::Identifier(Box::new("from".into())), at),
                    ]);
                    return self.parse_qualified(name, name_span);
                }
            }
            let function = self.models.function(&name).cloned();
            return self.parse_script_call(name, function, name_span);
        }

        if let Some(binding) = self.models.import(&name).cloned().filter(|_| !local)
            && let item::Import::Item {
                definitions: items,
                owner,
            }
            | item::Import::ModuleItem {
                definitions: items,
                owner,
            } = binding.target
        {
            // Not called, the name is in the value namespace, as rustc's:
            // `.map(text)` names the function a macro shares its name with.
            let items = value_or_call(macro_or_value(items.into_vec(), false), false);
            #[cfg(feature = "rust-import")]
            if items.len() == 1
                && items[0].imported_identity().is_some()
                && matches!(
                    items[0].kind(),
                    crate::host::item::Kind::Function | crate::host::item::Kind::AssociatedFunction
                )
            {
                // The name's own text, which the item's proof points at. A
                // std method Rawr runs takes it as the closure that calls it.
                let named = span(name_span.start(), text_end(name_span.start(), &name));
                return self.parse_host_function_or_item(items.into(), owner, named);
            }
            let [item] = items.as_slice() else {
                // `.map(text)`: an overloaded function as a value is the
                // closure `|a| text(a)`, whose call picks the overload.
                let arities = items
                    .iter()
                    .filter(|item| {
                        matches!(
                            item.kind(),
                            crate::host::item::Kind::Function
                                | crate::host::item::Kind::AssociatedFunction
                        )
                    })
                    .map(|item| item.signature().parameters().len())
                    .collect::<BTreeSet<_>>();
                if !matches!(self.peek(), Token::LeftParen | Token::Unit)
                    && arities.len() == 1
                    && let Some(&arity) = arities.first()
                    && arity > 0
                {
                    let at = name_span.start();
                    let parameter =
                        |index: usize| Token::Identifier(Box::new(format!("__a{index}").into()));
                    let mut tokens = Vec::new();
                    for index in 0..arity {
                        if index > 0 {
                            tokens.push((Token::Comma, at));
                        }
                        tokens.push((parameter(index), at));
                    }
                    tokens.push((Token::Pipe, at));
                    tokens.push((Token::Identifier(Box::new(name.as_str().into())), at));
                    tokens.push((Token::LeftParen, at));
                    for index in 0..arity {
                        if index > 0 {
                            tokens.push((Token::Comma, at));
                        }
                        tokens.push((parameter(index), at));
                    }
                    tokens.push((Token::RightParen, at));
                    self.splice_tokens(tokens);
                    return self.parse_closure(Token::Pipe, name_span, false);
                }
                return Err(Error::InvalidHostOperation {
                    reason: format!(
                        "`{}` has several forms here; call it with `(..)` to pick one",
                        binding.qualified
                    )
                    .into(),
                    item: binding.qualified,
                    operation: name_span,
                });
            };
            return self.parse_host_value(item.clone(), owner, name_span);
        }

        if let Some(function) = self.models.function(&name).cloned().filter(|_| !local) {
            if function.definition.body.constant {
                return Ok(self.constant_use(&function, name_span));
            }
            if this_parameter(&function.definition).is_some()
                || !copied_parameters(
                    &function.definition.parameters,
                    None,
                    function.definition.formatter.as_ref(),
                )
                .is_empty()
            {
                // A `&mut` parameter is copied in and back out of a call, so
                // the value is the closure `|a, ..| f(a, ..)` over the
                // function's parameter types, whose call does that.
                return self.mutable_function_value(&function, name, name_span);
            }
            return Ok(self.function_value(&function, name_span));
        }

        // `PI` after `use std::f64::consts::PI;`.
        if !self.bindings.contains_key(name.as_str())
            && let Some(binding) = self.models.import(&name)
            && let Some((value, single)) = crate::resolve::float_constant(&binding.qualified)
        {
            return Ok(self.float_constant(value, single, name_span));
        }
        self.record_capture(&name);
        let Some(binding) = self.bindings.get(name.as_str()) else {
            return Err(self.unknown_variable(&name, name_span));
        };
        let (declaration, ty) = (binding.declaration, binding.ty.clone());
        self.reject_shadowed_root(&name, declaration, name_span)?;
        self.mention(&name, declaration, &ty, name_span);
        if let Some(element) = self.aliased_element(&name, declaration, &ty, name_span)? {
            return Ok(element);
        }
        if let Some(held) = self
            .held_iter_mut
            .get(&name)
            .filter(|held| held.declaration == declaration)
            .cloned()
        {
            return self.step_iter_mut(&name, held, name_span);
        }
        // A local bound to a `RefCell` borrow: the cell's value. A closure
        // or async body reads its own borrows, not one it captures.
        if self
            .closures
            .last()
            .is_none_or(|scope| !scope.captures.contains(name.as_str()))
            && let Some(value) = self.cell_guard_use(&name, name_span)
        {
            return Ok(value);
        }
        let assigned = self.peek() == Token::Equals;
        let Some(binding) = self.bindings.get(name.as_str()) else {
            return Err(self.unknown_variable(&name, name_span));
        };
        // A local holding a map entry's `&mut`: the entry, read again.
        if let Some((_, tokens)) = self
            .entry_aliases
            .get(&name)
            .filter(|(declaration, _)| *declaration == binding.declaration)
            .cloned()
            && self.closures.is_empty()
        {
            let at = name_span.start();
            self.splice_tokens(tokens.into_iter().map(|(token, _)| (token, at)).collect());
            let mut value = self.parse_operand()?;
            value.span = span(name_span.start(), name_span.end());
            return Ok(value);
        }
        // A local iterator's chain, run where it is used; a closure using
        // it captures what its binding evaluated.
        if let Some(bound) = self
            .bound_iterators
            .get(&name)
            .filter(|bound| bound.declaration == binding.declaration)
            .cloned()
        {
            for scope in &mut self.closures {
                if scope.outer.get(&name) == Some(&bound.declaration) {
                    scope
                        .captures
                        .extend(bound.names.iter().map(|name| name.as_str().into()));
                }
            }
            let ty = Ty::iter(bound.item.clone());
            let mut value = self.iterator_expr(bound.pipeline, bound.item, name_span);
            value.lvalue = Some(Lvalue::Binding { binding: name, ty });
            return Ok(value);
        }
        // A moved local may be assigned again, as in Rust. A `let x;` used
        // before it has a value is rustc's E0381, which its borrow check
        // reports after the body's types.
        if !binding.available
            && !assigned
            && let Some(first) = self.deferred.get(&binding.declaration).copied()
            && !self.moved_at.contains_key(&binding.declaration)
        {
            let (declaration, ty) = (binding.declaration, binding.ty.clone());
            let unknown = matches!(self.inference.resolve(&ty), Ty::Infer(_));
            let operator = match self.peek() {
                Token::EqualsTo
                | Token::NotEqualsTo
                | Token::LessThan
                | Token::LessThanEqualsTo
                | Token::GreaterThan
                | Token::GreaterThanEqualsTo => Some("E0283"),
                Token::Plus | Token::Minus | Token::Multiply | Token::Divide | Token::Modulo => {
                    Some("E0284")
                }
                _ => None,
            }
            .filter(|_| unknown)
            .map(|code| (code, point_span(self.position())));
            self.uninitialized.push(UninitializedUse {
                name: name.clone(),
                declaration,
                reference: name_span,
                ty,
                possibly: first.is_some(),
                operator,
            });
        } else if !binding.available && !assigned {
            return Err(self.moved_value(&name, binding, name_span, false));
        }
        let Some(binding) = self.bindings.get(name.as_str()) else {
            return Err(self.unknown_variable(&name, name_span));
        };
        Ok(ParsedExpr {
            prelude: origin::Fragment::default(),
            source: origin::Fragment::plain(name.to_string()),
            ty: binding.ty.clone(),
            span: name_span,
            lvalue: Some(Lvalue::Binding {
                binding: name,
                ty: binding.ty.clone(),
            }),
            outcome: crate::host::usage::Outcome::Conservative,
            evaluation: crate::host::usage::Evaluation::Required,
        }
        .retain_origin())
    }

    /// A struct named as a value: `S` of `struct S;`, `W(a, b)` of a tuple
    /// struct, or `W` uncalled, the function from its fields to it, as
    /// Rust's tuple struct constructors are (`.map(W)`).
    fn parse_struct_value(
        &mut self,
        model: Nominal,
        expression: Span,
    ) -> Result<Option<ParsedExpr>, Error> {
        let Nominal::Struct { definition, item } = model else {
            return Ok(None);
        };
        match item.form {
            crate::item::StructForm::Unit if item.generics.is_empty() => {
                let ty = Ty::Script(definition.id(), Box::default());
                let helper = self.push_op(Op::Struct { definition, item });
                let source = origin::Fragment::call(&helper, []);
                return Ok(Some(self.operator_expr(source, ty, expression)));
            }
            crate::item::StructForm::Tuple => {}
            _ => return Ok(None),
        }
        let model = Nominal::Struct {
            definition: definition.clone(),
            item: item.clone(),
        };
        let (struct_ty, substitution) = self.fresh_nominal(&model, expression);
        let inputs: Box<[Ty]> = item
            .fields
            .iter()
            .map(|field| instantiate(&Ty::from_resolved(&field.ty), &substitution))
            .collect();
        let arity = inputs.len();
        let helper = self.push_op(Op::Struct {
            definition: definition.clone(),
            item,
        });
        if !matches!(self.peek(), Token::LeftParen | Token::Unit) {
            let parameters = (0..arity)
                .map(|index| format!("__rawr_argument_{index}"))
                .collect::<Vec<_>>()
                .join(", ");
            let source =
                origin::Fragment::plain(format!("(|{parameters}| {helper}({parameters}))"));
            let ty = Ty::Fn {
                inputs,
                output: Box::new(struct_ty),
            };
            return Ok(Some(self.operator_expr(source, ty, expression)));
        }
        let mut args = Vec::new();
        let end = if matches!(self.peek(), Token::Unit) {
            self.next().1
        } else {
            self.next();
            while !matches!(self.peek(), Token::RightParen) {
                args.push(self.parse_expr_expecting(inputs.get(args.len()))?);
                if matches!(self.peek(), Token::Comma) {
                    self.next();
                } else {
                    break;
                }
            }
            self.expect(Token::RightParen, "')' after tuple struct fields")?
        };
        if args.len() != arity {
            return Err(Error::argument_count(
                Callee::Struct,
                definition.name(),
                arity,
                args.len(),
                span(expression.start(), end),
            ));
        }
        for (value, expected) in args.iter_mut().zip(inputs.iter()) {
            self.coerce_expr(expected, value)?;
            self.move_local(value)?;
        }
        let parts: Vec<_> = args
            .iter()
            .enumerate()
            .map(|(slot, value)| (value, borrows::Slot::Field(definition.id(), None, slot)))
            .collect();
        self.compose(span(expression.start(), end), &struct_ty, &parts);
        let source = origin::Fragment::call(&helper, args.iter().map(ParsedExpr::rendered_source));
        Ok(Some(self.operator_expr(
            source,
            struct_ty,
            span(expression.start(), end),
        )))
    }

    /// The crate type `name` names, `Self` in an impl naming its owner.
    /// The type the path's one segment `owner` names when it is a generic
    /// parameter this body reads as a type: none of its own generics, nor
    /// a type of that name.
    fn parameter_type(&self, segments: &[(ImmutableString, Span)], owner: &str) -> Option<Ty> {
        if segments.len() != 1
            || self.generics.contains_key(owner)
            || self.models.get(owner).is_some()
        {
            return None;
        }
        self.trait_parameters
            .get(owner)
            .map(|ty| self.inference.resolve(ty))
    }

    fn named_model(&self, name: &str) -> Option<Nominal> {
        if let Some(model) = self.models.get(name) {
            return Some(model.clone());
        }
        match (name, &self.self_ty) {
            ("Self", Some(Ty::Script(id, _))) => Some(self.models.definition(*id).clone()),
            _ => None,
        }
    }

    /// The type of `S` when `name` is a unit struct `struct S;`.
    fn unit_struct(&self, name: &str) -> Option<Ty> {
        match self.models.get(name)? {
            model @ Nominal::Struct { item, .. }
                if item.form == crate::item::StructForm::Unit && item.generics.is_empty() =>
            {
                Some(model.ty())
            }
            _ => None,
        }
    }

    /// The Rust function a symbolic variant constructor (`Shape::Circle`,
    /// kept symbolic for the host) is when a script uses it as one.
    fn constructor_function(&mut self, ty: &Ty, expression: Span) -> Option<ParsedExpr> {
        let Ty::Callable {
            item: Some(item), ..
        } = ty
        else {
            return None;
        };
        if item.kind() != crate::item::Kind::VariantConstructor {
            return None;
        }
        let Some(Nominal::Enum {
            definition,
            item: declaration,
        }) = item
            .key()
            .owner()
            .and_then(|owner| self.models.by_identity.get(owner))
            .cloned()
        else {
            return None;
        };
        let variant = declaration
            .variants
            .iter()
            .position(|variant| variant.name.text.as_str() == item.key().name())?;
        Some(self.variant_function(&definition, &declaration, variant, expression))
    }

    /// `Some`, `Ok` or `Err` used as a value: the function from its payload.
    fn prelude_function(
        &mut self,
        variant: Prelude,
        ok: Ty,
        err: Ty,
        expression: Span,
    ) -> ParsedExpr {
        let (op, input, output) = match variant {
            Prelude::Some => (PreludeOp::Some, ok.clone(), Ty::Option(Box::new(ok))),
            Prelude::Ok => (
                PreludeOp::Ok,
                ok.clone(),
                Ty::Result(Box::new(ok), Box::new(err)),
            ),
            Prelude::Err => (
                PreludeOp::Err,
                err.clone(),
                Ty::Result(Box::new(ok), Box::new(err)),
            ),
            Prelude::None => unreachable!("`None` is a value, not a function"),
        };
        let call = self.prelude_op(op, [origin::Fragment::plain("__rawr_argument_0")]);
        let mut source = origin::Fragment::plain("(|__rawr_argument_0| ");
        source.push(call);
        source.push_text(")");
        self.operator_expr(
            source,
            Ty::Fn {
                inputs: Box::new([input]),
                output: Box::new(output),
            },
            expression,
        )
    }

    /// `::<A, B>` after a prelude constructor: its enum's type arguments.
    fn parse_prelude_turbofish(&mut self, variant: Prelude) -> Result<Option<Vec<Ty>>, Error> {
        if !(matches!(self.peek(), Token::DoubleColon)
            && matches!(self.peek_second(), Token::LessThan))
        {
            return Ok(None);
        }
        let start = self.next().1;
        self.next();
        let mut types = vec![self.parse_local_type()?];
        while matches!(self.peek(), Token::Comma) {
            self.next();
            if matches!(self.peek(), Token::GreaterThan | Token::RightShift) {
                break;
            }
            types.push(self.parse_local_type()?);
        }
        let end = self.expect_close_angle("'>' after turbofish type arguments")?;
        let expected = match variant {
            Prelude::Some | Prelude::None => 1,
            Prelude::Ok | Prelude::Err => 2,
        };
        if types.len() != expected {
            return Err(Error::InvalidTypeArguments {
                ty: variant.owner().into(),
                expected,
                actual: types.len(),
                reference: span(start, end),
            });
        }
        Ok(Some(types))
    }

    /// Where a `catch` follows the block that starts at the next token.
    fn catch_after_block(&self) -> Option<Position> {
        let mut ahead = self.input.clone();
        if !matches!(ahead.next(), Some((Token::LeftBrace, _))) {
            return None;
        }
        let mut depth = 1_usize;
        while depth > 0 {
            match ahead.next()? {
                (Token::LeftBrace | Token::MapStart, _) => depth += 1,
                (Token::RightBrace, _) => depth -= 1,
                (Token::EOF, _) => return None,
                _ => {}
            }
        }
        match ahead.next()? {
            (Token::Identifier(word), at) if word.as_str() == "catch" => Some(at),
            _ => None,
        }
    }

    /// Whether, from inside a literal's opening bracket, a method call,
    /// field, index or `?` follows its closing one.
    fn postfix_after_literal(&mut self) -> bool {
        let mut ahead = self.input.clone();
        let mut depth = 0_usize;
        while let Some((token, _)) = ahead.next() {
            match token {
                Token::LeftBracket | Token::LeftParen | Token::LeftBrace | Token::MapStart => {
                    depth += 1;
                }
                Token::RightBracket | Token::RightParen | Token::RightBrace if depth > 0 => {
                    depth -= 1;
                }
                Token::RightBracket | Token::RightParen => {
                    return matches!(ahead.next(), Some((Token::Period | Token::LeftBracket, _)));
                }
                Token::EOF => return false,
                _ => {}
            }
        }
        false
    }

    /// `[a, b]` or `[x; n]` (an array), or the same inside `vec!` (a `Vec`),
    /// after the opening bracket. Elements are evaluated in order.
    fn parse_array(
        &mut self,
        open: Position,
        close: Token,
        array: Option<usize>,
    ) -> Result<ParsedExpr, Error> {
        // The expected type is the literal's only when no postfix follows it
        // (`vec![Some(1)].into_iter().flatten().collect()` is not a `Vec<i64>`).
        let hint = self
            .array_hint
            .take()
            .filter(|_| !self.postfix_after_literal());
        self.frame_arrays.push(FrameArray {
            start: open,
            depth: self.frames.len(),
            noted: self.calls_noted,
            element: (self.calls_noted, 0, 0),
            bound: Vec::new(),
        });
        let parsed = self.parse_array_elements(&close, array);
        let bound = self
            .frame_arrays
            .pop()
            .map(|array| array.bound)
            .unwrap_or_default();
        let (mut elements, repeat, counted) = parsed?;
        self.bind_framed_elements(&mut elements, bound)?;
        let end = self.expect(close, "the end of an array")?;
        // Elements naming different functions of one signature meet at a
        // `fn` pointer, as `if` arms do.
        let types = elements
            .iter()
            .map(|element| self.inference.resolve(&element.ty))
            .collect::<Vec<_>>();
        let (element, skip) = match (hint, elements.first()) {
            (Some(hint), _) => (hint, 0),
            (None, Some(_)) if let Some(pointer) = self.fn_choice(&types) => (pointer, 0),
            (None, Some(first)) => (self.inference.resolve(&first.ty), 1),
            (None, None) => (self.inference.fresh(), 0),
        };
        for value in elements.iter_mut().skip(skip) {
            self.coerce_expr(&element, value)?;
        }
        for value in &mut elements {
            self.move_local(value)?;
        }
        let parts: Vec<_> = elements
            .iter()
            .map(|element| {
                (
                    element,
                    borrows::Slot::At(borrowck::Path::of(&[borrowck::Step::Arg(0)])),
                )
            })
            .collect();
        let array_ty = Ty::Array {
            element: Box::new(element.clone()),
            kind: sequence::Kind::Vec,
        };
        self.compose(span(open, end), &array_ty, &parts);
        let source = match (repeat, counted) {
            (Some(count), _) => {
                let value = elements.pop().expect("a repeat array has its value");
                let items = self.temporary("repeat");
                let mut source =
                    origin::Fragment::plain(format!("{{ let {items} = []; {items}.pad("));
                source.push_text(format!("{count}, "));
                source.push(value.rendered_source());
                source.push_text(format!("); {items} }}"));
                source
            }
            (None, Some(count)) => {
                // Rust evaluates the value, then the count.
                let value = elements.pop().expect("a repeat vector has its value");
                let (item, items) = (self.temporary("repeat"), self.temporary("repeat"));
                let mut source = origin::Fragment::plain(format!("{{ let {item} = "));
                source.push(value.rendered_source());
                source.push_text(format!("; let {items} = []; {items}.pad("));
                source.push(count.rendered_source());
                source.push_text(format!(", {item}); {items} }}"));
                #[cfg(feature = "rust-import")]
                let source = {
                    let constructed = self.sequence_source(
                        &array_ty,
                        span(open, end),
                        count.rendered_source(),
                        item.as_str().into(),
                        2,
                    );
                    let mut source = origin::Fragment::plain(format!("{{ let {item} = "));
                    source.push(value.rendered_source());
                    source.push_text("; ");
                    source.push(constructed);
                    source.push_text(" }");
                    source
                };
                source
            }
            (None, None) => {
                let source =
                    origin::Fragment::join(elements.iter().map(ParsedExpr::rendered_source), ", ")
                        .surrounded("[", "]");
                #[cfg(feature = "rust-import")]
                let source = if array.is_none() {
                    self.sequence_source(
                        &array_ty,
                        span(open, end),
                        elements.len().to_string().into(),
                        source,
                        1,
                    )
                } else {
                    source
                };
                source
            }
        };
        let kind = match array {
            Some(_) => sequence::Kind::Array(repeat.unwrap_or(elements.len())),
            None => sequence::Kind::Vec,
        };
        Ok(self.operator_expr(source, Ty::sequence(kind, element), span(open, end)))
    }

    /// An array literal's elements up to `close`, and its repeat count:
    /// a constant for an array, any `usize` for `vec![x; n]`.
    fn parse_array_elements(
        &mut self,
        close: &Token,
        array: Option<usize>,
    ) -> Result<ArrayElements, Error> {
        let mut elements = Vec::<ParsedExpr>::new();
        let mut repeat = None;
        let mut counted = None;
        while self.peek() != *close {
            if self.element_left_out()? {
                continue;
            }
            let frames = self.frames.last().map_or(0, Vec::len);
            let element = (self.calls_noted, elements.len(), frames);
            if let Some(array) = self.frame_arrays.last_mut() {
                array.element = element;
            }
            let outer = std::mem::replace(&mut self.no_struct, false);
            let element = self.parse_expr();
            self.no_struct = outer;
            elements.push(element?);
            if matches!(self.peek(), Token::SemiColon) && elements.len() == 1 {
                self.next();
                if array.is_some() {
                    repeat = Some(self.parse_array_length("repeat count")?);
                } else {
                    let outer = std::mem::replace(&mut self.no_struct, false);
                    let count = self.parse_expr();
                    self.no_struct = outer;
                    let mut count = count?;
                    self.coerce_expr(&Ty::Int(Int::Usize), &mut count)?;
                    counted = Some(count);
                }
                break;
            }
            if matches!(self.peek(), Token::Comma) {
                self.next();
            } else if self.peek() != *close {
                return Err(self.unsupported("array elements without ','"));
            }
        }
        Ok((elements, repeat, counted))
    }

    /// Bind the elements before each of `bound`'s, whose call opens a
    /// frame ahead of the statement, ahead of that frame, in order: `let
    /// element = icon().size(12);` before the frame of
    /// `Slot::from("Verified")`, so the elements run as Rust runs them.
    fn bind_framed_elements(
        &mut self,
        elements: &mut [ParsedExpr],
        bound: Vec<(usize, usize)>,
    ) -> Result<(), Error> {
        let mut done = 0;
        let mut added = 0;
        for (index, mark) in bound {
            let mut frames = Vec::new();
            for element in &mut elements[done..index] {
                // The element moves into its binding, as into the array.
                self.move_local(element)?;
                let name = self.temporary("element");
                let mut frame = Frame::default();
                frame.inner.push_text(format!("let {name} = "));
                frame.inner.push(element.rendered_source());
                frame.inner.push_text("; ");
                frames.push(frame);
                self.bindings.insert(
                    name.as_str().into(),
                    Binding {
                        ty: element.ty.clone(),
                        mutable: true,
                        available: true,
                        declaration: element.span,
                        kind: BindingKind::Local,
                    },
                );
                element.prelude = origin::Fragment::default();
                element.lvalue = Some(Lvalue::Binding {
                    binding: name.as_str().into(),
                    ty: element.ty.clone(),
                });
                element.source = origin::Fragment::plain(name);
            }
            done = done.max(index);
            let count = frames.len();
            if let Some(statement) = self.frames.last_mut() {
                let at = (mark + added).min(statement.len());
                statement.splice(at..at, frames);
            }
            added += count;
        }
        Ok(())
    }

    /// An array length or repeat count: an integer literal, or a `const`
    /// whose value is one (`[0; N]`, `[T; N]`), as rustc evaluates it.
    fn parse_array_length(&mut self, what: &str) -> Result<usize, Error> {
        let (token, position) = self.next();
        let value = match &token {
            Token::IntegerConstant(length) => Some(*length),
            // `Self::N`, `Grid::N`: an associated constant of an integer.
            Token::Identifier(owner) if matches!(self.peek(), Token::DoubleColon) => {
                self.next();
                let (name, _) = self.expect_name("an associated constant in an array length")?;
                self.named_model(owner.as_str())
                    .and_then(|model| self.models.method(model.definition().id(), &name))
                    .and_then(|definition| definition.integer())
            }
            Token::Identifier(name) => self
                .models
                .function(name.as_str())
                .filter(|function| function.definition.body.constant)
                .and_then(|function| function.definition.body.syntax.integer()),
            _ => None,
        };
        value
            .and_then(|value| usize::try_from(value).ok())
            .ok_or_else(|| Error::UnsupportedNominalSyntax {
                construct: format!("{what} other than an integer literal or a `const` of one")
                    .into(),
                source: point_span(position),
            })
    }

    /// One `Option<T>` runtime primitive applied to `operands`.
    fn prelude_op(
        &mut self,
        op: PreludeOp,
        operands: impl IntoIterator<Item = origin::Fragment>,
    ) -> origin::Fragment {
        let helper = self.push_op(Op::Prelude(op));
        origin::Fragment::call(&helper, operands)
    }

    /// `Some(value)`, `None`, `Ok(value)` or `Err(error)`, the prelude's,
    /// also as `Option::Some(value)` or `Result::Ok(value)`.
    fn parse_prelude_constructor(
        &mut self,
        variant: Prelude,
        start: Span,
    ) -> Result<ParsedExpr, Error> {
        let turbofish = self.parse_prelude_turbofish(variant)?;
        let inferred = turbofish.is_none();
        let (ok, err) = match turbofish {
            Some(types) => {
                let mut types = types.into_iter();
                let ok = types.next().expect("turbofish arity is checked");
                (ok, types.next().unwrap_or(Ty::Unit))
            }
            None => (self.inference.fresh(), self.inference.fresh()),
        };
        if variant == Prelude::None {
            // `None` holds no borrow.
            self.flow().set_origins(start, Vec::new());
            let source = self.prelude_op(PreludeOp::None, []);
            return Ok(self.operator_expr(source, Ty::Option(Box::new(ok)), start));
        }
        if !matches!(self.peek(), Token::LeftParen) {
            return Ok(self.prelude_function(variant, ok, err, start));
        }
        let (mut arguments, end) = self.parse_call_arguments("prelude variant arguments")?;
        let expression = span(start.start(), end);
        let [value] = arguments.as_mut_slice() else {
            return Err(Error::argument_count(
                Callee::Variant,
                variant.name(),
                1,
                arguments.len(),
                expression,
            ));
        };
        let stored = match variant {
            Prelude::Some | Prelude::None => "stored in an `Option`",
            Prelude::Ok | Prelude::Err => "stored in a `Result`",
        };
        self.reject_invocation(&value.ty, value.span, stored)?;
        // The payload is the turbofish's type, or the value's own.
        let expected = match variant {
            Prelude::Err => err.clone(),
            _ => ok.clone(),
        };
        if variant == Prelude::Err && inferred {
            self.box_returned_error(value)?;
        }
        if variant == Prelude::Err {
            self.err_payloads.insert(start.start(), value.span);
        }
        self.coerce_expr(&expected, value)?;
        self.move_local(value)?;
        let payload = Box::new(self.inference.resolve(&value.ty));
        let (op, ty) = match variant {
            Prelude::Some => (PreludeOp::Some, Ty::Option(payload)),
            Prelude::Ok => (PreludeOp::Ok, Ty::Result(payload, Box::new(err))),
            Prelude::Err => (PreludeOp::Err, Ty::Result(Box::new(ok), payload)),
            Prelude::None => unreachable!("`None` returned above"),
        };
        // `Some(&mut self.a)` returned as an `Option<&mut T>` output keeps
        // its place (`places::returned_place`).
        let place = match (&variant, self.place_output && self.closures.is_empty()) {
            (Prelude::Some, true)
                if matches!(
                    self.inference.resolve(&value.ty),
                    Ty::Reference { mutable: true, .. }
                ) =>
            {
                value.lvalue.clone().or_else(|| {
                    (value.source.as_str() == "self").then(|| Lvalue::Binding {
                        binding: "self".into(),
                        ty: value.ty.clone(),
                    })
                })
            }
            _ => None,
        };
        let at = match variant {
            Prelude::Err => borrowck::Step::Arg(1),
            _ => borrowck::Step::Arg(0),
        };
        self.compose(
            expression,
            &ty,
            &[(&*value, borrows::Slot::At(borrowck::Path::of(&[at])))],
        );
        let source = self.prelude_op(op, [value.rendered_source()]);
        let mut value = self.operator_expr(source, ty, expression);
        value.lvalue = place;
        Ok(value)
    }

    /// `value?` in a body returning `Option`: the `Some` value, or an early
    /// `return None`. In a body returning `Result<U, E>`, `value` is a
    /// `Result<T, E>`: the `Ok` value, or an early `return Err(error)`, the
    /// error converted by `impl From<E>` when `E` is not the body's error.
    fn parse_try(&mut self, value: ParsedExpr, end: Position) -> Result<ParsedExpr, Error> {
        let expression = span(value.span.start(), end);
        let (element, early, is, unwrap) = match self.inference.resolve(&value.ty) {
            Ty::Option(element) => (
                *element,
                Ty::Option(Box::new(self.inference.fresh())),
                PreludeOp::IsSome,
                PreludeOp::Unwrap,
            ),
            Ty::Result(ok, err) => (
                *ok,
                Ty::Result(Box::new(self.inference.fresh()), err),
                PreludeOp::IsOk,
                PreludeOp::UnwrapOk,
            ),
            actual => {
                return Err(Error::TypeMismatch {
                    expected: "Option<_> or Result<_, _>".into(),
                    actual: actual.name(&self.models),
                    expression: value.span,
                });
            }
        };
        let Some(output) = self.return_ty.clone() else {
            return Err(self.unsupported("`?` outside a function or closure"));
        };
        // `?` in a body returning `Result<_, Box<dyn Error>>` boxes an
        // error, a `&str` or a `String` as `dyn Error`, as its `From` impls do.
        if let (Ty::Result(_, error), Ty::Result(_, returned)) = (
            self.inference.resolve(&early),
            self.inference.resolve(&output),
        ) && !matches!(*error, Ty::Infer(_))
            && self.boxed_std_error(&returned)
            && !self.boxed_std_error(&error)
        {
            let ok = self.inference.fresh();
            self.unify_type(&output, &Ty::Result(Box::new(ok), returned));
            let operand = self.temporary("try");
            let mut source = origin::Fragment::plain(format!("{{ let {operand} = "));
            source.push(value.rendered_source());
            source.push_text("; if ");
            let test = self.prelude_op(is, [operand.as_str().into()]);
            source.push(test);
            source.push_text(" { ");
            let unwrapped = self.prelude_op(unwrap, [operand.as_str().into()]);
            source.push(unwrapped);
            source.push_text(" } else { return ");
            let unwrapped_error = self.prelude_op(PreludeOp::UnwrapErr, [operand.as_str().into()]);
            let Some(converted) = self.error_object(&error, unwrapped_error, true, value.span)?
            else {
                return Err(Error::MissingCapability {
                    owner: error.name(&self.models),
                    capability: "std::error::Error".into(),
                    operation: value.span,
                    derive: matches!(error.as_ref(), Ty::Script(..)),
                    required_by: None,
                });
            };
            let returned = self.prelude_op(PreludeOp::Err, [converted]);
            source.push(self.copied_return(returned.clone()).unwrap_or(returned));
            source.push_text("; } }");
            return Ok(self.operator_expr(source.surrounded("(", ")"), element, expression));
        }
        // `s.parse()?` there, whose error is known once the parse's target
        // is: boxed by the target's kind.
        if let (Ty::Result(_, error), Ty::Result(_, returned)) = (
            self.inference.resolve(&early),
            self.inference.resolve(&output),
        ) && matches!(*error, Ty::Infer(_))
            && self.boxed_std_error(&returned)
            && let Some(target) = self
                .pending_parses
                .iter()
                .find(|(_, pending, ..)| self.inference.resolve(pending) == *error)
                .map(|(target, ..)| target.clone())
        {
            let ok = self.inference.fresh();
            self.unify_type(&output, &Ty::Result(Box::new(ok), returned));
            let operand = self.temporary("try");
            let mut source = origin::Fragment::plain(format!("{{ let {operand} = "));
            source.push(value.rendered_source());
            source.push_text("; if ");
            let test = self.prelude_op(is, [operand.as_str().into()]);
            source.push(test);
            source.push_text(" { ");
            let unwrapped = self.prelude_op(unwrap, [operand.as_str().into()]);
            source.push(unwrapped);
            source.push_text(" } else { return ");
            let unwrapped_error = self.prelude_op(PreludeOp::UnwrapErr, [operand.as_str().into()]);
            let helper = self.push_op(Op::Kinded {
                op: PreludeOp::ErrorParse,
                kinds: [target.clone(), target],
            });
            let converted = origin::Fragment::call(&helper, [unwrapped_error]);
            let returned = self.prelude_op(PreludeOp::Err, [converted]);
            source.push(self.copied_return(returned.clone()).unwrap_or(returned));
            source.push_text("; } }");
            return Ok(self.operator_expr(source.surrounded("(", ")"), element, expression));
        }
        // `?` converts the error through `From`, as in Rust: `Err(e)?` in a
        // body returning `Result<_, F>` returns `Err(F::from(e))`.
        if let (Ty::Result(_, error), Ty::Result(_, returned)) = (
            self.inference.resolve(&early),
            self.inference.resolve(&output),
        ) && !matches!(*error, Ty::Infer(_))
            && let Some(target) = owner_id(&returned)
            && !self.unifies(&returned, &error)
            && let Some((conversion, ..)) =
                self.select_conversion(target, Some(&returned), &error, value.span)?
        {
            let ok = self.inference.fresh();
            self.unify_type(&output, &Ty::Result(Box::new(ok), returned));
            let mut value = value;
            self.move_out_value(&mut value)?;
            let drops = self.temporary_drops_here() + self.exit_drops(self.drop_base)?.as_str();
            let operand = self.temporary("try");
            let mut source = origin::Fragment::plain(format!("{{ let {operand} = "));
            source.push(value.rendered_source());
            source.push_text("; if ");
            let test = self.prelude_op(is, [operand.as_str().into()]);
            source.push(test);
            source.push_text(" { ");
            let unwrapped = self.prelude_op(unwrap, [operand.as_str().into()]);
            source.push(unwrapped);
            source.push_text(format!(" }} else {{ {drops}return "));
            let error = self.prelude_op(PreludeOp::UnwrapErr, [operand.as_str().into()]);
            let converted = origin::Fragment::call(method_name(conversion.id()), [error]);
            let returned = self.prelude_op(PreludeOp::Err, [converted]);
            source.push(self.copied_return(returned.clone()).unwrap_or(returned));
            source.push_text("; } }");
            return Ok(self.operator_expr(source.surrounded("(", ")"), element, expression));
        }
        // `?` into a host error converts through its `From`, as a crate's
        // `#[from] std::io::Error` writes it: `Err(e)?` returns
        // `Err(F::from(e))`.
        if let (Ty::Result(_, error), Ty::Result(_, returned)) = (
            self.inference.resolve(&early),
            self.inference.resolve(&output),
        ) && !matches!(*error, Ty::Infer(_))
            && matches!(*returned, Ty::Host { .. })
            && !self.unifies(&returned, &error)
        {
            let host = HostInto {
                forms: Vec::new(),
                owner: None,
                receiver: value.span,
            };
            let Some(helper) = self.host_into_helper(&host, &error, &returned, value.span)? else {
                // An imported error whose `from` the program never named:
                // its member, which the importer brings once it is demanded.
                #[cfg(feature = "rust-import")]
                if let Some(owner) = self.imported_public_path(&returned, "from") {
                    return Err(Error::UnknownMember {
                        owner,
                        member: "from".into(),
                        reference: value.span,
                    });
                }
                return Err(self.try_operator(&output, &early, expression));
            };
            let ok = self.inference.fresh();
            self.unify_type(&output, &Ty::Result(Box::new(ok), returned));
            let mut value = value;
            self.move_out_value(&mut value)?;
            let drops = self.temporary_drops_here() + self.exit_drops(self.drop_base)?.as_str();
            let operand = self.temporary("try");
            let mut source = origin::Fragment::plain(format!("{{ let {operand} = "));
            source.push(value.rendered_source());
            source.push_text("; if ");
            let test = self.prelude_op(is, [operand.as_str().into()]);
            source.push(test);
            source.push_text(" { ");
            let unwrapped = self.prelude_op(unwrap, [operand.as_str().into()]);
            source.push(unwrapped);
            source.push_text(format!(" }} else {{ {drops}return "));
            let error = self.prelude_op(PreludeOp::UnwrapErr, [operand.as_str().into()]);
            let converted = origin::Fragment::call(&helper, [error]);
            let returned = self.prelude_op(PreludeOp::Err, [converted]);
            source.push(self.copied_return(returned.clone()).unwrap_or(returned));
            source.push_text("; } }");
            return Ok(self.operator_expr(source.surrounded("(", ")"), element, expression));
        }
        if !self.unify_type(&output, &early) {
            return Err(self.try_operator(&output, &early, expression));
        }
        // The early return is the operand itself: `None`, or `Err(error)`.
        let mut value = value;
        self.move_out_value(&mut value)?;
        let drops = self.temporary_drops_here() + self.exit_drops(self.drop_base)?.as_str();
        let operand = self.temporary("try");
        let mut source = origin::Fragment::plain(format!("{{ let {operand} = "));
        source.push(value.rendered_source());
        source.push_text("; if ");
        let test = self.prelude_op(is, [operand.as_str().into()]);
        source.push(test);
        source.push_text(" { ");
        let unwrapped = self.prelude_op(unwrap, [operand.as_str().into()]);
        source.push(unwrapped);
        let returned = origin::Fragment::plain(operand.as_str());
        source.push_text(format!(" }} else {{ {drops}return "));
        source.push(self.copied_return(returned.clone()).unwrap_or(returned));
        source.push_text("; } }");
        let source = source.surrounded("(", ")");
        // `v.get_mut(i)?` is the element's place, as `unwrap()` keeps it.
        let place = match &value.lvalue {
            Some(lvalue)
                if !matches!(lvalue, Lvalue::Slice(_))
                    && matches!(
                        self.inference.resolve(&element),
                        Ty::Reference { mutable: true, .. }
                    ) =>
            {
                let mut nested = places::as_nested(lvalue);
                let mut setup = source.clone();
                setup.push_text(";");
                nested.steps.insert(0, PlaceStep::Setup(setup));
                Some(Lvalue::Nested(Box::new(nested)))
            }
            _ => None,
        };
        let mut value = self.operator_expr(source, element, expression);
        value.lvalue = place;
        Ok(value)
    }

    /// A `String` expression rendering `value` of type `ty` as Rust's
    /// `{:?}` does, when `ty` is `Debug`.
    fn debug_source(
        &mut self,
        ty: &Ty,
        value: origin::Fragment,
        span: Span,
    ) -> Result<origin::Fragment, Error> {
        let mut ty = self.inference.resolve(ty);
        // `let c: Result<Color, _> = v.try_into();`: the error type is the
        // impl's, which the target the annotation gave selects.
        if matches!(ty, Ty::Infer(_)) {
            self.settle_try_from_errors();
            ty = self.inference.resolve(&ty);
        }
        if self.std_error(&ty, false) {
            return Ok(self.error_text(value, true));
        }
        // A `Cow` writes what it derefs to.
        if let Ty::Cow(borrowed) = &ty {
            let payload = self.prelude_op(PreludeOp::CowValue, [value]);
            return self.debug_source(&cow::payload_ty(borrowed), payload, span);
        }
        if let Some(spec) = self.debug_spec.clone() {
            // A leaf under a `{:?}` spec: a number, `bool` or `()` takes it.
            let leaf = match &ty {
                // An integer literal's type defaults to `i32`.
                Ty::Int(Int::Var(_)) => Some(1032),
                Ty::Int(kind) => {
                    Some(i64::from(kind.bits()) + if kind.signed() { 1000 } else { 2000 })
                }
                Ty::Float(Float::F32) => Some(1),
                Ty::Float(_) => Some(2),
                Ty::Bool => Some(3),
                Ty::Unit => Some(4),
                _ => None,
            };
            if let Some(code) = leaf {
                return Ok(self.prelude_op(
                    PreludeOp::FormatDebugLeaf,
                    [value, spec.as_str().into(), code.to_string().into()],
                ));
            }
            match &ty {
                Ty::Script(id, ..) if self.models.std_impl(*id, "Debug", "fmt").is_none() => {
                    let model = self.models.definition(*id).clone();
                    self.require_capability(&model, capability::Kind::Debug, span)?;
                    return Ok(
                        self.prelude_op(PreludeOp::FormatSpecDebug, [value, spec.as_str().into()])
                    );
                }
                Ty::Script(..)
                | Ty::Param(_)
                | Ty::Projection(..)
                | Ty::Time(_)
                | Ty::Host { .. } => {
                    return Err(Error::UnsupportedNominalSyntax {
                        construct: format!(
                            "a width, precision, sign or `x?` in `{{:?}}` of `{}`, whose `Debug` \
                             is its own or generic",
                            ty.name(&self.models)
                        )
                        .into(),
                        source: span,
                    });
                }
                _ => {}
            }
        }
        Ok(match &ty {
            Ty::Reference { target, .. } | Ty::Boxed(target) => {
                self.debug_source(target, value, span)?
            }
            // An `Rc` or `Arc` writes its value's `Debug`, and a `Weak`
            // `(Weak)`, as std's do.
            Ty::Counted(_, target) => {
                let value = self.counted_call(PreludeOp::CountedGet, value, []);
                self.debug_source(target, value, span)?
            }
            Ty::Weak(..) => origin::Fragment::plain("\"(Weak)\""),

            // A generic value, an associated type's by its bounds, or a
            // trait object by its trait's.
            Ty::Param(_) | Ty::Projection(..) | Ty::Dyn(..) => {
                if !self.type_provides(&ty, capability::Kind::Debug) {
                    return Err(Error::MissingCapability {
                        owner: ty.name(&self.models),
                        capability: "Debug".into(),
                        operation: span,
                        derive: matches!(ty, Ty::Script(..)),
                        required_by: None,
                    });
                }
                if self.models.std_traits.contains("Debug") {
                    Self::std_dispatch("Debug", "fmt", value, &["\"\"".into()])
                } else {
                    self.prelude_op(PreludeOp::GenericDebug, [value])
                }
            }
            Ty::Unit => "\"()\"".into(),
            Ty::Int(Int::U64 | Int::Usize) => {
                self.prelude_op(PreludeOp::FormatRadix, [value, "10".into()])
            }
            Ty::Float(kind) => {
                let mode = if *kind == Float::F32 { "3" } else { "1" };
                self.prelude_op(PreludeOp::FormatFloat, [value, "-1".into(), mode.into()])
            }
            Ty::Bool | Ty::Int(_) => value.surrounded("(\"\" + ", ")"),
            // A `ParseIntError` and kin: its `Debug` by its message.
            Ty::ParseError(error) => {
                let message = self.temporary("debug");
                let mut source = origin::Fragment::plain(format!("{{ let {message} = "));
                source.push(value);
                source.push_text(format!("; __rawr_switch {message} {{ "));
                for (text, debug) in error.texts() {
                    source.push_text(format!("{text:?} => {debug:?}, "));
                }
                source.push_text(format!("_ => {message} }} }}"));
                source
            }
            // Rust's own `Debug` text of a `Duration` (`1.5s`) or `Instant`.
            Ty::Time(_) => {
                crate::host::debug::register::<std::time::Duration>();
                crate::host::debug::register::<crate::Instant>();
                self.prelude_op(PreludeOp::GenericDebug, [value])
            }
            Ty::Ordering => {
                let ordering = self.temporary("debug");
                let mut source = origin::Fragment::plain(format!("{{ let {ordering} = "));
                source.push(value);
                source.push_text(format!(
                    "; if {ordering} < 0 {{ \"Less\" }} else if {ordering} == 0 {{ \"Equal\" }} \
                     else {{ \"Greater\" }} }}"
                ));
                source
            }
            // Rust's `escape_debug`: quotes, `\n`, `\\`, `\u{..}` and kin.
            Ty::String | Ty::Str | Ty::Char => {
                let code = STR_PIECES
                    .iter()
                    .position(|name| *name == "debug")
                    .expect("a listed piece")
                    .to_string();
                self.prelude_op(
                    PreludeOp::StrPieces,
                    [value, code.into(), "()".into(), "()".into()],
                )
            }
            Ty::Option(element) => {
                let option = self.temporary("debug");
                let mut source = origin::Fragment::plain(format!("{{ let {option} = "));
                source.push(value);
                source.push_text("; if ");
                let test = self.prelude_op(PreludeOp::IsSome, [option.as_str().into()]);
                let inner = self.prelude_op(PreludeOp::Unwrap, [option.as_str().into()]);
                let inner = self.debug_source(element, inner, span)?;
                source.push(test);
                source.push_text(" { \"Some(\" + ");
                source.push(inner);
                source.push_text(" + \")\" } else { \"None\" } }");
                source
            }
            Ty::Result(ok, err) => {
                let result = self.temporary("debug");
                let mut source = origin::Fragment::plain(format!("{{ let {result} = "));
                source.push(value);
                source.push_text("; if ");
                let test = self.prelude_op(PreludeOp::IsOk, [result.as_str().into()]);
                let ok_value = self.prelude_op(PreludeOp::UnwrapOk, [result.as_str().into()]);
                let ok_value = self.debug_source(ok, ok_value, span)?;
                let err_value = self.prelude_op(PreludeOp::UnwrapErr, [result.as_str().into()]);
                let err_value = self.debug_source(err, err_value, span)?;
                source.push(test);
                source.push_text(" { \"Ok(\" + ");
                source.push(ok_value);
                source.push_text(" + \")\" } else { \"Err(\" + ");
                source.push(err_value);
                source.push_text(" + \")\" } }");
                source
            }
            Ty::Tuple(members) => {
                let tuple = self.temporary("debug");
                let mut source = origin::Fragment::plain(format!("{{ let {tuple} = "));
                source.push(value);
                source.push_text("; \"(\"");
                for (index, member) in members.iter().enumerate() {
                    let rendered =
                        self.debug_source(member, format!("{tuple}[{index}]").into(), span)?;
                    source.push_text(if index == 0 { " + " } else { " + \", \" + " });
                    source.push(rendered);
                }
                source.push_text(if members.len() == 1 {
                    " + \",)\" }"
                } else {
                    " + \")\" }"
                });
                source
            }
            Ty::Array { element, .. } => {
                let receiver = self.temporary("debug_values");
                let mut routed = origin::Fragment::plain(format!("{{ let {receiver} = "));
                routed.push(value);
                routed.push_text("; if ");
                routed.push(self.prelude_op(PreludeOp::NativePlace, [receiver.as_str().into()]));
                routed.push_text(" { ");
                routed.push(self.prelude_op(PreludeOp::GenericDebug, [receiver.as_str().into()]));
                routed.push_text(" } else ");
                let item = self.temporary("debug");
                let text = self.temporary("debug");
                let rendered = self.debug_source(element, item.as_str().into(), span)?;
                let mut source =
                    origin::Fragment::plain(format!("{{ let {text} = \"[\"; for {item} in "));
                source.push(receiver.as_str().into());
                source.push_text(format!(
                    " {{ if {text} != \"[\" {{ {text} += \", \"; }} {text} += "
                ));
                source.push(rendered);
                source.push_text(format!("; }} {text} + \"]\" }}"));
                routed.push(source);
                routed.push_text(" }");
                routed
            }
            Ty::Script(id, ..) if self.models.std_impl(*id, "Debug", "fmt").is_some() => {
                let definition = self
                    .models
                    .std_impl(*id, "Debug", "fmt")
                    .expect("guarded by the match arm");
                Self::std_call(definition, value, &["\"\"".into()])
            }
            Ty::Reverse(inner) => {
                let unwrapped = self.prelude_op(PreludeOp::ReverseInner, [value]);
                self.debug_source(inner, unwrapped, span)?
                    .surrounded("(\"Reverse(\" + ", " + \")\")")
            }
            // std's `Debug` of a `Mutex` no thread poisoned.
            Ty::Cell(CellKind::Mutex, inner) => {
                let unwrapped = self.prelude_op(PreludeOp::CellGet, [value]);
                self.debug_source(inner, unwrapped, span)?
                    .surrounded("(\"Mutex { data: \" + ", " + \", poisoned: false, .. }\")")
            }
            Ty::Cell(kind, inner) => {
                let unwrapped = self.prelude_op(PreludeOp::CellGet, [value]);
                let open = format!("(\"{} {{ value: \" + ", kind.name());
                self.debug_source(inner, unwrapped, span)?
                    .surrounded(&open, " + \" }\")")
            }
            // `{1: "a", 2: "b"}` and `{1, 2}`, in key order.
            Ty::Keyed(kind, arguments) => {
                let item = self.temporary("debug");
                let text = self.temporary("debug");
                let rendered = if kind.is_map() {
                    let mut pair =
                        self.debug_source(&arguments[0], format!("{item}[0]").into(), span)?;
                    pair.push_text(" + \": \" + ");
                    pair.push(self.debug_source(
                        &arguments[1],
                        format!("{item}[1]").into(),
                        span,
                    )?);
                    pair
                } else {
                    self.debug_source(&arguments[0], item.as_str().into(), span)?
                };
                let mut source =
                    origin::Fragment::plain(format!("{{ let {text} = \"{{\"; for {item} in "));
                source.push(value);
                source.push_text(format!(
                    " {{ if {text} != \"{{\" {{ {text} += \", \"; }} {text} += "
                ));
                source.push(rendered);
                source.push_text(format!("; }} {text} + \"}}\" }}"));
                source
            }
            // `Range<usize>` a crate gives (#707): its `Range<i64>` value,
            // which shows as Rust's `0..5` does, and an integer `a..=b` of
            // any kind, its `RangeInclusive<i64>`.
            Ty::Range(element) if matches!(**element, Ty::Int(_)) => {
                self.prelude_op(PreludeOp::GenericDebug, [value])
            }
            Ty::InclusiveRange(_) => self.prelude_op(PreludeOp::GenericDebug, [value]),
            // A facade type providing `Debug`: Rust's text, as its facade
            // registered it (`host::debug`).
            Ty::Host { proof, .. }
                if proof
                    .definition
                    .capabilities()
                    .contains(capability::Kind::Debug) =>
            {
                self.prelude_op(PreludeOp::GenericDebug, [value])
            }
            Ty::Script(id, ..) => {
                let model = self.models.definition(*id).clone();
                self.require_capability(&model, capability::Kind::Debug, span)?;
                let helper = self.push_op(Op::Debug {
                    definition: model.definition().clone(),
                });
                origin::Fragment::call(&helper, [value])
            }
            Ty::Thread(..) => self.thread_text(value, true),
            // A local's type nothing has given yet, as the element of a
            // `Vec::new()` only `.into()`s go in: rustc needs it (E0282).
            Ty::Infer(_) if let Some(error) = self.unsettled(&ty, span, None) => {
                return Err(error);
            }
            _ => {
                return Err(Error::MissingCapability {
                    owner: ty.name(&self.models),
                    capability: "Debug".into(),
                    operation: span,
                    derive: matches!(ty, Ty::Script(..)),
                    required_by: None,
                })
            }
        })
    }

    /// A use of `const NAME` evaluates its initializer, as Rust inlines it.
    fn constant_use(&mut self, function: &ScriptFunction, span: Span) -> ParsedExpr {
        let ty = function
            .definition
            .output
            .as_ref()
            .map_or(Ty::Unit, Ty::from_resolved);
        // A `const` or `static` of `&str` is `&'static str`, as a literal
        // is: it borrows nothing of the body.
        if ty == Ty::Str {
            self.flow().set_origins(span, Vec::new());
        }
        let source = origin::Fragment::call(function.generated.as_str(), []);
        self.operator_expr(source, ty, span)
    }

    fn parse_struct(
        &mut self,
        name: ImmutableString,
        name_span: Span,
    ) -> Result<ParsedExpr, Error> {
        if let Some(binding) = self.models.import(&name).cloned()
            && let item::Import::Type { definition, owner } = binding.target
        {
            if definition.identity().imported().is_none() {
                return self.parse_host_struct(definition, name_span);
            }
            #[cfg(feature = "rust-import")]
            return self.parse_imported_struct(binding.qualified, definition, owner, name_span);
            #[cfg(not(feature = "rust-import"))]
            let _ = owner;
        }
        let model = self.named_model(&name);
        self.parse_struct_fields(name, model, name_span)
    }

    /// `S { a: x, b, ..base }`: fields are evaluated in source order, then
    /// `base`, which supplies the fields not named, as in Rust.
    fn parse_struct_fields(
        &mut self,
        name: ImmutableString,
        model: Option<Nominal>,
        name_span: Span,
    ) -> Result<ParsedExpr, Error> {
        let Some(Nominal::Struct { definition, item }) = model else {
            return Err(Error::InvalidConstruction {
                owner: name,
                reason: "only declared structs use named-field construction".into(),
                construction: name_span,
            });
        };
        self.next();
        let model = Nominal::Struct {
            definition: definition.clone(),
            item: item.clone(),
        };
        let (struct_ty, substitution) = self.fresh_nominal(&model, name_span);
        let mut values = BTreeMap::<ImmutableString, (ParsedExpr, Span)>::new();
        let mut order = Vec::new();
        let mut base = None;

        while !matches!(self.peek(), Token::RightBrace) {
            if self.element_left_out()? {
                continue;
            }
            if matches!(self.peek(), Token::ExclusiveRange) {
                self.next();
                let mut value = self.parse_expr()?;
                self.coerce_expr(&struct_ty, &mut value)?;
                base = Some(value);
                if !matches!(self.peek(), Token::RightBrace) {
                    return Err(self.unsupported("fields after `..base` in a struct expression"));
                }
                break;
            }
            let (field, field_span) = self.expect_name("struct field initializer")?;
            order.push(field.clone());
            let value = if matches!(self.peek(), Token::Colon) {
                self.next();
                let expected = item
                    .fields
                    .iter()
                    .find(|known| known.name.text == field)
                    .map(|known| instantiate(&Ty::from_resolved(&known.ty), &substitution));
                self.parse_expr_expecting(expected.as_ref())?
            } else {
                self.parse_name_primary(field.clone(), field_span)?
            };
            if values.insert(field.clone(), (value, field_span)).is_some() {
                return Err(Error::InvalidConstruction {
                    owner: definition.name().into(),
                    reason: format!("field `{field}` is initialized more than once").into(),
                    construction: field_span,
                });
            }
            if matches!(self.peek(), Token::Comma) {
                self.next();
            } else if !matches!(self.peek(), Token::RightBrace) {
                return Err(self.unsupported("struct fields without ','"));
            }
        }
        let end = self.expect(Token::RightBrace, "'}' after struct fields")?;
        if let Some((field, (_, field_span))) = values
            .iter()
            .find(|(field, _)| !item.fields.iter().any(|known| known.name.text == **field))
        {
            return Err(Error::UnknownMember {
                owner: definition.name().into(),
                member: field.clone(),
                reference: *field_span,
            });
        }
        // Each value is bound in source order before construction.
        let mut prelude = origin::Fragment::plain("{\n");
        let mut temporaries = BTreeMap::new();
        for field in &order {
            let (value, _) = values.get_mut(field).expect("ordered fields were parsed");
            let declared = item
                .fields
                .iter()
                .find(|known| known.name.text == *field)
                .expect("unknown fields were rejected");
            let expected = instantiate(&Ty::from_resolved(&declared.ty), &substitution);
            self.coerce_expr(&expected, value)?;
            self.move_local(value)?;
            let temporary = self.temporary("field");
            prelude.push_text(format!("let {temporary} = "));
            prelude.push(value.rendered_source());
            prelude.push_text(";\n");
            temporaries.insert(field.clone(), temporary);
        }
        // `..base` moves the fields it gives out of `base`: those with drop
        // glue are left `()` there, and the rest of a value made for it
        // drops with the statement's temporaries.
        let mut base_clears = String::new();
        if let Some(base) = base.as_mut()
            && self.drops()
        {
            let mut taken = Vec::new();
            for (slot, field) in item.fields.iter().enumerate() {
                let ty = instantiate(&Ty::from_resolved(&field.ty), &substitution);
                if !temporaries.contains_key(field.name.text.as_str())
                    && self.needs_drop(&ty, base.span)?
                {
                    taken.push(slot);
                }
            }
            if !taken.is_empty() {
                self.reject_drop_move(definition.id(), true, base.span)?;
                let holder = match &base.lvalue {
                    Some(Lvalue::Binding { binding, .. }) => Some(binding.to_string()),
                    None => self.hold_temporary(base)?.map(|held| held.to_string()),
                    Some(_) => None,
                };
                let Some(holder) = holder else {
                    return Err(Error::UnsupportedNominalSyntax {
                        construct: "`..base` of a place other than a local, with fields whose \
                                    type implements `Drop`: not supported in Rawr yet"
                            .into(),
                        source: base.span,
                    });
                };
                for slot in taken {
                    let _ = write!(
                        base_clears,
                        "{}({holder}, {slot}); ",
                        crate::engine::KEYWORD_CLEAR
                    );
                }
            }
        }
        let base_name = self.temporary("base");
        if let Some(base) = &base {
            prelude.push_text(format!("let {base_name} = "));
            prelude.push(base.rendered_source());
            prelude.push_text(";\n");
            prelude.push_text(base_clears);
        }
        let mut ordered = Vec::with_capacity(item.fields.len());
        for (slot, field) in item.fields.iter().enumerate() {
            if let Some(temporary) = temporaries.get(field.name.text.as_str()) {
                ordered.push(origin::Fragment::plain(temporary.clone()));
            } else if base.is_some() {
                ordered.push(field_place(
                    origin::Fragment::plain(base_name.as_str()),
                    slot,
                ));
            } else {
                return Err(Error::InvalidConstruction {
                    owner: definition.name().into(),
                    reason: format!("missing field `{}`", field.name.text).into(),
                    construction: span(name_span.start(), end),
                });
            }
        }
        let mut parts = Vec::new();
        for (name, (value, _)) in &values {
            let slot = item
                .fields
                .iter()
                .position(|field| field.name.text == *name);
            parts.push((
                value,
                match slot {
                    Some(slot) => borrows::Slot::Field(definition.id(), None, slot),
                    None => borrows::Slot::At(borrowck::Path::ROOT),
                },
            ));
        }
        parts.extend(base.iter().map(|base| (base, borrows::Slot::Whole)));
        self.compose(span(name_span.start(), end), &struct_ty, &parts);
        let declared_order = base.is_none()
            && order
                .iter()
                .zip(item.fields.iter())
                .all(|(field, declared)| *field == declared.name.text);
        let helper = self.push_op(Op::Struct {
            definition: definition.clone(),
            item,
        });
        let source = if declared_order {
            let values = order.iter().map(|field| {
                let (value, _) = values.remove(field).expect("ordered fields were parsed");
                value.rendered_source()
            });
            origin::Fragment::call(&helper, values.collect::<Vec<_>>())
        } else {
            prelude.push(origin::Fragment::call(&helper, ordered));
            prelude.push_text("\n}");
            prelude.surrounded("(", ")")
        };
        Ok(ParsedExpr {
            prelude: origin::Fragment::default(),
            source,
            ty: struct_ty,
            span: span(name_span.start(), end),
            lvalue: None,
            outcome: crate::host::usage::Outcome::Conservative,
            evaluation: crate::host::usage::Evaluation::Required,
        }
        .retain_origin())
    }

    /// `{ x, y: value }` after a struct variant: `helper` called with the
    /// values in declared order, each evaluated in written order, as Rust
    /// does; with the position of the `}`.
    fn parse_named_values(
        &mut self,
        definition: &type_def::Definition,
        fields: &[(ImmutableString, Ty)],
        helper: &str,
    ) -> Result<(origin::Fragment, Position), Error> {
        self.expect(Token::LeftBrace, "'{' after a struct variant")?;
        let mut written = Vec::<(usize, ParsedExpr)>::new();
        while !matches!(self.peek(), Token::RightBrace) {
            let (field, field_span) = self.expect_name("variant field initializer")?;
            let Some(slot) = fields.iter().position(|(known, _)| *known == field) else {
                return Err(Error::UnknownMember {
                    owner: definition.name().into(),
                    member: field,
                    reference: field_span,
                });
            };
            if written.iter().any(|(known, _)| *known == slot) {
                return Err(Error::InvalidConstruction {
                    owner: definition.name().into(),
                    reason: format!("field `{field}` is initialized more than once").into(),
                    construction: field_span,
                });
            }
            let expected = &fields[slot].1;
            let mut value = if matches!(self.peek(), Token::Colon) {
                self.next();
                self.parse_expr_expecting(Some(expected))?
            } else {
                self.parse_name_primary(field, field_span)?
            };
            self.coerce_expr(expected, &mut value)?;
            self.move_local(&mut value)?;
            written.push((slot, value));
            if matches!(self.peek(), Token::Comma) {
                self.next();
            } else if !matches!(self.peek(), Token::RightBrace) {
                return Err(self.unsupported("variant fields without ','"));
            }
        }
        let end = self.expect(Token::RightBrace, "'}' after variant fields")?;
        if let Some((missing, _)) = (0..fields.len()).find_map(|slot| {
            (!written.iter().any(|(known, _)| *known == slot)).then(|| &fields[slot])
        }) {
            return Err(Error::InvalidConstruction {
                owner: definition.name().into(),
                reason: format!("missing field `{missing}`").into(),
                construction: point_span(end),
            });
        }
        if written
            .iter()
            .enumerate()
            .all(|(index, (slot, _))| index == *slot)
        {
            let values = written.iter().map(|(_, value)| value.rendered_source());
            return Ok((
                origin::Fragment::call(helper, values.collect::<Vec<_>>()),
                end,
            ));
        }
        let mut source = origin::Fragment::plain("({\n");
        let mut temporaries = vec![String::new(); fields.len()];
        for (slot, value) in &written {
            let temporary = self.temporary("field");
            source.push_text(format!("let {temporary} = "));
            source.push(value.rendered_source());
            source.push_text(";\n");
            temporaries[*slot] = temporary;
        }
        source.push(origin::Fragment::call(
            helper,
            temporaries
                .into_iter()
                .map(origin::Fragment::plain)
                .collect::<Vec<_>>(),
        ));
        source.push_text("\n})");
        Ok((source, end))
    }

    /// `T { a: x, ..base }` for a facade type: `base.a(x)` through the
    /// facade's field builders, after binding the values and then `base`
    /// in source order; without `..base`, it starts from `T::default()`.
    fn parse_host_struct(
        &mut self,
        definition: crate::host::type_def::Definition,
        name_span: Span,
    ) -> Result<ParsedExpr, Error> {
        self.expect(Token::LeftBrace, "'{' after a struct name")?;
        let mut fields = Vec::<(ImmutableString, Span, ParsedExpr)>::new();
        let mut base = None;
        while !matches!(self.peek(), Token::RightBrace) {
            if self.element_left_out()? {
                continue;
            }
            if matches!(self.peek(), Token::ExclusiveRange) {
                self.next();
                base = Some(self.parse_expr()?);
                if !matches!(self.peek(), Token::RightBrace) {
                    return Err(self.unsupported("fields after `..base` in a struct expression"));
                }
                break;
            }
            let (field, field_span) = self.expect_name("struct field initializer")?;
            let value = if matches!(self.peek(), Token::Colon) {
                self.next();
                self.parse_expr()?
            } else {
                self.parse_name_primary(field.clone(), field_span)?
            };
            if fields.iter().any(|(name, _, _)| *name == field) {
                return Err(Error::InvalidConstruction {
                    owner: definition.path().into(),
                    reason: format!("field `{field}` is initialized more than once").into(),
                    construction: field_span,
                });
            }
            fields.push((field, field_span, value));
            if matches!(self.peek(), Token::Comma) {
                self.next();
            } else if !matches!(self.peek(), Token::RightBrace) {
                return Err(self.unsupported("struct fields without ','"));
            }
        }
        let end = self.expect(Token::RightBrace, "'}' after struct fields")?;
        let mut prelude = origin::Fragment::plain("({\n");
        let mut values = Vec::new();
        // Each field's declared type, which a value no builder takes is
        // reported against.
        let mut declared_types = Vec::new();
        for (field, field_span, value) in fields {
            // The field's declared type is the value's expected type, as in
            // Rust: `background: Some(color.into())` converts into the
            // field's `Option<Background>`. A value it does not fit still
            // reaches the field's builders. The type the field reads as is
            // its declared type where a builder also takes it. A facade's
            // `f64` and `i64` stand for iced's narrower numbers, so a number
            // keeps its own type.
            let members = self
                .models
                .host
                .members(&definition.path())
                .filter(|item| item.path_ref().name() == field.as_str())
                .collect::<Vec<_>>();
            let reader = members
                .iter()
                .find(|item| {
                    item.receiver() == Some(crate::receiver::Mode::Shared)
                        && item.signature().parameters().is_empty()
                })
                .map(|item| item.signature().output().clone())
                .filter(|output| {
                    !matches!(output.shape(), crate::host::type_ref::Shape::Builtin(_))
                        && members.iter().any(|item| {
                        matches!(item.signature().parameters(), [input] if input == output)
                    })
                });
            let mut field_type = None;
            if let Some(declared) = reader
                && let Ok(declared) = self.instantiate_host_type(
                    &declared,
                    &mut BTreeMap::new(),
                    field.as_str(),
                    Span::NONE,
                    None,
                    Some(&definition),
                )
            {
                let checkpoint = self.inference.checkpoint();
                if self.unify_type(&declared, &value.ty) {
                    self.inference.commit(checkpoint);
                } else {
                    self.inference.rollback(checkpoint);
                }
                field_type = Some(declared);
            }
            declared_types.push(field_type);
            let temporary = self.temporary("field");
            prelude.push_text(format!("let {temporary} = "));
            prelude.push(value.rendered_source());
            prelude.push_text(";\n");
            let bound =
                self.operator_expr(origin::Fragment::plain(temporary), value.ty, value.span);
            values.push((field, field_span, bound));
        }
        let mut value = match base {
            Some(base) => {
                // `..Default::default()` is a value of the struct, as Rust
                // infers it from the literal.
                if let Ok(own) = crate::host::type_ref::Type::registered(definition.path().as_str())
                    && let Ok(own) = self.instantiate_host_type(
                        &own,
                        &mut BTreeMap::new(),
                        "struct base",
                        Span::NONE,
                        None,
                        Some(&definition),
                    )
                {
                    let checkpoint = self.inference.checkpoint();
                    if self.unify_type(&own, &base.ty) {
                        self.inference.commit(checkpoint);
                    } else {
                        self.inference.rollback(checkpoint);
                    }
                }
                let temporary = self.temporary("base");
                prelude.push_text(format!("let {temporary} = "));
                prelude.push(base.rendered_source());
                prelude.push_text(";\n");
                self.operator_expr(origin::Fragment::plain(temporary), base.ty, base.span)
            }
            None => {
                // Rust names every field of a literal without `..base`
                // (E0063): the fields the facade reads, all of them known.
                let fields = self
                    .models
                    .host
                    .members(&definition.path())
                    .filter(|item| {
                        item.receiver() == Some(crate::receiver::Mode::Shared)
                            && item.signature().parameters().is_empty()
                    })
                    .map(|item| ImmutableString::from(item.path_ref().name()))
                    .collect::<Vec<_>>();
                let missing = fields
                    .iter()
                    .filter(|field| !values.iter().any(|(name, ..)| name == *field))
                    .map(|field| format!("`{field}`"))
                    .collect::<Vec<_>>();
                if fields.is_empty() || !missing.is_empty() {
                    return Err(Error::InvalidConstruction {
                        owner: definition.path().into(),
                        reason: if fields.is_empty() {
                            "Rust needs every field or `..base` (E0063); write `..base`".into()
                        } else {
                            format!("missing fields {} (E0063)", missing.join(", ")).into()
                        },
                        construction: name_span,
                    });
                }
                let path = format!("{}::default", definition.path());
                let Some(items) = self.models.host_items(&path).map(<[_]>::to_vec) else {
                    return Err(Error::InvalidConstruction {
                        owner: definition.path().into(),
                        reason: "this struct needs every field, or `..base` from a value of it"
                            .into(),
                        construction: name_span,
                    });
                };
                let owner = self.models.item_owner(&path).cloned();
                self.pending_arguments = Some((Vec::new(), name_span.end()));
                self.parse_host_function(items.into_boxed_slice(), owner, name_span)?
            }
        };
        for ((field, field_span, bound), declared) in values.into_iter().zip(declared_types) {
            // A value no builder of the field takes is a mismatch with the
            // field's type, at the value, as rustc reports it (E0308).
            let mismatch = declared.map(|declared| {
                (
                    self.inference.resolve(&declared).name(&self.models),
                    self.inference.resolve(&bound.ty).name(&self.models),
                    bound.span,
                )
            });
            self.pending_arguments = Some((vec![bound], end));
            value = match (self.parse_host_method(value, field, field_span), mismatch) {
                (Ok(value), _) => value,
                (Err(Error::InvalidHostOperation { .. }), Some((expected, actual, expression))) => {
                    return Err(Error::TypeMismatch {
                        expected,
                        actual,
                        expression,
                    });
                }
                (Err(error), _) => return Err(error),
            };
        }
        // An enum's struct variant (`HandleShape::Rectangle { .. }`) is a
        // builder of its own, which its `into` makes the enum.
        let path = definition.path();
        if let Some((owner, _)) = path.rsplit_once("::")
            && self.models.host.source_type(owner).is_some()
        {
            self.pending_arguments = Some((Vec::new(), end));
            value = self.parse_host_method(value, "into".into(), name_span)?;
        }
        prelude.push(value.rendered_source());
        prelude.push_text("\n})");
        let ty = value.ty;
        Ok(ParsedExpr {
            prelude: origin::Fragment::default(),
            source: prelude,
            ty,
            span: span(name_span.start(), end),
            lvalue: None,
            outcome: crate::host::usage::Outcome::Conservative,
            evaluation: crate::host::usage::Evaluation::Required,
        }
        .retain_origin())
    }

    /// A crate's struct that `path` names in the value namespace, as rustc
    /// reads `badge::Secondary` and `badge::Wrap(1)`: its constructor. An
    /// uncalled tuple struct is its constructor as a function value, the
    /// closure that calls it by the path as `written`, as `.map(Wrap)`
    /// passes it.
    #[cfg(feature = "rust-import")]
    fn imported_struct_path(
        &mut self,
        path: &str,
        written: &[(&ImmutableString, Span)],
        path_span: Span,
    ) -> Result<Option<ParsedExpr>, Error> {
        let Some(definition) = self
            .models
            .host
            .source_type(path)
            .filter(|definition| definition.identity().imported().is_some())
            .cloned()
        else {
            return Ok(None);
        };
        let owner = self.models.host.type_owner(path).cloned();
        self.imported_struct_value(path.into(), definition, owner, written, path_span)
    }

    /// [`Self::imported_struct_path`] for the struct `definition` at `path`.
    #[cfg(feature = "rust-import")]
    fn imported_struct_value(
        &mut self,
        path: ImmutableString,
        definition: crate::host::type_def::Definition,
        owner: Option<crate::host::facade::Dependency>,
        written: &[(&ImmutableString, Span)],
        path_span: Span,
    ) -> Result<Option<ParsedExpr>, Error> {
        let crate::host::type_def::Shape::Struct { form, fields, .. } = definition.shape() else {
            return Ok(None);
        };
        let called = matches!(self.peek(), Token::LeftParen | Token::Unit);
        match form {
            crate::host::type_def::Form::Unit if !matches!(self.peek(), Token::DoubleColon) => {}
            crate::host::type_def::Form::Tuple if called => {}
            crate::host::type_def::Form::Tuple
                if !fields.is_empty()
                    && !matches!(self.peek(), Token::DoubleColon)
                    && (self.no_struct || !matches!(self.peek(), Token::LeftBrace)) =>
            {
                let at = path_span.start();
                let parameter =
                    |index: usize| Token::Identifier(Box::new(format!("__a{index}").into()));
                let arity = fields.len();
                let mut tokens = Vec::new();
                for index in 0..arity {
                    if index > 0 {
                        tokens.push((Token::Comma, at));
                    }
                    tokens.push((parameter(index), at));
                }
                tokens.push((Token::Pipe, at));
                for (index, (name, name_span)) in written.iter().enumerate() {
                    if index > 0 {
                        tokens.push((Token::DoubleColon, name_span.start()));
                    }
                    tokens.push((
                        Token::Identifier(Box::new(name.as_str().into())),
                        name_span.start(),
                    ));
                }
                tokens.push((Token::LeftParen, at));
                for index in 0..arity {
                    if index > 0 {
                        tokens.push((Token::Comma, at));
                    }
                    tokens.push((parameter(index), at));
                }
                tokens.push((Token::RightParen, at));
                self.splice_tokens(tokens);
                return self.parse_closure(Token::Pipe, path_span, false).map(Some);
            }
            _ => return Ok(None),
        }
        self.parse_imported_struct(path, definition, owner, path_span)
            .map(Some)
    }

    #[cfg(feature = "rust-import")]
    fn parse_imported_struct(
        &mut self,
        path: ImmutableString,
        definition: crate::host::type_def::Definition,
        owner: Option<crate::host::facade::Dependency>,
        name_span: Span,
    ) -> Result<ParsedExpr, Error> {
        let crate::host::type_def::Shape::Struct {
            form,
            fields,
            constructible,
            unsupported,
            ..
        } = definition.shape()
        else {
            return Err(Error::InvalidConstruction {
                owner: path,
                reason: "only admitted imported structs use named-field construction".into(),
                construction: name_span,
            });
        };
        // A public field whose type rawr can't hold yet is named with its
        // type, as the crate writes it: the literal is valid Rust (D664).
        if let Some(unsupported) = unsupported {
            return Err(Error::InvalidConstruction {
                owner: path,
                reason: unsupported_field(unsupported).into(),
                construction: name_span,
            });
        }
        if !constructible {
            return Err(Error::InvalidConstruction {
                owner: path,
                reason: "private Rust fields make this struct non-constructible".into(),
                construction: name_span,
            });
        }
        // A constructor infers its parameters from its fields and context;
        // defaults only fill parameters those constraints leave unknown.
        let instance = definition
            .parameters()
            .iter()
            .map(|_| self.inference.fresh())
            .collect::<Box<[_]>>();
        if let Some((expected, depth)) = &self.collect_hint
            && *depth == self.expression_stack.len()
            && let Ty::Host { proof, arguments } = self.inference.resolve(expected)
            && proof.definition.identity() == definition.identity()
        {
            for (parameter, argument) in instance.iter().zip(&arguments) {
                self.require_type(parameter, argument, name_span)?;
            }
        }
        let fields = fields.clone();
        let form = *form;
        let selected_owner = self.models.host.type_owner(&path).cloned();
        let mut base = None::<ParsedExpr>;
        let mut prelude = origin::Fragment::default();
        let (mut arguments, end) = match form {
            crate::host::type_def::Form::Unit => (Vec::new(), name_span.end()),
            crate::host::type_def::Form::Tuple => {
                let mut arguments = Vec::new();
                let end = if matches!(self.peek(), Token::Unit) {
                    self.next().1
                } else {
                    self.expect(Token::LeftParen, "'(' after imported tuple struct")?;
                    while !matches!(self.peek(), Token::RightParen) {
                        if self.element_left_out()? {
                            continue;
                        }
                        arguments.push(self.parse_expr()?);
                        if matches!(self.peek(), Token::Comma) {
                            self.next();
                        } else {
                            break;
                        }
                    }
                    self.expect(Token::RightParen, "')' after imported tuple struct")?
                };
                (arguments, end)
            }
            crate::host::type_def::Form::Record => {
                self.expect(Token::LeftBrace, "'{' after imported record struct")?;
                let mut values = BTreeMap::<ImmutableString, (ParsedExpr, Span)>::new();
                let mut order = Vec::new();
                while !matches!(self.peek(), Token::RightBrace) {
                    if self.element_left_out()? {
                        continue;
                    }
                    if matches!(self.peek(), Token::ExclusiveRange) {
                        self.next();
                        base = Some(self.parse_expr()?);
                        if !matches!(self.peek(), Token::RightBrace) {
                            return Err(
                                self.unsupported("fields after `..base` in a struct expression")
                            );
                        }
                        break;
                    }
                    let (field, field_span) =
                        self.expect_name("imported struct field initializer")?;
                    order.push(field.clone());
                    let value = if matches!(self.peek(), Token::Colon) {
                        self.next();
                        self.parse_expr()?
                    } else {
                        self.parse_name_primary(field.clone(), field_span)?
                    };
                    if values.insert(field.clone(), (value, field_span)).is_some() {
                        return Err(Error::InvalidConstruction {
                            owner: path,
                            reason: format!("field `{field}` is initialized more than once").into(),
                            construction: field_span,
                        });
                    }
                    if matches!(self.peek(), Token::Comma) {
                        self.next();
                    } else if !matches!(self.peek(), Token::RightBrace) {
                        return Err(self.unsupported("imported struct fields without ','"));
                    }
                }
                let end = self.expect(Token::RightBrace, "'}' after imported struct fields")?;
                // With `..base`, the named fields and then `base` are bound
                // in source order; `base` supplies the rest.
                let base_value = match base.take() {
                    Some(mut base) => {
                        let ty = Ty::Host {
                            proof: HostTyProof::new(
                                definition.clone(),
                                Some(definition.clone()),
                                owner.clone(),
                            ),
                            arguments: instance.clone(),
                        };
                        self.coerce_expr(&ty, &mut base)?;
                        for field in &order {
                            let (value, _) = values.get_mut(field).expect("parsed fields remain");
                            let temporary = self.temporary("field");
                            prelude.push_text(format!("let {temporary} = "));
                            prelude.push(value.rendered_source());
                            prelude.push_text(";\n");
                            value.prelude = origin::Fragment::default();
                            value.source = origin::Fragment::plain(temporary);
                        }
                        let temporary = self.temporary("base");
                        prelude.push_text(format!("let {temporary} = "));
                        prelude.push(base.rendered_source());
                        prelude.push_text(";\n");
                        base.prelude = origin::Fragment::default();
                        base.source = origin::Fragment::plain(temporary);
                        Some(base)
                    }
                    None => None,
                };
                // The named fields fix what they can of the type's
                // arguments first, so a field `base` gives is read knowing
                // whether it is `Copy`, as rustc decides it at the end.
                if base_value.is_some() {
                    let mut parameters = definition
                        .parameters()
                        .iter()
                        .cloned()
                        .zip(instance.iter().cloned())
                        .collect::<BTreeMap<_, _>>();
                    for field in &fields {
                        let Some((value, _)) = values.get(field.name()) else {
                            continue;
                        };
                        let actual = value.ty.clone();
                        let expected = self.instantiate_host_type(
                            field.ty(),
                            &mut parameters,
                            &path,
                            value.span,
                            selected_owner.as_ref(),
                            Some(&definition),
                        )?;
                        if self.unifies(&expected, &actual) {
                            self.unify_type(&expected, &actual);
                        }
                    }
                }
                let mut arguments = Vec::with_capacity(fields.len());
                for field in &fields {
                    if let (None, Some(base)) = (values.get(field.name()), &base_value) {
                        let read = self.read_imported_field(
                            ParsedExpr {
                                prelude: origin::Fragment::default(),
                                source: base.source.clone(),
                                ty: base.ty.clone(),
                                span: base.span,
                                lvalue: None,
                                outcome: crate::host::usage::Outcome::Conservative,
                                evaluation: crate::host::usage::Evaluation::Required,
                            },
                            definition.clone(),
                            field.name().into(),
                            base.span,
                        )?;
                        arguments.push(read);
                        continue;
                    }
                    let Some((value, _)) = values.remove(field.name()) else {
                        return Err(Error::InvalidConstruction {
                            owner: path,
                            reason: format!("missing field `{}`", field.name()).into(),
                            construction: span(name_span.start(), end),
                        });
                    };
                    arguments.push(value);
                }
                if let Some((field, (_, field_span))) = values.into_iter().next() {
                    return Err(Error::UnknownMember {
                        owner: path,
                        member: field,
                        reference: field_span,
                    });
                }
                (arguments, end)
            }
        };
        if arguments.len() != fields.len() {
            return Err(Error::InvalidConstruction {
                owner: path,
                reason: format!(
                    "imported struct expects {} value(s), found {}",
                    fields.len(),
                    arguments.len()
                )
                .into(),
                construction: span(name_span.start(), end),
            });
        }
        let mut parameters = definition
            .parameters()
            .iter()
            .cloned()
            .zip(instance.iter().cloned())
            .collect::<BTreeMap<_, _>>();
        for (argument, field) in arguments.iter_mut().zip(&fields) {
            let expected = self.instantiate_host_type(
                field.ty(),
                &mut parameters,
                &path,
                argument.span,
                selected_owner.as_ref(),
                Some(&definition),
            )?;
            self.coerce_expr(&expected, argument)?;
            self.consume_imported_value(argument, &path, argument.span)?;
        }
        for (index, parameter) in instance.iter().enumerate() {
            if matches!(self.inference.resolve(parameter), Ty::Infer(_))
                && let Some(Some(default)) = definition.defaults().get(index)
            {
                let default = self.instantiate_host_type(
                    default,
                    &mut parameters,
                    &path,
                    name_span,
                    owner.as_ref(),
                    Some(&definition),
                )?;
                self.structural_defaults
                    .push((parameter.clone(), default, name_span));
            }
        }
        let ordered = arguments
            .iter()
            .map(ParsedExpr::rendered_source)
            .collect::<Vec<_>>();
        let construction = span(name_span.start(), end);
        let selector =
            definition
                .imported_constructor()
                .ok_or_else(|| Error::InvalidHostOperation {
                    item: path.clone(),
                    reason: "constructing it with a struct literal is not supported in Rawr yet"
                        .into(),
                    operation: construction,
                })?;
        let output = Ty::Host {
            proof: HostTyProof::new(definition.clone(), Some(definition.clone()), owner),
            arguments: instance.clone(),
        };
        let imported = ImportedOperation {
            kind: crate::host::structural::Kind::Constructor,
            operands: arguments
                .iter()
                .map(|argument| (argument.ty.clone(), argument.span, false))
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            output: output.clone(),
            receiver: None,
        };
        let helper = self.push_imported_op(&path, selector, Some(imported), construction)?;
        let mut source = origin::Fragment::call(&helper, ordered);
        if !prelude.is_empty() {
            prelude.push(source);
            source = prelude.surrounded("({\n", "\n})");
        }
        Ok(ParsedExpr {
            prelude: origin::Fragment::default(),
            source,
            ty: output,
            span: construction,
            lvalue: None,
            outcome: crate::host::usage::Outcome::Conservative,
            evaluation: crate::host::usage::Evaluation::Required,
        }
        .retain_origin())
    }

    /// rustc's E0277 for `expression`, a `?` whose operand returns `early`
    /// where the function's output is `output`, which can't hold it.
    fn try_operator(&self, output: &Ty, early: &Ty, expression: Span) -> Error {
        let output = self.inference.resolve(output);
        let early = self.inference.resolve(early);
        let name = output.name(&self.models);
        let (message, label) = match (&output, &early) {
            (Ty::Result(_, returned), Ty::Result(_, error)) => (
                format!(
                    "`?` couldn't convert the error to `{}`",
                    returned.name(&self.models)
                ),
                format!(
                    "the trait `From<{}>` is not implemented for `{}`",
                    error.name(&self.models),
                    returned.name(&self.models)
                ),
            ),
            (Ty::Result(..), _) => (
                "the `?` operator can only be used on `Result`s, not `Option`s, in a function \
                 that returns `Result`"
                    .to_string(),
                format!("use `.ok_or(...)?` to provide an error compatible with `{name}`"),
            ),
            (Ty::Option(_), _) => (
                "the `?` operator can only be used on `Option`s, not `Result`s, in a function \
                 that returns `Option`"
                    .to_string(),
                format!(
                    "use `.ok()?` if you want to discard the `{}` error information",
                    early.name(&self.models)
                ),
            ),
            _ => (
                "the `?` operator can only be used in a function that returns `Result` or \
                 `Option` (or another type that implements `FromResidual`)"
                    .to_string(),
                format!("cannot use the `?` operator in a function that returns `{name}`"),
            ),
        };
        // rustc points at the `?` itself.
        let end = expression.end();
        let at = match (end.line(), end.position()) {
            (Some(line), Some(column)) if column > 1 => Span::new(
                Position::new(
                    u16::try_from(line).unwrap_or(u16::MAX),
                    u16::try_from(column - 1).unwrap_or(u16::MAX),
                ),
                end,
            ),
            _ => expression,
        };
        Error::Unimplemented {
            message: message.into(),
            label: label.into(),
            help: None,
            at,
        }
    }

    /// rustc's `overflowing_literals` error for the integer literal at
    /// `at`, whose number `value` `kind` can't hold: the literal as the
    /// program writes it, suffix included.
    fn overflowing(&self, kind: Int, value: Wide, at: Span) -> Error {
        let written = self
            .written_from(at.start())
            .map(|line| {
                let mut chars = line.char_indices();
                let start = usize::from(line.starts_with('-'));
                let end = chars
                    .find(|&(index, character)| {
                        index >= start && !(character.is_alphanumeric() || character == '_')
                    })
                    .map_or(line.len(), |(index, _)| index);
                line[..end].to_string()
            })
            .filter(|written| {
                written
                    .trim_start_matches('-')
                    .starts_with(|c: char| c.is_ascii_digit())
            })
            .unwrap_or_else(|| value.to_string());
        let literal = span(at.start(), text_end(at.start(), &written));
        overflow::error(kind, value, &written, literal)
    }

    /// `Type::member(..)` for a builtin type: `String::new()`, the
    /// prelude's and the crate's `From` conversions (`String::from(x)`,
    /// `i64::from(true)`), and crate trait functions without `self`.
    /// `concat!`, `env!`, `include_str!`, `include_bytes!` and `stringify!`,
    /// which rustc expands while it compiles: their text (or bytes) as a
    /// literal. `env!("CARGO_MANIFEST_DIR")` is the project's directory and
    /// `include_str!`'s path is relative to the file that names it.
    fn parse_compile_time_macro(
        &mut self,
        name: &str,
        name_span: Span,
    ) -> Result<ParsedExpr, Error> {
        let (text, end) = self.compile_time_text(name, name_span)?;
        let at = span(name_span.start(), end);
        if matches!(name, "line" | "column") {
            return Ok(self.literal(text, Ty::Int(Int::U32), at));
        }
        // `option_env!("NAME")`: `Some` of its value at compile time, or
        // `None`.
        if name == "option_env" {
            let value = self.environment_variable(&text);
            let ty = Ty::Option(Box::new(Ty::Str));
            let source = match value {
                Some(value) => self.prelude_op(PreludeOp::Some, [format!("{value:?}").into()]),
                None => self.prelude_op(PreludeOp::None, []),
            };
            return Ok(self.operator_expr(source, ty, at));
        }
        if name == "include_bytes" {
            // A file the host ships is read from its bundle's module, as the
            // constant the host built once: its bytes are the program's, as
            // its text is, and no array literal of them is parsed.
            if let Some((index, bytes)) = crate::resolve::bundled(&text) {
                let ty = Ty::Reference {
                    mutable: false,
                    target: Box::new(Ty::Array {
                        element: Box::new(Ty::Int(Int::U8)),
                        kind: sequence::Kind::Array(bytes.len()),
                    }),
                };
                return Ok(self.literal(crate::resolve::bundled_constant(index), ty, at));
            }
            let bytes = self.include(name, &text, at)?;
            return Ok(self.byte_literal(&bytes, at));
        }
        Ok(self.literal(format!("{text:?}"), Ty::Str, at))
    }

    /// `bytes` as a `&[u8; N]` literal: an array of integers at runtime.
    fn byte_literal(&self, bytes: &[u8], at: Span) -> ParsedExpr {
        let source = format!(
            "[{}]",
            bytes
                .iter()
                .map(u8::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        );
        let ty = Ty::Reference {
            mutable: false,
            target: Box::new(Ty::Array {
                element: Box::new(Ty::Int(Int::U8)),
                kind: sequence::Kind::Array(bytes.len()),
            }),
        };
        self.literal(source, ty, at)
    }

    /// The text of the compile-time macro `name`, after its `!`, and where
    /// it ends.
    fn compile_time_text(
        &mut self,
        name: &str,
        name_span: Span,
    ) -> Result<(String, Position), Error> {
        // `line!()`, `column!()`, `file!()` and `module_path!()`: where the
        // macro is written.
        if matches!(name, "line" | "column" | "file" | "module_path") {
            let end = match self.next() {
                (Token::Unit, position) => position,
                (Token::LeftParen, _) => self.expect(Token::RightParen, "`)` after `(`")?,
                (_, position) => {
                    return Err(Error::UnsupportedNominalSyntax {
                        construct: format!("`{name}!` without `()`").into(),
                        source: point_span(position),
                    })
                }
            };
            let at = name_span.start();
            let text = match name {
                "line" => at.line().unwrap_or_default().to_string(),
                "column" => at.position().unwrap_or_default().to_string(),
                "file" => match self.source_file() {
                    Some(file) => cargo_env::file_path(&file),
                    None => self.source_identity(),
                },
                _ => self.module_path(),
            };
            return Ok((text, end));
        }
        // `stringify!()` and `concat!()`: an empty string.
        if matches!(name, "stringify" | "concat") && matches!(self.peek(), Token::Unit) {
            let (_, position) = self.next();
            return Ok((String::new(), position));
        }
        let open = match self.next() {
            (Token::LeftParen | Token::LeftBracket | Token::LeftBrace, position) => position,
            (_, position) => {
                return Err(Error::UnsupportedNominalSyntax {
                    construct: format!("`{name}!` without `(`").into(),
                    source: point_span(position),
                })
            }
        };
        if name == "stringify" {
            // The tokens as written, between the delimiters.
            let mut depth = 0usize;
            let mut tokens = Vec::new();
            let end = loop {
                match self.next() {
                    (token @ (Token::LeftParen | Token::LeftBracket | Token::LeftBrace), at) => {
                        depth += 1;
                        tokens.push((token, at));
                    }
                    (Token::RightParen | Token::RightBracket | Token::RightBrace, position)
                        if depth == 0 =>
                    {
                        break position;
                    }
                    (token @ (Token::RightParen | Token::RightBracket | Token::RightBrace), at) => {
                        depth -= 1;
                        tokens.push((token, at));
                    }
                    (Token::EOF, position) => {
                        return Err(Error::UnsupportedNominalSyntax {
                            construct: "`stringify!` without its closing delimiter".into(),
                            source: span(open, position),
                        })
                    }
                    (token, at) => tokens.push((token, at)),
                }
            };
            let key = self.models.sources.key(self.models.context.index());
            let text = self
                .models
                .sources
                .resolve(key)
                .ok()
                .and_then(|source| stringify_tokens(source.text(), &tokens, end));
            let text = text.unwrap_or_else(|| {
                let pieces: Vec<String> =
                    tokens.iter().map(|(token, _)| token.to_string()).collect();
                pieces.join(" ")
            });
            return Ok((text, end));
        }
        let mut arguments = Vec::new();
        let end = loop {
            if let (Token::RightParen | Token::RightBracket | Token::RightBrace, position) =
                (self.peek(), self.position())
            {
                self.next();
                break position;
            }
            arguments.push(self.compile_time_argument()?);
            if matches!(self.peek(), Token::Comma) {
                self.next();
            } else if !matches!(
                self.peek(),
                Token::RightParen | Token::RightBracket | Token::RightBrace
            ) {
                return Err(self.unsupported("compile-time macro arguments without ','"));
            }
        };
        let at = span(name_span.start(), end);
        let one = |arguments: &[String]| -> Result<String, Error> {
            match arguments {
                [text] => Ok(text.clone()),
                _ => Err(Error::InvalidHostOperation {
                    item: format!("{name}!").into(),
                    reason: "takes one string literal".into(),
                    operation: at,
                }),
            }
        };
        let text = match name {
            "concat" => arguments.concat(),
            "env" => {
                let Some(variable) = arguments.first() else {
                    return Err(Error::InvalidHostOperation {
                        item: "env!".into(),
                        reason: "takes the name of an environment variable".into(),
                        operation: at,
                    });
                };
                match self.environment_variable(variable) {
                    Some(value) => value,
                    None => {
                        return Err(Error::InvalidHostOperation {
                            item: "env!".into(),
                            reason: arguments
                                .get(1)
                                .cloned()
                                .unwrap_or_else(|| {
                                    format!(
                                    "environment variable `{variable}` not defined at compile time"
                                )
                                })
                                .into(),
                            operation: at,
                        })
                    }
                }
            }
            "include_str" => {
                let path = one(&arguments)?;
                let bytes = self.include(name, &path, at)?;
                String::from_utf8(bytes).map_err(|_| Error::InvalidHostOperation {
                    item: "include_str!".into(),
                    reason: format!("`{path}` is not UTF-8").into(),
                    operation: at,
                })?
            }
            // `include_bytes!`'s path; its caller reads the file.
            _ => one(&arguments)?,
        };
        Ok((text, end))
    }

    /// One argument of `concat!` or another compile-time macro: a literal
    /// or a nested `concat!`, `env!` or `include_str!`.
    fn compile_time_argument(&mut self) -> Result<String, Error> {
        let negative = matches!(self.peek(), Token::Minus | Token::UnaryMinus);
        if negative {
            self.next();
        }
        let sign = if negative { "-" } else { "" };
        let text = self.next();
        // `1u8`: `concat!` writes the number without its suffix.
        if matches!(text.0, Token::IntegerConstant(_) | Token::FloatConstant(_))
            && let Token::Identifier(suffix) = self.peek()
            && numeric_type(suffix.as_str()).is_some()
        {
            self.next();
        }
        Ok(match text {
            (Token::StringConstant(text), _) if !negative => text.to_string(),
            (Token::IntegerConstant(value), _) => format!("{sign}{value}"),
            (Token::FloatConstant(value), _) => format!("{sign}{}", value.1),
            (Token::CharConstant(value), _) if !negative => value.to_string(),
            (Token::True, _) if !negative => "true".into(),
            (Token::False, _) if !negative => "false".into(),
            (Token::Identifier(name), name_position)
                if !negative
                    && matches!(
                        name.as_str(),
                        "concat"
                            | "env"
                            | "include_str"
                            | "stringify"
                            | "line"
                            | "column"
                            | "file"
                            | "module_path"
                    )
                    && matches!(self.peek(), Token::Bang) =>
            {
                self.next();
                let name_span = point_span(name_position);
                self.compile_time_text(name.as_str(), name_span)?.0
            }
            (_, position) => {
                return Err(Error::UnsupportedNominalSyntax {
                    construct: "a compile-time macro argument other than a literal".into(),
                    source: point_span(position),
                })
            }
        })
    }

    /// The name of the file this code is in, as its source names it.
    fn source_identity(&self) -> String {
        let key = self.models.sources.key(self.models.context.index());
        self.models
            .sources
            .resolve(key)
            .map(|source| source.identity().to_owned())
            .unwrap_or_default()
    }

    /// The program's root file, when it is a file on disk.
    fn entry_file(&self) -> Option<std::path::PathBuf> {
        let key = self.models.sources.key(self.models.entry.index());
        let source = self.models.sources.resolve(key).ok()?;
        let path = std::path::PathBuf::from(source.identity());
        path.is_file().then_some(path)
    }

    /// `module_path!()`: the crate's name and the modules of the item
    /// being read, as `app::geometry`.
    fn module_path(&self) -> String {
        let root = match self.entry_file() {
            Some(entry) => cargo_env::crate_name(&entry),
            None => std::path::Path::new(&self.source_identity())
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or_default()
                .replace('-', "_"),
        };
        let modules = self
            .callable
            .as_ref()
            .map(|key| key.modules())
            .unwrap_or_default();
        std::iter::once(root.as_str())
            .chain(modules.iter().map(ImmutableString::as_str))
            .collect::<Vec<_>>()
            .join("::")
    }

    /// The file this code is in, when it is a file on disk.
    fn source_file(&self) -> Option<std::path::PathBuf> {
        let key = self.models.sources.key(self.models.context.index());
        let source = self.models.sources.resolve(key).ok()?;
        let path = std::path::PathBuf::from(source.identity());
        path.is_file().then_some(path)
    }

    /// A compile-time environment variable: `CARGO_MANIFEST_DIR` is the
    /// directory of the project the file is in; any other is the
    /// environment's.
    fn environment_variable(&self, name: &str) -> Option<String> {
        // A program granted no environment has none: every variable is
        // undefined, as rustc reports one.
        if !self.models.access.env() {
            return None;
        }
        {
            // What Cargo sets for the crate this file is in.
            if name.starts_with("CARGO_")
                && let (Some(file), Some(entry)) = (self.source_file(), self.entry_file())
                && let Some(value) = cargo_env::variable(&file, &entry, name)
            {
                return value;
            }
            if name == "CARGO_MANIFEST_DIR" {
                let file = self.source_file()?;
                let manifest = file
                    .ancestors()
                    .skip(1)
                    .find(|directory| directory.join("Cargo.toml").is_file())
                    .or_else(|| {
                        file.ancestors()
                            .skip(1)
                            .find(|directory| {
                                directory.file_name().is_some_and(|name| name == "src")
                            })
                            .and_then(std::path::Path::parent)
                    })
                    .or_else(|| file.parent())?;
                return Some(manifest.to_string_lossy().into_owned());
            }
            std::env::var(name).ok()
        }
    }

    /// The bytes of the file `path` names, relative to this code's file.
    fn include(&self, macro_name: &str, path: &str, at: Span) -> Result<Vec<u8>, Error> {
        let error = |reason: String| Error::InvalidHostOperation {
            item: format!("{macro_name}!").into(),
            reason: reason.into(),
            operation: at,
        };
        // A file the host ships with its programs, as `?play` ships its
        // examples' assets.
        if let Some((_, bytes)) = crate::resolve::bundled(path) {
            return Ok(bytes.to_vec());
        }
        {
            let Some(file) = self.source_file() else {
                return Err(error(format!(
                    "`{path}` is read relative to a file on disk; this code is not in one"
                )));
            };
            let target = file.parent().map_or_else(
                || std::path::PathBuf::from(path),
                |directory| directory.join(path),
            );
            self.models
                .access
                .read(&target)
                .and_then(|()| std::fs::read(&target))
                .map_err(|failure| {
                    error(format!("couldn't read `{}`: {failure}", target.display()))
                })
        }
    }

    fn float_constant(&mut self, value: f64, single: bool, at: Span) -> ParsedExpr {
        let ty = Ty::Float(if single { Float::F32 } else { Float::F64 });
        self.literal(format!("{value:?}"), ty, at)
    }

    fn parse_builtin_path(
        &mut self,
        builtin: Ty,
        name: ImmutableString,
        name_span: Span,
    ) -> Result<ParsedExpr, Error> {
        self.next();
        let (member, member_span) = self.expect_member_name()?;
        // `char::to_ascii_uppercase`, `String::len(&s)`: a method of the
        // type as std declares it. A call of a number's inherent method is
        // its own (`parse_number_method`).
        if (!matches!(builtin, Ty::Int(_) | Ty::Float(_))
            || !matches!(self.peek(), Token::LeftParen | Token::Unit)
            || std_method::inherent(&builtin, &member).is_none())
            && let Some(value) =
                self.std_method_path(builtin.clone(), &name, &member, name_span, member_span)
        {
            return value;
        }
        // `f64::consts::PI`.
        if member.as_str() == "consts" && matches!(self.peek(), Token::DoubleColon) {
            self.next();
            let (constant, constant_span) = self.expect_member_name()?;
            let path = format!("{name}::consts::{constant}");
            let Some((value, single)) = crate::resolve::float_constant(&path) else {
                return Err(Error::UnknownMember {
                    owner: format!("{name}::consts").into(),
                    member: constant,
                    reference: constant_span,
                });
            };
            let at = span(
                name_span.start(),
                text_end(constant_span.start(), &constant),
            );
            return Ok(self.float_constant(value, single, at));
        }
        // `u8::MAX`, `i64::MIN`, `f64::MAX`: the kind's bounds (`u64::MAX`
        // and `usize::MAX` are all 64 bits set).
        if !matches!(self.peek(), Token::LeftParen | Token::Unit) {
            let at = span(name_span.start(), member_span.end());
            if let Ty::Int(kind) = builtin {
                let bound = match (kind, member.as_str()) {
                    (Int::I128, "MAX") => Some(Wide::Signed(i128::MAX)),
                    (Int::I128, "MIN") => Some(Wide::Signed(i128::MIN)),
                    (Int::U128, "MAX") => Some(Wide::Unsigned(u128::MAX)),
                    _ => None,
                };
                if let Some(bound) = bound {
                    return self.wide_constant(bound, builtin, at);
                }
                if member.as_str() == "BITS" {
                    return Ok(self.literal(kind.bits().to_string(), Ty::Int(Int::U32), at));
                }
            }
            let text = match (&builtin, member.as_str()) {
                (Ty::Int(kind), "MAX") => kind.store(kind.full_range().1).to_string(),
                (Ty::Int(kind), "MIN") => kind.range().0.to_string(),
                (Ty::Float(Float::F32), "MAX") => format!("{:?}", f64::from(f32::MAX)),
                (Ty::Float(Float::F32), "MIN") => format!("{:?}", f64::from(f32::MIN)),
                (Ty::Float(Float::F32), "MIN_POSITIVE") => {
                    format!("{:?}", f64::from(f32::MIN_POSITIVE))
                }
                (Ty::Float(Float::F32), "EPSILON") => format!("{:?}", f64::from(f32::EPSILON)),
                (Ty::Float(_), "MAX") => format!("{:?}", f64::MAX),
                (Ty::Float(_), "MIN") => format!("{:?}", f64::MIN),
                (Ty::Float(_), "MIN_POSITIVE") => format!("{:?}", f64::MIN_POSITIVE),
                (Ty::Float(_), "EPSILON") => format!("{:?}", f64::EPSILON),
                // IEEE division by zero, as rustc folds these.
                (Ty::Float(_), "INFINITY") => "(1.0 / 0.0)".into(),
                (Ty::Float(_), "NEG_INFINITY") => "(-1.0 / 0.0)".into(),
                (Ty::Float(_), "NAN") => "(0.0 / 0.0)".into(),
                // A function as a value is `std_method_path`'s; any other
                // name std doesn't declare for the type.
                _ => {
                    return Err(Error::UnknownMember {
                        owner: name,
                        member,
                        reference: member_span,
                    })
                }
            };
            let text = if text.starts_with('-') {
                format!("({text})")
            } else {
                text
            };
            return Ok(self.literal(text, builtin, span(name_span.start(), member_span.end())));
        }
        let (mut arguments, end) = self.parse_call_arguments("associated function arguments")?;
        let expression = span(name_span.start(), end);
        let owner = owner_id(&builtin);
        // `char::from_digit(n, radix)` and `char::from_u32(n)`: an
        // `Option<char>`.
        if builtin == Ty::Char
            && let ("from_digit", [_, _]) | ("from_u32", [_]) =
                (member.as_str(), arguments.as_slice())
        {
            let mut operands = Vec::new();
            for argument in &mut arguments {
                self.coerce_expr(&Ty::Int(Int::U32), argument)?;
                operands.push(argument.rendered_source());
            }
            if operands.len() == 1 {
                operands.push("-1".into());
            }
            let source = self.prelude_op(PreludeOp::CharFrom, operands);
            let ty = Ty::Option(Box::new(Ty::Char));
            return Ok(self.operator_expr(source, ty, expression));
        }
        // `u8::try_from(x)` of an integer: Rust's `TryFrom` among integers,
        // `Err` when the value does not fit.
        if let (Ty::Int(_), "try_from", [argument]) =
            (&builtin, member.as_str(), arguments.as_slice())
        {
            let source = self.integer_operand(argument)?;
            let value = self.int_op(
                "try_from",
                argument.rendered_source(),
                "0".into(),
                &builtin,
                &source,
            );
            let ty = Ty::Result(
                Box::new(builtin),
                Box::new(Ty::ParseError(ParseError::TryFromInt)),
            );
            return Ok(self.operator_expr(value, ty, expression));
        }
        // `i32::from_str(s)`: std's `FromStr` for the type, which is
        // `s.parse::<i32>()`.
        if member.as_str() == "from_str"
            && let Some(kind) = ParseError::of(&builtin)
            && let [text] = arguments.as_mut_slice()
        {
            self.coerce_expr(&Ty::Str, text)?;
            let helper = self.push_op(Op::Kinded {
                op: PreludeOp::Parse,
                kinds: [builtin.clone(), builtin.clone()],
            });
            let source = origin::Fragment::call(&helper, [text.rendered_source()]);
            let ty = Ty::Result(Box::new(builtin), Box::new(Ty::ParseError(kind)));
            return Ok(self.operator_expr(source, ty, expression));
        }
        if builtin == Ty::String && member.as_str() == "new" && arguments.is_empty() {
            return Ok(self.literal("\"\"", Ty::String, expression));
        }
        // `f64::sqrt(x)`, `f32::rem_euclid(x, y)`: the number's method on
        // the first argument, as Rust's path call is.
        if matches!(builtin, Ty::Int(_) | Ty::Float(_))
            && !matches!(
                member.as_str(),
                "from" | "try_from" | "default" | "from_str_radix" | "from_str"
            )
            && !arguments.is_empty()
        {
            let mut receiver = arguments.remove(0);
            self.coerce_expr(&builtin, &mut receiver)?;
            self.pending_arguments = Some((arguments, end));
            return self.parse_number_method(receiver, builtin, member, member_span);
        }
        // `String::default()`, `u32::default()`, `bool::default()`.
        if member.as_str() == "default" && arguments.is_empty() {
            let source = self.default_source(&builtin, expression)?;
            return Ok(self.operator_expr(source, builtin, expression));
        }
        if builtin == Ty::String
            && member.as_str() == "with_capacity"
            && let [capacity] = arguments.as_mut_slice()
        {
            self.coerce_expr(&Ty::Int(Int::Usize), capacity)?;
            return Ok(self.literal("\"\"", Ty::String, expression));
        }
        // `String::from_utf8(bytes)`: a `Result` of the text or the error.
        if builtin == Ty::String
            && member.as_str() == "from_utf8"
            && let [bytes] = arguments.as_mut_slice()
        {
            let expected = Ty::Array {
                element: Box::new(Ty::Int(Int::U8)),
                kind: sequence::Kind::Vec,
            };
            self.coerce_expr(&expected, bytes)?;
            let code = STR_PIECES
                .iter()
                .position(|name| *name == "from_utf8")
                .expect("a listed piece")
                .to_string();
            let source = self.prelude_op(
                PreludeOp::StrPieces,
                [
                    bytes.rendered_source(),
                    code.into(),
                    "()".into(),
                    "()".into(),
                ],
            );
            let ty = Ty::Result(
                Box::new(Ty::String),
                Box::new(Ty::ParseError(ParseError::Utf8)),
            );
            return Ok(self.operator_expr(source, ty, expression));
        }
        if member.as_str() == "from"
            && let [argument] = arguments.as_mut_slice()
        {
            let source = self.inference.resolve(&argument.ty);
            // `String`'s `From<&str>`, `From<&String>` and `From<String>`.
            if builtin == Ty::String
                && match &source {
                    Ty::Str | Ty::String => true,
                    Ty::Reference { target, .. } => self.inference.resolve(target) == Ty::String,
                    _ => false,
                }
            {
                let source = argument.rendered_source();
                return Ok(self.operator_expr(source, Ty::String, expression));
            }
            // The impl for this Self type: `f32::from` beside `f64::from`,
            // which share one impl owner.
            if let Some((definition, from, _)) = match owner {
                Some(id) => self.select_conversion(id, Some(&builtin), &source, expression)?,
                None => None,
            } {
                self.coerce_expr(&from, argument)?;
                return Ok(self.operator_expr(
                    origin::Fragment::call(
                        method_name(definition.id()),
                        [argument.rendered_source()],
                    ),
                    builtin,
                    expression,
                ));
            }
            if self.unifies(&builtin, &source) {
                self.unify_type(&builtin, &source);
                let value = argument.rendered_source().surrounded("(", ")");
                return Ok(self.operator_expr(value, builtin, expression));
            }
            // Rust's lossless `From` impls among numbers: a wider integer,
            // an `f64` from an `f32` or an integer of at most 32 bits.
            let lossless = match (&builtin, &source) {
                // Into an `i128` or a `u128` from a narrower kind of fixed
                // width; a `u64`'s bits are read unsigned.
                (Ty::Int(to), Ty::Int(from)) if to.wide() || from.wide() => {
                    let fits = !from.wide()
                        && !matches!(from, Int::Isize | Int::Usize)
                        && (*to == Int::I128 || !from.signed());
                    fits.then_some(*from == Int::U64)
                }
                (Ty::Int(to), Ty::Int(from)) => {
                    let ((low, high), (to_low, to_high)) = (from.full_range(), to.full_range());
                    (to_low <= low && high <= to_high).then_some(false)
                }
                (Ty::Float(Float::F64), Ty::Float(Float::F32)) => Some(false),
                (Ty::Float(Float::F64), Ty::Int(from)) => {
                    (from.checked() && !matches!(from, Int::U64 | Int::Usize)).then_some(true)
                }
                _ => None,
            };
            if let Some(cast) = lossless {
                let mut value = argument.rendered_source();
                if cast {
                    let code = match builtin {
                        Ty::Int(_) => kind_code(builtin.clone()) + UNSIGNED_SOURCE,
                        _ => 11,
                    };
                    value = self.prelude_op(PreludeOp::Cast, [value, code.to_string().into()]);
                }
                return Ok(self.operator_expr(value, builtin, expression));
            }
            // `f32::from(degrees)`: the source type's `into` that gives the
            // target, as a crate's `impl From<Degrees> for f32` makes it,
            // since rustc's `Into` comes from `From`. A generic body's
            // facade calls carry witnesses, so it calls the type's own
            // `into`.
            if let Ty::Host { .. } = &source {
                let (forms, owner) = self.conversion_forms(&source).unwrap_or_default();
                let gives = forms.iter().any(|item| {
                    let checkpoint = self.inference.checkpoint();
                    let fits = self
                        .instantiate_host_signature(item, Some(&source), Some(&[]), Span::NONE)
                        .is_ok_and(|signature| self.unify_type(&signature.output, &builtin));
                    self.inference.rollback(checkpoint);
                    fits
                });
                if gives && self.host_witnesses.is_empty() {
                    let value = self.parse_target_into(
                        argument.clone(),
                        &source,
                        forms,
                        owner,
                        expression.end(),
                    );
                    self.unify_type(&value.ty, &builtin);
                    return Ok(self.operator_expr(value.rendered_source(), builtin, expression));
                }
                if gives {
                    self.pending_arguments = Some((Vec::new(), expression.end()));
                    let value =
                        self.parse_host_method(argument.clone(), "into".into(), expression)?;
                    return Ok(self.operator_expr(value.rendered_source(), builtin, expression));
                }
                #[cfg(feature = "rust-import")]
                if forms.is_empty() {
                    self.demand_into(&source);
                }
            }
            if let Some(op) = self.std_conversion(&builtin, &source) {
                let value = match op {
                    Some(op) => self.prelude_op(op, [argument.rendered_source()]),
                    None => argument.rendered_source(),
                };
                return Ok(self.operator_expr(value, builtin, expression));
            }
            if let Some(error) = self.conversion_not_yet(&builtin, &source, expression) {
                return Err(error);
            }
            return Err(Error::MissingCapability {
                owner: name,
                capability: format!("From<{}>", source.name(&self.models)).into(),
                operation: expression,
                derive: false,
                required_by: None,
            });
        }
        // A crate trait's function without `self` for this type.
        let found = owner
            .and_then(|id| self.models.trait_methods.get(&(id, member.clone())))
            .and_then(|definitions| definitions.first())
            .and_then(|definition| definition.trait_name().cloned())
            .and_then(|trait_name| {
                let (_, declared) = self.models.traits.get(&trait_name)?;
                let function = declared
                    .functions
                    .iter()
                    .find(|function| function.name.text == member && function.receiver.is_none())?
                    .clone();
                Some((trait_name, function))
            });
        let (Some((trait_name, function)), Some(owner)) = (found, owner) else {
            return Err(Error::UnknownMember {
                owner: name,
                member,
                reference: member_span,
            });
        };
        self.call_static_trait_function(
            builtin,
            origin::Fragment::plain(owner_key(owner)),
            &trait_name,
            &function,
            arguments,
            expression,
        )
    }

    /// A number literal's type: its suffix's (`3usize`, `2.5f32`, `1f64`,
    /// the name the tokenizer leaves after the digits), else a fresh
    /// `{integer}` or `{float}`; whether an integer is written as a float.
    /// A suffixed integer must fit its kind, as Rust's lint denies.
    fn literal_kind(&mut self, value: INT, integer: bool, span: Span) -> Result<(Ty, bool), Error> {
        if let Token::Identifier(name) = self.peek()
            && let Some(ty) = numeric_type(name.as_str())
            && (integer || matches!(ty, Ty::Float(_)))
        {
            self.next();
            if let Ty::Int(kind) = ty
                && Wide::Signed(value.into()).fits(kind).is_err()
            {
                return Err(self.overflowing(kind, Wide::Signed(value.into()), span));
            }
            let float = integer && matches!(ty, Ty::Float(_));
            return Ok((ty, float));
        }
        let id = self.inference.fresh_id();
        self.literal_kinds.push((id, integer));
        if integer {
            self.literal_values.push((id, value, span));
        }
        Ok((
            if integer {
                Ty::Int(Int::Var(id))
            } else {
                Ty::Float(Float::Var(id))
            },
            false,
        ))
    }

    fn literal(&self, source: impl Into<String>, ty: Ty, span: Span) -> ParsedExpr {
        ParsedExpr {
            prelude: origin::Fragment::default(),
            source: origin::Fragment::plain(source.into()),
            ty,
            span,
            lvalue: None,
            outcome: crate::host::usage::Outcome::Conservative,
            evaluation: crate::host::usage::Evaluation::Required,
        }
        .retain_origin()
    }

    /// A `fmt` body returns the text it wrote: its `fmt::Result` tail is
    /// evaluated, then the formatter is the function's value.
    fn wrap_formatter(&mut self, body: Span) {
        let Some(formatter) = self.formatter.clone() else {
            return;
        };
        let (value, original) = match self.last_expression.take() {
            Some((start, original)) => {
                let mut value = self.output.split_off(start);
                value.trim_end();
                (value, original)
            }
            None => (origin::Fragment::plain("()"), body),
        };
        let mut rendered = origin::Fragment::plain("{ let __rawr_fmt_result = ");
        rendered.push(value);
        rendered.push_text(format!("; {formatter} }}"));
        self.write_output(rendered, original);
    }

    fn write_output(&mut self, rendered: impl Into<origin::Fragment>, original: Span) -> usize {
        let start = self.output.len();
        let mut rendered = rendered.into();
        rendered.cover(original);
        self.output.push(rendered);
        self.output.push_text("\n");
        start
    }

    fn private_call(
        &mut self,
        helper: &str,
        operands: impl IntoIterator<Item = ParsedExpr>,
    ) -> (origin::Fragment, origin::Fragment) {
        // Keep compiler-private legacy source shallow without changing the user's
        // Engine limits. Each pushed operation owns one unique helper spelling,
        // so its operand indexes also form collision-free temporary names.
        if !self.normalize_private_calls {
            // Legacy switch guards reject statement-block expressions. Retain
            // their earlier inline shape so guarded work stays branch-local.
            let operands = operands
                .into_iter()
                .map(|operand| {
                    debug_assert!(operand.prelude.is_empty());
                    operand.source
                })
                .collect::<Vec<_>>();
            return (origin::Fragment::default(), self.op_call(helper, operands));
        }
        let (prelude, operands) = self.private_operands(helper, operands);
        (prelude, self.op_call(helper, operands))
    }

    /// Each operand of a pushed operation evaluated once, in order, into a
    /// local of its own.
    fn private_operands(
        &mut self,
        helper: &str,
        operands: impl IntoIterator<Item = ParsedExpr>,
    ) -> (origin::Fragment, Vec<origin::Fragment>) {
        let mut prelude = origin::Fragment::default();
        let operands = operands
            .into_iter()
            .enumerate()
            .map(|(index, operand)| {
                self.materialize(operand, &format!("{helper}_arg_{index}"), &mut prelude)
            })
            .collect::<Vec<_>>();
        (prelude, operands)
    }

    /// The call of a pushed operation. A host operation in a body whose
    /// signature names a generic inside a host type also takes those
    /// generics' witnesses, which instantiate its proof at run time.
    fn op_call(
        &mut self,
        helper: &str,
        operands: impl IntoIterator<Item = origin::Fragment>,
    ) -> origin::Fragment {
        let mut operands = operands.into_iter().collect::<Vec<_>>();
        if self.host_witnesses.is_empty() {
            return origin::Fragment::call(helper, operands);
        }
        let index = helper
            .strip_prefix(SENTINEL)
            .and_then(|index| index.parse::<usize>().ok())
            .and_then(|index| index.checked_sub(self.op_offset));
        if let Some(index) = index
            && matches!(
                self.ops.get(index),
                Some(Op::Intrinsic { .. } | Op::Projection { .. })
            )
        {
            self.capture_witnesses();
            operands.extend(
                self.host_witnesses
                    .iter()
                    .map(|name| origin::Fragment::plain(witness_variable(name))),
            );
            self.witnessed_ops
                .insert(index, self.host_witnesses.clone().into_boxed_slice());
        }
        origin::Fragment::call(helper, operands)
    }

    fn materialize(
        &mut self,
        operand: ParsedExpr,
        temporary: &str,
        prelude: &mut origin::Fragment,
    ) -> origin::Fragment {
        // Invocation authority may only remain the exact current formal. It must
        // never acquire an alias, even in compiler-private generated source.
        if operand.ty.invocation().is_some() {
            debug_assert!(
                operand.prelude.is_empty(),
                "invocation authority must remain an unadorned current formal"
            );
            return operand.rendered_source();
        }
        prelude.push(operand.prelude);
        let mut statement = origin::Fragment::plain(format!("let {temporary} = "));
        statement.push(operand.source);
        statement.push_text(";\n");
        prelude.push(statement);
        // The operand's local is the call's alone: a value that moves goes
        // into the call, as Rust moves it, and the local keeps no copy of
        // it (#698). A `vec![Family::new(..)]` given to a crate function
        // then gives the function the families themselves. So does a
        // `&mut PathBuf` a crate's call takes moved in (`moved_input`).
        if self.moves_by_value(&operand.ty) || moved_input(&self.inference, &operand.ty) {
            return origin::Fragment::call(
                crate::engine::KEYWORD_MOVE,
                [origin::Fragment::plain(temporary)],
            );
        }
        origin::Fragment::plain(temporary)
    }

    fn materialize_in_place(
        &mut self,
        operand: &mut ParsedExpr,
        temporary: &str,
    ) -> origin::Fragment {
        if operand.ty.invocation().is_some() {
            let rendered = operand.rendered_source();
            operand.prelude.clear();
            return rendered;
        }
        let source = std::mem::take(&mut operand.source);
        let mut statement = origin::Fragment::plain(format!("let {temporary} = "));
        statement.push(source);
        statement.push_text(";\n");
        operand.prelude.push(statement);
        origin::Fragment::plain(temporary)
    }

    fn push_op(&mut self, op: Op) -> String {
        let name = format!("{SENTINEL}{}", self.op_offset + self.ops.len());
        self.ops.push(op);
        name
    }

    fn expect_name(&mut self, construct: &str) -> Result<(ImmutableString, Span), Error> {
        let (token, position) = self.next();
        match token {
            Token::Identifier(name)
                if name.starts_with(SENTINEL)
                    || name.starts_with(METHOD_SENTINEL)
                    || name.starts_with(NOMINAL_MATCH_SENTINEL) =>
            {
                Err(Error::UnsupportedNominalSyntax {
                    construct: "compiler-private nominal intrinsic".into(),
                    source: point_span(position),
                })
            }
            Token::Identifier(name) => Ok(((*name).into(), point_span(position))),
            Token::Reserved(name) if is_valid_identifier(&name) => {
                Ok(((*name).into(), point_span(position)))
            }
            token => Err(Error::UnsupportedNominalSyntax {
                construct: format!("{construct}; found `{token}`").into(),
                source: point_span(position),
            }),
        }
    }

    fn expect_qualified_name(&mut self) -> Result<(ImmutableString, Span), Error> {
        let (token, position) = self.next();
        match token {
            Token::Identifier(name) | Token::Reserved(name) => {
                Ok(((*name).into(), point_span(position)))
            }
            Token::SelfValue => Err(Error::StdPath {
                code: "E0433",
                message: "`self` in paths can only be used in start position or last position"
                    .into(),
                label: Some("can only be used in path start position or last position".into()),
                note: None,
                reference: Span::new(position, text_end(position, "self")),
            }),
            token => Err(Error::UnsupportedNominalSyntax {
                construct: format!("qualified nominal member; found `{token}`").into(),
                source: point_span(position),
            }),
        }
    }

    fn expect_member_name(&mut self) -> Result<(ImmutableString, Span), Error> {
        let (token, position) = self.next();
        match token {
            Token::Identifier(name) | Token::Reserved(name) => {
                Ok(((*name).into(), point_span(position)))
            }
            Token::IntegerConstant(index) if index >= 0 => {
                Ok((index.to_string().into(), point_span(position)))
            }
            // `t.0.1`: the number `0.1` is two tuple fields.
            Token::FloatConstant(ref number)
                if let Some((first, second)) = number.1.split_once('.')
                    && !first.is_empty()
                    && first.bytes().all(|byte| byte.is_ascii_digit())
                    && !second.is_empty()
                    && second.bytes().all(|byte| byte.is_ascii_digit()) =>
            {
                let second_position = Position::new(
                    position.line().unwrap_or(0) as u16,
                    (position.position().unwrap_or(0) + first.len() + 1) as u16,
                );
                self.pending_member = Some((second.into(), point_span(second_position)));
                Ok((first.into(), point_span(position)))
            }
            token => Err(Error::UnsupportedNominalSyntax {
                construct: format!("field or capability method; found `{token}`").into(),
                source: point_span(position),
            }),
        }
    }

    fn expect(&mut self, expected: Token, construct: &str) -> Result<Position, Error> {
        let (token, position) = self.next();
        if token == expected {
            Ok(position)
        } else {
            Err(Error::UnsupportedNominalSyntax {
                construct: format!("{construct}; found `{token}`").into(),
                source: point_span(position),
            })
        }
    }

    fn unsupported(&mut self, construct: &str) -> Error {
        Error::UnsupportedNominalSyntax {
            construct: construct.into(),
            source: point_span(self.position()),
        }
    }

    /// The `>` closing generic arguments, splitting a `>>`.
    fn expect_close_angle(&mut self, construct: &str) -> Result<Position, Error> {
        if matches!(self.peek(), Token::RightShift) {
            let (_, position) = self.next();
            let mut second = position;
            second.advance();
            self.split_angle = Some(second);
            return Ok(position);
        }
        self.expect(Token::GreaterThan, construct)
    }

    /// The token after the next one.
    fn peek_second(&mut self) -> Token {
        let mut ahead = self.input.clone();
        ahead.next();
        ahead.next().map_or(Token::EOF, |(token, _)| token)
    }

    fn peek_identifier(&mut self) -> Option<&str> {
        if self.split_angle.is_some() {
            return None;
        }
        match self.input.peek() {
            Some((Token::Identifier(name), _)) => Some(name),
            _ => None,
        }
    }

    fn peek(&mut self) -> Token {
        if self.split_angle.is_some() {
            return Token::GreaterThan;
        }
        self.input
            .peek()
            .expect("token streams always end with EOF")
            .0
            .clone()
    }

    fn position(&mut self) -> Position {
        if let Some(position) = self.split_angle {
            return position;
        }
        self.input
            .peek()
            .expect("token streams always end with EOF")
            .1
    }

    fn next(&mut self) -> (Token, Position) {
        let (token, position) = match self.split_angle.take() {
            Some(position) => (Token::GreaterThan, position),
            None => self
                .input
                .next()
                .expect("token streams always end with EOF"),
        };
        self.track_calls(&token, position);
        self.models.at = position;
        (token, position)
    }
}

/// The blocks of the import at `span` among a source's `imports`, and of
/// each import there that binds what it binds, which hoisting merged into
/// it: `None` when one of them is the module's.
fn use_blocks(imports: &[crate::item::Use], span: Span) -> Option<Vec<Span>> {
    let key = imports.iter().find(|used| used.span == span)?.hoist_key();
    imports
        .iter()
        .filter(|used| used.hoist_key() == key)
        .map(|used| used.scope)
        .collect()
}

/// Each name of an imported enum variant (`use Shape::*;`) as its path,
/// `Shape::Circle`, where a path may start: not after `.` or `::`, and not
/// before a `:` that names a field or a `::` that goes on to a name. A
/// turbofish (`Empty::<i32>`) stays on the variant.
fn expand_imported_variants(
    models: &Models,
    tokens: Vec<(Token, Position)>,
) -> Vec<(Token, Position)> {
    let mut expanded = Vec::with_capacity(tokens.len());
    for index in 0..tokens.len() {
        let (token, position) = &tokens[index];
        let name = match token {
            Token::Identifier(name)
                if !matches!(
                    expanded.last(),
                    Some((Token::Period | Token::DoubleColon, _))
                ) && !matches!(
                    (tokens.get(index + 1), tokens.get(index + 2)),
                    (Some((Token::Colon, _)), _)
                        | (
                            Some((Token::DoubleColon, _)),
                            Some((Token::Identifier(_), _))
                        )
                ) =>
            {
                Some(name)
            }
            _ => None,
        };
        // A crate enum's variant is its enum's, `Alignment::Center`.
        if let Some(path) = name.and_then(|name| models.imported_host_variant(name, *position)) {
            for (segment, name) in path.iter().enumerate() {
                if segment > 0 {
                    expanded.push((Token::DoubleColon, *position));
                }
                expanded.push((Token::Identifier(Box::new(name.as_str().into())), *position));
            }
            continue;
        }
        let owner = name.and_then(|name| models.imported_variant(name, *position));
        for segment in owner.into_iter().flatten() {
            expanded.push((
                Token::Identifier(Box::new(segment.as_str().into())),
                *position,
            ));
            expanded.push((Token::DoubleColon, *position));
        }
        expanded.push((token.clone(), *position));
    }
    expanded
}

/// An impl's where-clause bounds at an instance (D604): each bounded
/// type, the bound's trait identity and the trait's arguments, then each
/// bound that fixes an associated type (`ImplProjection`).
#[cfg(feature = "rust-import")]
struct ImplBounds {
    traits: Vec<(Ty, [u8; 32], Vec<Ty>)>,
    projections: Vec<ImplProjection>,
}

/// A bound `<ty as Trait>::Name == value` of an impl at an instance, with
/// the associated type each type `ty` may be gives (`facade::
/// ImplProjection`).
#[cfg(feature = "rust-import")]
#[derive(Debug)]
struct ImplProjection {
    ty: Ty,
    value: Ty,
    cases: Box<[(Ty, Ty)]>,
}

/// Why no impl fixed an `impl Trait`'s hidden type (D604): none fits the
/// type named, or more than one does.
#[cfg(feature = "rust-import")]
enum Selection {
    None(String),
    Many(String),
}

/// Whether a type still has an inference variable in it.
/// Whether `ty` holds a generic parameter of the body being checked where
/// a crate's item would take it as a type of its own, not as a script
/// value: the parameter itself, or inside a crate type, a borrow, an
/// `Option`, a `Result`, a tuple or a collection.
#[cfg(feature = "rust-import")]
fn reaches_param(ty: &Ty) -> bool {
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
fn mentions_script(ty: &Ty) -> bool {
    match ty {
        Ty::Script(..) => true,
        Ty::Reference { target, .. } | Ty::Boxed(target) => mentions_script(target),
        Ty::Host { arguments, .. } => arguments.iter().any(mentions_script),
        _ => false,
    }
}

fn contains_inference(ty: &Ty) -> bool {
    contains(ty, &|ty| matches!(ty, Ty::Infer(_)))
}

/// The first unknown type inside `ty`, outermost first.
#[cfg(feature = "rust-import")]
fn first_inference(ty: &Ty) -> Option<Ty> {
    let found = std::cell::RefCell::new(None);
    contains(ty, &|inner| {
        matches!(inner, Ty::Infer(_)) && found.replace(Some(inner.clone())).is_none()
    });
    found.into_inner()
}

/// Whether `ty` holds a literal whose kind is still open: `{integer}` or
/// `{float}`, which only the end of its body settles.
fn contains_open_literal(ty: &Ty) -> bool {
    contains(ty, &|ty| {
        matches!(ty, Ty::Int(Int::Var(_)) | Ty::Float(Float::Var(_)))
    })
}

/// Whether `ty` is itself a variable inference has not settled: an
/// unknown type, or a literal whose kind is still open.
fn is_unsettled(ty: &Ty) -> bool {
    matches!(
        ty,
        Ty::Infer(_) | Ty::Int(Int::Var(_)) | Ty::Float(Float::Var(_))
    )
}

/// Whether `ty` holds a variable inference has not settled: an unknown
/// type, or a literal whose kind is still open.
fn contains_unsettled(ty: &Ty) -> bool {
    contains_inference(ty) || contains_open_literal(ty)
}

/// Whether `ty` or a type inside it is one `leaf` picks.
fn contains(ty: &Ty, leaf: &dyn Fn(&Ty) -> bool) -> bool {
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

/// A loop being parsed. A `break` or `continue` to an enclosing loop sets
/// that loop's jump flag and breaks out of every loop in between.
struct LoopFrame {
    label: Option<ImmutableString>,
    jump: String,
    /// `loop`, which may `break` with a value.
    value: bool,
    ty: Option<Ty>,
    /// A nested loop jumps to this one.
    targeted: bool,
    /// Enclosing loops (by depth) that a jump inside this loop targets.
    exits: BTreeSet<usize>,
    /// The depth of `drop_scopes` outside the loop's body: a jump to the
    /// loop drops the scopes above it.
    drop_depth: usize,
    /// The loop's depth in the recorded control flow, which also counts a
    /// `while` whose condition holds this loop.
    flow_depth: usize,
    /// While a `while`'s condition is read: whether a labeled jump in it
    /// goes to this `while`.
    condition: Option<bool>,
}

/// The local a closure reads `self` through.
const SELF_CAPTURE: &str = "__rawr_self";

/// `lvalue` (a field, a local's element or a nested place) followed by
/// `step`: a nested place of type `ty`.
fn nested_step(lvalue: &Lvalue, step: PlaceStep, ty: Ty) -> Lvalue {
    let mut nested = match lvalue {
        Lvalue::Nested(nested) => (**nested).clone(),
        Lvalue::Field(field) => lvalue::Nested {
            binding: field.binding.clone(),
            base: origin::Fragment::plain(field.binding.to_string()),
            steps: vec![PlaceStep::Field { slot: field.slot }],
            ty: field.ty.clone(),
        },
        Lvalue::Place { binding, place, .. } => lvalue::Nested {
            binding: binding.clone(),
            base: place.clone(),
            steps: Vec::new(),
            ty: ty.clone(),
        },
        Lvalue::Binding { .. } | Lvalue::Slice(_) => {
            unreachable!("only fields, elements and nested places extend")
        }
    };
    nested.steps.push(step);
    nested.ty = ty;
    Lvalue::Nested(Box::new(nested))
}

/// The field at `slot` of the struct `base` is, as the runtime reaches it in
/// place: `base[slot]`. A local or a chain of parts below one stays a chain,
/// so `s.a.b` is the place `s[0][1]`; any other value is indexed as a
/// temporary, `(base)[slot]`.
fn field_place(base: origin::Fragment, slot: usize) -> origin::Fragment {
    let mut place = if is_place_chain(base.as_str()) {
        base
    } else {
        base.surrounded("(", ")")
    };
    place.push_text(format!("[{slot}]"));
    place
}

/// The place `value` reads, as the runtime reaches it where it is: a local
/// by its name, an element (`v[i]`), a field (`s[1]`), or a chain of fields,
/// members and elements below a local. `None` for any other value, or for
/// one read through a prelude.
fn in_place(value: &ParsedExpr) -> Option<origin::Fragment> {
    if !value.prelude.is_empty() {
        return None;
    }
    match value.lvalue.as_ref()? {
        Lvalue::Binding { binding, .. } => {
            (value.source.as_str() == binding.as_str()).then(|| value.source.clone())
        }
        Lvalue::Place { place, .. } => Some(place.clone()),
        Lvalue::Field(field) => Some(field.place()),
        Lvalue::Nested(nested) => {
            let mut place = nested.base.clone();
            for step in &nested.steps {
                match step {
                    PlaceStep::Field { slot } => place = field_place(place, *slot),
                    PlaceStep::Index(index) => {
                        place.push_text("[");
                        place.push(index.clone());
                        place.push_text("]");
                    }
                    PlaceStep::Member(member) => place.push_text(format!("[{member}]")),
                    _ => return None,
                }
            }
            Some(place)
        }
        Lvalue::Slice(_) => None,
    }
}

/// The local and the place of `value` when it is a chain of fields and
/// members below a local (`s.a.0` is `s[0][0]`), each slot a literal.
fn fields_below_local(value: &ParsedExpr) -> Option<(ImmutableString, origin::Fragment)> {
    match value.lvalue.as_ref()? {
        Lvalue::Field(field) => Some((field.binding.clone(), field.place())),
        Lvalue::Nested(nested)
            if nested
                .steps
                .iter()
                .all(|step| matches!(step, PlaceStep::Field { .. } | PlaceStep::Member(_))) =>
        {
            let base = nested.base.as_str();
            if !is_place_chain(base) {
                return None;
            }
            let root = base.trim_start_matches('(');
            let root = &root[..root
                .find(|character: char| character != '_' && !character.is_alphanumeric())
                .unwrap_or(root.len())];
            Some((root.into(), in_place(value)?))
        }
        _ => None,
    }
}

/// Whether `source` is a local's name followed by indexes only:
/// `s`, `this`, `s[0][i + 1]`, `(s)[0]`.
fn is_place_chain(source: &str) -> bool {
    // `(s)` reads as `s`.
    let (name, rest) = match source.strip_prefix('(') {
        Some(inner) => match inner.split_once(')') {
            Some(parts) => parts,
            None => return false,
        },
        None => source.split_at(
            source
                .find(|character: char| character != '_' && !character.is_alphanumeric())
                .unwrap_or(source.len()),
        ),
    };
    if name.is_empty()
        || name.starts_with(|character: char| character.is_ascii_digit())
        || variable_name(name).is_none()
    {
        return false;
    }
    let mut depth = 0usize;
    for character in rest.chars() {
        match character {
            '[' => depth += 1,
            ']' => match depth.checked_sub(1) {
                Some(outer) => depth = outer,
                None => return false,
            },
            '"' | '\'' => return false,
            _ if depth == 0 => return false,
            _ => {}
        }
    }
    depth == 0
}

fn variable_name(source: &str) -> Option<ImmutableString> {
    source
        .chars()
        .all(|character| character == '_' || character.is_alphanumeric())
        .then(|| source.into())
}

fn default_value(
    model: &Nominal,
    models: &Models,
    _stack: &mut Vec<type_def::Id>,
) -> Option<Dynamic> {
    let definition = model.definition();
    let identity = type_def::Identity::Script(definition.key().clone());
    models
        .runtime
        .materialize_default(&identity, definition.schema())
        .ok()
}

/// Rust's `PartialOrd` of builtin values: numbers, strings, chars, bools
/// and `()`, `Option` (`None` first) and arrays and tuples element by
/// element, and of script types deriving it. `None` when unordered: a NaN, or values Rawr's checks never
/// let reach here.
fn compare_value(lhs: &Dynamic, rhs: &Dynamic, models: &Models) -> Option<std::cmp::Ordering> {
    // Element by element, as Rust's derive and slices compare.
    let lexicographic = |lhs: &[Dynamic], rhs: &[Dynamic]| {
        for (lhs, rhs) in lhs.iter().zip(rhs) {
            match compare_value(lhs, rhs, models)? {
                std::cmp::Ordering::Equal => {}
                ordering => return Some(ordering),
            }
        }
        Some(lhs.len().cmp(&rhs.len()))
    };
    if let (Some(lhs), Some(rhs)) = (crate::option::read(lhs), crate::option::read(rhs)) {
        return match (lhs, rhs) {
            (Some(lhs), Some(rhs)) => compare_value(&lhs, &rhs, models),
            (lhs, rhs) => Some(lhs.is_some().cmp(&rhs.is_some())),
        };
    }
    // `Ok` before `Err`, then their values, as std's derive orders them.
    if let (Some(lhs), Some(rhs)) = (crate::result::read(lhs), crate::result::read(rhs)) {
        return match (lhs, rhs) {
            (Ok(lhs), Ok(rhs)) | (Err(lhs), Err(rhs)) => compare_value(&lhs, &rhs, models),
            (lhs, rhs) => Some(lhs.is_err().cmp(&rhs.is_err())),
        };
    }
    if let (Some(lhs), Some(rhs)) = (
        lhs.read_lock::<std::time::Duration>(),
        rhs.read_lock::<std::time::Duration>(),
    ) {
        return Some(lhs.cmp(&rhs));
    }
    if let (Some(lhs), Some(rhs)) = (
        lhs.read_lock::<crate::Instant>(),
        rhs.read_lock::<crate::Instant>(),
    ) {
        return Some(lhs.cmp(&rhs));
    }
    if let (Some(lhs), Some(rhs)) = (cell::value(lhs), cell::value(rhs)) {
        return compare_value(&lhs, &rhs, models);
    }
    if let (Some(lhs), Some(rhs)) = (counted::value(lhs), counted::value(rhs)) {
        return compare_value(&lhs, &rhs, models);
    }
    // `Reverse(a) < Reverse(b)` when `b < a`.
    if let (Some(lhs), Some(rhs)) = (
        lhs.read_lock::<std::cmp::Reverse<Dynamic>>(),
        rhs.read_lock::<std::cmp::Reverse<Dynamic>>(),
    ) {
        return compare_value(&rhs.0, &lhs.0, models);
    }
    // A `Cow` orders as what it derefs to.
    if let (Some(lhs), Some(rhs)) = (cow::payload(lhs), cow::payload(rhs)) {
        return compare_value(&lhs, &rhs, models);
    }
    // An `i128` or a `u128` past `i64`, boxed.
    if (matches!(lhs.0, Union::Variant(..)) || matches!(rhs.0, Union::Variant(..)))
        && let Some(ordering) = wide_order(lhs, rhs)
    {
        return Some(ordering);
    }
    match (&lhs.0, &rhs.0) {
        (Union::Unit(..), Union::Unit(..)) => Some(std::cmp::Ordering::Equal),
        (Union::Bool(lhs, ..), Union::Bool(rhs, ..)) => Some(lhs.cmp(rhs)),
        (Union::Char(lhs, ..), Union::Char(rhs, ..)) => Some(lhs.cmp(rhs)),
        // A `u64` a generic parameter holds is tagged as one.
        (Union::Int(lhs, tag, ..), Union::Int(rhs, other, ..))
            if *tag == crate::types::dynamic::UNSIGNED_TAG
                || *other == crate::types::dynamic::UNSIGNED_TAG =>
        {
            Some((*lhs as u64).cmp(&(*rhs as u64)))
        }
        (Union::Int(lhs, ..), Union::Int(rhs, ..)) => Some(lhs.cmp(rhs)),
        (Union::Str(lhs, ..), Union::Str(rhs, ..)) => Some(lhs.as_str().cmp(rhs.as_str())),
        (Union::Float(lhs, ..), Union::Float(rhs, ..)) => (**lhs).partial_cmp(&**rhs),
        (Union::Array(lhs, ..), Union::Array(rhs, ..)) => lexicographic(lhs, rhs),
        // An imported value keying a hashed collection.
        (Union::Variant(..), Union::Variant(..)) => crate::host::value::host_compare(lhs, rhs),
        // A derived `PartialOrd`: fields in declaration order; an enum's
        // variants by declaration order, then their payloads.
        (Union::Struct(lhs, ..), Union::Struct(rhs, ..)) if lhs.identity() == rhs.identity() => {
            match models.by_identity.get(lhs.identity()) {
                Some(Nominal::Struct { item, .. }) => typed_lexicographic(
                    lhs.fields(),
                    rhs.fields(),
                    item.fields.iter().map(|field| &field.ty),
                    models,
                ),
                _ => lexicographic(lhs.fields(), rhs.fields()),
            }
        }
        (Union::Enum(lhs, ..), Union::Enum(rhs, ..)) if lhs.identity() == rhs.identity() => {
            let Some(Nominal::Enum { item, .. }) = models.by_identity.get(lhs.identity()) else {
                return None;
            };
            let index = |name: &str| {
                item.variants
                    .iter()
                    .position(|variant| variant.name.text == name)
            };
            let variant = index(lhs.variant().name())?;
            match variant.cmp(&index(rhs.variant().name())?) {
                std::cmp::Ordering::Equal => typed_lexicographic(
                    lhs.payload(),
                    rhs.payload(),
                    item.variants[variant].payload.iter(),
                    models,
                ),
                ordering => Some(ordering),
            }
        }
        _ => None,
    }
}

/// Fields or payloads compared in order, a `u64` or `usize` one as the
/// unsigned number its bits are.
fn typed_lexicographic<'t>(
    lhs: &[Dynamic],
    rhs: &[Dynamic],
    types: impl Iterator<Item = &'t type_ref::Type>,
    models: &Models,
) -> Option<std::cmp::Ordering> {
    for ((lhs, rhs), ty) in lhs.iter().zip(rhs).zip(types) {
        let ordering = match (ty, &lhs.0, &rhs.0) {
            (type_ref::Type::Path(path), Union::Int(lhs, ..), Union::Int(rhs, ..))
                if path.target == type_ref::Target::Builtin
                    && matches!(path.name.as_str(), "u64" | "usize") =>
            {
                (*lhs as u64).cmp(&(*rhs as u64))
            }
            _ => compare_value(lhs, rhs, models)?,
        };
        if ordering != std::cmp::Ordering::Equal {
            return Some(ordering);
        }
    }
    Some(lhs.len().cmp(&rhs.len()))
}

pub(crate) fn equal_value(lhs: &Dynamic, rhs: &Dynamic) -> bool {
    if let (Some(lhs), Some(rhs)) = (cell::value(lhs), cell::value(rhs)) {
        return equal_value(&lhs, &rhs);
    }
    if let (Some(lhs), Some(rhs)) = (counted::value(lhs), counted::value(rhs)) {
        return equal_value(&lhs, &rhs);
    }
    if let (Some(lhs), Some(rhs)) = (
        lhs.read_lock::<std::time::Duration>(),
        rhs.read_lock::<std::time::Duration>(),
    ) {
        return *lhs == *rhs;
    }
    if let (Some(lhs), Some(rhs)) = (
        lhs.read_lock::<crate::Instant>(),
        rhs.read_lock::<crate::Instant>(),
    ) {
        return *lhs == *rhs;
    }
    if let (Some(lhs), Some(rhs)) = (
        lhs.read_lock::<std::cmp::Reverse<Dynamic>>(),
        rhs.read_lock::<std::cmp::Reverse<Dynamic>>(),
    ) {
        return equal_value(&lhs.0, &rhs.0);
    }
    // A `Cow` compares what it derefs to, `Borrowed` or `Owned`.
    if let (Some(lhs), Some(rhs)) = (cow::payload(lhs), cow::payload(rhs)) {
        return equal_value(&lhs, &rhs);
    }
    if let (Some(lhs), Some(rhs)) = (crate::option::read(lhs), crate::option::read(rhs)) {
        return match (lhs, rhs) {
            (Some(lhs), Some(rhs)) => equal_value(&lhs, &rhs),
            (lhs, rhs) => lhs.is_none() && rhs.is_none(),
        };
    }
    if let (Some(lhs), Some(rhs)) = (crate::result::read(lhs), crate::result::read(rhs)) {
        return match (lhs, rhs) {
            (Ok(lhs), Ok(rhs)) | (Err(lhs), Err(rhs)) => equal_value(&lhs, &rhs),
            _ => false,
        };
    }
    match (&lhs.0, &rhs.0) {
        (Union::Unit(..), Union::Unit(..)) => true,
        (Union::Bool(lhs, ..), Union::Bool(rhs, ..)) => lhs == rhs,
        (Union::Char(lhs, ..), Union::Char(rhs, ..)) => lhs == rhs,
        (Union::Int(lhs, ..), Union::Int(rhs, ..)) => lhs == rhs,
        (Union::Str(lhs, ..), Union::Str(rhs, ..)) => lhs == rhs,
        (Union::Float(lhs, ..), Union::Float(rhs, ..)) => lhs == rhs,
        (Union::Struct(lhs, ..), Union::Struct(rhs, ..)) => {
            lhs.identity() == rhs.identity()
                && lhs.schema() == rhs.schema()
                && lhs.fields().len() == rhs.fields().len()
                && lhs
                    .fields()
                    .iter()
                    .zip(rhs.fields())
                    .all(|(lhs, rhs)| equal_value(lhs, rhs))
        }
        (Union::Enum(lhs, ..), Union::Enum(rhs, ..)) => {
            lhs.identity() == rhs.identity()
                && lhs.schema() == rhs.schema()
                && lhs.variant() == rhs.variant()
                && lhs.payload().len() == rhs.payload().len()
                && lhs
                    .payload()
                    .iter()
                    .zip(rhs.payload())
                    .all(|(lhs, rhs)| equal_value(lhs, rhs))
        }
        // Vectors, arrays and tuples compare element by element.
        (Union::Array(lhs, ..), Union::Array(rhs, ..)) => {
            lhs.len() == rhs.len()
                && lhs
                    .iter()
                    .zip(rhs.iter())
                    .all(|(lhs, rhs)| equal_value(lhs, rhs))
        }
        // An `i128` or a `u128` past `i64`, boxed.
        (Union::Variant(..), Union::Variant(..) | Union::Int(..))
        | (Union::Int(..), Union::Variant(..))
            if let Some(ordering) = wide_order(lhs, rhs) =>
        {
            ordering == std::cmp::Ordering::Equal
        }
        (Union::Variant(..), Union::Variant(..)) => {
            crate::host::value::host_equal(lhs, rhs).unwrap_or(false)
        }
        _ => false,
    }
}

/// Move every position in `ast`'s statements and functions by `locate`:
/// from the lowered program to the script's text.
pub(crate) fn relocate(ast: &mut AST, locate: &mut dyn FnMut(Position) -> Position) {
    relocate_statements(ast.statements_mut(), locate);
    ast.for_each_fn_def_mut(|function| {
        relocate_statements(function.body.statements_mut(), locate);
    });
}

fn relocate_statements(statements: &mut [Stmt], locate: &mut dyn FnMut(Position) -> Position) {
    for statement in statements {
        relocate_statement(statement, locate);
    }
}

fn relocate_block(block: &mut ast::StmtBlock, locate: &mut dyn FnMut(Position) -> Position) {
    let span = block.span();
    block.set_position(locate(span.start()), locate(span.end()));
    relocate_statements(block.statements_mut(), locate);
}

fn relocate_statement(statement: &mut Stmt, locate: &mut dyn FnMut(Position) -> Position) {
    // An expression statement's or a block's position is its expression's
    // or its block's, which are moved below.
    if !matches!(statement, Stmt::Expr(..) | Stmt::Block(..)) {
        let position = locate(statement.position());
        statement.set_position(position);
    }
    match statement {
        Stmt::Var(value, ..) => relocate_expr(&mut value.1, locate),
        Stmt::Assignment(value) => {
            relocate_expr(&mut value.1.lhs, locate);
            relocate_expr(&mut value.1.rhs, locate);
        }
        Stmt::FnCall(call, ..) => relocate_call(call, locate),
        Stmt::Expr(expr) => relocate_expr(expr, locate),
        Stmt::Block(block) => relocate_block(block, locate),
        Stmt::If(flow, ..) | Stmt::While(flow, ..) => {
            relocate_expr(&mut flow.expr, locate);
            relocate_block(&mut flow.body, locate);
            relocate_block(&mut flow.branch, locate);
        }
        Stmt::For(value, ..) => {
            relocate_expr(&mut value.2.expr, locate);
            relocate_block(&mut value.2.body, locate);
        }
        Stmt::Switch(value, ..) => {
            relocate_expr(&mut value.0, locate);
            for expression in &mut value.1.expressions {
                relocate_expr(&mut expression.lhs, locate);
                relocate_expr(&mut expression.rhs, locate);
            }
        }
        Stmt::Return(expr, ..) | Stmt::BreakLoop(expr, ..) => {
            if let Some(expr) = expr {
                relocate_expr(expr, locate);
            }
        }
        _ => {}
    }
}

fn relocate_call(call: &mut FnCallExpr, locate: &mut dyn FnMut(Position) -> Position) {
    for arg in &mut call.args {
        relocate_expr(arg, locate);
    }
}

fn relocate_expr(expr: &mut ast::Expr, locate: &mut dyn FnMut(Position) -> Position) {
    if !matches!(expr, ast::Expr::Stmt(..)) {
        let position = locate(expr.position());
        expr.set_position(position);
    }
    match expr {
        ast::Expr::FnCall(call, ..) | ast::Expr::MethodCall(call, ..) => {
            relocate_call(call, locate)
        }
        ast::Expr::Array(values, ..) => {
            for value in values {
                relocate_expr(value, locate);
            }
        }
        ast::Expr::And(values, ..) | ast::Expr::Or(values, ..) => {
            for value in values.iter_mut() {
                relocate_expr(value, locate);
            }
        }
        ast::Expr::Map(value, ..) => {
            for (_, value) in &mut value.0 {
                relocate_expr(value, locate);
            }
        }
        ast::Expr::Stmt(block) => relocate_block(block, locate),
        ast::Expr::Dot(binary, ..) | ast::Expr::Index(binary, ..) => {
            relocate_expr(&mut binary.lhs, locate);
            relocate_expr(&mut binary.rhs, locate);
        }
        ast::Expr::DynamicConstant(value, ..) => relocate_constant(value, locate),
        _ => {}
    }
}

/// A folded closure's body, or those of the closures an array holds.
fn relocate_constant(value: &mut Dynamic, locate: &mut dyn FnMut(Position) -> Position) {
    if let Some(pointer) = value.downcast_mut::<crate::FnPtr>() {
        if let crate::types::fn_ptr::FnPtrType::Script(definition) = &mut pointer.typ {
            let definition = crate::func::shared_make_mut(definition);
            relocate_statements(definition.body.statements_mut(), locate);
        }
        for value in pointer.curry.iter_mut() {
            relocate_constant(value, locate);
        }
        return;
    }
    if let Some(array) = value.downcast_mut::<crate::Array>() {
        for element in array.iter_mut() {
            relocate_constant(element, locate);
        }
    }
}

fn rewrite_statements(
    statements: &mut [Stmt],
    rewrites: &BTreeMap<ImmutableString, ImmutableString>,
    patterns: &BTreeMap<ImmutableString, NominalPattern>,
) {
    for statement in statements {
        rewrite_statement(statement, rewrites, patterns);
    }
}

fn rewrite_statement(
    statement: &mut Stmt,
    rewrites: &BTreeMap<ImmutableString, ImmutableString>,
    patterns: &BTreeMap<ImmutableString, NominalPattern>,
) {
    match statement {
        Stmt::Var(value, ..) => {
            rewrite_expr(&mut value.1, rewrites, patterns);
            rewrite_variable(&mut value.0.name, rewrites);
        }
        Stmt::Assignment(value) => {
            rewrite_expr(&mut value.1.lhs, rewrites, patterns);
            rewrite_expr(&mut value.1.rhs, rewrites, patterns);
        }
        Stmt::FnCall(call, ..) => rewrite_call(call, rewrites, patterns, false),
        Stmt::Expr(expr) => rewrite_expr(expr, rewrites, patterns),
        Stmt::Block(block) => rewrite_statements(block.statements_mut(), rewrites, patterns),
        Stmt::If(flow, ..) | Stmt::While(flow, ..) => rewrite_flow(flow, rewrites, patterns),
        Stmt::For(value, ..) => {
            rewrite_expr(&mut value.2.expr, rewrites, patterns);
            rewrite_statements(value.2.body.statements_mut(), rewrites, patterns);
        }
        Stmt::Switch(value, ..) => {
            rewrite_expr(&mut value.0, rewrites, patterns);
            for expression in &mut value.1.expressions {
                rewrite_expr(&mut expression.lhs, rewrites, patterns);
                rewrite_expr(&mut expression.rhs, rewrites, patterns);
            }
            for case in &mut value.1.enum_cases {
                if case.enum_name.starts_with(NOMINAL_MATCH_SENTINEL) {
                    let pattern = patterns
                        .get(case.enum_name.as_str())
                        .expect("compiler match sentinel must carry a nominal pattern proof")
                        .clone();
                    match &pattern {
                        NominalPattern::Enum(pattern) => {
                            case.enum_name = pattern.enum_name.clone();
                            case.variant_name = pattern.variant.name().into();
                            case.variant_index = pattern
                                .variants
                                .iter()
                                .position(|variant| variant == pattern.variant.name())
                                .expect("a nominal pattern variant must belong to its proven enum");
                        }
                        NominalPattern::Option(some) => {
                            case.enum_name = "Option".into();
                            case.variant_name = if some.is_some() { "Some" } else { "None" }.into();
                            case.variant_index = usize::from(some.is_some());
                        }
                        NominalPattern::Result(variant) => {
                            case.enum_name = "Result".into();
                            case.variant_name = if variant.is_ok() { "Ok" } else { "Err" }.into();
                            case.variant_index = usize::from(variant.is_err());
                        }
                        _ => {
                            case.enum_name = "_".into();
                            case.variant_name = "_".into();
                            case.variant_index = usize::MAX;
                        }
                    }
                    case.nominal = Some(pattern);
                }
            }
        }
        Stmt::Return(expr, ..) | Stmt::BreakLoop(expr, ..) => {
            if let Some(expr) = expr {
                rewrite_expr(expr, rewrites, patterns);
            }
        }
        Stmt::Noop(..) => {}
        Stmt::Share(..) => {}
    }
}

fn rewrite_flow(
    flow: &mut FlowControl,
    rewrites: &BTreeMap<ImmutableString, ImmutableString>,
    patterns: &BTreeMap<ImmutableString, NominalPattern>,
) {
    rewrite_expr(&mut flow.expr, rewrites, patterns);
    rewrite_statements(flow.body.statements_mut(), rewrites, patterns);
    rewrite_statements(flow.branch.statements_mut(), rewrites, patterns);
}

fn rewrite_expr(
    expr: &mut ast::Expr,
    rewrites: &BTreeMap<ImmutableString, ImmutableString>,
    patterns: &BTreeMap<ImmutableString, NominalPattern>,
) {
    match expr {
        ast::Expr::FnCall(call, ..) => rewrite_call(call, rewrites, patterns, false),
        ast::Expr::MethodCall(call, ..) => rewrite_call(call, rewrites, patterns, true),
        ast::Expr::Array(values, ..) => {
            for value in values {
                rewrite_expr(value, rewrites, patterns);
            }
        }
        ast::Expr::And(values, ..) | ast::Expr::Or(values, ..) => {
            for value in values.iter_mut() {
                rewrite_expr(value, rewrites, patterns);
            }
        }
        ast::Expr::Map(value, ..) => {
            for (_, value) in &mut value.0 {
                rewrite_expr(value, rewrites, patterns);
            }
        }
        ast::Expr::Stmt(block) => rewrite_statements(block.statements_mut(), rewrites, patterns),
        ast::Expr::Dot(binary, ..) | ast::Expr::Index(binary, ..) => {
            rewrite_binary(binary, rewrites, patterns);
        }
        ast::Expr::Variable(variable, ..) => {
            rewrite_variable(&mut variable.1, rewrites);
        }
        // A closure carries its own function definition.
        ast::Expr::DynamicConstant(value, ..) => rewrite_constant(value, rewrites, patterns),
        ast::Expr::BoolConstant(..)
        | ast::Expr::IntegerConstant(..)
        | ast::Expr::CharConstant(..)
        | ast::Expr::StringConstant(..)
        | ast::Expr::Unit(..)
        | ast::Expr::ThisPtr(..)
        | ast::Expr::Property(..) => {}
        ast::Expr::FloatConstant(..) => {}
    }
}

/// A constant the optimizer folded: a closure, or an array holding one
/// (a `fn` pointer choice is `[index, closure]`).
fn rewrite_constant(
    value: &mut Dynamic,
    rewrites: &BTreeMap<ImmutableString, ImmutableString>,
    patterns: &BTreeMap<ImmutableString, NominalPattern>,
) {
    if let Some(pointer) = value.downcast_mut::<crate::FnPtr>() {
        rewrite_pointer(pointer, rewrites, patterns);
        return;
    }
    if let Some(array) = value.downcast_mut::<crate::Array>() {
        for element in array.iter_mut() {
            rewrite_constant(element, rewrites, patterns);
        }
    }
}

/// A closure's definition, and those of the closures it captured, which
/// the optimizer folds into its curried values.
fn rewrite_pointer(
    pointer: &mut crate::FnPtr,
    rewrites: &BTreeMap<ImmutableString, ImmutableString>,
    patterns: &BTreeMap<ImmutableString, NominalPattern>,
) {
    if let crate::types::fn_ptr::FnPtrType::Script(definition) = &mut pointer.typ {
        let definition = crate::func::shared_make_mut(definition);
        rewrite_statements(definition.body.statements_mut(), rewrites, patterns);
    }
    for value in pointer.curry.iter_mut() {
        if let Some(captured) = value.downcast_mut::<crate::FnPtr>() {
            rewrite_pointer(captured, rewrites, patterns);
        }
    }
}

fn rewrite_variable(
    name: &mut ImmutableString,
    rewrites: &BTreeMap<ImmutableString, ImmutableString>,
) {
    let Some((helper, argument)) = name.rsplit_once("_arg_") else {
        return;
    };
    if !helper.starts_with(SENTINEL) || argument.parse::<usize>().is_err() {
        return;
    }
    let Some(private) = rewrites.get(helper) else {
        return;
    };
    *name = format!("{private}.arg.{argument}").into();
}

fn rewrite_binary(
    binary: &mut BinaryExpr,
    rewrites: &BTreeMap<ImmutableString, ImmutableString>,
    patterns: &BTreeMap<ImmutableString, NominalPattern>,
) {
    rewrite_expr(&mut binary.lhs, rewrites, patterns);
    rewrite_expr(&mut binary.rhs, rewrites, patterns);
}

fn rewrite_call(
    call: &mut FnCallExpr,
    rewrites: &BTreeMap<ImmutableString, ImmutableString>,
    patterns: &BTreeMap<ImmutableString, NominalPattern>,
    is_method: bool,
) {
    for arg in &mut call.args {
        rewrite_expr(arg, rewrites, patterns);
    }
    if let Some(name) = rewrites.get(&call.name)
        && name.as_str() == NATIVE_ONLY
    {
        let arguments = call.args.len() + usize::from(is_method);
        call.hashes = FnCallHashes::from_native_only(calc_fn_hash(None, &call.name, arguments));
    } else if let Some(name) = rewrites.get(&call.name) {
        call.name = name.clone();
        call.hashes = if is_method {
            {
                FnCallHashes::from_script_and_native(
                    calc_fn_hash(None, name, call.args.len()),
                    calc_fn_hash(None, name, call.args.len() + 1),
                )
            }
        } else {
            FnCallHashes::from_hash(calc_fn_hash(None, name, call.args.len()))
        };
    }
}

const COMPARISON: u8 = 3;

/// Whether `ty` is one of `allowed`, any integer or float kind standing for
/// every kind of its class.
fn allowed_type(allowed: &[Ty], ty: &Ty) -> bool {
    allowed.iter().any(|allowed| match (allowed, ty) {
        (Ty::Int(_), Ty::Int(_)) | (Ty::Float(_), Ty::Float(_)) => true,
        (allowed, ty) => allowed == ty,
    })
}

/// A numeric kind as the runtime's `Narrow` and `Cast` take it: an integer
/// kind's index in `Int::KINDS`, `10` for `f32`, `11` for `f64`, `12` for
/// `char`.
/// `std::num::ParseIntError` and its kin, the errors of `str::parse`.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
enum ParseError {
    Int,
    Float,
    Bool,
    Char,
    /// `std::convert::Infallible`, the error of `parse::<String>()`.
    Infallible,
    /// `std::string::FromUtf8Error`, the error of `String::from_utf8`.
    Utf8,
    /// `std::num::TryFromIntError`, the error of `u8::try_from(x)` and
    /// `x.try_into()` among integers.
    TryFromInt,
}

impl ParseError {
    fn named(name: &str) -> Option<Self> {
        Some(match name {
            "ParseIntError" => Self::Int,
            "ParseFloatError" => Self::Float,
            "ParseBoolError" => Self::Bool,
            "ParseCharError" => Self::Char,
            "Infallible" => Self::Infallible,
            "FromUtf8Error" => Self::Utf8,
            "TryFromIntError" => Self::TryFromInt,
            _ => return None,
        })
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Int => "ParseIntError",
            Self::Float => "ParseFloatError",
            Self::Bool => "ParseBoolError",
            Self::Char => "ParseCharError",
            Self::Infallible => "Infallible",
            Self::Utf8 => "FromUtf8Error",
            Self::TryFromInt => "TryFromIntError",
        }
    }

    /// The error of parsing into `target`, which Rust's `FromStr` covers.
    fn of(target: &Ty) -> Option<Self> {
        Some(match target {
            Ty::Int(_) => Self::Int,
            Ty::Float(_) => Self::Float,
            Ty::Bool => Self::Bool,
            Ty::Char => Self::Char,
            Ty::String => Self::Infallible,
            _ => return None,
        })
    }

    /// Each message (the error's `Display`, its value at runtime) with the
    /// error's `Debug`, as Rust's std writes them. A `TryFromIntError` is
    /// its `Debug` at runtime instead, and [`Self::display`] its message.
    const fn texts(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Self::Int => &[
                (
                    "cannot parse integer from empty string",
                    "ParseIntError { kind: Empty }",
                ),
                (
                    "invalid digit found in string",
                    "ParseIntError { kind: InvalidDigit }",
                ),
                (
                    "number too large to fit in target type",
                    "ParseIntError { kind: PosOverflow }",
                ),
                (
                    "number too small to fit in target type",
                    "ParseIntError { kind: NegOverflow }",
                ),
            ],
            Self::Float => &[
                (
                    "cannot parse float from empty string",
                    "ParseFloatError { kind: Empty }",
                ),
                ("invalid float literal", "ParseFloatError { kind: Invalid }"),
            ],
            Self::Bool => &[(
                "provided string was not `true` or `false`",
                "ParseBoolError",
            )],
            Self::Char => &[
                (
                    "cannot parse char from empty string",
                    "ParseCharError { kind: EmptyString }",
                ),
                (
                    "too many characters in string",
                    "ParseCharError { kind: TooManyChars }",
                ),
            ],
            // A `TryFromIntError` is its `Debug` text, which names the
            // side it overflowed, and its message is one for both. A
            // `FromUtf8Error` is its `Debug` text, which holds its bytes.
            Self::Infallible | Self::Utf8 | Self::TryFromInt => &[],
        }
    }
}

/// `std::num::TryFromIntError`'s message, its `Display`.
const TRY_FROM_INT_MESSAGE: &str = "out of range integral type conversion attempted";

/// `text.parse()` into the kind `code` (`kind_code`): `Ok` of the value,
/// or `Err` of the std error's message, by Rust's own `FromStr`.
fn parse_text(text: &str, code: INT) -> Dynamic {
    fn result<T, E: std::fmt::Display>(
        parsed: Result<T, E>,
        value: impl FnOnce(T) -> Dynamic,
    ) -> Dynamic {
        match parsed {
            Ok(parsed) => crate::result::ok(value(parsed)),
            Err(error) => crate::result::err(ImmutableString::from(error.to_string()).into()),
        }
    }
    let int = |value: i128| Dynamic::from(value as INT);
    match code {
        10 => result(text.parse::<f32>(), |value| {
            Dynamic::from(crate::FLOAT::from(value))
        }),
        11 => result(text.parse::<crate::FLOAT>(), Dynamic::from),
        12 => result(text.parse::<char>(), Dynamic::from),
        15 => result(text.parse::<bool>(), Dynamic::from),
        16 => crate::result::ok(ImmutableString::from(text).into()),
        code => match Int::from_code(code).map_or("i64", Int::name) {
            "i128" => result(text.parse::<i128>(), wide_i128),
            "u128" => result(text.parse::<u128>(), wide_u128),
            "i8" => result(text.parse::<i8>(), |value| int(value.into())),
            "i16" => result(text.parse::<i16>(), |value| int(value.into())),
            "i32" => result(text.parse::<i32>(), |value| int(value.into())),
            "u8" => result(text.parse::<u8>(), |value| int(value.into())),
            "u16" => result(text.parse::<u16>(), |value| int(value.into())),
            "u32" => result(text.parse::<u32>(), |value| int(value.into())),
            // A `u64` or `usize` is its bits in the runtime's `i64`.
            "u64" | "usize" => result(text.parse::<u64>(), |value| int(value.into())),
            _ => result(text.parse::<i64>(), |value| int(value.into())),
        },
    }
}

/// `CollectStop`'s and `CollectTry`'s code for a `Result` target.
const COLLECT_RESULT: INT = 1;
/// Their code for an `Option` target.
const COLLECT_OPTION: INT = 2;

/// `collect()` into a `Result` or an `Option` (`kinds[0]`) of `items`, which
/// end at the first `Err` or `None` (`CollectStop`): that one, else `Ok` or
/// `Some` of the payloads, concatenated when the payload's kind
/// (`kinds[1]`) is a `String`'s.
fn collect_try(kinds: [INT; 2], items: Dynamic) -> RawrResult {
    let items = items
        .into_array()
        .map_err(|_| invariant("collected items are not an array"))?;
    let mut payloads = crate::Array::with_capacity(items.len());
    for item in items {
        let payload = if kinds[0] == COLLECT_RESULT {
            match crate::result::read(&item) {
                Some(Ok(payload)) => payload,
                Some(Err(_)) => return Ok(item),
                None => return Err(invariant("a collected item is not a `Result`")),
            }
        } else {
            match crate::option::read(&item) {
                Some(Some(payload)) => payload,
                Some(None) => return Ok(item),
                None => return Err(invariant("a collected item is not an `Option`")),
            }
        };
        payloads.push(payload);
    }
    let collected = if kinds[1] == kind_code(Ty::String) {
        let mut text = String::new();
        for payload in &payloads {
            match payload.as_char() {
                Ok(character) => text.push(character),
                Err(_) => text.push_str(&payload.to_string()),
            }
        }
        Dynamic::from(ImmutableString::from(text))
    } else {
        Dynamic::from_array(payloads)
    };
    Ok(if kinds[0] == COLLECT_RESULT {
        crate::result::ok(collected)
    } else {
        crate::option::some(collected)
    })
}

fn kind_code(ty: Ty) -> INT {
    match ty {
        Ty::Int(Int::I128) => WIDE_I128,
        Ty::Int(Int::U128) => WIDE_U128,
        Ty::Int(kind) => Int::KINDS
            .iter()
            .position(|(_, candidate)| *candidate == kind)
            .map_or(3, |index| index as INT),
        Ty::Float(Float::F32) => 10,
        Ty::Char => 12,
        Ty::Bool => 15,
        Ty::String | Ty::Str => 16,
        _ => 11,
    }
}

/// `stringify!`'s text of `tokens` (each with where it starts in `text`),
/// which end where `end` closes the macro, as rustc's `print_tts` prints
/// them: each token as written, and a space after one that the source
/// spaces from the next unless `space_between` says no. `None` when a
/// position is not in `text`.
fn stringify_tokens(text: &str, tokens: &[(Token, Position)], end: Position) -> Option<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut lines = vec![0usize];
    for (index, c) in chars.iter().enumerate() {
        if *c == '\n' {
            lines.push(index + 1);
        }
    }
    let offset = |at: Position| -> Option<usize> {
        let line = *lines.get(at.line()?.checked_sub(1)?)?;
        let index = line + at.position()?.checked_sub(1)?;
        (index <= chars.len()).then_some(index)
    };
    let starts = tokens
        .iter()
        .map(|(_, at)| offset(*at))
        .chain(std::iter::once(offset(end)))
        .collect::<Option<Vec<usize>>>()?;
    // A token runs to the next one's start. One a macro's expansion put
    // before tokens from elsewhere (`stringify!($hex)`) runs as far as it
    // is written: a word, a number or a quoted literal.
    let written = |start: usize| -> String {
        let rest = &chars[start..];
        match rest.first() {
            Some(quote @ ('"' | '\'')) => {
                let mut end = 1;
                while end < rest.len() && rest[end] != *quote {
                    end += if rest[end] == '\\' { 2 } else { 1 };
                }
                rest[..(end + 1).min(rest.len())].iter().collect()
            }
            _ => rest
                .iter()
                .take_while(|c| c.is_alphanumeric() || matches!(c, '_' | '.'))
                .collect(),
        }
    };
    let pieces: Vec<String> = starts
        .windows(2)
        .zip(tokens)
        .map(|(pair, (token, _))| {
            // Tokens a macro's own body wrote share its call's position.
            if pair[1] == pair[0] {
                return token.to_string();
            }
            if pair[1] < pair[0] {
                return written(pair[0]);
            }
            chars[pair[0]..pair[1]]
                .iter()
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect();
    // Rust's keywords, which an `(` after them is spaced from.
    const KEYWORDS: &[&str] = &[
        "as", "break", "const", "continue", "crate", "else", "enum", "extern", "false", "for",
        "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "ref", "return", "self",
        "static", "struct", "super", "trait", "true", "type", "unsafe", "use", "where", "while",
        "async", "await", "dyn",
    ];
    let word = |piece: &str| {
        piece
            .chars()
            .next()
            .is_some_and(|c| c.is_alphanumeric() || matches!(c, '_' | '"' | '\''))
    };
    let opens = |token: &Token| {
        matches!(
            token,
            Token::LeftParen | Token::LeftBracket | Token::LeftBrace
        )
    };
    let closes = |token: &Token| {
        matches!(
            token,
            Token::RightParen | Token::RightBracket | Token::RightBrace
        )
    };
    let mut out = String::new();
    for (index, (token, _)) in tokens.iter().enumerate() {
        let piece = if matches!(token, Token::Unit) {
            "()"
        } else {
            pieces[index].as_str()
        };
        out.push_str(piece);
        let Some((next, _)) = tokens.get(index + 1) else {
            break;
        };
        let next_piece = pieces[index + 1].as_str();
        // Joint in the source: no space.
        let spaced = chars
            .get(starts[index + 1].wrapping_sub(1))
            .is_some_and(|c| c.is_whitespace());
        if !spaced {
            continue;
        }
        // `{ b }` keeps its spaces inside; `(x)` and `[0]` have none.
        if matches!(token, Token::LeftBrace) || matches!(next, Token::RightBrace) {
            out.push(' ');
            continue;
        }
        if opens(token) || closes(next) {
            continue;
        }
        // A group, a word or a literal is not punctuation.
        let punct = |token: &Token, piece: &str| {
            !word(piece) && !opens(token) && !closes(token) && !matches!(token, Token::Unit)
        };
        let (left, right) = (punct(token, piece), punct(next, next_piece));
        let space = match () {
            // `x.y`, `tup.0`
            () if piece == "." && !right => false,
            // `foo,`, `x = 3;`, `x.y`
            () if !left && matches!(next_piece, "," | ";" | ".") => false,
            // `f(3)`, `fn(x: u8)`, `Self()`, `pub(crate)`
            () if word(piece)
                && !piece.starts_with(['"', '\''])
                && (!KEYWORDS.contains(&piece) || matches!(piece, "fn" | "Self" | "pub"))
                && matches!(next, Token::LeftParen | Token::Unit) =>
            {
                false
            }
            // `#[attr]`
            () if piece == "#" && matches!(next, Token::LeftBracket) => false,
            () => true,
        };
        if space {
            out.push(' ');
        }
    }
    Some(out)
}

/// rustc's size in bytes of a value of `ty`, for a type whose layout Rust
/// fixes: a number, `bool`, `char`, `()` or an array of them.
fn primitive_size(ty: &Ty) -> Option<u64> {
    Some(match ty {
        Ty::Unit => 0,
        Ty::Bool => 1,
        Ty::Char => 4,
        // An integer literal's type defaults to `i32`.
        Ty::Int(Int::Var(_)) => 4,
        Ty::Int(kind) => u64::from(kind.bits() / 8),
        Ty::Float(Float::F32) => 4,
        Ty::Float(_) => 8,
        Ty::Array {
            element,
            kind: sequence::Kind::Array(length),
        } => primitive_size(element)?.checked_mul(u64::try_from(*length).ok()?)?,
        _ => return None,
    })
}

/// The slice methods `SliceChunks` runs, by code: each gives the slices.
const SLICE_CHUNKS: &[&str] = &[
    "chunks",
    "windows",
    "chunks_exact",
    "rchunks",
    "rchunks_exact",
];

/// The `Vec` methods `VecMethod` runs in place, by code.
const VEC_METHODS: &[&str] = &[
    "insert",
    "remove",
    "swap",
    "dedup",
    "drain",
    "pop",
    "store",
    "fill",
    "copy_from_slice",
    "split_off",
    "swap_remove",
    "pop_front",
    "with_capacity",
    "push",
    "clear",
    "truncate",
    "reverse",
    "append",
    "resize",
    "resize_with",
    "retain",
    "sort_by",
    "sort_unstable_by",
    "sort",
    "sort_unstable",
    "extend_move",
    "extend_copy",
    "extend_clone",
    "deque_remove",
    "swap_remove_back",
    "swap_remove_front",
    "rotate_left",
    "rotate_right",
];

/// The `VecDeque` methods rawr runs, each as the `Vec` method of an array
/// whose back is its end, or as an operation of the deque's own (`remove`'s
/// `Option`, `range`, `as_slices`). std's others are not run yet (D712.1).
const DEQUE_METHODS: &[(&str, &str)] = &[
    ("push_back", "push"),
    ("pop_back", "pop"),
    ("push_front", "push_front"),
    ("pop_front", "pop_front"),
    ("front", "first"),
    ("back", "last"),
    ("front_mut", "first_mut"),
    ("back_mut", "last_mut"),
    ("len", "len"),
    ("is_empty", "is_empty"),
    ("iter", "iter"),
    ("iter_mut", "iter_mut"),
    ("into_iter", "into_iter"),
    ("get", "get"),
    ("get_mut", "get_mut"),
    ("contains", "contains"),
    ("insert", "insert"),
    ("swap", "swap"),
    ("clear", "clear"),
    ("truncate", "truncate"),
    ("retain", "retain"),
    ("extend", "extend"),
    ("append", "append"),
    ("split_off", "split_off"),
    ("drain", "drain"),
    ("resize", "resize"),
    ("resize_with", "resize_with"),
    ("clone", "clone"),
    ("remove", "deque_remove"),
    ("swap_remove_back", "swap_remove_back"),
    ("swap_remove_front", "swap_remove_front"),
    ("rotate_left", "rotate_left"),
    ("rotate_right", "rotate_right"),
    ("range", "range"),
    ("as_slices", "as_slices"),
    ("make_contiguous", "as_mut_slice"),
];

/// A `VEC_METHODS` code by name.
fn vec_method_code(name: &str) -> usize {
    VEC_METHODS
        .iter()
        .position(|method| *method == name)
        .expect("a listed `Vec` method")
}

/// `std::time`'s functions, methods and operators on Rust's `Duration`
/// and `Instant`, by code: each name, and what the operand is.
const TIME_METHODS: &[&str] = &[
    // Constructors of a `Duration`, from the operand.
    "from_secs",
    "from_millis",
    "from_micros",
    "from_nanos",
    "from_secs_f64",
    "ZERO",
    "MAX",
    "now",
    // Readers of a `Duration`.
    "as_secs",
    "as_millis",
    "as_micros",
    "as_nanos",
    "as_secs_f64",
    "subsec_millis",
    "subsec_micros",
    "subsec_nanos",
    "is_zero",
    // A `Duration` with another.
    "checked_add",
    "checked_sub",
    "saturating_add",
    "saturating_sub",
    "abs_diff",
    "mul_f64",
    "div_f64",
    // An `Instant`.
    "elapsed",
    "duration_since",
    "saturating_duration_since",
    "checked_duration_since",
    "instant_checked_add",
    "instant_checked_sub",
    // Operators.
    "add",
    "sub",
    "mul",
    "div",
    "instant_add",
    "instant_sub",
    "instant_since",
    "compare",
    "from_secs_f32",
    "as_secs_f32",
    // `std::thread::sleep(duration)`.
    "sleep",
];

fn time_code(name: &str) -> usize {
    TIME_METHODS
        .iter()
        .position(|method| *method == name)
        .expect("a listed `std::time` method")
}

/// One of `TIME_METHODS` on `args[0]` with the operand `args[2]`, with
/// Rust's panics.
fn time_method(args: &mut FnCallArgs) -> RawrResult {
    use crate::Instant;
    use std::time::Duration;
    let fail = |message: &str| -> RawrResult {
        Err(EvalAltResult::ErrorRuntime(message.into(), Position::NONE).into())
    };
    let duration = |value: &Dynamic| value.read_lock::<Duration>().map(|value| *value);
    let instant = |value: &Dynamic| value.read_lock::<Instant>().map(|value| *value);
    let unsigned = |value: &Dynamic| u64::try_from(value.as_int().unwrap_or(0)).unwrap_or(0);
    let number = |value: &Dynamic| value.as_float().unwrap_or(0.0);
    let int =
        |value: u128| Dynamic::from_int(crate::INT::try_from(value).unwrap_or(crate::INT::MAX));
    let optional =
        |value: Option<Dynamic>| value.map_or_else(crate::option::none, crate::option::some);
    let code = usize::try_from(args[1].as_int().unwrap_or(-1)).unwrap_or(usize::MAX);
    let name = TIME_METHODS.get(code).copied().unwrap_or("");
    let (value, operand) = (&*args[0], &*args[2]);
    let float = |value: f64| -> Result<Duration, Box<EvalAltResult>> {
        Duration::try_from_secs_f64(value).map_err(|error| {
            EvalAltResult::ErrorRuntime(
                format!("cannot convert float seconds to Duration: {error}").into(),
                Position::NONE,
            )
            .into()
        })
    };
    Ok(match name {
        "from_secs" => Dynamic::from(Duration::from_secs(unsigned(operand))),
        "from_millis" => Dynamic::from(Duration::from_millis(unsigned(operand))),
        "sleep" => {
            if let Some(duration) = duration(operand) {
                std::thread::sleep(duration);
            }
            Dynamic::UNIT
        }
        "from_micros" => Dynamic::from(Duration::from_micros(unsigned(operand))),
        "from_nanos" => Dynamic::from(Duration::from_nanos(unsigned(operand))),
        "from_secs_f64" | "from_secs_f32" => Dynamic::from(float(number(operand))?),
        "ZERO" => Dynamic::from(Duration::ZERO),
        "MAX" => Dynamic::from(Duration::MAX),
        "now" => Dynamic::from(Instant::now()),
        "elapsed"
        | "duration_since"
        | "saturating_duration_since"
        | "checked_duration_since"
        | "instant_checked_add"
        | "instant_checked_sub"
        | "instant_add"
        | "instant_sub"
        | "instant_since" => {
            let Some(at) = instant(value) else {
                return Err(invariant("an `Instant` is not Rust's at runtime"));
            };
            match name {
                "elapsed" => Dynamic::from(at.elapsed()),
                "duration_since" | "saturating_duration_since" | "instant_since" => {
                    let Some(earlier) = instant(operand) else {
                        return Err(invariant("an `Instant` is not Rust's at runtime"));
                    };
                    Dynamic::from(at.saturating_duration_since(earlier))
                }
                "checked_duration_since" => {
                    let Some(earlier) = instant(operand) else {
                        return Err(invariant("an `Instant` is not Rust's at runtime"));
                    };
                    optional(at.checked_duration_since(earlier).map(Dynamic::from))
                }
                _ => {
                    let Some(by) = duration(operand) else {
                        return Err(invariant("a `Duration` is not Rust's at runtime"));
                    };
                    let moved = if name.ends_with("add") {
                        at.checked_add(by)
                    } else {
                        at.checked_sub(by)
                    };
                    match name {
                        "instant_add" => match moved {
                            Some(moved) => Dynamic::from(moved),
                            None => return fail("overflow when adding duration to instant"),
                        },
                        "instant_sub" => match moved {
                            Some(moved) => Dynamic::from(moved),
                            None => return fail("overflow when subtracting duration from instant"),
                        },
                        _ => optional(moved.map(Dynamic::from)),
                    }
                }
            }
        }
        "compare" => {
            let ordering = match (
                duration(value),
                duration(operand),
                instant(value),
                instant(operand),
            ) {
                (Some(lhs), Some(rhs), ..) => lhs.cmp(&rhs),
                (.., Some(lhs), Some(rhs)) => lhs.cmp(&rhs),
                _ => return Err(invariant("a `std::time` value is not Rust's at runtime")),
            };
            Dynamic::from_int(ordering as crate::INT)
        }
        _ => {
            let Some(span) = duration(value) else {
                return Err(invariant("a `Duration` is not Rust's at runtime"));
            };
            let other = || {
                duration(operand).ok_or_else(|| invariant("a `Duration` is not Rust's at runtime"))
            };
            match name {
                "as_secs" => int(u128::from(span.as_secs())),
                "as_millis" => int(span.as_millis()),
                "as_micros" => int(span.as_micros()),
                "as_nanos" => int(span.as_nanos()),
                "as_secs_f64" => Dynamic::from_float(span.as_secs_f64()),
                "as_secs_f32" => Dynamic::from_float(f64::from(span.as_secs_f32())),
                "subsec_millis" => Dynamic::from_int(span.subsec_millis().into()),
                "subsec_micros" => Dynamic::from_int(span.subsec_micros().into()),
                "subsec_nanos" => Dynamic::from_int(span.subsec_nanos().into()),
                "is_zero" => Dynamic::from_bool(span.is_zero()),
                "checked_add" => optional(span.checked_add(other()?).map(Dynamic::from)),
                "checked_sub" => optional(span.checked_sub(other()?).map(Dynamic::from)),
                "saturating_add" => Dynamic::from(span.saturating_add(other()?)),
                "saturating_sub" => Dynamic::from(span.saturating_sub(other()?)),
                "abs_diff" => Dynamic::from(span.abs_diff(other()?)),
                "mul_f64" => Dynamic::from(float(span.as_secs_f64() * number(operand))?),
                "div_f64" => Dynamic::from(float(span.as_secs_f64() / number(operand))?),
                "add" => match span.checked_add(other()?) {
                    Some(sum) => Dynamic::from(sum),
                    None => return fail("overflow when adding durations"),
                },
                "sub" => match span.checked_sub(other()?) {
                    Some(difference) => Dynamic::from(difference),
                    None => return fail("overflow when subtracting durations"),
                },
                "mul" => match u32::try_from(operand.as_int().unwrap_or(-1))
                    .ok()
                    .and_then(|factor| span.checked_mul(factor))
                {
                    Some(product) => Dynamic::from(product),
                    None => return fail("overflow when multiplying duration by scalar"),
                },
                "div" => match u32::try_from(operand.as_int().unwrap_or(-1)).ok() {
                    Some(0) | None => {
                        return fail("divide by zero error when dividing duration by scalar")
                    }
                    Some(divisor) => Dynamic::from(span / divisor),
                },
                _ => return Err(invariant("an unknown `std::time` method")),
            }
        }
    })
}

/// `insert(i, x)`, `remove(i)`, `swap_remove(i)`, `swap(a, b)`, `dedup()` and
/// `drain(low, high, inclusive)` (`high` `-1` for an open end) on the array
/// taken by reference, with Rust's panics.
fn vec_method(context: Option<&NativeCallContext<'_>>, args: &mut FnCallArgs) -> RawrResult {
    let fail = |message: String| -> RawrResult {
        Err(EvalAltResult::ErrorRuntime(message.into(), Position::NONE).into())
    };
    let code = args[1].as_int().unwrap_or(-1);
    let at = |value: &Dynamic| usize::try_from(value.as_int().unwrap_or(-1)).unwrap_or(usize::MAX);
    if let Some(place) = crate::place::get(args[0]) {
        use crate::place::Operation;
        let name = VEC_METHODS
            .get(usize::try_from(code).unwrap_or(usize::MAX))
            .copied();
        let (operation, indexes): (Operation, &[usize]) = match name {
            Some("insert") => (Operation::Insert, &[2, 3]),
            Some("remove") => (Operation::Remove, &[2]),
            Some("swap_remove") => (Operation::SwapRemove, &[2]),
            Some("swap") => (Operation::Swap, &[2, 3]),
            Some("pop") => (Operation::Pop, &[]),
            Some("fill") => (Operation::Fill, &[3]),
            Some("dedup") => (Operation::Dedup, &[]),
            Some("drain") => (Operation::Drain, &[2, 3, 4]),
            Some("copy_from_slice") => (Operation::CopyFromSlice, &[3]),
            Some("split_off") => (Operation::SplitOff, &[2]),
            Some("push") => (Operation::Push, &[2]),
            Some("clear") => (Operation::Clear, &[]),
            Some("truncate") => (Operation::Truncate, &[2]),
            Some("reverse") => (Operation::Reverse, &[]),
            Some("append") => (Operation::Append, &[2]),
            Some("extend_move") => (Operation::Extend(crate::place::ExtendMode::Move), &[2]),
            Some("extend_copy") => (Operation::Extend(crate::place::ExtendMode::Copy), &[2]),
            Some("extend_clone") => (Operation::Extend(crate::place::ExtendMode::Clone), &[2]),
            Some("resize") => (Operation::Resize, &[2, 3]),
            Some("resize_with") => (Operation::ResizeWith, &[2, 3]),
            Some("retain") => (Operation::Retain, &[2]),
            Some("sort") => (Operation::Sort { stable: true }, &[]),
            Some("sort_unstable") => (Operation::Sort { stable: false }, &[]),
            Some("sort_by") => (Operation::SortBy { stable: true }, &[2]),
            Some("sort_unstable_by") => (Operation::SortBy { stable: false }, &[2]),
            _ => return Err(invariant("native sequence operation has no typed dispatch")),
        };
        let mut operands = indexes
            .iter()
            .map(|index| args[*index].take())
            .collect::<Vec<_>>();
        return match context {
            Some(context) => place.invoke_in(context, operation, &mut operands),
            None => place.invoke(operation, &mut operands),
        };
    }
    let (a, b) = (at(args[2]), at(args[3]));
    // The operands move in, as Rust's `push(x)` and `insert(i, x)` do.
    let first = args[2].take();
    let value = args[3].take();
    let inclusive = args[4].as_bool().unwrap_or(false);
    let mut array = args[0]
        .as_array_mut()
        .map_err(|_| invariant("a `Vec` is not an array at runtime"))?;
    let len = array.len();
    match VEC_METHODS
        .get(usize::try_from(code).unwrap_or(usize::MAX))
        .copied()
    {
        Some("extend_move" | "extend_copy" | "extend_clone") => {
            array.extend(
                first
                    .into_array()
                    .map_err(|_| invariant("an extend source is not an array"))?,
            );
            Ok(Dynamic::UNIT)
        }
        Some("push") => {
            array.push(first);
            Ok(Dynamic::UNIT)
        }
        Some("clear") => {
            array.clear();
            Ok(Dynamic::UNIT)
        }
        Some("truncate") => {
            array.truncate(a);
            Ok(Dynamic::UNIT)
        }
        Some("reverse") => {
            array.reverse();
            Ok(Dynamic::UNIT)
        }
        Some("append") => {
            let mut incoming = first
                .into_array()
                .map_err(|_| invariant("an append operand is not an array"))?;
            array.append(&mut incoming);
            Ok(Dynamic::UNIT)
        }
        Some("insert") if a > len => fail(format!(
            "insertion index (is {a}) should be <= len (is {len})"
        )),
        Some("insert") => {
            array.insert(a, value);
            Ok(Dynamic::UNIT)
        }
        Some("remove") if a >= len => {
            fail(format!("removal index (is {a}) should be < len (is {len})"))
        }
        Some("remove") => Ok(array.remove(a)),
        Some("swap_remove") if a >= len => fail(format!(
            "swap_remove index (is {a}) should be < len (is {len})"
        )),
        Some("swap_remove") => Ok(array.swap_remove(a)),
        // A new `Vec` with room for `a` items, as `VecCapacity` reads it
        // back; it grows by doubling from 4, as Rust's does.
        Some("with_capacity") => Ok(Dynamic::from_array(crate::Array::with_capacity(a))),
        Some("swap") => match [a, b].into_iter().find(|index| *index >= len) {
            Some(index) => fail(format!(
                "index out of bounds: the len is {len} but the index is {index}"
            )),
            None => {
                array.swap(a, b);
                Ok(Dynamic::UNIT)
            }
        },
        Some("dedup") => {
            array.dedup_by(|later, earlier| equal_value(earlier, later));
            Ok(Dynamic::UNIT)
        }
        Some("pop") => Ok(match array.pop() {
            Some(last) => crate::option::some(last),
            None => crate::option::none(),
        }),
        // A deque's removals, `None` past its end.
        Some("deque_remove" | "swap_remove_back" | "swap_remove_front") if a >= len => {
            Ok(crate::option::none())
        }
        Some("deque_remove") => Ok(crate::option::some(array.remove(a))),
        Some("swap_remove_back") => Ok(crate::option::some(array.swap_remove(a))),
        Some("swap_remove_front") => {
            array.swap(0, a);
            Ok(crate::option::some(array.remove(0)))
        }
        Some("rotate_left" | "rotate_right") if a > len => {
            fail("assertion failed: n <= self.len()".into())
        }
        Some("rotate_left") => {
            array.rotate_left(a);
            Ok(Dynamic::UNIT)
        }
        Some("rotate_right") => {
            array.rotate_right(a);
            Ok(Dynamic::UNIT)
        }
        Some("pop_front") if array.is_empty() => Ok(crate::option::none()),
        Some("pop_front") => Ok(crate::option::some(array.remove(0))),
        // A changed copy of `array[a..]` stored back over it.
        Some("store") => {
            let copy = value.into_array().unwrap_or_default();
            for (slot, element) in array.iter_mut().skip(a).zip(copy) {
                *slot = element;
            }
            Ok(Dynamic::UNIT)
        }
        Some("fill") => {
            array.fill(value);
            Ok(Dynamic::UNIT)
        }
        Some("split_off") if a > len => fail(format!(
            "`at` split index (is {a}) should be <= len (is {len})"
        )),
        Some("split_off") => Ok(Dynamic::from_array(array.split_off(a))),
        Some("copy_from_slice") => {
            let source = value.into_array().unwrap_or_default();
            if source.len() != len {
                return fail(format!(
                    "source slice length ({}) does not match destination slice length ({len})",
                    source.len()
                ));
            }
            *array = source;
            Ok(Dynamic::UNIT)
        }
        Some("drain") => {
            let high = (b != usize::MAX).then_some(b);
            match drain_window(a, high, inclusive, len) {
                Ok((low, high)) => Ok(Dynamic::from_array(array.drain(low..high).collect())),
                Err(message) => fail(message),
            }
        }
        _ => Err(invariant("an unknown `Vec` method")),
    }
}

/// The `String` methods `StrChange` runs in place, by code.
const STR_CHANGES: &[&str] = &[
    "pop",
    "clear",
    "insert",
    "insert_str",
    "remove",
    "truncate",
    "split_off",
    "push",
    "push_str",
    "retain",
];

/// `STR_CHANGES`' `code` on the string taken by reference, by bytes and
/// with Rust's panics.
fn str_change(context: Option<&NativeCallContext<'_>>, args: &mut FnCallArgs) -> RawrResult {
    let fail = |message: String| -> RawrResult {
        Err(EvalAltResult::ErrorRuntime(message.into(), Position::NONE).into())
    };
    let code = usize::try_from(args[1].as_int().unwrap_or(-1)).unwrap_or(usize::MAX);
    let at = usize::try_from(args[2].as_int().unwrap_or(-1)).unwrap_or(usize::MAX);
    let operand = args[3].clone();
    if let Some(place) = crate::place::get(args[0]) {
        use crate::place::Operation;
        let (operation, indexes): (Operation, &[usize]) = match STR_CHANGES.get(code).copied() {
            Some("pop") => (Operation::Pop, &[]),
            Some("clear") => (Operation::Clear, &[]),
            Some("truncate") => (Operation::Truncate, &[2]),
            Some("push") => (Operation::Push, &[3]),
            Some("push_str") => (Operation::PushStr, &[3]),
            Some("insert") => (Operation::Insert, &[2, 3]),
            Some("insert_str") => (Operation::InsertStr, &[2, 3]),
            Some("remove") => (Operation::Remove, &[2]),
            Some("split_off") => (Operation::SplitOff, &[2]),
            Some("retain") => (Operation::Retain, &[3]),
            _ => return Err(invariant("native String operation has no typed dispatch")),
        };
        let mut operands = indexes
            .iter()
            .map(|index| args[*index].take())
            .collect::<Vec<_>>();
        return match context {
            Some(context) => place.invoke_in(context, operation, &mut operands),
            None => place.invoke(operation, &mut operands),
        };
    }
    let mut text = args[0]
        .write_lock::<ImmutableString>()
        .ok_or_else(|| invariant("a `String` is not a string at runtime"))?;
    let name = STR_CHANGES.get(code).copied().unwrap_or_default();
    // Rust asserts a char boundary before changing the string there.
    let boundary = |text: &str, at: usize, what: &str| -> Result<(), Box<EvalAltResult>> {
        if text.is_char_boundary(at) {
            Ok(())
        } else {
            Err(EvalAltResult::ErrorRuntime(
                format!("assertion failed: self.is_char_boundary({what})").into(),
                Position::NONE,
            )
            .into())
        }
    };
    match name {
        "retain" => crate::place::text::retain_with(
            context.ok_or_else(|| invariant("String::retain requires its script call context"))?,
            operand,
            |predicate| text.make_mut().retain(predicate),
        ),
        "pop" => Ok(match text.make_mut().pop() {
            Some(c) => crate::option::some(c.into()),
            None => crate::option::none(),
        }),
        "clear" => {
            *text = ImmutableString::new();
            Ok(Dynamic::UNIT)
        }
        "insert" | "insert_str" => {
            boundary(&text, at, "idx")?;
            match operand.as_char() {
                Ok(c) => text.make_mut().insert(at, c),
                Err(_) => {
                    let piece = operand.clone_cast::<ImmutableString>();
                    text.make_mut().insert_str(at, &piece);
                }
            }
            Ok(Dynamic::UNIT)
        }
        "remove" => {
            let len = text.len();
            if at > len {
                return fail(format!(
                    "start byte index {at} is out of bounds for string of length {len}"
                ));
            }
            if !text.is_char_boundary(at) {
                let start = (0..at)
                    .rev()
                    .find(|index| text.is_char_boundary(*index))
                    .unwrap_or(0);
                let inside = text[start..].chars().next().unwrap_or_default();
                let end = start + inside.len_utf8();
                return fail(format!(
                    "start byte index {at} is not a char boundary; it is inside {inside:?} \
                     (bytes {start}..{end} of string)"
                ));
            }
            match text[at..].chars().next() {
                Some(c) => {
                    text.make_mut().remove(at);
                    Ok(c.into())
                }
                None => fail("cannot remove a char from the end of a string".into()),
            }
        }
        "truncate" => {
            if at <= text.len() {
                boundary(&text, at, "new_len")?;
                text.make_mut().truncate(at);
            }
            Ok(Dynamic::UNIT)
        }
        "split_off" => {
            boundary(&text, at, "at")?;
            let rest = text.make_mut().split_off(at);
            Ok(Dynamic::from(ImmutableString::from(rest.as_str())))
        }
        "push" | "push_str" => {
            match operand.as_char() {
                Ok(c) => text.make_mut().push(c),
                Err(_) => {
                    let piece = operand.clone_cast::<ImmutableString>();
                    text.make_mut().push_str(&piece);
                }
            }
            Ok(Dynamic::UNIT)
        }
        _ => Err(invariant("an unknown `String` method")),
    }
}

/// The string methods `StrPieces` runs, by code: each gives an array.
/// `from_utf8` gives a `Result` of the text or its error's `Debug` text,
/// `utf8_display` that error's message from its `Debug` text, and `debug`
/// a string's or a char's `Debug` text.
const STR_PIECES: &[&str] = &[
    "bytes",
    "char_indices",
    "splitn",
    "from_utf8",
    "debug",
    "utf8_display",
    "rsplitn",
];

fn str_pieces(args: &mut FnCallArgs) -> RawrResult {
    let code = usize::try_from(args[1].as_int().unwrap_or(-1)).unwrap_or(usize::MAX);
    let name = STR_PIECES.get(code).copied().unwrap_or_default();
    if name == "from_utf8" {
        let bytes: Vec<u8> = args[0]
            .as_array_ref()
            .map_err(|_| invariant("a `Vec<u8>` is not an array at runtime"))?
            .iter()
            .map(|byte| u8::try_from(byte.as_int().unwrap_or_default()).unwrap_or_default())
            .collect();
        return Ok(match String::from_utf8(bytes) {
            Ok(text) => crate::result::ok(Dynamic::from(ImmutableString::from(text))),
            Err(error) => {
                crate::result::err(Dynamic::from(ImmutableString::from(format!("{error:?}"))))
            }
        });
    }
    // `FromUtf8Error { bytes: [..], error: Utf8Error { valid_up_to: 0,
    // error_len: Some(1) } }`: std's `Display` of that `Utf8Error`.
    if name == "utf8_display" {
        let debug = args[0].clone_cast::<ImmutableString>();
        let number = |key: &str| {
            let rest = debug.rsplit_once(key)?.1;
            let digits = rest.split(|c: char| !c.is_ascii_digit()).next()?;
            digits.parse::<usize>().ok()
        };
        let valid_up_to = number("valid_up_to: ").unwrap_or_default();
        let text = match number("error_len: Some(") {
            Some(length) => {
                format!("invalid utf-8 sequence of {length} bytes from index {valid_up_to}")
            }
            None => format!("incomplete utf-8 byte sequence from index {valid_up_to}"),
        };
        return Ok(Dynamic::from(ImmutableString::from(text)));
    }
    if name == "debug" {
        if let Some(place) = crate::place::get(args[0]) {
            return place.invoke(crate::place::Operation::Debug, &mut []);
        }
        let text = match args[0].as_char() {
            Ok(c) => format!("{c:?}"),
            Err(_) => format!("{:?}", args[0].clone_cast::<ImmutableString>().as_str()),
        };
        return Ok(Dynamic::from(ImmutableString::from(text)));
    }
    let text = args[0].clone_cast::<ImmutableString>();
    let string = |piece: &str| Dynamic::from(ImmutableString::from(piece));
    let pieces: crate::Array = match name {
        "bytes" => text
            .bytes()
            .map(|byte| Dynamic::from(crate::INT::from(byte)))
            .collect(),
        "char_indices" => text
            .char_indices()
            .map(|(index, c)| Dynamic::from_array(vec![(index as crate::INT).into(), c.into()]))
            .collect(),
        "splitn" => {
            let count = usize::try_from(args[2].as_int().unwrap_or_default()).unwrap_or(0);
            match args[3].as_char() {
                Ok(c) => text.splitn(count, c).map(string).collect(),
                Err(_) => {
                    let pattern = args[3].clone_cast::<ImmutableString>();
                    text.splitn(count, pattern.as_str()).map(string).collect()
                }
            }
        }
        "rsplitn" => {
            let count = usize::try_from(args[2].as_int().unwrap_or_default()).unwrap_or(0);
            match args[3].as_char() {
                Ok(c) => text.rsplitn(count, c).map(string).collect(),
                Err(_) => {
                    let pattern = args[3].clone_cast::<ImmutableString>();
                    text.rsplitn(count, pattern.as_str()).map(string).collect()
                }
            }
        }
        _ => return Err(invariant("an unknown string method")),
    };
    Ok(Dynamic::from_array(pieces))
}

/// `char`'s methods by name and result (`c` a `char`, `b` a `bool`, `u` a
/// `usize`, `o` an `Option<u32>`): the code `CharMethod` runs.
/// The `str` methods `StrMethod` runs, by code, each with its arguments'
/// count and its output: `s` a `String`, `b` a `bool`, `f` an
/// `Option<usize>`, `o` an `Option<&str>`, `p` an `Option<(&str, &str)>`.
const STR_METHODS: &[(&str, usize, char)] = &[
    ("trim", 0, 's'),
    ("trim_start", 0, 's'),
    ("trim_end", 0, 's'),
    ("to_ascii_uppercase", 0, 's'),
    ("to_ascii_lowercase", 0, 's'),
    ("replace", 2, 's'),
    ("repeat", 1, 's'),
    ("find", 1, 'f'),
    ("rfind", 1, 'f'),
    ("strip_prefix", 1, 'o'),
    ("strip_suffix", 1, 'o'),
    ("split_once", 1, 'p'),
    ("rsplit_once", 1, 'p'),
    ("eq_ignore_ascii_case", 1, 'b'),
    ("is_char_boundary", 1, 'b'),
    ("trim_matches", 1, 's'),
    ("trim_start_matches", 1, 's'),
    ("trim_end_matches", 1, 's'),
];

/// `STR_METHODS`' `code` on `text`, with Rust's results; a pattern is a
/// `char` or a string.
fn str_method(
    text: &str,
    code: usize,
    a: &Dynamic,
    b: &Dynamic,
) -> Result<Dynamic, Box<EvalAltResult>> {
    let string = |value: &str| Dynamic::from(ImmutableString::from(value));
    let pattern = |value: &Dynamic| match value.as_char() {
        Ok(c) => c.to_string(),
        Err(_) => value.clone_cast::<ImmutableString>().to_string(),
    };
    let option = |value: Option<Dynamic>| match value {
        Some(value) => crate::option::some(value),
        None => crate::option::none(),
    };
    let name = STR_METHODS.get(code).map_or("", |(name, ..)| *name);
    Ok(match name {
        "trim" => string(text.trim()),
        "trim_start" => string(text.trim_start()),
        "trim_end" => string(text.trim_end()),
        "to_ascii_uppercase" => string(&text.to_ascii_uppercase()),
        "to_ascii_lowercase" => string(&text.to_ascii_lowercase()),
        "replace" => string(&text.replace(&pattern(a), &pattern(b))),
        "repeat" => string(&text.repeat(usize::try_from(a.as_int().unwrap_or(0)).unwrap_or(0))),
        "find" => option(text.find(&pattern(a)).map(|at| Dynamic::from(at as INT))),
        "rfind" => option(text.rfind(&pattern(a)).map(|at| Dynamic::from(at as INT))),
        "strip_prefix" => option(text.strip_prefix(&pattern(a)).map(string)),
        "strip_suffix" => option(text.strip_suffix(&pattern(a)).map(string)),
        // rustc takes only a `char` here; a string trims as both ends do.
        "trim_matches" => match a.as_char() {
            Ok(c) => string(text.trim_matches(c)),
            Err(_) => {
                let pattern = pattern(a);
                string(
                    text.trim_start_matches(pattern.as_str())
                        .trim_end_matches(pattern.as_str()),
                )
            }
        },
        "trim_start_matches" => string(text.trim_start_matches(pattern(a).as_str())),
        "trim_end_matches" => string(text.trim_end_matches(pattern(a).as_str())),
        "split_once" | "rsplit_once" => {
            let separator = pattern(a);
            let split = if name == "split_once" {
                text.split_once(separator.as_str())
            } else {
                text.rsplit_once(separator.as_str())
            };
            option(
                split.map(|(left, right)| Dynamic::from_array(vec![string(left), string(right)])),
            )
        }
        "eq_ignore_ascii_case" => text.eq_ignore_ascii_case(&pattern(a)).into(),
        "is_char_boundary" => text
            .is_char_boundary(usize::try_from(a.as_int().unwrap_or(0)).unwrap_or(0))
            .into(),
        _ => Dynamic::UNIT,
    })
}

const CHAR_METHODS: &[(&str, char)] = &[
    ("to_ascii_uppercase", 'c'),
    ("to_ascii_lowercase", 'c'),
    ("is_alphabetic", 'b'),
    ("is_numeric", 'b'),
    ("is_alphanumeric", 'b'),
    ("is_whitespace", 'b'),
    ("is_uppercase", 'b'),
    ("is_lowercase", 'b'),
    ("is_control", 'b'),
    ("is_ascii", 'b'),
    ("is_ascii_digit", 'b'),
    ("is_ascii_alphabetic", 'b'),
    ("is_ascii_alphanumeric", 'b'),
    ("is_ascii_uppercase", 'b'),
    ("is_ascii_lowercase", 'b'),
    ("is_ascii_whitespace", 'b'),
    ("is_ascii_punctuation", 'b'),
    ("is_ascii_hexdigit", 'b'),
    ("is_digit", 'b'),
    ("eq_ignore_ascii_case", 'b'),
    ("to_digit", 'o'),
    ("len_utf8", 'u'),
];

fn char_method(c: char, name: &str, argument: &Dynamic) -> Dynamic {
    let radix = || u32::try_from(argument.as_int().unwrap_or(10)).unwrap_or(10);
    match name {
        "to_ascii_uppercase" => c.to_ascii_uppercase().into(),
        "to_ascii_lowercase" => c.to_ascii_lowercase().into(),
        "to_digit" => match c.to_digit(radix()) {
            Some(digit) => crate::option::some(INT::from(digit).into()),
            None => crate::option::none(),
        },
        "len_utf8" => (c.len_utf8() as INT).into(),
        name => match name {
            "is_alphabetic" => c.is_alphabetic(),
            "is_numeric" => c.is_numeric(),
            "is_alphanumeric" => c.is_alphanumeric(),
            "is_whitespace" => c.is_whitespace(),
            "is_uppercase" => c.is_uppercase(),
            "is_lowercase" => c.is_lowercase(),
            "is_control" => c.is_control(),
            "is_ascii" => c.is_ascii(),
            "is_ascii_digit" => c.is_ascii_digit(),
            "is_ascii_alphabetic" => c.is_ascii_alphabetic(),
            "is_ascii_alphanumeric" => c.is_ascii_alphanumeric(),
            "is_ascii_uppercase" => c.is_ascii_uppercase(),
            "is_ascii_lowercase" => c.is_ascii_lowercase(),
            "is_ascii_whitespace" => c.is_ascii_whitespace(),
            "is_ascii_punctuation" => c.is_ascii_punctuation(),
            "is_ascii_hexdigit" => c.is_ascii_hexdigit(),
            "is_digit" => c.is_digit(radix()),
            _ => argument
                .as_char()
                .is_ok_and(|other| c.eq_ignore_ascii_case(&other)),
        }
        .into(),
    }
}

/// `Slice` (`v[low..high]`) and `SplitAt` (`v.split_at(mid)`) of an array
/// or a string, a copy of the range, with Rust's panics for a range out of
/// bounds or a string index off a `char` boundary. `SliceGetRange`
/// (`v.get(low..high)`) is `None` where `Slice` panics, and `Some` of the
/// copy.
fn slice_value(op: PreludeOp, args: &mut FnCallArgs) -> RawrResult {
    let checked = op == PreludeOp::SliceGetRange;
    let fail = |message: String| -> RawrResult {
        if checked {
            return Ok(crate::option::none());
        }
        Err(EvalAltResult::ErrorRuntime(message.into(), Position::NONE).into())
    };
    let found = |value: Dynamic| -> RawrResult {
        Ok(if checked {
            crate::option::some(value)
        } else {
            value
        })
    };
    // A native slice, as a marked page's `&BLOCKS[1..2]` (#1291), is cut
    // where it is: the range borrows its elements, as Rust's does.
    let place = crate::place::get(args[0]);
    let text = args[0].clone().into_immutable_string().ok();
    let len = match (&place, &text, args[0].as_array_ref()) {
        (Some(place), ..) => place
            .invoke(crate::place::Operation::Len, &mut [])?
            .as_int()
            .ok()
            .and_then(|len| usize::try_from(len).ok())
            .ok_or_else(|| invariant("a native slice's length is not a usize"))?,
        (None, Some(text), _) => text.len(),
        (None, None, Ok(array)) => array.len(),
        (None, None, Err(_)) => {
            return Err(invariant("a slice is not an array or a string at runtime"))
        }
    };
    let at = |value: &Dynamic| usize::try_from(value.as_int().unwrap_or_default()).ok();
    let (low, high) = if op == PreludeOp::SplitAt {
        let mid = at(args[1]).filter(|mid| *mid <= len);
        let Some(mid) = mid else {
            return fail("mid > len".into());
        };
        (mid, mid)
    } else {
        let low = at(args[1]).unwrap_or(usize::MAX);
        let high = match args[2].as_int().unwrap_or(-1) {
            -1 => None,
            high => Some(usize::try_from(high).unwrap_or(usize::MAX)),
        };
        match slice_window(low, high, args[3].as_bool().unwrap_or(false), len) {
            Ok(window) => window,
            Err(message) => return fail(message),
        }
    };
    if let Some(place) = place {
        let window = |range: std::ops::Range<usize>| {
            place
                .window(range)
                .unwrap_or_else(|| Err(invariant("a native slice that can't be cut")))
        };
        if op == PreludeOp::SplitAt {
            return Ok(Dynamic::from_array(vec![
                window(0..low)?,
                window(low..len)?,
            ]));
        }
        return window(low..high).and_then(found);
    }
    let pieces: Vec<Dynamic> = match (&text, args[0].as_array_ref()) {
        (Some(text), _) => {
            for index in [low, high] {
                if !text.is_char_boundary(index) {
                    return fail(format!("byte index {index} is not a char boundary"));
                }
            }
            if op == PreludeOp::SplitAt {
                let (left, right) = text.split_at(low);
                vec![left.into(), right.into()]
            } else {
                return found(Dynamic::from(ImmutableString::from(&text[low..high])));
            }
        }
        (None, Ok(array)) => {
            if op == PreludeOp::SplitAt {
                vec![
                    Dynamic::from_array(array[..low].to_vec()),
                    Dynamic::from_array(array[low..].to_vec()),
                ]
            } else {
                return found(Dynamic::from_array(array[low..high].to_vec()));
            }
        }
        (None, Err(_)) => return Err(invariant("a slice is not an array at runtime")),
    };
    Ok(Dynamic::from_array(pieces))
}

/// Rust's integer methods (`checked_*`, `wrapping_*`, `saturating_*`,
/// `overflowing_*`, bit counts, rotations, Euclidean division) and the bit
/// operators `<<`, `>>` and `!`, by code: `(name, operand, output)`, the
/// operand `n` none, `s` the same kind, `u` a `u32`; the output `o` an
/// `Option`, `t` the kind, `p` a `(value, overflowed)` pair, `u` a `u32`,
/// `b` a `bool`, `a` the unsigned kind, `r` a `Result`.
const INT_METHODS: &[(&str, char, char)] = &[
    ("checked_add", 's', 'o'),
    ("checked_sub", 's', 'o'),
    ("checked_mul", 's', 'o'),
    ("checked_div", 's', 'o'),
    ("checked_rem", 's', 'o'),
    ("checked_neg", 'n', 'o'),
    ("checked_pow", 'u', 'o'),
    ("wrapping_add", 's', 't'),
    ("wrapping_sub", 's', 't'),
    ("wrapping_mul", 's', 't'),
    ("wrapping_div", 's', 't'),
    ("wrapping_rem", 's', 't'),
    ("wrapping_neg", 'n', 't'),
    ("wrapping_pow", 'u', 't'),
    ("saturating_add", 's', 't'),
    ("saturating_sub", 's', 't'),
    ("saturating_mul", 's', 't'),
    ("saturating_div", 's', 't'),
    ("saturating_pow", 'u', 't'),
    ("overflowing_add", 's', 'p'),
    ("overflowing_sub", 's', 'p'),
    ("overflowing_mul", 's', 'p'),
    ("overflowing_div", 's', 'p'),
    ("overflowing_rem", 's', 'p'),
    ("overflowing_neg", 'n', 'p'),
    ("overflowing_pow", 'u', 'p'),
    ("count_ones", 'n', 'u'),
    ("count_zeros", 'n', 'u'),
    ("leading_zeros", 'n', 'u'),
    ("trailing_zeros", 'n', 'u'),
    ("rotate_left", 'u', 't'),
    ("rotate_right", 'u', 't'),
    ("rem_euclid", 's', 't'),
    ("div_euclid", 's', 't'),
    ("abs_diff", 's', 'a'),
    ("is_power_of_two", 'n', 'b'),
    ("ilog2", 'n', 'u'),
    ("ilog10", 'n', 'u'),
    // The operators, not spellable as methods.
    ("<<", 'x', 't'),
    (">>", 'x', 't'),
    ("!", 'n', 't'),
    ("try_from", 'x', 'r'),
    // A `u64`'s and a `usize`'s arithmetic, with Rust's overflow panics.
    ("+", 'x', 't'),
    ("-", 'x', 't'),
    ("*", 'x', 't'),
    ("/", 'x', 't'),
    ("%", 'x', 't'),
    ("**", 'x', 't'),
    // An `i128`'s and a `u128`'s operators, which the runtime's `i64` ones
    // do not compute past `i64`.
    ("neg", 'x', 't'),
    ("&", 'x', 't'),
    ("|", 'x', 't'),
    ("^", 'x', 't'),
    ("<", 'x', 'b'),
    (">", 'x', 'b'),
    ("<=", 'x', 'b'),
    (">=", 'x', 'b'),
    ("==", 'x', 'b'),
    ("!=", 'x', 'b'),
    // Methods of the signed kinds; `abs` and `signum` of the narrower ones
    // are the runtime's.
    ("abs", 'n', 't'),
    ("signum", 'n', 't'),
    ("unsigned_abs", 'n', 'a'),
    ("is_negative", 'n', 'b'),
    ("is_positive", 'n', 'b'),
];

/// The `INT_METHODS` only a signed kind has.
const SIGNED_INT_METHODS: &[&str] = &[
    "abs",
    "signum",
    "unsigned_abs",
    "is_negative",
    "is_positive",
];

/// An `INT_METHODS` entry by name, for a method call.
fn int_method_code(name: &str) -> Option<usize> {
    INT_METHODS
        .iter()
        .position(|(method, operand, _)| *method == name && *operand != 'x' && *method != "!")
}

/// std's panic for a range of a slice of `len` from `start` to `end`, in
/// the order `SliceIndex`'s checks find it.
fn slice_index_fail(start: usize, end: usize, len: usize) -> String {
    if start > len {
        format!("range start index {start} out of range for slice of length {len}")
    } else if start > end && end <= len {
        format!("slice index starts at {start} but ends at {end}")
    } else {
        format!("range end index {end} out of range for slice of length {len}")
    }
}

/// The elements `low..high` of a slice of `len` that `s[low..high]`,
/// `s[low..=high]` (`inclusive`) or `s[low..]` (`high` `None`) takes, or
/// std's panic for the range, as `Index` on a slice checks it.
pub(crate) fn slice_window(
    low: usize,
    high: Option<usize>,
    inclusive: bool,
    len: usize,
) -> Result<(usize, usize), String> {
    let (end, fits) = match high {
        None => (len, low <= len),
        Some(high) if inclusive && high < len => (high + 1, low <= high + 1),
        Some(high) if inclusive => (high, false),
        Some(high) => (high, low <= high && high <= len),
    };
    if fits {
        Ok((low, end))
    } else {
        Err(slice_index_fail(low, end, len))
    }
}

/// The elements `Vec::drain` takes of a `Vec` of `len`, or std's panic,
/// which checks the range's end before its start.
fn drain_window(
    low: usize,
    high: Option<usize>,
    inclusive: bool,
    len: usize,
) -> Result<(usize, usize), String> {
    let end = match high {
        None => len,
        Some(high) if inclusive && high >= len || !inclusive && high > len => {
            return Err(slice_index_fail(0, high, len));
        }
        Some(high) if inclusive => high + 1,
        Some(high) => high,
    };
    if low > end {
        Err(slice_index_fail(low, end, len))
    } else {
        Ok((low, end))
    }
}

/// `INT_METHODS[code]` on `value` of the kind `kinds[0]` (`Int::KINDS`
/// index), its operand of the kind `kinds[1]`, as debug Rust computes it:
/// in `i128`, a `u64`/`usize` read from its bits, the result truncated to
/// the kind; `try_from` converts from `kinds[1]` to `kinds[0]`.
fn int_method(kinds: [INT; 2], args: &mut FnCallArgs) -> RawrResult {
    let panic = |message: &str| -> RawrResult {
        Err(EvalAltResult::ErrorRuntime(message.into(), Position::NONE).into())
    };
    let kind = |code: INT| Int::from_code(code).unwrap_or(Int::I64);
    let (kind, mut operand_kind) = (kind(kinds[0]), kind(kinds[1]));
    let code = usize::try_from(args[1].as_int().unwrap_or_default()).unwrap_or(0);
    let (name, operand_type, output) = INT_METHODS.get(code).copied().unwrap_or(("", 'n', 't'));
    if kind.wide() || operand_kind.wide() && name == "try_from" {
        return wide_method(kind, operand_kind, args);
    }
    // A shift by an `i128` or a `u128`: an amount past `i64` overflows.
    let operand = if operand_kind.wide() {
        operand_kind = Int::I64;
        let amount = wide_value(args[2]).map_or(0, Wide::signed);
        INT::try_from(amount.max(-1)).unwrap_or(INT::MAX)
    } else {
        args[2].as_int().unwrap_or_default()
    };
    let bits = kind.bits();
    let mask = u128::MAX >> (128 - bits);
    let (low, high) = kind.full_range();
    let value = args[0].as_int().unwrap_or_default();
    if name == "try_from" {
        let source = operand_kind.widen(value);
        // `TryFromIntError`'s `Debug`, its runtime value.
        return Ok(if (low..=high).contains(&source) {
            crate::result::ok(kind.store(source).into())
        } else if source > high {
            crate::result::err(ImmutableString::from("TryFromIntError(PosOverflow)").into())
        } else {
            crate::result::err(ImmutableString::from("TryFromIntError(NegOverflow)").into())
        });
    }
    let a = kind.widen(value);
    let b = if operand_type == 's' {
        kind.widen(operand)
    } else {
        operand_kind.widen(operand)
    };
    let wrap = |x: u128| -> INT { kind.store(kind.sign((x & mask) as i128)) };
    let overflow_division = kind.signed() && a == low && b == -1;
    let operation = match name {
        "+" => "add",
        "-" => "sub",
        "*" => "mul",
        "/" => "div",
        "%" => "rem",
        "**" => "pow",
        _ => name.rsplit('_').next().unwrap_or(name),
    };
    // The exact result, `None` past `i128`, and the wrapped one.
    let (exact, wrapped) = match operation {
        "add" => (a.checked_add(b), wrap((a as u128).wrapping_add(b as u128))),
        "sub" => (a.checked_sub(b), wrap((a as u128).wrapping_sub(b as u128))),
        "mul" => (a.checked_mul(b), wrap((a as u128).wrapping_mul(b as u128))),
        "neg" => (Some(-a), wrap(0u128.wrapping_sub(a as u128))),
        "pow" => {
            let exponent = u32::try_from(b).unwrap_or(u32::MAX);
            (
                a.checked_pow(exponent),
                wrap((a as u128).wrapping_pow(exponent)),
            )
        }
        "div" | "rem" | "euclid" if b == 0 => {
            if name.starts_with("checked") {
                return Ok(crate::option::none());
            }
            return panic(if operation == "rem" || name == "rem_euclid" {
                "attempt to calculate the remainder with a divisor of zero"
            } else {
                "attempt to divide by zero"
            });
        }
        "div" => (Some(a / b), wrap((a / b) as u128)),
        "rem" if overflow_division => (None, 0),
        "rem" => (Some(a % b), wrap((a % b) as u128)),
        _ => (None, 0),
    };
    let in_range = exact.filter(|exact| (low..=high).contains(exact));
    let result: Dynamic = match output {
        'o' => match in_range {
            Some(exact) => crate::option::some(kind.store(exact).into()),
            None => crate::option::none(),
        },
        'p' => Dynamic::from_array(vec![wrapped.into(), in_range.is_none().into()]),
        't' if name.len() <= 2 && name != "<<" && name != ">>" && name != "!" => match in_range {
            Some(exact) => kind.store(exact).into(),
            None => {
                let verb = match operation {
                    "add" => "add",
                    "sub" => "subtract",
                    "div" => "divide",
                    "rem" => "calculate the remainder",
                    "pow" => "exponentiate",
                    _ => "multiply",
                };
                return panic(&format!("attempt to {verb} with overflow"));
            }
        },
        't' if name.starts_with("wrapping") => wrapped.into(),
        't' if name.starts_with("saturating") => {
            let negative = match operation {
                "add" => b < 0,
                "sub" => b > 0,
                "mul" | "div" => (a < 0) != (b < 0),
                _ => a < 0 && b % 2 == 1,
            };
            kind.store(in_range.unwrap_or(if negative { low } else { high }))
                .into()
        }
        _ => {
            let pattern = (a as u128) & mask;
            let ones = pattern.count_ones() as INT;
            match name {
                "count_ones" => ones,
                "count_zeros" => INT::from(bits) - ones,
                "leading_zeros" => INT::from(pattern.leading_zeros()) - (128 - INT::from(bits)),
                "trailing_zeros" => INT::from(pattern.trailing_zeros().min(bits)),
                "rotate_left" | "rotate_right" => {
                    let turn = b.rem_euclid(i128::from(bits)) as u32;
                    let turn = if name == "rotate_left" {
                        turn
                    } else {
                        (bits - turn) % bits
                    };
                    match turn {
                        0 => wrap(pattern),
                        _ => wrap((pattern << turn) | (pattern >> (bits - turn))),
                    }
                }
                "rem_euclid" | "div_euclid" if overflow_division => {
                    return panic(if name == "rem_euclid" {
                        "attempt to calculate the remainder with overflow"
                    } else {
                        "attempt to divide with overflow"
                    })
                }
                "rem_euclid" => kind.store(a.rem_euclid(b)),
                "div_euclid" => kind.store(a.div_euclid(b)),
                "abs_diff" => kind.unsigned().store((a - b).abs()),
                "unsigned_abs" => kind.unsigned().store(a.abs()),
                "is_negative" => return Ok((a < 0).into()),
                "is_positive" => return Ok((a > 0).into()),
                "is_power_of_two" => return Ok((a > 0 && a & (a - 1) == 0).into()),
                "ilog2" | "ilog10" if a <= 0 => {
                    return panic("argument of integer logarithm must be positive")
                }
                "ilog2" => INT::from(a.ilog2()),
                "ilog10" => INT::from(a.ilog10()),
                "<<" | ">>" if !(0..i128::from(bits)).contains(&b) => {
                    return panic(if name == "<<" {
                        "attempt to shift left with overflow"
                    } else {
                        "attempt to shift right with overflow"
                    })
                }
                "<<" => wrap(pattern << b as u32),
                ">>" => wrap((a >> b) as u128),
                "!" => wrap(!(a as u128)),
                _ => return Err(invariant("an unknown integer method at runtime")),
            }
            .into()
        }
    };
    Ok(result)
}

/// The place an assignment's text `text` assigns: `g.c[0]` of
/// `g.c[0] += 2`.
fn assigned_place(text: &str) -> &str {
    let bytes = text.as_bytes();
    let assignment = (0..bytes.len()).find(|&index| {
        bytes[index] == b'='
            && bytes.get(index + 1) != Some(&b'=')
            && !matches!(
                index.checked_sub(1).map(|before| bytes[before]),
                Some(b'=' | b'!' | b'<' | b'>')
            )
    });
    let Some(assignment) = assignment else {
        return text.trim();
    };
    text[..assignment]
        .trim_end()
        .trim_end_matches(['+', '-', '*', '/', '%', '^', '&', '|', '<', '>'])
        .trim_end()
}

/// rustc's message for arithmetic `symbol` of `left` and `right` that no
/// impl covers, and the operator's trait.
fn arithmetic_trait(symbol: &str, left: &str, right: &str) -> Option<(String, &'static str)> {
    Some(match symbol {
        "+" => (format!("cannot add `{right}` to `{left}`"), "Add"),
        "-" => (format!("cannot subtract `{right}` from `{left}`"), "Sub"),
        "*" => (format!("cannot multiply `{left}` by `{right}`"), "Mul"),
        "/" => (format!("cannot divide `{left}` by `{right}`"), "Div"),
        "%" => (
            format!("cannot calculate the remainder of `{left}` divided by `{right}`"),
            "Rem",
        ),
        _ => return None,
    })
}

/// `value as` the kind of `code` (`kind_code`), Rust's `as`: integers wrap
/// to the width, floats saturate (`NaN` is `0`), `char` and `bool` are their
/// numbers, a `u8` is a `char`, an `f32` is rounded.
fn cast_value(value: &Dynamic, code: INT) -> RawrResult {
    let fail = |message: &str| -> RawrResult {
        Err(EvalAltResult::ErrorRuntime(message.into(), Position::NONE).into())
    };
    // Into an `i128` or a `u128` (from a `u64`'s or a `usize`'s bits when
    // `UNSIGNED_SOURCE` is added), or out of a boxed one.
    let (code, source) = match code {
        code if code >= UNSIGNED_SOURCE => (code - UNSIGNED_SOURCE, Int::U64),
        code => (code, Int::I64),
    };
    let target = Int::from_code(code).filter(|kind| kind.wide());
    if let (Some(target), Ok(float)) = (target, value.as_float()) {
        // Rust's `as` saturates, and `NaN` is `0`.
        return Ok(if target == Int::I128 {
            wide_i128(float as i128)
        } else {
            wide_u128(float as u128)
        });
    }
    if (target.is_some() || matches!(value.0, Union::Variant(..)))
        && let Some(number) = wide_value(value).map(|_| kind_value(value, source))
    {
        return Ok(match (code, Int::from_code(code)) {
            (_, Some(kind)) => number.store(kind),
            (10, _) => Dynamic::from_float(match number {
                Wide::Signed(value) => value as f32 as crate::FLOAT,
                Wide::Unsigned(value) => value as f32 as crate::FLOAT,
            }),
            (12, _) => Dynamic::from(char::from(number.unsigned() as u8)),
            _ => Dynamic::from_float(match number {
                Wide::Signed(value) => value as crate::FLOAT,
                Wide::Unsigned(value) => value as crate::FLOAT,
            }),
        });
    }
    if let Ok(float) = value.as_float() {
        return Ok(match code {
            10 => Dynamic::from_float(float as f32 as crate::FLOAT),
            11 => Dynamic::from_float(float),
            _ if matches!(Int::KINDS[code as usize].1, Int::U64 | Int::Usize) => {
                (float as u64 as INT).into()
            }
            _ => {
                let (low, high) = Int::KINDS[code as usize].1.range();
                let saturated = if float.is_nan() {
                    0
                } else if float <= low as crate::FLOAT {
                    low
                } else if float >= high as crate::FLOAT {
                    high
                } else {
                    float as INT
                };
                saturated.into()
            }
        });
    }
    let integer = match (value.as_int(), value.as_char(), value.as_bool()) {
        (Ok(integer), ..) => integer,
        (_, Ok(c), _) => INT::from(u32::from(c)),
        (.., Ok(b)) => INT::from(b),
        _ => return fail("an `as` operand is not a number at runtime"),
    };
    Ok(match code {
        10 => Dynamic::from_float(integer as f32 as crate::FLOAT),
        11 => Dynamic::from_float(integer as crate::FLOAT),
        12 => Dynamic::from(char::from(integer as u8)),
        // A char range's item by its code point (`iterator::CHAR_OF_CODE`).
        15 => {
            match u32::try_from(integer).ok().and_then(char::from_u32) {
                Some(c) => Dynamic::from(c),
                None => return fail(
                    "a `char` range across the surrogates U+D800 to U+DFFF is not supported in \
                     Rawr yet",
                ),
            }
        }
        // A `u64`'s or a `usize`'s bits as an `f32` or an `f64`.
        13 => Dynamic::from_float(integer as u64 as f32 as crate::FLOAT),
        14 => Dynamic::from_float(integer as u64 as crate::FLOAT),
        // A `char` or a `bool` into an `i128` or a `u128`.
        _ if target.is_some() => integer.into(),
        _ => {
            let kind = Int::KINDS[code as usize].1;
            let wrapped = match kind {
                Int::I8 => integer as i8 as INT,
                Int::I16 => integer as i16 as INT,
                Int::I32 => integer as i32 as INT,
                Int::U8 => integer as u8 as INT,
                Int::U16 => integer as u16 as INT,
                Int::U32 => integer as u32 as INT,
                _ => integer,
            };
            wrapped.into()
        }
    })
}

/// A `let`'s local and its type (`Lowering::local_types`).
struct LocalType {
    name: ImmutableString,
    declaration: Span,
    ty: Ty,
    /// The `let`'s value, when it has one.
    value: Option<Span>,
}

/// A use of a `let x;` before an assignment gives it a value
/// (`Lowering::uninitialized`).
struct UninitializedUse {
    name: ImmutableString,
    declaration: Span,
    reference: Span,
    ty: Ty,
    /// Whether some path to the use gives the local a value.
    possibly: bool,
    /// The operator the use is the operand of while its type is unknown,
    /// which needs the type there: rustc's code for it and its place.
    operator: Option<(&'static str, Span)>,
}

/// Whether `pattern` binds some part of its value by value: a name, not a
/// `ref`, a `_` or a `&` pattern.
fn binds_by_value(pattern: &pattern::Syntax) -> bool {
    match pattern {
        pattern::Syntax::Binding { .. } => true,
        pattern::Syntax::Rest { binding, .. } => binding.is_some(),
        pattern::Syntax::TupleStruct { members, .. } | pattern::Syntax::Tuple { members, .. } => {
            members.iter().any(binds_by_value)
        }
        pattern::Syntax::Or { alternatives, .. } => alternatives.iter().any(binds_by_value),
        pattern::Syntax::Prelude { payload, .. } => payload.as_deref().is_some_and(binds_by_value),
        pattern::Syntax::Cow { payload, .. } => binds_by_value(payload),
        pattern::Syntax::Slice {
            before,
            rest,
            after,
            ..
        } => {
            before.iter().chain(after.iter()).any(binds_by_value)
                || rest.as_deref().is_some_and(binds_by_value)
        }
        _ => false,
    }
}

fn compound_operator(assignment: &Token) -> Option<Token> {
    Some(match assignment {
        Token::PlusAssign => Token::Plus,
        Token::MinusAssign => Token::Minus,
        Token::MultiplyAssign => Token::Multiply,
        Token::DivideAssign => Token::Divide,
        Token::ModuloAssign => Token::Modulo,
        Token::AndAssign => Token::Ampersand,
        Token::OrAssign => Token::Pipe,
        Token::XOrAssign => Token::XOr,
        Token::LeftShiftAssign => Token::LeftShift,
        Token::RightShiftAssign => Token::RightShift,
        _ => return None,
    })
}

fn binary_precedence(token: &Token) -> Option<u8> {
    Some(match token {
        Token::Or => 1,
        Token::And => 2,
        Token::EqualsTo
        | Token::NotEqualsTo
        | Token::LessThan
        | Token::GreaterThan
        | Token::LessThanEqualsTo
        | Token::GreaterThanEqualsTo => COMPARISON,
        Token::Pipe => 4,
        Token::XOr => 5,
        Token::Ampersand => 6,
        Token::LeftShift | Token::RightShift => 7,
        Token::Plus | Token::Minus => 8,
        Token::Multiply | Token::Divide | Token::Modulo => 9,
        _ => return None,
    })
}

/// A `u64`'s or a `usize`'s bits with the sign bit flipped, which orders
/// them as `i64`s the way they order as unsigned numbers.
fn unsigned_order(value: origin::Fragment) -> origin::Fragment {
    value.surrounded("((", ") ^ (-9223372036854775807 - 1))")
}

/// `=` or a compound assignment.
/// The panic macros of Rust's prelude (`parse_panic_macro`).
const PANIC_MACROS: &[&str] = &[
    "panic",
    "assert",
    "assert_eq",
    "assert_ne",
    "debug_assert",
    "debug_assert_eq",
    "debug_assert_ne",
    "unreachable",
    "todo",
    "unimplemented",
];

/// Macro arguments split at their top-level commas.
fn split_arguments(tokens: Vec<(Token, Position)>) -> Vec<Vec<(Token, Position)>> {
    let mut parts = vec![Vec::new()];
    let mut depth = 0usize;
    for (token, position) in tokens {
        match token {
            Token::LeftParen | Token::LeftBracket | Token::LeftBrace | Token::MapStart => {
                depth += 1;
            }
            Token::RightParen | Token::RightBracket | Token::RightBrace => {
                depth = depth.saturating_sub(1);
            }
            Token::Comma if depth == 0 => {
                parts.push(Vec::new());
                continue;
            }
            _ => {}
        }
        parts
            .last_mut()
            .expect("there is always a part")
            .push((token, position));
    }
    if parts.len() > 1 && parts.last().is_some_and(Vec::is_empty) {
        parts.pop();
    }
    parts
}

/// An expression's tokens as Rust's `stringify!` prints them: binary
/// operators spaced, nothing around `.`, `::` and brackets.
fn token_text(tokens: &[(Token, Position)]) -> String {
    let mut text = String::new();
    let mut previous: Option<&Token> = None;
    let mut unary = false;
    for (token, _) in tokens {
        let word = match token {
            Token::StringConstant(value) => format!("{:?}", value.as_str()),
            Token::CharConstant(value) => format!("{value:?}"),
            token => token.to_string(),
        };
        let operand_before = previous.is_some_and(ends_operand);
        let tight_before = matches!(
            token,
            Token::Period
                | Token::Comma
                | Token::RightParen
                | Token::RightBracket
                | Token::SemiColon
                | Token::DoubleColon
                | Token::Unit
        ) || matches!(token, Token::Reserved(word) if word.as_str() == "?")
            || matches!(token, Token::LeftParen | Token::LeftBracket | Token::Bang)
                && operand_before
            || matches!(token, Token::LeftParen | Token::LeftBracket)
                && matches!(previous, Some(Token::Bang));
        let tight_after_previous = unary
            || matches!(
                previous,
                Some(Token::Period | Token::LeftParen | Token::LeftBracket | Token::DoubleColon)
            );
        if previous.is_some() && !tight_before && !tight_after_previous {
            text.push(' ');
        }
        // `-x`, `!x`, `&x`, `*x`: a prefix operator where no operand ends.
        unary = matches!(
            token,
            Token::UnaryMinus | Token::Minus | Token::Bang | Token::Ampersand | Token::Multiply
        ) && !operand_before;
        text.push_str(&word);
        previous = Some(token);
    }
    text
}

/// `ty` under all its references.
fn without_references(mut ty: &Ty) -> &Ty {
    while let Ty::Reference { target, .. } = ty {
        ty = target;
    }
    ty
}

/// Whether an operand can end at `token`, so an operator after it is binary.
fn ends_operand(token: &Token) -> bool {
    match token {
        Token::Identifier(_)
        | Token::IntegerConstant(_)
        | Token::StringConstant(_)
        | Token::ByteStringConstant(_)
        | Token::ByteConstant(_)
        | Token::CharConstant(_)
        | Token::True
        | Token::False
        | Token::Unit
        | Token::RightParen
        | Token::RightBracket
        | Token::RightBrace
        | Token::SelfValue => true,
        Token::FloatConstant(_) => true,
        Token::Reserved(word) => word.as_str() == "?",
        _ => false,
    }
}

fn assignment_token(token: &Token) -> bool {
    *token == Token::Equals || compound_operator(token).is_some()
}

fn point_span(position: Position) -> Span {
    span(position, position)
}

/// Whether the evaluator's standard library has a function `name`.
fn standard_function(name: &str) -> bool {
    use crate::packages::Package as _;
    static NAMES: std::sync::OnceLock<BTreeSet<String>> = std::sync::OnceLock::new();
    NAMES
        .get_or_init(|| {
            crate::packages::StandardPackage::new()
                .as_shared_module()
                .iter_fn()
                .map(|(_, metadata)| metadata.name.to_string())
                .collect()
        })
        .contains(name)
}

/// What an expression ends, which decides the tokens rustc lists as
/// expected after it.
#[derive(Clone, Copy)]
enum Continuation {
    /// A path, which `!`, `::` or `{` may continue.
    Path,
    /// A statement, which the block's `}` may end.
    Statement,
    /// A `let`'s value, which `else` may continue.
    Let,
}

fn text_end(mut position: Position, text: &str) -> Position {
    for _ in text.chars() {
        position.advance();
    }
    position
}

fn span(start: Position, end: Position) -> Span {
    Span::new(start, end)
}

/// Why a literal can't build an imported value whose public field rawr
/// can't hold yet: the field and its type, as the crate writes them.
#[cfg(feature = "rust-import")]
fn unsupported_field(unsupported: &crate::host::type_def::Unsupported) -> String {
    format!(
        "field `{}` has type `{}`, which rawr can't hold yet",
        unsupported.field, unsupported.ty
    )
}

#[cfg(test)]
mod tests {
    use super::{
        default_value, inference_metrics, matches_runtime_model, model_clones,
        reset_inference_metrics, reset_model_clones, Inference, Models, Ty,
    };
    #[cfg(feature = "rust-import")]
    use super::{host_ty_proof_allocations, reset_host_ty_proof_allocations, HostTyProof};
    use crate::host::{facade, item, signature, type_ref};
    use crate::{
        host, receiver, resolve, Dynamic, Engine, OptimizationLevel, ParseErrorType, Shared,
    };
    use std::fmt::Write as _;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    #[derive(Clone)]
    struct RequiredValue;

    #[derive(Clone)]
    struct GenericValue;

    #[derive(Clone)]
    struct OrderedValue;

    struct ExternalValue;

    #[cfg(feature = "rust-import")]
    fn symbolic_imported(
        engine: &Engine,
        program: &resolve::Program,
    ) -> Result<Box<[super::ImportedUse]>, resolve::ImportedFailure> {
        let mut allocation = host::TestImportAllocation::default();
        super::imported(
            engine,
            program,
            &engine.host_registry.share(),
            &mut allocation,
        )
        .map(|output| output.uses)
    }

    #[test]
    fn compiler_ir_layout_keeps_parser_types_compact() {
        #[cfg(not(feature = "rust-import"))]
        assert_eq!(std::mem::size_of::<Ty>(), 40);
        #[cfg(feature = "rust-import")]
        assert_eq!(std::mem::size_of::<Ty>(), 48);
        #[cfg(not(feature = "rust-import"))]
        assert!(
            std::mem::size_of::<super::Op>() < 600,
            "pending operation size: {}",
            std::mem::size_of::<super::Op>()
        );
        #[cfg(feature = "rust-import")]
        assert_eq!(std::mem::size_of::<super::Op>(), 320);
        #[cfg(not(feature = "rust-import"))]
        assert!(
            std::mem::size_of::<super::ExecutableOp>() < 1_112,
            "executable operation size: {}",
            std::mem::size_of::<super::ExecutableOp>()
        );
        #[cfg(feature = "rust-import")]
        assert_eq!(std::mem::size_of::<super::ExecutableOp>(), 624);
    }

    #[cfg(feature = "rust-import")]
    #[test]
    fn host_type_clones_share_one_parser_proof_allocation() {
        let facade = imported_alias_facade(false);
        let definition = facade
            .types()
            .find(|definition| definition.path() == "aliases::First")
            .expect("the alias fixture retains First")
            .clone();
        let alias = facade
            .types()
            .find(|definition| definition.path() == "aliases::Second")
            .expect("the alias fixture retains Second")
            .clone();

        reset_host_ty_proof_allocations();
        let ty = Ty::Host {
            proof: HostTyProof::new(definition.clone(), Some(alias.clone()), None),
            arguments: Box::default(),
        };
        let clones = std::iter::repeat_with(|| ty.clone())
            .take(64)
            .collect::<Vec<_>>();
        assert_eq!(clones.len(), 64);
        assert_eq!(host_ty_proof_allocations(), 1);

        let _second_resolution = Ty::Host {
            proof: HostTyProof::new(definition, Some(alias), None),
            arguments: Box::default(),
        };
        assert_eq!(
            host_ty_proof_allocations(),
            2,
            "each independent resolution allocates exactly one proof, no more than the former canonical Definition Rc",
        );
    }

    #[cfg(feature = "sync")]
    #[test]
    fn executable_ops_remain_send_and_sync() {
        const fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<super::ExecutableOp>();
    }

    #[cfg(feature = "rust-import")]
    #[test]
    fn imported_struct_fields_variants_and_methods_remain_symbolic() {
        let mut engine = Engine::new();
        engine
            .register_host_facade(imported_facade())
            .expect("symbolic imported facade installs");
        let program = imported_program(
            &engine,
            r#"
use imported::{Choice, Pair};
let pair = Pair { left: 1, flag: true };
pair.left;
pair.shared();
Choice::Count(2)
"#,
        );
        let uses = symbolic_imported(&engine, &program)
            .unwrap_or_else(|failure| panic!("symbolic imported source: {}", failure.into_error()));
        let paths = uses
            .iter()
            .map(|used| used.path.as_str())
            .collect::<Vec<_>>();

        assert_eq!(
            paths,
            [
                "imported::Choice",
                "imported::Pair",
                "imported::Pair",
                "imported::Pair::shared",
            ]
        );
        assert!(uses.iter().all(|used| !used.span.is_none()));
    }

    #[cfg(feature = "rust-import")]
    #[test]
    fn imported_struct_update_syntax_reads_the_remaining_fields_from_base() {
        let mut engine = Engine::new();
        engine
            .register_host_facade(imported_facade())
            .expect("symbolic imported facade installs");
        let program = imported_program(
            &engine,
            r#"
use imported::Pair;
let base = Pair { left: 1, flag: true };
let pair = Pair { left: 2, ..base };
pair.left
"#,
        );
        symbolic_imported(&engine, &program)
            .unwrap_or_else(|failure| panic!("symbolic imported source: {}", failure.into_error()));
        let program = imported_program(
            &engine,
            "use imported::Pair;\nlet pair = Pair { left: 2 };\npair.left\n",
        );
        assert!(symbolic_imported(&engine, &program).is_err());
    }

    #[cfg(feature = "rust-import")]
    #[test]
    fn imported_structure_stays_non_executable_before_generated_adapter_admission() {
        let mut engine = Engine::new();
        engine
            .register_host_facade(imported_facade())
            .expect("symbolic imported facade installs");
        let error = engine
            .compile(
                r#"
use imported::Pair;
pub fn run() {
    let pair = Pair { left: 1, flag: true };
    pair.left;
}
"#,
            )
            .expect_err("structure is rejected before generated artifact admission");
        assert!(
            matches!(
                error.err_type(),
                ParseErrorType::Resolution(crate::resolve::Error::InvalidHostOperation {
                    reason,
                    ..
                }) if reason.contains("constructing it is not supported in Rawr yet")
            ),
            "unexpected pre-admission rejection: {error:?}"
        );
    }

    #[cfg(feature = "rust-import")]
    #[test]
    fn fully_qualified_nested_imported_type_owner_remains_symbolic() {
        let mut engine = Engine::new();
        engine
            .register_host_facade(imported_facade())
            .expect("symbolic imported facade installs");
        let program = imported_program(&engine, "imported::domain::State::new(20, true)");
        let uses = symbolic_imported(&engine, &program).unwrap_or_else(|failure| {
            panic!(
                "fully qualified nested imported type stays exact: {}",
                failure.into_error()
            )
        });
        assert_eq!(
            uses.iter()
                .map(|used| used.path.as_str())
                .collect::<Vec<_>>(),
            ["imported::domain::State", "imported::domain::State::new"]
        );
        let (executable, collected) = resolve::lower_package_with_uses(
            &engine,
            program,
            &mut host::TestImportAllocation::default(),
            None,
            OptimizationLevel::None,
        )
        .unwrap_or_else(|failure| panic!("qualified owner executable: {}", failure.into_error()));
        assert_eq!(collected.uses, uses);
        assert!(collected.uses.iter().any(|used| {
            used.path == "imported::domain::State"
                && used.selector == rawr_import::surface::identity::Selector::Declaration
        }));
        assert_eq!(executable.runtime_requirements().len(), 1);
    }

    #[cfg(feature = "rust-import")]
    #[test]
    fn imported_aliases_preserve_origin_through_recursive_methods_in_both_install_orders() {
        for alias_first in [false, true] {
            for alias in ["First", "Second"] {
                let mut engine = Engine::new();
                engine
                    .register_host_facade(imported_alias_facade(alias_first))
                    .expect("same-definition aliases install");
                let source = format!(
                    "use aliases::{alias}; let value = {alias} {{ value: 1 }}; value.recursive().value"
                );
                let program = imported_program(&engine, &source);
                let uses = symbolic_imported(&engine, &program).unwrap_or_else(|failure| {
                    panic!("alias origin stays exact: {}", failure.into_error())
                });
                let paths = uses
                    .iter()
                    .map(|used| used.path.as_str())
                    .collect::<Vec<_>>();
                assert_eq!(
                    paths,
                    [
                        format!("aliases::{alias}"),
                        format!("aliases::{alias}"),
                        format!("aliases::{alias}::recursive"),
                    ]
                );
            }
        }
    }

    #[cfg(feature = "rust-import")]
    #[test]
    fn semantic_imported_returns_defer_alias_selection_in_both_install_orders() {
        for alias_first in [false, true] {
            let mut engine = Engine::new();
            engine
                .register_host_facade(imported_alias_facade(alias_first))
                .expect("same-definition aliases install");

            let program = imported_program(&engine, "use aliases::make; let value = make();");
            let uses = symbolic_imported(&engine, &program).unwrap_or_else(|failure| {
                panic!("pathless return stays valid: {}", failure.into_error())
            });
            assert_eq!(
                uses.iter()
                    .map(|used| used.path.as_str())
                    .collect::<Vec<_>>(),
                ["aliases::make"]
            );

            // A field reads the same through every alias of one type, as a
            // method does, so a pathless return's field resolves (D561).
            let program = imported_program(&engine, "use aliases::make; make().value");
            let uses = symbolic_imported(&engine, &program).unwrap_or_else(|failure| {
                panic!(
                    "a pathless return's field resolves: {}",
                    failure.into_error()
                )
            });
            assert_eq!(
                uses.iter()
                    .map(|used| used.path.as_str())
                    .collect::<Vec<_>>(),
                ["aliases::First", "aliases::make"]
            );

            // A member no alias has still names every alias it could be.
            let program = imported_program(&engine, "use aliases::make; make().missing");
            let error = symbolic_imported(&engine, &program)
                .expect_err("a member no alias has selects no exact alias")
                .into_error();
            let ParseErrorType::Resolution(crate::resolve::Error::InvalidHostOperation {
                item,
                reason,
                operation,
            }) = error.err_type()
            else {
                panic!("alias ambiguity is a positioned host operation error: {error}");
            };
            assert_eq!(item, "aliases::First");
            assert!(reason.contains("aliases::First, aliases::Second"));
            assert!(!operation.is_none());
        }
    }

    #[cfg(feature = "rust-import")]
    #[test]
    fn local_enum_payload_patterns_bind_imported_values() {
        for pattern in ["pair", "_"] {
            let mut engine = Engine::new();
            engine
                .register_host_facade(imported_facade())
                .expect("symbolic imported facade installs");
            let source = format!(
                r#"
use imported::Pair;
enum Payload {{ Pair(Pair), Empty }}
let payload = Payload::Pair(Pair {{ left: 1, flag: true }});
match payload {{ Payload::Pair({pattern}) => 1, Payload::Empty => 0 }}
"#
            );
            let program = imported_program(&engine, &source);
            symbolic_imported(&engine, &program).unwrap_or_else(|failure| {
                panic!("imported payload pattern: {}", failure.into_error())
            });
        }
    }

    /// A crate type exported under several paths, as iced's `Vector` is,
    /// prints as the program imports it, else by its shortest path (#508).
    #[cfg(feature = "rust-import")]
    #[test]
    fn a_type_of_several_paths_prints_by_one_of_them() {
        fn mismatch(source: &str) -> (String, String) {
            let mut engine = Engine::new();
            engine
                .register_host_facade(vector_alias_facade())
                .expect("a type under two paths installs");
            let program = imported_program(&engine, source);
            let error = symbolic_imported(&engine, &program)
                .expect_err("an i64 can't hold a Vector")
                .into_error();
            let ParseErrorType::Resolution(crate::resolve::Error::TypeMismatch {
                expected,
                actual,
                ..
            }) = error.err_type()
            else {
                panic!("a type mismatch: {error}");
            };
            (expected.to_string(), actual.to_string())
        }

        // No `use` names the type: its shortest path.
        assert_eq!(
            mismatch("use vectors::make; let value: i64 = make();"),
            ("i64".into(), "vectors::Vector<f32>".into())
        );
        // A `use` of the longer path: that path.
        assert_eq!(
            mismatch("use vectors::Deep::Vector; use vectors::make; let value: i64 = make();"),
            ("i64".into(), "vectors::Deep::Vector<f32>".into())
        );
    }

    /// A crate type a macro's `$crate` reaches through its crate's hidden
    /// segment prints by a public path when the dependency has one, else
    /// from its own crate's root, as rustc does: no message names the
    /// segment (#563).
    #[cfg(feature = "rust-import")]
    #[test]
    fn a_type_behind_a_hidden_crate_segment_prints_by_a_public_path() {
        fn mismatch(paths: &[&str]) -> (String, String) {
            let mut builder =
                facade::Builder::new("vectors", facade::Abi::new(1).unwrap()).unwrap();
            let mut allocation = host::TestImportAllocation::default();
            let identity = host::type_def::import::Identity::new([0x57; 32]);
            for module in [HIDDEN, "deep", "deep::er"] {
                builder.imported_module(module, &mut allocation).unwrap();
            }
            for path in paths {
                builder
                    .imported_type(
                        path,
                        identity,
                        &[],
                        Box::from(["T".into()]),
                        Box::from([None]),
                        host::type_def::Shape::Opaque.into(),
                        &mut allocation,
                    )
                    .unwrap();
            }
            let mut make = signature::Builder::new();
            make.output(type_ref::Type::imported(
                identity,
                [type_ref::Type::builtin(type_ref::Builtin::F32)],
            ));
            builder
                .item(
                    item::Definition::function("make", make.finish())
                        .unwrap()
                        .imported(rawr_import::surface::identity::Selector::Unavailable),
                )
                .unwrap();
            let mut engine = Engine::new();
            engine
                .register_host_facade(builder.finish().unwrap())
                .expect("the facade installs");
            let source = "use vectors::make; let value: i64 = make();";
            let program = imported_program(&engine, source);
            let error = symbolic_imported(&engine, &program)
                .expect_err("an i64 can't hold a Vector")
                .into_error();
            assert!(!error.to_string().contains("__rawr"), "{error}");
            let ParseErrorType::Resolution(crate::resolve::Error::TypeMismatch { actual, .. }) =
                error.err_type()
            else {
                panic!("a type mismatch: {error}");
            };
            let report = crate::diagnostic::Report::from_parse(
                crate::diagnostic::Source::new("main.rs", source).unwrap(),
                &error,
            )
            .expect("the error reports at its span");
            assert!(!report.message().contains("__rawr"), "{}", report.message());
            (error.to_string(), actual.to_string())
        }
        const HIDDEN: &str = "__rawr_crate_linear_00000000000000ab";

        // A public path, though the hidden one is shorter.
        let (_, actual) = mismatch(&[&format!("{HIDDEN}::Vector"), "deep::er::Vector"]);
        assert_eq!(actual, "vectors::deep::er::Vector<f32>");
        // Only the hidden path: the defining crate's own.
        let (message, actual) = mismatch(&[&format!("{HIDDEN}::Vector")]);
        assert_eq!(actual, "linear::Vector<f32>");
        assert!(message.contains("linear::Vector<f32>"), "{message}");
    }

    /// `Vector<T>` at `vectors::Deep::Vector` and `vectors::Vector`, and
    /// `make() -> Vector<f32>`.
    #[cfg(feature = "rust-import")]
    fn vector_alias_facade() -> facade::Definition {
        let mut builder = facade::Builder::new("vectors", facade::Abi::new(1).unwrap()).unwrap();
        let mut allocation = host::TestImportAllocation::default();
        let identity = host::type_def::import::Identity::new([0x56; 32]);
        builder.imported_module("Deep", &mut allocation).unwrap();
        for path in ["Deep::Vector", "Vector"] {
            builder
                .imported_type(
                    path,
                    identity,
                    &[],
                    Box::from(["T".into()]),
                    Box::from([None]),
                    host::type_def::Shape::Opaque.into(),
                    &mut allocation,
                )
                .unwrap();
        }
        let mut make = signature::Builder::new();
        make.output(type_ref::Type::imported(
            identity,
            [type_ref::Type::builtin(type_ref::Builtin::F32)],
        ));
        builder
            .item(
                item::Definition::function("make", make.finish())
                    .unwrap()
                    .imported(rawr_import::surface::identity::Selector::Unavailable),
            )
            .unwrap();
        builder.finish().unwrap()
    }

    #[cfg(feature = "rust-import")]
    #[test]
    fn imported_structure_and_receiver_failures_keep_rawr_positions() {
        for (source, expected) in [
            (
                "use imported::Pair; let pair = Pair { left: 1, flag: true }; pair.missing",
                "Pair::missing",
            ),
            (
                "use imported::Pair; let pair = Pair { left: 1, flag: true }; pair.mutable()",
                "as mutable",
            ),
            (
                "use imported::Pair; let pair = Pair { left: 1, flag: true }; pair.owned(); pair.shared()",
                "moved value",
            ),
        ] {
            let mut engine = Engine::new();
            engine
                .register_host_facade(imported_facade())
                .expect("symbolic imported facade installs");
            let program = imported_program(&engine, source);
            let failure = symbolic_imported(&engine, &program)
                .expect_err("invalid imported use is rejected");
            let error = failure.into_error();

            assert!(!error.position().is_none(), "{source}");
            assert!(error.to_string().contains(expected), "{error}");
        }
    }

    #[cfg(feature = "rust-import")]
    #[test]
    fn imported_ownership_consumes_exactly_non_copy_values() {
        for source in [
            "use imported::{Pair, take_pair}; let pair = Pair { left: 1, flag: true }; take_pair(pair); take_pair(pair)",
            "use imported::Pair; let first = Pair { left: 1, flag: true }; let second = Pair { left: 2, flag: false }; first.combine(second); first.combine(second)",
            "use imported::{Holder, Pair}; let pair = Pair { left: 1, flag: true }; let holder = Holder { pair }; pair.shared()",
            "use imported::{Holder, Pair}; let pair = Pair { left: 1, flag: true }; let holder = Holder { pair }; holder.pair; holder.pair",
            "use imported::Pair; let first = Pair { left: 1, flag: true }; let second = first; first.shared()",
            "use imported::Pair; let mut destination = Pair { left: 1, flag: true }; let source = Pair { left: 2, flag: false }; destination = source; source.shared()",
            "use imported::Pair; let pair = Pair { left: 1, flag: true }; let moved = if true { pair } else { Pair { left: 2, flag: false } }; pair.shared()",
        ] {
            let mut engine = Engine::new();
            engine
                .register_host_facade(imported_facade())
                .expect("symbolic imported facade installs");
            let program = imported_program(&engine, source);
            let failure = symbolic_imported(&engine, &program)
                .expect_err("a non-Copy imported value cannot be consumed twice");
            let error = failure.into_error();
            assert!(!error.position().is_none(), "{source}");
            assert!(error.to_string().contains("moved value"), "{error}");
        }

        for source in [
            "use imported::Pair; let pair = Pair { left: 1, flag: true }; pair.left; pair.shared(); pair.shared()",
            "use imported::Pair; let mut pair = Pair { left: 1, flag: true }; pair.mutable(); pair.mutable()",
            "use imported::{CopyPair, take_copy}; let copy = CopyPair { value: 1 }; take_copy(copy); take_copy(copy); copy.owned_copy(); copy.owned_copy()",
            "use imported::Pair; let mut destination = Pair { left: 1, flag: true }; let source = Pair { left: 2, flag: false }; destination = source; destination.shared()",
            "use imported::CopyPair; let first = CopyPair { value: 1 }; let second = first; first.value + second.value",
        ] {
            let mut engine = Engine::new();
            engine
                .register_host_facade(imported_facade())
                .expect("symbolic imported facade installs");
            let program = imported_program(&engine, source);
            symbolic_imported(&engine, &program)
                .unwrap_or_else(|failure| panic!("exact imported ownership: {}", failure.into_error()));
        }
    }

    #[cfg(feature = "rust-import")]
    #[test]
    fn derived_debug_takes_an_imported_field_whose_type_implements_debug() {
        let mut engine = Engine::new();
        engine
            .register_host_facade(imported_facade())
            .expect("symbolic imported facade installs");
        let program = imported_program(
            &engine,
            "use imported::CopyPair; #[derive(Debug)] struct Panel { pair: CopyPair } let panel = Panel { pair: CopyPair { value: 1 } }; format!(\"{panel:?} {:?}\", panel.pair)",
        );
        symbolic_imported(&engine, &program)
            .unwrap_or_else(|failure| panic!("imported Debug field: {}", failure.into_error()));

        let Err(error) = resolve::resolve(
            engine
                .parse_unit("use imported::Pair; #[derive(Debug)] struct Panel { pair: Pair } 0")
                .expect("imported fixture parses"),
            &engine.host_registry,
        ) else {
            panic!("an imported type without Debug");
        };
        assert!(
            matches!(&error, resolve::Error::InvalidDerive { capability, .. } if capability.as_str() == "Debug"),
            "{error:?}"
        );
    }

    /// A module re-exports a crate enum's variant by `pub use`, by name and
    /// by a glob, and the root names it through the module (#1345).
    #[cfg(feature = "rust-import")]
    #[test]
    fn a_module_re_exports_an_imported_enum_variant() {
        let mut engine = Engine::new();
        engine
            .register_host_facade(imported_facade())
            .expect("symbolic imported facade installs");
        let text = r#"mod picks {
    pub use imported::Choice::Count;
    pub mod all {
        pub use imported::Choice::*;
    }
}
use picks::all::Empty;
fn picked(empty: bool) -> imported::Choice {
    if empty { Empty } else { picks::Count(2) }
}
fn counted() -> imported::Choice {
    picks::all::Count(3)
}
"#;
        let path = std::path::PathBuf::from("/main.rs");
        let snapshot =
            crate::package::snapshot::Snapshot::new(path.clone(), vec![(path, text.into())])
                .unwrap();
        let graph = crate::package::Graph::load_snapshot(&engine, &snapshot).unwrap();
        let program = resolve::package::resolve(graph, &engine.host_registry)
            .unwrap_or_else(|failure| panic!("re-exported variants: {:?}", failure.error));
        let uses = symbolic_imported(&engine, &program)
            .unwrap_or_else(|failure| panic!("symbolic imported source: {}", failure.into_error()));
        assert!(
            uses.iter()
                .all(|used| used.path.as_str() == "imported::Choice"),
            "{:?}",
            uses.iter()
                .map(|used| used.path.as_str())
                .collect::<Vec<_>>()
        );
        // Two outputs and three variants.
        assert_eq!(uses.len(), 5);
    }

    #[cfg(feature = "rust-import")]
    #[test]
    fn an_imported_type_is_send_as_an_iced_message_needs() {
        let mut engine = Engine::new();
        engine
            .register_host_facade(imported_facade())
            .expect("symbolic imported facade installs");
        for path in ["imported::Choice", "imported::Pair"] {
            let Some(host::Entry::Type(definition)) = engine.host_registry.entry(path) else {
                panic!("{path} is an imported type");
            };
            for capability in [
                crate::capability::Kind::Send,
                crate::capability::Kind::Static,
            ] {
                assert!(
                    definition.capabilities().contains(capability),
                    "{path} {capability:?}"
                );
            }
        }
    }

    #[cfg(feature = "rust-import")]
    #[test]
    fn derived_clone_takes_an_imported_field_whose_type_implements_clone() {
        let mut engine = Engine::new();
        engine
            .register_host_facade(imported_facade())
            .expect("symbolic imported facade installs");
        resolve::resolve(
            engine
                .parse_unit(
                    "use imported::Choice; #[derive(Clone)] enum Message { Picked(Choice) } \
                     fn again(message: &Message) -> Message { message.clone() } 0",
                )
                .expect("imported fixture parses"),
            &engine.host_registry,
        )
        .unwrap_or_else(|error| panic!("imported Clone field: {error:?}"));

        let Err(error) = resolve::resolve(
            engine
                .parse_unit("use imported::Pair; #[derive(Clone)] struct Panel { pair: Pair } 0")
                .expect("imported fixture parses"),
            &engine.host_registry,
        ) else {
            panic!("an imported type without Clone");
        };
        assert!(
            matches!(&error, resolve::Error::InvalidDerive { capability, .. } if capability.as_str() == "Clone"),
            "{error:?}"
        );
    }

    mod stress {
        pub(super) struct Element;
        pub(super) struct Container;
        pub(super) struct Text;
        pub(super) struct Style;
    }

    #[test]
    fn inference_transactions_replay_winners_without_leaking_nested_changes() {
        let mut inference = Inference::default();
        let Ty::Infer(stable) = inference.fresh() else {
            panic!("fresh inference type must carry its binding id");
        };
        inference.bind(stable, Ty::Int(super::Int::I64));

        let candidate = inference.checkpoint();
        let fresh = inference.fresh();
        let Ty::Infer(fresh_id) = fresh else {
            panic!("fresh inference type must carry its binding id");
        };
        inference.bind(fresh_id, Ty::Bool);
        let patch = inference.patch(candidate);
        inference.rollback(candidate);

        assert_eq!(inference.next, 1);
        assert_eq!(
            inference.resolve(&Ty::Infer(stable)),
            Ty::Int(super::Int::I64)
        );
        assert_eq!(inference.resolve(&fresh), fresh);

        let statement = inference.checkpoint();
        inference.apply(patch);
        let nested = inference.checkpoint();
        inference.bind(stable, Ty::String);
        inference.commit(nested);
        assert_eq!(inference.resolve(&Ty::Infer(stable)), Ty::String);
        assert_eq!(inference.resolve(&fresh), Ty::Bool);
        inference.rollback(statement);

        assert_eq!(inference.next, 1);
        assert_eq!(
            inference.resolve(&Ty::Infer(stable)),
            Ty::Int(super::Int::I64)
        );
        assert_eq!(inference.resolve(&fresh), fresh);
        assert!(inference.journal.is_empty());
    }

    #[test]
    fn gallery_shaped_compiler_work_scales_linearly() {
        let mut builder = facade::Builder::new("stress", facade::Abi::new(1).unwrap()).unwrap();
        builder
            .type_::<stress::Element>("Element", &["Message"], &[])
            .unwrap();
        builder
            .type_::<stress::Container>("Container", &["Message"], &[])
            .unwrap();
        builder.type_::<stress::Text>("Text", &[], &[]).unwrap();
        builder.type_::<stress::Style>("Style", &[], &[]).unwrap();
        builder
            .project_with("Text", "Element", &[host::type_def::Argument::fresh()])
            .unwrap();
        builder
            .project("Container", "Element", &["Message"])
            .unwrap();

        for input in [
            type_ref::Type::reference(false, type_ref::Type::builtin(type_ref::Builtin::String)),
            type_ref::Type::builtin(type_ref::Builtin::I64),
            type_ref::Type::builtin(type_ref::Builtin::Bool),
        ] {
            let mut text = signature::Builder::new();
            text.parameter(input)
                .output(type_ref::Type::registered("Text").unwrap());
            builder
                .item(item::Definition::function("text", text.finish()).unwrap())
                .unwrap();
        }
        for input in [
            type_ref::Type::applied("Element", [type_ref::Type::parameter("Message").unwrap()])
                .unwrap(),
            type_ref::Type::builtin(type_ref::Builtin::I64),
            type_ref::Type::builtin(type_ref::Builtin::Bool),
        ] {
            let mut container = signature::Builder::new();
            container.type_parameter("Message", &[]).unwrap();
            container
                .parameter(input)
                .parameter(type_ref::Type::parameter("Message").unwrap())
                .output(
                    type_ref::Type::applied(
                        "Container",
                        [type_ref::Type::parameter("Message").unwrap()],
                    )
                    .unwrap(),
                );
            builder
                .item(item::Definition::function("container", container.finish()).unwrap())
                .unwrap();
        }
        builder
            .item(
                item::Definition::value("rounded", type_ref::Type::registered("Style").unwrap())
                    .unwrap(),
            )
            .unwrap();
        let mut apply_style = signature::Builder::new();
        apply_style.type_parameter("Message", &[]).unwrap();
        apply_style
            .parameter(type_ref::Type::registered("Style").unwrap())
            .output(
                type_ref::Type::applied(
                    "Container",
                    [type_ref::Type::parameter("Message").unwrap()],
                )
                .unwrap(),
            );
        builder
            .item(
                item::Definition::method(
                    "Container::style",
                    receiver::Mode::Owned,
                    apply_style.finish(),
                )
                .unwrap(),
            )
            .unwrap();

        let mut engine = Engine::new();
        engine
            .register_host_facade(builder.finish().unwrap())
            .unwrap();
        let mut observed = Vec::new();
        for rows in [64, 128] {
            let mut source = String::from(
                "use stress::{container, rounded, text};\nenum Message { Tick }\npub fn view() -> stress::Element<Message> {\n",
            );
            for index in 0..rows {
                writeln!(
                    source,
                    "let row_{index} = container(text(\"row {index}\"), Message::Tick).style(rounded);"
                )
                .unwrap();
            }
            writeln!(source, "row_{}\n}}", rows - 1).unwrap();

            reset_inference_metrics();
            reset_model_clones();
            engine.compile_executable(&source).unwrap();
            let (writes, peak) = inference_metrics();
            observed.push((writes, peak, model_clones()));
        }
        let [(small_writes, small_peak, small_model_clones), (large_writes, large_peak, large_model_clones)] =
            observed.as_slice()
        else {
            panic!("both structural workload sizes must be measured");
        };
        eprintln!(
            "journal writes/peak/model clones: 64 rows={small_writes}/{small_peak}/{small_model_clones}, 128 rows={large_writes}/{large_peak}/{large_model_clones}"
        );
        assert!(*small_writes > 64);
        assert!(*large_writes <= small_writes * 2 + 32);
        assert!(
            *small_peak <= 16,
            "small workload journal peaked at {small_peak}"
        );
        assert!(
            *large_peak <= 16,
            "large workload journal peaked at {large_peak}"
        );
        assert_eq!(small_model_clones, large_model_clones);
        assert!(
            *large_model_clones <= 2,
            "support closures retained {large_model_clones} owned model clones"
        );
    }

    #[test]
    fn external_signature_outputs_retain_both_exact_facade_dependencies() {
        let mut base = facade::Builder::new("base", facade::Abi::new(1).unwrap()).unwrap();
        base.type_::<ExternalValue>("Value", &["Message"], &[])
            .unwrap();
        let base = base.finish().unwrap();
        let exported = base.export_type("Value").unwrap();

        let mut make = signature::Builder::new();
        make.type_parameter("Message", &[]).unwrap();
        make.output(
            type_ref::Type::external(exported, [type_ref::Type::parameter("Message").unwrap()])
                .unwrap(),
        );
        let mut extension =
            facade::Builder::new("extension", facade::Abi::new(1).unwrap()).unwrap();
        extension
            .item(item::Definition::function("make", make.finish()).unwrap())
            .unwrap();

        let mut engine = Engine::new();
        engine.register_host_facade(base).unwrap();
        engine
            .register_host_facade(extension.finish().unwrap())
            .unwrap();
        let executable = engine
            .compile_executable(
                r#"
use base::Value;
use extension::make;
enum Message { Tick }
pub fn view() -> Value<Message> { make() }
"#,
            )
            .unwrap();
        let roots = executable
            .host_dependencies()
            .iter()
            .map(host::facade::Dependency::root)
            .collect::<Vec<_>>();

        assert_eq!(roots, ["base", "extension"]);
    }

    #[test]
    fn user_cannot_spell_compiler_private_argument_temporaries() {
        let error = Engine::new()
            .compile("pub fn run() { let __rawr_nominal_op_0_arg_0 = 1; }")
            .unwrap_err();

        assert!(matches!(
            error.err_type(),
            ParseErrorType::Resolution(resolve::Error::UnsupportedNominalSyntax {
                construct,
                ..
            }) if construct == "compiler-private nominal intrinsic"
        ));
    }

    #[test]
    fn private_call_temporaries_and_inclusive_ranges_preserve_operand_order_once() {
        for level in [
            OptimizationLevel::None,
            OptimizationLevel::Simple,
            OptimizationLevel::Full,
        ] {
            let mut engine = Engine::new();
            engine.set_optimization_level(level);
            engine.register_host_facade(ordering_facade()).unwrap();
            let engine = Shared::new(engine);
            let authority = engine
                .compile_authority(
                    r#"
use order::{make_receiver, mark, range_width};
enum Gate { Closed, Open }
pub fn skipped() -> i64 {
    match Gate::Closed {
        Gate::Open if mark(9) == 9 => 1,
        _ => 0,
    }
}
pub fn run() -> i64 {
    make_receiver().combine(mark(1), mark(2))
}
pub fn inclusive() -> i64 {
    range_width(mark(1) + mark(10) ..= mark(2) + mark(20))
}
"#,
                )
                .unwrap();

            let events = Arc::new(Mutex::new(Vec::new()));
            let mut runtime = host::runtime::Builder::new(ordering_facade());
            let observed = events.clone();
            runtime
                .bind("order::make_receiver", move |_| {
                    observed.lock().unwrap().push(0);
                    Ok(Dynamic::from(OrderedValue))
                })
                .unwrap();
            let observed = events.clone();
            runtime
                .bind("order::mark", move |args| {
                    observed.lock().unwrap().push(args[0].as_int().unwrap());
                    Ok(args[0].take())
                })
                .unwrap();
            let observed = events.clone();
            runtime
                .bind("order::Value::combine", move |_| {
                    observed.lock().unwrap().push(3);
                    Ok(Dynamic::from(42_i64))
                })
                .unwrap();
            runtime
                .bind("order::range_width", |args| {
                    let range = args[0]
                        .read_lock::<std::ops::RangeInclusive<crate::INT>>()
                        .unwrap();
                    assert_eq!(*range, 11..=22);
                    Ok(Dynamic::from(*range.end() - *range.start()))
                })
                .unwrap();
            let mut registry = host::runtime::Registry::new();
            registry.install(runtime.finish()).unwrap();
            let registry = Shared::new(registry);

            let skipped = authority
                .items()
                .find(|item| item.key().name() == "skipped")
                .unwrap()
                .clone();
            let run = authority
                .items()
                .find(|item| item.key().name() == "run")
                .unwrap()
                .clone();
            let inclusive = authority
                .items()
                .find(|item| item.key().name() == "inclusive")
                .unwrap()
                .clone();
            let skipped_output = authority
                .invoke_bound(&skipped, &mut [], registry.clone())
                .unwrap();
            assert_eq!(skipped_output.as_int().unwrap(), 0);
            assert!(events.lock().unwrap().is_empty(), "{level:?}");
            let output = authority
                .invoke_bound(&run, &mut [], registry.clone())
                .unwrap();

            assert_eq!(output.as_int().unwrap(), 42);
            assert_eq!(*events.lock().unwrap(), [0, 1, 2, 3], "{level:?}");

            events.lock().unwrap().clear();
            let output = authority
                .invoke_bound(&inclusive, &mut [], registry.clone())
                .unwrap();
            assert_eq!(output.as_int().unwrap(), 11);
            assert_eq!(*events.lock().unwrap(), [1, 10, 2, 20], "{level:?}");
        }
    }

    #[test]
    fn usage_metadata_perturbs_exact_proofs_and_rejects_unsafe_elision() {
        let ordinary = usage_facade(false, false, false).unwrap();
        let required = usage_facade(true, false, false).unwrap();
        assert_ne!(ordinary.fingerprint(), required.fingerprint());

        let discardable = usage_facade(true, true, false).unwrap();
        assert_ne!(required.fingerprint(), discardable.fingerprint());

        let elidable = usage_facade(true, true, true).unwrap();
        assert_ne!(discardable.fingerprint(), elidable.fingerprint());

        assert!(matches!(
            usage_facade(true, false, true),
            Err(host::Error::InvalidElidableSignature { .. })
        ));

        let mut receiver = usage_builder(true);
        receiver
            .item(
                item::Definition::method(
                    "Value::produce",
                    receiver::Mode::Owned,
                    value_signature().finish(),
                )
                .unwrap()
                .discardable()
                .elidable(),
            )
            .unwrap();
        assert!(matches!(
            receiver.finish(),
            Err(host::Error::InvalidElidableSignature { .. })
        ));

        let mut parameter = usage_builder(true);
        let mut definition = value_signature();
        definition.parameter(type_ref::Type::builtin(type_ref::Builtin::I64));
        parameter
            .item(
                item::Definition::function("produce", definition.finish())
                    .unwrap()
                    .discardable()
                    .elidable(),
            )
            .unwrap();
        assert!(matches!(
            parameter.finish(),
            Err(host::Error::InvalidElidableSignature { .. })
        ));
    }

    #[test]
    fn usage_metadata_retains_required_work_and_erases_exact_inert_work() {
        let mut engine = Engine::new();
        engine.register_host_facade(execution_facade()).unwrap();
        let engine = Shared::new(engine);
        let authority = engine
            .compile_authority(
                r#"
use effect::{idle, identity, required};
pub fn run() -> i64 {
    required();
    idle();
    identity(42)
}
"#,
            )
            .unwrap();

        assert_eq!(authority.runtime_requirements().len(), 2);

        let required_calls = Arc::new(AtomicUsize::new(0));
        let idle_calls = Arc::new(AtomicUsize::new(0));
        let identity_calls = Arc::new(AtomicUsize::new(0));
        let mut runtime = host::runtime::Builder::new(execution_facade());
        let calls = required_calls.clone();
        runtime
            .bind("effect::required", move |_| {
                calls.fetch_add(1, Ordering::Relaxed);
                Ok(Dynamic::from(RequiredValue))
            })
            .unwrap();
        let calls = idle_calls.clone();
        runtime
            .bind("effect::idle", move |_| {
                calls.fetch_add(1, Ordering::Relaxed);
                Ok(Dynamic::from(GenericValue))
            })
            .unwrap();
        let calls = identity_calls.clone();
        runtime
            .bind("effect::identity", move |args| {
                calls.fetch_add(1, Ordering::Relaxed);
                Ok(args[0].take())
            })
            .unwrap();
        let mut registry = host::runtime::Registry::new();
        registry.install(runtime.finish()).unwrap();

        let run = authority
            .items()
            .find(|item| item.key().name() == "run")
            .unwrap()
            .clone();
        let output = authority
            .invoke_bound(&run, &mut [], Shared::new(registry))
            .unwrap();

        assert_eq!(output.as_int().unwrap(), 42);
        assert_eq!(required_calls.load(Ordering::Relaxed), 1);
        assert_eq!(idle_calls.load(Ordering::Relaxed), 0);
        assert_eq!(identity_calls.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn recursive_member_schema_changes_invalidate_retained_values() {
        let engine = Engine::new();
        let old = resolve::resolve(
            engine
                .parse_unit(
                    "#[derive(Default)] struct Inner { value: i64 } #[derive(Default)] struct Outer { inner: Inner }",
                )
                .unwrap(),
            &host::Registry::new(),
        )
        .unwrap();
        let new = resolve::resolve(
            engine
                .parse_unit(
                    "#[derive(Default)] struct Inner { value: bool } #[derive(Default)] struct Outer { inner: Inner }",
                )
                .unwrap(),
            &host::Registry::new(),
        )
        .unwrap();
        let old_generation = crate::program::GenerationId::issue();
        let new_generation = crate::program::GenerationId::issue();
        let old_runtime = crate::program::model::Model::new(old_generation, &old);
        let new_runtime = crate::program::model::Model::new(new_generation, &new);
        let old_models = Models::new(
            &old,
            old_runtime,
            Shared::new(host::Registry::new()),
            &Engine::new_raw(),
        );
        let new_models = Models::new(
            &new,
            new_runtime,
            Shared::new(host::Registry::new()),
            &Engine::new_raw(),
        );
        let old_outer = old_models.get("Outer").unwrap();
        let new_outer = new_models.get("Outer").unwrap();
        let retained = default_value(old_outer, &old_models, &mut Vec::new()).unwrap();

        assert_eq!(
            old_outer.definition().schema(),
            new_outer.definition().schema()
        );
        assert!(!matches_runtime_model(&retained, new_outer, &new_models));
    }

    fn usage_facade(
        required: bool,
        discardable: bool,
        elidable: bool,
    ) -> Result<facade::Definition, host::Error> {
        let mut builder = usage_builder(required);
        let mut definition = item::Definition::function("produce", value_signature().finish())?;
        if discardable {
            definition = definition.discardable();
        }
        if elidable {
            definition = definition.elidable();
        }
        builder.item(definition)?;
        builder.finish()
    }

    #[cfg(feature = "rust-import")]
    fn imported_program(engine: &Engine, source: &str) -> resolve::Program {
        resolve::resolve(
            engine.parse_unit(source).expect("imported fixture parses"),
            &engine.host_registry,
        )
        .expect("imported fixture headers resolve")
    }

    #[cfg(feature = "rust-import")]
    fn imported_facade() -> facade::Definition {
        let mut builder = facade::Builder::new("imported", facade::Abi::new(1).unwrap()).unwrap();
        let mut allocation = host::TestImportAllocation::default();
        let pair_identity = host::type_def::import::Identity::new([0x31; 32]);
        let copy_identity = host::type_def::import::Identity::new([0x33; 32]);
        let state_identity = host::type_def::import::Identity::new([0x35; 32]);
        builder.module("domain").unwrap();
        let pair_shape: Shared<host::type_def::Shape> = host::type_def::Shape::Struct {
            form: host::type_def::Form::Record,
            constructor: Some(rawr_import::surface::identity::Selector::Unavailable),
            fields: Box::from([
                host::type_def::Field::new(
                    "left".into(),
                    type_ref::Type::builtin(type_ref::Builtin::I64),
                    rawr_import::surface::identity::Selector::Unavailable,
                ),
                host::type_def::Field::new(
                    "flag".into(),
                    type_ref::Type::builtin(type_ref::Builtin::Bool),
                    rawr_import::surface::identity::Selector::Unavailable,
                ),
            ]),
            constructible: true,
            unsupported: None,
        }
        .into();
        builder
            .imported_type(
                "Pair",
                pair_identity,
                &[],
                Box::default(),
                Box::default(),
                pair_shape,
                &mut allocation,
            )
            .unwrap();
        builder
            .imported_type(
                "domain::State",
                state_identity,
                &[],
                Box::default(),
                Box::default(),
                host::type_def::Shape::Struct {
                    form: host::type_def::Form::Record,
                    constructor: None,
                    fields: Box::from([
                        host::type_def::Field::new(
                            "dial".into(),
                            type_ref::Type::builtin(type_ref::Builtin::I64),
                            rawr_import::surface::identity::Selector::Unavailable,
                        ),
                        host::type_def::Field::new(
                            "enabled".into(),
                            type_ref::Type::builtin(type_ref::Builtin::Bool),
                            rawr_import::surface::identity::Selector::Unavailable,
                        ),
                    ]),
                    constructible: false,
                    unsupported: None,
                }
                .into(),
                &mut allocation,
            )
            .unwrap();
        let mut state_new = signature::Builder::new();
        state_new
            .parameter(type_ref::Type::builtin(type_ref::Builtin::I64))
            .parameter(type_ref::Type::builtin(type_ref::Builtin::Bool))
            .output(type_ref::Type::imported(state_identity, []));
        builder
            .item(
                item::Definition::associated_function("domain::State::new", state_new.finish())
                    .unwrap()
                    .imported(rawr_import::surface::identity::Selector::Unavailable),
            )
            .unwrap();
        builder
            .imported_type(
                "Holder",
                host::type_def::import::Identity::new([0x34; 32]),
                &[],
                Box::default(),
                Box::default(),
                host::type_def::Shape::Struct {
                    form: host::type_def::Form::Record,
                    constructor: Some(rawr_import::surface::identity::Selector::Unavailable),
                    fields: Box::from([host::type_def::Field::new(
                        "pair".into(),
                        type_ref::Type::imported(pair_identity, []),
                        rawr_import::surface::identity::Selector::Unavailable,
                    )]),
                    constructible: true,
                    unsupported: None,
                }
                .into(),
                &mut allocation,
            )
            .unwrap();
        builder
            .imported_type(
                "CopyPair",
                copy_identity,
                &[
                    crate::capability::Kind::Copy,
                    crate::capability::Kind::Debug,
                    crate::capability::Kind::Clone,
                ],
                Box::default(),
                Box::default(),
                host::type_def::Shape::Struct {
                    form: host::type_def::Form::Record,
                    constructor: Some(rawr_import::surface::identity::Selector::Unavailable),
                    fields: Box::from([host::type_def::Field::new(
                        "value".into(),
                        type_ref::Type::builtin(type_ref::Builtin::I64),
                        rawr_import::surface::identity::Selector::Unavailable,
                    )]),
                    constructible: true,
                    unsupported: None,
                }
                .into(),
                &mut allocation,
            )
            .unwrap();
        builder
            .imported_type(
                "Choice",
                host::type_def::import::Identity::new([0x32; 32]),
                &[crate::capability::Kind::Clone],
                Box::default(),
                Box::default(),
                host::type_def::Shape::Enum(Box::from([
                    host::type_def::Variant::new(
                        "Empty".into(),
                        Box::default(),
                        true,
                        rawr_import::surface::identity::Selector::EnumVariant {
                            index: 0,
                            name: "Empty".into(),
                            form: rawr_import::surface::identity::Form::Unit,
                        },
                    ),
                    host::type_def::Variant::new(
                        "Count".into(),
                        Box::from([host::type_def::Field::new(
                            "0".into(),
                            type_ref::Type::builtin(type_ref::Builtin::I64),
                            rawr_import::surface::identity::Selector::Unavailable,
                        )]),
                        true,
                        rawr_import::surface::identity::Selector::EnumVariant {
                            index: 1,
                            name: "Count".into(),
                            form: rawr_import::surface::identity::Form::Tuple { arity: 1 },
                        },
                    ),
                ]))
                .into(),
                &mut allocation,
            )
            .unwrap();
        for (name, mode) in [
            ("shared", receiver::Mode::Shared),
            ("mutable", receiver::Mode::Mutable),
            ("owned", receiver::Mode::Owned),
        ] {
            let mut signature = signature::Builder::new();
            signature.output(type_ref::Type::imported(pair_identity, []));
            builder
                .item(
                    item::Definition::method(format!("Pair::{name}"), mode, signature.finish())
                        .unwrap()
                        .imported(rawr_import::surface::identity::Selector::Unavailable),
                )
                .unwrap();
        }
        let mut combine = signature::Builder::new();
        combine
            .parameter(type_ref::Type::imported(pair_identity, []))
            .output(type_ref::Type::builtin(type_ref::Builtin::I64));
        builder
            .item(
                item::Definition::method("Pair::combine", receiver::Mode::Shared, combine.finish())
                    .unwrap()
                    .imported(rawr_import::surface::identity::Selector::Unavailable),
            )
            .unwrap();
        let mut take_pair = signature::Builder::new();
        take_pair
            .parameter(type_ref::Type::imported(pair_identity, []))
            .output(type_ref::Type::builtin(type_ref::Builtin::I64));
        builder
            .item(
                item::Definition::function("take_pair", take_pair.finish())
                    .unwrap()
                    .imported(rawr_import::surface::identity::Selector::Unavailable),
            )
            .unwrap();
        let mut take_copy = signature::Builder::new();
        take_copy
            .parameter(type_ref::Type::imported(copy_identity, []))
            .output(type_ref::Type::builtin(type_ref::Builtin::I64));
        builder
            .item(
                item::Definition::function("take_copy", take_copy.finish())
                    .unwrap()
                    .imported(rawr_import::surface::identity::Selector::Unavailable),
            )
            .unwrap();
        let mut owned_copy = signature::Builder::new();
        owned_copy.output(type_ref::Type::builtin(type_ref::Builtin::I64));
        builder
            .item(
                item::Definition::method(
                    "CopyPair::owned_copy",
                    receiver::Mode::Owned,
                    owned_copy.finish(),
                )
                .unwrap()
                .imported(rawr_import::surface::identity::Selector::Unavailable),
            )
            .unwrap();
        builder.finish().unwrap()
    }

    #[cfg(feature = "rust-import")]
    fn imported_alias_facade(alias_first: bool) -> facade::Definition {
        let mut builder = facade::Builder::new("aliases", facade::Abi::new(1).unwrap()).unwrap();
        let mut allocation = host::TestImportAllocation::default();
        let identity = host::type_def::import::Identity::new([0x41; 32]);
        let shape: Shared<host::type_def::Shape> = host::type_def::Shape::Struct {
            form: host::type_def::Form::Record,
            constructor: Some(rawr_import::surface::identity::Selector::Unavailable),
            fields: Box::from([host::type_def::Field::new(
                "value".into(),
                type_ref::Type::builtin(type_ref::Builtin::I64),
                rawr_import::surface::identity::Selector::Unavailable,
            )]),
            constructible: true,
            unsupported: None,
        }
        .into();
        let aliases = if alias_first {
            ["Second", "First"]
        } else {
            ["First", "Second"]
        };
        for alias in aliases {
            builder
                .imported_type(
                    alias,
                    identity,
                    &[],
                    Box::default(),
                    Box::default(),
                    shape.clone(),
                    &mut allocation,
                )
                .unwrap();
            let mut signature = signature::Builder::new();
            signature.output(type_ref::Type::imported(identity, []));
            builder
                .item(
                    item::Definition::method(
                        format!("{alias}::recursive"),
                        receiver::Mode::Shared,
                        signature.finish(),
                    )
                    .unwrap()
                    .imported(rawr_import::surface::identity::Selector::Unavailable),
                )
                .unwrap();
        }
        let mut make = signature::Builder::new();
        make.output(type_ref::Type::imported(identity, []));
        builder
            .item(
                item::Definition::function("make", make.finish())
                    .unwrap()
                    .imported(rawr_import::surface::identity::Selector::Unavailable),
            )
            .unwrap();
        builder.finish().unwrap()
    }

    fn usage_builder(required: bool) -> facade::Builder {
        let mut builder = facade::Builder::new("usage", facade::Abi::new(1).unwrap()).unwrap();
        if required {
            builder
                .required_type::<RequiredValue>("Value", &[], &[])
                .unwrap();
        } else {
            builder.type_::<RequiredValue>("Value", &[], &[]).unwrap();
        }
        builder
    }

    fn value_signature() -> signature::Builder {
        let mut signature = signature::Builder::new();
        signature.output(type_ref::Type::registered("Value").unwrap());
        signature
    }

    fn execution_facade() -> facade::Definition {
        let mut builder = facade::Builder::new("effect", facade::Abi::new(1).unwrap()).unwrap();
        builder
            .required_type::<RequiredValue>("Value", &[], &[])
            .unwrap();
        builder
            .required_type::<GenericValue>("Slot", &["T"], &[])
            .unwrap();

        let mut required = signature::Builder::new();
        required.output(type_ref::Type::registered("Value").unwrap());
        builder
            .item(
                item::Definition::function("required", required.finish())
                    .unwrap()
                    .discardable(),
            )
            .unwrap();

        let mut idle = signature::Builder::new();
        idle.type_parameter("T", &[]).unwrap();
        idle.output(
            type_ref::Type::applied("Slot", [type_ref::Type::parameter("T").unwrap()]).unwrap(),
        );
        builder
            .item(
                item::Definition::function("idle", idle.finish())
                    .unwrap()
                    .discardable()
                    .elidable(),
            )
            .unwrap();

        let mut identity = signature::Builder::new();
        identity.type_parameter("T", &[]).unwrap();
        identity.parameter(type_ref::Type::parameter("T").unwrap());
        identity.output(type_ref::Type::parameter("T").unwrap());
        builder
            .item(item::Definition::function("identity", identity.finish()).unwrap())
            .unwrap();

        builder.finish().unwrap()
    }

    fn ordering_facade() -> facade::Definition {
        let mut builder = facade::Builder::new("order", facade::Abi::new(1).unwrap()).unwrap();
        builder
            .required_type::<OrderedValue>("Value", &[], &[])
            .unwrap();

        let mut make_receiver = signature::Builder::new();
        make_receiver.output(type_ref::Type::registered("Value").unwrap());
        builder
            .item(item::Definition::function("make_receiver", make_receiver.finish()).unwrap())
            .unwrap();

        let mut mark = signature::Builder::new();
        mark.parameter(type_ref::Type::builtin(type_ref::Builtin::I64));
        mark.output(type_ref::Type::builtin(type_ref::Builtin::I64));
        builder
            .item(item::Definition::function("mark", mark.finish()).unwrap())
            .unwrap();

        let mut range_width = signature::Builder::new();
        range_width.parameter(type_ref::Type::builtin(type_ref::Builtin::InclusiveRange));
        range_width.output(type_ref::Type::builtin(type_ref::Builtin::I64));
        builder
            .item(item::Definition::function("range_width", range_width.finish()).unwrap())
            .unwrap();

        let mut combine = signature::Builder::new();
        combine.parameter(type_ref::Type::builtin(type_ref::Builtin::I64));
        combine.parameter(type_ref::Type::builtin(type_ref::Builtin::I64));
        combine.output(type_ref::Type::builtin(type_ref::Builtin::I64));
        builder
            .item(
                item::Definition::method("Value::combine", receiver::Mode::Owned, combine.finish())
                    .unwrap(),
            )
            .unwrap();

        builder.finish().unwrap()
    }
}
