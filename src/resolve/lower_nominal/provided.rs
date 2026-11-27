//! A crate trait's provided method whose signature can't cross, run from
//! the trait's own source where the impl rustc selects leaves it to the
//! trait (D1118, #2698), as iced's `Widget::overlay` is for a native
//! `Text`. The opened trait module holds each provided method the program
//! reaches as a generic function over the trait's `Self`
//! (`crate_source::provided`), and the call is that function's, given the
//! receiver as its method's receiver takes it.

use super::*;

impl<'a> Parser<'a> {
    /// `value.member(..)` of a type whose impl of a crate trait leaves the
    /// trait's provided `member` to it and has no item of it that crosses:
    /// the trait's own body, from its source. `Err(value)` back when no
    /// opened trait's provided method of the name is the type's, or more
    /// than one is.
    pub(super) fn provided_body_call(
        &mut self,
        value: ParsedExpr,
        member: &str,
        member_span: Span,
    ) -> Result<Result<ParsedExpr, ParsedExpr>, Error> {
        if !matches!(self.peek(), Token::LeftParen | Token::Unit) {
            return Ok(Err(value));
        }
        let receiver = match self.inference.resolve(&value.ty) {
            Ty::Reference { target, .. } => self.inference.resolve(&target),
            ty => ty,
        };
        let templates = self.provided_templates(member);
        let mut found = templates
            .into_iter()
            .filter(|(trait_path, _)| self.leaves_to_trait(&receiver, trait_path, member));
        let (Some((_, function)), None) = (found.next(), found.next()) else {
            return Ok(Err(value));
        };
        let (arguments, end) = self.parse_borrowing_arguments("method arguments")?;
        let mut value = value;
        self.provided_receiver(&mut value, &function)?;
        let mut inputs = Vec::with_capacity(arguments.len() + 1);
        inputs.push(value);
        inputs.extend(arguments);
        self.pending_arguments = Some((inputs, end));
        let name = function.definition.name.text.clone();
        self.parse_script_call(name, Some(function), member_span)
            .map(Ok)
    }

    /// Each opened trait's provided method `member` as its function
    /// (`crate_source::provided`), with the trait's path, which bounds the
    /// function's `Self`.
    fn provided_templates(&self, member: &str) -> Vec<(ImmutableString, ScriptFunction)> {
        self.models
            .functions_by_key
            .values()
            .filter_map(|function| {
                let bound = function
                    .definition
                    .generics
                    .iter()
                    .find(|parameter| {
                        parameter.name.text.as_str()
                            == crate::package::crate_source::provided::SELF_TYPE
                    })?
                    .native_traits
                    .first()?;
                let trait_name = bound.rsplit("::").next()?;
                let written = crate::package::crate_source::provided(trait_name, member);
                (function.definition.name.text.as_str() == written)
                    .then(|| (bound.clone(), function.clone()))
            })
            .collect()
    }

    /// Whether `ty`'s impl of the crate trait `trait_path` leaves its
    /// provided `member` to the trait: a crate type's impl that doesn't
    /// override it (`ImplHeader::defaulted`), or the program's own impl
    /// for a program type, which gave no method of the name.
    fn leaves_to_trait(&mut self, ty: &Ty, trait_path: &str, member: &str) -> bool {
        let Some(header) = crate::host::path::Path::parse(trait_path)
            .ok()
            .and_then(|path| self.models.host.trait_header(&path))
            .cloned()
        else {
            return false;
        };
        match ty {
            Ty::Host { .. } => {
                let host = self.models.host.clone();
                host.owned_impl_headers(header.identity)
                    .filter(|(implementation, _)| {
                        implementation
                            .defaulted
                            .iter()
                            .any(|name| name.as_str() == member)
                    })
                    .any(|(implementation, declaring)| {
                        let checkpoint = self.inference.checkpoint();
                        let arguments = header
                            .parameters
                            .iter()
                            .map(|_| self.inference.fresh())
                            .collect::<Vec<_>>();
                        let fits = self
                            .apply_impl(implementation, ty, &arguments, declaring)
                            .is_some();
                        self.inference.rollback(checkpoint);
                        fits
                    })
            }
            Ty::Script(..) => {
                let Some(owner) = ty.script() else {
                    return false;
                };
                let known = self.trait_known_as(trait_path);
                self.models.script_impls.iter().any(|candidate| {
                    matches!(&candidate.owner, crate::resolve::type_ref::Type::Path(
                        crate::resolve::type_ref::Path {
                            target: crate::resolve::type_ref::Target::Script { definition, .. },
                            ..
                        }
                    ) if *definition == owner)
                        && known.is_some()
                        && self.trait_known_as(&candidate.path) == known
                })
            }
            // Rawr's string, whose trait impl for `&str` leaves it, as
            // iced's `impl Widget<..> for &str` leaves `overlay`.
            ty if ty.is_text() => {
                let host = self.models.host.clone();
                host.owned_impl_headers(header.identity)
                    .filter(|(implementation, _)| {
                        implementation
                            .defaulted
                            .iter()
                            .any(|name| name.as_str() == member)
                    })
                    .any(|(implementation, declaring)| {
                        let checkpoint = self.inference.checkpoint();
                        let mut parameters = implementation
                            .parameters
                            .iter()
                            .map(|name| (name.clone(), self.inference.fresh()))
                            .collect();
                        let text = self
                            .instantiate_host_type(
                                &implementation.self_type,
                                &mut parameters,
                                "impl",
                                Span::NONE,
                                declaring,
                                None,
                            )
                            .is_ok_and(|self_type| self.inference.resolve(&self_type).is_text());
                        self.inference.rollback(checkpoint);
                        text
                    })
            }
            _ => false,
        }
    }

    /// `value` as the first argument of the provided method `function`:
    /// borrowed for a `&self` or `&mut self` receiver, as a method call
    /// autorefs it, and taken for a `self` one, as a move or copy does.
    fn provided_receiver(
        &mut self,
        value: &mut ParsedExpr,
        function: &ScriptFunction,
    ) -> Result<(), Error> {
        let borrowed = match function
            .definition
            .parameters
            .first()
            .map(|input| &input.ty)
        {
            Some(crate::resolve::type_ref::Type::Reference { mutable, .. }) => Some(*mutable),
            _ => None,
        };
        let resolved = self.inference.resolve(&value.ty);
        match borrowed {
            Some(_) if matches!(resolved, Ty::Reference { .. }) => Ok(()),
            // `&mut place`, which the call stores back, as a `&mut`
            // argument written in place is.
            Some(true)
                if value.lvalue.is_some() || self.mutable_receiver_place(value).is_some() =>
            {
                if value.lvalue.is_none() {
                    value.lvalue = self.mutable_receiver_place(value);
                }
                let whole = value.span;
                if let Some(error) = self.immutable_borrow(value, whole) {
                    return Err(error);
                }
                let inner = self.may_hold_borrow(&resolved);
                let reference = borrows::is_reference(&resolved);
                let loan = self.flow().borrow(
                    value.span.start(),
                    value.span,
                    whole,
                    true,
                    inner,
                    reference,
                );
                self.reborrowed(loan);
                value.ty = Ty::Reference {
                    mutable: true,
                    target: Box::new(resolved),
                };
                Ok(())
            }
            Some(mutable) => {
                self.borrow_receiver(value, mutable);
                Ok(())
            }
            None => self.move_local(value),
        }
    }
}
