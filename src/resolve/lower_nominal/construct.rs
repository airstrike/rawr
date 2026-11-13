//! Values built by name: struct and tuple struct expressions, arrays and
//! `vec!`, prelude constructors, and variants and structs as functions.

use super::*;

impl<'a> Parser<'a> {
    /// A struct named as a value: `S` of `struct S;`, `W(a, b)` of a tuple
    /// struct, or `W` uncalled, the function from its fields to it, as
    /// Rust's tuple struct constructors are (`.map(W)`).
    pub(super) fn parse_struct_value(
        &mut self,
        model: Nominal,
        expression: Span,
    ) -> Result<Option<ParsedExpr>, Error> {
        let Nominal::Struct { definition, item } = model else {
            return Ok(None);
        };
        match item.form {
            crate::item::StructForm::Unit if item.generics.is_empty() => {
                let ty = Ty::Script(definition.id(), Box::default());
                let helper = self.push_op(Op::Struct {
                    definition,
                    item: Box::new(item),
                });
                let source = origin::Fragment::call(&helper, []);
                return Ok(Some(self.operator_expr(source, ty, expression)));
            }
            crate::item::StructForm::Tuple => {}
            _ => return Ok(None),
        }
        let model = Nominal::Struct {
            definition: definition.clone(),
            item: item.clone(),
        };
        let (struct_ty, substitution) = self.fresh_nominal(&model, expression);
        let inputs: Box<[Ty]> = item
            .fields
            .iter()
            .map(|field| instantiate(&Ty::from_resolved(&field.ty), &substitution))
            .collect();
        let arity = inputs.len();
        let helper = self.push_op(Op::Struct {
            definition: definition.clone(),
            item: Box::new(item),
        });
        if !matches!(self.peek(), Token::LeftParen | Token::Unit) {
            let parameters = (0..arity)
                .map(|index| format!("__rawr_argument_{index}"))
                .collect::<Vec<_>>()
                .join(", ");
            let source =
                origin::Fragment::plain(format!("(|{parameters}| {helper}({parameters}))"));
            let ty = Ty::Fn {
                inputs,
                output: Box::new(struct_ty),
            };
            return Ok(Some(self.operator_expr(source, ty, expression)));
        }
        let mut args = Vec::new();
        let end = if matches!(self.peek(), Token::Unit) {
            self.next().1
        } else {
            self.next();
            while !matches!(self.peek(), Token::RightParen) {
                args.push(self.parse_expr_expecting(inputs.get(args.len()))?);
                if matches!(self.peek(), Token::Comma) {
                    self.next();
                } else {
                    break;
                }
            }
            self.expect(Token::RightParen, "')' after tuple struct fields")?
        };
        if args.len() != arity {
            return Err(Error::argument_count(
                Callee::Struct,
                definition.name(),
                arity,
                args.len(),
                span(expression.start(), end),
            ));
        }
        for (value, expected) in args.iter_mut().zip(inputs.iter()) {
            self.coerce_expr(expected, value)?;
            self.move_local(value)?;
        }
        let parts: Vec<_> = args
            .iter()
            .enumerate()
            .map(|(slot, value)| (value, borrows::Slot::Field(definition.id(), None, slot)))
            .collect();
        self.compose(span(expression.start(), end), &struct_ty, &parts);
        let source = origin::Fragment::call(&helper, args.iter().map(ParsedExpr::rendered_source));
        Ok(Some(self.operator_expr(
            source,
            struct_ty,
            span(expression.start(), end),
        )))
    }

    /// The crate type `name` names, `Self` in an impl naming its owner.
    /// The type the path's one segment `owner` names when it is a generic
    /// parameter this body reads as a type: none of its own generics, nor
    /// a type of that name.
    pub(super) fn parameter_type(
        &self,
        segments: &[(ImmutableString, Span)],
        owner: &str,
    ) -> Option<Ty> {
        if segments.len() != 1
            || self.generics.contains_key(owner)
            || self.models.get(owner).is_some()
        {
            return None;
        }
        self.trait_parameters
            .get(owner)
            .map(|ty| self.inference.resolve(ty))
    }

    pub(super) fn named_model(&self, name: &str) -> Option<Nominal> {
        if let Some(model) = self.models.get(name) {
            return Some(model.clone());
        }
        match (name, &self.self_ty) {
            ("Self", Some(Ty::Script(id, _))) => Some(self.models.definition(*id).clone()),
            _ => None,
        }
    }

    /// The type of `S` when `name` is a unit struct `struct S;`.
    pub(super) fn unit_struct(&self, name: &str) -> Option<Ty> {
        match self.models.get(name)? {
            model @ Nominal::Struct { item, .. }
                if item.form == crate::item::StructForm::Unit && item.generics.is_empty() =>
            {
                Some(model.ty())
            }
            _ => None,
        }
    }

    /// The Rust function a symbolic variant constructor (`Shape::Circle`,
    /// kept symbolic for the host) is when a script uses it as one.
    pub(super) fn constructor_function(&mut self, ty: &Ty, expression: Span) -> Option<ParsedExpr> {
        let Ty::Callable {
            item: Some(item), ..
        } = ty
        else {
            return None;
        };
        if item.kind() != crate::item::Kind::VariantConstructor {
            return None;
        }
        let Some(Nominal::Enum {
            definition,
            item: declaration,
        }) = item
            .key()
            .owner()
            .and_then(|owner| self.models.by_identity.get(owner))
            .cloned()
        else {
            return None;
        };
        let variant = declaration
            .variants
            .iter()
            .position(|variant| variant.name.text.as_str() == item.key().name())?;
        Some(self.variant_function(&definition, &declaration, variant, expression))
    }

    /// `Some`, `Ok` or `Err` used as a value: the function from its payload.
    pub(super) fn prelude_function(
        &mut self,
        variant: Prelude,
        ok: Ty,
        err: Ty,
        expression: Span,
    ) -> ParsedExpr {
        let (op, input, output) = match variant {
            Prelude::Some => (PreludeOp::Some, ok.clone(), Ty::Option(Box::new(ok))),
            Prelude::Ok => (
                PreludeOp::Ok,
                ok.clone(),
                Ty::Result(Box::new(ok), Box::new(err)),
            ),
            Prelude::Err => (
                PreludeOp::Err,
                err.clone(),
                Ty::Result(Box::new(ok), Box::new(err)),
            ),
            Prelude::None => unreachable!("`None` is a value, not a function"),
        };
        let call = self.prelude_op(op, [origin::Fragment::plain("__rawr_argument_0")]);
        let mut source = origin::Fragment::plain("(|__rawr_argument_0| ");
        source.push(call);
        source.push_text(")");
        self.operator_expr(
            source,
            Ty::Fn {
                inputs: Box::new([input]),
                output: Box::new(output),
            },
            expression,
        )
    }

