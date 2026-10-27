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
        binding: Option<(&ImmutableString, Span)>,
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
            // Rawr binds the value below a reference, which Rust binds as
            // the reference: the loans it keeps are left to rustc.
            Some((_, span)) if !derefs.is_empty() => {
                trace.unmodeled = Some(("a binding of a matched reference's value", span));
            }
            Some((name, span)) => trace.parts.push(Part {
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
        compiled: &HostPattern,
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
            return Summary::unknown();
        }
        // All of an input, at every position of the value.
        let all = |input| Tap {
            into: Path::ROOT,
            from: Source::Input(input, Path::OWN),
        };
        // `Parser::new(&text)`'s value borrows its `&str` inputs; a method
        // whose frame holds what it takes may borrow its receiver or them.
        if item.opens() {
            return match mode {
                None => Summary::taps(
                    Some(signature::Receiver::Owned),
                    (0..arguments)
                        .map(|index| all(Input::Argument(index)))
                        .collect(),
                    Vec::new(),
                ),
                Some(_) => Summary::unknown(),
            };
        }
        let lends = item.guard().is_some() || *output == Ty::Str;
        match (mode, lends) {
            (Some(mode @ (signature::Receiver::Shared | signature::Receiver::Mutable)), true) => {
                Summary::taps(Some(mode), vec![all(Input::Receiver)], Vec::new())
            }
            (_, true) => Summary::unknown(),
            (mode, false) => Summary::nothing(mode),
        }
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
            Some(origins) => self.flow().give_back(&origins, value),
            None => self.flow().unmodeled(
                "a borrow a function returns that these facts do not follow",
                value,
            ),
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
        while let Ty::Reference { target, .. } | Ty::Boxed(_, target) = base_ty {
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
        let parameters: Vec<ImmutableString> = generics
            .iter()
            .map(|generic| generic.name.text.clone())
            .collect();
        signature::field_positions(ty, regions, &parameters, &|target| {
            self.declared_lifetimes(target)
        })
    }

    /// Where a `for` loop's element sits in the value it iterates, of type
    /// `ty`: pairs of the element's position and the iterated value's.
    pub(super) fn element_positions(&self, ty: &Ty) -> Vec<(Path, Path)> {
        let a0 = path(&[Step::Arg(0)]);
        let element = path(&[Step::Arg(0), Step::Arg(0)]);
        match self.inference.resolve(ty) {
            Ty::Iter(_) | Ty::Array { .. } | Ty::Option(_) | Ty::Range(_) => {
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
                    match self.field_positions(definition, variant, index) {
                        Some(positions) => {
                            for (field, at) in positions {
                                let part = region::sub(&held, field);
                                match at {
                                    Some(at) => origins.extend(region::place(&part, at)),
                                    None => statics.extend(part),
                                }
                            }
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
            None => builtin_summary(&receiver.ty, &output, member).unwrap_or_else(Summary::unknown),
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
                            known &= self.flow().store(receiver.start, &stored);
                        }
                        _ => known = false,
                    }
                }
            }
            None if any_holds => known = false,
            None => {}
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

    /// A `format!` argument `value`, borrowed until the macro has read the
    /// arguments after it: a place's shared borrow, made where it is read,
    /// or the borrows a value made for it holds (`&p`, `s.as_str()`).
    pub(super) fn format_argument_borrow(&self, value: &ParsedExpr, borrowed: &mut Held) {
        if value.lvalue.is_some() {
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
            // The old value drops before the new one is stored.
            match self.drop_use(&local.ty) {
                drops::DropUse::None => {}
                drops::DropUse::All => self.flow().drop_use(local.declaration),
                drops::DropUse::Some => self.flow().unmodeled(
                    "a value whose drop uses some of the borrows it holds and not others",
                    local.declaration,
                ),
            }
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
            .map(<[(Path, Origin)]>::to_vec);
        let stored = holds
            && !compound
            && self.may_hold_borrow(target_ty)
            && origins.is_some_and(|origins| self.store_borrows(target.start(), lvalue, &origins));
        // `s += &t` of a `String` makes a new value that holds no borrow.
        let holds = holds && self.may_hold_borrow(target_ty);
        if !stored {
            self.flow().escapes(replacement.span, start, holds);
        }
    }

    /// The place the use starting at `start` reads, the target `lvalue`,
    /// takes `held`: a crate type's field of a local at the field's
    /// positions in the local's type. Whether these facts follow it.
    fn store_borrows(&self, start: Position, lvalue: &Lvalue, held: &[(Path, Origin)]) -> bool {
        if let Lvalue::Field(field) = lvalue
            && let Some(positions) = self.field_positions(field.definition.id(), None, field.slot)
        {
            let mut placed = Held::new();
            let mut statics = Held::new();
            for (field_at, at) in positions {
                let part = region::sub(held, field_at);
                match at {
                    Some(at) => placed.extend(region::place(&part, at)),
                    None => statics.extend(part),
                }
            }
            if !statics.is_empty() {
                self.flow().forever(&statics);
            }
            return self.flow().store_field(start, &placed);
        }
        self.flow().store(start, held)
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
        // rustc reports each error; this reports the first in the source.
        let conflict = borrowck::check(&body, &borrowck::nll::Nll);
        if let Some(moved) = borrowck::moves::check(&body)
            && conflict
                .as_ref()
                .is_none_or(|conflict| moved.access.start() <= conflict.access.start())
        {
            return Err(Error::MovedValue {
                name: moved.name.into(),
                moved: moved.moved,
                reference: moved.access,
                // A move in a loop's body, made again on its next iteration.
                previous_iteration: moved.moved == moved.access,
            });
        }
        match conflict {
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
            Ty::Boxed(_, target) => {
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

/// What std's method `member` on a builtin value of type `receiver`, whose
/// value has type `output`, keeps of its receiver and arguments, position by
/// position, as its signature says, or `None` for one not listed. A method
/// whose value holds no borrow needs no entry unless it stores its
/// arguments.
///
/// The receiver is as the method takes it: for `&self`, `[Own]` is the
/// borrow and `Self` is below `[Arg(0)]`.
fn builtin_summary(receiver: &Ty, output: &Ty, member: &str) -> Option<Summary> {
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
    let call = Source::Call;
    let entry =
        |mode, output: Vec<Tap>, stores: Vec<Tap>| Some(Summary::taps(Some(mode), output, stores));
    // All of the receiver, borrow and all, at every position: a value
    // whose regions are all the borrow's (`s.trim()`).
    let all = || vec![tap(&[], own(&[O]))];
    // All of what `&self` refers to, without the borrow, at every position:
    // a value these facts don't place (`v.remove(i)` of a `Vec` or a
    // `VecDeque`).
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
        Ty::Array { .. } => match member {
            // `v.get(1..3)` gives a slice.
            "get" | "get_mut"
                if matches!(output, Ty::Option(item)
                    if matches!(&**item, Ty::Reference { target, .. }
                        if matches!(&**target, Ty::Array { .. }))) =>
            {
                entry(Shared, vec![tap(&[A0], own(&[]))], Vec::new())
            }
            "first" | "last" | "get" | "front" | "back" => {
                entry(Shared, reference(&[A0], &[A0]), Vec::new())
            }
            "get_mut" | "first_mut" | "last_mut" | "front_mut" | "back_mut" => {
                entry(Mutable, reference(&[A0], &[A0]), Vec::new())
            }
            "iter" => entry(Shared, iter(&[A0]), Vec::new()),
            "iter_mut" => entry(Mutable, iter(&[A0]), Vec::new()),
            "as_slice" | "as_ref" => entry(Shared, vec![tap(&[], own(&[]))], Vec::new()),
            "as_mut_slice" | "as_mut" => entry(Mutable, vec![tap(&[], own(&[]))], Vec::new()),
            "split_at" => entry(
                Shared,
                vec![tap(&[A0], own(&[])), tap(&[A1], own(&[]))],
                Vec::new(),
            ),
            "split_at_mut" => entry(
                Mutable,
                vec![tap(&[A0], own(&[])), tap(&[A1], own(&[]))],
                Vec::new(),
            ),
            "windows" | "chunks" | "chunks_exact" | "rchunks" | "range" => entry(
                Shared,
                vec![tap(&[O], own(&[O])), tap(&[A0], own(&[]))],
                Vec::new(),
            ),
            "split_first" | "split_last" => entry(Shared, all(), Vec::new()),
            "chunks_mut" | "split_first_mut" | "split_last_mut" => {
                entry(Mutable, all(), Vec::new())
            }
            "drain" => entry(
                Mutable,
                vec![tap(&[O], own(&[O])), tap(&[A0], own(&[A0, A0]))],
                Vec::new(),
            ),
            "to_vec" | "clone" | "to_owned" => {
                entry(Shared, vec![tap(&[], own(&[A0]))], Vec::new())
            }
            "concat" => entry(Shared, inner(), Vec::new()),
            "pop" | "pop_front" | "pop_back" => {
                entry(Mutable, vec![tap(&[A0], own(&[A0, A0]))], Vec::new())
            }
            "remove" | "swap_remove" => entry(Mutable, inner(), Vec::new()),
            "split_off" => entry(Mutable, vec![tap(&[], own(&[A0]))], Vec::new()),
            "into_iter" if references > 0 => entry(Owned, iter(&[A0]), Vec::new()),
            "into_iter" => entry(Owned, vec![tap(&[A0], own(&[A0]))], Vec::new()),
            "push" | "push_back" | "push_front" | "fill" => {
                entry(Mutable, Vec::new(), vec![tap(&[A0], argument(0, &[]))])
            }
            "insert" | "resize" => entry(Mutable, Vec::new(), vec![tap(&[A0], argument(1, &[]))]),
            "extend" => entry(Mutable, Vec::new(), vec![tap(&[A0], argument(0, &[O]))]),
            "append" | "extend_from_slice" => entry(
                Mutable,
                Vec::new(),
                vec![tap(&[A0], argument(0, &[A0, A0]))],
            ),
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
            | "split_at" => entry(Shared, all(), Vec::new()),
            // `Split<'a, P>` keeps its pattern.
            "split" | "rsplit" | "split_terminator" | "split_inclusive" | "matches"
            | "match_indices" => {
                let mut taps = all();
                taps.push(tap(&[], argument(0, &[O])));
                entry(Shared, taps, Vec::new())
            }
            "splitn" | "rsplitn" => {
                let mut taps = all();
                taps.push(tap(&[], argument(1, &[O])));
                entry(Shared, taps, Vec::new())
            }
            _ => None,
        },
        Ty::Keyed(_, arguments) => {
            let map = map(arguments);
            // Where a map keeps its values, or a set its keys.
            let values: &[Step] = if map { &[A1] } else { &[A0] };
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
                "get" | "first" | "last" => entry(Shared, reference(&[A0], values), Vec::new()),
                "get_mut" => entry(Mutable, reference(&[A0], values), Vec::new()),
                "get_key_value" | "first_key_value" | "last_key_value" if map => {
                    entry(Shared, entry_ref(&[A0]), Vec::new())
                }
                "iter" | "range" => {
                    let mut taps = vec![tap(&[O], own(&[O]))];
                    taps.extend(entry_ref(&[A0]));
                    entry(Shared, taps, Vec::new())
                }
                "iter_mut" => {
                    let mut taps = vec![tap(&[O], own(&[O]))];
                    taps.extend(entry_ref(&[A0]));
                    entry(Mutable, taps, Vec::new())
                }
                "keys" => entry(Shared, iter(&[A0]), Vec::new()),
                "values" => entry(Shared, iter(values), Vec::new()),
                "values_mut" => entry(Mutable, iter(values), Vec::new()),
                "union" | "intersection" | "difference" | "symmetric_difference" => {
                    let mut taps = iter(&[A0]);
                    taps.push(tap(&[O], argument(0, &[O])));
                    taps.push(tap(&[A0, O], argument(0, &[O])));
                    taps.push(tap(&[A0, A0], argument(0, &[A0, A0])));
                    entry(Shared, taps, Vec::new())
                }
                "remove" | "take" => {
                    let mut at = vec![A0];
                    at.extend_from_slice(values);
                    entry(Mutable, vec![tap(&[A0], own(&at))], Vec::new())
                }
                "remove_entry" | "pop_first" | "pop_last" if map => {
                    entry(Mutable, vec![tap(&[A0], own(&[A0]))], Vec::new())
                }
                "pop_first" | "pop_last" => {
                    entry(Mutable, vec![tap(&[A0], own(&[A0, A0]))], Vec::new())
                }
                "insert" if map => entry(
                    Mutable,
                    vec![tap(&[A0], own(&[A0, A1]))],
                    vec![tap(&[A0], argument(0, &[])), tap(&[A1], argument(1, &[]))],
                ),
                "insert" => entry(Mutable, Vec::new(), vec![tap(&[A0], argument(0, &[]))]),
                "extend" | "push" => entry(Mutable, Vec::new(), vec![tap(&[], argument(0, &[O]))]),
                "clone" => entry(Shared, vec![tap(&[], own(&[A0]))], Vec::new()),
                "into_iter" if references > 0 => {
                    let mut taps = vec![tap(&[O], own(&[O]))];
                    taps.extend(entry_ref(&[A0]));
                    entry(Owned, taps, Vec::new())
                }
                // A map's `(K, V)` items are where its arguments are.
                "into_iter" if map => entry(Owned, vec![tap(&[A0], own(&[]))], Vec::new()),
                "into_iter" | "into_keys" => entry(Owned, vec![tap(&[A0], own(&[A0]))], Vec::new()),
                "into_values" => entry(Owned, vec![tap(&[A0], own(&[A1]))], Vec::new()),
                _ => None,
            }
        }
        Ty::Option(_) | Ty::Result(..) => {
            let result = matches!(ty, Ty::Result(..));
            // A `Result`'s error stays where it is.
            let error = |mut taps: Vec<Tap>| {
                if result {
                    taps.push(tap(&[A1], own(&[A1])));
                }
                taps
            };
            match member {
                "unwrap" | "expect" | "unwrap_or_default" | "unwrap_unchecked" | "flatten" => {
                    entry(Owned, vec![tap(&[], own(&[A0]))], Vec::new())
                }
                "unwrap_err" | "expect_err" => entry(Owned, vec![tap(&[], own(&[A1]))], Vec::new()),
                "ok" => entry(Owned, vec![tap(&[A0], own(&[A0]))], Vec::new()),
                "err" => entry(Owned, vec![tap(&[A0], own(&[A1]))], Vec::new()),
                "filter" | "inspect" => entry(Owned, vec![tap(&[], own(&[]))], Vec::new()),
                "unwrap_or" => entry(
                    Owned,
                    vec![tap(&[], own(&[A0])), tap(&[], argument(0, &[]))],
                    Vec::new(),
                ),
                "ok_or" => entry(
                    Owned,
                    vec![tap(&[A0], own(&[A0])), tap(&[A1], argument(0, &[]))],
                    Vec::new(),
                ),
                "or" | "xor" => entry(
                    Owned,
                    vec![tap(&[], own(&[])), tap(&[], argument(0, &[]))],
                    Vec::new(),
                ),
                "zip" => entry(
                    Owned,
                    vec![
                        tap(&[A0, A0], own(&[A0])),
                        tap(&[A0, A1], argument(0, &[A0])),
                    ],
                    Vec::new(),
                ),
                "and" => entry(Owned, vec![tap(&[], argument(0, &[]))], Vec::new()),
                "as_ref" | "as_mut" => {
                    let mut taps = reference(&[A0], &[A0]);
                    if result {
                        taps.extend(reference(&[A1], &[A1]));
                    }
                    entry(
                        if member == "as_ref" { Shared } else { Mutable },
                        taps,
                        Vec::new(),
                    )
                }
                "as_deref" => entry(Shared, all(), Vec::new()),
                "as_deref_mut" => entry(Mutable, all(), Vec::new()),
                "iter" => entry(Shared, iter(&[A0]), Vec::new()),
                "iter_mut" => entry(Mutable, iter(&[A0]), Vec::new()),
                "get_or_insert" | "insert" => entry(
                    Mutable,
                    vec![tap(&[O], own(&[O])), tap(&[A0], own(&[A0, A0]))],
                    vec![tap(&[A0], argument(0, &[]))],
                ),
                // What it stores is its closure's value.
                "get_or_insert_with" => entry(
                    Mutable,
                    vec![tap(&[O], own(&[O])), tap(&[A0], own(&[A0, A0]))],
                    vec![tap(&[A0], call(0))],
                ),
                "take" => entry(Mutable, vec![tap(&[], own(&[A0]))], Vec::new()),
                "replace" => entry(
                    Mutable,
                    vec![tap(&[], own(&[A0]))],
                    vec![tap(&[A0], argument(0, &[]))],
                ),
                "copied" | "cloned" => {
                    entry(Owned, error(vec![tap(&[A0], own(&[A0, A0]))]), Vec::new())
                }
                "map" => entry(Owned, error(vec![tap(&[A0], call(0))]), Vec::new()),
                "and_then" => entry(Owned, error(vec![tap(&[], call(0))]), Vec::new()),
                "unwrap_or_else" => entry(
                    Owned,
                    vec![tap(&[], own(&[A0])), tap(&[], call(0))],
                    Vec::new(),
                ),
                "map_or" => entry(
                    Owned,
                    vec![tap(&[], argument(0, &[])), tap(&[], call(1))],
                    Vec::new(),
                ),
                "map_or_else" => entry(
                    Owned,
                    vec![tap(&[], call(0)), tap(&[], call(1))],
                    Vec::new(),
                ),
                "ok_or_else" | "map_err" => entry(
                    Owned,
                    vec![tap(&[A0], own(&[A0])), tap(&[A1], call(0))],
                    Vec::new(),
                ),
                "or_else" => entry(
                    Owned,
                    vec![tap(&[], own(&[])), tap(&[], call(0))],
                    Vec::new(),
                ),
                _ => None,
            }
        }
        // An adapter holds the iterator it takes, and its closure; a method
        // whose value is an item holds the item's regions only.
        Ty::Iter(_) => {
            let closure = |item: Vec<Tap>| {
                let mut taps = vec![tap(&[O], own(&[O])), tap(&[O], argument(0, &[O]))];
                taps.extend(item);
                taps
            };
            match member {
                "rev" | "skip" | "take" | "step_by" | "peekable" | "fuse" | "cycle"
                | "into_iter" => entry(Owned, vec![tap(&[], own(&[]))], Vec::new()),
                "by_ref" => entry(Mutable, vec![tap(&[], own(&[]))], Vec::new()),
                "enumerate" => entry(
                    Owned,
                    vec![tap(&[O], own(&[O])), tap(&[A0, A1], own(&[A0]))],
                    Vec::new(),
                ),
                "copied" | "cloned" => entry(
                    Owned,
                    vec![tap(&[O], own(&[O])), tap(&[A0], own(&[A0, A0]))],
                    Vec::new(),
                ),
                "filter" | "take_while" | "skip_while" | "inspect" => {
                    entry(Owned, closure(vec![tap(&[A0], own(&[A0]))]), Vec::new())
                }
                "map" | "filter_map" | "flat_map" | "map_while" => {
                    entry(Owned, closure(vec![tap(&[A0], call(0))]), Vec::new())
                }
                "flatten" => entry(Owned, vec![tap(&[], own(&[O]))], Vec::new()),
                "zip" => entry(
                    Owned,
                    vec![
                        tap(&[O], own(&[O])),
                        tap(&[O], argument(0, &[O])),
                        tap(&[A0, A0], own(&[A0])),
                        tap(&[A0, A1], argument(0, &[O])),
                    ],
                    Vec::new(),
                ),
                "chain" => entry(
                    Owned,
                    vec![
                        tap(&[O], own(&[O])),
                        tap(&[O], argument(0, &[O])),
                        tap(&[A0], own(&[A0])),
                        tap(&[A0], argument(0, &[O])),
                    ],
                    Vec::new(),
                ),
                "scan" => entry(
                    Owned,
                    vec![
                        tap(&[], own(&[O])),
                        tap(&[], argument(0, &[O])),
                        tap(&[], argument(1, &[O])),
                    ],
                    Vec::new(),
                ),
                "next" | "next_back" | "nth" | "nth_back" | "find" => {
                    entry(Mutable, vec![tap(&[A0], own(&[A0, A0]))], Vec::new())
                }
                "find_map" => entry(Mutable, vec![tap(&[A0], call(0))], Vec::new()),
                "peek" => entry(Mutable, reference(&[A0], &[A0]), Vec::new()),
                "last" | "max" | "min" | "max_by_key" | "min_by_key" | "max_by" | "min_by" => {
                    entry(Owned, vec![tap(&[A0], own(&[A0]))], Vec::new())
                }
                "collect" => {
                    let taps = match output {
                        Ty::Array { .. } => vec![tap(&[A0], own(&[A0]))],
                        Ty::Keyed(_, arguments) if map(arguments) => {
                            vec![tap(&[A0], own(&[A0, A0])), tap(&[A1], own(&[A0, A1]))]
                        }
                        Ty::Keyed(..) => vec![tap(&[A0], own(&[A0]))],
                        // Made of the items, wherever they are in it.
                        _ => vec![tap(&[], own(&[A0, O]))],
                    };
                    entry(Owned, taps, Vec::new())
                }
                _ => None,
            }
        }
        _ => None,
    }
}
