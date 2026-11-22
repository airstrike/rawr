//! A `&mut` stored in a value (D1113): a struct's field, a tuple's member,
//! an `Option`'s `Some`, an element of an array or a `vec!`. A `&mut`
//! local that stays a local is the place it borrows, or a runtime local
//! holding where that place is (`reborrow.rs`), and costs nothing more.
//! One that enters a value is a runtime reference instead, which reaches
//! its place from any frame: the shared handle of the place's local
//! (`__rawr_ref`, made shared where it is, as a closure's capture makes
//! it) and the path below it.
//!
//! Reading the field, the member or the element gives that reference. A
//! use of what it refers to (`*s.a = 1`, `s.a.push(2)`, `w.out.len`)
//! holds it in a hidden local of the statement, and reaches the place
//! through that local, as a `&mut` local given new borrows reaches its
//! place (`__rawr_through_h`).

use super::*;
use crate::engine::{KEYWORD_REF, KEYWORD_THROUGH};

/// The role of the hidden local holding a stored `&mut` a use reaches
/// its place through (`through_stored`).
const STORED: &str = "stored";

impl Parser<'_> {
    /// Read the value a value stores, with `read`: `&mut place` may be
    /// written there, and a `&mut` the value is becomes a reference that
    /// reaches its place from anywhere (`store_borrow`).
    pub(super) fn parse_stored(
        &mut self,
        read: impl FnOnce(&mut Self) -> Result<ParsedExpr, Error>,
    ) -> Result<ParsedExpr, Error> {
        let mut value = self.parse_borrowable(read)?;
        self.store_borrow(&mut value)?;
        Ok(value)
    }

    /// Read a value with `read`, where `&mut place` may be written: the
    /// value itself, as `&mut place` is read for a `&mut` argument.
    pub(super) fn parse_borrowable(
        &mut self,
        read: impl FnOnce(&mut Self) -> Result<ParsedExpr, Error>,
    ) -> Result<ParsedExpr, Error> {
        let borrowing =
            matches!(self.peek(), Token::Ampersand) && matches!(self.peek_second(), Token::Mut);
        let outer = self.mutable_borrow;
        if borrowing {
            self.mutable_borrow = Some(self.expression_stack.len() + 1);
        }
        let value = read(self);
        self.mutable_borrow = outer;
        value
    }

    /// `value`, stored in a value: a `&mut` becomes the reference to its
    /// place (`__rawr_ref`), and an `Option<&mut T>` that names its
    /// payload's place (`v.last_mut()`) is `Some` of one. A value that
    /// already holds references, and any other value, stays as it is.
    pub(super) fn store_borrow(&mut self, value: &mut ParsedExpr) -> Result<(), Error> {
        match self.inference.resolve(&value.ty) {
            Ty::Reference { mutable: true, .. } => self.store_reference(value),
            Ty::Option(element)
                if matches!(
                    self.inference.resolve(&element),
                    Ty::Reference { mutable: true, .. }
                ) =>
            {
                self.store_optional(value)
            }
            _ => Ok(()),
        }
    }

    /// `value`, a `&mut` stored in a value: the reference to its place.
    fn store_reference(&mut self, value: &mut ParsedExpr) -> Result<(), Error> {
        if value.lvalue.is_none() {
            // A crate's own `&mut` value, or a temporary's: as before.
            return Ok(());
        }
        let refused = |construct: &str, at: Span| Error::UnsupportedNominalSyntax {
            construct: format!("{construct}: not supported in Rawr yet").into(),
            source: at,
        };
        if let Some(lvalue) = &value.lvalue
            && self.copied_parameter(lvalue.binding())
        {
            return Err(refused(
                "a `&mut` parameter's place kept in a value",
                value.span,
            ));
        }
        if !value.prelude.is_empty() {
            return Err(refused(
                "a `&mut` of this place kept in a value",
                value.span,
            ));
        }
        let Some((path, _)) = self.reborrow_source(value) else {
            return Err(refused(
                "a `&mut` of this place kept in a value",
                value.span,
            ));
        };
        value.source = origin::Fragment::call(KEYWORD_REF, [origin::Fragment::plain(path)]);
        value.source.cover(value.span);
        value.lvalue = None;
        Ok(())
    }

    /// `value`, an `Option<&mut T>` stored in a value: `Some` of the
    /// reference to the place its payload names, or `None`.
    fn store_optional(&mut self, value: &mut ParsedExpr) -> Result<(), Error> {
        let Some(lvalue) = value.lvalue.clone() else {
            return Ok(());
        };
        // An `Option` that holds its reference already (a local, a field).
        if self.inference.resolve(lvalue.ty()) == self.inference.resolve(&value.ty) {
            return Ok(());
        }
        let refused = Error::UnsupportedNominalSyntax {
            construct: "an `Option<&mut T>` of this place kept in a value: not supported in \
                        Rawr yet"
                .into(),
            source: value.span,
        };
        if self.in_async_body() || self.copied_parameter(lvalue.binding()) {
            return Err(refused);
        }
        let Some(located) = self.place_located(&lvalue) else {
            return Err(refused);
        };
        let option = self.temporary("option");
        let mut source = origin::Fragment::plain(format!("{{ let {option} = "));
        source.push(value.rendered_source());
        source.push_text("; if ");
        let some = self.prelude_op(PreludeOp::IsSome, [origin::Fragment::plain(&option)]);
        source.push(some);
        source.push_text(" { ");
        let reference = origin::Fragment::call(KEYWORD_REF, [located]);
        source.push(self.prelude_op(PreludeOp::Some, [reference]));
        source.push_text(format!(" }} else {{ {option} }} }}"));
        value.source = source.surrounded("(", ")");
        value.source.cover(value.span);
        value.prelude = origin::Fragment::default();
        value.lvalue = None;
        Ok(())
    }

    /// Whether `binding` is a function's `&mut` parameter, a copy the call
    /// stores back when it ends, which no reference may outlive.
    fn copied_parameter(&self, binding: &str) -> bool {
        self.bindings.get(binding).is_some_and(|local| {
            self.parameter_declarations.contains(&local.declaration)
                && matches!(
                    self.inference.resolve(&local.ty),
                    Ty::Reference { mutable: true, .. }
                )
        })
    }

    /// Whether `value`, dereferenced (`*s.a = 1`), reads a `&mut` a value
    /// stores: a field, a member or an element of a `&mut` type, whose
    /// runtime value is the reference. A field, a member or an element
    /// read and used in a chain is that place already (`parse_postfix_steps`).
    pub(super) fn stored_holder(&self, value: &ParsedExpr) -> bool {
        let ty = self.inference.resolve(&value.ty);
        if !matches!(ty, Ty::Reference { mutable: true, .. }) {
            return false;
        }
        // A field is the struct's own, of the type its definition gives.
        // A member or an element is one of a local whose type holds a
        // `&mut` (a tuple of them, a `Vec<&mut T>`): one an alias names
        // is a place of the referent's own type.
        let holds = |binding: &str| {
            self.bindings.get(binding).is_some_and(|local| {
                let local = self.inference.resolve(&local.ty);
                let inner = match &local {
                    Ty::Reference { target, .. } => &**target,
                    local => local,
                };
                super::ty::contains(inner, &|part| {
                    matches!(part, Ty::Reference { mutable: true, .. })
                })
            })
        };
        match &value.lvalue {
            Some(Lvalue::Field(field)) => self.inference.resolve(&field.ty) == ty,
            Some(Lvalue::Place {
                binding,
                place,
                ty: held,
            }) => {
                !place.as_str().starts_with(KEYWORD_THROUGH)
                    && place.as_str().ends_with(']')
                    && self.inference.resolve(held) == ty
                    && holds(binding)
            }
            Some(Lvalue::Nested(nested)) => {
                !nested.base.as_str().starts_with(KEYWORD_THROUGH)
                    && matches!(
                        nested.steps.last(),
                        Some(PlaceStep::Field { .. } | PlaceStep::Member(_) | PlaceStep::Index(_))
                    )
                    && self.inference.resolve(&nested.ty) == ty
                    && holds(&nested.binding)
            }
            _ => false,
        }
    }

    /// `value`, whose runtime value is a stored `&mut` (`stored_holder`,
    /// or an `Option`'s payload taken out): the place it refers to, held
    /// in a hidden local of the statement and reached through it. The
    /// local is given the reference where the place is first reached.
    pub(super) fn through_stored(&mut self, mut value: ParsedExpr) -> ParsedExpr {
        let Ty::Reference {
            mutable: true,
            target,
        } = self.inference.resolve(&value.ty)
        else {
            return value;
        };
        let holder: ImmutableString = self.temporary(STORED).into();
        self.field_entries.push((
            origin::Fragment::plain(format!("let {holder} = (); ")),
            origin::Fragment::default(),
        ));
        self.bindings.insert(
            holder.clone(),
            Binding {
                ty: value.ty.clone(),
                mutable: true,
                available: true,
                declaration: value.span,
                kind: BindingKind::Local,
            },
        );
        let mut setup = origin::Fragment::plain(format!("{holder} = "));
        setup.push(value.rendered_source());
        setup.push_text(";");
        let through = format!("{KEYWORD_THROUGH}{holder}");
        let mut source = setup.clone().surrounded("({ ", " ");
        source.push_text(format!("{through} }})"));
        value.source = source;
        value.source.cover(value.span);
        value.prelude = origin::Fragment::default();
        value.lvalue = Some(Lvalue::Nested(Box::new(lvalue::Nested {
            binding: holder,
            base: origin::Fragment::plain(through),
            steps: vec![PlaceStep::Setup(setup)],
            ty: *target,
        })));
        value
    }
}