    /// `::<A, B>` after a prelude constructor: its enum's type arguments.
    pub(super) fn parse_prelude_turbofish(
        &mut self,
        variant: Prelude,
    ) -> Result<Option<Vec<Ty>>, Error> {
        if !(matches!(self.peek(), Token::DoubleColon)
            && matches!(self.peek_second(), Token::LessThan))
        {
            return Ok(None);
        }
        let start = self.next().1;
        self.next();
        let mut types = vec![self.parse_local_type()?];
        while matches!(self.peek(), Token::Comma) {
            self.next();
            if matches!(self.peek(), Token::GreaterThan | Token::RightShift) {
                break;
            }
            types.push(self.parse_local_type()?);
        }
        let end = self.expect_close_angle("'>' after turbofish type arguments")?;
        let expected = match variant {
            Prelude::Some | Prelude::None => 1,
            Prelude::Ok | Prelude::Err => 2,
        };
        if types.len() != expected {
            return Err(Error::InvalidTypeArguments {
                ty: variant.owner().into(),
                expected,
                actual: types.len(),
                reference: span(start, end),
            });
        }
        Ok(Some(types))
    }

    /// `[a, b]` or `[x; n]` (an array), or the same inside `vec!` (a `Vec`),
    /// after the opening bracket. Elements are evaluated in order.
    pub(super) fn parse_array(
        &mut self,
        open: Position,
        close: Token,
        array: Option<usize>,
    ) -> Result<ParsedExpr, Error> {
        // The expected type is the literal's only when no postfix follows it
        // (`vec![Some(1)].into_iter().flatten().collect()` is not a `Vec<i64>`).
        let hint = self
            .array_hint
            .take()
            .filter(|_| !self.postfix_after_literal());
        self.frame_arrays.push(FrameArray {
            start: open,
            depth: self.frames.len(),
            noted: self.calls_noted,
            element: (self.calls_noted, 0, 0),
            bound: Vec::new(),
        });
        let parsed = self.parse_array_elements(&close, array);
        let bound = self
            .frame_arrays
            .pop()
            .map(|array| array.bound)
            .unwrap_or_default();
        let (mut elements, repeat, counted) = parsed?;
        self.bind_framed_elements(&mut elements, bound)?;
        let end = self.expect(close, "the end of an array")?;
        // Elements naming different functions of one signature meet at a
        // `fn` pointer, as `if` arms do.
        let types = elements
            .iter()
            .map(|element| self.inference.resolve(&element.ty))
            .collect::<Vec<_>>();
        let (element, skip) = match (hint, elements.first()) {
            (Some(hint), _) => (hint, 0),
            (None, Some(_)) if let Some(pointer) = self.fn_choice(&types) => (pointer, 0),
            (None, Some(first)) => (self.inference.resolve(&first.ty), 1),
            (None, None) => (self.inference.fresh(), 0),
        };
        for value in elements.iter_mut().skip(skip) {
            self.coerce_expr(&element, value)?;
        }
        for value in &mut elements {
            self.move_local(value)?;
        }
        let parts: Vec<_> = elements
            .iter()
            .map(|element| {
                (
                    element,
                    borrows::Slot::At(borrowck::Path::of(&[borrowck::Step::Arg(0)])),
                )
            })
            .collect();
        let array_ty = Ty::Array {
            element: Box::new(element.clone()),
            kind: sequence::Kind::Vec,
        };
        self.compose(span(open, end), &array_ty, &parts);
        let source = match (repeat, counted) {
            (Some(count), _) => {
                let value = elements.pop().expect("a repeat array has its value");
                let items = self.temporary("repeat");
                let mut source =
                    origin::Fragment::plain(format!("{{ let {items} = []; {items}.pad("));
                source.push_text(format!("{count}, "));
                source.push(value.rendered_source());
                source.push_text(format!("); {items} }}"));
                source
            }
            (None, Some(count)) => {
                // Rust evaluates the value, then the count.
                let value = elements.pop().expect("a repeat vector has its value");
                let (item, items) = (self.temporary("repeat"), self.temporary("repeat"));
                let mut source = origin::Fragment::plain(format!("{{ let {item} = "));
                source.push(value.rendered_source());
                source.push_text(format!("; let {items} = []; {items}.pad("));
                source.push(count.rendered_source());
                source.push_text(format!(", {item}); {items} }}"));
                #[cfg(feature = "rust-import")]
                let source = {
                    let constructed = self.sequence_source(
                        &array_ty,
                        span(open, end),
                        count.rendered_source(),
                        item.as_str().into(),
                        2,
                    );
                    let mut source = origin::Fragment::plain(format!("{{ let {item} = "));
                    source.push(value.rendered_source());
                    source.push_text("; ");
                    source.push(constructed);
                    source.push_text(" }");
                    source
                };
                source
            }
            (None, None) => {
                let source =
                    origin::Fragment::join(elements.iter().map(ParsedExpr::rendered_source), ", ")
                        .surrounded("[", "]");
                #[cfg(feature = "rust-import")]
                let source = if array.is_none() {
                    self.sequence_source(
                        &array_ty,
                        span(open, end),
                        elements.len().to_string().into(),
                        source,
                        1,
                    )
                } else {
                    source
                };
                source
            }
        };
        let kind = match array {
            Some(_) => sequence::Kind::Array(repeat.unwrap_or(elements.len())),
            None => sequence::Kind::Vec,
        };
        Ok(self.operator_expr(source, Ty::sequence(kind, element), span(open, end)))
    }

    /// An array literal's elements up to `close`, and its repeat count:
    /// a constant for an array, any `usize` for `vec![x; n]`.
    pub(super) fn parse_array_elements(
        &mut self,
        close: &Token,
        array: Option<usize>,
    ) -> Result<ArrayElements, Error> {
        let mut elements = Vec::<ParsedExpr>::new();
        let mut repeat = None;
        let mut counted = None;
        while self.peek() != *close {
            if self.element_left_out()? {
                continue;
            }
            let frames = self.frames.last().map_or(0, Vec::len);
            let element = (self.calls_noted, elements.len(), frames);
            if let Some(array) = self.frame_arrays.last_mut() {
                array.element = element;
            }
            let outer = std::mem::replace(&mut self.no_struct, false);
            let element = self.parse_expr();
            self.no_struct = outer;
            elements.push(element?);
            if matches!(self.peek(), Token::SemiColon) && elements.len() == 1 {
                self.next();
                if array.is_some() {
                    repeat = Some(self.parse_array_length("repeat count")?);
                } else {
                    let outer = std::mem::replace(&mut self.no_struct, false);
                    let count = self.parse_expr();
                    self.no_struct = outer;
                    let mut count = count?;
                    self.coerce_expr(&Ty::Int(Int::Usize), &mut count)?;
                    counted = Some(count);
                }
                break;
            }
            if matches!(self.peek(), Token::Comma) {
                self.next();
            } else if self.peek() != *close {
                return Err(self.unsupported("array elements without ','"));
            }
        }
        Ok((elements, repeat, counted))
    }

