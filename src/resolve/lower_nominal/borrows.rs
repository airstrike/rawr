//! The lowering's side of the borrow checker (D572): what each construct
//! records in the body's facts (`borrowck::record`), and the check once
//! the body is read. Rawr has no references at runtime: a shared borrow is
//! a copy and a `&mut` local is its place, which matches Rust exactly when
//! the borrows are the ones rustc accepts. What this records ends each
//! borrow where stable rustc's NLL does, through branches and loops.

use super::*;
use crate::resolve::borrowck::{
    self,
    record::{self, Matched, Recorder, ValueStart},
    signature::{self, Keeps, Summary},
    Access, Origin, Place, Projection,
};

/// A method call's receiver, as the call's value may keep it.
#[derive(Clone)]
pub(super) struct CallReceiver {
    start: Position,
    span: Span,
    ty: Ty,
    place: bool,
}

/// Whether a value of type `ty` is a reference, whose own loan is apart from
/// what it refers to.
pub(super) fn is_reference(ty: &Ty) -> bool {
    matches!(ty, Ty::Reference { .. } | Ty::Str)
}

/// What a pattern does to the parts of what it matches, collected while
/// the pattern is resolved: the path below the subject being read, and
/// each part it tests or binds there.
#[derive(Default)]
pub(super) struct PatternTrace {
    path: Vec<Projection>,
    parts: Vec<Part>,
    /// The `ref` or `ref mut` binding being resolved: its mutability.
    explicit: Option<bool>,
    unmodeled: Option<(&'static str, Span)>,
}

struct Part {
    path: Vec<Projection>,
    kind: PartKind,
}

enum PartKind {
    /// A test of the part: a discriminant, a literal or a range, read
    /// where the subject is, or a slice's length, read where its pattern is
    /// (`Some`), as rustc reports them.
    Test(Access, Option<Span>),
    /// A binding of the part, by value or by reference (`Some` with its
    /// mutability), of type `ty`.
    Bind {
        name: ImmutableString,
        span: Span,
        by_ref: Option<bool>,
        ty: Ty,
    },
}

/// The place `path` below `base`.
fn below(base: &Place, path: &[Projection]) -> Place {
    let mut place = base.clone();
    place.projection.extend(path.iter().cloned());
    place
}

/// The element a slice pattern's member names: the `index`th of `count`
/// members before its `..`, or after it, with `fixed` members in all, of an
/// array of `length` or a slice. An array's are counted from its start.
pub(super) fn slice_element(
    index: usize,
    count: usize,
    after: bool,
    fixed: usize,
    length: Option<usize>,
) -> Projection {
    let (offset, min_length, from_end) = match (after, length) {
        (false, _) => (index, length.unwrap_or(fixed), false),
        (true, Some(length)) => (length - count + index, length, false),
        // rustc counts from the end from 1, the last element.
        (true, None) => (count - index, fixed, true),
    };
    Projection::ConstantIndex {
        offset: offset as u32,
        min_length: min_length as u32,
        from_end,
    }
}

impl Parser<'_> {
    /// Starts collecting what the pattern about to be resolved does to its
    /// subject's parts; the trace it replaces, for [`Parser::end_trace`].
    pub(super) fn begin_trace(&self) -> Option<PatternTrace> {
        self.pattern_trace.replace(Some(PatternTrace::default()))
    }

    /// The trace collected since [`Parser::begin_trace`], which gave
    /// `outer`.
    pub(super) fn end_trace(&self, outer: Option<PatternTrace>) -> PatternTrace {
        self.pattern_trace.replace(outer).unwrap_or_default()
    }

    /// The pattern being resolved steps below its part: the path's length
    /// before, for [`Parser::trace_leave`].
    pub(super) fn trace_enter(&self, step: Projection) -> usize {
        let mut trace = self.pattern_trace.borrow_mut();
        let Some(trace) = trace.as_mut() else {
            return 0;
        };
        trace.path.push(step);
        trace.path.len() - 1
    }

    /// The length of the path below the subject being read, for
    /// [`Parser::trace_leave`].
    pub(super) fn trace_length(&self) -> usize {
        self.pattern_trace
            .borrow()
            .as_ref()
            .map_or(0, |trace| trace.path.len())
    }

    pub(super) fn trace_leave(&self, length: usize) {
        if let Some(trace) = self.pattern_trace.borrow_mut().as_mut() {
            trace.path.truncate(length);
        }
    }

    /// The pattern being resolved tests its part, where the subject is or
    /// at `at`.
    pub(super) fn trace_test(&self, access: Access, at: Option<Span>) {
        if let Some(trace) = self.pattern_trace.borrow_mut().as_mut() {
            let path = trace.path.clone();
            trace.parts.push(Part {
                path,
                kind: PartKind::Test(access, at),
            });
        }
    }

    /// The pattern being resolved binds its part to `name`: by reference
    /// in a `ref` binding or Rust's default binding mode, else by value.
    pub(super) fn trace_bind(&self, name: &ImmutableString, span: Span, ty: &Ty) {
        if let Some(trace) = self.pattern_trace.borrow_mut().as_mut() {
            let by_ref = trace.explicit.take().or(self.binding_mode.get());
            let path = trace.path.clone();
            trace.parts.push(Part {
                path,
                kind: PartKind::Bind {
                    name: name.clone(),
                    span,
                    by_ref,
                    ty: ty.clone(),
                },
            });
        }
    }

    /// The binding about to be traced is `ref` (or `ref mut`).
    pub(super) fn trace_ref(&self, mutable: bool) {
        if let Some(trace) = self.pattern_trace.borrow_mut().as_mut() {
            trace.explicit = Some(mutable);
        }
    }

    /// The pattern being resolved does something to its subject these
    /// facts do not model.
    pub(super) fn trace_unmodeled(&self, what: &'static str, span: Span) {
        if let Some(trace) = self.pattern_trace.borrow_mut().as_mut() {
            trace.unmodeled.get_or_insert((what, span));
        }
    }

