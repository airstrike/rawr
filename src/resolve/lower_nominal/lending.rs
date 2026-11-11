//! `s.as_ref()` and `s.borrow()` of a native value: the method of a std
//! trait that lends one of the receiver's views, `AsRef<T>` or
//! `Borrow<T>`, whose `T` the use fixes, as rustc infers the trait's
//! argument. The call waits with the pending `.into()`s and is chosen by
//! the same pass (`finish_conversions`), once its target is known: std's
//! impls for the receiver say which targets it may lend, and rawr runs
//! the one the target names.

use super::*;

impl<'a> Parser<'a> {
    /// The std trait whose method `member`, called with no arguments, lends
    /// a view of the native value `receiver`: `AsRef` of a `String`'s
    /// `as_ref`. `None` where the receiver's type has its own method of
    /// that name, as an `Option`'s `as_ref`.
    pub(super) fn lending_call(&mut self, receiver: &Ty, member: &str) -> Option<&'static str> {
        if !matches!(self.peek(), Token::Unit) {
            return None;
        }
        std_method::lending_trait(without_references(receiver), member)
    }

    /// `value.member()` through the std trait `definition`
    /// (`lending_call`): a reference whose target the use gives, written
    /// once it is known (`lending_helper`).
    pub(super) fn parse_lending(
        &mut self,
        value: ParsedExpr,
        definition: &'static str,
        method: Span,
        end: Position,
    ) -> ParsedExpr {
        // `Self` is the type `&self` refers to: a `&str`'s is `str`.
        let source = match without_references(&self.inference.resolve(&value.ty)) {
            Ty::Str => Ty::UnsizedStr,
            ty => ty.clone(),
        };
        let expression = span(value.span.start(), end);
        let placeholder = conversion_placeholder(self.pending_conversions.len());
        let width = placeholder.len() + 1;
        let placeholder = format!("{placeholder:_<width$}");
        let target = self.inference.fresh();
        self.pending_conversions.push(PendingConversion {
            placeholder: placeholder.clone(),
            source,
            target: target.clone(),
            span: expression,
            host: None,
            try_from: None,
            lending: Some(Lending { definition, method }),
        });
        let mut rendered = origin::Fragment::call(&placeholder, [value.rendered_source()]);
        rendered.push_text(")");
        self.operator_expr(rendered, target, expression)
    }

    /// `Trait::method(text.as_ref(), ..)` of a crate's trait, spliced as a
    /// method call on the pending lending `receiver`: the target the
    /// trait's impl takes, as rustc selects the one impl whose self type
    /// unifies with `&_`. A crate trait's method is called on a reference
    /// only through its impl for `&str` (`surface::text`), so the target
    /// is `str` where std's impls lend it, and the trait's impl is asked
    /// for as any text receiver's is.
    #[cfg(feature = "rust-import")]
    pub(super) fn settle_spliced_lending(&mut self, receiver: &Ty) {
        let resolved = self.inference.resolve(receiver);
        if !matches!(resolved, Ty::Infer(_)) {
            return;
        }
        let Some((definition, source)) = self.pending_conversions.iter().find_map(|pending| {
            let lending = pending.lending?;
            (self.inference.resolve(&pending.target) == resolved)
                .then(|| (lending.definition, pending.source.clone()))
        }) else {
            return;
        };
        let checkpoint = self.inference.checkpoint();
        let lends_text =
            std_impl::holds(self, definition, &source, &[Ty::UnsizedStr], true, 0).is_ok();
        self.inference.rollback(checkpoint);
        if lends_text {
            self.unify_type(receiver, &Ty::Str);
        }
    }

    /// The helper that lends `conversion`'s view through its std trait
    /// (`lending`): none where the view is the value as rawr holds it (a
    /// `String`'s `str`, a `Vec`'s `[T]`), or a prelude op (a text's
    /// bytes). rustc's E0283 where no use fixed the target, and its E0277
    /// where std has no impl that lends it.
    pub(super) fn lending_helper(
        &mut self,
        lending: Lending,
        conversion: &PendingConversion,
    ) -> Result<String, Error> {
        let Lending { definition, method } = lending;
        let borrowed = match self.inference.resolve(&conversion.target) {
            Ty::Infer(_) => {
                return Err(Error::StdPath {
                    code: "E0283",
                    message: "type annotations needed".into(),
                    label: Some("type must be known at this point".into()),
                    note: None,
                    reference: conversion.span,
                });
            }
            Ty::Str => Ty::UnsizedStr,
            Ty::Reference {
                mutable: false,
                target,
            } => self.inference.resolve(&target),
            target => {
                return Err(Error::TypeMismatch {
                    expected: target.name(&self.models),
                    actual: "&_".into(),
                    expression: conversion.span,
                });
            }
        };
        let trait_name = std_method::trait_name(definition);
        let bound = format!("{trait_name}<{}>", borrowed.name(&self.models));
        let checkpoint = self.inference.checkpoint();
        // A literal still to be inferred takes the kind the target names,
        // as std's `impl<T> Borrow<T> for T` binds it.
        if matches!(
            self.inference.resolve(&conversion.source),
            Ty::Int(Int::Var(_)) | Ty::Float(Float::Var(_))
        ) {
            self.unify_type(&conversion.source, &borrowed);
        }
        let source = self.inference.resolve(&conversion.source);
        if std_impl::holds(
            self,
            definition,
            &source,
            std::slice::from_ref(&borrowed),
            true,
            0,
        )
        .is_err()
        {
            self.inference.rollback(checkpoint);
            // A type rawr holds through a facade, as std's `Path` or a
            // crate's type: std's or the crate's impl for it isn't one
            // rawr reads or runs yet.
            if matches!(borrowed, Ty::Host { .. }) {
                return Err(Error::NotYet {
                    path: format!("impl {bound} for {}", source.name(&self.models)).into(),
                    reference: conversion.span,
                });
            }
            let owner = source.name(&self.models);
            return Err(Error::StdPath {
                code: "E0277",
                message: format!("the trait bound `{owner}: {bound}` is not satisfied").into(),
                label: Some(format!("the trait `{bound}` is not implemented for `{owner}`").into()),
                note: None,
                reference: method,
            });
        }
        self.inference.commit(checkpoint);
        match runs_lending(&source, &self.inference.resolve(&borrowed)) {
            Some(op) => Ok(op
                .map(|op| self.push_op(Op::Prelude(op)))
                .unwrap_or_default()),
            None => Err(Error::NotYet {
                path: format!("impl {bound} for {}", source.name(&self.models)).into(),
                reference: conversion.span,
            }),
        }
    }
}