    /// Bind the elements before each of `bound`'s, whose call opens a
    /// frame ahead of the statement, ahead of that frame, in order: `let
    /// element = icon().size(12);` before the frame of
    /// `Slot::from("Verified")`, so the elements run as Rust runs them.
    pub(super) fn bind_framed_elements(
        &mut self,
        elements: &mut [ParsedExpr],
        bound: Vec<(usize, usize)>,
    ) -> Result<(), Error> {
        let mut done = 0;
        let mut added = 0;
        for (index, mark) in bound {
            let mut frames = Vec::new();
            for element in &mut elements[done..index] {
                // The element moves into its binding, as into the array.
                self.move_local(element)?;
                let name = self.temporary("element");
                let mut frame = Frame::default();
                frame.inner.push_text(format!("let {name} = "));
                frame.inner.push(element.rendered_source());
                frame.inner.push_text("; ");
                frames.push(frame);
                self.bindings.insert(
                    name.as_str().into(),
                    Binding {
                        ty: element.ty.clone(),
                        mutable: true,
                        available: true,
                        declaration: element.span,
                        kind: BindingKind::Local,
                    },
                );
                element.prelude = origin::Fragment::default();
                element.lvalue = Some(Lvalue::Binding {
                    binding: name.as_str().into(),
                    ty: element.ty.clone(),
                });
                element.source = origin::Fragment::plain(name);
            }
            done = done.max(index);
            let count = frames.len();
            if let Some(statement) = self.frames.last_mut() {
                let at = (mark + added).min(statement.len());
                statement.splice(at..at, frames);
            }
            added += count;
        }
        Ok(())
    }

    /// An array length or repeat count: an integer literal, or a `const`
    /// whose value is one (`[0; N]`, `[T; N]`), as rustc evaluates it.
    pub(super) fn parse_array_length(&mut self, what: &str) -> Result<usize, Error> {
        let (token, position) = self.next();
        let value = match &token {
            Token::IntegerConstant(length) => Some(*length),
            // `Self::N`, `Grid::N`: an associated constant of an integer.
            Token::Identifier(owner) if matches!(self.peek(), Token::DoubleColon) => {
                self.next();
                let (name, _) = self.expect_name("an associated constant in an array length")?;
                self.named_model(owner.as_str())
                    .and_then(|model| self.models.method(model.definition().id(), &name))
                    .and_then(|definition| definition.integer())
            }
            Token::Identifier(name) => self
                .models
                .function(name.as_str())
                .filter(|function| function.definition.body.constant)
                .and_then(|function| function.definition.body.syntax.integer()),
            _ => None,
        };
        value
            .and_then(|value| usize::try_from(value).ok())
            .ok_or_else(|| Error::UnsupportedNominalSyntax {
                construct: format!("{what} other than an integer literal or a `const` of one")
                    .into(),
                source: point_span(position),
            })
    }

    /// One `Option<T>` runtime primitive applied to `operands`.
    pub(super) fn prelude_op(
        &mut self,
        op: PreludeOp,
        operands: impl IntoIterator<Item = origin::Fragment>,
    ) -> origin::Fragment {
        let helper = self.push_op(Op::Prelude(op));
        origin::Fragment::call(&helper, operands)
    }

    /// `Some(value)`, `None`, `Ok(value)` or `Err(error)`, the prelude's,
    /// also as `Option::Some(value)` or `Result::Ok(value)`.
    pub(super) fn parse_prelude_constructor(
        &mut self,
        variant: Prelude,
        start: Span,
    ) -> Result<ParsedExpr, Error> {
        let turbofish = self.parse_prelude_turbofish(variant)?;
        let inferred = turbofish.is_none();
        let (ok, err) = match turbofish {
            Some(types) => {
                let mut types = types.into_iter();
                let ok = types.next().expect("turbofish arity is checked");
                (ok, types.next().unwrap_or(Ty::Unit))
            }
            None => (self.inference.fresh(), self.inference.fresh()),
        };
        if variant == Prelude::None {
            // `None` holds no borrow.
            self.flow().set_origins(start, Vec::new());
            let source = self.prelude_op(PreludeOp::None, []);
            return Ok(self.operator_expr(source, Ty::Option(Box::new(ok)), start));
        }
        if !matches!(self.peek(), Token::LeftParen) {
            return Ok(self.prelude_function(variant, ok, err, start));
        }
        let (mut arguments, end) = self.parse_call_arguments("prelude variant arguments")?;
        let expression = span(start.start(), end);
        let [value] = arguments.as_mut_slice() else {
            return Err(Error::argument_count(
                Callee::Variant,
                variant.name(),
                1,
                arguments.len(),
                expression,
            ));
        };
        let stored = match variant {
            Prelude::Some | Prelude::None => "stored in an `Option`",
            Prelude::Ok | Prelude::Err => "stored in a `Result`",
        };
        self.reject_invocation(&value.ty, value.span, stored)?;
        // The payload is the turbofish's type, or the value's own.
        let expected = match variant {
            Prelude::Err => err.clone(),
            _ => ok.clone(),
        };
        if variant == Prelude::Err && inferred {
            self.box_returned_error(value)?;
        }
        match variant {
            Prelude::Err => self.err_payloads.insert(start.start(), value.span),
            Prelude::Some => self.some_payloads.insert(start.start(), value.span),
            Prelude::Ok | Prelude::None => None,
        };
        self.coerce_expr(&expected, value)?;
        self.move_local(value)?;
        let payload = Box::new(self.inference.resolve(&value.ty));
        let (op, ty) = match variant {
            Prelude::Some => (PreludeOp::Some, Ty::Option(payload)),
            Prelude::Ok => (PreludeOp::Ok, Ty::Result(payload, Box::new(err))),
            Prelude::Err => (PreludeOp::Err, Ty::Result(Box::new(ok), payload)),
            Prelude::None => unreachable!("`None` returned above"),
        };
        // `Some(&mut self.a)` returned as an `Option<&mut T>` output keeps
        // its place (`places::returned_place`).
        let place = match (&variant, self.place_output && self.closures.is_empty()) {
            (Prelude::Some, true)
                if matches!(
                    self.inference.resolve(&value.ty),
                    Ty::Reference { mutable: true, .. }
                ) =>
            {
                value.lvalue.clone().or_else(|| {
                    (value.source.as_str() == "self").then(|| Lvalue::Binding {
                        binding: "self".into(),
                        ty: value.ty.clone(),
                    })
                })
            }
            _ => None,
        };
        let at = match variant {
            Prelude::Err => borrowck::Step::Arg(1),
            _ => borrowck::Step::Arg(0),
        };
        self.compose(
            expression,
            &ty,
            &[(&*value, borrows::Slot::At(borrowck::Path::of(&[at])))],
        );
        let source = self.prelude_op(op, [value.rendered_source()]);
        let mut value = self.operator_expr(source, ty, expression);
        value.lvalue = place;
        Ok(value)
    }

