//! The lowering's side of the borrow checker (D572): what each construct
//! records in the body's facts (`borrowck::record`), and the check once
//! the body is read. Rawr has no references at runtime: a shared borrow is
//! a copy and a `&mut` local is its place, which matches Rust exactly when
//! the borrows are the ones rustc accepts. What this records ends each
//! borrow where stable rustc's NLL does, through branches and loops.

use super::*;
use crate::resolve::borrowck::{
    self,
    record::{Recorder, ValueStart},
    Projection,
};

impl Parser<'_> {
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

    /// Whether matching on a value of type `ty` reads a discriminant: an
    /// enum's, through any references.
    pub(super) fn has_discriminant(&self, ty: &Ty) -> bool {
        match self.inference.resolve(ty) {
            Ty::Reference { target, .. } => self.has_discriminant(&target),
            Ty::Option(_) | Ty::Result(..) | Ty::Host { .. } => true,
            Ty::Script(id, _) => matches!(self.models.definition(id), Nominal::Enum { .. }),
            _ => false,
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