    /// What `subject`, begun at `start`, is to the patterns matched on it.
    pub(super) fn matched(&self, subject: &ParsedExpr, start: ValueStart) -> Matched {
        let place = subject.lvalue.is_some() || self.flow().is_place(subject.span);
        let matched = self
            .flow()
            .matched(subject.span.start(), subject.span, place, start);
        match matched {
            // A value that is no reference holds its borrows in every part.
            Matched::Origins(origins) if !is_reference(&self.inference.resolve(&subject.ty)) => {
                Matched::Held(origins)
            }
            matched => matched,
        }
    }

    /// A pattern's facts, from its trace, on what it matches (the subject
    /// spanning `subject`): its tests, then its bindings, each the local
    /// `declarations` declares by its name.
    pub(super) fn pattern_facts(
        &self,
        trace: PatternTrace,
        matched: &Matched,
        subject: Span,
        declarations: &[PatternBinding],
    ) {
        self.pattern_tests(&trace, matched, subject);
        self.pattern_binds(trace, matched, declarations);
    }

    /// A pattern's tests, read where the subject spanning `subject` is. A
    /// match's arm tests before its bindings are made, after the tests of
    /// the arms before it: rustc's false edges let each arm follow those.
    pub(super) fn pattern_tests(&self, trace: &PatternTrace, matched: &Matched, subject: Span) {
        let Matched::Place(base) = matched else {
            return;
        };
        let mut flow = self.flow();
        for part in &trace.parts {
            if let PartKind::Test(access, at) = &part.kind {
                flow.part(below(base, &part.path), *access, at.unwrap_or(subject));
            }
        }
    }

    /// A pattern's bindings, each the local `declarations` declares by its
    /// name: a borrow of its part, a read or a move of it, or the loans of
    /// the reference it is below.
    pub(super) fn pattern_binds(
        &self,
        trace: PatternTrace,
        matched: &Matched,
        declarations: &[PatternBinding],
    ) {
        // What each binding is, before the facts are borrowed.
        let binds: Vec<_> = trace
            .parts
            .into_iter()
            .filter_map(|part| match part.kind {
                PartKind::Bind {
                    name,
                    span,
                    by_ref,
                    ty,
                } => {
                    let value = match (by_ref, self.inference.resolve(&ty)) {
                        (Some(_), Ty::Reference { target, .. }) => *target,
                        (_, ty) => ty,
                    };
                    let holds = self.may_hold_borrow(&value);
                    let moves = self.moves_by_value(&ty);
                    Some((part.path, name, span, by_ref, holds, moves))
                }
                PartKind::Test(..) => None,
            })
            .collect();
        let mut flow = self.flow();
        if let Some((what, span)) = trace.unmodeled {
            flow.unmodeled(what, span);
        }
        // An or-pattern's alternatives bind one local: it is given its
        // value once, and takes each alternative's loans.
        let mut defined = std::collections::BTreeSet::new();
        for (path, name, span, by_ref, holds, moves) in binds {
            let declaration = declarations
                .iter()
                .find(|binding| binding.name == name)
                .map_or(span, |binding| binding.span);
            let into = flow.local(&name, declaration);
            let define = defined.insert(into);
            match (matched, by_ref) {
                (Matched::Place(base), Some(mutable)) => {
                    flow.part_borrow(below(base, &path), mutable, span, into, define);
                }
                (Matched::Place(base), None) => {
                    let access = if moves { Access::Move } else { Access::Read };
                    flow.part(below(base, &path), access, span);
                    // The whole of a local that holds borrows: its loans.
                    if holds && path.is_empty() && base.projection.is_empty() {
                        flow.take(into, &[Origin::Local(base.local)], define);
                    }
                }
                // Below a reference made for the match, a binding by
                // reference reborrows through it; the reference itself,
                // bound whole, is its loans.
                (Matched::Origins(origins), Some(_)) => flow.take(into, origins, define),
                (Matched::Origins(origins), None) if holds && path.is_empty() => {
                    flow.take(into, origins, define);
                }
                (Matched::Origins(_), None) => {}
                // `Some(v)` of `v.first()`: a part of a value holding
                // borrows holds them, bound by value or by reference.
                (Matched::Held(origins), by_ref) if holds || by_ref.is_some() => {
                    flow.take(into, origins, define);
                }
                (Matched::Held(_), _) => {}
                (Matched::Value { made }, by_ref) => {
                    if *made && (by_ref.is_some() || holds) {
                        flow.unmodeled("a pattern binding a borrow its subject made", span);
                    }
                }
            }
        }
    }

    /// A match arm `name =>`, `name` declared at `declaration`, binds the
    /// whole subject (of type `ty`) by value.
    pub(super) fn binding_trace(
        &self,
        name: &ImmutableString,
        declaration: Span,
        ty: &Ty,
    ) -> PatternTrace {
        let mut trace = PatternTrace::default();
        trace.parts.push(Part {
            path: Vec::new(),
            kind: PartKind::Bind {
                name: name.clone(),
                span: declaration,
                by_ref: None,
                ty: ty.clone(),
            },
        });
        trace
    }

    /// An arm of a match on a scalar or a string of type `written`: whether
    /// its pattern tests the value, and the name it binds, with its
    /// declaration.
    pub(super) fn scalar_trace(
        &self,
        tests: bool,
        binding: Option<(&ImmutableString, Span)>,
        written: &Ty,
    ) -> PatternTrace {
        let mut trace = PatternTrace::default();
        let derefs = self.derefs(written);
        if tests {
            trace.parts.push(Part {
                path: derefs.clone(),
                kind: PartKind::Test(Access::Read, None),
            });
        }
        match binding {
            // Rawr binds the value below a reference, which Rust binds as
            // the reference: the loans it keeps are left to rustc.
            Some((_, span)) if !derefs.is_empty() => {
                trace.unmodeled = Some(("a binding of a matched reference's value", span));
            }
            Some((name, span)) => trace.parts.push(Part {
                path: Vec::new(),
                kind: PartKind::Bind {
                    name: name.clone(),
                    span,
                    by_ref: None,
                    ty: written.clone(),
                },
            }),
            None => {}
        }
        trace
    }