    pub(super) fn parse_struct(
        &mut self,
        name: ImmutableString,
        name_span: Span,
    ) -> Result<ParsedExpr, Error> {
        if let Some(binding) = self.models.import(&name).cloned()
            && let item::Import::Type { definition, owner } = binding.target
        {
            if definition.identity().imported().is_none() {
                return self.parse_host_struct(definition, name_span);
            }
            #[cfg(feature = "rust-import")]
            return self.parse_imported_struct(binding.qualified, definition, owner, name_span);
            #[cfg(not(feature = "rust-import"))]
            let _ = owner;
        }
        let model = self.named_model(&name);
        self.parse_struct_fields(name, model, name_span)
    }

    /// `S { a: x, b, ..base }`: fields are evaluated in source order, then
    /// `base`, which supplies the fields not named, as in Rust.
    pub(super) fn parse_struct_fields(
        &mut self,
        name: ImmutableString,
        model: Option<Nominal>,
        name_span: Span,
    ) -> Result<ParsedExpr, Error> {
        let Some(Nominal::Struct { definition, item }) = model else {
            return Err(Error::InvalidConstruction {
                owner: name,
                reason: "only declared structs use named-field construction".into(),
                construction: name_span,
            });
        };
        // A lowering that reads its fields whole is reused only while
        // they are as they were (#1885).
        #[cfg(feature = "rust-import")]
        self.models.read_fields(&definition);
        self.next();
        let model = Nominal::Struct {
            definition: definition.clone(),
            item: item.clone(),
        };
        let (struct_ty, substitution) = self.fresh_nominal(&model, name_span);
        let mut values = BTreeMap::<ImmutableString, (ParsedExpr, Span)>::new();
        let mut order = Vec::new();
        let mut base = None;

        while !matches!(self.peek(), Token::RightBrace) {
            if self.element_left_out()? {
                continue;
            }
            if matches!(self.peek(), Token::ExclusiveRange) {
                self.next();
                let mut value = self.parse_expr()?;
                self.coerce_expr(&struct_ty, &mut value)?;
                base = Some(value);
                if !matches!(self.peek(), Token::RightBrace) {
                    return Err(self.unsupported("fields after `..base` in a struct expression"));
                }
                break;
            }
            let (field, field_span) = self.expect_name("struct field initializer")?;
            order.push(field.clone());
            let value = if matches!(self.peek(), Token::Colon) {
                self.next();
                let expected = item
                    .fields
                    .iter()
                    .find(|known| known.name.text == field)
                    .map(|known| instantiate(&Ty::from_resolved(&known.ty), &substitution));
                self.parse_expr_expecting(expected.as_ref())?
            } else {
                self.parse_name_primary(field.clone(), field_span)?
            };
            if values.insert(field.clone(), (value, field_span)).is_some() {
                return Err(Error::InvalidConstruction {
                    owner: definition.name().into(),
                    reason: format!("field `{field}` is initialized more than once").into(),
                    construction: field_span,
                });
            }
            if matches!(self.peek(), Token::Comma) {
                self.next();
            } else if !matches!(self.peek(), Token::RightBrace) {
                return Err(self.unsupported("struct fields without ','"));
            }
        }
        let end = self.expect(Token::RightBrace, "'}' after struct fields")?;
        if let Some((field, (_, field_span))) = values
            .iter()
            .find(|(field, _)| !item.fields.iter().any(|known| known.name.text == **field))
        {
            return Err(Error::UnknownMember {
                owner: definition.name().into(),
                member: field.clone(),
                reference: *field_span,
            });
        }
        // Each value is bound in source order before construction.
        let mut prelude = origin::Fragment::plain("{\n");
        let mut temporaries = BTreeMap::new();
        for field in &order {
            let (value, _) = values.get_mut(field).expect("ordered fields were parsed");
            let declared = item
                .fields
                .iter()
                .find(|known| known.name.text == *field)
                .expect("unknown fields were rejected");
            let expected = instantiate(&Ty::from_resolved(&declared.ty), &substitution);
            self.coerce_expr(&expected, value)?;
            self.move_local(value)?;
            let temporary = self.temporary("field");
            prelude.push_text(format!("let {temporary} = "));
            prelude.push(value.rendered_source());
            prelude.push_text(";\n");
            temporaries.insert(field.clone(), temporary);
        }
        // `..base` moves the fields it gives out of `base`: those with drop
        // glue are left `()` there, and the rest of a value made for it
        // drops with the statement's temporaries.
        let mut base_clears = String::new();
        if let Some(base) = base.as_mut()
            && self.drops()
        {
            let mut taken = Vec::new();
            for (slot, field) in item.fields.iter().enumerate() {
                let ty = instantiate(&Ty::from_resolved(&field.ty), &substitution);
                if !temporaries.contains_key(field.name.text.as_str())
                    && self.needs_drop(&ty, base.span)?
                {
                    taken.push(slot);
                }
            }
            if !taken.is_empty() {
                self.reject_drop_move(definition.id(), true, base.span)?;
                let holder = match &base.lvalue {
                    Some(Lvalue::Binding { binding, .. }) => Some(binding.to_string()),
                    None => self.hold_temporary(base)?.map(|held| held.to_string()),
                    Some(_) => None,
                };
                let Some(holder) = holder else {
                    return Err(Error::UnsupportedNominalSyntax {
                        construct: "`..base` of a place other than a local, with fields whose \
                                    type implements `Drop`: not supported in Rawr yet"
                            .into(),
                        source: base.span,
                    });
                };
                for slot in taken {
                    let _ = write!(
                        base_clears,
                        "{}({holder}, {slot}); ",
                        crate::engine::KEYWORD_CLEAR
                    );
                }
            }
        }
        let base_name = self.temporary("base");
        if let Some(base) = &base {
            prelude.push_text(format!("let {base_name} = "));
            prelude.push(base.rendered_source());
            prelude.push_text(";\n");
            prelude.push_text(base_clears);
        }
        let mut ordered = Vec::with_capacity(item.fields.len());
        for (slot, field) in item.fields.iter().enumerate() {
            if let Some(temporary) = temporaries.get(field.name.text.as_str()) {
                ordered.push(origin::Fragment::plain(temporary.clone()));
            } else if base.is_some() {
                ordered.push(field_place(
                    origin::Fragment::plain(base_name.as_str()),
                    slot,
                ));
            } else {
                return Err(Error::InvalidConstruction {
                    owner: definition.name().into(),
                    reason: format!("missing field `{}`", field.name.text).into(),
                    construction: span(name_span.start(), end),
                });
            }
        }
        let mut parts = Vec::new();
        for (name, (value, _)) in &values {
            let slot = item
                .fields
                .iter()
                .position(|field| field.name.text == *name);
            parts.push((
                value,
                match slot {
                    Some(slot) => borrows::Slot::Field(definition.id(), None, slot),
                    None => borrows::Slot::At(borrowck::Path::ROOT),
                },
            ));
        }
        parts.extend(base.iter().map(|base| (base, borrows::Slot::Whole)));
        self.compose(span(name_span.start(), end), &struct_ty, &parts);
        let declared_order = base.is_none()
            && order
                .iter()
                .zip(item.fields.iter())
                .all(|(field, declared)| *field == declared.name.text);
        let helper = self.push_op(Op::Struct {
            definition: definition.clone(),
            item: Box::new(item),
        });
        let source = if declared_order {
            let values = order.iter().map(|field| {
                let (value, _) = values.remove(field).expect("ordered fields were parsed");
                value.rendered_source()
            });
            origin::Fragment::call(&helper, values.collect::<Vec<_>>())
        } else {
            prelude.push(origin::Fragment::call(&helper, ordered));
            prelude.push_text("\n}");
            prelude.surrounded("(", ")")
        };
        Ok(ParsedExpr {
            prelude: origin::Fragment::default(),
            source,
            ty: struct_ty,
            span: span(name_span.start(), end),
            lvalue: None,
            outcome: crate::host::usage::Outcome::Conservative,
            evaluation: crate::host::usage::Evaluation::Required,
        }
        .retain_origin())
    }

