//! `std::borrow::Cow<'_, B>`, std's clone-on-write pointer: a borrowed `B`
//! or its owned form. Its impls (`Deref`, `Clone`, `PartialEq`, `Display`,
//! `From`) and `B`'s `ToOwned::Owned` come from std's table (D711); this
//! module is their execution mapping.
//!
//! At runtime a `Cow` is a [`Value`]: its variant and its payload. The
//! borrowed `B` and the owned value have one runtime form (a `&str` and a
//! `String` are one string, a `&[T]` and a `Vec<T>` one array), so `Deref`
//! and `into_owned` read the payload as it is, and `==`, ordering and
//! `Debug` of a `Cow` are its payload's, as std's impls are `B`'s.

use super::*;
use crate::std_table::{self, Namespace, Resolved};

/// std's definition of `Cow`.
pub(super) const DEFINITION: &str = "alloc::borrow::Cow";

/// A `Cow` at runtime.
#[derive(Clone, Debug)]
pub(crate) enum Value {
    Borrowed(Dynamic),
    Owned(Dynamic),
}

impl Value {
    /// The borrowed or owned value.
    pub(crate) fn payload(&self) -> &Dynamic {
        match self {
            Self::Borrowed(value) | Self::Owned(value) => value,
        }
    }

    fn into_payload(self) -> Dynamic {
        match self {
            Self::Borrowed(value) | Self::Owned(value) => value,
        }
    }
}

/// The payload of a runtime `Cow`, which `==`, ordering and `Debug` read in
/// its place; `None` for another value.
pub(super) fn payload(value: &Dynamic) -> Option<Dynamic> {
    value.read_lock::<Value>().map(|cow| cow.payload().clone())
}

/// `Cow::Borrowed(value)`, `Cow::Owned(value)`, and a `Cow`'s payload.
pub(super) fn run(op: PreludeOp, value: &mut Dynamic) -> RawrResult {
    Ok(match op {
        PreludeOp::CowBorrowed => Dynamic::from(Value::Borrowed(value.take())),
        PreludeOp::CowOwned => Dynamic::from(Value::Owned(value.take())),
        _ => value
            .take()
            .try_cast::<Value>()
            .map(Value::into_payload)
            .ok_or_else(|| invariant("a `Cow` operand is not one at runtime"))?,
    })
}

/// The type a `Cow<B>`'s payload is read as: `B`, but `&str` for `str`,
/// whose text is the same.
pub(super) fn payload_ty(borrowed: &Ty) -> Ty {
    match borrowed {
        Ty::UnsizedStr => Ty::Str,
        borrowed => borrowed.clone(),
    }
}

/// Whether `name` is a variant std declares on `Cow`: `Some(true)` for
/// `Owned`.
fn variant_named(name: &str) -> Option<bool> {
    let item = std_table::item(&format!("{DEFINITION}::{name}"))?;
    (item.kind == "variant").then(|| name == "Owned")
}

/// `&B`, a `Cow::Borrowed`'s payload: `&str` of `str`.
pub(super) fn reference_to(borrowed: Ty) -> Ty {
    match borrowed {
        Ty::UnsizedStr => Ty::Str,
        borrowed => Ty::Reference {
            mutable: false,
            target: Box::new(borrowed),
        },
    }
}