    /// An arm of a match on a facade value of type `written`, whose pattern
    /// spanning `span` compiled to `compiled`: its tests read the value;
    /// what it binds is left to rustc.
    pub(super) fn host_trace(
        &self,
        compiled: &HostPattern,
        written: &Ty,
        span: Span,
    ) -> PatternTrace {
        let mut trace = PatternTrace::default();
        if !compiled.tests.is_empty() {
            trace.parts.push(Part {
                path: self.derefs(written),
                kind: PartKind::Test(Access::Read, None),
            });
        }
        if !compiled.binds.is_empty() {
            trace.unmodeled = Some(("a pattern binding a part of a facade value", span));
        }
        trace
    }

    /// The steps through each reference `ty` is below.
    fn derefs(&self, ty: &Ty) -> Vec<Projection> {
        let mut steps = Vec::new();
        let mut ty = self.inference.resolve(ty);
        while let Ty::Reference { target, .. } = ty {
            steps.push(Projection::Deref);
            ty = self.inference.resolve(&target);
        }
        steps
    }

    /// A pattern whose facts are not recorded binds `bindings`: one by
    /// reference is left to rustc.
    pub(super) fn pattern_unrecorded(&self, bindings: &[PatternBinding], span: Span) {
        if bindings
            .iter()
            .any(|binding| self.may_hold_borrow(&binding.ty))
        {
            self.flow()
                .unmodeled("a pattern binding a part of its subject by reference", span);
        }
    }
    pub(super) fn flow(&self) -> std::cell::RefMut<'_, Recorder> {
        self.flow.borrow_mut()
    }

    /// A use of the local `name` declared at `declaration`, at `span`.
    pub(super) fn mention(&self, name: &str, declaration: Span, ty: &Ty, span: Span) {
        let ty = self.inference.resolve(ty);
        let reference = matches!(ty, Ty::Reference { .. });
        let holder = self.may_hold_borrow(&ty);
        self.flow()
            .mention(name, declaration, holder, reference, span);
    }

    /// What the borrow facts need of a call's argument.
    pub(super) fn argument_fact(&self, argument: &ParsedExpr) -> record::Argument {
        let ty = self.inference.resolve(&argument.ty);
        record::Argument {
            span: argument.span,
            holds: self.may_hold_borrow(&ty),
            reference: is_reference(&ty),
        }
    }

    /// `value` as a method call's receiver; `from_call` when it is another
    /// call's value.
    pub(super) fn call_receiver(&self, value: &ParsedExpr, from_call: bool) -> CallReceiver {
        CallReceiver {
            start: value.span.start(),
            span: value.span,
            ty: self.inference.resolve(&value.ty),
            place: !from_call && (value.lvalue.is_some() || self.flow().is_place(value.span)),
        }
    }

    /// How many lifetime parameters the crate type `target` declares.
    fn declared_lifetimes(&self, target: &type_ref::Target) -> Option<usize> {
        let type_ref::Target::Script { definition, .. } = target else {
            return Some(0);
        };
        Some(match self.models.definition(*definition) {
            Nominal::Struct { item, .. } => item.lifetimes,
            Nominal::Enum { item, .. } => item.lifetimes,
        })
    }

    /// What a call of the crate method `definition` keeps of its inputs.
    pub(super) fn method_summary(&self, definition: &method::Definition) -> Summary {
        let signature = signature::Signature {
            receiver: definition.receiver().map(|mode| match mode {
                receiver::Mode::Shared => signature::Receiver::Shared,
                receiver::Mode::Mutable => signature::Receiver::Mutable,
                receiver::Mode::Owned => signature::Receiver::Owned,
            }),
            parameters: definition
                .parameters()
                .iter()
                .map(|parameter| &parameter.ty)
                .collect(),
            output: definition.output(),
            lifetimes: definition.lifetimes(),
        };
        signature::summary(&signature, &|target| self.declared_lifetimes(target))
    }

    /// The method's own signature, checked as rustc checks it before its
    /// body ([`Parser::check_signature`]).
    pub(super) fn check_method_signature(
        &self,
        definition: &method::Definition,
    ) -> Result<(), Error> {
        if definition.constant() {
            return Ok(());
        }
        let receiver = definition.receiver().map(|mode| match mode {
            receiver::Mode::Shared => signature::Receiver::Shared,
            receiver::Mode::Mutable => signature::Receiver::Mutable,
            receiver::Mode::Owned => signature::Receiver::Owned,
        });
        self.check_signature(
            &signature::Signature {
                receiver,
                parameters: definition
                    .parameters()
                    .iter()
                    .map(|parameter| &parameter.ty)
                    .collect(),
                output: definition.output(),
                lifetimes: definition.lifetimes(),
            },
            receiver.and(definition.receiver_span()),
            definition.parameters(),
        )
    }

    /// The function's own signature, checked as rustc checks it before its
    /// body ([`Parser::check_signature`]).
    pub(super) fn check_function_signature(&self, function: &item::Function) -> Result<(), Error> {
        // A constant's type is `'static` where it leaves a lifetime out.
        let (Some(output), false) = (&function.output, function.body.constant) else {
            return Ok(());
        };
        self.check_signature(
            &signature::Signature {
                receiver: None,
                parameters: function
                    .parameters
                    .iter()
                    .map(|parameter| &parameter.ty)
                    .collect(),
                output,
                lifetimes: &function.lifetimes,
            },
            None,
            &function.parameters,
        )
    }

    /// A lifetime of the output that elision gives no input lifetime to is
    /// rustc's E0106. What the body returns is checked against the rest
    /// (`borrowck::check`): `self`, declared at `receiver`, and the
    /// `parameters`, and the output.
    fn check_signature(
        &self,
        signature: &signature::Signature<'_>,
        receiver: Option<Span>,
        parameters: &[item::Parameter],
    ) -> Result<(), Error> {
        let declared = |target: &type_ref::Target| self.declared_lifetimes(target);
        if let Some(at) = signature::missing_lifetime(signature, &declared) {
            return Err(Error::BorrowConflict {
                code: "E0106",
                message: "missing lifetime specifier".into(),
                access: at,
                borrow: Span::NONE,
            });
        }
        let own = signature::own(signature, &declared);
        // A parameter a pattern destructures is not one local.
        let named = parameters
            .iter()
            .zip(own.parameters)
            .filter(|(parameter, _)| parameter.pattern.is_none())
            .map(|(parameter, borrows)| {
                (
                    parameter.name.text.clone(),
                    parameter.name.span,
                    borrows,
                    false,
                )
            });
        let parameters: Vec<_> = receiver
            .map(|span| (ImmutableString::from("this"), span, own.receiver, true))
            .into_iter()
            .chain(named)
            .collect();
        self.flow()
            .own_signature(&parameters, own.output, &signature.lifetimes.bounds);
        Ok(())
    }

    /// What a call of the crate function `function` keeps of its inputs.
    pub(super) fn function_summary(&self, function: &item::Function) -> Summary {
        let unit = type_ref::Type::Unit(function.body.span);
        let signature = signature::Signature {
            receiver: None,
            parameters: function
                .parameters
                .iter()
                .map(|parameter| &parameter.ty)
                .collect(),
            output: function.output.as_ref().unwrap_or(&unit),
            lifetimes: &function.lifetimes,
        };
        signature::summary(&signature, &|target| self.declared_lifetimes(target))
    }

    /// What a call of the facade or imported item `item` keeps of its
    /// inputs: the importer's lifetimes (D572.4). It admits a call whose
    /// value borrows only as a borrow guard, a lent `&str`, or a scope frame
    /// (D564); any other value it gives owns what it holds.
    pub(super) fn host_summary(
        &self,
        item: &crate::host::item::Definition,
        output: &Ty,
        arguments: usize,
    ) -> Summary {
        let mode = item.receiver().map(|mode| match mode {
            receiver::Mode::Shared => signature::Receiver::Shared,
            receiver::Mode::Mutable => signature::Receiver::Mutable,
            receiver::Mode::Owned => signature::Receiver::Owned,
        });
        if item.imported_identity().is_none() {
            return Summary::unknown(arguments);
        }
        // `Parser::new(&text)`'s value borrows its `&str` inputs; a method
        // whose frame holds what it takes may borrow its receiver or them.
        if item.opens() {
            return match mode {
                None => Summary::builtin(
                    signature::Receiver::Owned,
                    Keeps::Nothing,
                    &vec![Keeps::All; arguments],
                    &[],
                    arguments,
                ),
                Some(_) => Summary::unknown(arguments),
            };
        }
        let lends = item.guard().is_some() || *output == Ty::Str;
        match (mode, lends) {
            (Some(mode @ (signature::Receiver::Shared | signature::Receiver::Mutable)), true) => {
                Summary::builtin(mode, Keeps::All, &[], &[], arguments)
            }
            (_, true) => Summary::unknown(arguments),
            (mode, false) => Summary::nothing(mode, arguments),
        }
    }

    /// The function returns the value spanning `value`, of type `ty`: its
    /// borrows flow into the output's region, every point of the body. A
    /// closure's return is its own: what a call of it keeps.
    pub(super) fn returned(&self, value: Span, ty: &Ty) {
        if !self.may_hold_borrow(ty) {
            return;
        }
        let origins = self.flow().origins(value).map(<[Origin]>::to_vec);
        if !self.closures.is_empty() {
            self.flow().closure_return(origins);
            return;
        }
        match origins {
            Some(origins) => self.flow().give_back(&origins, value),
            None => self.flow().unmodeled(
                "a borrow a function returns that these facts do not follow",
                value,
            ),
        }
    }

    /// `&value` (spanning `whole`) of a value that is no place: a borrow of
    /// a temporary its temporary scope holds, which a `let` may extend, or
    /// of a constant promoted to `'static` (`&1`, `&[1, 2]`), which borrows
    /// nothing. A value that reads no local and calls nothing is taken for
    /// a constant.
    pub(super) fn borrow_temporary(&self, value: &ParsedExpr, whole: Span) {
        let ty = self.inference.resolve(&value.ty);
        let holds = self.may_hold_borrow(&ty);
        let held = if holds {
            self.flow().origins(value.span).map(<[Origin]>::to_vec)
        } else {
            Some(Vec::new())
        };
        // A part of a place (`&v[1..]`), or one read through a reference
        // (`&s.as_bytes()[1..]`), is a place.
        let place = self.flow().in_place(value.span);
        match held {
            Some(held) if !place && self.made_here(value.span, &ty) => {
                let loan = self.flow().borrow_temporary(value.span, false, &held, true);
                self.flow().set_origins(whole, vec![Origin::Loan(loan)]);
            }
            // `&v.first()` of a value these facts do not follow: what the
            // value holds.
            _ if holds => self.flow().same_origins(whole, value.span),
            _ => self.flow().set_origins(whole, Vec::new()),
        }
    }

    /// Whether the value of type `ty` spanning `span` is made here, at run
    /// time, rather than a constant promoted to `'static`: it reads a local
    /// or calls something, or only a call makes a value of its type.
    fn made_here(&self, span: Span, ty: &Ty) -> bool {
        if self.flow().part_of_made(span) {
            return true;
        }
        matches!(
            ty,
            Ty::String | Ty::Array { length: None, .. } | Ty::Keyed(..)
        ) || self.flow().computed(span)
    }

    /// The type of the part `path` below a value of type `ty`: a struct's
    /// field, a tuple's member, or what a reference refers to. `None` for
    /// a path these types don't follow.
    pub(super) fn part_ty(&self, ty: &Ty, path: &[Projection]) -> Option<Ty> {
        let mut ty = self.inference.resolve(ty);
        for step in path {
            ty = match (step, ty) {
                (Projection::Deref, Ty::Reference { target, .. }) => *target,
                (Projection::Field(member), Ty::Tuple(members)) => {
                    members.get(member.parse::<usize>().ok()?)?.clone()
                }
                (Projection::Field(member), Ty::Script(id, arguments)) => {
                    let Nominal::Struct { item, .. } = self.models.definition(id) else {
                        return None;
                    };
                    let field = item
                        .fields
                        .iter()
                        .find(|field| field.name.text == *member)?;
                    let substitution = self.nominal_substitution(id, &arguments);
                    instantiate(&Ty::from_resolved(&field.ty), &substitution)
                }
                _ => return None,
            };
            ty = self.inference.resolve(&ty);
        }
        Some(ty)
    }

    /// Whether the part `path` below a value of type `ty` is `Copy`, when
    /// its type is known.
    pub(super) fn part_copy(&self, ty: &Ty, path: &[Projection]) -> Option<bool> {
        let ty = self.part_ty(ty, path)?;
        if matches!(ty, Ty::Infer(_) | Ty::Unknown) {
            return None;
        }
        Some(!self.moves_by_value(&ty))
    }

    /// The value spanning `whole`, of type `ty`, is a part of the one
    /// spanning `base` (a field, a member, an element): below no reference,
    /// it holds what that value holds. A part read through a reference
    /// holds that reference's inner borrows, which these facts can't tell
    /// apart from its own loan yet.
    pub(super) fn part_of(&self, base: (Span, &Ty), whole: Span, ty: &Ty) {
        let base_ty = self.inference.resolve(base.1);
        let reference = is_reference(&base_ty);
        let made = self.made_here(base.0, &base_ty);
        self.flow().projected(whole, base.0, reference, made);
        if self.may_hold_borrow(ty) && !reference {
            self.flow().same_origins(whole, base.0);
        }
    }

    /// A value of type `ty` spanning `whole` is made of `parts` (a struct's
    /// fields, `Some(x)`, a tuple's or an array's members): it holds what
    /// they hold. A type that can hold no borrow of the body holds only
    /// `'static` ones (`Foo { x: &a }` of `x: &'static u32`).
    pub(super) fn compose(&self, whole: Span, ty: &Ty, parts: &[&ParsedExpr]) {
        self.flow().constructed(whole.end());
        let mut origins = Vec::new();
        for part in parts {
            if !self.may_hold_borrow(&part.ty) {
                continue;
            }
            let held = self.flow().origins(part.span).map(<[Origin]>::to_vec);
            match held {
                Some(held) => origins.extend(held),
                None => {
                    self.flow().unmodeled(
                        "a borrow stored in a value these facts do not follow",
                        part.span,
                    );
                    return;
                }
            }
        }
        if self.may_hold_borrow(ty) {
            // `impl Copy for Foo<'static>` asks a copied `Foo` for `'static`
            // borrows, which these facts do not read.
            if let Ty::Script(id, _) = self.inference.resolve(ty)
                && self.models.static_impls.contains(&id)
            {
                self.flow()
                    .unmodeled("a value whose type has an impl for `'static` only", whole);
                return;
            }
            if self.needs_drop(ty, whole).unwrap_or(true) {
                self.flow()
                    .unmodeled("a value with drop glue that holds a borrow", whole);
                return;
            }
            self.flow().set_origins(whole, origins);
        } else if !origins.is_empty() {
            self.flow().forever(&origins);
        }
    }

    /// The summary of the call whose `)` is at `end`, for the receiver's
    /// method call to read once its value is made.
    pub(super) fn note_call(&mut self, end: Position, summary: Summary) {
        self.call_summaries.insert(end, summary);
    }

    /// The method `member` called on `receiver` made `value`: what the value
    /// keeps of the receiver and the arguments, and what a `&mut self` call
    /// stored in its receiver.
    pub(super) fn finish_method_call(
        &mut self,
        receiver: &CallReceiver,
        member: &str,
        value: &ParsedExpr,
    ) {
        let end = value.span.end();
        let arguments = self.flow().arguments(end).map_or(0, |found| found.len());
        let summary = match self.call_summaries.remove(&end) {
            Some(summary) => summary,
            None => builtin_summary(&receiver.ty, member, arguments)
                .unwrap_or_else(|| Summary::unknown(arguments)),
        };
        // A `&mut self` call whose lowering recorded no borrow of its place
        // (`o.take()`): its two-phase borrow, reserved where the receiver
        // was read and activated here, after the arguments.
        if summary.mode == Some(signature::Receiver::Mutable)
            && receiver.place
            && !matches!(receiver.ty, Ty::Reference { mutable: false, .. } | Ty::Str)
        {
            self.flow()
                .mutable_call(receiver.start, span(receiver.start, end));
        }
        let ty = self.inference.resolve(&value.ty);
        self.kept_by_call(value.span, &ty, Some(receiver), &summary);
        // A `&self` call borrows its place over its arguments, whatever its
        // value keeps (`v.get({ v.push(2); 0 })` is E0502).
        if summary.mode == Some(signature::Receiver::Shared)
            && receiver.place
            && receiver.ty != Ty::Str
            && !matches!(receiver.ty, Ty::Reference { mutable: false, .. })
        {
            let below = self.may_hold_borrow(&receiver.ty);
            self.flow()
                .autoref(receiver.start, value.span, false, below);
        }
    }

    /// The call spanning `call`, whose value has type `ty`, keeps what
    /// `summary` says of `receiver` and of its arguments: the value's
    /// borrows, a `&mut self` receiver's stored ones, and those borrowed for
    /// `'static`. What the facts can't follow is left to rustc (D572.5).
    pub(super) fn kept_by_call(
        &self,
        call: Span,
        ty: &Ty,
        receiver: Option<&CallReceiver>,
        summary: &Summary,
    ) {
        let arguments = self.flow().arguments(call.end()).unwrap_or_default();
        // A temporary a call borrows is no `let`'s to extend.
        for span in arguments
            .iter()
            .map(|argument| argument.span)
            .chain(receiver.map(|receiver| receiver.span))
        {
            let origins = self.flow().origins(span).map(<[Origin]>::to_vec);
            if let Some(origins) = origins {
                self.flow().pin(&origins);
            }
        }
        let holds = self.may_hold_borrow(ty);
        // An argument that may hold borrows these facts don't follow may be
        // kept in ways they can't see.
        let mut known = !arguments
            .iter()
            .any(|argument| argument.holds && self.flow().origins(argument.span).is_none());
        // A value with drop glue uses its borrows where it drops, which
        // these facts don't follow yet.
        if holds && self.needs_drop(ty, call).unwrap_or(true) {
            known = false;
        }
        // An argument's borrows that `keeps` says are kept, or `None`.
        let argument = |argument: &record::Argument, keeps: Keeps| -> Option<Vec<Origin>> {
            let flow = self.flow();
            let origins = flow.origins(argument.span).map(<[Origin]>::to_vec);
            match keeps {
                Keeps::Nothing => Some(Vec::new()),
                _ if !argument.holds => Some(Vec::new()),
                Keeps::All => origins,
                Keeps::Inner if !argument.reference => origins,
                Keeps::Inner => flow.inner_origins(argument.span),
                Keeps::Unknown => None,
            }
        };
        let mut origins = Vec::new();
        if holds {
            if let Some(receiver) = receiver {
                match self.receiver_kept(receiver, summary, call) {
                    Some(kept) => origins.extend(kept),
                    None => known = false,
                }
            }
            for (index, found) in arguments.iter().enumerate() {
                let keeps = summary
                    .parameters
                    .get(index)
                    .copied()
                    .unwrap_or(Keeps::Unknown);
                match argument(found, keeps) {
                    Some(kept) => origins.extend(kept),
                    None => known = false,
                }
            }
        }
        for (index, found) in arguments.iter().enumerate() {
            let stores = summary.stores.get(index).copied().unwrap_or(Keeps::Nothing);
            if stores != Keeps::Nothing {
                match (argument(found, stores), receiver) {
                    (Some(stored), _) if stored.is_empty() => {}
                    (Some(stored), Some(receiver)) if receiver.place => {
                        known &= self.flow().store(receiver.start, &stored);
                    }
                    _ => known = false,
                }
            }
            let statics = summary
                .statics
                .get(index)
                .copied()
                .unwrap_or(Keeps::Nothing);
            if statics != Keeps::Nothing {
                match argument(found, statics) {
                    Some(forever) => self.flow().forever(&forever),
                    None => known = false,
                }
            }
            // A receiver that can hold no borrow stores none (`s.push_str`).
            if summary.unknown_stores
                && found.holds
                && receiver.is_none_or(|receiver| self.may_hold_borrow(&receiver.ty))
            {
                known = false;
            }
        }
        if !known {
            self.flow()
                .unmodeled("a borrow a call keeps that these facts do not follow", call);
        } else if holds {
            self.flow().set_origins(call, origins);
        }
    }

    /// What a call's value keeps of its receiver, as `summary` says, or
    /// `None`.
    fn receiver_kept(
        &self,
        receiver: &CallReceiver,
        summary: &Summary,
        call: Span,
    ) -> Option<Vec<Origin>> {
        let holds = self.may_hold_borrow(&receiver.ty);
        let reference = is_reference(&receiver.ty);
        let value = || self.flow().origins(receiver.span).map(<[Origin]>::to_vec);
        let held = || -> Option<Vec<Origin>> {
            if !holds {
                return Some(Vec::new());
            }
            let held = self.flow().held(receiver.start);
            match held {
                Some(origin) => Some(vec![origin]),
                None => value(),
            }
        };
        let mode = summary.mode.unwrap_or(signature::Receiver::Owned);
        match (summary.receiver, mode) {
            (Keeps::Nothing, _) => Some(Vec::new()),
            (Keeps::Unknown, _) => None,
            // By value: what the receiver holds, as it is.
            (_, signature::Receiver::Owned) if !holds => Some(Vec::new()),
            (_, signature::Receiver::Owned) if receiver.place => held(),
            (_, signature::Receiver::Owned) => value(),
            // `s.trim()` of a `&str`: the reference itself, reborrowed.
            (_, _) if receiver.ty == Ty::Str => {
                if receiver.place {
                    held()
                } else {
                    value()
                }
            }
            (Keeps::All, mode) if receiver.place => {
                let mutable = mode == signature::Receiver::Mutable;
                let below = match &receiver.ty {
                    Ty::Reference { target, .. } => self.may_hold_borrow(target),
                    ty => self.may_hold_borrow(ty),
                };
                let whole = span(receiver.start, call.end());
                self.flow()
                    .autoref(receiver.start, whole, mutable, below)
                    .map(|origin| vec![origin])
            }
            // A reference made here (`v.iter().as_slice()`) is its loans.
            (Keeps::All, _) if reference => value(),
            // A value made here (`String::from("a").as_str()`): a temporary
            // borrowed for the call, which its temporary scope holds.
            (Keeps::All, mode)
                if !self.flow().in_place(receiver.span)
                    && self.made_here(receiver.span, &receiver.ty) =>
            {
                let held = if holds { value()? } else { Vec::new() };
                let mutable = mode == signature::Receiver::Mutable;
                let loan = self
                    .flow()
                    .borrow_temporary(receiver.span, mutable, &held, false);
                Some(vec![Origin::Loan(loan)])
            }
            (Keeps::All, _) => None,
            (Keeps::Inner, _) if receiver.place && !reference => held(),
            (Keeps::Inner, _) if !reference => value(),
            (Keeps::Inner, _) => None,
        }
    }

    /// The place `value` reads one step further down.
    pub(super) fn project_place(&self, value: &ParsedExpr, step: Projection) {
        if value.lvalue.is_some() {
            self.flow().project(value.span.start(), step);
        }
    }

    /// `value[i]`: an element of an array is a place below it; `Index` on a
    /// `Vec`, a `String` or a map borrows all of it.
    pub(super) fn indexed(&self, value: &ParsedExpr) {
        if value.lvalue.is_none() {
            return;
        }
        match self.inference.resolve(&value.ty) {
            Ty::Array { length: None, .. } | Ty::String | Ty::Keyed(..) => {
                self.flow().overloaded_index(value.span.start(), value.span);
            }
            _ => self.project_place(value, Projection::Index),
        }
    }

    /// A method call on the place `value`: a shared borrow of it, unless
    /// the method's mutability check makes it a `&mut` one.
    pub(super) fn call_on_place(&self, value: &ParsedExpr) {
        if value.lvalue.is_some() {
            self.flow().shared(value.span.start());
        }
    }

    /// The place `value` is borrowed `&mut` for a call made here, after its
    /// arguments: a two-phase borrow, reserved where it was read.
    pub(super) fn mutable_use(&self, value: &ParsedExpr, at: Span) {
        if value.lvalue.is_some() {
            self.flow()
                .mutable_call(value.span.start(), span(value.span.start(), at.end()));
        }
    }

    /// The place an expression spanning `at` uses is borrowed `&mut` for a
    /// change made here (a mutability check names the expression's span).
    pub(super) fn mutable_binding_use(&self, at: Span) {
        self.flow().mutable_call(at.start(), at);
    }

    /// `r = ..` of a `&mut` local `r` that is its place at run time, before
    /// the value is read: one given new borrows is a runtime local of its
    /// own (`reborrow.rs`), but not yet one that borrows a field or an
    /// element, or one in an async body, for which a new borrow would write
    /// the place instead.
    pub(super) fn refuse_rebound_alias(&mut self, target: Span, ty: &Ty) -> Result<(), Error> {
        let Some((name, declaration)) = self.flow().mentioned_local(target.start()) else {
            return Ok(());
        };
        let alias = self
            .aliases
            .get(name.as_str())
            .is_some_and(|(aliased, _)| *aliased == declaration);
        // `r = r` keeps the place it is.
        let same = matches!(self.peek(), Token::Identifier(ref other) if other.as_str() == name.as_str())
            && matches!(
                self.peek_second(),
                Token::SemiColon | Token::Comma | Token::RightBrace
            );
        if alias && matches!(self.inference.resolve(ty), Ty::Reference { .. }) && !same {
            return Err(Error::UnsupportedNominalSyntax {
                construct: format!(
                    "giving the `&mut` local `{name}` a new borrow when it borrows a field or \
                     an element, or in an async body: not supported in Rawr yet"
                )
                .into(),
                source: target,
            });
        }
        Ok(())
    }

    /// `let name = value;`, its value begun at `start`: the local takes the
    /// borrows the value is.
    pub(super) fn bound_value(
        &self,
        name: &str,
        declaration: Span,
        value: &ParsedExpr,
        start: ValueStart,
    ) {
        let holds = self.may_hold_borrow(&value.ty);
        // A value with drop glue uses its borrows where it drops, which
        // these facts don't follow yet.
        if holds && self.needs_drop(&value.ty, value.span).unwrap_or(true) {
            self.flow()
                .unmodeled("a value with drop glue that holds a borrow", value.span);
        }
        let mut flow = self.flow();
        let into = flow.local(name, declaration);
        flow.claim(into, value.span, start, holds);
    }

    /// `target = replacement` (or a compound assignment), its value begun
    /// at `start`: a holder given a new borrow (`r = &w`) takes it, and any
    /// other place is written once the value is made.
    pub(super) fn assigned_place(
        &self,
        target: Span,
        target_ty: &Ty,
        lvalue: &Lvalue,
        replacement: &ParsedExpr,
        start: ValueStart,
        compound: bool,
    ) {
        let holds = self.may_hold_borrow(&replacement.ty);
        // A local that holds borrows given a whole new value (`r = &w`,
        // `view = View { value: &b }`) holds only the new value's.
        if !compound
            && let Lvalue::Binding { binding, .. } = lvalue
            && let Some(local) = self.bindings.get(binding)
            && self.may_hold_borrow(&local.ty)
            && self.may_hold_borrow(target_ty)
        {
            let whole = span(target.start(), replacement.span.end());
            let into = self.flow().rebound(target.start(), whole);
            if let Some(into) = into {
                self.flow().claim(into, replacement.span, start, holds);
            }
            return;
        }
        let whole = span(target.start(), replacement.span.end());
        self.flow().assigned(target.start(), whole, compound);
        // `pair.right = &b`: the local takes the borrows besides its own.
        let origins = self
            .flow()
            .origins(replacement.span)
            .map(<[borrowck::Origin]>::to_vec);
        let stored = holds
            && !compound
            && self.may_hold_borrow(target_ty)
            && origins.is_some_and(|origins| self.flow().store(target.start(), &origins));
        // `s += &t` of a `String` makes a new value that holds no borrow.
        let holds = holds && self.may_hold_borrow(target_ty);
        if !stored {
            self.flow().escapes(replacement.span, start, holds);
        }
    }

    /// Whether a value of type `ty` may hold a borrow: its loans then flow
    /// into what holds it.
    pub(super) fn may_hold_borrow(&self, ty: &Ty) -> bool {
        match self.inference.resolve(ty) {
            Ty::Never
            | Ty::Unit
            | Ty::Bool
            | Ty::Char
            | Ty::Int(_)
            | Ty::Float(_)
            | Ty::String
            | Ty::InclusiveRange
            | Ty::FloatRange
            | Ty::Ordering
            | Ty::Time(_)
            | Ty::ParseError(_) => false,
            Ty::Array { element, .. } | Ty::Option(element) | Ty::Range(element) => {
                self.may_hold_borrow(&element)
            }
            Ty::Tuple(members) => members.iter().any(|member| self.may_hold_borrow(member)),
            Ty::Result(ok, error) => self.may_hold_borrow(&ok) || self.may_hold_borrow(&error),
            Ty::Keyed(_, arguments) => arguments
                .iter()
                .any(|argument| self.may_hold_borrow(argument)),
            // A crate type holds a borrow only through a lifetime it declares
            // or a type argument.
            Ty::Script(id, arguments) => {
                let lifetimes = match self.models.definition(id) {
                    Nominal::Struct { item, .. } => item.lifetimes,
                    Nominal::Enum { item, .. } => item.lifetimes,
                };
                lifetimes > 0
                    || arguments
                        .iter()
                        .any(|argument| self.may_hold_borrow(argument))
            }
            // A crate function holds nothing; a closure lifted to an item
            // holds what it captured.
            Ty::FnItem { key, .. } => !self.models.functions_by_key.contains_key(&*key),
            _ => true,
        }
    }

    /// The body's borrows, checked once it is read: the first conflict, as
    /// rustc reports it.
    pub(super) fn check_borrows(&self) -> Result<(), Error> {
        let body = std::mem::take(&mut *self.flow()).finish();
        #[cfg(test)]
        borrowck::note_body(&body);
        match borrowck::check(&body, &borrowck::nll::Nll) {
            None => Ok(()),
            Some(conflict) => Err(Error::BorrowConflict {
                code: conflict.code,
                message: conflict.message.into(),
                access: conflict.access,
                borrow: conflict.borrow,
            }),
        }
    }
}