    /// `{ x, y: value }` after a struct variant: `helper` called with the
    /// values in declared order, each evaluated in written order, as Rust
    /// does; with the position of the `}`.
    pub(super) fn parse_named_values(
        &mut self,
        definition: &type_def::Definition,
        fields: &[(ImmutableString, Ty)],
        helper: &str,
    ) -> Result<(origin::Fragment, Position), Error> {
        self.expect(Token::LeftBrace, "'{' after a struct variant")?;
        let mut written = Vec::<(usize, ParsedExpr)>::new();
        while !matches!(self.peek(), Token::RightBrace) {
            let (field, field_span) = self.expect_name("variant field initializer")?;
            let Some(slot) = fields.iter().position(|(known, _)| *known == field) else {
                return Err(Error::UnknownMember {
                    owner: definition.name().into(),
                    member: field,
                    reference: field_span,
                });
            };
            if written.iter().any(|(known, _)| *known == slot) {
                return Err(Error::InvalidConstruction {
                    owner: definition.name().into(),
                    reason: format!("field `{field}` is initialized more than once").into(),
                    construction: field_span,
                });
            }
            let expected = &fields[slot].1;
            let mut value = if matches!(self.peek(), Token::Colon) {
                self.next();
                self.parse_expr_expecting(Some(expected))?
            } else {
                self.parse_name_primary(field, field_span)?
            };
            self.coerce_expr(expected, &mut value)?;
            self.move_local(&mut value)?;
            written.push((slot, value));
            if matches!(self.peek(), Token::Comma) {
                self.next();
            } else if !matches!(self.peek(), Token::RightBrace) {
                return Err(self.unsupported("variant fields without ','"));
            }
        }
        let end = self.expect(Token::RightBrace, "'}' after variant fields")?;
        if let Some((missing, _)) = (0..fields.len()).find_map(|slot| {
            (!written.iter().any(|(known, _)| *known == slot)).then(|| &fields[slot])
        }) {
            return Err(Error::InvalidConstruction {
                owner: definition.name().into(),
                reason: format!("missing field `{missing}`").into(),
                construction: point_span(end),
            });
        }
        if written
            .iter()
            .enumerate()
            .all(|(index, (slot, _))| index == *slot)
        {
            let values = written.iter().map(|(_, value)| value.rendered_source());
            return Ok((
                origin::Fragment::call(helper, values.collect::<Vec<_>>()),
                end,
            ));
        }
        let mut source = origin::Fragment::plain("({\n");
        let mut temporaries = vec![String::new(); fields.len()];
        for (slot, value) in &written {
            let temporary = self.temporary("field");
            source.push_text(format!("let {temporary} = "));
            source.push(value.rendered_source());
            source.push_text(";\n");
            temporaries[*slot] = temporary;
        }
        source.push(origin::Fragment::call(
            helper,
            temporaries
                .into_iter()
                .map(origin::Fragment::plain)
                .collect::<Vec<_>>(),
        ));
        source.push_text("\n})");
        Ok((source, end))
    }

