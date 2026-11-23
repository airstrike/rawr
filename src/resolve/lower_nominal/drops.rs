//! Rust's drop elaboration for a crate with `impl Drop`.
//!
//! A value of a type with `impl Drop`, or one that holds such a value, has
//! drop glue. The lowering writes a call of the glue where rustc drops the
//! value: at the end of its local's scope (block, arm, iteration or body,
//! the locals in reverse order of declaration), at a `return`, `break`,
//! `continue` or `?` for each scope it leaves, after an expression
//! statement or `let _ = value;`, and before an assignment stores over a
//! value. The glue runs the type's own `drop`, then drops its fields,
//! payload or elements in order (`write_glue`, `parts`).
//!
//! A move out of a local leaves `()` in it (`__rawr_move`, and
//! `__rawr_clear` for a part), which the glue passes over: Rust's drop flag,
//! checked at runtime. A crate without `impl Drop` lowers as before.

use super::*;
use crate::resolve::borrowck::{Path, Step};

/// The generated function that drops a value: its own `drop`, then its
/// parts.
pub(crate) const GLUE: &str = "__rawr_drop_glue";

/// A local whose value may drop at the end of its scope.
#[derive(Clone)]
pub(super) struct Local {
    /// The name the lowered code reads the value by: the local's own, or
    /// a hidden copy of it while a later binding shadows the name.
    place: ImmutableString,
    name: ImmutableString,
    declaration: Span,
    ty: Ty,
}

/// What holds a matched value, which a pattern's moves leave `()` in.
pub(super) enum Holder {
    /// The crate has no `impl Drop`.
    None,
    /// A local, or a part of it at the indices.
    Local(String, Vec<usize>),
    /// A place a move cannot leave `()` in, as an element.
    Place,
}

/// Where a pattern's moves leave `()` in what it was matched on.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Clears {
    /// In an arm, where the pattern's bindings are in scope: a binding
    /// that shadows the local holding the value defers the move to after
    /// the match (`deferred_clears`).
    InArm,
    /// Before the pattern's bindings are declared.
    BeforeBindings,
}

/// Where the temporaries a `let` condition's subject makes drop.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Subject {
    /// With the condition's scope: at the end of the consequent or of the
    /// iteration, and without a match before the alternative, as in Rust
    /// 2024's `if let` and in `while let`. The caller declares them.
    Scoped,
    /// With the enclosing statement's (a let chain's link).
    Statement,
}

/// A `let` condition's drops beyond its scope's.
pub(super) struct Condition {
    /// Without a match: the drop of the value made for the condition, and
    /// of the temporaries its subject made.
    pub(super) otherwise: String,
    /// The temporaries the subject made, which the caller declares before
    /// the subject: its condition's scope drops them.
    pub(super) temporaries: Vec<String>,
    /// Copies of the outer locals the pattern's bindings shadow, made
    /// before it binds them (`shadow_copies`).
    pub(super) copies: String,
    /// What runs after the match: a move out of a local the pattern's
    /// bindings shadow.
    pub(super) after: String,
}

/// The locals one block, arm, body or iteration binds, in order.
#[derive(Default)]
pub(super) struct Scope {
    locals: Vec<Local>,
    /// How many of `locals`, the first, are a body's parameters, which end
    /// after its tail's temporaries.
    parameters: usize,
    /// Outer locals this scope's bindings shadow, by scope and position,
    /// whose own names read them again once it ends.
    shadowed: Vec<(usize, usize)>,
    /// The borrow facts have the uses its drops make at its end already
    /// ([`Parser::block_drop_uses`]).
    uses_recorded: bool,
    /// What ends the thread's thread-local values after the scope's own
    /// drops: `main`'s body's (`end_main_thread`).
    thread_end: Option<String>,
}

/// One temporary scope (a statement, a match arm, a condition): the hidden
/// locals holding the values made in it that nothing moves, in the order
/// made.
pub(super) struct Temporaries {
    /// How many drop scopes were open when it opened: it is inside the
    /// last of them, so its temporaries drop before that scope's locals.
    depth: usize,
    pub(super) made: Vec<String>,
    /// A statement's, before edition 2024: as a block's tail it keeps its
    /// temporaries past the block's locals, which is known only at its end.
    tail: bool,
    /// The drops jumps out of it wrote before that is known.
    exits: Vec<Exit>,
}

/// A jump's drops of a statement's temporaries before edition 2024: two
/// places in the jump's text, before the locals of the statement's block
/// and after them, and the drops, which go in the first unless the
/// statement is the block's tail ([`Parser::hand_tail`]).
pub(super) struct Exit {
    early: String,
    late: String,
    drops: String,
}

impl Exit {
    /// Each of the two places, as the text that marks it.
    fn marks(&self) -> (String, String) {
        (format!("/*{}*/", self.early), format!("/*{}*/", self.late))
    }
}

/// Where a block's tail temporaries drop before edition 2024: after the
/// block's locals, at the end of the temporary scope around the block, or,
/// in a body's own tail, between its locals and its parameters. Edition
/// 2024 drops them with the tail, before the locals.
#[derive(Default)]
pub(super) struct Tails {
    /// The block or body being read keeps its tail's temporaries past its
    /// locals.
    kept: bool,
    /// The temporaries its tail handed it, in the order made.
    handed: Vec<String>,
    /// The block ahead is a block expression, whose tail's temporaries go
    /// on to the temporary scope around it. A body's block, a loop's, a
    /// closure's or a branch's is a temporary scope of its own.
    escapes: bool,
}

/// Which of the borrows a value holds its drop uses, as rustc's drop check
/// has it: a type's own `Drop` may use all of them, and a reference or
/// `std`'s containers (whose `Drop` is `#[may_dangle]`) use none of their
/// own.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum DropUse {
    None,
    All,
    /// Some and not others, which these facts do not tell apart.
    Some,
}

/// The parts of `value` the glue goes on to after the value's own `drop`:
/// a struct's fields, an enum variant's payload, an array's (`Vec`, array,
/// tuple) elements, or what an `Option` or `Result` holds, in that order,
/// each that holds a value of a type with `impl Drop`, as `[part, shape]`
/// pairs. `shape` is the value's [`Shape`], as `Shape::text` writes it:
/// a part its type says is a reference is passed over, since what it
/// refers to is not the value's to drop.
pub(super) fn parts(value: &Dynamic, shape: &Dynamic, models: &Models) -> Dynamic {
    let mut parts = crate::Array::new();
    each_part(value, shape, models, &mut |part, shape| {
        if !matches!(shape.0, Union::Bool(false, ..)) && holds(part, models) {
            parts.push(vec![part.clone(), shape].into());
        }
    });
    parts.into()
}

/// Each part of `value` with its shape, from `shape`, the value's. Under
/// `()`, the walk of every part, a crate type's field or payload its
/// declaration types as a reference is passed over: only a generic
/// argument that is a reference is still walked.
fn each_part(
    value: &Dynamic,
    shape: &Dynamic,
    models: &Models,
    each: &mut dyn FnMut(&Dynamic, Dynamic),
) {
    let declared = |identity: &type_def::Identity, variant: Option<&str>| {
        let types: Vec<&type_ref::Type> = match models.by_identity.get(identity) {
            Some(Nominal::Struct { item, .. }) => {
                item.fields.iter().map(|field| &field.ty).collect()
            }
            Some(Nominal::Enum { item, .. }) => item
                .variants
                .iter()
                .filter(|candidate| Some(candidate.name.text.as_str()) == variant)
                .flat_map(|candidate| candidate.payload.iter())
                .collect(),
            None => Vec::new(),
        };
        let shapes: crate::Array = types
            .into_iter()
            .map(|ty| match ty {
                type_ref::Type::Reference { .. } => false.into(),
                _ => Dynamic::UNIT,
            })
            .collect();
        Dynamic::from(shapes)
    };
    let at = |shape: &Dynamic, index: usize| match &shape.0 {
        Union::Array(shapes, ..) => shapes.get(index).cloned().unwrap_or(Dynamic::UNIT),
        Union::Map(shapes, ..) => shapes.get("e").cloned().unwrap_or(Dynamic::UNIT),
        _ => Dynamic::UNIT,
    };
    let key = |shape: &Dynamic, key: &str| match &shape.0 {
        Union::Map(shapes, ..) => shapes.get(key).cloned().unwrap_or(Dynamic::UNIT),
        _ => Dynamic::UNIT,
    };
    let walk = shape.is_unit();
    match &value.0 {
        Union::Struct(value, ..) => {
            let shape = if walk {
                &declared(value.identity(), None)
            } else {
                shape
            };
            for (index, field) in value.fields().iter().enumerate() {
                each(field, at(shape, index));
            }
        }
        Union::Enum(value, ..) => {
            let payload = if walk {
                declared(value.identity(), Some(value.variant().name()))
            } else {
                key(shape, value.variant().name())
            };
            for (index, part) in value.payload().iter().enumerate() {
                each(part, at(&payload, index));
            }
        }
        Union::Array(value, ..) => {
            for (index, element) in value.iter().enumerate() {
                each(element, at(shape, index));
            }
        }
        Union::Shared(..) => each_part(&value.flatten_clone(), shape, models, each),
        Union::Variant(..) => {
            if let Some(inner) = cell::owned_value(value) {
                each(&inner, shape.clone());
            } else if let Some(Some(inner)) = crate::option::read(value) {
                each(&inner, shape.clone());
            } else if let Some(Ok(inner)) = crate::result::read(value) {
                each(&inner, key(shape, "ok"));
            } else if let Some(Err(inner)) = crate::result::read(value) {
                each(&inner, key(shape, "err"));
            }
        }
        _ => {}
    }
}

/// How the glue walks a value of a type: rustc's drop glue follows the
/// type, and a reference's referent is not dropped with what holds the
/// reference, though at run time a reference reads the referent's value.
/// Only a type holding a reference to a value with drop glue needs more
/// than [`Shape::Walk`].
enum Shape {
    /// Every part the glue finds at run time.
    Walk,
    /// Nothing: a reference.
    Skip,
    /// A struct's fields or a tuple's members, by position.
    Parts(Vec<Shape>),
    /// Each element alike.
    Elements(Box<Shape>),
    /// Each variant's payload, by the variant's name.
    Variants(Vec<(ImmutableString, Vec<Shape>)>),
    /// `Ok`'s and `Err`'s.
    Result(Box<Shape>, Box<Shape>),
}