/// What std's method `member` on a builtin value of type `receiver` keeps of
/// its receiver and arguments, as its signature says, or `None` for one not
/// listed. A method whose value holds no borrow needs no entry unless it
/// stores its arguments.
fn builtin_summary(receiver: &Ty, member: &str, arguments: usize) -> Option<Summary> {
    use signature::Receiver::{Mutable, Owned, Shared};
    use Keeps::{All, Inner, Nothing};
    let mut ty = receiver;
    while let Ty::Reference { target, .. } = ty {
        ty = target;
    }
    let entry = |mode, receiver, parameters: &[Keeps], stores: &[Keeps]| {
        Some(Summary::builtin(
            mode, receiver, parameters, stores, arguments,
        ))
    };
    match ty {
        Ty::Array { .. } => match member {
            "first" | "last" | "get" | "iter" | "as_slice" | "split_first" | "split_last"
            | "split_at" | "windows" | "chunks" | "chunks_exact" | "rchunks" | "front" | "back"
            | "as_ref" | "range" => entry(Shared, All, &[], &[]),
            "get_mut" | "first_mut" | "last_mut" | "iter_mut" | "as_mut_slice" | "split_at_mut"
            | "chunks_mut" | "drain" | "front_mut" | "back_mut" | "split_first_mut"
            | "split_last_mut" => entry(Mutable, All, &[], &[]),
            "to_vec" | "clone" | "concat" | "to_owned" => entry(Shared, Inner, &[], &[]),
            "pop" | "remove" | "swap_remove" | "pop_front" | "pop_back" | "split_off" => {
                entry(Mutable, Inner, &[], &[])
            }
            "into_iter" => entry(Owned, All, &[], &[]),
            "push" | "push_back" | "push_front" | "extend" | "fill" => {
                entry(Mutable, Nothing, &[], &[All])
            }
            "insert" | "resize" => entry(Mutable, Nothing, &[], &[Nothing, All]),
            "append" | "extend_from_slice" => entry(Mutable, Nothing, &[], &[Inner]),
            _ => None,
        },
        Ty::String | Ty::Str => match member {
            "as_str"
            | "as_bytes"
            | "chars"
            | "bytes"
            | "char_indices"
            | "lines"
            | "split_whitespace"
            | "split_ascii_whitespace"
            | "trim"
            | "trim_start"
            | "trim_end"
            | "trim_matches"
            | "trim_start_matches"
            | "trim_end_matches"
            | "strip_prefix"
            | "strip_suffix"
            | "split_once"
            | "rsplit_once"
            | "get"
            | "as_ref"
            | "split_at" => entry(Shared, All, &[], &[]),
            // `Split<'a, P>` keeps its pattern.
            "split" | "rsplit" | "split_terminator" | "split_inclusive" | "matches"
            | "match_indices" => entry(Shared, All, &[All], &[]),
            "splitn" | "rsplitn" => entry(Shared, All, &[Nothing, All], &[]),
            _ => None,
        },
        Ty::Keyed(..) => match member {
            "get" | "get_key_value" | "iter" | "keys" | "values" | "first_key_value"
            | "last_key_value" | "range" | "first" | "last" => entry(Shared, All, &[], &[]),
            "union" | "intersection" | "difference" | "symmetric_difference" => {
                entry(Shared, All, &[All], &[])
            }
            "get_mut" | "iter_mut" | "values_mut" => entry(Mutable, All, &[], &[]),
            "remove" | "remove_entry" | "pop_first" | "pop_last" | "take" => {
                entry(Mutable, Inner, &[], &[])
            }
            "insert" => entry(Mutable, Inner, &[], &[All, All]),
            "extend" | "push" => entry(Mutable, Nothing, &[], &[All]),
            "clone" => entry(Shared, Inner, &[], &[]),
            "into_iter" | "into_keys" | "into_values" => entry(Owned, All, &[], &[]),
            _ => None,
        },
        // A value's borrows are one set in these facts, so a method whose
        // value is part of an item (`copied` of `Option<&T>`) or a closure's
        // result can't be followed when that value holds a borrow.
        Ty::Option(_) | Ty::Result(..) => match member {
            "unwrap" | "expect" | "unwrap_or_default" | "flatten" | "ok" | "err" | "unwrap_err"
            | "expect_err" | "filter" | "inspect" => entry(Owned, All, &[], &[]),
            "unwrap_or" | "ok_or" | "or" | "xor" | "zip" => entry(Owned, All, &[All], &[]),
            "and" => entry(Owned, Nothing, &[All], &[]),
            "as_ref" | "as_deref" | "iter" => entry(Shared, All, &[], &[]),
            "as_mut" | "as_deref_mut" | "iter_mut" => entry(Mutable, All, &[], &[]),
            "get_or_insert" | "insert" => entry(Mutable, All, &[], &[All]),
            // What it stores is its closure's value, not the closure.
            "get_or_insert_with" => entry(Mutable, All, &[], &[Keeps::Unknown]),
            "take" => entry(Mutable, Inner, &[], &[]),
            "replace" => entry(Mutable, Inner, &[], &[All]),
            _ => None,
        },
        // An adapter holds the iterator it takes, and its closure; a method
        // whose value is items is not listed.
        Ty::Iter(_) => match member {
            "enumerate" | "rev" | "skip" | "take" | "step_by" | "cloned" | "copied"
            | "peekable" | "flatten" | "fuse" | "cycle" | "into_iter" => {
                entry(Owned, All, &[], &[])
            }
            "map" | "filter" | "filter_map" | "flat_map" | "zip" | "chain" | "take_while"
            | "skip_while" | "map_while" | "inspect" => entry(Owned, All, &[All], &[]),
            "scan" => entry(Owned, All, &[All, All], &[]),
            "by_ref" => entry(Mutable, All, &[], &[]),
            _ => None,
        },
        _ => None,
    }
}