    /// `T { a: x, ..base }` for a facade type: `base.a(x)` through the
    /// facade's field builders, after binding the values and then `base`
    /// in source order; without `..base`, it starts from `T::default()`.
    pub(super) fn parse_host_struct(
        &mut self,
        definition: crate::host::type_def::Definition,
        name_span: Span,
    ) -> Result<ParsedExpr, Error> {
        self.expect(Token::LeftBrace, "'{' after a struct name")?;
        let mut fields = Vec::<(ImmutableString, Span, ParsedExpr)>::new();
        let mut base = None;
        while !matches!(self.peek(), Token::RightBrace) {
            if self.element_left_out()? {
                continue;
            }
            if matches!(self.peek(), Token::ExclusiveRange) {
                self.next();
                base = Some(self.parse_expr()?);
                if !matches!(self.peek(), Token::RightBrace) {
                    return Err(self.unsupported("fields after `..base` in a struct expression"));
                }
                break;
            }
            let (field, field_span) = self.expect_name("struct field initializer")?;
            let value = if matches!(self.peek(), Token::Colon) {
                self.next();
                self.parse_expr()?
            } else {
                self.parse_name_primary(field.clone(), field_span)?
            };
            if fields.iter().any(|(name, _, _)| *name == field) {
                return Err(Error::InvalidConstruction {
                    owner: definition.path().into(),
                    reason: format!("field `{field}` is initialized more than once").into(),
                    construction: field_span,
                });
            }
            fields.push((field, field_span, value));
            if matches!(self.peek(), Token::Comma) {
                self.next();
            } else if !matches!(self.peek(), Token::RightBrace) {
                return Err(self.unsupported("struct fields without ','"));
            }
        }
        let end = self.expect(Token::RightBrace, "'}' after struct fields")?;
        let mut prelude = origin::Fragment::plain("({\n");
        let mut values = Vec::new();
        // Each field's declared type, which a value no builder takes is
        // reported against.
        let mut declared_types = Vec::new();
        for (field, field_span, value) in fields {
            // The field's declared type is the value's expected type, as in
            // Rust: `background: Some(color.into())` converts into the
            // field's `Option<Background>`. A value it does not fit still
            // reaches the field's builders. The type the field reads as is
            // its declared type where a builder also takes it. A facade's
            // `f64` and `i64` stand for iced's narrower numbers, so a number
            // keeps its own type.
            let members = self
                .models
                .host
                .members(&definition.path())
                .filter(|item| item.path_ref().name() == field.as_str())
                .collect::<Vec<_>>();
            let reader = members
                .iter()
                .find(|item| {
                    item.receiver() == Some(crate::receiver::Mode::Shared)
                        && item.signature().parameters().is_empty()
                })
                .map(|item| item.signature().output().clone())
                .filter(|output| {
                    !matches!(output.shape(), crate::host::type_ref::Shape::Builtin(_))
                        && members.iter().any(|item| {
                        matches!(item.signature().parameters(), [input] if input == output)
                    })
                });
            let mut field_type = None;
            if let Some(declared) = reader
                && let Ok(declared) = self.instantiate_host_type(
                    &declared,
                    &mut BTreeMap::new(),
                    field.as_str(),
                    Span::NONE,
                    None,
                    Some(&definition),
                )
            {
                let checkpoint = self.inference.checkpoint();
                if self.unify_type(&declared, &value.ty) {
                    self.inference.commit(checkpoint);
                } else {
                    self.inference.rollback(checkpoint);
                }
                field_type = Some(declared);
            }
            declared_types.push(field_type);
            let temporary = self.temporary("field");
            prelude.push_text(format!("let {temporary} = "));
            prelude.push(value.rendered_source());
            prelude.push_text(";\n");
            let bound =
                self.operator_expr(origin::Fragment::plain(temporary), value.ty, value.span);
            values.push((field, field_span, bound));
        }
        let mut value = match base {
            Some(base) => {
                // `..Default::default()` is a value of the struct, as Rust
                // infers it from the literal.
                if let Ok(own) = crate::host::type_ref::Type::registered(definition.path().as_str())
                    && let Ok(own) = self.instantiate_host_type(
                        &own,
                        &mut BTreeMap::new(),
                        "struct base",
                        Span::NONE,
                        None,
                        Some(&definition),
                    )
                {
                    let checkpoint = self.inference.checkpoint();
                    if self.unify_type(&own, &base.ty) {
                        self.inference.commit(checkpoint);
                    } else {
                        self.inference.rollback(checkpoint);
                    }
                }
                let temporary = self.temporary("base");
                prelude.push_text(format!("let {temporary} = "));
                prelude.push(base.rendered_source());
                prelude.push_text(";\n");
                self.operator_expr(origin::Fragment::plain(temporary), base.ty, base.span)
            }
            None => {
                // Rust names every field of a literal without `..base`
                // (E0063): the fields the facade reads, all of them known.
                let fields = self
                    .models
                    .host
                    .members(&definition.path())
                    .filter(|item| {
                        item.receiver() == Some(crate::receiver::Mode::Shared)
                            && item.signature().parameters().is_empty()
                    })
                    .map(|item| ImmutableString::from(item.path_ref().name()))
                    .collect::<Vec<_>>();
                let missing = fields
                    .iter()
                    .filter(|field| !values.iter().any(|(name, ..)| name == *field))
                    .map(|field| format!("`{field}`"))
                    .collect::<Vec<_>>();
                if fields.is_empty() || !missing.is_empty() {
                    return Err(Error::InvalidConstruction {
                        owner: definition.path().into(),
                        reason: if fields.is_empty() {
                            "Rust needs every field or `..base` (E0063); write `..base`".into()
                        } else {
                            format!("missing fields {} (E0063)", missing.join(", ")).into()
                        },
                        construction: name_span,
                    });
                }
                let path = format!("{}::default", definition.path());
                let Some(items) = self.models.host_items(&path).map(<[_]>::to_vec) else {
                    return Err(Error::InvalidConstruction {
                        owner: definition.path().into(),
                        reason: "this struct needs every field, or `..base` from a value of it"
                            .into(),
                        construction: name_span,
                    });
                };
                let owner = self.models.item_owner(&path).cloned();
                self.pending_arguments = Some((Vec::new(), name_span.end()));
                self.parse_host_function(items.into_boxed_slice(), owner, name_span)?
            }
        };
        for ((field, field_span, bound), declared) in values.into_iter().zip(declared_types) {
            // A value no builder of the field takes is a mismatch with the
            // field's type, at the value, as rustc reports it (E0308).
            let mismatch = declared.map(|declared| {
                (
                    self.inference.resolve(&declared).name(&self.models),
                    self.inference.resolve(&bound.ty).name(&self.models),
                    bound.span,
                )
            });
            self.pending_arguments = Some((vec![bound], end));
            value = match (self.parse_host_method(value, field, field_span), mismatch) {
                (Ok(value), _) => value,
                (Err(Error::InvalidHostOperation { .. }), Some((expected, actual, expression))) => {
                    return Err(Error::TypeMismatch {
                        expected,
                        actual,
                        expression,
                    });
                }
                (Err(error), _) => return Err(error),
            };
        }
        // An enum's struct variant (`HandleShape::Rectangle { .. }`) is a
        // builder of its own, which its `into` makes the enum.
        let path = definition.path();
        if let Some((owner, _)) = path.rsplit_once("::")
            && self.models.host.source_type(owner).is_some()
        {
            self.pending_arguments = Some((Vec::new(), end));
            value = self.parse_host_method(value, "into".into(), name_span)?;
        }
        prelude.push(value.rendered_source());
        prelude.push_text("\n})");
        let ty = value.ty;
        Ok(ParsedExpr {
            prelude: origin::Fragment::default(),
            source: prelude,
            ty,
            span: span(name_span.start(), end),
            lvalue: None,
            outcome: crate::host::usage::Outcome::Conservative,
            evaluation: crate::host::usage::Evaluation::Required,
        }
        .retain_origin())
    }

    /// A crate's struct that `path` names in the value namespace, as rustc
    /// reads `badge::Secondary` and `badge::Wrap(1)`: its constructor. An
    /// uncalled tuple struct is its constructor as a function value, the
    /// closure that calls it by the path as `written`, as `.map(Wrap)`
    /// passes it.
    #[cfg(feature = "rust-import")]
    pub(super) fn imported_struct_path(
        &mut self,
        path: &str,
        written: &[(&ImmutableString, Span)],
        path_span: Span,
    ) -> Result<Option<ParsedExpr>, Error> {
        let Some(definition) = self
            .models
            .host
            .source_type(path)
            .filter(|definition| definition.identity().imported().is_some())
            .cloned()
        else {
            return Ok(None);
        };
        let owner = self.models.host.type_owner(path).cloned();
        self.imported_struct_value(path.into(), definition, owner, written, path_span)
    }

