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
    region::{self, Held},
    signature::{self, Input, Source, Summary, Tap},
    Access, Origin, Path, Place, Projection, Step,
};
use crate::resolve::outlives;
use std::cell::RefCell;

/// Where a part is in the value it is read from (T127).
#[derive(Clone)]
pub(super) enum PartOf {
    /// An element of a `Vec`, an array or a slice, or a map's value
    /// (`m[&k]`).
    Element,
    /// A tuple's member.
    Member(usize),
    /// A crate struct's field, by its slot.
    Field(type_def::Id, usize),
    /// What a reference or a box holds (`*r`).
    Deref,
}

/// Where a part goes in a value made of parts (T127).
#[derive(Clone)]
pub(super) enum Slot<'a> {
    /// At a position: a tuple's member, an array's elements, `Some`'s
    /// payload.
    At(Path),
    /// A crate type's field by its slot, or a variant's payload by its
    /// position.
    Field(type_def::Id, Option<&'a str>, usize),
    /// The whole value, of the same type (`..base`).
    Whole,
}

/// The path of `steps`.
fn path(steps: &[Step]) -> Path {
    Path::of(steps)
}

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
    /// For each step of `path`, where it goes in what the value holds.
    regions: Vec<TraceRegion>,
    parts: Vec<Part>,
    /// The `ref` or `ref mut` binding being resolved: its mutability.
    explicit: Option<bool>,
    unmodeled: Option<(&'static str, Span)>,
}

struct Part {
    path: Vec<Projection>,
    regions: Vec<TraceRegion>,
    kind: PartKind,
}

/// Where a pattern's step goes in what the value it steps into holds
/// (T127).
#[derive(Clone)]
pub(super) enum TraceRegion {
    /// To a position: a tuple's member, a reference's target, a payload.
    At(Path),
    /// To a crate type's field by its slot, of a variant for an enum.
    Field(type_def::Id, Option<ImmutableString>, usize),
    /// Somewhere these facts don't place: all of the value.
    Unknown,
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

/// The subslice a slice pattern's `rest @ ..` binds, between its `before`
/// and `after` elements, of an array of `length` or of a slice, as rustc
/// places it.
pub(super) fn slice_middle(before: usize, after: usize, length: Option<usize>) -> Projection {
    let (to, from_end) = match length {
        Some(length) => (length - after, false),
        None => (after, true),
    };
    Projection::Subslice {
        from: before as u32,
        to: to as u32,
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
        let region = match step {
            Projection::Deref | Projection::Index | Projection::ConstantIndex { .. } => {
                TraceRegion::At(path(&[Step::Arg(0)]))
            }
            Projection::Subslice { .. } => TraceRegion::At(Path::ROOT),
            Projection::Field(_) | Projection::Downcast(_) => TraceRegion::Unknown,
        };
        self.trace_enter_at(step, region)
    }

    /// [`Parser::trace_enter`], with where the step goes in what the value
    /// holds.
    pub(super) fn trace_enter_at(&self, step: Projection, region: TraceRegion) -> usize {
        let mut trace = self.pattern_trace.borrow_mut();
        let Some(trace) = trace.as_mut() else {
            return 0;
        };
        trace.path.push(step);
        trace.regions.push(region);
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
            trace.regions.truncate(length);
        }
    }