impl Shape {
    fn walks(&self) -> bool {
        matches!(self, Self::Walk)
    }

    fn parts(parts: Vec<Self>) -> Self {
        if parts.iter().all(Self::walks) {
            Self::Walk
        } else {
            Self::Parts(parts)
        }
    }

    /// The shape as the glue reads it: `()`, `false`, an array by
    /// position, or a map by element (`e`), variant, or `ok` and `err`.
    fn text(&self) -> String {
        let list = |parts: &[Self]| {
            let parts: Vec<String> = parts.iter().map(Self::text).collect();
            format!("[{}]", parts.join(", "))
        };
        match self {
            Self::Walk => "()".into(),
            Self::Skip => "false".into(),
            Self::Parts(parts) => list(parts),
            Self::Elements(element) => format!("#{{\"e\": {}}}", element.text()),
            Self::Variants(variants) => {
                let variants: Vec<String> = variants
                    .iter()
                    .map(|(name, payload)| format!("\"{name}\": {}", list(payload)))
                    .collect();
                format!("#{{{}}}", variants.join(", "))
            }
            Self::Result(ok, err) => {
                format!("#{{\"ok\": {}, \"err\": {}}}", ok.text(), err.text())
            }
        }
    }
}

/// Whether `value` is or holds a value of a type with `impl Drop`.
fn holds(value: &Dynamic, models: &Models) -> bool {
    let own = match &value.0 {
        Union::Struct(value, ..) => Some(value.identity()),
        Union::Enum(value, ..) => Some(value.identity()),
        Union::Array(..) | Union::Shared(..) | Union::Variant(..) => None,
        _ => return false,
    };
    if own
        .and_then(|identity| models.by_identity.get(identity))
        .is_some_and(|nominal| models.has_drop(nominal.definition().id()))
    {
        return true;
    }
    let mut found = false;
    each_part(value, &Dynamic::UNIT, models, &mut |part, shape| {
        found = found || !matches!(shape.0, Union::Bool(false, ..)) && holds(part, models);
    });
    found
}

/// The glue: `__rawr_drop_glue(value, shape)` runs the `drop` of the
/// value's type, when it has one, then the glue of each part, as rustc's
/// drop glue does. `shape` is the value's [`Shape`].
pub(super) fn write_glue(
    output: &mut location::Generated,
    models: &Models,
    ops: &mut Vec<ExecutableOp>,
    source: crate::program::source::Id,
    declaration: Span,
    type_index: &str,
) {
    let parts = format!("{SENTINEL}{}", ops.len());
    ops.push(ExecutableOp::Prelude(PreludeOp::DropParts));
    let mut text = format!(
        "__rawr_private fn {GLUE}(__rawr_value, __rawr_shape) {{\nlet __rawr_type = \
         {type_index}(__rawr_value);\n"
    );
    for ((owner, _), definitions) in models
        .trait_methods
        .iter()
        .filter(|((_, name), _)| name.as_str() == "drop")
    {
        for definition in definitions
            .iter()
            .filter(|definition| definition.trait_name().is_some_and(|name| name == "Drop"))
        {
            let _ = writeln!(
                text,
                "if __rawr_type == {} {{ __rawr_value.{}(); }}",
                owner_key(*owner),
                method_name(definition)
            );
        }
    }
    let _ = write!(
        text,
        "for __rawr_part in {parts}(__rawr_value, __rawr_shape) {{ \
         {GLUE}(__rawr_part[0], __rawr_part[1]); }}\n}}\n"
    );
    output.declaration(source, declaration, &text);
}

/// `__rawr_clear(place, ..)` of each moved path below `prefix`.
fn clear_text(place: &str, prefix: &[usize], moves: &[Vec<usize>]) -> String {
    let mut text = String::new();
    for path in moves {
        let _ = write!(text, "{}({place}", crate::engine::KEYWORD_CLEAR);
        for index in prefix.iter().chain(path.iter()) {
            let _ = write!(text, ", {index}");
        }
        text.push_str("); ");
    }
    text
}

/// The names a pattern binds.
pub(super) fn pattern_names(syntax: &pattern::Syntax) -> Vec<ImmutableString> {
    let mut names = Vec::new();
    collect_names(syntax, &mut names);
    names.into_iter().map(|(name, _)| name).collect()
}

/// The names a pattern binds, in written order, with their declarations.
fn collect_names(syntax: &pattern::Syntax, names: &mut Vec<(ImmutableString, Span)>) {
    match syntax {
        pattern::Syntax::Binding { name, span, .. } | pattern::Syntax::Ref { name, span, .. } => {
            names.push((name.clone(), *span));
        }
        pattern::Syntax::At {
            name,
            name_span,
            pattern,
        } => {
            names.push((name.clone(), *name_span));
            collect_names(pattern, names);
        }
        pattern::Syntax::Rest {
            binding: Some((name, span)),
            ..
        } => names.push((name.clone(), *span)),
        pattern::Syntax::TupleStruct { members, .. } | pattern::Syntax::Tuple { members, .. } => {
            members
                .iter()
                .for_each(|member| collect_names(member, names));
        }
        pattern::Syntax::Enum(pattern) => pattern
            .payload
            .iter()
            .for_each(|member| collect_names(member, names)),
        pattern::Syntax::Prelude {
            payload: Some(payload),
            ..
        }
        | pattern::Syntax::Cow { payload, .. } => collect_names(payload, names),
        pattern::Syntax::Reference { pattern, .. } => collect_names(pattern, names),
        pattern::Syntax::Or { alternatives, .. } => {
            if let Some(first) = alternatives.first() {
                collect_names(first, names);
            }
        }
        pattern::Syntax::Slice {
            before,
            rest,
            after,
            ..
        } => {
            before
                .iter()
                .for_each(|member| collect_names(member, names));
            if let Some(rest) = rest {
                collect_names(rest, names);
            }
            after.iter().for_each(|member| collect_names(member, names));
        }
        _ => {}
    }
}

/// The statements that declare `temporaries` before what assigns them.
pub(super) fn declarations(temporaries: &[String]) -> String {
    temporaries
        .iter()
        .map(|temporary| format!("let {temporary} = (); "))
        .collect()
}

/// A drop of the value `place` reads, whose type's [`Shape`] is `shape`
/// ([`Parser::drop_shape`]): the glue's call, as a statement.
pub(super) fn glue_call(place: &str, shape: &str) -> String {
    format!("{GLUE}({place}, {shape});")
}

/// What the glue reads for a type whose drop glue walks every part.
pub(super) const WALK: &str = "()";

impl Models {
    /// Whether `owner` has `impl Drop`.
    pub(super) fn has_drop(&self, owner: type_def::Id) -> bool {
        self.impls.contains(&("Drop".into(), owner))
    }
}

