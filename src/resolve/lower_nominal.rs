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
mod scope;
mod sequence;
use places::PlacePattern;
mod reborrow;
mod reverse;
mod statement;
mod std_impl;
mod std_item;
mod std_method;
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
mod tests;
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
use builtin_method::default_value;
pub(crate) use builtin_method::slice_window;
use builtin_method::{
    char_method, int_method, primitive_size, slice_value, str_change, str_method, str_pieces,
    vec_method, vec_method_code, CHAR_METHODS, DEQUE_METHODS, INT_METHODS, SLICE_CHUNKS,
    STR_CHANGES, STR_PIECES,
};
pub(crate) use cell::{cell_held, cell_lock, cell_new, is_cell};
use closure::SELF_CAPTURE;
pub(crate) use compare::equal_value;
use compare::Shape;
use compare::{allowed_type, COMPARISON};
use conversion::{HostInto, PendingConversion, TextInto};
use coverage::{
    bool_literals, field_order, reject_empty_range, scalar_text, tuple_members, Scalar,
};
pub(crate) use dispatch::this_parameter;
use dispatch::{builtin_conversions_from, builtin_operator, BUILTIN_CONVERSIONS};
use dispatch::{copied_parameters, inherit_names, method_copied};
#[cfg(feature = "rust-import")]
use entry::body_source;
pub(super) use entry::lower;
pub(super) use entry::runtime_entry;
#[cfg(feature = "rust-import")]
pub(crate) use entry::with_uses;
pub(crate) use entry::Output;
use entry::{expand_imported_variants, trait_objects, use_blocks, Lowered};
use expr::ParsedExpr;
use format::PANIC_MACROS;
use from_str::{parse_text, ParseError};
use generics::{
    bound_kinds, bounded_value, dyn_arguments, generic_conversion_call, generic_equalities,
    generic_trait_arguments, generic_traits, host_witnesses, instantiate, is_iterator_trait,
    item_generics, mentions_parameter, supertraits, witness_generics, PendingInstance,
};
#[cfg(feature = "rust-import")]
use generics::{ImplBounds, ImplProjection, Selection};
use host_call::{host_number_accepts, macro_or_value, value_or_call, HostCheck};
#[cfg(feature = "rust-import")]
use host_call::{names_variant, unsupported_field, ImportedContext, UNSUPPORTED_MESSAGE_FUNCTION};
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
use op::invariant;
#[cfg(test)]
use op::matches_runtime_model;
#[cfg(feature = "rust-import")]
use op::ImportedOperation;
use op::{
    builtin_code, builtin_path_type, copied_input, copied_places, entry_key, impl_self_ty,
    move_into_call, moved_input, ordering_discriminant, owner_id, owner_key, probe_rank,
    receiver_rank, script_type, ExecutableOp, InstantiatedHostSignature, IntrinsicOperation, Keys,
    Op, PreludeOp, ProjectionOperation, WitnessedOp, FLOAT_BINARY_METHODS, FLOAT_UNARY_METHODS,
    SHAPED_HASHED, SHAPED_TOTAL,
};
use operator::{cast_value, compound_operator, kind_code, unsigned_order};
use pattern::{PatternBinding, Prelude};
pub(crate) use relocate::relocate;
use relocate::rewrite_statements;
use scope::{
    binds_by_value, ArrayElements, Binding, BindingKind, BoundIterator, Branch, Frame, FrameArray,
    LocalType, LoopElement, LoopFrame, UninitializedUse,
};
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
pub(crate) use tokens::lex_rust;
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
    /// A block whose tail is a closure, by its span: its statements, which
    /// bind what the closure captures where the host takes it (#1450).
    closure_blocks: BTreeMap<Span, origin::Fragment>,
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
            closure_blocks: BTreeMap::new(),
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
