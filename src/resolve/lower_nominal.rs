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
mod constant;
mod construct;
mod conversion;
pub(crate) mod counted;
mod coverage;
pub(crate) mod cow;

mod dispatch;
mod drops;
mod emit;
mod entry;
mod error_object;
mod expand;
mod expr;
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
mod naming;
mod op;
mod operator;
mod origin;
mod overflow;
mod path;
mod pattern;
mod places;
mod postfix;
mod proof;
mod relocate;
mod sequence;
use places::PlacePattern;
mod reborrow;
mod reverse;
mod statement;
mod std_impl;
mod std_method;
mod std_path;
mod surface;
pub(crate) mod thread;
mod time;
mod tokens;
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
use super::{intrinsic, item, type_ref, BindingSite, Callee, Error, MoveCause, Unresolved};
use crate::ast::{NominalPattern, NominalPayloadType, NOMINAL_MATCH_SENTINEL};
use crate::func::{FnCallArgs, NativeCallContext};
use crate::tokenizer::{is_valid_identifier, Token};
use crate::types::{dynamic::Union, nominal};
use crate::{
    capability, method, receiver, type_def, Dynamic, Engine, EvalAltResult, ImmutableString,
    Position, RawrResult, Scope, Shared, Span, INT,
};
use closure::SELF_CAPTURE;
use compare::Shape;
use coverage::{
    bool_literals, field_order, reject_empty_range, scalar_text, tuple_members, Scalar,
};
use dispatch::{builtin_conversions_from, builtin_operator, BUILTIN_CONVERSIONS};
pub(super) use entry::lower;
#[cfg(feature = "rust-import")]
pub(crate) use entry::with_uses;
pub(crate) use entry::Output;
use entry::{expand_imported_variants, trait_objects, use_blocks, Lowered};
#[cfg(feature = "rust-import")]
pub(super) use imported::imported;
#[cfg(all(test, feature = "rust-import"))]
use imported::imported_result;
#[cfg(feature = "rust-import")]
use imported::instance_names;
#[cfg(feature = "rust-import")]
use imported::UseArguments;
#[cfg(feature = "rust-import")]
pub(crate) use imported::{reveal, ImportedOutput};
use imported::{ImplArguments, ImplUse};
#[cfg(test)]
use inference::{inference_metrics, reset_inference_metrics};
use inference::{Checkpoint, Inference, Patch};
use keyed::keyed_query;
use location::retain_locations;
pub(crate) use location::Location;
use lvalue::{opens_compound, Lvalue, PlaceStep, Writeback};
#[cfg(feature = "rust-import")]
use models::ScriptTraitImpl;
#[cfg(test)]
use models::{model_clones, reset_model_clones};
use models::{Models, Nominal, ScriptFunction};
#[cfg(feature = "debugging")]
pub(crate) use naming::DEBUG_DISPATCHER;
#[cfg(any(test, feature = "debugging"))]
pub(crate) use naming::DEBUG_GENERIC;
use naming::{
    conversion_placeholder, function_name, instance_dispatcher_name, instance_placeholder,
    instance_text, method_name, pattern_name, static_dispatcher_name,
    static_instance_dispatcher_name, static_instance_placeholder, text_into_placeholder,
    trait_dispatcher_name, witness_placeholder, witness_variable, CLOSURE_SENTINEL,
    DEFAULT_DISPATCHER, FALLBACK_INSTANCE, METHOD_SENTINEL, PRIVATE_PREFIX, SENTINEL,
};
pub(crate) use naming::{function_display_name, private_function_name};
#[cfg(test)]
use op::builtin_type_key;
#[cfg(feature = "rust-import")]
use op::ImportedOperation;
use op::{
    builtin_code, builtin_path_type, copied_input, copied_places, entry_key, impl_self_ty,
    move_into_call, moved_input, ordering_discriminant, owner_id, owner_key, probe_rank,
    receiver_rank, script_type, ExecutableOp, InstantiatedHostSignature, IntrinsicOperation, Keys,
    Op, PreludeOp, ProjectionOperation, WitnessedOp, FLOAT_BINARY_METHODS, FLOAT_UNARY_METHODS,
    SHAPED_HASHED, SHAPED_TOTAL,
};
use pattern::{PatternBinding, Prelude};
pub(crate) use relocate::relocate;
use relocate::rewrite_statements;
use std::any::TypeId;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fmt::Write;
use std::iter::Peekable;
use std::rc::Rc;
use std::vec::IntoIter;
#[cfg(feature = "rust-import")]
use surface::{
    array_length, fix_array_length, fixed_instance_surface_type, fixed_parameter,
    fixed_surface_type, holds_value, impl_surface_type, imported_surface_type, instance_parameters,
    instance_surface_type, keyless_type_argument, length_parameter, lifetime_text, structural_use,
    unknown_const_argument, ImportedFunctionProof, ScopePaths,
};
#[cfg(feature = "rust-import")]
pub(crate) use surface::{imported_range, ImportedFunctionUse, ImportedMapperUse, ImportedUse};
use surface::{
    method_without_call, range_inclusive_element, range_type_id, unnamed_signature_type, unpassable,
};
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
/// Why a crate call can't take the program's function that builds its
/// message, as `Message::Changed` given to iced's `on_input`, here.
#[cfg(feature = "rust-import")]
const UNSUPPORTED_MESSAGE_FUNCTION: &str =
    "passing the program's function that builds its message is not supported here in Rawr yet";

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