    /// [`Self::imported_struct_path`] for the struct `definition` at `path`.
    #[cfg(feature = "rust-import")]
    pub(super) fn imported_struct_value(
        &mut self,
        path: ImmutableString,
        definition: crate::host::type_def::Definition,
        owner: Option<crate::host::facade::Dependency>,
        written: &[(&ImmutableString, Span)],
        path_span: Span,
    ) -> Result<Option<ParsedExpr>, Error> {
        let crate::host::type_def::Shape::Struct { form, fields, .. } = definition.shape() else {
            return Ok(None);
        };
        let called = matches!(self.peek(), Token::LeftParen | Token::Unit);
        match form {
            crate::host::type_def::Form::Unit if !matches!(self.peek(), Token::DoubleColon) => {}
            crate::host::type_def::Form::Tuple if called => {}
            crate::host::type_def::Form::Tuple
                if !fields.is_empty()
                    && !matches!(self.peek(), Token::DoubleColon)
                    && (self.no_struct || !matches!(self.peek(), Token::LeftBrace)) =>
            {
                let at = path_span.start();
                let parameter =
                    |index: usize| Token::Identifier(Box::new(format!("__a{index}").into()));
                let arity = fields.len();
                let mut tokens = Vec::new();
                for index in 0..arity {
                    if index > 0 {
                        tokens.push((Token::Comma, at));
                    }
                    tokens.push((parameter(index), at));
                }
                tokens.push((Token::Pipe, at));
                for (index, (name, name_span)) in written.iter().enumerate() {
                    if index > 0 {
                        tokens.push((Token::DoubleColon, name_span.start()));
                    }
                    tokens.push((
                        Token::Identifier(Box::new(name.as_str().into())),
                        name_span.start(),
                    ));
                }
                tokens.push((Token::LeftParen, at));
                for index in 0..arity {
                    if index > 0 {
                        tokens.push((Token::Comma, at));
                    }
                    tokens.push((parameter(index), at));
                }
                tokens.push((Token::RightParen, at));
                self.splice_tokens(tokens);
                return self.parse_closure(Token::Pipe, path_span, false).map(Some);
            }
            _ => return Ok(None),
        }
        self.parse_imported_struct(path, definition, owner, path_span)
            .map(Some)
    }

