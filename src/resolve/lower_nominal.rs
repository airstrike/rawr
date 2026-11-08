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
use crate::types::dynamic::Union;
use crate::{
    capability, method, receiver, type_def, Dynamic, Engine, EvalAltResult, ImmutableString,
    Position, RawrResult, Scope, Shared, Span, INT,
};
pub(crate) use builtin_method::slice_window;
use builtin_method::{
    char_method, int_method, primitive_size, slice_value, str_change, str_method, str_pieces,
    vec_method, vec_method_code, CHAR_METHODS, DEQUE_METHODS, INT_METHODS, SLICE_CHUNKS,
    STR_CHANGES, STR_PIECES,
};
use closure::SELF_CAPTURE;
pub(crate) use compare::equal_value;
use compare::Shape;
use compare::{allowed_type, COMPARISON};
use coverage::{
    bool_literals, field_order, reject_empty_range, scalar_text, tuple_members, Scalar,
};
pub(crate) use dispatch::this_parameter;
use dispatch::{builtin_conversions_from, builtin_operator, BUILTIN_CONVERSIONS};
use dispatch::{copied_parameters, inherit_names, method_copied};
#[cfg(feature = "rust-import")]
use entry::body_source;
pub(super) use entry::lower;
#[cfg(feature = "rust-import")]
pub(crate) use entry::with_uses;
pub(crate) use entry::Output;
use entry::{expand_imported_variants, trait_objects, use_blocks, Lowered};
use from_str::{parse_text, ParseError};
use generics::{
    bound_kinds, bounded_value, dyn_arguments, generic_conversion_call, generic_equalities,
    generic_trait_arguments, generic_traits, host_witnesses, instantiate, is_iterator_trait,
    item_generics, mentions_parameter, supertraits, witness_generics, PendingInstance,
};
#[cfg(feature = "rust-import")]
use generics::{ImplBounds, ImplProjection, Selection};
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
use iterator::{collect_try, COLLECT_OPTION, COLLECT_RESULT};
use keyed::keyed_query;
use location::retain_locations;
pub(crate) use location::Location;
use lvalue::{
    assigned_place, field_place, fields_below_local, in_place, nested_step, variable_name,
};
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
use time::{time_code, time_method};
use tokens::{
    argument_count, assignment_token, point_span, source_full_span, source_span, span,
    split_arguments, split_first_argument, standard_function, stringify_tokens, template_tokens,
    text_end, token_text, without_borrow, without_reference, without_references, Continuation,
};
#[cfg(feature = "rust-import")]
use ty::uninstantiated_call;
use ty::{
    contains, contains_inference, contains_open_literal, contains_unsettled, exact_symbolic_type,
    is_unsettled, owned_symbolic_type, symbolic_input_type,
};
#[cfg(feature = "rust-import")]
use ty::{first_inference, mentions_script, reaches_param};
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
pub(crate) use cell::{cell_held, cell_lock, cell_new, is_cell};
pub(super) use entry::runtime_entry;
use host_call::{host_number_accepts, macro_or_value, value_or_call, HostCheck};
#[cfg(feature = "rust-import")]
use host_call::{names_variant, unsupported_field, ImportedContext, UNSUPPORTED_MESSAGE_FUNCTION};
use op::invariant;
#[cfg(test)]
use op::matches_runtime_model;
pub(crate) use tokens::lex_rust;

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
