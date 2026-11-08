//! Trait method calls: `Trait::method(x)`, `<T as Trait>::method(x)` and
//! std's traits called by path.

use super::*;

impl<'a> Parser<'a> {
    /// `Self::f(..)`, a trait function without `self`, for the type
    /// `self_ty` whose runtime witness is `witness`: checked against the
    /// trait's signature, and run through the trait's static dispatcher;
    /// a generic trait's instance is taken from a bound, the type's one
    /// impl, or inference.
    pub(super) fn call_static_trait_function(
        &mut self,
        self_ty: Ty,
        witness: origin::Fragment,
        trait_name: &ImmutableString,
        function: &item::Function,
        mut arguments: Vec<ParsedExpr>,
        call: Span,
    ) -> Result<ParsedExpr, Error> {
        let mut substitution = self.fresh_generics(&function.generics, call);
        self.apply_trait_turbofish(&function.generics, &substitution, call)?;
        substitution.insert("Self".into(), self_ty.clone());
        let parameters = self
            .models
            .traits
            .get(trait_name)
            .map(|(_, declared)| {
                declared
                    .generics
                    .iter()
                    .map(|generic| generic.name.text.clone())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let mut trait_arguments = Vec::new();
        let explicit = self.explicit_trait_arguments.take();
        if !parameters.is_empty() {
            let known = match self.inference.resolve(&self_ty) {
                _ if explicit.is_some() => explicit,
                // Two bounds by the trait: inference picks.
                ty @ (Ty::Param(_) | Ty::Projection(..)) => {
                    let lists = self.bound_trait_arguments(&ty, trait_name);
                    (lists.len() == 1).then(|| lists[0].to_vec())
                }
                ty => owner_id(&ty).and_then(|id| {
                    let mut candidates = self.impl_candidates(trait_name, id);
                    (candidates.len() == 1).then(|| candidates.remove(0))
                }),
            };
            trait_arguments = known
                .filter(|known| known.len() == parameters.len())
                .unwrap_or_else(|| parameters.iter().map(|_| self.inference.fresh()).collect());
            self.pending_traits.push((
                self_ty.clone(),
                trait_name.clone(),
                trait_arguments.clone().into_boxed_slice(),
                call,
            ));
        }
        for (parameter, argument) in parameters.iter().zip(&trait_arguments) {
            substitution.insert(parameter.clone(), argument.clone());
        }
        if arguments.len() != function.parameters.len() {
            return Err(Error::argument_count(
                Callee::AssociatedFunction,
                crate::resolve::Qualified(&self_ty.name(&self.models), &function.name.text)
                    .to_string(),
                function.parameters.len(),
                arguments.len(),
                call,
            ));
        }
        for (argument, parameter) in arguments.iter_mut().zip(function.parameters.iter()) {
            let expected = instantiate(&Ty::from_resolved(&parameter.ty), &substitution);
            self.coerce_argument(&parameter.ty, &expected, argument)?;
            self.move_local(argument)?;
        }
        let output = instantiate(
            &function.output.as_ref().map_or(Ty::Unit, Ty::from_resolved),
            &substitution,
        );
        // `f32::unit()` beside `f64::unit()`: one runtime type, so the impl
        // is the Self type's, selected here.
        let resolved = self.inference.resolve(&self_ty);
        if let Some(definition) = owner_id(&resolved)
            .and_then(|owner| self.self_typed_function(owner, &function.name.text, &resolved))
        {
            let source = origin::Fragment::call(
                method_name(definition.id()),
                arguments.iter().map(ParsedExpr::rendered_source),
            );
            return Ok(self.operator_expr(source, output, call));
        }
        let dispatcher = if trait_arguments.is_empty() {
            static_dispatcher_name(trait_name, &function.name.text)
        } else {
            let placeholder = static_instance_placeholder(
                trait_name,
                &function.name.text,
                self.pending_instances.len(),
            );
            self.pending_instances.push((
                placeholder.clone(),
                trait_name.clone(),
                function.name.text.clone(),
                trait_arguments.into_boxed_slice(),
                call,
                true,
                self_ty.clone(),
            ));
            placeholder
        };
        let source = origin::Fragment::call(
            &dispatcher,
            std::iter::once(witness).chain(arguments.iter().map(ParsedExpr::rendered_source)),
        );
        Ok(self.operator_expr(source, output, call))
    }

    /// Whether `name` is a crate trait a qualified call can name: one the
    /// crate declares, not a std trait Rawr provides itself.
    pub(super) fn ufcs_trait(&self, name: &str) -> bool {
        self.models
            .traits
            .get(name)
            .is_some_and(|(_, declared)| declared.name.span != Span::NONE)
            && !self.models.std_traits.contains(name)
    }

    /// `<A, B>` after a trait's name in a qualified path.
    pub(super) fn parse_trait_path_arguments(&mut self) -> Result<Vec<Ty>, Error> {
        self.expect(Token::LessThan, "'<' before a trait's arguments")?;
        let mut types = vec![self.parse_local_type()?];
        while matches!(self.peek(), Token::Comma) {
            self.next();
            if matches!(self.peek(), Token::GreaterThan | Token::RightShift) {
                break;
            }
            types.push(self.parse_local_type()?);
        }
        self.expect_close_angle("'>' after a trait's arguments")?;
        Ok(types)
    }

    /// `Default::default`, `Iterator::next`, `std::cmp::max`: a std trait's
    /// function, or a free function of `std::cmp`, by path, after its first
    /// segment `first`, with the number of tokens up to its arguments, as
    /// std's table resolves it through the module's `use`s and the prelude
    /// (D711 step 2). A trait the crate declares itself is its own
    /// (`parse_trait_path_call`).
    pub(super) fn std_trait_path(
        &self,
        first: &str,
    ) -> Option<(&'static str, &'static str, usize)> {
        let mut ahead = self.input.clone();
        let mut segments = vec![ImmutableString::from(first)];
        while matches!(ahead.peek(), Some((Token::DoubleColon, _))) {
            ahead.next();
            match ahead.next() {
                Some((Token::Identifier(name), _)) => segments.push((*name).as_str().into()),
                _ => return None,
            }
        }
        let called = matches!(ahead.peek(), Some((Token::LeftParen | Token::Unit, _)));
        if segments.len() < 2 {
            return None;
        }
        let written = segments
            .iter()
            .map(ImmutableString::as_str)
            .collect::<Vec<_>>()
            .join("::");
        let binding = self
            .models
            .import(first)
            .map(|binding| binding.qualified.as_str());
        // The first segment through its `use`, else a root of std's or a
        // name of the prelude: `Iterator::next` is
        // `std::prelude::rust_2024::Iterator::next`.
        let base = match binding {
            Some(binding) => binding.to_owned(),
            None if crate::std_table::is_root(first) => first.to_owned(),
            None => crate::std_table::prelude("2024")
                .find(|path| path.path.rsplit("::").next() == Some(first))?
                .path
                .to_owned(),
        };
        let qualified = format!("{base}{}", &written[first.len()..]);
        let parts = qualified.split("::").collect::<Vec<_>>();
        // Whether it is a value when not called: a method, or a function
        // of `std::cmp`.
        let (trait_name, member, valued) =
            match crate::std_table::resolve(&parts, crate::std_table::Namespace::Value).ok()? {
                // `std::cmp::max(a, b)`: a free function of `std::cmp`.
                crate::std_table::Resolved::Path(path)
                    if path.kind == "fn" && path.definition.starts_with("core::cmp::") =>
                {
                    ("cmp", path.definition.rsplit("::").next()?, true)
                }
                crate::std_table::Resolved::Member {
                    owner,
                    member: Some(member),
                } if owner.kind == "trait"
                    && owner.stability == crate::std_table::Stability::Stable
                    && member.kind == "assoc-fn"
                    && member.stability == crate::std_table::Stability::Stable =>
                {
                    (
                        owner.definition.rsplit("::").next()?,
                        member.name(),
                        member.receiver().is_some(),
                    )
                }
                _ => return None,
            };
        // `Iterator::count`, `From::from` as a value is a closure
        // (`parse_std_trait_call`).
        if !called && !valued && !matches!(trait_name, "From" | "Into" | "Clone" | "ToString") {
            return None;
        }
        if self
            .models
            .traits
            .get(trait_name)
            .is_some_and(|(_, declared)| declared.name.span != Span::NONE)
        {
            return None;
        }
        Some((trait_name, member, 2 * (segments.len() - 1)))
    }

    /// The std trait `trait_name`'s function `member`, as std's table
    /// declares it.
    pub(super) fn std_trait_function(
        &self,
        trait_name: &str,
        member: &str,
    ) -> Option<crate::std_table::Item> {
        let uses = self
            .models
            .import(trait_name)
            .map(|binding| (trait_name, binding.qualified.as_str()));
        let definition = crate::resolve::std_trait::named(trait_name, uses)
            .or_else(|| crate::resolve::std_trait::anywhere(trait_name))?;
        let member = crate::std_table::item(&format!("{definition}::{member}"))?;
        (member.kind == "assoc-fn").then_some(member)
    }

    /// `Trait::f(arguments)` through a std or prelude trait, once the
    /// path up to `f` is read, as Rust resolves it: `Default::default()`
    /// takes its type from the expected type, and the others are the
    /// method call or operator they name (`From::from(x)` and
    /// `Into::into(x)` are `x.into()`).
    pub(super) fn parse_std_trait_call(
        &mut self,
        trait_name: &str,
        member: &str,
        start: Position,
    ) -> Result<ParsedExpr, Error> {
        // `.map(From::from)`, `ToString::to_string` as a value: the closure
        // `|value| value.into()` (or the method named).
        if !matches!(self.peek(), Token::LeftParen | Token::Unit)
            && matches!(trait_name, "From" | "Into" | "Clone" | "ToString")
        {
            let at = start;
            let argument = || (Token::Identifier(Box::new("__rawr_trait_value".into())), at);
            let method = if matches!(trait_name, "From" | "Into") {
                "into"
            } else {
                member
            };
            self.splice_tokens(vec![
                argument(),
                (Token::Pipe, at),
                argument(),
                (Token::Period, at),
                (Token::Identifier(Box::new(method.into())), at),
                (Token::Unit, at),
            ]);
            return self.parse_closure(Token::Pipe, point_span(at), false);
        }
        // `.fold(0, std::cmp::max)`: the closure that calls it.
        if trait_name == "cmp"
            && !matches!(self.peek(), Token::LeftParen | Token::Unit)
            && let Some(function) = crate::std_table::item(&format!("core::cmp::{member}"))
            && let Some(declared) = crate::std_table::Declared::parse(function.declaration())
        {
            let arity = crate::std_table::split_list(declared.inputs).len();
            return self.function_value_closure(
                &format!("std::cmp::{member}"),
                arity,
                None,
                point_span(start),
            );
        }
        // `.map(Iterator::count)`, `.max_by(Ord::cmp)`: the closure that
        // calls the method on its first input, which takes `self` as the
        // trait declares it.
        if !matches!(self.peek(), Token::LeftParen | Token::Unit)
            && let Some(function) = self.std_trait_function(trait_name, member)
            && let Some(mode) = std_method::mode_of(&function)
            && let Some(declared) = crate::std_table::Declared::parse(function.declaration())
        {
            let own = self.inference.fresh();
            let receiver = match mode {
                receiver::Mode::Owned => own,
                mode => Ty::Reference {
                    mutable: mode == receiver::Mode::Mutable,
                    target: Box::new(own),
                },
            };
            let count = crate::std_table::split_list(declared.inputs).len();
            let inputs = std::iter::once(receiver)
                .chain((1..count).map(|_| self.inference.fresh()))
                .collect();
            let at = point_span(start);
            return self.method_value(member.into(), inputs, at, at);
        }
        let (arguments, close) = self.take_argument_tokens()?;
        let call = span(start, close);
        let found = argument_count(&arguments);
        let expects = |count: usize| {
            Error::argument_count(
                Callee::AssociatedFunction,
                crate::resolve::Qualified(trait_name, member).to_string(),
                count,
                found,
                call,
            )
        };
        if trait_name == "Default" {
            if !arguments.is_empty() {
                return Err(expects(0));
            }
            // The type is the expected one, fixed by inference: the default
            // is written once it is known.
            let ty = self.inference.fresh();
            // Wide enough for a struct built from its fields' defaults.
            let placeholder = format!(
                "__rawr_nominal_default_value_{:0228}",
                self.pending_defaults.len()
            );
            self.capture_witnesses();
            self.pending_defaults
                .push((placeholder.clone(), ty.clone(), call));
            return Ok(self.operator_expr(origin::Fragment::plain(placeholder), ty, call));
        }
        if arguments.is_empty() {
            return Err(expects(1));
        }
        let identifier = |name: &str| Token::Identifier(Box::new(name.into()));
        let mut tokens = vec![(Token::LeftParen, start)];
        match (trait_name, member) {
            // `x.into()`, `x.clone()`, `x.to_string()`: the receiver as
            // written, its `&` dropped (`Clone::clone(&x)` is `x.clone()`).
            ("From", "from")
            | ("Into", "into")
            | ("Clone", "clone")
            | ("ToString", "to_string") => {
                let (receiver, rest) = split_first_argument(arguments);
                if !rest.is_empty() {
                    return Err(expects(1));
                }
                let receiver = if matches!(trait_name, "From" | "Into") {
                    receiver
                } else {
                    without_reference(receiver)
                };
                let method = if matches!(trait_name, "From" | "Into") {
                    "into"
                } else {
                    member
                };
                tokens.extend(receiver);
                tokens.extend([
                    (Token::RightParen, close),
                    (Token::Period, close),
                    (identifier(method), close),
                    (Token::Unit, close),
                ]);
            }
            // `std::cmp::max_by_key(a, b, f)` and kin: the iterator's own
            // over `[a, b]`, which keeps the same one on a tie (the second
            // for a max, the first for a min).
            ("cmp", "max_by" | "min_by" | "max_by_key" | "min_by_key") => {
                let (left, rest) = split_first_argument(arguments);
                let (right, rest) = split_first_argument(rest);
                let (function, extra) = split_first_argument(rest);
                if function.is_empty() || !extra.is_empty() {
                    return Err(expects(3));
                }
                let text = format!(
                    "vec![__rawr_left, __rawr_right].into_iter().{member}(__rawr_function).unwrap()"
                );
                let tokens = template_tokens(
                    &text,
                    &[
                        ("__rawr_left", left),
                        ("__rawr_right", right),
                        ("__rawr_function", function),
                    ],
                    start,
                );
                self.splice_tokens(tokens);
                return self.parse_primary();
            }
            // `Ord::max(a, b)` and `std::cmp::max(a, b)` are `a.max(b)`.
            ("Ord", "min" | "max" | "clamp") | ("cmp", _) => {
                let (left, right) = split_first_argument(arguments);
                if right.is_empty() {
                    return Err(expects(if member == "clamp" { 3 } else { 2 }));
                }
                tokens.extend(left);
                tokens.extend([
                    (Token::RightParen, close),
                    (Token::Period, close),
                    (identifier(member), close),
                    (Token::LeftParen, close),
                ]);
                tokens.extend(right);
                tokens.push((Token::RightParen, close));
            }
            // `Ord::cmp(&a, &b)` is `a.cmp(&b)`.
            ("Ord", _) | ("PartialOrd", "partial_cmp") => {
                let (left, right) = split_first_argument(arguments);
                let (right, extra) = split_first_argument(right);
                if right.is_empty() || !extra.is_empty() {
                    return Err(expects(2));
                }
                tokens.extend(without_reference(left));
                tokens.extend([
                    (Token::RightParen, close),
                    (Token::Period, close),
                    (identifier(member), close),
                    (Token::LeftParen, close),
                ]);
                tokens.extend(right);
                tokens.push((Token::RightParen, close));
            }
            // `PartialEq::eq(&a, &b)` is `a == b`.
            ("PartialEq" | "PartialOrd", _) => {
                let (left, right) = split_first_argument(arguments);
                let (right, extra) = split_first_argument(right);
                if right.is_empty() || !extra.is_empty() {
                    return Err(expects(2));
                }
                let operator = match member {
                    "eq" => Token::EqualsTo,
                    "ne" => Token::NotEqualsTo,
                    "lt" => Token::LessThan,
                    "le" => Token::LessThanEqualsTo,
                    "gt" => Token::GreaterThan,
                    _ => Token::GreaterThanEqualsTo,
                };
                tokens.push((Token::LeftParen, start));
                tokens.extend(without_reference(left));
                tokens.extend([
                    (Token::RightParen, close),
                    (operator, close),
                    (Token::LeftParen, close),
                ]);
                tokens.extend(without_reference(right));
                tokens.extend([(Token::RightParen, close), (Token::RightParen, close)]);
            }
            // `FromStr::from_str(s)` is `s.parse()`, which is std's own
            // `FromStr` lookup by the expected type.
            ("FromStr", "from_str") => {
                let (text, extra) = split_first_argument(arguments);
                if !extra.is_empty() {
                    return Err(expects(1));
                }
                tokens.extend(text);
                tokens.extend([
                    (Token::RightParen, close),
                    (Token::Period, close),
                    (identifier("parse"), close),
                    (Token::Unit, close),
                ]);
            }
            // `fmt::Display::fmt(&self.0, f)` in a `fmt` body writes the
            // value into its formatter: `write!(f, "{}", self.0)`.
            ("Display" | "Debug", "fmt") => {
                let (value, formatter) = split_first_argument(arguments);
                let (formatter, extra) = split_first_argument(formatter);
                if formatter.is_empty() || !extra.is_empty() {
                    return Err(expects(2));
                }
                if self.formatter.is_none() {
                    return Err(Error::UnsupportedNominalSyntax {
                        construct: format!(
                            "`{trait_name}::fmt` outside a `Display` or `Debug` `fmt` body"
                        )
                        .into(),
                        source: call,
                    });
                }
                let template = if trait_name == "Debug" { "{:?}" } else { "{}" };
                tokens = vec![
                    (identifier("write"), start),
                    (Token::Bang, start),
                    (Token::LeftParen, start),
                ];
                tokens.extend(formatter);
                tokens.extend([
                    (Token::Comma, close),
                    (Token::StringConstant(Box::new(template.into())), close),
                    (Token::Comma, close),
                ]);
                tokens.extend(without_reference(value));
                tokens.push((Token::RightParen, close));
            }
            // Any other trait function that takes `self` is the method call
            // on its first argument, which method lookup resolves as Rust's
            // does: `Iterator::next(&mut it)` is `it.next()`,
            // `Clone::clone_from(&mut a, &b)` is `a.clone_from(&b)`.
            _ => match self.std_receiver(trait_name, member) {
                Some(Some(receiver)) => {
                    let (first, rest) = split_first_argument(arguments);
                    let first = if receiver == "self" {
                        first
                    } else {
                        without_borrow(first)
                    };
                    tokens.extend(first);
                    tokens.extend([
                        (Token::RightParen, close),
                        (Token::Period, close),
                        (identifier(member), close),
                    ]);
                    if rest.is_empty() {
                        tokens.push((Token::Unit, close));
                    } else {
                        tokens.push((Token::LeftParen, close));
                        tokens.extend(rest);
                        tokens.push((Token::RightParen, close));
                    }
                }
                // An associated function rawr has no lookup for yet.
                Some(None) => {
                    return Err(Error::NotYet {
                        path: crate::resolve::Qualified(trait_name, member)
                            .to_string()
                            .into(),
                        reference: call,
                    });
                }
                None => {
                    return Err(Error::UnknownMember {
                        owner: trait_name.into(),
                        member: member.into(),
                        reference: call,
                    });
                }
            },
        }
        self.splice_tokens(tokens);
        self.parse_primary()
    }

    /// `<T as Clone>::clone(&x)`, `<T as Ord>::cmp(&a, &b)` and the other
    /// std traits' functions for a written `Self`: the first argument,
    /// through its references, has type `T`; then the call as
    /// `parse_std_trait_call` reads it, on that argument bound once as a
    /// `T`.
    pub(super) fn parse_self_typed_std_call(
        &mut self,
        trait_name: &str,
        member: &str,
        self_ty: Ty,
        start: Position,
    ) -> Result<ParsedExpr, Error> {
        let (arguments, close) = self.take_argument_tokens()?;
        let (receiver, rest) = split_first_argument(arguments);
        if receiver.is_empty() {
            return Err(Error::InvalidMethodCall {
                owner: trait_name.into(),
                method: member.into(),
                reason: "expects a `self` argument".into(),
                call: span(start, close),
            });
        }
        let mut tokens = vec![(Token::LeftParen, start)];
        tokens.extend(receiver);
        tokens.push((Token::RightParen, close));
        self.splice_tokens(tokens);
        let value = self.parse_primary()?;
        let mut actual = self.inference.resolve(&value.ty);
        while let Ty::Reference { target, .. } = actual {
            actual = self.inference.resolve(&target);
        }
        self.require_type(&self_ty, &actual, value.span)?;
        // References are the value at runtime; the call reads it as `Self`.
        let name = self.temporary("self");
        self.bindings.insert(
            name.as_str().into(),
            Binding {
                ty: actual,
                mutable: false,
                available: true,
                declaration: value.span,
                kind: BindingKind::Local,
            },
        );
        let mut tokens = vec![
            (Token::LeftParen, start),
            (Token::Identifier(Box::new(name.as_str().into())), start),
        ];
        if !rest.is_empty() {
            tokens.push((Token::Comma, close));
            tokens.extend(rest);
        }
        tokens.push((Token::RightParen, close));
        self.splice_tokens(tokens);
        let call = self.parse_std_trait_call(trait_name, member, start);
        self.bindings.remove(name.as_str());
        let call = call?;
        let mut source = origin::Fragment::plain(format!("{{ let {name} = "));
        source.push(value.rendered_source());
        source.push_text("; ");
        source.push(call.rendered_source());
        source.push_text(" }");
        Ok(self.operator_expr(source.surrounded("(", ")"), call.ty, span(start, close)))
    }

    /// `<T as Default>::default()`, `<T as From<S>>::from(x)`,
    /// `Into::<T>::into(x)` once `::` is read: the std trait's function as
    /// `parse_std_trait_call` reads it, with the written types as `let`
    /// annotations coerce: the value is `T`, and `x` is `S` (`From`) or the
    /// written `Self` (`Into`).
    pub(super) fn parse_typed_std_trait_call(
        &mut self,
        trait_name: &'static str,
        self_ty: Option<Ty>,
        explicit: Option<Vec<Ty>>,
        start: Position,
    ) -> Result<ParsedExpr, Error> {
        let (member, member_span) = self.expect_member_name()?;
        let Some(member) = crate::resolve::std_trait::anywhere(trait_name)
            .and_then(|definition| crate::std_table::item(&format!("{definition}::{member}")))
            .filter(|found| found.kind == "assoc-fn")
            .map(|found| found.name())
        else {
            return Err(Error::UnknownMember {
                owner: trait_name.into(),
                member,
                reference: member_span,
            });
        };
        let mut explicit = explicit.unwrap_or_default();
        let arity = usize::from(matches!(trait_name, "From" | "Into"));
        if !explicit.is_empty() && explicit.len() != arity {
            return Err(Error::InvalidTypeArguments {
                ty: trait_name.into(),
                expected: arity,
                actual: explicit.len(),
                reference: member_span,
            });
        }
        let argument = explicit.pop();
        let (source, target) = match trait_name {
            "Default" => (None, self_ty),
            "From" => (argument, self_ty),
            "Into" => (self_ty, argument),
            _ => {
                return match self_ty {
                    Some(self_ty) => {
                        self.parse_self_typed_std_call(trait_name, member, self_ty, start)
                    }
                    None => self.parse_std_trait_call(trait_name, member, start),
                };
            }
        };
        let mut value = if trait_name == "Default" {
            self.parse_std_trait_call(trait_name, member, start)?
        } else {
            // `x` as written, coerced to its type, then `.into()`, ended by
            // a `;` that stops the postfix loop.
            let (arguments, close) = self.take_argument_tokens()?;
            let found = argument_count(&arguments);
            let (argument, rest) = split_first_argument(arguments);
            if argument.is_empty() || !rest.is_empty() {
                return Err(Error::argument_count(
                    Callee::AssociatedFunction,
                    crate::resolve::Qualified(trait_name, member).to_string(),
                    1,
                    found,
                    span(start, close),
                ));
            }
            let mut tokens = vec![(Token::LeftParen, start)];
            tokens.extend(argument);
            tokens.push((Token::RightParen, close));
            self.splice_tokens(tokens);
            let mut argument = self.parse_primary()?;
            if let Some(source) = &source {
                self.coerce_expr(source, &mut argument)?;
            }
            let call = span(start, close);
            if let Some(target) = &target
                && let Some((items, owner)) = self.text_from_target(&argument.ty, target, call)?
            {
                self.move_local(&mut argument)?;
                let mut value = self.call_host_function(&items, owner, vec![argument], call)?;
                self.coerce_expr(target, &mut value)?;
                return Ok(value);
            }
            self.splice_tokens(vec![
                (Token::Period, close),
                (Token::Identifier(Box::new("into".into())), close),
                (Token::Unit, close),
                (Token::SemiColon, close),
            ]);
            let mut value = self.parse_postfix_of(argument)?;
            self.expect(Token::SemiColon, "the end of a qualified conversion")?;
            value.span = span(start, close);
            value
        };
        if let Some(target) = &target {
            self.coerce_expr(target, &mut value)?;
        }
        Ok(value)
    }

    /// `Trait::f(receiver, ..)` and `<T as Trait>::f(..)` once `::` is
    /// read: the trait's function `f`, for `self_ty` when written, else for
    /// the receiver argument's type, as Rust resolves a fully qualified
    /// call. Inherent functions of the type are never candidates.
    pub(super) fn parse_trait_path_call(
        &mut self,
        self_ty: Option<Ty>,
        trait_name: ImmutableString,
        trait_span: Span,
        explicit: Option<Vec<Ty>>,
        start: Position,
    ) -> Result<ParsedExpr, Error> {
        let (member, member_span) = self.expect_member_name()?;
        // `Trait::f::<T>(..)`: the function's own type arguments.
        let turbofish = if matches!(self.peek(), Token::DoubleColon)
            && matches!(self.peek_second(), Token::LessThan)
            && self.ufcs_trait(&trait_name)
        {
            self.next();
            self.next();
            Some(self.parse_turbofish_types()?)
        } else {
            None
        };
        // `<S::Error as Error>::custom(..)` of a type parameter native code
        // fills: the function of its opaque type (#593).
        #[cfg(feature = "rust-import")]
        if let Some(Ty::Host { proof, .. }) = self_ty.as_ref().map(|ty| self.inference.resolve(ty))
            && turbofish.is_none()
            && rawr_import::surface::opaque::declared(&proof.definition.path())
        {
            let owner = proof.definition.path().to_string();
            return self.opaque_function_call(&owner, &member, member_span);
        }
        // `<Look as Tone>::tone` of a crate's trait a `use` names: the
        // method on its first input or argument, as `Tone::tone` is.
        if !self.ufcs_trait(&trait_name)
            && let Some(binding) = self.models.import(&trait_name).cloned()
            && matches!(binding.target, item::Import::Module)
            && self
                .models
                .host_items(&format!("{}::{member}", binding.qualified))
                .is_none()
        {
            if matches!(self.peek(), Token::LeftParen) {
                if let Some(tokens) =
                    self.path_method_call(&member, member_span, Some(binding.qualified.clone()))
                {
                    self.splice_tokens(tokens);
                    return self.parse_primary();
                }
            } else if !matches!(self.peek(), Token::Unit) {
                let arity = self
                    .closure_hint
                    .as_ref()
                    .map_or(1, |hint| hint.len().max(1));
                let mut inputs: Vec<Ty> = (0..arity).map(|_| self.inference.fresh()).collect();
                if let Some(self_ty) = self_ty {
                    // The written `Self` is the receiver's, through any
                    // reference the use gives it.
                    let given = self
                        .closure_hint
                        .as_ref()
                        .and_then(|hint| hint.first().cloned());
                    let receiver = match given.map(|given| self.inference.resolve(&given)) {
                        Some(Ty::Reference { mutable, .. }) => Ty::Reference {
                            mutable,
                            target: Box::new(self_ty),
                        },
                        _ => self_ty,
                    };
                    inputs[0] = receiver;
                }
                return self.method_value(member, inputs.into(), trait_span, member_span);
            }
        }
        if !self.ufcs_trait(&trait_name) {
            return Err(Error::UnsupportedNominalSyntax {
                construct: format!(
                    "a qualified call through `{trait_name}`, which is not a trait the crate \
                     declares"
                )
                .into(),
                source: trait_span,
            });
        }
        let function = self
            .models
            .traits
            .get(&trait_name)
            .and_then(|(_, declared)| {
                declared
                    .functions
                    .iter()
                    .find(|function| function.name.text == member)
                    .cloned()
            });
        let Some(function) = function else {
            return Err(Error::UnknownMember {
                owner: trait_name,
                member,
                reference: member_span,
            });
        };
        // `Trait::f` or `<T as Trait>::f` as a value: the closure
        // `|a, ..| Trait::f(a, ..)`, the receiver first.
        if let Some(receiver) = &function.receiver
            && !function.body.constant
            && !matches!(self.peek(), Token::LeftParen | Token::Unit)
        {
            let arity = function.parameters.len() + 1;
            let at = member_span.start();
            let parameter =
                |index: usize| Token::Identifier(Box::new(format!("__rawr_method{index}").into()));
            let mut tokens = Vec::new();
            for index in 0..arity {
                if index > 0 {
                    tokens.push((Token::Comma, at));
                }
                tokens.push((parameter(index), at));
            }
            tokens.push((Token::Pipe, at));
            tokens.push((
                Token::Identifier(Box::new(trait_name.as_str().into())),
                trait_span.start(),
            ));
            tokens.push((Token::DoubleColon, at));
            tokens.push((Token::Identifier(Box::new(member.as_str().into())), at));
            tokens.push((Token::LeftParen, at));
            for index in 0..arity {
                if index > 0 {
                    tokens.push((Token::Comma, at));
                }
                tokens.push((parameter(index), at));
            }
            tokens.push((Token::RightParen, at));
            // The written type is the receiver's, as Rust takes it.
            if let Some(self_ty) = self_ty {
                let receiver = match receiver {
                    crate::item::Receiver::Shared(_) => Ty::Reference {
                        mutable: false,
                        target: Box::new(self_ty),
                    },
                    crate::item::Receiver::Mutable(_) => Ty::Reference {
                        mutable: true,
                        target: Box::new(self_ty),
                    },
                    _ => self_ty,
                };
                let mut hint = vec![receiver];
                hint.extend((1..arity).map(|_| self.inference.fresh()));
                self.closure_hint = Some(hint.into_boxed_slice());
            }
            self.splice_tokens(tokens);
            return self.parse_closure(Token::Pipe, span(start, member_span.end()), false);
        }
        // `Trait::f(&mut x, ..)` of a `&mut self` function: the receiver
        // is the place `x`, which the call changes as `x.f(..)` does.
        let mut lent = false;
        if matches!(function.receiver, Some(crate::item::Receiver::Mutable(_)))
            && matches!(self.peek(), Token::LeftParen)
        {
            let mut ahead = self.input.clone();
            ahead.next();
            if matches!(ahead.next(), Some((Token::Ampersand, _)))
                && matches!(ahead.next(), Some((Token::Mut, _)))
            {
                let open = self.next();
                self.next();
                self.next();
                self.splice_tokens(vec![open]);
                lent = true;
            }
        }
        let (mut arguments, end) = if function.body.constant {
            (Vec::new(), text_end(member_span.start(), &member))
        } else {
            self.parse_call_arguments("qualified trait function arguments")?
        };
        let call = span(start, end);
        let missing = |this: &Self, ty: &Ty| Error::MissingCapability {
            owner: ty.name(&this.models),
            capability: trait_name.clone(),
            operation: call,
            derive: matches!(ty, Ty::Script(..)),
            required_by: None,
        };
        let generic_trait = self
            .models
            .traits
            .get(&trait_name)
            .is_some_and(|(_, declared)| !declared.generics.is_empty());
        let Some(receiver_mode) = &function.receiver else {
            // `<T as Make>::make()`: the written type's witness picks the impl.
            let Some(self_ty) = self_ty else {
                return Err(Error::InvalidMethodCall {
                    owner: trait_name.clone(),
                    method: member,
                    reason: "has no `self`, so the type it is for cannot be inferred here; \
                             write `<Type as Trait>::function(..)`"
                        .into(),
                    call,
                });
            };
            let self_ty = self.inference.resolve(&self_ty);
            if !generic_trait && !self.type_implements(&self_ty, &trait_name) {
                return Err(missing(self, &self_ty));
            }
            let witness = match &self_ty {
                Ty::Param(name) if self.type_implements(&self_ty, &trait_name) => {
                    self.capture_witnesses();
                    origin::Fragment::plain(witness_variable(name))
                }
                ty => match owner_id(ty) {
                    Some(id) => origin::Fragment::plain(owner_key(id)),
                    None => return Err(missing(self, ty)),
                },
            };
            self.explicit_trait_arguments = explicit;
            self.trait_turbofish = turbofish;
            return self.call_static_trait_function(
                self_ty,
                witness,
                &trait_name,
                &function,
                arguments,
                call,
            );
        };
        if arguments.is_empty() {
            return Err(Error::InvalidMethodCall {
                owner: trait_name.clone(),
                method: member,
                reason: "takes `self`: pass the receiver as the first argument".into(),
                call,
            });
        }
        let mut receiver = arguments.remove(0);
        // `&self` takes `&x`; `self` takes `x`, as written.
        let by_reference = matches!(
            receiver_mode,
            crate::item::Receiver::Shared(_) | crate::item::Receiver::Mutable(_)
        );
        let receiver_ty = match (by_reference, self.inference.resolve(&receiver.ty)) {
            (true, ty) if lent => ty,
            (true, Ty::Reference { target, .. }) => *target,
            (true, ty) => {
                return Err(Error::TypeMismatch {
                    expected: format!("&{}", self_ty.as_ref().unwrap_or(&ty).name(&self.models))
                        .into(),
                    actual: ty.name(&self.models),
                    expression: receiver.span,
                });
            }
            (false, ty) => ty,
        };
        if let Some(self_ty) = &self_ty {
            self.require_type(self_ty, &receiver_ty, receiver.span)?;
        }
        let receiver_ty = self.inference.resolve(&receiver_ty);
        if matches!(receiver_ty, Ty::Infer(_)) {
            return Err(Error::InvalidMethodCall {
                owner: trait_name.clone(),
                method: member,
                reason: "type annotations needed: the receiver's type is unknown".into(),
                call,
            });
        }
        if !generic_trait && !self.type_implements(&receiver_ty, &trait_name) {
            return Err(missing(self, &receiver_ty));
        }
        receiver.span = span(start, receiver.span.end());
        // `Pick::pick::<i64>(&s)` of a generic function on a program type:
        // the type's own impl's method, called as `s.pick::<i64>()` is,
        // which is compiled for the call's types.
        if !function.generics.is_empty()
            && explicit.is_none()
            && let Some(owner) = owner_id(&receiver_ty)
            && self.models.method(owner, &member).is_some()
        {
            let owner_name = receiver_ty.name(&self.models);
            self.pending_turbofish = turbofish;
            let called = self.call_owner_method(
                receiver,
                owner,
                owner_name,
                &member,
                member_span,
                arguments,
                end,
            )?;
            let Ok(mut value) = called else {
                return Err(Error::UnknownMember {
                    owner: trait_name,
                    member,
                    reference: member_span,
                });
            };
            value.span = call;
            return Ok(value);
        }
        self.explicit_trait_arguments = explicit;
        self.trait_turbofish = turbofish;
        let mut value = self.call_trait_method(
            receiver,
            receiver_ty,
            &trait_name,
            &function,
            arguments,
            member_span,
            end,
        )?;
        value.span = call;
        Ok(value)
    }

    /// `value.member(..)` on a value of the generic type or trait object
    /// `self_ty`, through its trait `trait_name`: checked against the
    /// trait's signature with `Self` as `self_ty`, and dispatched at runtime
    /// by the value's type.
    pub(super) fn call_trait_method(
        &mut self,
        mut value: ParsedExpr,
        self_ty: Ty,
        trait_name: &ImmutableString,
        function: &item::Function,
        mut arguments: Vec<ParsedExpr>,
        member_span: Span,
        end: Position,
    ) -> Result<ParsedExpr, Error> {
        let call = span(value.span.start(), end);
        let param = &self_ty.name(&self.models);
        let Some(receiver) = &function.receiver else {
            return Err(Error::InvalidMethodCall {
                owner: param.clone(),
                method: function.name.text.clone(),
                reason: format!(
                    "`{}::{}` has no `self`; call it on a type",
                    trait_name, function.name.text
                )
                .into(),
                call,
            });
        };
        if arguments.len() != function.parameters.len() {
            return Err(Error::argument_count(
                Callee::Method,
                crate::resolve::Qualified(param, &function.name.text).to_string(),
                function.parameters.len(),
                arguments.len(),
                span(member_span.start(), end),
            ));
        }
        // A trait object's value is unsized: it cannot be moved out.
        if matches!(self_ty, Ty::Dyn(..))
            && matches!(
                receiver,
                crate::item::Receiver::Owned(_) | crate::item::Receiver::OwnedMutable(_)
            )
        {
            return Err(Error::InvalidMethodCall {
                owner: param.clone(),
                method: function.name.text.clone(),
                reason: "takes `self` by value, which a trait object's size does not allow".into(),
                call,
            });
        }
        let mut substitution = self.fresh_generics(&function.generics, call);
        self.apply_trait_turbofish(&function.generics, &substitution, call)?;
        substitution.insert("Self".into(), self_ty.clone());
        // `Convert<i64>`: the trait's arguments, from the trait object, the
        // bound, or the receiver type's one impl; else inferred.
        let parameters = self
            .models
            .traits
            .get(trait_name)
            .map(|(_, declared)| {
                declared
                    .generics
                    .iter()
                    .map(|generic| generic.name.text.clone())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let explicit = self.explicit_trait_arguments.take();
        let trait_arguments = if parameters.is_empty() {
            Vec::new()
        } else {
            let known = match self.inference.resolve(&self_ty) {
                _ if explicit.is_some() => explicit,
                Ty::Dyn(name, bindings) if name == *trait_name => Some(dyn_arguments(&bindings)),
                // A supertrait's method on `dyn Both<i64>`: `Convert<i64>`.
                Ty::Dyn(name, bindings) => {
                    let mut instances =
                        self.dyn_supertrait_instances(&name, &bindings, &self_ty, trait_name);
                    (instances.len() == 1).then(|| instances.remove(0))
                }
                // Two bounds by the trait: inference picks.
                ty @ (Ty::Param(_) | Ty::Projection(..)) => {
                    let lists = self.bound_trait_arguments(&ty, trait_name);
                    (lists.len() == 1).then(|| lists[0].to_vec())
                }
                Ty::Script(id, _) => {
                    let mut candidates = self.impl_candidates(trait_name, id);
                    (candidates.len() == 1).then(|| candidates.remove(0))
                }
                _ => None,
            };
            let arguments = known
                .filter(|known| known.len() == parameters.len())
                .unwrap_or_else(|| parameters.iter().map(|_| self.inference.fresh()).collect());
            self.pending_traits.push((
                self_ty.clone(),
                trait_name.clone(),
                arguments.clone().into_boxed_slice(),
                call,
            ));
            arguments
        };
        for (parameter, argument) in parameters.iter().zip(&trait_arguments) {
            substitution.insert(parameter.clone(), argument.clone());
        }
        for (argument, parameter) in arguments.iter_mut().zip(function.parameters.iter()) {
            let expected = instantiate(&Ty::from_resolved(&parameter.ty), &substitution);
            self.coerce_argument(&parameter.ty, &expected, argument)?;
            self.move_local(argument)?;
        }
        let output = instantiate(
            &function.output.as_ref().map_or(Ty::Unit, Ty::from_resolved),
            &substitution,
        );
        // `t.slot(0)` of a trait's `fn slot(&mut self, i: usize) -> &mut
        // i32`: the place below `t` the output borrows.
        let borrowed = (matches!(receiver, crate::item::Receiver::Mutable(_))
            && function.name.text != "index_mut")
            .then(|| self.place_output(&output))
            .flatten();
        let borrowed_receiver = match &borrowed {
            Some(_) => Some(self.borrowed_receiver(&value, call)?),
            None => None,
        };
        // `&mut self` through a generic or a trait object: the receiver's
        // place is changed (a local, an element, a place below a field).
        let mut place = None;
        if borrowed.is_none() && matches!(receiver, crate::item::Receiver::Mutable(_)) {
            let root = value.lvalue.as_ref().map(|lvalue| lvalue.binding().clone());
            let mutable = root.as_ref().is_some_and(|root| {
                self.bindings.get(root.as_str()).is_some_and(|binding| {
                    binding.mutable || matches!(binding.ty, Ty::Reference { mutable: true, .. })
                })
            });
            if root.is_none() {
                return Err(Error::InvalidMethodCall {
                    owner: param.clone(),
                    method: function.name.text.clone(),
                    reason: "cannot borrow a value that is not a place as mutable".into(),
                    call: member_span,
                });
            }
            if !mutable {
                return Err(Error::InvalidMethodCall {
                    owner: param.clone(),
                    method: function.name.text.clone(),
                    reason: "cannot borrow an immutable binding as mutable".into(),
                    call: member_span,
                });
            }
            place = match &value.lvalue {
                Some(Lvalue::Place { place, .. }) => Some(place.clone()),
                Some(Lvalue::Field(field)) => {
                    let nested = NestedLvalue {
                        binding: field.binding.clone(),
                        base: origin::Fragment::plain(field.binding.to_string()),
                        steps: vec![PlaceStep::Field { slot: field.slot }],
                        ty: field.ty.clone(),
                    };
                    Some(self.nested_place(&nested))
                }
                Some(Lvalue::Nested(nested)) => {
                    let nested = (**nested).clone();
                    Some(self.nested_place(&nested))
                }
                _ => None,
            };
        }
        // `&mut` parameters are copied in and back out, as an inherent
        // method's are (`copied_parameters`).
        let copied = if matches!(trait_name.as_str(), "Debug" | "Display") {
            Vec::new()
        } else {
            copied_parameters(&function.parameters, None, function.formatter.as_ref())
        };
        let places = self.mutable_arguments(&arguments, copied)?;
        let dispatcher = if trait_arguments.is_empty() {
            trait_dispatcher_name(trait_name, &function.name.text)
        } else {
            // The instance's dispatcher, once inference fixes the arguments.
            let placeholder = instance_placeholder(
                trait_name,
                &function.name.text,
                self.pending_instances.len(),
            );
            self.pending_instances.push((
                placeholder.clone(),
                trait_name.clone(),
                function.name.text.clone(),
                trait_arguments.into_boxed_slice(),
                call,
                false,
                self_ty.clone(),
            ));
            placeholder
        };
        let arguments =
            origin::Fragment::join(arguments.iter().map(ParsedExpr::rendered_source), ", ");
        if let (Some(borrowed), Some(receiver)) = (borrowed, borrowed_receiver) {
            let mut source =
                origin::Fragment::plain(format!("{}.{dispatcher}(", places::RECEIVER_PLACE));
            source.push(arguments);
            source.push_text(")");
            let source = self.copied_call(source, &places)?;
            return Ok(self.borrowed_call(&receiver, source, borrowed, call));
        }
        let source = if matches!(
            receiver,
            crate::item::Receiver::Owned(_) | crate::item::Receiver::OwnedMutable(_)
        ) {
            // The method takes the value, as a move or copy does.
            self.move_local(&mut value)?;
            let temporary = self.temporary("receiver");
            let mut source = origin::Fragment::plain(format!("({{\nlet {temporary} = "));
            source.push(value.rendered_source());
            source.push_text(format!(";\n{temporary}.{dispatcher}("));
            source.push(arguments);
            source.push_text(")\n})");
            source
        } else {
            let mut source = place.unwrap_or_else(|| value.rendered_source());
            source.push_text(format!(".{dispatcher}("));
            source.push(arguments);
            source.push_text(")");
            source
        };
        let source = self.copied_call(source, &places)?;
        Ok(self.operator_expr(source, output, call))
    }
}