    /// The pattern being resolved tests its part, where the subject is or
    /// at `at`.
    pub(super) fn trace_test(&self, access: Access, at: Option<Span>) {
        if let Some(trace) = self.pattern_trace.borrow_mut().as_mut() {
            let path = trace.path.clone();
            let regions = trace.regions.clone();
            trace.parts.push(Part {
                path,
                regions,
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
            let regions = trace.regions.clone();
            trace.parts.push(Part {
                path,
                regions,
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
        // A local is the place its use names. The binding a scope frame's
        // call is read through (`scoped_call`) is the call's value, which
        // starts where the call's receiver does but is no use of it.
        let named = match &subject.lvalue {
            Some(Lvalue::Binding { binding, .. }) => self
                .flow()
                .mentioned_local(subject.span.start())
                .is_some_and(|(mentioned, _)| mentioned == *binding),
            lvalue => lvalue.is_some(),
        };
        let place = named || self.flow().is_place(subject.span);
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
    ) -> Result<(), Error> {
        self.pattern_tests(&trace, matched, subject);
        self.pattern_binds(trace, matched, subject, declarations)
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
    /// the reference it is below. A by-value binding can't move its part
    /// out from behind a reference (rustc's E0507), nor out of a value whose
    /// type implements `Drop` (E0509), which rustc reports at the subject
    /// spanning `subject`.
    pub(super) fn pattern_binds(
        &self,
        trace: PatternTrace,
        matched: &Matched,
        subject: Span,
        declarations: &[PatternBinding],
    ) -> Result<(), Error> {
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
                    Some((part.path, part.regions, name, span, by_ref, holds, moves))
                }
                PartKind::Test(..) => None,
            })
            .collect();
        let behind = matches!(matched, Matched::Place(base)
            if base.projection.contains(&Projection::Deref));
        for (path, regions, _, _, by_ref, _, moves) in &binds {
            if by_ref.is_some() || !moves {
                continue;
            }
            let dropped = regions.iter().find_map(|region| match region {
                TraceRegion::Field(owner, ..) if self.models.has_drop(*owner) => Some(*owner),
                _ => None,
            });
            if let Some(owner) = dropped {
                self.reject_drop_move(owner, true, subject)?;
            }
            if behind || path.contains(&Projection::Deref) {
                return Err(Error::BorrowConflict {
                    code: "E0507",
                    message: "cannot move out of a value behind a reference".into(),
                    access: subject,
                    borrow: subject,
                });
            }
        }
        let mut flow = self.flow();
        if let Some((what, span)) = trace.unmodeled {
            flow.unmodeled(what, span);
        }
        // An or-pattern's alternatives bind one local: it is given its
        // value once, and takes each alternative's loans.
        let mut defined = std::collections::BTreeSet::new();
        for (path, regions, name, span, by_ref, holds, moves) in binds {
            let declaration = declarations
                .iter()
                .find(|binding| binding.name == name)
                .map_or(span, |binding| binding.span);
            let into = flow.local(&name, declaration);
            let define = defined.insert(into);
            match (matched, by_ref) {
                (Matched::Place(base), Some(mutable)) => {
                    flow.part_borrow(below(base, &path), mutable, holds, span, into, define);
                }
                (Matched::Place(base), None) => {
                    let access = if moves { Access::Move } else { Access::Read };
                    flow.part(below(base, &path), access, span);
                    // A part that holds borrows: what the place holds there.
                    let held = if holds {
                        self.walk(vec![(Path::ROOT, record::place_origin(base))], &regions)
                    } else {
                        Vec::new()
                    };
                    flow.take(into, &held, define);
                }
                // Below a reference made for the match, a binding by
                // reference reborrows through it, and holds all of it.
                (Matched::Origins(origins), Some(_)) => {
                    flow.take(into, &region::collapse(origins), define);
                }
                // `&(x, _)`: a part copied out from below it.
                (Matched::Origins(origins), None) if holds => {
                    flow.take(into, &self.walk(origins.clone(), &regions), define);
                }
                // `Some(v)` of `v.first()`: a part of a value holding
                // borrows holds them, bound by value or by reference.
                (Matched::Held(origins), None) if holds => {
                    flow.take(into, &self.walk(origins.clone(), &regions), define);
                }
                // Each binding is given its value here, and has it again
                // each time the pattern binds it, as in a loop.
                (Matched::Origins(_), None) | (Matched::Held(_), None) => {
                    flow.take(into, &[], define);
                }
                (Matched::Held(origins), Some(_)) => {
                    let part = self.walk(origins.clone(), &regions);
                    flow.take(into, &region::collapse(&part), define);
                }
                (Matched::Value { made }, by_ref) => {
                    if *made && (by_ref.is_some() || holds) {
                        flow.unmodeled("a pattern binding a borrow its subject made", span);
                    }
                    flow.take(into, &[], define);
                }
            }
        }
        Ok(())
    }

    /// What a part `regions` below a value holding `held` holds.
    fn walk(&self, mut held: Held, regions: &[TraceRegion]) -> Held {
        for step in regions {
            held = match step {
                TraceRegion::At(at) => region::sub(&held, *at),
                TraceRegion::Field(definition, variant, slot) => {
                    self.field_held(&held, *definition, variant.as_deref(), *slot)
                }
                TraceRegion::Unknown => region::collapse(&held),
            };
        }
        held
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
            regions: Vec::new(),
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
        binding: Option<(&ImmutableString, Span, super::branch::Bound)>,
        written: &Ty,
    ) -> PatternTrace {
        let mut trace = PatternTrace::default();
        let derefs = self.derefs(written);
        if tests {
            trace.parts.push(Part {
                path: derefs.clone(),
                regions: Vec::new(),
                kind: PartKind::Test(Access::Read, None),
            });
        }
        match binding {
            // Below a `&mut`, rawr binds the value it refers to, which Rust
            // binds as the reference: the loans it keeps are left to rustc.
            Some((_, span, _)) if !derefs.is_empty() && !self.shared_references(written) => {
                trace.unmodeled = Some(("a binding of a matched reference's value", span));
            }
            // `ref v` and `ref mut v` borrow the subject as written.
            Some((
                name,
                span,
                bound @ (super::branch::Bound::Shared | super::branch::Bound::Unique),
            )) => {
                let mutable = bound == super::branch::Bound::Unique;
                trace.parts.push(Part {
                    path: Vec::new(),
                    regions: Vec::new(),
                    kind: PartKind::Bind {
                        name: name.clone(),
                        span,
                        by_ref: Some(mutable),
                        ty: Ty::Reference {
                            mutable,
                            target: Box::new(written.clone()),
                        },
                    },
                });
            }
            // The binding is the subject as written, `&T` included, and
            // holds its loans.
            Some((name, span, _)) => trace.parts.push(Part {
                path: Vec::new(),
                regions: Vec::new(),
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
        compiled: &pattern::Host,
        written: &Ty,
        span: Span,
    ) -> PatternTrace {
        let mut trace = PatternTrace::default();
        if !compiled.tests.is_empty() {
            trace.parts.push(Part {
                path: self.derefs(written),
                regions: Vec::new(),
                kind: PartKind::Test(Access::Read, None),
            });
        }
        if !compiled.binds.is_empty() {
            trace.unmodeled = Some(("a pattern binding a part of a host value", span));
        }
        trace
    }

    /// Each local a facade pattern binds is given its value where the
    /// pattern binds it, and again each time it does, as in a loop's body:
    /// a move of one is no use of the value an earlier iteration moved.
    /// What the value borrows is left to rustc, with the rest of the
    /// pattern (`host_trace`).
    pub(super) fn define_host_binds(&self, binds: &[PatternBinding]) {
        let mut flow = self.flow();
        for binding in binds {
            let local = flow.local(&binding.name, binding.span);
            flow.take(local, &[], true);
        }
    }

    /// Whether `ty` is a reference, and each reference on the way to the
    /// value it refers to is shared: `&i64`, `&&String`.
    pub(super) fn shared_references(&self, ty: &Ty) -> bool {
        let mut ty = self.inference.resolve(ty);
        let mut reference = false;
        while let Ty::Reference { mutable, target } = ty {
            if mutable {
                return false;
            }
            reference = true;
            ty = self.inference.resolve(&target);
        }
        reference
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
        let returns = match &ty {
            Ty::Fn { output, .. } | Ty::FnItem { output, .. } | Ty::FnChoice { output, .. } => {
                self.may_hold_borrow(output)
            }
            _ => true,
        };
        record::Argument {
            span: argument.span,
            holds: self.may_hold_borrow(&ty),
            returns,
        }
    }

    /// `value` as a method call's receiver; `from_call` when it is another
    /// call's value.
    pub(super) fn call_receiver(&self, value: &ParsedExpr, from_call: bool) -> CallReceiver {
        CallReceiver {
            start: value.span.start(),
            span: value.span,
            ty: self.inference.resolve(&value.ty),
            place: !from_call && (is_place(value) || self.flow().is_place(value.span)),
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
            definition.name_span(),
        )
    }

    /// The function's own signature, checked as rustc checks it before its
    /// body ([`Parser::check_signature`]).
    pub(super) fn check_function_signature(&self, function: &item::Function) -> Result<(), Error> {
        // A constant's type is `'static` where it leaves a lifetime out.
        if function.body.constant {
            return Ok(());
        }
        let unit = type_ref::Type::Unit(function.body.span);
        let output = function.output.as_ref().unwrap_or(&unit);
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
            Span::NONE,
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
        name: Span,
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
        let visiting = RefCell::new(Vec::new());
        let variance = |target: &type_ref::Target| self.type_fields(target, &visiting);
        let own = signature::own(signature, &declared, &variance);
        // A parameter a pattern destructures is not one local.
        let named = parameters
            .iter()
            .zip(own.parameters)
            .filter(|(parameter, _)| parameter.pattern.is_none())
            .map(|(parameter, regions)| {
                (
                    parameter.name.text.clone(),
                    parameter.name.span,
                    regions,
                    false,
                )
            });
        let parameters: Vec<_> = receiver
            .map(|span| (ImmutableString::from("self"), span, own.receiver, true))
            .into_iter()
            .chain(named)
            .collect();
        let item = if receiver.is_some() {
            outlives::Item::Method
        } else if signature.lifetimes.owner.is_some() {
            outlives::Item::AssociatedFunction
        } else {
            outlives::Item::Function
        };
        // The receiver's elided lifetime is at no place of its type.
        let elided = own
            .elided
            .iter()
            .map(|elision| {
                let at = if elision.at.is_none() {
                    receiver.unwrap_or(Span::NONE)
                } else {
                    elision.at
                };
                let mut end = at.start();
                if !end.is_none() {
                    end.advance();
                }
                outlives::Reference {
                    ampersand: Span::new(at.start(), end),
                    mutable: elision.mutable,
                    receiver: elision.at.is_none(),
                }
            })
            .collect();
        let variances = own
            .output
            .invariances()
            .chain(
                parameters
                    .iter()
                    .flat_map(|(_, _, parameter, _)| parameter.invariances()),
            )
            .filter_map(|invariance| Some((invariance.clone(), self.variance(invariance)?)))
            .collect();
        let names = borrowck::Names {
            item,
            name,
            declared: signature.lifetimes.declared.to_vec(),
            elided,
            variances,
            naming: borrowck::Naming {
                elisions: own.elided,
                input_elided: own.input_elided,
                first: signature.lifetimes.first.clone(),
                introduced: signature.lifetimes.introduced,
                implemented: signature.lifetimes.owner.is_some(),
            },
        };
        self.flow().own_signature(parameters, own.output, names);
        Ok(())
    }

    /// Why a position of a signature's type is invariant, as rustc's notes
    /// say: `None` for a crate type these facts don't name.
    fn variance(&self, invariance: &signature::Invariance) -> Option<outlives::Variance> {
        let argument = |ty: &type_ref::Type, at: usize| match ty {
            type_ref::Type::Path(path) => path
                .arguments
                .iter()
                .filter_map(|argument| match argument {
                    type_ref::Argument::Type(ty) => Some(ty.erased()),
                    type_ref::Argument::Lifetime(_) => None,
                })
                .nth(at),
            _ => None,
        };
        Some(match invariance {
            signature::Invariance::Referent(referent) => outlives::Variance::Referent {
                referent: referent.erased().into(),
            },
            signature::Invariance::Cell(cell) => {
                let type_ref::Type::Path(path) = cell else {
                    return None;
                };
                // rustc names a lock by its path, which another type's
                // name shares.
                let prefix = match path.name.as_str() {
                    "Mutex" | "RwLock" => "std::sync::",
                    _ => "",
                };
                outlives::Variance::Argument {
                    ty: format!("{prefix}{}", cell.erased()).into(),
                    argument: argument(cell, 0)?.into(),
                    kind: "struct",
                    declared: format!("{prefix}{}<T>", path.name).into(),
                    parameter: "T".into(),
                }
            }
            signature::Invariance::Nominal(nominal, below) => {
                let type_ref::Type::Path(type_ref::Path {
                    target: type_ref::Target::Script { definition, .. },
                    ..
                }) = nominal
                else {
                    return None;
                };
                let (kind, name, regions, generics) = match self.models.definition(*definition) {
                    Nominal::Struct { item, .. } => {
                        ("struct", &item.name, &item.regions, &item.generics)
                    }
                    Nominal::Enum { item, .. } => {
                        ("enum", &item.name, &item.regions, &item.generics)
                    }
                };
                let parameters: Vec<String> = regions
                    .params
                    .iter()
                    .map(|lifetime| format!("'{lifetime}"))
                    .chain(generics.iter().map(|generic| generic.name.text.to_string()))
                    .collect();
                let (argument, parameter) = match below.steps().next() {
                    Some(Step::Lifetime(index)) => (
                        "'_".to_string(),
                        format!("'{}", regions.params.get(usize::from(index))?),
                    ),
                    Some(Step::Arg(index)) => (
                        argument(nominal, usize::from(index))?,
                        generics.get(usize::from(index))?.name.text.to_string(),
                    ),
                    _ => return None,
                };
                outlives::Variance::Argument {
                    ty: nominal.erased().into(),
                    argument: argument.into(),
                    kind,
                    declared: format!("{}<{}>", name.text, parameters.join(", ")).into(),
                    parameter: parameter.into(),
                }
            }
        })
    }

    /// What the fields of the crate type `target` make of its parameters
    /// ([`signature::Fields`]). A type `visiting` holds is one whose fields
    /// name it again, read as covariant there and implying nothing: the
    /// rest of its fields say the rest.
    fn type_fields(
        &self,
        target: &type_ref::Target,
        visiting: &RefCell<Vec<type_def::Id>>,
    ) -> signature::Fields {
        let type_ref::Target::Script { definition, .. } = target else {
            return signature::Fields::default();
        };
        if visiting.borrow().contains(definition) {
            return signature::Fields::default();
        }
        visiting.borrow_mut().push(*definition);
        let (regions, generics, fields): (_, _, Vec<&type_ref::Type>) =
            match self.models.definition(*definition) {
                Nominal::Struct { item, .. } => (
                    &item.regions,
                    &item.generics,
                    item.fields.iter().map(|field| &field.ty).collect(),
                ),
                Nominal::Enum { item, .. } => (
                    &item.regions,
                    &item.generics,
                    item.variants
                        .iter()
                        .flat_map(|variant| &variant.payload)
                        .collect(),
                ),
            };
        let parameters: Vec<ImmutableString> = generics
            .iter()
            .map(|generic| generic.name.text.clone())
            .collect();
        let declared = |target: &type_ref::Target| self.declared_lifetimes(target);
        let variance = |target: &type_ref::Target| self.type_fields(target, visiting);
        let mut varies: Vec<(Path, signature::Varies)> = Vec::new();
        let mut outlives = Vec::new();
        for ty in fields {
            let field = signature::field_variance(ty, regions, &parameters, &declared, &variance);
            for (path, at) in field.varies {
                match varies.iter_mut().find(|(known, _)| *known == path) {
                    Some((_, known)) => *known = known.join(at),
                    None => varies.push((path, at)),
                }
            }
            for bound in field.outlives {
                if !outlives.contains(&bound) {
                    outlives.push(bound);
                }
            }
        }
        // A bound the type declares between its lifetimes (`'b: 'a`) is
        // implied where it is named, as its fields' are.
        let lifetime = |name: &ImmutableString| {
            let index = regions.params.iter().position(|param| param == name)?;
            u8::try_from(index).ok()
        };
        for (longer, shorter) in &regions.bounds {
            if let (Some(longer), Some(shorter)) = (lifetime(longer), lifetime(shorter))
                && !outlives.contains(&(longer, shorter))
            {
                outlives.push((longer, shorter));
            }
        }
        visiting.borrow_mut().pop();
        let with = |kind| {
            varies
                .iter()
                .filter(|(_, at)| *at == kind)
                .map(|(path, _)| *path)
                .collect()
        };
        signature::Fields {
            invariant: with(signature::Varies::Invariant),
            contravariant: with(signature::Varies::Contravariant),
            outlives,
        }
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

    /// What a call of the trait method `function`, as its trait declares
    /// it, keeps of its inputs: the call of a generic parameter's bound
    /// (`t.get()` of `T: Get`) or a trait object's (`b.get()` of `Box<dyn
    /// Get>`), whichever impl it runs.
    pub(super) fn trait_method_summary(&self, function: &item::Function) -> Summary {
        let unit = type_ref::Type::Unit(function.body.span);
        // `self` is of the trait's parameter `Self`.
        let lifetimes = item::Lifetimes {
            owner: Some(Box::new(type_ref::Type::Path(type_ref::Path {
                name: "Self".into(),
                target: type_ref::Target::Parameter,
                arguments: Box::default(),
                span: function.name.span,
            }))),
            ..function.lifetimes.clone()
        };
        let signature = signature::Signature {
            receiver: function.receiver.as_ref().map(|receiver| match receiver {
                crate::item::Receiver::Shared(_) => signature::Receiver::Shared,
                crate::item::Receiver::Mutable(_) => signature::Receiver::Mutable,
                crate::item::Receiver::Owned(_) | crate::item::Receiver::OwnedMutable(_) => {
                    signature::Receiver::Owned
                }
            }),
            parameters: function
                .parameters
                .iter()
                .map(|parameter| &parameter.ty)
                .collect(),
            output: function.output.as_ref().unwrap_or(&unit),
            lifetimes: &lifetimes,
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
        receiver: Option<&Ty>,
        output: &Ty,
        arguments: &[Ty],
    ) -> Summary {
        let mut summary = self.host_keeps(item, receiver, output, arguments.len());
        // A string borrowed where only a `&'static str` goes, as iced's
        // `title(impl Title<State>)` takes one, or a sequence where only a
        // `&'static [T]` goes, as `State::new(blocks: &'static [Block])`
        // takes one, is borrowed for `'static`: a literal or a `static` is,
        // and a borrow of a local is rustc's E0597.
        if let Some(statics) = &mut summary.statics {
            for (index, ty) in arguments.iter().enumerate() {
                let lent = match self.inference.resolve(ty) {
                    Ty::Str => true,
                    Ty::Reference {
                        mutable: false,
                        target,
                    } => matches!(
                        *target,
                        Ty::String
                            | Ty::Str
                            | Ty::Array {
                                kind: sequence::Kind::Array(_)
                                    | sequence::Kind::Slice
                                    | sequence::Kind::Vec,
                                ..
                            }
                    ),
                    _ => false,
                };
                if lent && item.lent_static(index) {
                    statics.push((Input::Argument(index), Path::OWN));
                }
            }
        }
        summary
    }

    fn host_keeps(
        &self,
        item: &crate::host::item::Definition,
        receiver: Option<&Ty>,
        output: &Ty,
        arguments: usize,
    ) -> Summary {
        let mode = item.receiver().map(|mode| match mode {
            receiver::Mode::Shared => signature::Receiver::Shared,
            receiver::Mode::Mutable => signature::Receiver::Mutable,
            receiver::Mode::Owned => signature::Receiver::Owned,
        });
        if item.imported_identity().is_none() {
            return Summary::unknown();
        }
        // All of an input, at every position of the value.
        let all = |input| Tap {
            into: Path::ROOT,
            from: Source::Input(input, Path::OWN),
        };
        // `Parser::new(&text)`'s value borrows its `&str` inputs, and so
        // does a method's whose frame holds what it takes, as jiff's
        // `strftime(&self, format: &'f F)` and quick-xml's
        // `read_event_into(&mut self, buf)` give: it borrows its receiver
        // for the call alone. One whose frame holds its receiver borrowed,
        // as quick-xml's `BytesStart::name(&self) -> QName<'_>` does, keeps
        // that borrow, and the inputs it takes at the receiver's lifetime,
        // as `view<'a>(&'a self, theme: &'a Theme)` takes its theme, for as
        // long as the value lives.
        match item.frame() {
            Some(crate::host::item::Frame::Inputs) => {
                return Summary::taps(
                    mode.or(Some(signature::Receiver::Owned)),
                    (0..arguments)
                        .map(|index| all(Input::Argument(index)))
                        .collect(),
                    Vec::new(),
                );
            }
            Some(crate::host::item::Frame::Receiver) => {
                return match mode {
                    Some(mode @ (signature::Receiver::Shared | signature::Receiver::Mutable)) => {
                        Summary::taps(
                            Some(mode),
                            std::iter::once(all(Input::Receiver))
                                .chain(
                                    (0..arguments)
                                        .filter(|index| item.lends(*index))
                                        .map(|index| all(Input::Argument(index))),
                                )
                                .collect(),
                            Vec::new(),
                        )
                    }
                    _ => Summary::unknown(),
                };
            }
            None => {}
        }
        // `draw<'a>(&'a self, pass: &mut RenderPass<'a>)` keeps its
        // receiver borrowed in what `pass` refers to.
        let receiver_into: Vec<usize> = (0..arguments)
            .filter(|index| item.keeps_receiver(*index))
            .collect();
        if mode == Some(signature::Receiver::Shared) && !receiver_into.is_empty() {
            return Summary {
                receiver_into,
                ..Summary::nothing(mode)
            };
        }
        // `view<'a>(items: &'a [Item]) -> impl Show + 'a`'s value keeps
        // the borrows of the inputs at the lifetime its type names.
        let kept: Vec<Tap> = (0..arguments)
            .filter(|index| item.lends(*index))
            .map(|index| all(Input::Argument(index)))
            .collect();
        if !kept.is_empty() {
            return Summary::taps(mode, kept, Vec::new());
        }
        // `again(&self) -> Self` of an imported `impl Trait` value keeps
        // what the value it is called on keeps, as the `'a` its type
        // captures: not the receiver's borrow, but what it refers to.
        if let Some(signature::Receiver::Shared | signature::Receiver::Mutable) = mode
            && let Some(receiver) = receiver
            && self.imported_opaque(output)
            && without_references(&self.inference.resolve(receiver)) == output
        {
            let referent = Tap {
                into: Path::ROOT,
                from: Source::Input(Input::Receiver, Path::of(&[Step::Arg(0)])),
            };
            return Summary::taps(mode, vec![referent], Vec::new());
        }
        // `view(&self) -> impl Widget<..>`'s value captures the receiver's
        // lifetime, under edition 2024's rules or by `+ '_`: it keeps the
        // receiver borrowed for as long as it lives.
        let lends = item.guard().is_some() || *output == Ty::Str || item.lends_receiver();
        match (mode, lends) {
            (Some(mode @ (signature::Receiver::Shared | signature::Receiver::Mutable)), true) => {
                Summary::taps(Some(mode), vec![all(Input::Receiver)], Vec::new())
            }
            (_, true) => Summary::unknown(),
            (mode, false) => Summary::nothing(mode),
        }
    }

    /// Whether `ty` is an imported `impl Trait` value, which may hold the
    /// borrows of the call that made it (`surface::opaque`).
    fn imported_opaque(&self, ty: &Ty) -> bool {
        #[cfg(feature = "rust-import")]
        if let Ty::Host { proof, .. } = ty {
            return rawr_import::surface::opaque::declared(&proof.definition.path());
        }
        let _ = ty;
        false
    }

    /// The function returns the value spanning `value`, of type `ty`: its
    /// borrows flow into the output's region, every point of the body. A
    /// closure's return is its own: what a call of it keeps.
    pub(super) fn returned(&self, value: Span, ty: &Ty) {
        if !self.may_hold_borrow(ty) {
            return;
        }
        let origins = self.flow().origins(value).map(<[(Path, Origin)]>::to_vec);
        if !self.closures.is_empty() {
            self.flow().closure_return(origins);
            return;
        }
        match origins {
            Some(origins) => {
                self.flow().give_back(&origins, value);
                // A `&mut` place read through a reference is reborrowed.
                let mutable = matches!(
                    self.inference.resolve(ty),
                    Ty::Reference { mutable: true, .. }
                );
                let reborrow = mutable
                    .then(|| self.flow().value_place(value.start()))
                    .flatten();
                if let Some(place) = reborrow
                    && let Some(through) = self.reborrow_regions(&place)
                {
                    self.flow().returned_reborrow(place.local, through);
                }
            }
            None => self.flow().unmodeled(
                "a borrow a function returns that these facts do not follow",
                value,
            ),
        }
    }

    /// `loan`, a reborrow, lasts within the references its place goes
    /// through ([`Parser::reborrow_regions`]).
    pub(super) fn reborrowed(&self, loan: Option<borrowck::LoanId>) {
        let Some(loan) = loan else {
            return;
        };
        let place = self.flow().loan_place(loan).clone();
        if let Some(through) = self.reborrow_regions(&place) {
            self.flow().reborrow_through(loan, through);
        }
    }

    /// The region positions, in the type of its local, of the references a
    /// reborrow of `place` lasts within, as rustc's reborrow constraints
    /// have them: from the innermost reference the place goes through out
    /// to the first shared one. `None` where these facts don't place them.
    fn reborrow_regions(&self, place: &Place) -> Option<Vec<Path>> {
        if !place.projection.contains(&Projection::Deref) {
            return None;
        }
        let mut through = Vec::new();
        for deref in self.walk_place(place)?.derefs.into_iter().rev() {
            through.push(deref.own);
            if !deref.mutable {
                break;
            }
        }
        Some(through)
    }

    /// Where `place` is in the type of its local ([`Walked`]), or `None`
    /// where these facts don't place it.
    fn walk_place(&self, place: &Place) -> Option<Walked> {
        let info = self.flow().local_info(place.local).clone();
        let binding = self
            .bindings
            .get(info.name.as_str())
            .filter(|binding| binding.declaration == info.declaration)?;
        let mut ty = self.inference.resolve(&binding.ty);
        let mut positions = vec![(Path::ROOT, Path::ROOT)];
        let mut derefs = Vec::new();
        for step in &place.projection {
            let (below, next) = match (step, &ty) {
                (Projection::Deref, Ty::Reference { mutable, target }) => {
                    derefs.push(Deref {
                        own: locate(&positions, Path::OWN)?,
                        mutable: *mutable,
                    });
                    (descend(&positions, Step::Arg(0)), (**target).clone())
                }
                (Projection::Deref, Ty::Boxed(target)) => {
                    (descend(&positions, Step::Arg(0)), (**target).clone())
                }
                (Projection::Field(member), Ty::Tuple(members)) => {
                    let index = member.parse::<usize>().ok()?;
                    let member = members.get(index)?.clone();
                    (descend(&positions, Step::Arg(index as u8)), member)
                }
                (Projection::Field(member), Ty::Script(id, _)) => {
                    let Nominal::Struct { item, .. } = self.models.definition(*id) else {
                        return None;
                    };
                    let slot = item
                        .fields
                        .iter()
                        .position(|field| field.name.text == *member)?;
                    let below = self
                        .field_positions(*id, None, slot)?
                        .into_iter()
                        .filter_map(|(field, at)| Some((field, locate(&positions, at?)?)))
                        .collect();
                    (below, self.part_ty(&ty, std::slice::from_ref(step))?)
                }
                _ => return None,
            };
            positions = below;
            ty = self.inference.resolve(&next);
        }
        Some(Walked { positions, derefs })
    }

    /// The place the use starting at `start` reads, below a reference, takes
    /// `held` at the positions of its value, or of the value it is a field
    /// of when `field`, by the call or the assignment spanning `span`: what
    /// a parameter's place takes is checked against the signature.
    fn stored_below(
        &self,
        start: Position,
        held: &[(Path, Origin)],
        field: bool,
        span: Span,
        cause: outlives::Cause,
    ) {
        let Some(mut place) = self.flow().place_at(start) else {
            return;
        };
        if field && matches!(place.projection.last(), Some(Projection::Field(_))) {
            place.projection.pop();
        }
        if !place.projection.contains(&Projection::Deref) {
            return;
        }
        if let Some(walked) = self.walk_place(&place) {
            self.flow()
                .store_below(place.local, walked.positions, held, span, cause);
        }
    }

    /// The value spanning `whole`, of type `ty`, is the `part` of the one
    /// spanning `base` (a field, a member, an element): it holds what that
    /// value holds at the part's positions, through any reference it is
    /// read through.
    pub(super) fn part_of(&self, base: (Span, &Ty), whole: Span, ty: &Ty, part: PartOf) {
        if !matches!(part, PartOf::Deref) {
            let base_ty = self.inference.resolve(base.1);
            let reference = is_reference(&base_ty);
            let made = self.made_here(base.0, &base_ty);
            self.flow().projected(whole, base.0, reference, made);
        }
        if !self.may_hold_borrow(ty) {
            return;
        }
        let Some(held) = self.flow().origins(base.0).map(<[(Path, Origin)]>::to_vec) else {
            return;
        };
        if let PartOf::Deref = part {
            let value = region::sub(&held, path(&[Step::Arg(0)]));
            self.flow().set_origins(whole, value);
            return;
        }
        let mut at = Path::ROOT;
        let mut base_ty = self.inference.resolve(base.1);
        while let Ty::Reference { target, .. } | Ty::Boxed(target) | Ty::Counted(_, target) =
            base_ty
        {
            at = at.push(Step::Arg(0));
            base_ty = self.inference.resolve(&target);
        }
        let value = region::sub(&held, at);
        let part = match (part, &base_ty) {
            (PartOf::Element, Ty::Array { .. }) => region::sub(&value, path(&[Step::Arg(0)])),
            (PartOf::Element, Ty::Keyed(_, arguments)) if arguments.len() == 2 => {
                region::sub(&value, path(&[Step::Arg(1)]))
            }
            (PartOf::Member(index), Ty::Tuple(_)) => {
                region::sub(&value, path(&[Step::Arg(index as u8)]))
            }
            (PartOf::Field(definition, slot), Ty::Script(..)) => {
                self.field_held(&value, definition, None, slot)
            }
            _ => region::collapse(&value),
        };
        self.flow().set_origins(whole, part);
    }

    /// What the field `slot` (of `variant`, for an enum) of a value of the
    /// crate type `definition` holding `value` holds.
    fn field_held(
        &self,
        value: &[(Path, Origin)],
        definition: type_def::Id,
        variant: Option<&str>,
        slot: usize,
    ) -> Held {
        match self.field_positions(definition, variant, slot) {
            Some(positions) => positions
                .into_iter()
                .filter_map(|(field, at)| Some(region::place(&region::sub(value, at?), field)))
                .flatten()
                .collect(),
            None => region::collapse(value),
        }
    }

    /// Where each region position of the field `slot` (of `variant`, for an
    /// enum) of the crate type `definition` sits in the type, or `None` for
    /// one these facts can't place.
    fn field_positions(
        &self,
        definition: type_def::Id,
        variant: Option<&str>,
        slot: usize,
    ) -> Option<Vec<(Path, Option<Path>)>> {
        let (regions, parameters, ty) = self.field_type(definition, variant, slot)?;
        signature::field_positions(ty, regions, &parameters, &|target| {
            self.declared_lifetimes(target)
        })
    }

    /// What a value holding `held` puts in a value of the crate type
    /// `definition` as its field `slot` (of `variant`, for an enum): what
    /// it holds at each of the field's region positions, placed at the
    /// type's, and what it holds at `'static` ones. A bound the type
    /// declares (`'b: 'a`, `T: 'a`) puts it in the positions it bounds too.
    /// `None` for a field these facts can't place.
    pub(super) fn field_stored(
        &self,
        held: &[(Path, Origin)],
        definition: type_def::Id,
        variant: Option<&str>,
        slot: usize,
    ) -> Option<(Held, Held)> {
        let positions = self.field_positions(definition, variant, slot)?;
        let (regions, parameters, _) = self.field_type(definition, variant, slot)?;
        let mut placed = Held::new();
        let mut statics = Held::new();
        for (field, at) in positions {
            let part = region::sub(held, field);
            let Some(at) = at else {
                statics.extend(part);
                continue;
            };
            for bounded in signature::outlived(regions, &parameters, at) {
                placed.extend(region::place(&region::collapse(&part), bounded));
            }
            placed.extend(region::place(&part, at));
        }
        Some((placed, statics))
    }

    /// The field `slot` (of `variant`, for an enum) of the crate type
    /// `definition`: the type's regions, its type parameters' names, and
    /// the field's type.
    fn field_type(
        &self,
        definition: type_def::Id,
        variant: Option<&str>,
        slot: usize,
    ) -> Option<(
        &crate::resolve::item::TypeRegions,
        Vec<ImmutableString>,
        &type_ref::Type,
    )> {
        let (regions, generics, ty) = match self.models.definition(definition) {
            Nominal::Struct { item, .. } => {
                (&item.regions, &item.generics, &item.fields.get(slot)?.ty)
            }
            Nominal::Enum { item, .. } => {
                let variant = item
                    .variants
                    .iter()
                    .find(|declared| Some(declared.name.text.as_str()) == variant)?;
                (&item.regions, &item.generics, variant.payload.get(slot)?)
            }
        };
        let parameters = generics
            .iter()
            .map(|generic| generic.name.text.clone())
            .collect();
        Some((regions, parameters, ty))
    }

    /// Where a `for` loop's element sits in the value it iterates, of type
    /// `ty`: pairs of the element's position and the iterated value's.
    pub(super) fn element_positions(&self, ty: &Ty) -> Vec<(Path, Path)> {
        let a0 = path(&[Step::Arg(0)]);
        let element = path(&[Step::Arg(0), Step::Arg(0)]);
        match self.inference.resolve(ty) {
            Ty::Iter(..) | Ty::Array { .. } | Ty::Option(_) | Ty::Range(_) => {
                vec![(Path::ROOT, a0)]
            }
            // A map's `(K, V)` items are where its arguments are.
            Ty::Keyed(_, arguments) if arguments.len() == 2 => vec![(Path::ROOT, Path::ROOT)],
            Ty::Keyed(..) => vec![(Path::ROOT, a0)],
            Ty::Reference { target, .. } => match self.inference.resolve(&target) {
                Ty::Array { .. } | Ty::Option(_) => vec![(Path::OWN, Path::OWN), (a0, element)],
                Ty::Keyed(_, arguments) if arguments.len() == 2 => vec![
                    (path(&[Step::Arg(0), Step::Own]), Path::OWN),
                    (element, element),
                    (path(&[Step::Arg(1), Step::Own]), Path::OWN),
                    (
                        path(&[Step::Arg(1), Step::Arg(0)]),
                        path(&[Step::Arg(0), Step::Arg(1)]),
                    ),
                ],
                Ty::Keyed(..) => vec![(Path::OWN, Path::OWN), (a0, element)],
                _ => vec![(Path::ROOT, Path::OWN)],
            },
            _ => vec![(Path::ROOT, Path::OWN)],
        }
    }

    /// `&mut value` (spanning `whole`) of a closure written in place, as a
    /// function's `&mut impl FnMut` argument: a `&mut` borrow of a
    /// temporary holding what the closure holds of its captures.
    pub(super) fn borrow_temporary_mut(&self, value: &ParsedExpr, whole: Span) {
        let held = self.flow().origins(value.span).map(<[_]>::to_vec);
        let Some(held) = held else {
            self.flow().unmodeled(
                "a `&mut` borrow of a value these facts do not follow",
                whole,
            );
            return;
        };
        let reference = self
            .flow()
            .borrow_temporary(value.span, true, &held, false, false);
        self.flow().set_origins(whole, reference);
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
            self.flow()
                .origins(value.span)
                .map(<[(Path, Origin)]>::to_vec)
        } else {
            Some(Vec::new())
        };
        // A part of a place (`&v[1..]`), or one read through a reference
        // (`&s.as_bytes()[1..]`), is a place.
        let place = self.flow().in_place(value.span);
        match held {
            Some(held) if !place && self.made_here(value.span, &ty) => {
                let drops = self.temporary_drop_use(&ty, value.span);
                let reference = self
                    .flow()
                    .borrow_temporary(value.span, false, &held, true, drops);
                self.flow().set_origins(whole, reference);
            }
            // `&v.first()` of a value these facts do not follow: what the
            // value holds, below its own region.
            Some(held) if holds => {
                let below = region::place(&held, path(&[Step::Arg(0)]));
                self.flow().set_origins(whole, below);
            }
            _ if holds => {}
            _ => self.flow().set_origins(whole, Vec::new()),
        }
    }

    /// Whether the drop of a temporary of type `ty` spanning `span` uses
    /// the borrows it holds. One whose drop uses only some is noted.
    fn temporary_drop_use(&self, ty: &Ty, span: Span) -> bool {
        match self.drop_use(ty) {
            drops::DropUse::None => false,
            drops::DropUse::All => true,
            drops::DropUse::Some => {
                self.flow().unmodeled(
                    "a value whose drop uses some of the borrows it holds and not others",
                    span,
                );
                false
            }
        }
    }

    /// Whether the value of type `ty` spanning `span` is made here, at run
    /// time, rather than a constant promoted to `'static`: it reads a local
    /// or calls something, or only a call makes a value of its type.
    fn made_here(&self, span: Span, ty: &Ty) -> bool {
        // A static a block declares lives as long as the program.
        if self.static_reads.contains(&span) {
            return false;
        }
        // A function, or a closure that captures nothing, is a constant,
        // whatever its body reads, as `&Code::highlight` is (#1981).
        if let Ty::FnItem { key, .. } = ty {
            return self
                .lifted
                .iter()
                .any(|closure| closure.key == **key && !closure.captures.is_empty());
        }
        // No constant whose value needs a drop is promoted (`&D("a")` of a
        // `D` with `impl Drop`).
        if self.flow().part_of_made(span) || self.needs_drop(ty, span).unwrap_or(false) {
            return true;
        }
        matches!(
            ty,
            Ty::String
                | Ty::Array {
                    kind: sequence::Kind::Vec | sequence::Kind::Deque,
                    ..
                }
                | Ty::Keyed(..)
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
        if self.copy_unsettled(&ty) {
            return None;
        }
        Some(!self.moves_by_value(&ty))
    }

    /// A value of type `ty` spanning `whole` is made of `parts` (a struct's
    /// fields, `Some(x)`, a tuple's or an array's members), each in its
    /// slot: it holds what they hold, at their positions. A type that can
    /// hold no borrow of the body holds only `'static` ones (`Foo { x: &a }`
    /// of `x: &'static u32`).
    pub(super) fn compose(&self, whole: Span, ty: &Ty, parts: &[(&ParsedExpr, Slot)]) {
        self.flow().constructed(whole.end());
        let mut origins = Held::new();
        let mut statics = Held::new();
        for (part, slot) in parts {
            if !self.may_hold_borrow(&part.ty) {
                continue;
            }
            let held = self
                .flow()
                .origins(part.span)
                .map(<[(Path, Origin)]>::to_vec);
            let Some(held) = held else {
                self.flow().unmodeled(
                    "a borrow stored in a value these facts do not follow",
                    part.span,
                );
                return;
            };
            match *slot {
                Slot::At(at) => origins.extend(region::place(&held, at)),
                Slot::Whole => origins.extend(held),
                Slot::Field(definition, variant, index) => {
                    match self.field_stored(&held, definition, variant, index) {
                        Some((placed, forever)) => {
                            origins.extend(placed);
                            statics.extend(forever);
                        }
                        None => origins.extend(region::collapse(&held)),
                    }
                }
            }
        }
        if !statics.is_empty() {
            self.flow().forever(&statics);
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
        let output = self.inference.resolve(&value.ty);
        let summary = match self.call_summaries.remove(&end) {
            Some(summary) => summary,
            // std's inherent method's, from its declaration (D711 step
            // 4), else a std trait method's rawr binds.
            None => {
                super::std_method::summary(&self.inference.resolve(&receiver.ty), &output, member)
                    .or_else(|| builtin_summary(&receiver.ty, member))
                    .unwrap_or_else(Summary::unknown)
            }
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
    /// `'static`, each at its position. What the facts can't follow is left
    /// to rustc (D572.5).
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
            let held = self.flow().origins(span).map(<[(Path, Origin)]>::to_vec);
            if let Some(held) = held {
                self.flow().pin(&held);
            }
        }
        let holds = self.may_hold_borrow(ty);
        // An argument that may hold borrows these facts don't follow may be
        // kept in ways they can't see.
        let mut known = !arguments
            .iter()
            .any(|argument| argument.holds && self.flow().origins(argument.span).is_none());
        let mut inputs = CallInputs {
            call,
            receiver,
            mode: summary.mode,
            arguments: &arguments,
            taken: None,
        };
        let mut origins = Held::new();
        if holds {
            match &summary.output {
                Some(taps) => {
                    for tap in taps {
                        match self.read_input(&mut inputs, tap.from) {
                            Some(held) => origins.extend(region::place(&held, tap.into)),
                            None => known = false,
                        }
                    }
                }
                None => known = false,
            }
        }
        let any_holds = arguments.iter().any(|argument| argument.holds);
        match &summary.stores {
            Some(taps) => {
                for tap in taps {
                    match (self.read_input(&mut inputs, tap.from), receiver) {
                        (Some(stored), _) if stored.is_empty() => {}
                        (Some(stored), Some(receiver)) if receiver.place => {
                            let stored = region::place(&stored, tap.into);
                            let kept = self.flow().store(receiver.start, &stored);
                            if !kept {
                                self.stored_below(
                                    receiver.start,
                                    &stored,
                                    false,
                                    call,
                                    outlives::Cause::Argument,
                                );
                            }
                            known &= kept;
                        }
                        _ => known = false,
                    }
                }
            }
            None if any_holds => known = false,
            None => {}
        }
        // What a `&mut` argument refers to takes another input's borrows
        // (`store(&mut v, &x)`).
        for (index, taps) in &summary.argument_stores {
            for tap in taps {
                match (
                    self.read_input(&mut inputs, tap.from),
                    arguments.get(*index),
                ) {
                    (Some(stored), _) if stored.is_empty() => {}
                    (Some(stored), Some(argument)) => {
                        known &= self.flow().store_through(argument.span, &stored);
                    }
                    _ => known = false,
                }
            }
        }
        // The receiver's own borrow, kept in what a `&mut` argument
        // refers to (`scene.draw(&mut pass)`).
        for &index in &summary.receiver_into {
            let kept = self.read_input(&mut inputs, Source::Input(Input::Receiver, Path::OWN));
            match (kept, arguments.get(index)) {
                (Some(kept), _) if kept.is_empty() => {}
                (Some(kept), Some(argument)) => {
                    known &= self.flow().store_through(argument.span, &kept);
                }
                _ => known = false,
            }
        }
        match &summary.statics {
            Some(statics) => {
                for &(input, at) in statics {
                    match self.read_input(&mut inputs, Source::Input(input, at)) {
                        Some(forever) if forever.is_empty() => {}
                        Some(forever) => self.flow().forever(&forever),
                        None => known = false,
                    }
                }
            }
            None if any_holds => known = false,
            None => {}
        }
        // A value made for a `&self` or `&mut self` call is a temporary,
        // whose drop at its temporary scope's end may use what it holds.
        if let Some(receiver) = receiver
            && inputs.taken.is_none()
            && !receiver.place
            && matches!(
                summary.mode,
                Some(signature::Receiver::Shared | signature::Receiver::Mutable)
            )
            && self.drop_use(&receiver.ty) != drops::DropUse::None
        {
            inputs.taken = Some(self.receiver_input(receiver, summary.mode, call));
        }
        // A receiver that can hold no borrow stores none (`s.push_str`).
        if summary.unknown_stores
            && any_holds
            && receiver.is_none_or(|receiver| self.may_hold_borrow(&receiver.ty))
        {
            known = false;
        }
        if !known {
            self.flow()
                .unmodeled("a borrow a call keeps that these facts do not follow", call);
        } else if holds {
            self.flow().set_origins(call, origins);
        }
    }

    /// What `from` holds of a call's inputs, or `None` where these facts
    /// can't tell.
    fn read_input(&self, inputs: &mut CallInputs<'_>, from: Source) -> Option<Held> {
        match from {
            Source::Input(Input::Argument(index), at) => {
                let argument = inputs.arguments.get(index)?;
                if !argument.holds {
                    return Some(Held::new());
                }
                let held = self.flow().origins(argument.span)?.to_vec();
                Some(region::sub(&held, at))
            }
            Source::Input(Input::Receiver, at) => {
                let receiver = inputs.receiver?;
                // The receiver's own region is the borrow made for the call,
                // made only when a position reads it.
                let own = at.is_root() || at.steps().next() == Some(Step::Own);
                if own && inputs.taken.is_none() {
                    inputs.taken = Some(self.receiver_input(receiver, inputs.mode, inputs.call));
                }
                match &inputs.taken {
                    Some(taken) => Some(region::sub(taken.as_ref()?, at)),
                    None => {
                        let value = self.receiver_value(receiver, inputs.mode)?;
                        Some(region::sub(&value, at))
                    }
                }
            }
            // What a closure's call returns of what it captured.
            Source::Call(index) => {
                let argument = inputs.arguments.get(index)?;
                match self.flow().closure_keeps(argument.span) {
                    Some(keeps) => keeps.clone(),
                    None if !argument.returns => Some(Held::new()),
                    None => None,
                }
            }
        }
    }

    /// The receiver of a method taking it as `mode`, as the method takes it,
    /// but for a `&self` method's own region: for `&self` the reference, with
    /// the value it refers to below `Arg(0)`, through any references and
    /// boxes the call goes through. `None` where these facts can't tell.
    fn receiver_value(
        &self,
        receiver: &CallReceiver,
        mode: Option<signature::Receiver>,
    ) -> Option<Held> {
        let value = if !self.may_hold_borrow(&receiver.ty) {
            Held::new()
        } else if receiver.place
            && let Some(origin) = self.flow().held(receiver.start)
        {
            vec![(Path::ROOT, origin)]
        } else {
            self.flow().origins(receiver.span)?.to_vec()
        };
        // By value, or `s.trim()` of a `&str`: the receiver as it is.
        if mode.is_none_or(|mode| mode == signature::Receiver::Owned) || receiver.ty == Ty::Str {
            return Some(value);
        }
        match through(&receiver.ty) {
            // A reference to a reference: all of it, wherever it is.
            (_, references) if references > 1 => Some(region::collapse(&value)),
            // A reference made here is the reference passed.
            (at, 1) if !receiver.place && at.len() == 1 => Some(value),
            (at, _) => Some(region::place(
                &region::sub(&value, at),
                path(&[Step::Arg(0)]),
            )),
        }
    }

    /// The receiver as a method taking it as `mode` takes it, with its own
    /// region: for `&self` of a place, the borrow made for the call.
    fn receiver_input(
        &self,
        receiver: &CallReceiver,
        mode: Option<signature::Receiver>,
        call: Span,
    ) -> Option<Held> {
        let mut value = self.receiver_value(receiver, mode)?;
        let mode = mode.unwrap_or(signature::Receiver::Owned);
        if mode == signature::Receiver::Owned || receiver.ty == Ty::Str {
            return Some(value);
        }
        let (at, references) = through(&receiver.ty);
        if references > 1 {
            return Some(value);
        }
        let mutable = mode == signature::Receiver::Mutable;
        if !receiver.place {
            // A reference made here (`v.iter().as_slice()`) has its own
            // region already.
            if references == 1 && at.len() == 1 {
                return Some(value);
            }
            // A value made here (`String::from("a").as_str()`): a temporary
            // borrowed for the call, which its temporary scope holds.
            if references == 0
                && !self.flow().in_place(receiver.span)
                && self.made_here(receiver.span, &receiver.ty)
            {
                let held = region::sub(&value, path(&[Step::Arg(0)]));
                let drops = self.temporary_drop_use(&receiver.ty, receiver.span);
                return Some(self.flow().borrow_temporary(
                    receiver.span,
                    mutable,
                    &held,
                    false,
                    drops,
                ));
            }
            return None;
        }
        let below = match &receiver.ty {
            Ty::Reference { target, .. } => self.may_hold_borrow(target),
            ty => self.may_hold_borrow(ty),
        };
        let whole = span(receiver.start, call.end());
        let loan = self.flow().autoref(receiver.start, whole, mutable, below)?;
        value.push((Path::OWN, loan));
        Some(value)
    }

    /// The place `value` reads one step further down.
    pub(super) fn project_place(&self, value: &ParsedExpr, step: Projection) {
        if is_place(value) {
            self.flow().project(value.span.start(), step);
        }
    }

    /// `value[i]`: an element of an array is a place below it; `Index` on a
    /// `Vec`, a `VecDeque`, a `String` or a map borrows all of it.
    pub(super) fn indexed(&self, value: &ParsedExpr) {
        if !is_place(value) {
            return;
        }
        match self.inference.resolve(&value.ty) {
            Ty::Array {
                kind: sequence::Kind::Vec | sequence::Kind::Deque,
                ..
            }
            | Ty::String
            | Ty::Keyed(..) => {
                self.flow().overloaded_index(value.span.start(), value.span);
            }
            _ => self.project_place(value, Projection::Index),
        }
    }

    /// A field read of `value`, a value made here whose `Drop` uses the
    /// borrows it holds (`G(&x).0`): a temporary its statement drops at its
    /// end, which uses them there.
    pub(super) fn field_of_temporary(&self, value: &ParsedExpr) {
        if is_place(value) || self.flow().in_place(value.span) {
            return;
        }
        let ty = self.inference.resolve(&value.ty);
        if !self.made_here(value.span, &ty) || self.drop_use(&ty) != drops::DropUse::All {
            return;
        }
        let held = self.flow().origins(value.span).map(<[_]>::to_vec);
        if let Some(held) = held {
            self.flow().hold_temporary(value.span, &held);
        }
    }

    /// `cell.borrow()` or `cell.borrow_mut()` spanning `call`: its guard
    /// holds the `&self` borrow of the cell for as long as it lives, as
    /// `Ref` and `RefMut` do.
    pub(super) fn cell_borrowed(&self, cell: &ParsedExpr, call: Span) {
        if !is_place(cell) {
            return;
        }
        let below = self.may_hold_borrow(&cell.ty);
        let loan = self.flow().autoref(cell.span.start(), call, false, below);
        if let Some(loan) = loan {
            self.flow().set_origins(call, vec![(Path::OWN, loan)]);
        }
    }

    /// `value`, a shared reference the call spanning `call` makes of a
    /// place and takes as its first input, as a slice member takes
    /// `&items[..]` (#2097): the place's loan, made where it is read and
    /// kept over the arguments, is that input, which the call's value may
    /// keep. A value that is no place is the input as it is.
    #[cfg(feature = "rust-import")]
    pub(super) fn lend_first_input(&self, value: &ParsedExpr, call: Span) {
        if is_place(value) {
            let below = match self.inference.resolve(&value.ty) {
                Ty::Reference { target, .. } => self.may_hold_borrow(&target),
                ty => self.may_hold_borrow(&ty),
            };
            let loan = self.flow().autoref(value.span.start(), call, false, below);
            match loan {
                Some(loan) => self.flow().set_origins(value.span, vec![(Path::OWN, loan)]),
                None => self.flow().shared(value.span.start()),
            }
        }
        let mut arguments = self.flow().arguments(call.end()).unwrap_or_default();
        arguments.insert(0, self.argument_fact(value));
        self.flow().call_operands(&arguments, call.end());
    }

    /// A method call on the place `value`: a shared borrow of it, unless
    /// the method's mutability check makes it a `&mut` one.
    pub(super) fn call_on_place(&self, value: &ParsedExpr) {
        if is_place(value) {
            self.flow().shared(value.span.start());
        }
    }

    /// A `format!` argument `value`, borrowed until the macro has read the
    /// arguments after it: a place's shared borrow, made where it is read,
    /// or the borrows a value made for it holds (`&p`, `s.as_str()`).
    pub(super) fn format_argument_borrow(&self, value: &ParsedExpr, borrowed: &mut Held) {
        if is_place(value) {
            // `&r` of a reference keeps what `r` borrows.
            let inner = self.may_hold_borrow(&self.inference.resolve(&value.ty));
            let loan = self
                .flow()
                .autoref(value.span.start(), value.span, false, inner);
            borrowed.extend(loan.map(|loan| (Path::OWN, loan)));
        } else if let Some(held) = self.flow().origins(value.span) {
            borrowed.extend_from_slice(held);
        }
    }

    /// The place `value` is borrowed `&mut` for a call made here, after its
    /// arguments: a two-phase borrow, reserved where it was read.
    pub(super) fn mutable_use(&self, value: &ParsedExpr, at: Span) {
        if is_place(value) {
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
    /// own (`reborrow.rs`), but not yet one whose place no runtime local
    /// can hold (`reborrow_source`), or one in an async body, for which a
    /// new borrow would write the place instead. A shared reference written through it (`*r = x`,
    /// `h.x = x`) is a value its place takes.
    pub(super) fn refuse_rebound_alias(&mut self, target: Span, ty: &Ty) -> Result<(), Error> {
        let Some((name, declaration)) = self.flow().mentioned_local(target.start()) else {
            return Ok(());
        };
        let through = self
            .flow()
            .value_place(target.start())
            .is_some_and(|place| !place.projection.is_empty());
        if through
            && matches!(
                self.inference.resolve(ty),
                Ty::Reference { mutable: false, .. }
            )
        {
            return Ok(());
        }
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
                    "giving the `&mut` local `{name}` a new borrow when it borrows a place no \
                     runtime local can hold, or in an async body: not supported in Rawr yet"
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
        #[cfg(feature = "rust-import")]
        self.bind_opaque_bound(name, declaration, value);
        let holds = self.may_hold_borrow(&value.ty);
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
        assignment: record::Assignment,
    ) {
        let compound = assignment != record::Assignment::Plain;
        let holds = self.may_hold_borrow(&replacement.ty);
        // A local that holds borrows given a whole new value (`r = &w`,
        // `view = View { value: &b }`) holds only the new value's.
        if !compound
            && let Lvalue::Binding { binding, .. } = lvalue
            && let Some(local) = self.bindings.get(binding)
            && self.may_hold_borrow(&local.ty)
            && self.may_hold_borrow(target_ty)
        {
            // The old value drops before the new one is stored.
            match self.drop_use(&local.ty) {
                drops::DropUse::None => {}
                drops::DropUse::All => self.flow().drop_use(local.declaration),
                drops::DropUse::Some => self.flow().unmodeled(
                    "a value whose drop uses some of the borrows it holds and not others",
                    local.declaration,
                ),
            }
            let whole = assignee::assignment_span(target, replacement);
            let into = self.flow().rebound(target.start(), whole);
            if let Some(into) = into {
                self.flow().claim(into, replacement.span, start, holds);
            }
            return;
        }
        let whole = assignee::assignment_span(target, replacement);
        self.flow().assigned(target.start(), whole, assignment);
        // `pair.right = &b`: the local takes the borrows besides its own.
        let origins = self
            .flow()
            .origins(replacement.span)
            .map(<[(Path, Origin)]>::to_vec);
        let stored = holds
            && !compound
            && self.may_hold_borrow(target_ty)
            && origins
                .is_some_and(|origins| self.store_borrows(target.start(), lvalue, &origins, whole));
        // `s += &t` of a `String` makes a new value that holds no borrow.
        let holds = holds && self.may_hold_borrow(target_ty);
        if !stored {
            self.flow().escapes(replacement.span, start, holds);
        }
    }

    /// The place the use starting at `start` reads, the target `lvalue`,
    /// takes `held`: a crate type's field of a local at the field's
    /// positions in the local's type. Whether these facts follow it.
    fn store_borrows(
        &self,
        start: Position,
        lvalue: &Lvalue,
        held: &[(Path, Origin)],
        whole: Span,
    ) -> bool {
        if let Lvalue::Field(field) = lvalue
            && let Some((placed, statics)) =
                self.field_stored(held, field.definition.id(), None, field.slot)
        {
            if !statics.is_empty() {
                self.flow().forever(&statics);
            }
            let kept = self.flow().store_field(start, &placed);
            if !kept {
                self.stored_below(start, &placed, true, whole, outlives::Cause::Assignment);
            }
            return kept;
        }
        let kept = self.flow().store(start, held);
        if !kept {
            self.stored_below(start, held, false, whole, outlives::Cause::Assignment);
        }
        kept
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
            | Ty::InclusiveRange(_)
            | Ty::FloatRange
            | Ty::Ordering
            | Ty::Time(_)
            | Ty::ParseError(_) => false,
            Ty::Array { element, .. }
            | Ty::Option(element)
            | Ty::Range(element)
            | Ty::OpenRange(_, element)
            | Ty::InclusiveOf(element) => self.may_hold_borrow(&element),
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
        // rustc reports each error; this reports the first in the source.
        let conflict = borrowck::check(&body, &borrowck::nll::Nll);
        let moved = borrowck::moves::check(&body);
        if let Some(twice) = borrowck::assigns::check(&body)
            && conflict
                .as_ref()
                .is_none_or(|conflict| twice.assignment.start() <= conflict.access.start())
            && moved
                .as_ref()
                .is_none_or(|moved| twice.assignment.start() <= moved.access.start())
        {
            return Err(Error::ImmutableBinding {
                binding: twice.name.into(),
                mutation: self.expression_span(twice.assignment),
                declaration: twice
                    .first
                    .map_or(Span::NONE, |first| self.expression_span(first)),
                declared: twice.declared,
                matched: self.matched_bindings.contains(&twice.declared.start()),
            });
        }
        if let Some(moved) = moved
            && conflict
                .as_ref()
                .is_none_or(|conflict| moved.access.start() <= conflict.access.start())
        {
            // The facts' spans are where each token starts: a move of one
            // token is that token whole, and a move site is whole already.
            let whole = |at: Span| {
                if at.start() == at.end() {
                    self.expression_span(at)
                } else {
                    at
                }
            };
            return Err(Error::MovedValue {
                cause: self.checked_move_cause(
                    moved.moved,
                    &moved.moved_place,
                    moved.declaration,
                    moved.partial,
                ),
                name: moved.name.into(),
                moved: whole(self.move_span(moved.moved)),
                // A use ends where its last token starts, and a `&mut`
                // borrow where its text does.
                reference: if moved.whole {
                    moved.access
                } else {
                    self.expression_span(moved.access)
                },
                borrowed: moved.borrowed,
                partial: moved.partial,
                // A move in a loop's body, made again on its next iteration.
                previous_iteration: moved.moved == moved.access,
            });
        }
        match conflict {
            None => Ok(()),
            Some(borrowck::Conflict {
                report: Some(report),
                ..
            }) => {
                // The facts give where the value's first and last tokens start.
                Err(Error::Outlives(Box::new(
                    report.returned_at(|returned| self.expression_span(returned)),
                )))
            }
            Some(conflict) => Err(Error::BorrowConflict {
                code: conflict.code,
                message: conflict.message.into(),
                access: conflict.access,
                borrow: conflict.borrow,
            }),
        }
    }
}

/// Where a place is in the type of its local.
struct Walked {
    /// Where each position of the place's value is in the local's type, by
    /// prefix.
    positions: Vec<(Path, Path)>,
    /// Each reference the place goes through, outermost first.
    derefs: Vec<Deref>,
}

/// A reference a place goes through.
struct Deref {
    /// Its own region's position in the local's type.
    own: Path,
    mutable: bool,
}

/// Where the position `at` of a value is in a local's type, by the
/// value's `positions` there.
fn locate(positions: &[(Path, Path)], at: Path) -> Option<Path> {
    positions
        .iter()
        .filter(|(value, _)| at.starts_with(*value))
        .max_by_key(|(value, _)| value.len())
        .map(|(value, local)| local.join(at.strip(*value)))
}

/// The `positions` of what is at `step` below a value.
fn descend(positions: &[(Path, Path)], step: Step) -> Vec<(Path, Path)> {
    let below = path(&[step]);
    positions
        .iter()
        .filter_map(|&(value, local)| {
            if value.starts_with(below) {
                Some((value.strip(below), local))
            } else if value.is_root() {
                Some((Path::ROOT, local.push(step)))
            } else {
                None
            }
        })
        .collect()
}

/// The path through the references and boxes of `ty` to the value they
/// hold, and how many of them are references.
fn through(ty: &Ty) -> (Path, usize) {
    let mut ty = ty;
    let mut at = Path::ROOT;
    let mut references = 0;
    loop {
        match ty {
            Ty::Reference { target, .. } => {
                references += 1;
                at = at.push(Step::Arg(0));
                ty = target;
            }
            Ty::Boxed(target) | Ty::Counted(_, target) => {
                at = at.push(Step::Arg(0));
                ty = target;
            }
            _ => return (at, references),
        }
    }
}

/// A call's inputs, as [`Parser::kept_by_call`] reads them.
struct CallInputs<'a> {
    call: Span,
    receiver: Option<&'a CallReceiver>,
    mode: Option<signature::Receiver>,
    arguments: &'a [record::Argument],
    /// The receiver as the method takes it, with its own region, once a
    /// position reads that.
    taken: Option<Option<Held>>,
}

/// What a std trait method rawr binds (`clone`, `as_ref`, `into_iter`,
/// `extend`) on a builtin value of type `receiver` keeps of its receiver
/// and arguments, position by position, as its signature says, or `None`
/// for one not listed. An inherent method's and an iterator's come from
/// their declarations (`std_method::summary`, D711 step 4). A method whose
/// value holds no borrow needs no entry unless it stores its arguments.
///
/// The receiver is as the method takes it: for `&self`, `[Own]` is the
/// borrow and `Self` is below `[Arg(0)]`.
fn builtin_summary(receiver: &Ty, member: &str) -> Option<Summary> {
    use signature::Receiver::{Mutable, Owned, Shared};
    const O: Step = Step::Own;
    const A0: Step = Step::Arg(0);
    const A1: Step = Step::Arg(1);
    let mut ty = receiver;
    let mut references = 0;
    while let Ty::Reference { target, .. } = ty {
        ty = target;
        references += 1;
    }
    let tap = |into: &[Step], from: Source| Tap {
        into: path(into),
        from,
    };
    let own = |at: &[Step]| Source::Input(Input::Receiver, path(at));
    let argument = |index: usize, at: &[Step]| Source::Input(Input::Argument(index), path(at));
    let entry =
        |mode, output: Vec<Tap>, stores: Vec<Tap>| Some(Summary::taps(Some(mode), output, stores));
    // All of the receiver, borrow and all, at every position: a value
    // whose regions are all the borrow's (`s.as_ref()`).
    let all = || vec![tap(&[], own(&[O]))];
    // All of what `&self` refers to, without the borrow, at every position:
    // a value these facts don't place (`<[T] as Concat<T>>::Output`).
    let inner = || vec![tap(&[], own(&[A0, O]))];
    // `&'a T` of what `&'a self` refers to at `at`, placed at `into`.
    let reference = |into: &[Step], at: &[Step]| {
        let mut target = into.to_vec();
        target.push(A0);
        let mut referred = vec![A0];
        referred.extend_from_slice(at);
        let mut own_at = into.to_vec();
        own_at.push(O);
        vec![tap(&own_at, own(&[O])), tap(&target, own(&referred))]
    };
    // An iterator of `&'a T` of what `&'a self` refers to at `at`.
    let iter = |at: &[Step]| {
        let mut taps = vec![tap(&[O], own(&[O]))];
        taps.extend(reference(&[A0], at));
        taps
    };
    let map = |arguments: &[Ty]| arguments.len() == 2;
    match ty {
        // Inherent methods are std's own (`std_method::summary`).
        Ty::Array { .. } => match member {
            "as_ref" | "borrow" => entry(Shared, vec![tap(&[], own(&[]))], Vec::new()),
            "as_mut" => entry(Mutable, vec![tap(&[], own(&[]))], Vec::new()),
            "clone" | "to_owned" => entry(Shared, vec![tap(&[], own(&[A0]))], Vec::new()),
            "concat" => entry(Shared, inner(), Vec::new()),
            "into_iter" if references > 0 => entry(Owned, iter(&[A0]), Vec::new()),
            "into_iter" => entry(Owned, vec![tap(&[A0], own(&[A0]))], Vec::new()),
            "extend" => entry(Mutable, Vec::new(), vec![tap(&[A0], argument(0, &[O]))]),
            _ => None,
        },
        Ty::String | Ty::Str => match member {
            "as_ref" | "borrow" => entry(Shared, all(), Vec::new()),
            _ => None,
        },
        Ty::Keyed(_, arguments) => {
            let map = map(arguments);
            // `(&'a K, &'a V)` of a map's entry, or `&'a K` of a set's.
            let entry_ref = |into: &[Step]| {
                if map {
                    let mut taps = Vec::new();
                    for (member, argument) in [(A0, A0), (A1, A1)] {
                        let mut at = into.to_vec();
                        at.push(member);
                        taps.extend(reference(&at, &[argument]));
                    }
                    taps
                } else {
                    reference(into, &[A0])
                }
            };
            match member {
                "extend" | "push" => entry(Mutable, Vec::new(), vec![tap(&[], argument(0, &[O]))]),
                "clone" => entry(Shared, vec![tap(&[], own(&[A0]))], Vec::new()),
                "into_iter" if references > 0 => {
                    let mut taps = vec![tap(&[O], own(&[O]))];
                    taps.extend(entry_ref(&[A0]));
                    entry(Owned, taps, Vec::new())
                }
                // A map's `(K, V)` items are where its arguments are.
                "into_iter" if map => entry(Owned, vec![tap(&[A0], own(&[]))], Vec::new()),
                "into_iter" => entry(Owned, vec![tap(&[A0], own(&[A0]))], Vec::new()),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Whether `value` is a place the borrow facts follow: a local or a part of
/// one, or the receiver `self`, which is a local when it is owned.
fn is_place(value: &ParsedExpr) -> bool {
    value.lvalue.is_some() || value.source.as_str() == "self"
}