impl Parser<'_> {
    /// Whether the crate has `impl Drop`: only then does the lowering
    /// write drops.
    pub(super) fn drops(&self) -> bool {
        self.models.std_traits.contains("Drop")
    }

    /// Whether the lowering keeps its locals' drop scopes: for the glue of
    /// the crate's `impl Drop`, or for the borrow checker's drop uses of an
    /// imported `impl Trait` value, which may run code of its own when it
    /// drops (`drop_use`).
    pub(super) fn drop_scoped(&self) -> bool {
        cfg!(feature = "rust-import") || self.drops()
    }

    /// Whether a value of `ty` may have drop glue: a type with `impl Drop`,
    /// or one holding such a type in a field, payload or element. A generic
    /// parameter, a trait object or a type still inferred may, and its glue
    /// checks the runtime value. A holder Rawr cannot drop exactly is an
    /// error at `at`.
    pub(super) fn needs_drop(&self, ty: &Ty, at: Span) -> Result<bool, Error> {
        if !self.drops() {
            return Ok(false);
        }
        self.needs_drop_in(ty, at, &mut Vec::new())
    }

    /// Whether a value of `ty` has drop glue in Rust, as std's
    /// `needs_drop` says, whether or not a drop of it does what the program
    /// sees: a `Vec` or a `String` has, a `Cell<i32>` or a `&T` hasn't. A
    /// type rawr can't see through (a generic, a trait object, a library
    /// type, an iterator) is taken to have it.
    pub(super) fn std_drop_glue(&self, ty: &Ty) -> bool {
        self.std_drop_glue_in(ty, &mut Vec::new())
    }

    fn std_drop_glue_in(&self, ty: &Ty, visiting: &mut Vec<(type_def::Id, Box<[Ty]>)>) -> bool {
        let resolved = self.inference.resolve(ty);
        match &resolved {
            Ty::Script(id, arguments) => {
                if self.models.has_drop(*id) {
                    return true;
                }
                let key = (*id, arguments.clone());
                if visiting.contains(&key) {
                    return false;
                }
                visiting.push(key);
                let substitution = self.nominal_substitution(*id, arguments);
                let fields: Vec<Ty> = match self.models.definition(*id) {
                    Nominal::Struct { item, .. } => item
                        .fields
                        .iter()
                        .map(|field| instantiate(&Ty::from_resolved(&field.ty), &substitution))
                        .collect(),
                    Nominal::Enum { item, .. } => item
                        .variants
                        .iter()
                        .flat_map(|variant| variant.payload.iter())
                        .map(|ty| instantiate(&Ty::from_resolved(ty), &substitution))
                        .collect(),
                };
                let glue = fields
                    .iter()
                    .any(|field| self.std_drop_glue_in(field, visiting));
                visiting.pop();
                glue
            }
            // A fixed array holds its elements in place; a `Vec`, a
            // `VecDeque` and a boxed slice own their buffer.
            Ty::Array {
                element,
                kind: sequence::Kind::Array(_),
            } => self.std_drop_glue_in(element, visiting),
            Ty::Option(inner)
            | Ty::Range(inner)
            | Ty::OpenRange(_, inner)
            | Ty::InclusiveOf(inner)
            | Ty::Reverse(inner) => self.std_drop_glue_in(inner, visiting),
            Ty::Cell(CellKind::Cell | CellKind::RefCell, inner) => {
                self.std_drop_glue_in(inner, visiting)
            }
            Ty::Result(ok, err) => {
                self.std_drop_glue_in(ok, visiting) || self.std_drop_glue_in(err, visiting)
            }
            Ty::Tuple(members) => members
                .iter()
                .any(|member| self.std_drop_glue_in(member, visiting)),
            Ty::Never
            | Ty::Unit
            | Ty::Bool
            | Ty::Char
            | Ty::Int(_)
            | Ty::Float(_)
            | Ty::Str
            | Ty::InclusiveRange(_)
            | Ty::FloatRange
            | Ty::Ordering
            | Ty::Time(_)
            | Ty::ParseError(_)
            | Ty::Arguments
            | Ty::Reference { .. }
            | Ty::Fn { .. }
            | Ty::FnItem { .. }
            | Ty::FnChoice { .. }
            | Ty::Const(_) => false,
            _ => true,
        }
    }

    /// How the glue walks a value of `ty` ([`Shape`]), as its second
    /// argument.
    pub(super) fn drop_shape(&self, ty: &Ty) -> String {
        self.shape_in(ty, &mut Vec::new()).text()
    }

    fn shape_in(&self, ty: &Ty, visiting: &mut Vec<(type_def::Id, Box<[Ty]>)>) -> Shape {
        let resolved = self.inference.resolve(ty);
        match &resolved {
            // What a reference refers to is not dropped with it.
            Ty::Reference { target, .. } => {
                if self
                    .needs_drop_in(target, Span::NONE, &mut Vec::new())
                    .unwrap_or(true)
                {
                    Shape::Skip
                } else {
                    Shape::Walk
                }
            }
            Ty::Script(id, arguments) => {
                let key = (*id, arguments.clone());
                if visiting.contains(&key) {
                    return Shape::Walk;
                }
                visiting.push(key);
                let substitution = self.nominal_substitution(*id, arguments);
                let mut shape = |ty: &type_ref::Type| {
                    self.shape_in(
                        &instantiate(&Ty::from_resolved(ty), &substitution),
                        visiting,
                    )
                };
                let shape = match self.models.definition(*id) {
                    Nominal::Struct { item, .. } => {
                        Shape::parts(item.fields.iter().map(|field| shape(&field.ty)).collect())
                    }
                    Nominal::Enum { item, .. } => {
                        let variants: Vec<_> = item
                            .variants
                            .iter()
                            .map(|variant| {
                                (
                                    variant.name.text.clone(),
                                    variant.payload.iter().map(&mut shape).collect::<Vec<_>>(),
                                )
                            })
                            .collect();
                        if variants
                            .iter()
                            .all(|(_, payload)| payload.iter().all(Shape::walks))
                        {
                            Shape::Walk
                        } else {
                            Shape::Variants(variants)
                        }
                    }
                };
                visiting.pop();
                shape
            }
            Ty::Array { element, .. } => match self.shape_in(element, visiting) {
                Shape::Walk => Shape::Walk,
                element => Shape::Elements(Box::new(element)),
            },
            // At run time, a `Box` is the value it holds, and the glue walks
            // a cell's value as its part.
            Ty::Option(inner) | Ty::Boxed(inner) | Ty::Cell(_, inner) => {
                self.shape_in(inner, visiting)
            }
            Ty::Result(ok, err) => {
                let (ok, err) = (self.shape_in(ok, visiting), self.shape_in(err, visiting));
                if ok.walks() && err.walks() {
                    Shape::Walk
                } else {
                    Shape::Result(Box::new(ok), Box::new(err))
                }
            }
            Ty::Tuple(members) => Shape::parts(
                members
                    .iter()
                    .map(|member| self.shape_in(member, visiting))
                    .collect(),
            ),
            _ => Shape::Walk,
        }
    }

    /// Which of the borrows a value of `ty` holds its drop uses. Without an
    /// `impl Drop` of the program's own, only an imported `impl Trait`
    /// value's drop does, as rustc assumes of every opaque type.
    pub(super) fn drop_use(&self, ty: &Ty) -> DropUse {
        match self.drop_uses_in(ty, &mut Vec::new()) {
            (false, _) => DropUse::None,
            (true, false) => DropUse::All,
            (true, true) => DropUse::Some,
        }
    }

    /// Whether the drop of a value of `ty` uses some borrow the value
    /// holds, and whether it leaves some other unused.
    fn drop_uses_in(&self, ty: &Ty, visiting: &mut Vec<(type_def::Id, Box<[Ty]>)>) -> (bool, bool) {
        if !self.may_hold_borrow(ty) {
            return (false, false);
        }
        let any = |parts: &mut dyn Iterator<Item = (bool, bool)>| {
            parts.fold((false, false), |(used, unused), (a, b)| {
                (used || a, unused || b)
            })
        };
        match &self.inference.resolve(ty) {
            Ty::Reference { .. } => (false, true),
            // An imported `impl Trait` may have a `Drop` of its own.
            #[cfg(feature = "rust-import")]
            Ty::Host { proof, .. }
                if rawr_import::surface::opaque::declared(&proof.definition.path()) =>
            {
                (true, false)
            }
            Ty::Script(id, arguments) => {
                if self.models.has_drop(*id) {
                    return (true, false);
                }
                let key = (*id, arguments.clone());
                if visiting.contains(&key) {
                    return (false, false);
                }
                visiting.push(key);
                let substitution = self.nominal_substitution(*id, arguments);
                let fields: Vec<Ty> = match self.models.definition(*id) {
                    Nominal::Struct { item, .. } => item
                        .fields
                        .iter()
                        .map(|field| instantiate(&Ty::from_resolved(&field.ty), &substitution))
                        .collect(),
                    Nominal::Enum { item, .. } => item
                        .variants
                        .iter()
                        .flat_map(|variant| variant.payload.iter())
                        .map(|ty| instantiate(&Ty::from_resolved(ty), &substitution))
                        .collect(),
                };
                let uses = any(&mut fields
                    .iter()
                    .map(|field| self.drop_uses_in(field, visiting)));
                visiting.pop();
                uses
            }
            Ty::Array { element, .. }
            | Ty::Option(element)
            | Ty::Boxed(element)
            | Ty::Counted(_, element)
            | Ty::Weak(_, element)
            | Ty::Cell(_, element)
            | Ty::Reverse(element) => self.drop_uses_in(element, visiting),
            Ty::Result(ok, err) => any(&mut [ok, err]
                .into_iter()
                .map(|ty| self.drop_uses_in(ty, visiting))),
            Ty::Tuple(members) | Ty::Keyed(_, members) => any(&mut members
                .iter()
                .map(|member| self.drop_uses_in(member, visiting))),
            // A generic or a trait object may have a `Drop` of its own.
            Ty::Param(..) | Ty::Projection(..) | Ty::Dyn(..) | Ty::Infer(_) | Ty::Unknown
                if self.drops() =>
            {
                (true, false)
            }
            _ => (false, true),
        }
    }

    /// The local's drop here uses the borrows it holds, as rustc's drop
    /// liveness has it.
    fn note_drop_use(&self, local: &Local) {
        match self.drop_use(&local.ty) {
            DropUse::None => {}
            DropUse::All => self.flow().drop_use(local.declaration),
            DropUse::Some => {
                if self.drops_in_part(&local.ty, local.declaration) {
                    self.flow().drop_use(local.declaration);
                }
            }
        }
    }

    /// Whether the drop of the local of type `ty` declared at
    /// `declaration`, which uses only some of the borrows it holds, uses a
    /// part of it these facts follow (`Recorder::drops_in_part`). One whose
    /// part they can't tell is noted.
    pub(super) fn drops_in_part(&self, ty: &Ty, declaration: Span) -> bool {
        match self.drop_positions(ty, &mut Vec::new()) {
            Some(used) => {
                self.flow().drops_in_part(declaration, &used);
                true
            }
            None => {
                self.flow().unmodeled(
                    "a value whose drop uses some of the borrows it holds and not others",
                    declaration,
                );
                false
            }
        }
    }

    /// The region positions of a value of `ty` whose borrows its drop
    /// uses, as rustc's drop check has it, or `None` where these facts
    /// can't tell: all of a type with a `Drop` of its own, none of a
    /// reference, and of any other type what its parts' drops use, as
    /// `drop_uses_in` reads them.
    fn drop_positions(
        &self,
        ty: &Ty,
        visiting: &mut Vec<(type_def::Id, Box<[Ty]>)>,
    ) -> Option<Vec<Path>> {
        if !self.may_hold_borrow(ty) {
            return Some(Vec::new());
        }
        let mut below = |members: &mut dyn Iterator<Item = &Ty>| {
            let mut used = Vec::new();
            for (index, member) in members.enumerate() {
                let at = Path::of(&[Step::Arg(index as u8)]);
                used.extend(
                    self.drop_positions(member, visiting)?
                        .into_iter()
                        .map(|path| at.join(path)),
                );
            }
            Some(used)
        };
        match &self.inference.resolve(ty) {
            Ty::Reference { .. } => Some(Vec::new()),
            #[cfg(feature = "rust-import")]
            Ty::Host { proof, .. }
                if rawr_import::surface::opaque::declared(&proof.definition.path()) =>
            {
                Some(vec![Path::ROOT])
            }
            Ty::Script(id, arguments) => {
                if self.models.has_drop(*id) {
                    return Some(vec![Path::ROOT]);
                }
                let key = (*id, arguments.clone());
                if visiting.contains(&key) {
                    return None;
                }
                visiting.push(key);
                let used = self.field_drop_positions(*id, arguments, visiting);
                visiting.pop();
                used
            }
            Ty::Array { element, .. }
            | Ty::Option(element)
            | Ty::Boxed(element)
            | Ty::Counted(_, element)
            | Ty::Weak(_, element)
            | Ty::Cell(_, element)
            | Ty::Reverse(element) => below(&mut std::iter::once(&**element)),
            Ty::Result(ok, err) => below(&mut [&**ok, &**err].into_iter()),
            Ty::Tuple(members) | Ty::Keyed(_, members) => below(&mut members.iter()),
            Ty::Param(..) | Ty::Projection(..) | Ty::Dyn(..) | Ty::Infer(_) | Ty::Unknown
                if self.drops() =>
            {
                Some(vec![Path::ROOT])
            }
            _ => Some(Vec::new()),
        }
    }

    /// The region positions of a value of the crate type `id`, with no
    /// `Drop` of its own, whose borrows the drops of its fields or payloads
    /// use: each field's, at the type's positions its own sit at.
    fn field_drop_positions(
        &self,
        id: type_def::Id,
        arguments: &[Ty],
        visiting: &mut Vec<(type_def::Id, Box<[Ty]>)>,
    ) -> Option<Vec<Path>> {
        let substitution = self.nominal_substitution(id, arguments);
        let fields: Vec<(Option<&str>, usize, &type_ref::Type)> = match self.models.definition(id) {
            Nominal::Struct { item, .. } => item
                .fields
                .iter()
                .enumerate()
                .map(|(slot, field)| (None, slot, &field.ty))
                .collect(),
            Nominal::Enum { item, .. } => item
                .variants
                .iter()
                .flat_map(|variant| {
                    variant
                        .payload
                        .iter()
                        .enumerate()
                        .map(|(slot, ty)| (Some(variant.name.text.as_str()), slot, ty))
                })
                .collect(),
        };
        let mut used = Vec::new();
        for (variant, slot, declared) in fields {
            let field = instantiate(&Ty::from_resolved(declared), &substitution);
            let dropped = self.drop_positions(&field, visiting)?;
            if dropped.is_empty() {
                continue;
            }
            for (at, position) in self.field_positions(id, variant, slot)? {
                // A `'static` position holds nothing the drop could outlive.
                let Some(position) = position else {
                    continue;
                };
                if dropped.iter().any(|whole| at.starts_with(*whole)) {
                    used.push(position);
                    continue;
                }
                // A type parameter's position: what its argument's drop uses.
                used.extend(
                    dropped
                        .iter()
                        .filter(|part| part.starts_with(at))
                        .map(|part| position.join(part.strip(at))),
                );
            }
        }
        Some(used)
    }

    /// The locals of the innermost scope whose drops use the borrows they
    /// hold, by declaration, for the block's end to record with its
    /// locals' ends (`Recorder::storage_dead`). A local whose drop uses
    /// only some is noted here.
    pub(super) fn block_drop_uses(&mut self) -> Vec<Span> {
        let mut uses = Vec::new();
        if !self.drop_scoped() {
            return uses;
        }
        let Some(scope) = self.drop_scopes.last() else {
            return uses;
        };
        for local in &scope.locals {
            match self.drop_use(&local.ty) {
                DropUse::None => {}
                DropUse::All => uses.push(local.declaration),
                DropUse::Some => {
                    if self.drops_in_part(&local.ty, local.declaration) {
                        uses.push(local.declaration);
                    }
                }
            }
        }
        if let Some(scope) = self.drop_scopes.last_mut() {
            scope.uses_recorded = true;
        }
        uses
    }

    /// A jump at `at` out of the scopes above `depth`: their locals end
    /// there, the last declared first, each after the uses its drop makes,
    /// as at the scopes' ends. A cell's guard among them releases its
    /// borrow there.
    pub(super) fn exit_drop_uses(&self, depth: usize, at: Span) {
        if !self.drop_scoped() {
            return;
        }
        let leaving = self
            .drop_scopes
            .iter()
            .skip(depth)
            .flat_map(|scope| &scope.locals);
        let declared: Vec<Span> = leaving.clone().map(|local| local.declaration).collect();
        let mut uses = Vec::new();
        for local in leaving {
            match self.drop_use(&local.ty) {
                DropUse::None => {}
                DropUse::All => uses.push(local.declaration),
                DropUse::Some => {
                    if self.drops_in_part(&local.ty, local.declaration) {
                        uses.push(local.declaration);
                    }
                }
            }
        }
        uses.extend(self.cell_guard_drops(&declared));
        self.flow().scope_exit(&declared, &uses, at);
    }

    fn needs_drop_in(
        &self,
        ty: &Ty,
        at: Span,
        visiting: &mut Vec<(type_def::Id, Box<[Ty]>)>,
    ) -> Result<bool, Error> {
        let unsupported = |holder: &str| Error::UnsupportedNominalSyntax {
            construct: crate::resolve::Unsupported::said(format!(
                "a value whose type implements `Drop` inside {holder}: dropping it where Rust \
                 does is not supported in Rawr yet"
            )),
            source: at,
        };
        let resolved = self.inference.resolve(ty);
        Ok(match &resolved {
            Ty::Script(id, arguments) => {
                if self.models.has_drop(*id) {
                    return Ok(true);
                }
                let key = (*id, arguments.clone());
                if visiting.contains(&key) {
                    return Ok(false);
                }
                visiting.push(key);
                let substitution = self.nominal_substitution(*id, arguments);
                let fields: Vec<Ty> = match self.models.definition(*id) {
                    Nominal::Struct { item, .. } => item
                        .fields
                        .iter()
                        .map(|field| instantiate(&Ty::from_resolved(&field.ty), &substitution))
                        .collect(),
                    Nominal::Enum { item, .. } => item
                        .variants
                        .iter()
                        .flat_map(|variant| variant.payload.iter())
                        .map(|ty| instantiate(&Ty::from_resolved(ty), &substitution))
                        .collect(),
                };
                let mut needs = false;
                for field in &fields {
                    if self.needs_drop_in(field, at, visiting)? {
                        needs = true;
                        break;
                    }
                }
                visiting.pop();
                needs
            }
            Ty::Array { element, .. } | Ty::Option(element) | Ty::Boxed(element) => {
                self.needs_drop_in(element, at, visiting)?
            }
            Ty::Result(ok, err) => {
                self.needs_drop_in(ok, at, visiting)? || self.needs_drop_in(err, at, visiting)?
            }
            Ty::Tuple(members) => {
                let mut needs = false;
                for member in members.iter() {
                    needs |= self.needs_drop_in(member, at, visiting)?;
                }
                needs
            }
            Ty::Counted(counter, inner) | Ty::Weak(counter, inner) => {
                if self.needs_drop_in(inner, at, visiting)? {
                    return Err(unsupported(&format!("an `{}`", counter.name())));
                }
                false
            }

            // A `RefCell`, `Cell` or `Mutex` drops its value with it.
            Ty::Cell(_, inner) => self.needs_drop_in(inner, at, visiting)?,
            Ty::Keyed(kind, arguments) => {
                for argument in arguments.iter() {
                    if self.needs_drop_in(argument, at, visiting)? {
                        let name = match kind {
                            Keyed::HashMap => "a `HashMap`",
                            Keyed::BTreeMap => "a `BTreeMap`",
                            Keyed::HashSet => "a `HashSet`",
                            Keyed::BTreeSet => "a `BTreeSet`",
                        };
                        return Err(unsupported(name));
                    }
                }
                false
            }
            Ty::Reverse(inner) => {
                if self.needs_drop_in(inner, at, visiting)? {
                    return Err(unsupported("a `Reverse`"));
                }
                false
            }
            Ty::Iter(item, _, _) => {
                if self.needs_drop_in(item, at, visiting)? {
                    return Err(unsupported("an iterator"));
                }
                false
            }
            Ty::Host { arguments, .. } => {
                for argument in arguments.iter() {
                    if self.needs_drop_in(argument, at, visiting)? {
                        return Err(unsupported("a library type"));
                    }
                }
                false
            }
            Ty::Param(..) | Ty::Projection(..) | Ty::Dyn(..) | Ty::Infer(_) | Ty::Unknown => true,
            _ => false,
        })
    }

    /// Open the scope of a block, arm, body or iteration.
    pub(super) fn open_drops(&mut self) {
        if self.drop_scoped() {
            self.drop_scopes.push(Scope::default());
        }
    }

    /// The local `name` bound at `declaration` drops at the end of the
    /// innermost scope. When it shadows a local of this body whose value
    /// may have drop glue, that value is kept in a hidden local, whose
    /// statement (`let hidden = __rawr_move(name);`) the caller writes
    /// before the new binding, after its value.
    pub(super) fn note_drop(
        &mut self,
        name: &ImmutableString,
        declaration: Span,
        ty: &Ty,
    ) -> Result<Option<String>, Error> {
        if !self.drop_scoped() || self.drop_scopes.len() <= self.drop_base {
            return Ok(None);
        }
        let copies = self.shadow_copies(std::slice::from_ref(name))?;
        let hold = (!copies.is_empty()).then_some(copies);
        if let Some(scope) = self.drop_scopes.last_mut() {
            scope.locals.push(Local {
                place: name.clone(),
                name: name.clone(),
                declaration,
                ty: ty.clone(),
            });
        }
        Ok(hold)
    }

    /// The statements that copy each live local of this body named in
    /// `names` whose value may have drop glue, before a binding of that
    /// name shadows it: a drop at a jump out reads the copy, and once the
    /// innermost scope ends the name reads the local again. A local the
    /// innermost scope binds stays shadowed.
    pub(super) fn shadow_copies(&mut self, names: &[ImmutableString]) -> Result<String, Error> {
        let mut text = String::new();
        if !self.drops() || self.drop_scopes.len() <= self.drop_base {
            return Ok(text);
        }
        let base = self.drop_base;
        for name in names {
            let mut shadowed = None;
            for (index, scope) in self.drop_scopes[base..].iter().enumerate() {
                for (position, local) in scope.locals.iter().enumerate() {
                    if local.place == *name {
                        shadowed = Some((base + index, position));
                    }
                }
            }
            let Some((scope, position)) = shadowed else {
                continue;
            };
            let local = self.drop_scopes[scope].locals[position].clone();
            if !self.needs_drop(&local.ty, local.declaration)? {
                continue;
            }
            let copy = self.temporary("shadowed");
            let _ = write!(text, "let {copy} = {name}; ");
            self.drop_scopes[scope].locals[position].place = copy.as_str().into();
            let innermost = self.drop_scopes.len() - 1;
            if scope < innermost {
                self.drop_scopes[innermost].shadowed.push((scope, position));
            }
        }
        Ok(text)
    }

    /// `note_drop` for a binding whose statement cannot keep a shadowed
    /// value: shadowing one with drop glue is an error.
    pub(super) fn note_drop_here(
        &mut self,
        name: &ImmutableString,
        declaration: Span,
        ty: &Ty,
    ) -> Result<(), Error> {
        match self.note_drop(name, declaration, ty)? {
            None => Ok(()),
            Some(_) => Err(Error::UnsupportedNominalSyntax {
                construct: crate::resolve::Unsupported::said(format!(
                    "a pattern binding `{name}` that shadows a value whose type implements \
                     `Drop`: keeping the shadowed value is not supported in Rawr yet"
                )),
                source: declaration,
            }),
        }
    }

    /// The drops of `locals`, in reverse order of declaration.
    fn local_drops(&self, locals: &[Local]) -> Result<String, Error> {
        let mut text = String::new();
        for local in locals.iter().rev() {
            if self.needs_drop(&local.ty, local.declaration)? {
                text.push_str(&glue_call(&local.place, &self.drop_shape(&local.ty)));
                text.push(' ');
            }
        }
        Ok(text)
    }

    /// Close the innermost scope: the drops of its locals, as statements.
    pub(super) fn close_drops(&mut self) -> Result<String, Error> {
        self.close_drops_after(&[])
    }

    /// Close the innermost scope: the drops of its locals, then of `tail`,
    /// its tail's temporaries that outlive them, then of its parameters.
    fn close_drops_after(&mut self, tail: &[String]) -> Result<String, Error> {
        if !self.drop_scoped() {
            return Ok(self.temporary_drops(tail));
        }
        let Some(scope) = self.drop_scopes.pop() else {
            return Ok(self.temporary_drops(tail));
        };
        // The outer locals this scope shadowed have their names again.
        for (outer, position) in scope.shadowed.iter().rev() {
            if let Some(local) = self
                .drop_scopes
                .get_mut(*outer)
                .and_then(|outer| outer.locals.get_mut(*position))
            {
                local.place = local.name.clone();
            }
        }
        if !scope.uses_recorded {
            for local in scope.locals.iter().rev() {
                self.note_drop_use(local);
            }
        }
        // A body's tail temporaries drop after its locals, before its
        // parameters.
        let (parameters, locals) = scope.locals.split_at(scope.parameters);
        let mut drops = self.local_drops(locals)?;
        drops.push_str(&self.temporary_drops(tail));
        drops.push_str(&self.local_drops(parameters)?);
        if let Some(end) = &scope.thread_end {
            drops.push_str(end);
        }
        Ok(drops)
    }

    /// What a jump out of every scope above `depth` drops, innermost
    /// first: the temporaries made so far in each temporary scope inside
    /// them, each before the locals of the scope it is in, as rustc drops
    /// them. A statement's, which may be its block's tail before edition
    /// 2024, go in a place its end chooses (`Exit`).
    pub(super) fn exit_drops(&mut self, depth: usize) -> Result<String, Error> {
        if !self.drops() {
            return Ok(String::new());
        }
        // The temporary scopes the jump leaves, innermost first, by
        // position and depth.
        let leaving: Vec<(usize, usize)> = (self.temporary_base..self.temporary_scopes.len())
            .rev()
            .map(|at| (at, self.temporary_scopes[at].depth))
            .take_while(|&(_, inside)| inside > depth)
            .collect();
        let mut temporaries = leaving.into_iter().peekable();
        let mut text = String::new();
        for index in (depth..self.drop_scopes.len()).rev() {
            let mut late = String::new();
            while let Some((at, _)) = temporaries.next_if(|&(_, inside)| inside > index) {
                let drops = self.temporary_drops(&self.temporary_scopes[at].made);
                if !self.temporary_scopes[at].tail || drops.is_empty() {
                    text.push_str(&drops);
                    continue;
                }
                let exit = Exit {
                    early: self.temporary("early"),
                    late: self.temporary("late"),
                    drops,
                };
                let (early_mark, late_mark) = exit.marks();
                text.push_str(&early_mark);
                late.push_str(&late_mark);
                self.temporary_scopes[at].exits.push(exit);
            }
            let scope = &self.drop_scopes[index];
            let (parameters, locals) = scope.locals.split_at(scope.parameters);
            text.push_str(&self.local_drops(locals)?);
            text.push_str(&late);
            text.push_str(&self.local_drops(parameters)?);
            if let Some(end) = &scope.thread_end {
                text.push_str(end);
            }
        }
        Ok(text)
    }

    /// End the thread's thread-local values with `statement` after the
    /// innermost scope's drops, at its end and at each jump out of it.
    pub(super) fn end_thread_after_scope(&mut self, statement: String) {
        if let Some(scope) = self.drop_scopes.last_mut() {
            scope.thread_end = Some(statement);
        }
    }

    /// `value` with `drops` after it: `{ let v = value; drops v }`.
    pub(super) fn then_drops(&mut self, value: origin::Fragment, drops: &str) -> origin::Fragment {
        if drops.is_empty() {
            return value;
        }
        let kept = self.temporary("dropped");
        let mut block = origin::Fragment::plain(format!("{{ let {kept} = "));
        block.push(value);
        block.push_text(format!("; {drops}{kept} }}"));
        block
    }

    /// A block's statements and tail with the drops of its scope, closed
    /// here, after the tail's value, then the drops of `temporaries`, the
    /// tail's that outlive the block's locals.
    pub(super) fn close_block_drops(
        &mut self,
        prelude: &mut origin::Fragment,
        source: &mut origin::Fragment,
        tail: bool,
        temporaries: &[String],
    ) -> Result<(), Error> {
        let drops = self.close_drops_after(temporaries)?;
        if drops.is_empty() {
            return Ok(());
        }
        if tail {
            prelude.push_text(declarations(temporaries));
            let kept = self.temporary("dropped");
            prelude.push_text(format!("let {kept} = "));
            prelude.push(std::mem::replace(
                source,
                origin::Fragment::plain(kept.clone()),
            ));
            prelude.push_text(";\n");
        }
        prelude.push_text(drops);
        prelude.push_text("\n");
        Ok(())
    }

    /// A tail, `return`, `break` or `?` value that names a local, or a part
    /// of one, moves it out. With drop glue its value moves out at run time,
    /// to drop with its new owner; otherwise it reads as it did, and the
    /// move check follows the move.
    pub(super) fn move_out_value(&mut self, value: &mut ParsedExpr) -> Result<(), Error> {
        if self.needs_drop(&value.ty, value.span)? {
            self.move_local(value)
        } else {
            self.move_local(&mut value.clone())
        }
    }

    /// An expression statement's value, which nothing keeps, drops at the
    /// statement's end: a local named there moves out to be dropped, as
    /// in Rust.
    pub(super) fn drop_statement_value(
        &mut self,
        value: &ParsedExpr,
    ) -> Result<Option<origin::Fragment>, Error> {
        if !self.needs_drop(&value.ty, value.span)? {
            return Ok(None);
        }
        let mut value = value.clone();
        self.move_local(&mut value)?;
        let shape = origin::Fragment::plain(self.drop_shape(&value.ty));
        Ok(Some(origin::Fragment::call(
            GLUE,
            [value.rendered_source(), shape],
        )))
    }

    /// Open a function's or method's body: its parameters, in order, drop
    /// at its end after its locals.
    pub(super) fn open_body_drops(
        &mut self,
        parameters: impl IntoIterator<Item = (ImmutableString, Span)>,
    ) -> Result<(), Error> {
        // The body's own tail, closed by `close_body_drops`.
        let _ = self.open_tails();
        if !self.drop_scoped() {
            return Ok(());
        }
        self.drop_base = self.drop_scopes.len();
        self.open_drops();
        for (name, declaration) in parameters {
            let Some(binding) = self.bindings.get(name.as_str()) else {
                continue;
            };
            let owned = match &binding.kind {
                BindingKind::Local => true,
                // An owned `self`.
                BindingKind::Receiver { .. } => {
                    !matches!(self.inference.resolve(&binding.ty), Ty::Reference { .. })
                }
                BindingKind::Invocation => false,
            };
            if owned {
                let ty = binding.ty.clone();
                self.note_drop_here(&name, declaration, &ty)?;
            }
        }
        if let Some(scope) = self.drop_scopes.last_mut() {
            scope.parameters = scope.locals.len();
        }
        Ok(())
    }

    /// An `async` function's parameters live in its future, which Rawr does
    /// not drop: one with drop glue is refused.
    pub(super) fn refuse_async_drops(
        &self,
        parameters: impl IntoIterator<Item = (Ty, Span)>,
    ) -> Result<(), Error> {
        for (ty, declaration) in parameters {
            if self.needs_drop(&ty, declaration)? {
                return Err(Error::UnsupportedNominalSyntax {
                    construct: crate::resolve::Unsupported::said(
                        "an `async` function's parameter whose type implements `Drop`: \
                                dropping it with the future is not supported in Rawr yet",
                    ),
                    source: declaration,
                });
            }
        }
        Ok(())
    }

    /// Close a body: its scope's drops after its tail's value.
    pub(super) fn close_body_drops(&mut self) -> Result<(), Error> {
        // The body's locals and parameters end, each after its drop, in
        // reverse order of declaration: a loan of one that a later one's
        // drop uses is rustc's E0597.
        let live: Vec<Span> = self
            .bindings
            .values()
            .map(|binding| binding.declaration)
            .collect();
        let guards = self.cell_guard_drops(&live);
        if self.drop_scoped() && self.drop_scopes.len() > self.drop_base {
            let mut uses = self.block_drop_uses();
            uses.extend(guards);
            let (parameters, declared): (Vec<Span>, Vec<Span>) = self
                .drop_scopes
                .last()
                .map(|scope| {
                    let declarations = scope.locals.iter().map(|local| local.declaration);
                    (
                        declarations.clone().take(scope.parameters).collect(),
                        declarations.skip(scope.parameters).collect(),
                    )
                })
                .unwrap_or_default();
            // At the body's `}`, as rustc reports what ends there.
            let end = self.last_read();
            self.flow().body_end(&declared, &parameters, &uses, end);
        } else if !guards.is_empty() {
            let end = self.last_read();
            self.flow().body_end(&[], &[], &guards, end);
        }
        let temporaries = std::mem::take(&mut self.tails.handed);
        let drops = self.close_drops_after(&temporaries)?;
        if drops.is_empty() {
            return Ok(());
        }
        match self.last_expression {
            Some((start, endpoint)) => {
                let mut value = self.output.split_off(start);
                value.trim_end();
                let mut tail = self.then_drops(value, &drops);
                if !temporaries.is_empty() {
                    // Declared before the tail, which gives them their values.
                    tail = tail.surrounded(&format!("{{ {}", declarations(&temporaries)), " }");
                }
                let start = self.write_output(tail, endpoint);
                self.last_expression = Some((start, endpoint));
            }
            None => {
                self.output.push_text(drops);
                self.output.push_text("\n");
            }
        }
        Ok(())
    }

    /// `place = replacement`: the place's old value drops once the new one
    /// is made, before it is stored, as in Rust. `read` reads the old one.
    pub(super) fn drop_replaced(
        &mut self,
        ty: &Ty,
        read: origin::Fragment,
        replacement: &mut ParsedExpr,
    ) -> Result<(), Error> {
        if !self.needs_drop(ty, replacement.span)? {
            return Ok(());
        }
        let kept = self.temporary("replacement");
        let mut source = origin::Fragment::plain(format!("{{ let {kept} = "));
        source.push(replacement.rendered_source());
        source.push_text(format!("; {GLUE}("));
        source.push(read);
        source.push_text(format!(", {}); {kept} }}", self.drop_shape(ty)));
        replacement.prelude = origin::Fragment::default();
        replacement.source = source;
        Ok(())
    }

    /// The old value an assignment to `lvalue` replaces: what reads it.
    pub(super) fn replaced_read(&mut self, lvalue: &Lvalue) -> Option<origin::Fragment> {
        Some(match lvalue {
            Lvalue::Binding { binding, .. } => origin::Fragment::plain(binding.as_str()),
            Lvalue::Place { place, .. } => place.clone(),
            Lvalue::Field(field) => field.place(),
            Lvalue::Nested(nested) => {
                let nested = (**nested).clone();
                let (read, place, _) = self.nested_parts(&nested);
                let mut block = origin::Fragment::plain("{ ");
                block.push(read);
                block.push(place);
                block.push_text(" }");
                block
            }
            Lvalue::Slice(_) => return None,
        })
    }

    /// What holds the value a pattern is matched on, which the pattern's
    /// moves leave `()` in. `at` is the subject's start: a `*` there reads
    /// through a reference, a place.
    pub(super) fn subject_holder(
        &mut self,
        subject: &ParsedExpr,
        scrutinee: &str,
        dereferenced: bool,
    ) -> Result<Holder, Error> {
        if !self.drops() {
            return Ok(Holder::None);
        }
        if dereferenced {
            return Ok(Holder::Place);
        }
        Ok(match &subject.lvalue {
            Some(Lvalue::Binding { binding, .. }) => Holder::Local(binding.to_string(), Vec::new()),
            Some(Lvalue::Field(field)) => {
                Holder::Local(field.binding.to_string(), vec![field.slot])
            }
            Some(_) => Holder::Place,
            None if subject.source.as_str() == "self" => Holder::Local("self".into(), Vec::new()),
            None => {
                // A value made for the match: it drops when the match ends,
                // what the pattern moved out aside.
                if self.needs_drop(&subject.ty, subject.span)? {
                    self.note_drop_here(&scrutinee.into(), subject.span, &subject.ty)?;
                }
                Holder::Local(scrutinee.into(), Vec::new())
            }
        })
    }

    /// The statements that leave `()` where `syntax`, matched on a value of
    /// type `ty` in `holder`, moves out a part with drop glue; the pattern's
    /// `bindings` drop at the end of the innermost scope.
    pub(super) fn pattern_drops(
        &mut self,
        syntax: Option<&pattern::Syntax>,
        ty: &Ty,
        holder: &Holder,
        bindings: &[(ImmutableString, Span, Ty)],
        order: Clears,
    ) -> Result<String, Error> {
        if !self.drops() {
            return Ok(String::new());
        }
        let mut moves = Vec::new();
        let at = syntax.map_or(Span::NONE, pattern::Syntax::span);
        match syntax {
            Some(syntax) => {
                self.pattern_moves(syntax, ty, &mut Vec::new(), &mut moves)?;
            }
            // A catch-all binding takes the whole value.
            None => {
                if self.needs_drop(ty, at)? {
                    moves.push(Vec::new());
                }
            }
        }
        for (name, declaration, ty) in bindings {
            self.note_drop_here(name, *declaration, ty)?;
        }
        if moves.is_empty() {
            return Ok(String::new());
        }
        // `if let Some(x) = x`: in the arm, the binding hides the local the
        // move leaves `()` in. The arm leaves it in the local's copy, which
        // a jump out drops (`shadow_copies`), and marks itself taken; the
        // local itself follows after the match.
        let shadowed = match holder {
            Holder::Local(place, _) if order == Clears::InArm => {
                bindings.iter().any(|(name, ..)| name.as_str() == place)
            }
            _ => false,
        };
        if shadowed {
            let Holder::Local(place, prefix) = holder else {
                unreachable!("a shadowed holder is a local");
            };
            let copy = self
                .drop_scopes
                .iter()
                .flat_map(|scope| scope.locals.iter())
                .rev()
                .find(|local| local.name.as_str() == place && local.place.as_str() != place)
                .map(|local| local.place.to_string());
            let Some(copy) = copy else {
                return Err(Error::UnsupportedNominalSyntax {
                    construct: crate::resolve::Unsupported::said(format!(
                        "a pattern binding that shadows `{place}`, the local it moves a value \
                         whose type implements `Drop` out of: not supported in Rawr yet"
                    )),
                    source: at,
                });
            };
            let taken = self.temporary("taken");
            let mut arm = clear_text(&copy, prefix, &moves);
            let _ = write!(arm, "{taken} = true; ");
            let after = format!("if {taken} {{ {}}} ", clear_text(place, prefix, &moves));
            self.deferred_clears
                .push((format!("let {taken} = false; "), after));
            return Ok(arm);
        }
        let (place, prefix) = match holder {
            Holder::None => return Ok(String::new()),
            Holder::Local(place, prefix) => (place, prefix),
            Holder::Place => {
                return Err(Error::UnsupportedNominalSyntax {
                    construct: crate::resolve::Unsupported::said(
                        "a pattern that moves a value whose type implements `Drop` out of \
                                a place other than a local or its field: not supported in Rawr yet",
                    ),
                    source: at,
                })
            }
        };
        Ok(clear_text(place, prefix, &moves))
    }

    /// The deferred moves `pattern_drops` made for arms: the declarations
    /// that go before the match and what runs after it.
    pub(super) fn take_deferred_clears(&mut self) -> (String, String) {
        let mut before = String::new();
        let mut after = String::new();
        for (declare, then) in std::mem::take(&mut self.deferred_clears) {
            before.push_str(&declare);
            after.push_str(&then);
        }
        (before, after)
    }

    /// `let pattern = value;` with the value in `whole`: the statements
    /// that leave `()` where the pattern moved parts out of a local, which
    /// go before the bindings, and those that do it in a value made for the
    /// statement and drop its rest, which go after them; and whether the
    /// value was made for it.
    pub(super) fn let_pattern_drops(
        &mut self,
        syntax: &pattern::Syntax,
        value: &ParsedExpr,
        ty: &Ty,
        whole: &str,
        dereferenced: bool,
    ) -> Result<(String, String, bool), Error> {
        let made = value.lvalue.is_none() && value.source.as_str() != "self" && !dereferenced;
        let holder = if made {
            Holder::Local(whole.into(), Vec::new())
        } else {
            self.subject_holder(value, whole, dereferenced)?
        };
        let bound = self.pattern_bindings(syntax);
        let mut text =
            self.pattern_drops(Some(syntax), ty, &holder, &bound, Clears::BeforeBindings)?;
        if !made || !self.needs_drop(ty, value.span)? {
            return Ok((text, String::new(), false));
        }
        // `let (ref a, _) = make();` keeps the value to the block's end,
        // Rust's temporary lifetime extension.
        if bound
            .iter()
            .any(|(_, _, ty)| matches!(self.inference.resolve(ty), Ty::Reference { .. }))
        {
            return Err(Error::UnsupportedNominalSyntax {
                construct: crate::resolve::Unsupported::said(
                    "a `let` pattern that borrows from a value whose type implements \
                            `Drop` made for it: keeping that value is not supported in Rawr yet",
                ),
                source: syntax.span(),
            });
        }
        text.push_str(&glue_call(whole, &self.drop_shape(ty)));
        Ok((String::new(), text, true))
    }

    /// The names `syntax` binds, in order, with their declarations and
    /// types.
    pub(super) fn pattern_bindings(
        &self,
        syntax: &pattern::Syntax,
    ) -> Vec<(ImmutableString, Span, Ty)> {
        let mut names = Vec::new();
        collect_names(syntax, &mut names);
        names
            .into_iter()
            .filter_map(|(name, declaration)| {
                let binding = self.bindings.get(name.as_str())?;
                (binding.declaration == declaration)
                    .then(|| (name, declaration, binding.ty.clone()))
            })
            .collect()
    }

    /// The paths (field, payload and element indices) of the parts with
    /// drop glue `syntax` binds by value from a value of type `ty` at
    /// `path`, into `moves`. Whether a part with drop glue stays behind.
    fn pattern_moves(
        &mut self,
        syntax: &pattern::Syntax,
        ty: &Ty,
        path: &mut Vec<usize>,
        moves: &mut Vec<Vec<usize>>,
    ) -> Result<bool, Error> {
        let resolved = self.inference.resolve(ty);
        // Through a reference, a pattern binds references: it owns nothing.
        if matches!(resolved, Ty::Reference { .. }) {
            return Ok(false);
        }
        let at = syntax.span();
        Ok(match syntax {
            pattern::Syntax::Binding { .. } | pattern::Syntax::At { .. } => {
                if self.needs_drop(&resolved, at)? {
                    moves.push(path.clone());
                }
                false
            }
            pattern::Syntax::Ref { .. } | pattern::Syntax::Discard { .. } => {
                self.needs_drop(&resolved, at)?
            }
            pattern::Syntax::Rest { binding, .. } => {
                let needs = self.needs_drop(&resolved, at)?;
                if needs && binding.is_some() {
                    moves.push(path.clone());
                    false
                } else {
                    needs
                }
            }
            pattern::Syntax::Literal { .. }
            | pattern::Syntax::Range { .. }
            | pattern::Syntax::UnitStruct { .. }
            | pattern::Syntax::Reference { .. } => false,
            pattern::Syntax::TupleStruct {
                definition,
                fields,
                members,
                ..
            } => {
                let before = moves.len();
                let mut leftover = false;
                for (slot, (member, field)) in members.iter().zip(fields.iter()).enumerate() {
                    path.push(slot);
                    leftover |= self.pattern_moves(member, field, path, moves)?;
                    path.pop();
                }
                self.reject_drop_move(definition.id(), moves.len() > before, at)?;
                leftover
            }
            pattern::Syntax::Tuple { members, span } => {
                let Ty::Tuple(types) = &resolved else {
                    return Ok(false);
                };
                let discard = pattern::Syntax::Discard { span: *span };
                let members = tuple_members(members, types.len(), &discard)?;
                let mut leftover = false;
                for (index, (member, ty)) in members.iter().zip(types.iter()).enumerate() {
                    path.push(index);
                    leftover |= self.pattern_moves(member, ty, path, moves)?;
                    path.pop();
                }
                leftover
            }
            pattern::Syntax::Enum(pattern) => {
                let Ty::Script(id, arguments) = &resolved else {
                    return Ok(false);
                };
                let Nominal::Enum { definition, item } = self.models.definition(*id).clone() else {
                    return Ok(false);
                };
                let Some(variant) = item
                    .variants
                    .iter()
                    .find(|variant| variant.name.text == pattern.variant)
                else {
                    return Ok(false);
                };
                let discard = pattern::Syntax::Discard {
                    span: pattern.variant_span,
                };
                let children: Vec<&pattern::Syntax> = match (&pattern.named, &variant.fields) {
                    (Some((written, rest)), declared) => {
                        let declared = declared.as_deref().unwrap_or_default();
                        let rest = *rest || variant.payload.is_empty();
                        let names = declared.iter().map(|field| field.text.as_str());
                        field_order(
                            definition.name(),
                            Some(&pattern.variant),
                            names,
                            written,
                            rest,
                            at,
                        )?
                        .into_iter()
                        .map(|index| index.map_or(&discard, |index| &pattern.payload[index]))
                        .chain(std::iter::repeat_n(
                            &discard,
                            if declared.is_empty() {
                                variant.payload.len()
                            } else {
                                0
                            },
                        ))
                        .collect()
                    }
                    (None, _) => tuple_members(&pattern.payload, variant.payload.len(), &discard)?,
                };
                let substitution = self.nominal_substitution(*id, arguments);
                let before = moves.len();
                let mut leftover = false;
                for (index, (child, payload)) in
                    children.into_iter().zip(variant.payload.iter()).enumerate()
                {
                    let payload = instantiate(&Ty::from_resolved(payload), &substitution);
                    path.push(index);
                    leftover |= self.pattern_moves(child, &payload, path, moves)?;
                    path.pop();
                }
                self.reject_drop_move(*id, moves.len() > before, at)?;
                leftover
            }
            pattern::Syntax::Prelude {
                variant, payload, ..
            } => {
                let inner = match (&resolved, variant) {
                    (Ty::Option(inner), Prelude::Some) | (Ty::Result(inner, _), Prelude::Ok) => {
                        (**inner).clone()
                    }
                    (Ty::Result(_, inner), Prelude::Err) => (**inner).clone(),
                    (polled, Prelude::Ready) if let Some(output) = self.poll_output(polled) => {
                        output
                    }
                    _ => return Ok(false),
                };
                let Some(payload) = payload else {
                    return self.needs_drop(&inner, at);
                };
                // A part of an `Option` or `Result` is not left behind: a
                // move out of its payload moves it whole.
                let mut inside = Vec::new();
                let leftover = self.pattern_moves(payload, &inner, &mut Vec::new(), &mut inside)?;
                if !inside.is_empty() {
                    if leftover {
                        return Err(self.partial_drop_move(at));
                    }
                    moves.push(path.clone());
                }
                leftover
            }
            // A `Cow::Owned` binding moves its owned value out, which drop
            // glue would need to follow.
            pattern::Syntax::Cow { owned, payload, .. } => {
                if let Ty::Cow(borrowed) = &resolved
                    && *owned
                    && binds_by_value(payload)
                    && let Some(owned) = self.owned_of(borrowed)
                    && self.needs_drop(&owned, at)?
                {
                    return Err(Error::UnsupportedNominalSyntax {
                        construct: crate::resolve::Unsupported::said(
                            "a `Cow::Owned` pattern that moves a value whose type \
                                    implements `Drop`: not supported in Rawr yet",
                        ),
                        source: at,
                    });
                }
                false
            }
            pattern::Syntax::Or { alternatives, .. } => {
                let mut leftover = false;
                for alternative in alternatives.iter() {
                    let mut inside = Vec::new();
                    leftover |= self.pattern_moves(alternative, &resolved, path, &mut inside)?;
                    if !inside.is_empty() {
                        return Err(Error::UnsupportedNominalSyntax {
                            construct: crate::resolve::Unsupported::said(
                                "an `|` pattern that moves a value whose type implements \
                                        `Drop`: not supported in Rawr yet",
                            ),
                            source: at,
                        });
                    }
                }
                leftover
            }
            pattern::Syntax::Slice { .. } => {
                if binds_by_value(syntax) && self.needs_drop(&resolved, at)? {
                    return Err(Error::UnsupportedNominalSyntax {
                        construct: crate::resolve::Unsupported::said(
                            "a slice pattern that moves a value whose type implements \
                                    `Drop`: not supported in Rawr yet",
                        ),
                        source: at,
                    });
                }
                self.needs_drop(&resolved, at)?
            }
        })
    }

    fn partial_drop_move(&self, at: Span) -> Error {
        Error::UnsupportedNominalSyntax {
            construct: crate::resolve::Unsupported::said(
                "a pattern that moves a part of an `Option` or `Result` whose type \
                        implements `Drop` and leaves another such part: not supported in Rawr yet",
            ),
            source: at,
        }
    }

    /// rustc's E0509: a part cannot move out of a value whose type
    /// implements `Drop`, whose `drop` sees the whole value.
    pub(super) fn reject_drop_move(
        &self,
        owner: type_def::Id,
        moved: bool,
        at: Span,
    ) -> Result<(), Error> {
        if !moved || !self.models.has_drop(owner) {
            return Ok(());
        }
        let name = self.models.definition(owner).name();
        Err(Error::BorrowConflict {
            code: "E0509",
            message: format!("cannot move out of type `{name}`, which implements the `Drop` trait")
                .into(),
            access: at,
            borrow: at,
        })
    }

    /// The temporaries `made` by a `let` condition's subject, as locals of
    /// the condition's scope, just opened: they drop with it, and at a jump
    /// out of it. Where the scope keeps no locals, they stay the enclosing
    /// statement's, and none is the scope's.
    pub(super) fn scope_temporaries(&mut self, made: Vec<String>) -> Result<Vec<String>, Error> {
        if !self.drop_scoped() || self.drop_scopes.len() <= self.drop_base {
            if let Some(scope) = self.temporary_scopes.last_mut() {
                scope.made.extend(made);
            }
            return Ok(Vec::new());
        }
        for temporary in &made {
            let binding = &self.bindings[temporary.as_str()];
            let (declaration, ty) = (binding.declaration, binding.ty.clone());
            self.note_drop_here(&temporary.as_str().into(), declaration, &ty)?;
        }
        Ok(made)
    }

    /// The temporary scope of a `let` closed: the temporaries the `let`
    /// extends, as the borrow facts have it, and the others, which drop
    /// when its value is made.
    pub(super) fn extended_temporaries(&mut self) -> (Vec<String>, Vec<String>) {
        let temporaries = self.close_temporaries();
        if temporaries.is_empty() {
            return (Vec::new(), Vec::new());
        }
        let extends = self.flow().extended();
        temporaries.into_iter().partition(|temporary| {
            self.bindings
                .get(temporary.as_str())
                .is_some_and(|binding| extends.contains(&binding.declaration))
        })
    }

    /// `extended`, temporaries a `let` of the value at `value` keeps to the
    /// end of its block, as locals of the block's scope: their
    /// declarations, which go before the `let`.
    pub(super) fn extend_temporaries(
        &mut self,
        extended: &[String],
        value: Span,
    ) -> Result<String, Error> {
        if extended.is_empty() {
            return Ok(String::new());
        }
        if !self.drop_scoped() || self.drop_scopes.len() <= self.drop_base {
            return Err(Error::UnsupportedNominalSyntax {
                construct: crate::resolve::Unsupported::said(
                    "a `let` that borrows a temporary value whose type implements `Drop` \
                            outside a block: keeping it to the block's end is not supported in \
                            Rawr yet",
                ),
                source: value,
            });
        }
        for temporary in extended {
            let binding = &self.bindings[temporary.as_str()];
            let (declaration, ty) = (binding.declaration, binding.ty.clone());
            self.note_drop_here(&temporary.as_str().into(), declaration, &ty)?;
        }
        Ok(declarations(extended))
    }

    /// Open the tail of a block or body: before edition 2024 it keeps its
    /// tail's temporaries past its locals. Gives what the enclosing one's
    /// was, to close it with, and whether this block's escape to the
    /// temporary scope around it.
    pub(super) fn open_tails(&mut self) -> (Tails, bool) {
        let kept = self.drops() && !self.models.edition().drops_tail_temporaries_first();
        let mut outer = std::mem::replace(
            &mut self.tails,
            Tails {
                kept,
                ..Tails::default()
            },
        );
        let escapes = std::mem::take(&mut outer.escapes);
        (outer, escapes)
    }

    /// Close the tail opened with `outer` given: the temporaries its tail
    /// handed it.
    pub(super) fn close_tails(&mut self, outer: Tails) -> Vec<String> {
        std::mem::replace(&mut self.tails, outer).handed
    }

    /// The block ahead is a block expression: its tail's temporaries go on
    /// to the temporary scope around it before edition 2024.
    pub(super) fn escape_tails(&mut self) {
        self.tails.escapes = true;
    }

    /// `temporaries`, the block's tail's that outlive its locals: those of
    /// a block expression go on to the temporary scope around it, as
    /// Rust's do; the others drop at the block's end, after its locals.
    /// Gives the ones that drop there.
    pub(super) fn escaped_tails(&mut self, temporaries: Vec<String>, escapes: bool) -> Vec<String> {
        match self.temporary_scopes.last_mut() {
            Some(scope) if escapes => {
                scope.made.extend(temporaries);
                Vec::new()
            }
            _ => temporaries,
        }
    }

    /// The temporaries of the statement written from `start`, closed: a
    /// tail of a block or body that keeps them hands them to it, and none
    /// drop with the statement.
    /// A jump out of it put its drops of them before the block's locals,
    /// or after them for a tail that keeps them (`Exit`).
    pub(super) fn hand_tail(
        &mut self,
        start: usize,
        (temporaries, exits): (Vec<String>, Vec<Exit>),
    ) -> Vec<String> {
        let kept =
            matches!(self.last_expression, Some((tail, _)) if tail >= start) && self.tails.kept;
        for exit in &exits {
            let (early, late) = exit.marks();
            let (now, never) = if kept { (late, early) } else { (early, late) };
            self.output.replace(&now, &exit.drops);
            self.output.replace(&never, "");
        }
        if kept {
            self.tails.handed.extend(temporaries);
            return Vec::new();
        }
        temporaries
    }

    /// Open a temporary scope (a statement, a match arm, a condition): the
    /// values made in it that nothing moves drop at its end.
    pub(super) fn open_temporaries(&mut self) {
        self.open_temporary_scope(false);
    }

    /// Open a statement's temporary scope, which may be its block's tail.
    pub(super) fn open_statement_temporaries(&mut self) {
        self.open_temporary_scope(self.tails.kept);
    }

    fn open_temporary_scope(&mut self, tail: bool) {
        if self.drops() {
            self.temporary_scopes.push(Temporaries {
                depth: self.drop_scopes.len(),
                made: Vec::new(),
                tail,
                exits: Vec::new(),
            });
        }
    }

    /// Close a temporary scope: its temporaries, in the order made.
    pub(super) fn close_temporaries(&mut self) -> Vec<String> {
        if !self.drops() {
            return Vec::new();
        }
        self.close_statement_temporaries().0
    }

    /// Close a statement's temporary scope: its temporaries, in the order
    /// made, and the drops of them jumps out of it wrote (`hand_tail`).
    pub(super) fn close_statement_temporaries(&mut self) -> (Vec<String>, Vec<Exit>) {
        if !self.drops() {
            return (Vec::new(), Vec::new());
        }
        self.temporary_scopes
            .pop()
            .map(|scope| (scope.made, scope.exits))
            .unwrap_or_default()
    }

    /// `value` with the temporaries made in it: `{ let t = (); let v =
    /// value; drops v }`, the temporaries dropped in reverse order.
    pub(super) fn with_temporaries(
        &mut self,
        temporaries: &[String],
        value: origin::Fragment,
    ) -> origin::Fragment {
        if temporaries.is_empty() {
            return value;
        }
        let kept = self.temporary("made");
        let mut block = origin::Fragment::plain("{ ");
        for temporary in temporaries {
            block.push_text(format!("let {temporary} = (); "));
        }
        block.push_text(format!("let {kept} = "));
        block.push(value);
        block.push_text(format!("; {}{kept} }}", self.temporary_drops(temporaries)));
        block
    }

    /// The drops of `temporaries`, in reverse order of making.
    pub(super) fn temporary_drops(&self, temporaries: &[String]) -> String {
        temporaries
            .iter()
            .rev()
            .map(|temporary| {
                let shape = self
                    .temporary_shapes
                    .get(temporary)
                    .map_or(WALK, String::as_str);
                glue_call(temporary, shape) + " "
            })
            .collect()
    }

    /// A value made by an expression, with drop glue, that a borrow,
    /// a field's read or a method by reference uses: it is kept in a
    /// temporary of the enclosing temporary scope, which drops it at its
    /// end, as Rust's temporaries are. A place, or a value read through a
    /// reference, is not one.
    pub(super) fn hold_temporary(
        &mut self,
        value: &mut ParsedExpr,
    ) -> Result<Option<ImmutableString>, Error> {
        if !self.drops()
            || value.lvalue.is_some()
            || variable_name(value.source.as_str()).is_some()
            || self.borrowed.contains(&value.span)
            || matches!(self.inference.resolve(&value.ty), Ty::Reference { .. })
            || !self.needs_drop(&value.ty, value.span)?
        {
            return Ok(None);
        }
        let temporary = self.temporary("temporary");
        let Some(scope) = self.temporary_scopes.last_mut() else {
            return Err(Error::UnsupportedNominalSyntax {
                construct: crate::resolve::Unsupported::said(
                    "a temporary value whose type implements `Drop` outside a statement: \
                            dropping it where Rust does is not supported in Rawr yet",
                ),
                source: value.span,
            });
        };
        scope.made.push(temporary.clone());
        let shape = self.drop_shape(&value.ty);
        if shape != WALK {
            self.temporary_shapes.insert(temporary.clone(), shape);
        }
        // In parentheses, so a statement may start with it (`make().ok();`).
        let mut source = origin::Fragment::plain(format!("({{ {temporary} = "));
        source.push(value.rendered_source());
        source.push_text(format!("; {temporary} }})"));
        value.source = source;
        value.prelude = origin::Fragment::default();
        // A field moved out of it leaves `()` there.
        self.bindings.insert(
            temporary.as_str().into(),
            Binding {
                ty: value.ty.clone(),
                mutable: false,
                available: true,
                declaration: value.span,
                kind: BindingKind::Local,
            },
        );
        Ok(Some(temporary.into()))
    }

    /// `value.member(arguments)` of `Option` or `Result` (`result`), in a
    /// crate with `impl Drop`: a method that takes the value moves it out
    /// of its local, one by reference keeps a value made here in a
    /// temporary, and one that would drop a part it does not give back is
    /// refused when that part has drop glue.
    pub(super) fn builtin_enum_receiver(
        &mut self,
        value: &mut ParsedExpr,
        result: Option<(&Ty, &Ty)>,
        member: &str,
        arguments: &[ParsedExpr],
        at: Span,
    ) -> Result<(), Error> {
        // How std's declaration takes the receiver (D711 step 4).
        let owner = self.inference.resolve(&value.ty);
        let by_reference = super::std_method::receiver_mode(without_references(&owner), member)
            .is_some_and(|mode| mode != crate::receiver::Mode::Owned);
        // `r.unwrap()` of an `&Option<String>`: a method that takes the
        // value moves it out from behind the reference, rustc's E0507.
        // std's `IntoIterator` for `&Option<T>` and `&Result<T, E>` takes
        // the reference itself.
        if let Ty::Reference { mutable, target } = self.inference.resolve(&value.ty) {
            if !by_reference && member != "into_iter" && self.moves_by_value(&target) {
                let behind = if mutable { "mutable" } else { "shared" };
                return Err(Error::BorrowConflict {
                    code: "E0507",
                    message: format!(
                        "cannot move out of a value behind a {behind} reference by `{member}`"
                    )
                    .into(),
                    access: at,
                    borrow: at,
                });
            }
            return Ok(());
        }
        // Without drops, a method that takes the value only moves it out of
        // its place, as `o.unwrap()` does (T153).
        if !self.drops() {
            if !by_reference {
                self.move_local(&mut value.clone())?;
            }
            return Ok(());
        }
        // The part a method drops: the other variant's value, or an
        // argument, or the receiver itself.
        let discarded = match (member, result) {
            ("ok" | "is_ok_and" | "unwrap_or_default" | "into_iter", Some((_, err))) => {
                Some(self.needs_drop(err, at)?)
            }
            ("err" | "is_err_and", Some((ok, _))) => Some(self.needs_drop(ok, at)?),
            // `Option::unwrap_or` drops its unused default itself.
            ("unwrap_or", None) => None,
            (
                "unwrap_or" | "or" | "and" | "xor" | "zip" | "filter" | "map_or" | "ok_or"
                | "insert" | "get_or_insert",
                _,
            ) => {
                let mut needs = self.needs_drop(&value.ty, at)?;
                for argument in arguments {
                    needs |= self.needs_drop(&argument.ty, argument.span)?;
                }
                Some(needs)
            }
            _ => None,
        };
        if discarded == Some(true) {
            return Err(Error::UnsupportedNominalSyntax {
                construct: crate::resolve::Unsupported::said(format!(
                    "`{member}`, which drops a value whose type implements `Drop` that it does \
                     not return: dropping it where Rust does is not supported in Rawr yet"
                )),
                source: at,
            });
        }
        if !self.needs_drop(&value.ty, at)? {
            return Ok(());
        }
        if by_reference {
            self.hold_temporary(value)?;
        } else {
            self.move_local(value)?;
        }
        Ok(())
    }

    /// `return value`, `break value`: `value`, then the drops of the scopes
    /// the jump leaves, then `jump` with the kept value.
    pub(super) fn jump_with_drops(
        &mut self,
        drops: &str,
        jump: &str,
        value: Option<origin::Fragment>,
    ) -> origin::Fragment {
        let mut source = origin::Fragment::plain("{ ");
        match value {
            Some(value) => {
                let kept = self.temporary("leaving");
                source.push_text(format!("let {kept} = "));
                source.push(value);
                source.push_text(format!("; {drops}{jump} {kept}; }}"));
            }
            None => source.push_text(format!("{drops}{jump}; }}")),
        }
        source
    }
}
