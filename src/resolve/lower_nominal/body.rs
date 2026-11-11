//! Function and method bodies: reading one, its output (an opaque `impl
//! Trait` revealed by the body), and the passes that settle it once read.

use super::*;

impl<'a> Parser<'a> {
    pub(super) fn parse(&mut self) -> Result<Lowered, Error> {
        if self.peek_identifier().is_some_and(|name| {
            name.starts_with(SENTINEL)
                || name.starts_with(METHOD_SENTINEL)
                || name.starts_with(NOMINAL_MATCH_SENTINEL)
        }) {
            return Err(self.unsupported("compiler-private nominal intrinsic"));
        }

        // The script's statements are a body: its locals drop at its end,
        // after its final value, as `fn main`'s do.
        self.open_body_drops([])?;
        while !matches!(self.peek(), Token::EOF) {
            let result = match self.peek() {
                Token::Let => self.parse_binding(true),
                Token::Const => self.parse_binding(false),
                _ if self.static_ahead() => self.parse_block_statics(),
                _ if self.macro_definition_ahead() => self.parse_local_macro(),
                _ if self.macro_statement_ahead() => self.parse_macro_statement(),
                _ => self.parse_statement(),
            };

            result?;
        }
        if let Some((start, span)) = self.last_expression {
            let mut value = self.take_tail(start, span);
            self.move_out_value(&mut value)?;
            let start = self.write_output(value.rendered_source(), span);
            self.last_expression = Some((start, span));
        }
        self.close_body_drops()?;

        self.finish_uninitialized()?;

        self.check_borrows()?;
        self.finish_conversions()?;
        self.finish_bounds()?;
        self.finish_witnesses()?;
        let ops = self.finish_ops()?;
        let (mut source, locations) = std::mem::take(&mut self.output).finish();
        // As in a block, a script whose last statement ends with `;` has
        // the value `()`.
        if source.trim_end().ends_with(';') {
            source.push_str("\n()");
        }

        Ok(Lowered {
            source,
            locations,
            ops,
            patterns: std::mem::take(&mut self.patterns),
            closures: self.take_lifted(),
        })
    }

    pub(super) fn parse_method_body(
        &mut self,
        definition: &method::Definition,
    ) -> Result<Lowered, Error> {
        self.method_body(definition)
    }