impl Parser<'_> {
    /// Whether `place` is reached through a stored `&mut` a hidden local
    /// holds (`through_stored`), whose path starts at its handle.
    pub(super) fn stored_place(place: &Lvalue) -> bool {
        let Lvalue::Nested(nested) = place else {
            return false;
        };
        nested
            .base
            .as_str()
            .strip_prefix(KEYWORD_THROUGH)
            .and_then(|holder| holder.strip_prefix("__rawr_"))
            .and_then(|holder| holder.strip_prefix(STORED))
            .is_some_and(|rest| rest.starts_with('_'))
    }

    /// Whether `subject`, matched by a pattern, is a value that holds
    /// stored `&mut`s: a local, a field or an element whose type has a
    /// `&mut` below it (`Option<&mut T>`, `(&mut A, &mut B)`). Its `&mut`
    /// bindings are the references themselves, moved out of it, not parts
    /// of it (`bind_stored`). An `Option<&mut T>` that names its payload's
    /// place (`v.get_mut(i)`) is matched on that place.
    pub(super) fn holds_stored(&self, subject: &ParsedExpr) -> bool {
        let ty = self.inference.resolve(&subject.ty);
        if matches!(ty, Ty::Reference { .. }) {
            return false;
        }
        let Some(lvalue) = &subject.lvalue else {
            return false;
        };
        self.inference.resolve(lvalue.ty()) == ty
            && super::ty::contains(&ty, &|part| {
                matches!(part, Ty::Reference { mutable: true, .. })
            })
    }

    /// The bindings of a pattern matched on a value that holds stored
    /// `&mut`s (`holds_stored`): each `&mut` one is the reference moved
    /// out, a `&mut` local reaching its place through it (`reborrow.rs`).
    pub(super) fn bind_stored(&mut self, stored: bool, bindings: &[PatternBinding]) {
        if !stored {
            return;
        }
        for binding in bindings {
            if matches!(
                self.inference.resolve(&binding.ty),
                Ty::Reference { mutable: true, .. }
            ) {
                self.select_local(
                    binding.name.clone(),
                    binding.span,
                    binding.ty.clone(),
                    binding.mutable,
                );
            }
        }
    }
}