    #[cfg(feature = "rust-import")]
    pub(super) fn parse_imported_struct(
        &mut self,
        path: ImmutableString,
        definition: crate::host::type_def::Definition,
        owner: Option<crate::host::facade::Dependency>,
        name_span: Span,
    ) -> Result<ParsedExpr, Error> {
        let crate::host::type_def::Shape::Struct {
            form,
            fields,
            constructible,
            unsupported,
            ..
        } = definition.shape()
        else {
            return Err(Error::InvalidConstruction {
                owner: path,
                reason: "only admitted imported structs use named-field construction".into(),
                construction: name_span,
            });
        };
        // A public field whose type rawr can't hold yet is named with its
        // type, as the crate writes it: the literal is valid Rust (D664).
        if let Some(unsupported) = unsupported {
            return Err(Error::InvalidConstruction {
                owner: path,
                reason: unsupported_field(unsupported).into(),
                construction: name_span,
            });
        }
        if !constructible {
            return Err(Error::InvalidConstruction {
                owner: path,
                reason: "private Rust fields make this struct non-constructible".into(),
                construction: name_span,
            });
        }
        // A constructor infers its parameters from its fields and context;
        // defaults only fill parameters those constraints leave unknown.
        let instance = definition
            .parameters()
            .iter()
            .map(|_| self.inference.fresh())
            .collect::<Box<[_]>>();
        if let Some((expected, depth)) = &self.collect_hint
            && *depth == self.expression_stack.len()
            && let Ty::Host { proof, arguments } = self.inference.resolve(expected)
            && proof.definition.identity() == definition.identity()
        {
            for (parameter, argument) in instance.iter().zip(&arguments) {
                self.require_type(parameter, argument, name_span)?;
            }
        }
        let fields = fields.clone();
        let form = *form;
        let selected_owner = self.models.host.type_owner(&path).cloned();
        let mut base = None::<ParsedExpr>;
        let mut prelude = origin::Fragment::default();
        let (mut arguments, end) = match form {
            crate::host::type_def::Form::Unit => (Vec::new(), name_span.end()),
            crate::host::type_def::Form::Tuple => {
                let mut arguments = Vec::new();
                let end = if matches!(self.peek(), Token::Unit) {
                    self.next().1
                } else {
                    self.expect(Token::LeftParen, "'(' after imported tuple struct")?;
                    while !matches!(self.peek(), Token::RightParen) {
                        if self.element_left_out()? {
                            continue;
                        }
                        arguments.push(self.parse_expr()?);
                        if matches!(self.peek(), Token::Comma) {
                            self.next();
                        } else {
                            break;
                        }
                    }
                    self.expect(Token::RightParen, "')' after imported tuple struct")?
                };
                (arguments, end)
            }
            crate::host::type_def::Form::Record => {
                self.expect(Token::LeftBrace, "'{' after imported record struct")?;
                let mut values = BTreeMap::<ImmutableString, (ParsedExpr, Span)>::new();
                let mut order = Vec::new();
                while !matches!(self.peek(), Token::RightBrace) {
                    if self.element_left_out()? {
                        continue;
                    }
                    if matches!(self.peek(), Token::ExclusiveRange) {
                        self.next();
                        base = Some(self.parse_expr()?);
                        if !matches!(self.peek(), Token::RightBrace) {
                            return Err(
                                self.unsupported("fields after `..base` in a struct expression")
                            );
                        }
                        break;
                    }
                    let (field, field_span) =
                        self.expect_name("imported struct field initializer")?;
                    order.push(field.clone());
                    let value = if matches!(self.peek(), Token::Colon) {
                        self.next();
                        self.parse_expr()?
                    } else {
                        self.parse_name_primary(field.clone(), field_span)?
                    };
                    if values.insert(field.clone(), (value, field_span)).is_some() {
                        return Err(Error::InvalidConstruction {
                            owner: path,
                            reason: format!("field `{field}` is initialized more than once").into(),
                            construction: field_span,
                        });
                    }
                    if matches!(self.peek(), Token::Comma) {
                        self.next();
                    } else if !matches!(self.peek(), Token::RightBrace) {
                        return Err(self.unsupported("imported struct fields without ','"));
                    }
                }
                let end = self.expect(Token::RightBrace, "'}' after imported struct fields")?;
                // With `..base`, the named fields and then `base` are bound
                // in source order; `base` supplies the rest.
                let base_value = match base.take() {
                    Some(mut base) => {
                        let ty = Ty::Host {
                            proof: HostTyProof::new(
                                definition.clone(),
                                Some(definition.clone()),
                                owner.clone(),
                            ),
                            arguments: instance.clone(),
                        };
                        self.coerce_expr(&ty, &mut base)?;
                        for field in &order {
                            let (value, _) = values.get_mut(field).expect("parsed fields remain");
                            let temporary = self.temporary("field");
                            prelude.push_text(format!("let {temporary} = "));
                            prelude.push(value.rendered_source());
                            prelude.push_text(";\n");
                            value.prelude = origin::Fragment::default();
                            value.source = origin::Fragment::plain(temporary);
                        }
                        let temporary = self.temporary("base");
                        prelude.push_text(format!("let {temporary} = "));
                        prelude.push(base.rendered_source());
                        prelude.push_text(";\n");
                        base.prelude = origin::Fragment::default();
                        base.source = origin::Fragment::plain(temporary);
                        Some(base)
                    }
                    None => None,
                };
                // The named fields fix what they can of the type's
                // arguments first, so a field `base` gives is read knowing
                // whether it is `Copy`, as rustc decides it at the end.
                if base_value.is_some() {
                    let mut parameters = definition
                        .parameters()
                        .iter()
                        .cloned()
                        .zip(instance.iter().cloned())
                        .collect::<BTreeMap<_, _>>();
                    for field in &fields {
                        let Some((value, _)) = values.get(field.name()) else {
                            continue;
                        };
                        let actual = value.ty.clone();
                        let expected = self.instantiate_host_type(
                            field.ty(),
                            &mut parameters,
                            &path,
                            value.span,
                            selected_owner.as_ref(),
                            Some(&definition),
                        )?;
                        if self.unifies(&expected, &actual) {
                            self.unify_type(&expected, &actual);
                        }
                    }
                }
                let mut arguments = Vec::with_capacity(fields.len());
                for field in &fields {
                    if let (None, Some(base)) = (values.get(field.name()), &base_value) {
                        let read = self.read_imported_field(
                            ParsedExpr {
                                prelude: origin::Fragment::default(),
                                source: base.source.clone(),
                                ty: base.ty.clone(),
                                span: base.span,
                                lvalue: None,
                                outcome: crate::host::usage::Outcome::Conservative,
                                evaluation: crate::host::usage::Evaluation::Required,
                            },
                            definition.clone(),
                            field.name().into(),
                            base.span,
                        )?;
                        arguments.push(read);
                        continue;
                    }
                    let Some((value, _)) = values.remove(field.name()) else {
                        return Err(Error::InvalidConstruction {
                            owner: path,
                            reason: format!("missing field `{}`", field.name()).into(),
                            construction: span(name_span.start(), end),
                        });
                    };
                    arguments.push(value);
                }
                if let Some((field, (_, field_span))) = values.into_iter().next() {
                    return Err(Error::UnknownMember {
                        owner: path,
                        member: field,
                        reference: field_span,
                    });
                }
                (arguments, end)
            }
        };
        if arguments.len() != fields.len() {
            return Err(Error::InvalidConstruction {
                owner: path,
                reason: format!(
                    "imported struct expects {} value(s), found {}",
                    fields.len(),
                    arguments.len()
                )
                .into(),
                construction: span(name_span.start(), end),
            });
        }
        let mut parameters = definition
            .parameters()
            .iter()
            .cloned()
            .zip(instance.iter().cloned())
            .collect::<BTreeMap<_, _>>();
        for (argument, field) in arguments.iter_mut().zip(&fields) {
            let expected = self.instantiate_host_type(
                field.ty(),
                &mut parameters,
                &path,
                argument.span,
                selected_owner.as_ref(),
                Some(&definition),
            )?;
            self.coerce_expr(&expected, argument)?;
            self.consume_imported_value(argument, &path, argument.span)?;
            // A field the crate keeps for the program's life, as tracing's
            // `Identifier(pub &'static dyn Callsite)`: what the value
            // borrows is borrowed for `'static`, which a local's borrow is
            // not (rustc's E0597, #1620).
            // The flow's guard ends with the statement that copies `held`.
            if lasting(field.ty()) {
                let held = self.flow().origins(argument.span).map(<[_]>::to_vec);
                if let Some(held) = held {
                    self.flow().forever(&held);
                }
            }
        }
        for (index, parameter) in instance.iter().enumerate() {
            if matches!(self.inference.resolve(parameter), Ty::Infer(_))
                && let Some(Some(default)) = definition.defaults().get(index)
            {
                let default = self.instantiate_host_type(
                    default,
                    &mut parameters,
                    &path,
                    name_span,
                    owner.as_ref(),
                    Some(&definition),
                )?;
                self.structural_defaults
                    .push((parameter.clone(), default, name_span));
            }
        }
        let ordered = arguments
            .iter()
            .map(ParsedExpr::rendered_source)
            .collect::<Vec<_>>();
        let construction = span(name_span.start(), end);
        let selector =
            definition
                .imported_constructor()
                .ok_or_else(|| Error::InvalidHostOperation {
                    item: path.clone(),
                    reason: "constructing it with a struct literal is not supported in Rawr yet"
                        .into(),
                    operation: construction,
                })?;
        let output = Ty::Host {
            proof: HostTyProof::new(definition.clone(), Some(definition.clone()), owner),
            arguments: instance.clone(),
        };
        let imported = ImportedOperation {
            kind: crate::host::structural::Kind::Constructor,
            operands: arguments
                .iter()
                .map(|argument| (argument.ty.clone(), argument.span, false))
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            output: output.clone(),
            receiver: None,
        };
        let helper = self.push_imported_op(&path, selector, Some(imported), construction)?;
        let mut source = origin::Fragment::call(&helper, ordered);
        if !prelude.is_empty() {
            prelude.push(source);
            source = prelude.surrounded("({\n", "\n})");
        }
        Ok(ParsedExpr {
            prelude: origin::Fragment::default(),
            source,
            ty: output,
            span: construction,
            lvalue: None,
            outcome: crate::host::usage::Outcome::Conservative,
            evaluation: crate::host::usage::Evaluation::Required,
        }
        .retain_origin())
    }
}

/// Whether a crate's field of type `ty` holds a borrow at `'static`: a
/// `&'static dyn Trait` (D872).
#[cfg(feature = "rust-import")]
fn lasting(ty: &crate::host::type_ref::Type) -> bool {
    matches!(
        ty.shape(),
        crate::host::type_ref::Shape::Object {
            pointer: rawr_import::classify::representability::object::Pointer::Lasting,
            ..
        }
    )
}