    pub(super) fn method_body(
        &mut self,
        definition: &method::Definition,
    ) -> Result<Lowered, Error> {
        self.check_method_signature(definition)?;
        // The value of an associated `const NAME: T = value;` is a tail.
        if definition.constant() {
            self.flow().constant();
            self.parse_statement()?;
            if !matches!(self.peek(), Token::EOF) {
                return Err(self.unsupported("tokens after an associated const"));
            }
            let expected = Ty::from_resolved(definition.output());
            self.coerce_tail(&expected, definition.body())?;
            self.finish_uninitialized()?;
            self.check_borrows()?;
            self.finish_conversions()?;
            self.finish_bounds()?;
            self.finish_witnesses()?;
            let ops = self.finish_ops()?;
            let (source, locations) = std::mem::take(&mut self.output).finish();
            return Ok(Lowered {
                source,
                locations,
                ops,
                patterns: std::mem::take(&mut self.patterns),
                closures: self.take_lifted(),
            });
        }
        if definition.asynchronous() {
            if definition.receiver().is_some() && definition.builtin_owner().is_none() {
                let model = self.models.definition(definition.owner()).clone();
                let helper = self.push_op(Op::Validate {
                    definition: model.definition().clone(),
                });
                self.write_output(format!("{helper}(self);"), definition.body());
            }
            let receiver = self
                .bindings
                .get("self")
                .filter(|binding| !matches!(binding.ty, Ty::Reference { .. }))
                .map(|binding| (binding.ty.clone(), binding.declaration));
            self.refuse_async_drops(
                receiver.into_iter().chain(
                    definition
                        .parameters()
                        .iter()
                        .map(|parameter| (Ty::from_resolved(&parameter.ty), parameter.name.span)),
                ),
            )?;
            self.destructure_parameters(definition.parameters(), definition.body())?;
            let output = Ty::from_resolved(definition.output());
            self.parse_async_body(&output, definition.body())?;
            if !matches!(self.peek(), Token::EOF) {
                return Err(self.unsupported("tokens after a method body"));
            }
            self.coerce_tail(&output, definition.body())?;
            self.finish_uninitialized()?;
            self.check_borrows()?;
            self.finish_conversions()?;
            self.finish_bounds()?;
            self.finish_witnesses()?;
            let ops = self.finish_ops()?;
            let (source, locations) = std::mem::take(&mut self.output).finish();
            return Ok(Lowered {
                source,
                locations,
                ops,
                patterns: std::mem::take(&mut self.patterns),
                closures: self.take_lifted(),
            });
        }
        self.expect(Token::LeftBrace, "'{' to start a method body")?;

        if definition.receiver().is_some() && definition.builtin_owner().is_none() {
            let model = self.models.definition(definition.owner()).clone();
            let helper = self.push_op(Op::Validate {
                definition: model.definition().clone(),
            });
            self.write_output(format!("{helper}(self);"), definition.body());
        }
        let receiver = definition
            .receiver_span()
            .filter(|_| definition.receiver().is_some())
            .map(|span| (ImmutableString::from("self"), span));
        self.open_body_drops(
            receiver.into_iter().chain(
                definition
                    .parameters()
                    .iter()
                    .map(|parameter| (parameter.name.text.clone(), parameter.name.span)),
            ),
        )?;
        self.destructure_parameters(definition.parameters(), definition.body())?;

        while !matches!(self.peek(), Token::RightBrace | Token::EOF) {
            match self.peek() {
                Token::Let => self.parse_binding(true)?,
                // `const fn f()` is a nested `fn` item.
                Token::Const if matches!(self.peek_second(), Token::Fn) => {
                    self.next();
                    self.skip_nested_fn()?;
                }
                Token::Const => self.parse_binding(false)?,
                _ if self.static_ahead() => self.parse_block_statics()?,
                Token::Fn => self.skip_nested_fn()?,
                Token::Use => self.skip_nested_use()?,
                _ if self.macro_definition_ahead() => self.parse_local_macro()?,
                _ if self.macro_statement_ahead() => self.parse_macro_statement()?,
                _ if self.nested_item_ahead() => self.skip_nested_item()?,
                _ => self.parse_statement()?,
            }
        }

        self.expect(Token::RightBrace, "'}' to close a method body")?;
        if !matches!(self.peek(), Token::EOF) {
            return Err(self.unsupported("tokens after a method body"));
        }

        #[cfg(feature = "rust-import")]
        self.settle_collected_tail(Some(definition.output()));
        let expected = self.tail_expected(Some(definition.output()));
        self.coerce_tail(&expected, definition.body())?;
        #[cfg(feature = "rust-import")]
        self.settle_opaque(definition.output());
        self.close_body_drops()?;
        self.wrap_formatter(definition.body());

        self.finish_uninitialized()?;

        self.check_borrows()?;
        self.finish_conversions()?;
        self.finish_bounds()?;
        self.finish_witnesses()?;
        let ops = self.finish_ops()?;
        let (source, locations) = std::mem::take(&mut self.output).finish();

        Ok(Lowered {
            source,
            locations,
            ops,
            patterns: std::mem::take(&mut self.patterns),
            closures: self.take_lifted(),
        })
    }

    /// The type a body's tail is checked against: its declared output, or
    /// the output being revealed.
    pub(super) fn tail_expected(&self, output: Option<&type_ref::Type>) -> Ty {
        #[cfg(feature = "rust-import")]
        if let Some(revealing) = &self.revealing {
            return revealing.clone();
        }
        output.map_or(Ty::Unit, Ty::from_resolved)
    }