/// A pending `.as_ref()` or `.borrow()`: the std trait whose impl lends
/// its target, and the method's name, where rustc reports an impl it
/// lacks.
#[derive(Clone, Copy)]
pub(super) struct Lending {
    pub(super) definition: &'static str,
    pub(super) method: Span,
}

/// How rawr runs std's impl that lends `borrowed` of `source`, both
/// resolved (D711's execution mapping): the value as it is, where the two
/// have one runtime form, or a prelude op. `None` when rawr doesn't run
/// it yet. Which impls exist is the table's to say.
fn runs_lending(source: &Ty, borrowed: &Ty) -> Option<Option<PreludeOp>> {
    let text = |ty: &Ty| matches!(ty, Ty::String | Ty::Str | Ty::UnsizedStr);
    match (source, borrowed) {
        _ if source == borrowed => Some(None),
        (source, Ty::UnsizedStr) if text(source) => Some(None),
        // A text's bytes are an array of integers at runtime.
        (
            source,
            Ty::Array {
                kind: sequence::Kind::Slice,
                ..
            },
        ) if text(source) => Some(Some(PreludeOp::StrBytes)),
        // Every sequence is an array at runtime.
        (Ty::Array { .. }, Ty::Array { .. }) => Some(None),
        _ => None,
    }
}