impl Parser<'_> {
    /// Whether `name`, written in a type, names std's `Cow`: its own
    /// spelling, unless the crate names a type `Cow`.
    pub(super) fn cow_named(&self, name: &str) -> bool {
        name == "Cow" && self.models.get(name).is_none()
    }

    /// `Cow<'a, B>` in a body, after the name.
    pub(super) fn parse_cow_type(&mut self) -> Result<Ty, Error> {
        self.expect(Token::LessThan, "'<' after `Cow`")?;
        if self.skip_lifetime() {
            self.expect(Token::Comma, "',' after `Cow`'s lifetime")?;
        }
        let borrowed = self.parse_local_type()?;
        self.expect_close_angle("'>' after `Cow<'_, B`")?;
        Ok(Ty::Cow(Box::new(borrowed)))
    }

    /// `<B as ToOwned>::Owned`, from std's impls: `String` of `str`,
    /// `Vec<T>` of `[T]`, `T` of a `Clone` `T`.
    pub(super) fn owned_of(&self, borrowed: &Ty) -> Option<Ty> {
        let mut probe = std_impl::Probe(self);
        let borrowed = self.inference.resolve(borrowed);
        let selected = std_impl::select(
            &mut probe,
            "alloc::borrow::ToOwned",
            &borrowed,
            &[],
            false,
            0,
        )
        .ok()?;
        std_impl::associated(&mut probe, &selected, "Owned")
    }

    /// The `B` of `Cow::Owned(value)` when nothing else names it: `value`'s
    /// `Deref` target when that target's `ToOwned::Owned` is `value`'s type
    /// (`str` of a `String`, `[T]` of a `Vec<T>`), else `value`'s own type.
    fn borrowed_of(&mut self, owned: &Ty) -> Ty {
        let owned = self.inference.resolve(owned);
        if let Some((target, std_impl::DerefCall::Value)) = self.deref_target(&owned, false)
            && self
                .owned_of(&target)
                .is_some_and(|back| self.unifies(&back, &owned))
        {
            return target;
        }
        owned
    }

    /// Whether the path `segments` names std's `Cow` as a type, resolved
    /// as rustc does: through the `use` that binds its first segment, or
    /// std's path; a bare `Cow` as [`Self::cow_named`] reads it.
    fn names_cow(&self, segments: &[&str]) -> bool {
        let [first, ..] = segments else {
            return false;
        };
        let written = segments.join("::");
        let qualified = match self.models.import(first) {
            Some(binding) => {
                crate::resolve::std_path::qualified(&written, Some(&binding.qualified))
            }
            None if segments.len() == 1 => return self.cow_named(first),
            None => crate::resolve::std_path::qualified(&written, None),
        };
        qualified.is_some_and(|qualified| {
            let segments = qualified.split("::").collect::<Vec<_>>();
            matches!(
                std_table::resolve(&segments, Namespace::Type),
                Ok(Resolved::Path(path)) if path.definition == DEFINITION
            )
        })
    }

    /// The variant `Cow::Borrowed` or `Cow::Owned` the path `segments`
    /// names: a path to `Cow` and a variant std declares on it.
    /// `Some(true)` for `Owned`.
    pub(super) fn cow_variant(&self, segments: &[&str]) -> Option<bool> {
        let [owner @ .., variant] = segments else {
            return None;
        };
        if owner.is_empty() || !self.names_cow(owner) {
            return None;
        }
        variant_named(variant)
    }

    /// The tokens ahead as a path to a member of `Cow` (`Cow::Borrowed`,
    /// `std::borrow::Cow::from`), after its first segment `name` (already
    /// read): the member, with its span, the path consumed. `None` leaves
    /// the input as it is.
    pub(super) fn cow_member_ahead(&mut self, name: &str) -> Option<(ImmutableString, Span)> {
        let mut ahead = self.input.clone();
        let mut segments = vec![name.to_owned()];
        let mut consumed = 0;
        let mut last = None;
        while let Some((Token::DoubleColon, _)) = ahead.next() {
            let Some((Token::Identifier(segment), at)) = ahead.next() else {
                break;
            };
            segments.push(segment.as_str().to_owned());
            consumed += 2;
            last = Some(span(at, text_end(at, &segment)));
        }
        let (member, owner) = segments.split_last()?;
        let owner = owner.iter().map(String::as_str).collect::<Vec<_>>();
        if owner.is_empty() || !self.names_cow(&owner) {
            return None;
        }
        for _ in 0..consumed {
            self.next();
        }
        Some((member.as_str().into(), last?))
    }

    /// `Cow::member` in a body, from `start` to the member: a variant's
    /// constructor, `Cow::from(value)`, or rustc's error for another.
    pub(super) fn parse_cow_associated(
        &mut self,
        member: &str,
        start: Span,
    ) -> Result<ParsedExpr, Error> {
        if let Some(owned) = variant_named(member) {
            return self.parse_cow_constructor(owned, start);
        }
        if member == "from" && matches!(self.peek(), Token::LeftParen) {
            return self.parse_cow_from(start);
        }
        if let Some(error) = crate::resolve::std_path::not_yet(
            &format!("std::borrow::Cow::{member}"),
            &format!("Cow::{member}"),
            Namespace::Value,
            start,
        ) {
            return Err(error);
        }
        Err(Error::UnknownMember {
            owner: "Cow".into(),
            member: member.into(),
            reference: start,
        })
    }

    /// `Cow::from(value)`: the `Cow` std's `From` of the value's type makes
    /// (`Cow<str>` of a `&str` or a `String`, `Cow<[T]>` of a `Vec<T>`).
    fn parse_cow_from(&mut self, start: Span) -> Result<ParsedExpr, Error> {
        let (mut arguments, end) = self.parse_call_arguments("`Cow::from` arguments")?;
        let expression = span(start.start(), end);
        let [value] = arguments.as_mut_slice() else {
            return Err(Error::argument_count(
                crate::resolve::Callee::AssociatedFunction,
                "Cow::from",
                1,
                arguments.len(),
                expression,
            ));
        };
        let source = self.inference.resolve(&value.ty);
        let target = Ty::Cow(Box::new(self.inference.fresh()));
        let checkpoint = self.inference.checkpoint();
        let selected = std_impl::select(
            self,
            "core::convert::From",
            &target,
            std::slice::from_ref(&source),
            true,
            0,
        );
        let Ok(_) = selected else {
            self.inference.rollback(checkpoint);
            return Err(Error::MissingCapability {
                owner: "Cow<'_, _>".into(),
                capability: format!("From<{}>", source.name(&self.models)).into(),
                operation: value.span,
                derive: false,
                required_by: None,
            });
        };
        self.inference.commit(checkpoint);
        let target = self.inference.resolve(&target);
        let Some(op) = std_impl::runs_from(&target, &source) else {
            return Err(Error::NotYet {
                path: format!(
                    "impl From<{}> for {}",
                    source.name(&self.models),
                    target.name(&self.models)
                )
                .into(),
                reference: expression,
            });
        };
        self.move_local(value)?;
        let mut rendered = value.rendered_source();
        if let Some(op) = op {
            rendered = self.prelude_op(op, [rendered]);
        }
        Ok(self.operator_expr(rendered, target, expression))
    }

    /// `Cow::Borrowed(value)` or `Cow::Owned(value)`, from `start`. Its `B`
    /// is the `Cow` the context expects, else the borrowed value's referent
    /// or the owned value's borrowed form.
    pub(super) fn parse_cow_constructor(
        &mut self,
        owned: bool,
        start: Span,
    ) -> Result<ParsedExpr, Error> {
        let variant = if owned { "Owned" } else { "Borrowed" };
        let hinted = self
            .collect_hint
            .as_ref()
            .filter(|(_, depth)| *depth == self.expression_stack.len())
            .and_then(|(expected, _)| match self.inference.resolve(expected) {
                Ty::Cow(borrowed) => Some(*borrowed),
                _ => None,
            });
        let (mut arguments, end) = self.parse_call_arguments("`Cow` variant arguments")?;
        let expression = span(start.start(), end);
        let [value] = arguments.as_mut_slice() else {
            return Err(Error::argument_count(
                crate::resolve::Callee::Variant,
                variant,
                1,
                arguments.len(),
                expression,
            ));
        };
        let given = self.inference.resolve(&value.ty);
        let borrowed = match (hinted, owned) {
            (Some(borrowed), _) => borrowed,
            (None, false) => match &given {
                Ty::Str => Ty::UnsizedStr,
                Ty::Reference {
                    mutable: false,
                    target,
                } => (**target).clone(),
                _ => {
                    return Err(Error::TypeMismatch {
                        expected: "&_".into(),
                        actual: given.name(&self.models),
                        expression: value.span,
                    })
                }
            },
            (None, true) => self.borrowed_of(&given),
        };
        let expected = if owned {
            self.owned_of(&borrowed)
                .ok_or_else(|| Error::MissingCapability {
                    owner: borrowed.name(&self.models),
                    capability: "ToOwned".into(),
                    operation: value.span,
                    derive: false,
                    required_by: Some("std::borrow::Cow".into()),
                })?
        } else {
            reference_to(borrowed.clone())
        };
        self.coerce_expr(&expected, value)?;
        self.move_local(value)?;
        let op = if owned {
            PreludeOp::CowOwned
        } else {
            PreludeOp::CowBorrowed
        };
        let source = self.prelude_op(op, [value.rendered_source()]);
        Ok(self.operator_expr(source, Ty::Cow(Box::new(borrowed)), expression))
    }

    /// A member of a `Cow` (`value`, through any references) that is the
    /// `Cow`'s own: `into_owned`, `to_mut` and `clone`, or rustc's error for
    /// another inherent item std declares (`is_owned` is unstable). `None`
    /// for any other member, which is its `Deref` target's.
    pub(super) fn cow_member(
        &mut self,
        value: &mut ParsedExpr,
        member: &str,
        member_span: Span,
    ) -> Result<Option<ParsedExpr>, Error> {
        let mut peeled = self.inference.resolve(&value.ty);
        while let Ty::Reference { target, .. } = peeled {
            peeled = self.inference.resolve(&target);
        }
        let Ty::Cow(borrowed) = peeled.clone() else {
            return Ok(None);
        };
        let call = matches!(self.peek(), Token::Unit);
        match member {
            "into_owned" | "clone" if call => {
                let end = self.next().1;
                let expression = span(value.span.start(), end);
                if member == "clone" {
                    if !self.type_provides(&peeled, capability::Kind::Clone) {
                        return Err(Error::MissingCapability {
                            owner: peeled.name(&self.models),
                            capability: "Clone".into(),
                            operation: member_span,
                            derive: false,
                            required_by: None,
                        });
                    }
                    return Ok(Some(self.operator_expr(
                        value.rendered_source(),
                        peeled,
                        expression,
                    )));
                }
                let owned = self.owned_of(&borrowed).unwrap_or(Ty::Unknown);
                self.move_local(value)?;
                let source = self.prelude_op(PreludeOp::CowValue, [value.rendered_source()]);
                Ok(Some(self.operator_expr(source, owned, expression)))
            }
            "to_mut" if call => {
                let end = self.next().1;
                let owned = self.owned_of(&borrowed).unwrap_or(Ty::Unknown);
                self.cow_to_mut(value.clone(), owned, span(value.span.start(), end))
                    .map(Some)
            }
            _ => {
                let Some(item) = std_table::item(&format!("{DEFINITION}::{member}")) else {
                    return Ok(None);
                };
                // A function of `Cow`'s own with no `self` (`is_owned`) is no
                // method (rustc's E0599); a method is rustc's or rawr's
                // error for it.
                if std_method::mode_of(&item).is_none() {
                    return Err(Error::UnknownMember {
                        owner: peeled.name(&self.models),
                        member: member.into(),
                        reference: member_span,
                    });
                }
                Err(crate::resolve::std_path::not_yet(
                    &format!("std::borrow::Cow::{member}"),
                    &format!("Cow::{member}"),
                    Namespace::Value,
                    member_span,
                )
                .unwrap_or_else(|| Error::UnknownMember {
                    owner: peeled.name(&self.models),
                    member: member.into(),
                    reference: member_span,
                }))
            }
        }
    }

    /// `cow.to_mut()` of a place: a `&mut` place whose read is the payload
    /// and whose store makes the `Cow` owned of the value stored, as std's
    /// `to_mut` clones a borrowed value into an owned one.
    fn cow_to_mut(
        &mut self,
        receiver: ParsedExpr,
        owned: Ty,
        expression: Span,
    ) -> Result<ParsedExpr, Error> {
        let Some(lvalue) = receiver.lvalue.clone() else {
            return Err(self.unsupported("`to_mut` of a `Cow` that is not a place"));
        };
        let place = self.temporary("place");
        let stored = self.temporary("stored");
        let mut read = origin::Fragment::plain("{ ");
        read.push(self.prelude_op(
            PreludeOp::CowValue,
            [origin::Fragment::plain(place.clone())],
        ));
        read.push_text(" }");
        let mut store = origin::Fragment::plain("{ ");
        store.push(self.prelude_op(
            PreludeOp::CowOwned,
            [origin::Fragment::plain(stored.clone())],
        ));
        store.push_text(" }");
        let step = PlaceStep::Accessor {
            place: place.clone(),
            stored,
            read: read.clone(),
            store,
        };
        let lvalue = match &lvalue {
            Lvalue::Binding { binding, .. } => Lvalue::Nested(Box::new(NestedLvalue {
                binding: binding.clone(),
                base: origin::Fragment::plain(binding.to_string()),
                steps: vec![step],
                ty: owned.clone(),
            })),
            Lvalue::Slice(_) => {
                return Err(self.unsupported("`to_mut` of a `Cow` that is not a place"))
            }
            lvalue => nested_step(lvalue, step, owned.clone()),
        };
        let mut source = origin::Fragment::plain(format!("{{ let {place} = "));
        source.push(receiver.rendered_source());
        source.push_text("; ");
        source.push(read);
        source.push_text(" }");
        let mut value = self.operator_expr(
            source,
            Ty::Reference {
                mutable: true,
                target: Box::new(owned),
            },
            expression,
        );
        value.lvalue = Some(lvalue);
        Ok(value)
    }

    /// `Cow::Borrowed(payload)` (a `&B`) or `Cow::Owned(payload)` (`B`'s
    /// owned form) against a `Cow<'_, B>`.
    pub(super) fn resolve_cow_pattern(
        &self,
        owned: bool,
        payload: &PatternSyntax,
        at: Span,
        expected: &Ty,
        bound_names: &mut BTreeMap<ImmutableString, Span>,
        binding_declarations: &mut Vec<PatternBinding>,
    ) -> Result<(NominalPattern, CoveragePattern), Error> {
        let variant = if owned { "Owned" } else { "Borrowed" };
        let expected = self.inference.resolve(expected);
        let Ty::Cow(borrowed) = &expected else {
            return Err(Error::TypeMismatch {
                expected: expected.name(&self.models),
                actual: "Cow<'_, _>".into(),
                expression: at,
            });
        };
        let element = if owned {
            self.owned_of(borrowed).unwrap_or(Ty::Unknown)
        } else {
            reference_to((**borrowed).clone())
        };
        // The pattern tests the variant; its payload is below it.
        self.trace_test(FactAccess::Read, None);
        let path = self.trace_enter_at(
            FactStep::Downcast(variant.into()),
            borrows::TraceRegion::At(FactPath::of(&[FactRegion::Arg(0)])),
        );
        self.trace_enter_at(
            FactStep::Field("0".into()),
            borrows::TraceRegion::At(FactPath::ROOT),
        );
        let (child, coverage) = self.resolve_child_pattern(
            payload,
            &element,
            "Cow",
            variant,
            bound_names,
            binding_declarations,
        )?;
        self.trace_leave(path);
        Ok((
            NominalPattern::Cow {
                owned,
                payload: Box::new(child),
            },
            CoveragePattern::Constructor {
                key: CoverageKey::Cow(owned),
                name: format!("Cow::{variant}").into(),
                payload: Box::new([coverage]),
            },
        ))
    }

    /// `value`, a `Cow` (through any references), read as its payload for
    /// `==`: the payload's type, `&str` for `str`.
    pub(super) fn cow_compared(&mut self, value: &mut ParsedExpr) {
        let mut peeled = self.inference.resolve(&value.ty);
        while let Ty::Reference { target, .. } = peeled {
            peeled = self.inference.resolve(&target);
        }
        if let Ty::Cow(borrowed) = peeled {
            value.source = self.prelude_op(PreludeOp::CowValue, [value.rendered_source()]);
            value.prelude = origin::Fragment::default();
            value.lvalue = None;
            value.ty = payload_ty(&borrowed);
        }
    }
}