    /// D604: the type parameters of `hidden`, the body's type for a return
    /// type's `impl Trait<..>`, that only the trait's defaults and the
    /// crate's impls decide, as `fn view(&self) -> impl Widget<Message>`
    /// leaves the `Renderer` of `Column<Element<Message, Theme, _>>`. The
    /// trait's written arguments and its defaults are the goal, and the one
    /// impl whose self type and arguments fit it fixes them, then each of
    /// its where-clause bounds the same way. Nothing else is selected:
    /// method lookup and other inference are as before.
    ///
    /// An `impl Trait` inside the output, as `Option<impl Widget<Message>>`,
    /// is fixed the same way at its place in the body's type, which
    /// `opaque_expected` built with the output's shape.
    #[cfg(feature = "rust-import")]
    pub(super) fn fix_opaque(
        &mut self,
        written: &type_ref::Type,
        hidden: &Ty,
    ) -> Result<(), Error> {
        if written.opaque_trait().is_some() {
            return self.fix_opaque_in(written, hidden, &BTreeMap::new());
        }
        if !written.has_opaque() {
            return Ok(());
        }
        let hidden = self.inference.resolve(hidden);
        match (written, &hidden) {
            (type_ref::Type::Tuple { members, .. }, Ty::Tuple(hidden)) => {
                for (member, hidden) in members.iter().zip(hidden.iter()) {
                    self.fix_opaque(member, hidden)?;
                }
            }
            (type_ref::Type::Reference { target, .. }, Ty::Reference { target: hidden, .. }) => {
                self.fix_opaque(target, hidden)?;
            }
            (type_ref::Type::Path(path), _) => {
                let inner = match &hidden {
                    Ty::Option(element)
                    | Ty::Array { element, .. }
                    | Ty::Boxed(element)
                    | Ty::Counted(_, element)
                    | Ty::Weak(_, element) => {
                        vec![&**element]
                    }
                    Ty::Result(ok, err) => vec![&**ok, &**err],
                    Ty::Host { arguments, .. } | Ty::Script(_, arguments) => {
                        arguments.iter().collect()
                    }
                    _ => Vec::new(),
                };
                let written = path.arguments.iter().filter_map(|argument| match argument {
                    type_ref::Argument::Type(ty) => Some(ty),
                    type_ref::Argument::Lifetime(_) => None,
                });
                for (written, hidden) in written.zip(inner) {
                    self.fix_opaque(written, hidden)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// While an opaque output is revealed, the written `impl Trait`'s
    /// arguments and defaults fix what the tail left open before the body's
    /// end falls back (`finish_structural_instances`), as rustc proves an
    /// opaque type's bounds before its fallback. The renderer of iced's
    /// gallery's viewer, `Some(opaque(mouse_area(container(image))))` in an
    /// `Option<impl Widget<Message>>`, is then the default one, which fixes
    /// the image's `Handle` (#1213) before `impl Into<Handle>` would take
    /// the argument's type, so the revealed output names iced's handle. What
    /// stays open is `reveal`'s `fix_opaque` to report.
    #[cfg(feature = "rust-import")]
    pub(super) fn settle_opaque(&mut self, written: &type_ref::Type) {
        if let Some(hidden) = self.revealing.clone() {
            let _ = self.fix_opaque(written, &hidden);
        }
    }

    /// While a body's calls are collected, before its tail meets the output
    /// the body is read with: a tail still to be inferred that a call of a
    /// function written `-> impl Trait` gave is known by that function's
    /// bound, and the body's written output fixes its arguments, as rustc
    /// proves the opaque output's bound: `glyph(name)` in `fn new_icon<M>()
    /// -> impl Widget<M>` is a `glyph::<M>` (#1633).
    #[cfg(feature = "rust-import")]
    pub(super) fn settle_collected_tail(&mut self, written: Option<&type_ref::Type>) {
        let Some(written) = written else {
            return;
        };
        let tail = self.last_type.clone();
        if self.collecting_instances
            && self.revealing.is_none()
            && self.last_expression.is_some()
            && matches!(self.inference.resolve(&tail), Ty::Infer(_))
        {
            let _ = self.fix_opaque(written, &tail);
        }
    }

    /// `fix_opaque` with the generic parameters `written` names at the
    /// types `substitution` gives them, as a generic function's bound
    /// `impl Widget<Message>` is at a call (D629).
    #[cfg(feature = "rust-import")]
    pub(super) fn fix_opaque_in(
        &mut self,
        written: &type_ref::Type,
        hidden: &Ty,
        substitution: &BTreeMap<ImmutableString, Ty>,
    ) -> Result<(), Error> {
        let Some(trait_name) = written.opaque_trait() else {
            return Ok(());
        };
        let resolved = self.inference.resolve(hidden);
        // A value still to be inferred is known by its own bound, when a
        // call of a function written `-> impl Trait` gave it.
        let pending = matches!(resolved, Ty::Infer(_));
        if pending
            && !self
                .opaque_values
                .iter()
                .any(|value| self.inference.resolve(&value.value) == resolved)
        {
            return Ok(());
        }
        let type_ref::Type::Path(opaque) = written else {
            return Ok(());
        };
        let written_arguments = opaque
            .arguments
            .iter()
            .filter_map(|argument| match argument {
                type_ref::Argument::Type(ty) => {
                    Some(instantiate(&Ty::from_resolved(ty), substitution))
                }
                type_ref::Argument::Lifetime(_) => None,
            })
            .collect::<Vec<_>>();
        // A known type fixes the bound's arguments a call leaves open, as
        // `Message` of `fn zoomed<Message>(content: impl Widget<Message>)`
        // given a `Rule<Message>` is `impl<M> Widget<M> for Rule<M>`'s.
        let open = written_arguments
            .iter()
            .any(|argument| contains_inference(&self.inference.resolve(argument)));
        if !contains_inference(&resolved) && !open {
            return Ok(());
        }
        let Ok(path) = crate::host::path::Path::parse(trait_name) else {
            return Ok(());
        };
        let Some(header) = self.models.host.trait_header(&path).cloned() else {
            return Ok(());
        };
        // The facade that declares the trait, whose types its headers name.
        let owner = self.models.host.type_owner(trait_name).cloned();
        let mut parameters = BTreeMap::new();
        let mut arguments = Vec::with_capacity(header.parameters.len());
        for (index, name) in header.parameters.iter().enumerate() {
            let ty = match written_arguments.get(index) {
                Some(ty) => ty.clone(),
                None => match header.defaults.get(index).and_then(Option::as_ref) {
                    Some(default) => self.instantiate_host_type(
                        default,
                        &mut parameters,
                        trait_name,
                        crate::resolve::type_span(written),
                        owner.as_ref(),
                        None,
                    )?,
                    None => self.inference.fresh(),
                },
            };
            parameters.insert(name.clone(), ty.clone());
            arguments.push(ty);
        }
        // `action(new_icon(), ..)`: the bound `impl Widget<Message>` of
        // `action` and the one `new_icon` is written with name the same
        // value, so their arguments are one (#1633).
        if pending {
            self.take_opaque_arguments(&resolved, header.identity, &arguments, &[]);
            return Ok(());
        }
        let span = match written {
            type_ref::Type::Unit(span)
            | type_ref::Type::Tuple { span, .. }
            | type_ref::Type::Reference { span, .. } => *span,
            type_ref::Type::Path(path) => path.span,
        };
        let unfixed = |this: &Self, reason: &str| Error::UnsupportedNominalSyntax {
            construct: format!(
                "`impl {trait_name}` whose body returns `{}`: {reason}",
                this.inference.resolve(hidden).name(&this.models)
            )
            .into(),
            source: span,
        };
        match self.select_impl(&resolved, header.identity, &arguments, owner.as_ref(), 0) {
            Ok(()) => {}
            Err(Selection::None(ty)) => {
                return Err(unfixed(
                    self,
                    &format!(
                        "no impl of the crate's fits `{ty}` to fix the type parameter left \
                         open in it"
                    ),
                ));
            }
            Err(Selection::Many(ty)) => {
                return Err(unfixed(
                    self,
                    &format!(
                        "more than one impl of the crate's fits `{ty}`, so the type \
                         parameter left open in it isn't fixed"
                    ),
                ));
            }
        }
        if contains_inference(&self.inference.resolve(hidden)) {
            return Err(unfixed(
                self,
                "a type parameter in it is fixed by neither the trait's defaults nor the \
                 crate's impls",
            ));
        }
        Ok(())
    }

    /// The body's output, when `output` is opaque, as the type its body
    /// gives, which `reveal` names from then on: an erased reading of the
    /// body goes on to its end.
    #[cfg(feature = "rust-import")]
    pub(super) fn expect_output(&mut self, output: Option<&type_ref::Type>) -> Result<(), Error> {
        if let Some(output) = output.filter(|output| output.has_opaque()) {
            let expected = self.opaque_expected(output)?;
            self.return_ty = Some(expected.clone());
            self.revealing = Some(expected);
        }
        Ok(())
    }

    /// `output` with a fresh inference variable for each opaque `impl
    /// Trait` in it, which the body's tail then binds to its hidden type.
    #[cfg(feature = "rust-import")]
    pub(super) fn opaque_expected(&mut self, output: &type_ref::Type) -> Result<Ty, Error> {
        if output.opaque_trait().is_some() {
            return Ok(self.inference.fresh());
        }
        if !output.has_opaque() {
            return Ok(Ty::from_resolved(output));
        }
        let unsupported = || Error::UnsupportedNominalSyntax {
            construct: "`impl Trait` of an imported trait inside this return type".into(),
            source: match output {
                type_ref::Type::Unit(span)
                | type_ref::Type::Tuple { span, .. }
                | type_ref::Type::Reference { span, .. } => *span,
                type_ref::Type::Path(path) => path.span,
            },
        };
        let arguments = |this: &mut Self, path: &type_ref::Path| {
            path.arguments
                .iter()
                .filter_map(|argument| match argument {
                    type_ref::Argument::Type(ty) => Some(this.opaque_expected(ty)),
                    type_ref::Argument::Lifetime(_) => None,
                })
                .collect::<Result<Vec<_>, _>>()
        };
        Ok(match output {
            type_ref::Type::Unit(_) => Ty::Unit,
            type_ref::Type::Tuple { members, .. } => Ty::Tuple(
                members
                    .iter()
                    .map(|member| self.opaque_expected(member))
                    .collect::<Result<_, _>>()?,
            ),
            type_ref::Type::Reference {
                mutable, target, ..
            } => Ty::Reference {
                mutable: *mutable,
                target: Box::new(self.opaque_expected(target)?),
            },
            type_ref::Type::Path(path) => {
                let mut inner = arguments(self, path)?.into_iter();
                let mut next = || inner.next().map(Box::new).ok_or_else(unsupported);
                if output.option_element().is_some() {
                    Ty::Option(next()?)
                } else if let Some((_, kind)) = sequence::Kind::of(output) {
                    Ty::Array {
                        element: next()?,
                        kind,
                    }
                } else if let Some((pointer, _)) = output.pointer_element() {
                    let target = next()?;
                    Pointer::named(pointer).ty(*target)
                } else if output.result_elements().is_some() {
                    Ty::Result(next()?, next()?)
                } else {
                    // A crate's or the program's own generic type, as
                    // loupe's `Loupe<impl Widget<Message> + 'a>`: the type
                    // at the arguments built here, each opaque one the
                    // body's tail binds.
                    match Ty::from_resolved(output) {
                        Ty::Host { proof, .. } => Ty::Host {
                            proof,
                            arguments: inner.collect(),
                        },
                        Ty::Script(definition, _) => Ty::Script(definition, inner.collect()),
                        _ => return Err(unsupported()),
                    }
                }
            }
        })
    }

    pub(super) fn parse_function_body(
        &mut self,
        definition: &item::Function,
    ) -> Result<Lowered, Error> {
        self.function_body(definition)
    }

    pub(super) fn function_body(&mut self, definition: &item::Function) -> Result<Lowered, Error> {
        self.check_function_signature(definition)?;
        if definition.body.constant {
            // The initializer of `const NAME: T = value;` is a tail, whose
            // temporaries are promoted to `'static`.
            self.flow().constant();
            self.parse_statement()?;
        } else {
            self.parse_block_function_body(definition)?;
        }
        if !matches!(self.peek(), Token::EOF) {
            return Err(self.unsupported("tokens after a function body"));
        }

        #[cfg(feature = "rust-import")]
        self.settle_collected_tail(definition.output.as_ref());
        let expected = self.tail_expected(definition.output.as_ref());
        self.coerce_tail(&expected, definition.body.span)?;
        #[cfg(feature = "rust-import")]
        if let Some(output) = &definition.output {
            self.settle_opaque(output);
        }
        if !definition.body.constant && !definition.body.asynchronous {
            self.close_body_drops()?;
            self.wrap_formatter(definition.body.span);
        }

        self.finish_uninitialized()?;

        self.check_borrows()?;
        self.finish_conversions()?;
        self.finish_bounds()?;
        self.finish_witnesses()?;
        let ops = self.finish_ops()?;
        let (source, locations) = std::mem::take(&mut self.output).finish();

        Ok(Lowered {
            source,
            locations,
            ops,
            patterns: std::mem::take(&mut self.patterns),
            closures: self.take_lifted(),
        })
    }

    /// Each parameter of a script struct type, or a reference to one, is
    /// checked against this program's schema where the call enters, as a
    /// method's receiver is: a value of an older schema is refused there,
    /// since its fields are then read where they are. An enum is checked
    /// where a `match` reads it, which lets another type fall through to
    /// `_`.
    pub(super) fn validate_parameters(&mut self, definition: &item::Function) {
        let receiver = this_parameter(definition);
        for (index, parameter) in definition.parameters.iter().enumerate() {
            let ty = match &parameter.ty {
                type_ref::Type::Reference { target, .. } => &**target,
                ty => ty,
            };
            let type_ref::Type::Path(path) = ty else {
                continue;
            };
            let type_ref::Target::Script { definition: id, .. } = &path.target else {
                continue;
            };
            let Nominal::Struct {
                definition: model, ..
            } = self.models.definition(*id)
            else {
                continue;
            };
            let helper = self.push_op(Op::Validate {
                definition: model.clone(),
            });
            let name = if Some(index) == receiver {
                "self"
            } else {
                parameter.name.text.as_str()
            };
            self.write_output(format!("{helper}({name});"), definition.body.span);
        }
    }

    pub(super) fn parse_block_function_body(
        &mut self,
        definition: &item::Function,
    ) -> Result<(), Error> {
        if definition.body.asynchronous {
            let output = definition
                .output
                .as_ref()
                .map_or(Ty::Unit, Ty::from_resolved);
            self.refuse_async_drops(
                definition
                    .parameters
                    .iter()
                    .map(|parameter| (Ty::from_resolved(&parameter.ty), parameter.name.span)),
            )?;
            self.destructure_parameters(&definition.parameters, definition.body.span)?;
            // `#[tokio::main] async fn main`: the attribute is the runtime's
            // entry, which the shadow crate carries as written (D513.4).
            // `main` registers it, as the macro's expansion would build the
            // runtime, and gives its body's future for the entry to await.
            if definition.name.text == "main"
                && let Some(attribute) = definition
                    .header
                    .attributes
                    .iter()
                    .find(|attribute| runtime_entry(&attribute.path))
                && let Some(app) = self.models.entry_crate.clone()
            {
                // At the attribute, which names the entry this call starts.
                let at = attribute.span.start();
                let ident = |text: &str| (Token::Identifier(Box::new(text.into())), at);
                self.splice_tokens(vec![
                    ident(&app),
                    (Token::DoubleColon, at),
                    ident(crate::package::ENTRY_MODULE),
                    (Token::DoubleColon, at),
                    ident("start"),
                    (Token::Unit, at),
                    (Token::SemiColon, at),
                ]);
                self.parse_statement()?;
            }
            return self.parse_async_body(&output, definition.body.span);
        }
        self.expect(Token::LeftBrace, "'{' to start a function body")?;
        self.validate_parameters(definition);
        self.open_body_drops(
            definition
                .parameters
                .iter()
                .map(|parameter| (parameter.name.text.clone(), parameter.name.span)),
        )?;
        self.destructure_parameters(&definition.parameters, definition.body.span)?;

        while !matches!(self.peek(), Token::RightBrace | Token::EOF) {
            match self.peek() {
                Token::Let => self.parse_binding(true)?,
                // `const fn f()` is a nested `fn` item.
                Token::Const if matches!(self.peek_second(), Token::Fn) => {
                    self.next();
                    self.skip_nested_fn()?;
                }
                Token::Const => self.parse_binding(false)?,
                _ if self.static_ahead() => self.parse_block_statics()?,
                Token::Fn => self.skip_nested_fn()?,
                Token::Use => self.skip_nested_use()?,
                _ if self.macro_definition_ahead() => self.parse_local_macro()?,
                _ if self.macro_statement_ahead() => self.parse_macro_statement()?,
                _ if self.nested_item_ahead() => self.skip_nested_item()?,
                _ => {
                    // A tail literal is expected to be the return type:
                    // `vec![Box::new(A), Box::new(B)]` as `Vec<Box<dyn
                    // Named>>`.
                    let expected = self.tail_expected(definition.output.as_ref());
                    if let Some(element) = self.literal_element(&expected)
                        && self.literal_ends_block()
                    {
                        self.array_hint = Some(element);
                    }
                    let statement = self.parse_tail_expecting(Some(&expected));
                    self.array_hint = None;
                    statement?;
                }
            }
        }

        self.expect(Token::RightBrace, "'}' to close a function body")?;
        Ok(())
    }

    /// Each local whose value never comes and whose uses gave it no type
    /// is `!`, as rustc's 2024 fallback makes it: `format!("{v:?}")` of
    /// `let v = loop {}` checks, and never runs.
    pub(super) fn fall_back_diverging(&mut self) {
        for open in std::mem::take(&mut self.diverging) {
            if let Ty::Infer(id) = self.inference.resolve(&open) {
                self.inference.bind(id, Ty::Never);
            }
        }
    }

    /// rustc's error for the inference variable `unknown`, which the use at
    /// `needed` needs known, at the first `let` whose type holds it: E0282
    /// "type annotations needed for `Vec<_>`". A conversion nothing
    /// settles, whose trait bound is `ambiguous` (`From<u8>`), is E0283 at
    /// the `let` of its value, and E0282 at a `let` whose own value made
    /// the variable (`Vec::new()`), which rustc marks.
    pub(super) fn unsettled(
        &self,
        unknown: &Ty,
        needed: Span,
        ambiguous: Option<String>,
    ) -> Option<Error> {
        let unknown @ Ty::Infer(_) = self.inference.resolve(unknown) else {
            return None;
        };
        let (local, ty) = self.local_types.iter().find_map(|local| {
            let ty = self.inference.resolve(&local.ty);
            contains(&ty, &|inner| *inner == unknown).then_some((local, ty))
        })?;
        let made = local
            .value
            .filter(|value| !(value.start() <= needed.start() && needed.end() <= value.end()))
            .filter(|_| ty != unknown);
        Some(Error::LocalTypeNeeded {
            code: if ambiguous.is_some() && made.is_none() {
                "E0283"
            } else {
                "E0282"
            },
            name: local.name.clone(),
            ty: (ty != unknown).then(|| ty.name(&self.models)),
            declaration: self.name_span(local.declaration, &local.name),
            needed: Some(made.unwrap_or(needed)),
            note: ambiguous
                .filter(|_| made.is_none())
                .map(|bound| format!("the type must implement `{bound}`").into()),
        })
    }

    /// The first use of a `let x;` before it has a value, now the body's
    /// types are settled: rustc's E0381, or, for a local whose type is
    /// still unknown, the E0282 its type check reports first (E0283 or
    /// E0284 where the use is an operator's operand).
    pub(super) fn finish_uninitialized(&mut self) -> Result<(), Error> {
        let Some(first) = std::mem::take(&mut self.uninitialized).into_iter().next() else {
            return Ok(());
        };
        let declaration = self.name_span(first.declaration, &first.name);
        let unknown = matches!(self.inference.resolve(&first.ty), Ty::Infer(_));
        Err(match first.operator {
            Some((code, needed)) => Error::LocalTypeNeeded {
                code,
                name: first.name,
                ty: None,
                declaration,
                needed: Some(needed),
                note: None,
            },
            None if unknown => Error::LocalTypeNeeded {
                code: "E0282",
                name: first.name,
                ty: None,
                declaration,
                needed: None,
                note: None,
            },
            None => Error::Uninitialized {
                reference: self.name_span(first.reference, &first.name),
                name: first.name,
                possibly: first.possibly,
                declaration,
            },
        })
    }
}
