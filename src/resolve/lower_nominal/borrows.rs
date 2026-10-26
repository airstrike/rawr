//! The lowering's side of the borrow checker (D572): what each construct
//! records in the body's facts (`borrowck::record`), and the check once
//! the body is read. Rawr has no references at runtime: a shared borrow is
//! a copy and a `&mut` local is its place, which matches Rust exactly when
//! the borrows are the ones rustc accepts. What this records ends each
//! borrow where stable rustc's NLL does, through branches and loops.

use super::*;
use crate::resolve::borrowck::{
    self,
    record::{Matched, Recorder, ValueStart},
    Access, Origin, Place, Projection,
};

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
        self.flow()
            .matched(subject.span.start(), subject.span, place, start)
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
        let holder = matches!(self.inference.resolve(ty), Ty::Reference { .. });
        self.flow().mention(name, declaration, holder, span);
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

    /// `r = ..` of a `&mut` local `r`, which is its place at run time: a
    /// new borrow for it would write the place instead.
    pub(super) fn refuse_rebound_alias(
        &self,
        target: Span,
        ty: &Ty,
        lvalue: &Lvalue,
        replacement: &ParsedExpr,
    ) -> Result<(), Error> {
        let Some((name, declaration)) = self.flow().mentioned_local(target.start()) else {
            return Ok(());
        };
        let alias = self
            .aliases
            .get(name.as_str())
            .is_some_and(|(aliased, _)| *aliased == declaration);
        // `r = r` keeps the place it is.
        let same = |other: &Lvalue| match (lvalue, other) {
            (
                Lvalue::Place { binding, place, .. },
                Lvalue::Place {
                    binding: other,
                    place: other_place,
                    ..
                },
            ) => binding == other && place.as_str() == other_place.as_str(),
            _ => false,
        };
        if alias
            && matches!(self.inference.resolve(ty), Ty::Reference { .. })
            && !replacement.lvalue.as_ref().is_some_and(same)
        {
            return Err(Error::UnsupportedNominalSyntax {
                construct: format!(
                    "giving the `&mut` local `{name}` a new borrow: not supported in Rawr yet"
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
        if !compound
            && let Lvalue::Binding { binding, .. } = lvalue
            && let Some(local) = self.bindings.get(binding)
            && matches!(self.inference.resolve(&local.ty), Ty::Reference { .. })
            && matches!(self.inference.resolve(target_ty), Ty::Reference { .. })
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
        self.flow().escapes(replacement.span, start, holds);
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
