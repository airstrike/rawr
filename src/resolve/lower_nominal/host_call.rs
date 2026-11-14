//! Crate items used in a body: calls of crate functions, methods and
//! macros, crate values and members, and the fields of crate structs.

use super::*;

impl<'a> Parser<'a> {
    pub(super) fn push_intrinsic(
        &mut self,
        item: crate::host::item::Definition,
        owner: Option<crate::host::facade::Dependency>,
        receiver: Option<&ParsedExpr>,
        arguments: &mut Vec<ParsedExpr>,
        output: Ty,
        requirements: Box<[(ImmutableString, Ty, capability::Set)]>,
        call: Span,
    ) -> Result<String, Error> {
        let owner = owner.ok_or_else(|| Error::InvalidHostOperation {
            item: item.path().into(),
            reason: "calling it by this path is not supported in Rawr yet".into(),
            operation: call,
        })?;
        let facade = owner.fingerprint().clone();
        #[cfg(feature = "rust-import")]
        self.check_into_arguments(&item, &requirements, arguments)?;
        #[cfg(feature = "rust-import")]
        if self.imported.is_some() {
            let count = arguments
                .iter()
                .filter(|argument| argument.ty.is_imported_static_operand())
                .count();
            self.retain_imported(
                Some(count),
                count.checked_mul(std::mem::size_of::<(Ty, Span, bool)>()),
            )?;
        }
        let mut operands = Vec::with_capacity(arguments.len() + usize::from(receiver.is_some()));
        if let Some(receiver) = receiver {
            operands.push((receiver.ty.clone(), receiver.span, true));
        }
        // Imported static operands leave the argument list; there are none
        // without the importer.
        arguments.retain_mut(|argument| {
            if argument.ty.is_imported_static_operand() {
                #[cfg(feature = "rust-import")]
                if self.imported.is_some() {
                    // Move the existing static proof into the pending operation. Final
                    // inference must settle before native-use evidence is materialized.
                    let ty = std::mem::replace(&mut argument.ty, Ty::Unknown);
                    operands.push((ty, argument.span, false));
                }
                false
            } else {
                // A program's iterator is its step function at run time.
                #[cfg(feature = "rust-import")]
                let ty = self
                    .steps
                    .get(argument.source.as_str())
                    .unwrap_or(&argument.ty)
                    .clone();
                #[cfg(not(feature = "rust-import"))]
                let ty = argument.ty.clone();
                operands.push((ty, argument.span, false));
                true
            }
        });
        #[cfg(feature = "rust-import")]
        self.fix_bounds_by_impls(&item, &requirements, &operands, call)?;
        Ok(self.push_op(Op::Intrinsic {
            operation: Box::new(IntrinsicOperation {
                facade,
                owner,
                item,
                requirements,
                operands: operands.into_boxed_slice(),
                output,
                span: call,
            }),
        }))
    }

    pub(super) fn symbolic_expr(
        &mut self,
        value: crate::item::Value,
        inputs: Box<[Ty]>,
        output: Ty,
        expression: Span,
        captures: Vec<origin::Fragment>,
    ) -> ParsedExpr {
        let helper = self.push_op(Op::Symbolic {
            value: value.clone(),
            captures: captures.len(),
        });
        ParsedExpr {
            prelude: origin::Fragment::default(),
            source: origin::Fragment::call(&helper, captures),
            ty: Ty::Callable {
                inputs,
                output: Box::new(output),
                item: Some(Box::new(value)),
            },
            span: expression,
            lvalue: None,
            outcome: crate::host::usage::Outcome::Conservative,
            evaluation: crate::host::usage::Evaluation::Required,
        }
        .retain_origin()
    }

    /// A script function passed as a callable: `imported` when an imported
    /// item takes it, whose output may be a tuple (`owned_symbolic_type`).
    pub(super) fn symbolic_function(
        &mut self,
        function: &ScriptFunction,
        imported: bool,
        expression: Span,
    ) -> Result<ParsedExpr, Error> {
        let inputs = function
            .definition
            .parameters
            .iter()
            .map(|parameter| {
                symbolic_input_type(&parameter.ty).ok_or_else(|| Error::InvalidMethodCall {
                    owner: ImmutableString::new(),
                    method: function.definition.name.text.clone(),
                    reason: "symbolic callable inputs must be owned, runtime-supported types"
                        .into(),
                    call: expression,
                })
            })
            .collect::<Result<Box<[_]>, _>>()?;
        let output = function
            .definition
            .output
            .as_ref()
            .map_or(Some(Ty::Unit), |output| {
                if imported {
                    owned_symbolic_type(output)
                } else {
                    exact_symbolic_type(output)
                }
            })
            .ok_or_else(|| Error::InvalidMethodCall {
                owner: ImmutableString::new(),
                method: function.definition.name.text.clone(),
                reason: "symbolic callable output must be one owned, runtime-supported type".into(),
                call: expression,
            })?;
        let value = crate::item::Value::new(
            function.key.clone(),
            crate::item::Kind::Function,
            None,
            None,
            crate::signature::Fingerprint::function(
                false,
                None,
                &function.definition.parameters,
                function.definition.output.as_ref(),
            ),
            capability::Fingerprint::default(),
        );
        Ok(self.symbolic_expr(value, inputs, output, expression, Vec::new()))
    }

    /// An associated function named as a value (`Db::new`, or `Db::from`
    /// of an `impl From<T> for Db`): a function item that calls it, which
    /// coerces to a `fn` pointer as a named function does.
    pub(super) fn associated_value(
        &mut self,
        definition: &method::Definition,
        expression: Span,
    ) -> Result<ParsedExpr, Error> {
        let substitution = self.method_substitution(definition, None, expression)?;
        let inputs: Box<[Ty]> = definition
            .parameters()
            .iter()
            .map(|parameter| instantiate(&Ty::from_resolved(&parameter.ty), &substitution))
            .collect();
        let output = instantiate(&Ty::from_resolved(definition.output()), &substitution);
        let parameters = (0..inputs.len())
            .map(|index| format!("__rawr_argument_{index}"))
            .collect::<Vec<_>>()
            .join(", ");
        let source = origin::Fragment::plain(format!(
            "(|{parameters}| {}({parameters}))",
            method_name(definition)
        ));
        Ok(self.operator_expr(
            source,
            Ty::FnItem {
                key: Box::new(definition.key().clone()),
                inputs,
                output: Box::new(output),
            },
            expression,
        ))
    }

    pub(super) fn symbolic_associated(
        &mut self,
        definition: &method::Definition,
        imported: bool,
        expression: Span,
    ) -> Result<ParsedExpr, Error> {
        // Any number of inputs, as a hand-written `Default`'s `State::default`
        // that `application(State::default, ..)` takes has none.
        let inputs = definition
            .parameters()
            .iter()
            .map(|parameter| {
                symbolic_input_type(&parameter.ty).ok_or_else(|| Error::InvalidMethodCall {
                    owner: definition.owner_name().into(),
                    method: definition.name().into(),
                    reason: "symbolic callable inputs must be owned, runtime-supported types"
                        .into(),
                    call: expression,
                })
            })
            .collect::<Result<Box<[_]>, _>>()?;
        let output = if imported {
            owned_symbolic_type(definition.output())
        } else {
            exact_symbolic_type(definition.output())
        };
        let output = output.ok_or_else(|| Error::InvalidMethodCall {
            owner: definition.owner_name().into(),
            method: definition.name().into(),
            reason: "symbolic callable output must be one owned, runtime-supported type".into(),
            call: expression,
        })?;
        let owner = definition.owner_identity().clone();
        let value = crate::item::Value::new(
            definition.key().clone(),
            crate::item::Kind::AssociatedFunction,
            Some(owner),
            None,
            definition.fingerprint().clone(),
            self.models
                .definition(definition.owner())
                .capabilities()
                .fingerprint(),
        );
        Ok(self.symbolic_expr(value, inputs, output, expression, Vec::new()))
    }

    pub(super) fn parse_host_function(
        &mut self,
        items: Box<[crate::host::item::Definition]>,
        owner: Option<crate::host::facade::Dependency>,
        name_span: Span,
    ) -> Result<ParsedExpr, Error> {
        // A call names the function where a macro has the same path, as
        // iced's `text` and `text!` do; `text![..]` names the macro.
        let callable = |item: &crate::host::item::Definition| {
            matches!(
                item.kind(),
                crate::host::item::Kind::Function
                    | crate::host::item::Kind::IntrinsicFunction
                    | crate::host::item::Kind::AssociatedFunction
            )
        };
        let items = if items.iter().any(|item| item.kind().is_macro()) && items.iter().any(callable)
        {
            items
                .into_vec()
                .into_iter()
                .filter(|item| !item.kind().is_macro())
                .collect()
        } else {
            items
        };
        let item = items
            .first()
            .expect("a host function has at least one declaration");
        if !matches!(
            item.kind(),
            crate::host::item::Kind::Function
                | crate::host::item::Kind::IntrinsicFunction
                | crate::host::item::Kind::AssociatedFunction
        ) {
            return Err(Error::InvalidHostOperation {
                item: item.path().into(),
                reason: if item.kind().is_macro() {
                    format!(
                        "`{name}` is a macro here; write `{name}![..]`",
                        name = item.path_ref().name()
                    )
                } else {
                    format!(
                        "`{name}` is not a function; write it without `(..)`",
                        name = item.path_ref().name()
                    )
                }
                .into(),
                operation: name_span,
            });
        }
        self.hint_closures(&items, None);
        // An imported function that borrows a value mutably takes `&mut
        // place`, as pulldown-cmark's `push_html(&mut s, ..)` does.
        let borrowing = item.signature().parameters().iter().any(|parameter| {
            matches!(
                parameter.shape(),
                crate::host::type_ref::Shape::Reference { mutable: true, .. }
            )
        });
        let (mut arguments, end) = if borrowing {
            self.parse_borrowing_arguments("host function arguments")?
        } else {
            self.parse_call_arguments("host function arguments")?
        };
        // The written arguments move into the call, as Rust's do.
        for argument in &mut arguments {
            self.move_local(argument)?;
        }
        let call = span(name_span.start(), end);
        self.call_host_function(&items, owner, arguments, call)
    }

    /// A facade function's call with its parsed `arguments`.
    pub(super) fn call_host_function(
        &mut self,
        items: &[crate::host::item::Definition],
        owner: Option<crate::host::facade::Dependency>,
        mut arguments: Vec<ParsedExpr>,
        call: Span,
    ) -> Result<ParsedExpr, Error> {
        self.supply_display_labeller(items, &mut arguments, call)?;
        self.deref_facade_strings(items, &mut arguments)?;
        // `Element::from(element)`: core's `impl<T> From<T> for T`, which
        // no crate lists among its own `From` impls, gives the value back.
        if let [argument] = arguments.as_slice()
            && matches!(self.inference.resolve(&argument.ty), Ty::Host { .. })
            && items.iter().all(|item| {
                item.path_ref().name() == "from"
                    && matches!(item.kind(), crate::host::item::Kind::AssociatedFunction)
            })
            && let Some(item) = items.first()
        {
            let checkpoint = self.inference.checkpoint();
            let reflexive = self
                .instantiate_host_signature(item, None, None, call)
                .is_ok_and(|signature| self.unify_type(&signature.output, &argument.ty));
            if reflexive {
                self.inference.commit(checkpoint);
                return Ok(arguments.pop().expect("one argument"));
            }
            self.inference.rollback(checkpoint);
            // `PathBuf::from(handle)`: a crate's `impl From<FileHandle> for
            // PathBuf`, which is the argument's own `into` (core's blanket
            // `Into`), as `f32::from(degrees)` is. A std type Rawr's std
            // holds lists only std's `from`s.
            let target = self
                .instantiate_host_signature(item, None, None, call)
                .map(|signature| signature.output);
            let source = self.inference.resolve(&argument.ty);
            if let Ok(target) = target
                && conversion::held_std(&target)
                && !self.host_from(&target, &source)
                && self.source_into(&target, &source)
            {
                let argument = arguments
                    .pop()
                    .expect("the call's one argument was matched above");
                let (forms, owner) = self.conversion_forms(&source).unwrap_or_default();
                let value = self.parse_target_into(argument, &source, forms, owner, call.end());
                self.unify_type(&value.ty, &target);
                return Ok(value);
            }
        }
        self.host_turbofish = self.pending_turbofish.take();
        let selected = self.select_host_signature(items, None, &mut arguments, false, call);
        self.host_turbofish = None;
        let (item, signature) = selected?;
        if signature.inputs.len() != arguments.len() {
            return Err(Error::argument_count(
                Callee::Function,
                item.path(),
                signature.inputs.len(),
                arguments.len(),
                call,
            ));
        }
        for (expected, argument) in signature.inputs.iter().zip(arguments.iter_mut()) {
            #[cfg(feature = "rust-import")]
            let profile_mapper = self.coerce_profile_mapper(&item, expected, argument)?;
            #[cfg(not(feature = "rust-import"))]
            let profile_mapper = false;
            // An array where the host takes a slice, as iced's
            // `pick_list(.., options: impl Borrow<[T]>, ..)`: Rust's
            // unsized coercion, or the array's own `Borrow<[T]>`.
            if let (
                Ty::Array {
                    kind: sequence::Kind::Vec,
                    ..
                },
                Ty::Array {
                    element,
                    kind: sequence::Kind::Array(_),
                },
            ) = (
                self.inference.resolve(expected),
                self.inference.resolve(&argument.ty),
            ) {
                argument.ty = Ty::Array {
                    element,
                    kind: sequence::Kind::Vec,
                };
            }
            self.collect_iterable(expected, argument);
            if !profile_mapper {
                self.fill_fn_bound(expected, argument);
                self.coerce_expr(expected, argument)?;
            }
            self.lend_guarded_place(expected, argument);
            if item.imported_identity().is_some() && !argument.ty.is_imported_static_operand() {
                self.consume_imported_value(argument, &item.path(), call)?;
            }
        }
        let output = self.inference.resolve(&signature.output);
        self.reject_invocation(&output, call, "returned by a function")?;
        let outcome = item.outcome();
        let evaluation = item.evaluation();
        let ty = output.clone();
        let types = arguments
            .iter()
            .map(|argument| argument.ty.clone())
            .collect::<Vec<_>>();
        let summary = self.host_summary(&item, None, &self.inference.resolve(&ty), &types);
        let opens = item.opens();
        let frame = item.frame();
        let helper = self.push_intrinsic(
            item,
            owner,
            None,
            &mut arguments,
            output,
            signature.requirements,
            call,
        )?;
        let places = copied_places(&self.inference, &signature.inputs, &arguments);
        move_into_call(&self.inference, &signature.inputs, &mut arguments);
        let opens = opens && !self.lend_static_texts(frame, &mut arguments);
        let runtime_arguments = arguments
            .into_iter()
            .filter(|argument| !argument.ty.is_imported_static_operand());
        let (prelude, source) = self.private_call(&helper, runtime_arguments);
        let value = ParsedExpr {
            prelude,
            source,
            ty,
            span: call,
            lvalue: None,
            outcome,
            evaluation,
        }
        .retain_origin();
        let kept = self.inference.resolve(&value.ty);
        self.kept_by_call(call, &kept, None, &summary);
        self.scoped_call(value, opens, &places)
    }

    /// An iterator where the host takes an iterable, as iced's
    /// `column(sizes.map(|size| ..))` and iced_test's `simulate(click())`
    /// take `impl IntoIterator<Item = X>`: its items, collected where the
    /// call takes them. A lazy one's pipeline is collected, and a crate's,
    /// as the opaque `impl Iterator<Item = Event>` `click()` gives, is
    /// walked by its own `next` (#1679). rustc's check of the crate holds
    /// the parameter to one that takes an iterator.
    fn collect_iterable(&mut self, expected: &Ty, argument: &mut ParsedExpr) {
        if !matches!(
            self.inference.resolve(expected),
            Ty::Array {
                kind: sequence::Kind::Vec,
                ..
            }
        ) {
            return;
        }
        let (source, item) = match self.inference.resolve(&argument.ty) {
            Ty::Iter(item, _) => (
                self.prelude_op(PreludeOp::CollectVec, [argument.rendered_source()]),
                item,
            ),
            ty @ Ty::Host { .. } if self.type_implements(&ty, "Iterator") => {
                let Some((source, item)) = self.collected_items(argument) else {
                    return;
                };
                (source, Box::new(item))
            }
            _ => return,
        };
        *argument = self.operator_expr(
            source,
            Ty::Array {
                element: item,
                kind: sequence::Kind::Vec,
            },
            argument.span,
        );
    }

    #[cfg(feature = "rust-import")]
    pub(super) fn coerce_profile_mapper(
        &mut self,
        invocation: &crate::host::item::Definition,
        expected: &Ty,
        argument: &mut ParsedExpr,
    ) -> Result<bool, Error> {
        if invocation.imported_identity().is_none()
            || invocation.kind() != crate::host::item::Kind::AssociatedFunction
        {
            return Ok(false);
        }
        let invocation_path = invocation.path();
        let Some((owner_path, _)) = invocation_path.rsplit_once("::") else {
            return Ok(false);
        };
        if self
            .models
            .host
            .type_(owner_path)
            .is_none_or(|owner| owner.identity().imported().is_none())
        {
            return Ok(false);
        }
        let Ty::ImportedFunctionItem {
            input: expected_input,
            output: expected_output,
            item: None,
        } = self.inference.resolve(expected)
        else {
            return Ok(false);
        };
        let Ty::Callable {
            inputs,
            output,
            item: Some(mapper),
        } = self.inference.resolve(&argument.ty)
        else {
            return Ok(false);
        };
        let [input] = &*inputs else {
            return Ok(false);
        };
        let input = Box::new(input.clone());
        let checkpoint = self.inference.checkpoint();
        if !self.unify_type(&expected_input, &input) {
            self.inference.rollback(checkpoint);
            return Ok(false);
        }
        let seal =
            crate::digest::event_mapper(&mapper).map_err(|_| Error::InvalidHostOperation {
                item: invocation.path().into(),
                reason: UNSUPPORTED_MESSAGE_FUNCTION.into(),
                operation: argument.span,
            })?;
        let message = Ty::ProfileMessage {
            payload: input.clone(),
            script: output.clone(),
            seal,
        };
        if !self.unify_type(&expected_output, &message) {
            self.inference.rollback(checkpoint);
            return Ok(false);
        }
        self.inference.commit(checkpoint);
        argument.ty = Ty::ProfileMapper {
            input,
            output,
            item: mapper,
            seal,
        };
        argument.lvalue = None;
        Ok(true)
    }

    #[cfg(feature = "rust-import")]
    pub(super) fn parse_imported_function_item(
        &mut self,
        items: Box<[crate::host::item::Definition]>,
        owner: Option<crate::host::facade::Dependency>,
        expression: Span,
    ) -> Result<ParsedExpr, Error> {
        // Both callers name one crate function, as Rust has no overloads.
        let [item] = items.as_ref() else {
            unreachable!("a crate function named as a value has one declaration")
        };
        assert!(
            item.imported_identity().is_some()
                && matches!(
                    item.kind(),
                    crate::host::item::Kind::Function | crate::host::item::Kind::AssociatedFunction
                ),
            "only a crate function is named as a function item"
        );
        if owner.is_none() {
            return Err(Error::InvalidHostOperation {
                item: item.path().into(),
                reason: "naming it as a value by this path is not supported in Rawr yet".into(),
                operation: expression,
            });
        }
        let signature = self.instantiate_host_signature(item, None, None, expression)?;
        let owner = owner.expect("guard proves one imported function owner");
        // One input is itself; none is `()`, and several are their tuple,
        // as a host callable's are (`checkbox::primary(&Theme, Status)`).
        let input = match signature.inputs.as_slice() {
            [input] => input.clone(),
            [] => Ty::Unit,
            inputs => Ty::Tuple(inputs.into()),
        };
        // A generic function is reached at the types its use infers for its
        // parameters, which the runner writes as its turbofish.
        let declared = item
            .signature()
            .declared_type_parameters()
            .collect::<Vec<_>>();
        let mut parameters = declared
            .iter()
            .filter_map(|name| {
                signature
                    .requirements
                    .iter()
                    .find(|(candidate, _, _)| candidate == name)
                    .map(|(name, ty, _)| (name.clone(), ty.clone()))
            })
            .collect::<Vec<_>>();
        if parameters.len() != signature.requirements.len() {
            parameters = signature
                .requirements
                .iter()
                .map(|(name, ty, _)| (name.clone(), ty.clone()))
                .collect();
        }
        // A function item takes no space, as in Rust: the proof is its
        // type's, and its value is `()`, which a tuple or array holding it
        // reads before a member coerces to a `fn` pointer.
        Ok(ParsedExpr {
            prelude: origin::Fragment::default(),
            source: origin::Fragment::plain("()"),
            ty: Ty::ImportedFunctionItem {
                input: Box::new(input),
                output: Box::new(signature.output),
                item: Some(Box::new(ImportedFunctionProof {
                    item: item.clone(),
                    owner,
                    span: expression,
                    parameters: parameters.into_boxed_slice(),
                })),
            },
            span: expression,
            lvalue: None,
            outcome: item.outcome(),
            evaluation: item.evaluation(),
        }
        .retain_origin())
    }

    pub(super) fn parse_host_function_or_item(
        &mut self,
        items: Box<[crate::host::item::Definition]>,
        owner: Option<crate::host::facade::Dependency>,
        expression: Span,
    ) -> Result<ParsedExpr, Error> {
        // A crate's function or method named as a value where a std method
        // Rawr runs takes a closure, as `message.map(canvas::Action::publish)`
        // or `.map(canvas::Action::and_capture)`: the closure that calls it,
        // which takes the types the method gives it (T327).
        #[cfg(feature = "rust-import")]
        if std::mem::take(&mut self.script_callable)
            && !matches!(self.peek(), Token::LeftParen | Token::Unit)
            && let [item] = items.as_ref()
            && item.imported_identity().is_some()
            && matches!(
                item.kind(),
                crate::host::item::Kind::Function
                    | crate::host::item::Kind::AssociatedFunction
                    | crate::host::item::Kind::Method
            )
        {
            return self.host_function_value(item.clone(), expression);
        }
        // `let name: fn(&Look) -> &str = Look::name;`: a crate type's
        // method named without a call anywhere else is the same closure.
        #[cfg(feature = "rust-import")]
        if !matches!(self.peek(), Token::LeftParen | Token::Unit)
            && let [item] = items.as_ref()
            && item.imported_identity().is_some()
            && item.kind() == crate::host::item::Kind::Method
        {
            return self.host_function_value(item.clone(), expression);
        }
        // `Look::mix(a, b)`: a crate type's method called by its path is
        // the method called on its first argument.
        if matches!(self.peek(), Token::LeftParen)
            && let [item] = items.as_ref()
            && item.kind() == crate::host::item::Kind::Method
        {
            let member = item.path_ref().name().to_owned();
            if let Some(tokens) = self.path_method_call(&member, expression, None) {
                self.splice_tokens(tokens);
                return self.parse_primary();
            }
            return Err(Error::argument_count(
                Callee::AssociatedFunction,
                item.path(),
                item.signature().parameters().len() + 1,
                0,
                expression,
            ));
        }
        #[cfg(feature = "rust-import")]
        if !matches!(self.peek(), Token::LeftParen | Token::Unit)
            && items.len() == 1
            && items[0].imported_identity().is_some()
            && matches!(
                items[0].kind(),
                crate::host::item::Kind::Function | crate::host::item::Kind::AssociatedFunction
            )
        {
            return self.parse_imported_function_item(items, owner, expression);
        }
        // `window::close`, `Theme::to_string`: a library function or method
        // named without a call is a function value, as in Rust.
        if !matches!(self.peek(), Token::LeftParen | Token::Unit)
            && let [item] = items.as_ref()
            && item.imported_identity().is_none()
            && matches!(
                item.kind(),
                crate::host::item::Kind::Function
                    | crate::host::item::Kind::AssociatedFunction
                    | crate::host::item::Kind::Method
            )
        {
            return self.host_function_value(item.clone(), expression);
        }
        let default = items.iter().all(|item| {
            item.kind() == crate::host::item::Kind::AssociatedFunction
                && item.path_ref().name() == "default"
        });
        let value = self.parse_host_function(items, owner, expression)?;
        // `Seed::default()` exists only where the facade's type is `Default`,
        // as in Rust (a struct literal still starts from it).
        if default
            && let Ty::Host { proof, .. } = &value.ty
            && !proof
                .definition
                .capabilities()
                .contains(capability::Kind::Default)
        {
            return Err(Error::MissingCapability {
                owner: value.ty.name(&self.models),
                capability: "Default".into(),
                operation: expression,
                derive: matches!(value.ty, Ty::Script(..)),
                required_by: None,
            });
        }
        Ok(value)
    }

    /// A library function or method as a value: the closure that calls it,
    /// `|a, b| path(a, b)` or `|a: &Owner, b| a.method(b)`, read in its place
    /// so it takes the parameter types its use expects.
    pub(super) fn host_function_value(
        &mut self,
        item: crate::host::item::Definition,
        expression: Span,
    ) -> Result<ParsedExpr, Error> {
        let path = item.path().to_string();
        let arity = item.signature().parameters().len();
        if item.kind() != crate::host::item::Kind::Method {
            return self.function_value_closure(&path, arity, None, expression);
        }
        let (owner, member) = path.rsplit_once("::").expect("a method has an owner");
        // A receiver of a type without parameters is annotated, so the
        // value has a type where nothing expects one.
        let annotation = self
            .models
            .host
            .type_(owner)
            .filter(|definition| definition.parameters().is_empty())
            .map(|_| {
                let reference = match item.receiver() {
                    Some(receiver::Mode::Owned) => "",
                    Some(receiver::Mode::Mutable) => "&mut ",
                    _ => "&",
                };
                format!("{reference}{owner}")
            });
        self.function_value_closure(member, arity + 1, Some(annotation), expression)
    }

    /// Read `|a0, ..| path(a0, ..)` (or, for a method, `|a0, ..| a0.path(..)`)
    /// in place of a function named as a value.
    pub(super) fn function_value_closure(
        &mut self,
        path: &str,
        arity: usize,
        method: Option<Option<String>>,
        at: Span,
    ) -> Result<ParsedExpr, Error> {
        let position = at.start();
        let mut tokens = Vec::<Token>::new();
        let path_tokens = |path: &str, tokens: &mut Vec<Token>| {
            for (index, segment) in path.split("::").enumerate() {
                if index > 0 {
                    tokens.push(Token::DoubleColon);
                }
                tokens.push(Token::Identifier(Box::new(segment.into())));
            }
        };
        let name =
            |index: usize| Token::Identifier(Box::new(format!("__rawr_argument_{index}").into()));
        if arity == 0 {
            tokens.push(Token::Or);
        } else {
            tokens.push(Token::Pipe);
            for index in 0..arity {
                if index > 0 {
                    tokens.push(Token::Comma);
                }
                tokens.push(name(index));
                if index == 0
                    && let Some(Some(annotation)) = &method
                {
                    tokens.push(Token::Colon);
                    let mut annotation = annotation.as_str();
                    if let Some(rest) = annotation.strip_prefix("&mut ") {
                        tokens.push(Token::Ampersand);
                        tokens.push(Token::Mut);
                        annotation = rest;
                    } else if let Some(rest) = annotation.strip_prefix('&') {
                        tokens.push(Token::Ampersand);
                        annotation = rest;
                    }
                    path_tokens(annotation, &mut tokens);
                }
            }
            tokens.push(Token::Pipe);
        }
        let first = if method.is_some() {
            tokens.push(name(0));
            tokens.push(Token::Period);
            tokens.push(Token::Identifier(Box::new(path.into())));
            1
        } else {
            path_tokens(path, &mut tokens);
            0
        };
        tokens.push(Token::LeftParen);
        for index in first..arity {
            if index > first {
                tokens.push(Token::Comma);
            }
            tokens.push(name(index));
        }
        tokens.push(Token::RightParen);
        let rest = self.input.by_ref().collect::<Vec<_>>();
        self.input = tokens
            .into_iter()
            .map(|token| (token, position))
            .chain(rest)
            .collect::<Vec<_>>()
            .into_iter()
            .peekable();
        let value = self.parse_primary()?;
        Ok(self.path_value(value))
    }

    /// `value`, a closure read in place of a path to a function, kept as one.
    pub(super) fn path_value(&mut self, value: ParsedExpr) -> ParsedExpr {
        if let Ty::FnItem { key, .. } = &value.ty {
            self.path_values.insert((**key).clone());
        }
        value
    }

    pub(super) fn parse_host_value(
        &mut self,
        item: crate::host::item::Definition,
        owner: Option<crate::host::facade::Dependency>,
        name_span: Span,
    ) -> Result<ParsedExpr, Error> {
        if !matches!(
            item.kind(),
            crate::host::item::Kind::Value | crate::host::item::Kind::AssociatedValue
        ) {
            return Err(Error::InvalidHostOperation {
                item: item.path().into(),
                reason: format!(
                    "`{name}` is not a value; call it with `{name}(..)`",
                    name = item.path_ref().name()
                )
                .into(),
                operation: name_span,
            });
        }
        let signature = self.instantiate_host_signature(&item, None, None, name_span)?;
        assert!(
            signature.inputs.is_empty(),
            "a host value or constant takes no inputs"
        );
        self.reject_invocation(&signature.output, name_span, "read as a constant")?;
        let outcome = item.outcome();
        let evaluation = item.evaluation();
        let helper = self.push_intrinsic(
            item,
            owner,
            None,
            &mut Vec::new(),
            signature.output.clone(),
            signature.requirements,
            name_span,
        )?;
        Ok(ParsedExpr {
            prelude: origin::Fragment::default(),
            source: self.op_call(&helper, []),
            ty: signature.output,
            span: name_span,
            lvalue: None,
            outcome,
            evaluation,
        }
        .retain_origin())
    }

    pub(super) fn parse_host_macro(
        &mut self,
        items: Box<[crate::host::item::Definition]>,
        owner: Option<crate::host::facade::Dependency>,
        name_span: Span,
    ) -> Result<ParsedExpr, Error> {
        let item = items
            .first()
            .expect("a host macro has at least one declaration");
        // A dependency's `macro_rules!` macro expands from its source. A
        // facade's own macro of the name wins on the facade path (D594.3).
        if !items
            .iter()
            .any(|item| item.kind() == crate::host::item::Kind::IntrinsicMacro)
            && let Some(rules) = items
                .iter()
                .find_map(crate::host::item::Definition::macro_rules_source)
        {
            return self.expand_macro_rules(rules, name_span);
        }
        if item.kind() != crate::host::item::Kind::IntrinsicMacro {
            return Err(Error::InvalidHostOperation {
                item: item.path().into(),
                reason: if matches!(
                    item.kind(),
                    crate::host::item::Kind::Function
                        | crate::host::item::Kind::IntrinsicFunction
                        | crate::host::item::Kind::AssociatedFunction
                ) {
                    format!(
                        "`{name}` is a function here; write `{name}(..)`",
                        name = item.path_ref().name()
                    )
                } else {
                    format!("`{}` is not a macro", item.path_ref().name())
                }
                .into(),
                operation: name_span,
            });
        }
        self.expect(Token::Bang, "'!' after an intrinsic macro name")?;
        // A macro taking one string reads `format!`'s arguments: iced's
        // `text!("{} items", n)`.
        let formats = items.iter().all(|item| {
            item.signature().parameters()
                == [crate::host::type_ref::Type::builtin(
                    crate::host::type_ref::Builtin::String,
                )]
        });
        if formats && matches!(self.peek(), Token::LeftParen) {
            let text = self.parse_format(name_span, false)?;
            let call = text.span;
            let mut arguments = vec![text];
            let (item, signature) =
                self.select_host_signature(&items, None, &mut arguments, true, call)?;
            let output = self.inference.resolve(&signature.output);
            let (outcome, evaluation) = (item.outcome(), item.evaluation());
            let helper = self.push_intrinsic(
                item,
                owner,
                None,
                &mut arguments,
                output.clone(),
                signature.requirements,
                call,
            )?;
            let (prelude, source) = self.private_call(&helper, arguments);
            return Ok(ParsedExpr {
                prelude,
                source,
                ty: output,
                span: call,
                lvalue: None,
                outcome,
                evaluation,
            }
            .retain_origin());
        }
        // As in Rust, a macro takes `[..]`, `(..)` or `{..}` alike.
        let close = match self.peek() {
            Token::LeftBracket => Token::RightBracket,
            Token::LeftParen => Token::RightParen,
            Token::LeftBrace => Token::RightBrace,
            _ => {
                return Err(Error::InvalidHostOperation {
                    item: item.path().into(),
                    reason: format!(
                        "write `{}![..]` with square brackets",
                        item.path_ref().name()
                    )
                    .into(),
                    operation: span(name_span.start(), self.position()),
                });
            }
        };
        self.next();
        let mut arguments = Vec::new();
        while !(self.peek() == close || matches!(self.peek(), Token::EOF)) {
            arguments.push(self.parse_expr()?);
            if matches!(self.peek(), Token::Comma) {
                self.next();
            } else if self.peek() != close {
                return Err(Error::InvalidHostOperation {
                    item: item.path().into(),
                    reason: "separate the elements with commas".into(),
                    operation: span(name_span.start(), self.position()),
                });
            }
        }
        let end = self.expect(
            close,
            "the closing delimiter after intrinsic macro elements",
        )?;
        let call = span(name_span.start(), end);
        self.lower_host_macro(&items, owner, arguments, call)
    }

    /// A list macro's call on `arguments`, each of its one repeated type.
    pub(super) fn lower_host_macro(
        &mut self,
        items: &[crate::host::item::Definition],
        owner: Option<crate::host::facade::Dependency>,
        mut arguments: Vec<ParsedExpr>,
        call: Span,
    ) -> Result<ParsedExpr, Error> {
        let (item, signature) =
            self.select_host_signature(items, None, &mut arguments, true, call)?;
        // Registration refuses any other (`InvalidIntrinsicMacroSignature`).
        let [expected] = signature.inputs.as_slice() else {
            unreachable!("an intrinsic list macro declares one repeated input")
        };
        for argument in &mut arguments {
            self.coerce_expr(expected, argument)?;
        }
        let output = self.inference.resolve(&signature.output);
        self.reject_invocation(&output, call, "returned by a macro")?;
        let outcome = item.outcome();
        let evaluation = item.evaluation();
        let helper = self.push_intrinsic(
            item,
            owner,
            None,
            &mut arguments,
            output.clone(),
            signature.requirements,
            call,
        )?;
        let (prelude, source) = self.private_call(&helper, arguments);
        Ok(ParsedExpr {
            prelude,
            source,
            ty: output,
            span: call,
            lvalue: None,
            outcome,
            evaluation,
        }
        .retain_origin())
    }

    pub(super) fn parse_host_method(
        &mut self,
        receiver: ParsedExpr,
        member: ImmutableString,
        member_span: Span,
    ) -> Result<ParsedExpr, Error> {
        // A facade member is named as in Rust: `r#type` is `type`.
        let member: ImmutableString = match crate::tokenizer::unraw(&member) {
            name if name.len() == member.len() => member,
            name => name.into(),
        };

        let turbofish = self.pending_turbofish.take();
        let resolved = self.inference.resolve(&receiver.ty);
        // `value.to_string()` of an imported type: Rust's blanket
        // `ToString` through its `Display`, which `cargo check` holds the
        // type to.
        #[cfg(feature = "rust-import")]
        if member.as_str() == "to_string"
            && self.pending_arguments.is_none()
            && matches!(self.peek(), Token::Unit)
            && let Ty::Host { proof, .. } = match &resolved {
                Ty::Reference { target, .. } => self.inference.resolve(target),
                ty => ty.clone(),
            }
            && proof.definition.identity().imported().is_some()
            && self.imported_member_owner(&resolved, &member).is_none()
        {
            let end = self.next().1;
            let source = self.prelude_op(PreludeOp::DisplayText, [receiver.rendered_source()]);
            return Ok(self.operator_expr(source, Ty::String, span(receiver.span.start(), end)));
        }
        #[cfg(feature = "rust-import")]
        let alias = resolved
            .host_alias()
            .cloned()
            .or_else(|| self.imported_member_owner(&resolved, &member));
        #[cfg(not(feature = "rust-import"))]
        let alias = resolved.host_alias().cloned();
        let Some(definition) = alias else {
            return Err(self.unsupported("host method receiver type"));
        };
        #[cfg(feature = "rust-import")]
        let definition = match self
            .models
            .host_items(&format!("{}::{member}", definition.path()))
        {
            Some(_) => definition,
            None => self
                .imported_member_elsewhere(&resolved, &member)
                .unwrap_or(definition),
        };
        let path = format!("{}::{member}", definition.path());
        let (member, path) = self.exported_member(&definition, member, path, member_span)?;
        // Distinct imported candidates at one path: which one Rust selects
        // depends on this call's receiver and lexical scope, which a
        // proof-backed application settles (D650).
        #[cfg(feature = "rust-import")]
        if self.models.host.pending_family(&path).is_some()
            && definition.identity().imported().is_some()
        {
            return self.parse_applied_method(
                receiver,
                &definition,
                member,
                &path,
                member_span,
                None,
            );
        }
        #[cfg(feature = "rust-import")]
        if let Some(error) = self.pending_family_call(&path, &member, member_span) {
            return Err(error);
        }
        // `*rotation.radians_mut() = angle`: a facade writes a `&mut`
        // accessor as the function that stores the value back.
        if let Some(base) = member.strip_suffix("_mut")
            && matches!(self.peek(), Token::Unit)
            && let Some(setters) = self.models.host_items(&path).filter(|items| {
                items.iter().all(|item| {
                    item.kind() == crate::host::item::Kind::AssociatedFunction
                        && item.signature().parameters().len() == 2
                })
            })
        {
            let setters = setters.to_vec();
            let base: ImmutableString = base.into();
            return self.parse_host_accessor(receiver, &path, base, &setters, member_span);
        }
        // `value.clone()` of a library type that is `Clone`: the value, as
        // Rawr's library values are copied where they are used.
        if member.as_str() == "clone"
            && self.models.host_items(&path).is_none()
            && definition.capabilities().contains(capability::Kind::Clone)
            && self.pending_arguments.is_none()
            && matches!(self.peek(), Token::Unit)
        {
            let end = self.next().1;
            let owned = match resolved {
                Ty::Reference { target, .. } => *target,
                ty => ty,
            };
            // A copy of the handle, not a move of the receiver; a native
            // element's place is cloned now, by the element's own `Clone`,
            // since the element may change before the copy is used (D651).
            let source = self.prelude_op(PreludeOp::ValueClone, [receiver.rendered_source()]);
            return Ok(self.operator_expr(source, owned, span(receiver.span.start(), end)));
        }
        if self.models.host_items(&path).is_none()
            && let Some(erased) = self.erasure_with(&resolved, &member)
        {
            let mut receiver = receiver;
            self.coerce_expr(&erased, &mut receiver)?;
            return self.parse_host_method(receiver, member, member_span);
        }
        // A tuple variant's constructor shares its path with the reader a
        // pattern uses: a method call names the reader.
        let items = self.models.host_items(&path).map(|items| {
            let methods = items
                .iter()
                .filter(|item| {
                    matches!(
                        item.kind(),
                        crate::host::item::Kind::Method | crate::host::item::Kind::IntrinsicMethod
                    )
                })
                .cloned()
                .collect::<Vec<_>>();
            if methods.is_empty() {
                items.to_vec()
            } else {
                methods
            }
        });
        let Some(items) = items else {
            let error = Error::UnknownMember {
                owner: definition.path().into(),
                member: member.clone(),
                reference: member_span,
            };
            #[cfg(feature = "rust-import")]
            let imported_miss = definition.identity().imported().is_some()
                && self.models.host.entry(&path).is_none();
            #[cfg(feature = "rust-import")]
            let error = self.native_miss(imported_miss, error);
            // A method of an imported type the facade does not hold: the
            // dependencies' intrinsic facts may prove which trait provides
            // it, with no new extraction (D609.6). Unproved, it is the
            // ordinary miss.
            #[cfg(feature = "rust-import")]
            if imported_miss
                && (self.pending_arguments.is_some()
                    || matches!(self.peek(), Token::LeftParen | Token::Unit))
            {
                let start = receiver.span.start();
                return self.recover_miss(start, |parser| {
                    parser.parse_applied_method(
                        receiver,
                        &definition,
                        member,
                        &path,
                        member_span,
                        Some(error),
                    )
                });
            }
            return Err(error);
        };
        let owner = self.models.item_owner(&path).cloned();
        let item = items
            .first()
            .expect("a host method has at least one declaration");
        if items.iter().any(|item| {
            item.kind() != crate::host::item::Kind::Method
                && item.kind() != crate::host::item::Kind::IntrinsicMethod
        }) {
            return Err(Error::InvalidHostOperation {
                item: item.path().into(),
                reason: format!("`{}` is not a method", item.path_ref().name()).into(),
                operation: member_span,
            });
        }
        let scoped = definition.storage() == crate::host::type_def::Storage::Invocation;
        if scoped
            && items
                .iter()
                .any(|item| item.receiver() != Some(receiver::Mode::Mutable))
        {
            return Err(Error::InvalidHostOperation {
                item: item.path().into(),
                reason: format!(
                    "calling `{member}` on the `{}` a callback receives is not supported in \
                     Rawr yet",
                    definition.path()
                )
                .into(),
                operation: member_span,
            });
        }
        if scoped {
            self.require_invocation_receiver(&receiver, member_span)?;
        }
        // `tree.size = size` and `self.shadow.offset.x = x`: a facade
        // field of a place is a place, set through the field's owned
        // builder, which returns the changed value.
        if self.pending_arguments.is_none()
            && !matches!(self.peek(), Token::LeftParen | Token::Unit)
            && receiver.lvalue.is_some()
            && items.iter().any(|item| {
                item.receiver() == Some(receiver::Mode::Owned)
                    && item.signature().parameters().len() == 1
            })
        {
            return self.parse_host_field_place(receiver, member, member_span);
        }
        // A facade type's field: a parameterless shared-receiver reader,
        // written `value.field` as in Rust.
        let written_field = self.pending_arguments.is_none()
            && !matches!(self.peek(), Token::LeftParen | Token::Unit);
        // A field read for a pattern (`let Radius { top_left, .. } = r`)
        // passes no arguments: the reader, when the field has one.
        let read = !written_field
            && self
                .pending_arguments
                .as_ref()
                .is_some_and(|(arguments, _)| arguments.is_empty())
            && items.iter().any(|item| {
                item.receiver() == Some(receiver::Mode::Shared)
                    && item.signature().parameters().is_empty()
            });
        if read {
            self.pending_arguments = None;
        }
        let field = written_field || read;
        let items = field_or_builder(items.clone(), field);
        if written_field
            && (definition.identity().imported().is_some()
                || items.iter().any(|item| {
                    item.receiver() != Some(receiver::Mode::Shared)
                        || !item.signature().parameters().is_empty()
                }))
        {
            return Err(Error::InvalidHostOperation {
                item: item.path().into(),
                reason: method_without_call(&member),
                operation: member_span,
            });
        }
        let (arguments, end) = if field {
            (Vec::new(), member_span.end())
        } else {
            self.hint_closures(&items, Some(&receiver.ty));
            // A method that borrows a value mutably, as std's
            // `read_line(&mut buf)` and `render_stateful_widget(&mut
            // state)` do (D558.5).
            if items.first().is_some_and(|item| {
                item.signature().parameters().iter().any(|parameter| {
                    matches!(
                        parameter.shape(),
                        crate::host::type_ref::Shape::Reference { mutable: true, .. }
                    )
                })
            }) {
                self.parse_borrowing_arguments("host method arguments")?
            } else {
                self.parse_call_arguments("host method arguments")?
            }
        };
        let call = span(receiver.span.start(), end);
        let choice = arguments.iter().position(|argument| {
            matches!(self.inference.resolve(&argument.ty), Ty::FnChoice { .. })
        });
        // `value.method::<T>(..)`: the method's own parameters take the
        // turbofish, as a host function's do.
        let outer = std::mem::replace(&mut self.host_turbofish, turbofish);
        let finished = match choice {
            Some(index) => self.dispatch_host_method(
                receiver,
                &definition,
                &items,
                owner,
                scoped,
                member_span,
                arguments,
                index,
                call,
            ),
            None => self.finish_host_method(
                receiver,
                &definition,
                &items,
                owner,
                scoped,
                member_span,
                arguments,
                call,
            ),
        };
        self.host_turbofish = outer;
        finished
    }

    /// An app's exported type keeps fields and methods apart, as Rust does:
    /// `order.items` reads the field and `order.items()` calls the method.
    /// Using one as the other is rustc's error. Returns the member and path
    /// to look up.
    pub(super) fn exported_member(
        &mut self,
        definition: &crate::host::type_def::Definition,
        member: ImmutableString,
        path: String,
        member_span: Span,
    ) -> Result<(ImmutableString, String), Error> {
        use crate::host::EXPORTED_FIELD;
        if !self.models.host.is_exported(&definition.path()) || member.starts_with(EXPORTED_FIELD) {
            return Ok((member, path));
        }
        let field = format!("{}::{EXPORTED_FIELD}{member}", definition.path());
        let has_field = self.models.host_items(&field).is_some();
        let has_method = self.models.host_items(&path).is_some();
        let called = self.pending_arguments.is_some()
            || matches!(self.peek(), Token::LeftParen | Token::Unit);
        let kind = |field| Error::MemberKind {
            owner: definition.path().into(),
            member: member.clone(),
            field,
            reference: member_span,
        };
        match (called, has_field, has_method) {
            (false, true, _) => Ok((format!("{EXPORTED_FIELD}{member}").into(), field)),
            (false, false, true) => Err(kind(false)),
            (true, true, false) => Err(kind(true)),
            _ => Ok((member, path)),
        }
    }

    /// `value.field` of a facade type as an assignment's target: a place
    /// whose reader is the field's reader and whose store is its owned
    /// builder, `value = value.field(new)`.
    pub(super) fn parse_host_field_place(
        &mut self,
        receiver: ParsedExpr,
        field: ImmutableString,
        member_span: Span,
    ) -> Result<ParsedExpr, Error> {
        let Some(lvalue) = receiver.lvalue.clone() else {
            return Err(self.unsupported("assignment target"));
        };
        let place = self.temporary("place");
        let stored = self.temporary("stored");
        let receiver_ty = receiver.ty.clone();
        let at = receiver.span;
        let local = self.operator_expr(
            origin::Fragment::plain(place.clone()),
            receiver_ty.clone(),
            at,
        );
        // The place's local has no lvalue, so this reads the field.
        let getter = self.parse_host_method(local.clone(), field.clone(), member_span)?;
        let ty = self.inference.resolve(&getter.ty);
        let operand = self.operator_expr(origin::Fragment::plain(stored.clone()), ty.clone(), at);
        self.pending_arguments = Some((vec![operand], member_span.end()));
        let setter = self.parse_host_method(local, field, member_span)?;
        let mut read = origin::Fragment::plain("{ ");
        read.push(getter.prelude.clone());
        read.push(getter.source.clone());
        read.push_text(" }");
        let mut store = origin::Fragment::plain("{ ");
        store.push(setter.prelude.clone());
        store.push(setter.source.clone());
        store.push_text(" }");
        let step = PlaceStep::Accessor {
            place: place.clone(),
            stored,
            read: read.clone(),
            store,
        };
        let lvalue = match &lvalue {
            Lvalue::Binding { binding, .. } => Lvalue::Nested(Box::new(lvalue::Nested {
                binding: binding.clone(),
                base: origin::Fragment::plain(binding.to_string()),
                steps: vec![step],
                ty: ty.clone(),
            })),
            Lvalue::Slice(_) => return Err(self.unsupported("assignment target")),
            lvalue => nested_step(lvalue, step, ty.clone()),
        };
        let mut source = origin::Fragment::plain(format!("{{ let {place} = "));
        source.push(receiver.rendered_source());
        source.push_text("; ");
        source.push(read);
        source.push_text(" }");
        let mut value = self.operator_expr(source, ty, span(at.start(), member_span.end()));
        value.lvalue = Some(lvalue);
        Ok(value)
    }

    /// `value.name_mut()` of a facade type: a `&mut` place whose reader is
    /// `value.name()` and whose store is `Type::name_mut(value, new)`.
    pub(super) fn parse_host_accessor(
        &mut self,
        receiver: ParsedExpr,
        path: &str,
        base: ImmutableString,
        setters: &[crate::host::item::Definition],
        member_span: Span,
    ) -> Result<ParsedExpr, Error> {
        let Some(lvalue) = receiver.lvalue.clone() else {
            return Err(Error::InvalidHostOperation {
                item: path.into(),
                reason: "lends a `&mut` only of a place, such as a field or a local".into(),
                operation: member_span,
            });
        };
        let place = self.temporary("place");
        let stored = self.temporary("stored");
        let receiver_ty = receiver.ty.clone();
        let at = receiver.span;
        let local = self.operator_expr(
            origin::Fragment::plain(place.clone()),
            receiver_ty.clone(),
            at,
        );
        let getter = self.parse_host_method(local, base, member_span)?;
        let ty = self.inference.resolve(&getter.ty);
        let owner = self.models.item_owner(path).cloned();
        let arguments = vec![
            self.operator_expr(origin::Fragment::plain(place.clone()), receiver_ty, at),
            self.operator_expr(origin::Fragment::plain(stored.clone()), ty.clone(), at),
        ];
        let setter = self.call_host_function(setters, owner, arguments, getter.span)?;
        let mut read = origin::Fragment::plain("{ ");
        read.push(getter.prelude.clone());
        read.push(getter.source.clone());
        read.push_text(" }");
        let mut store = origin::Fragment::plain("{ ");
        store.push(setter.prelude.clone());
        store.push(setter.source.clone());
        store.push_text(" }");
        let step = PlaceStep::Accessor {
            place: place.clone(),
            stored,
            read: read.clone(),
            store,
        };
        let lvalue = match &lvalue {
            Lvalue::Binding { binding, .. } => Lvalue::Nested(Box::new(lvalue::Nested {
                binding: binding.clone(),
                base: origin::Fragment::plain(binding.to_string()),
                steps: vec![step],
                ty: ty.clone(),
            })),
            Lvalue::Slice(_) => {
                return Err(Error::InvalidHostOperation {
                    item: path.into(),
                    reason: "lends a `&mut` only of a place, such as a field or a local".into(),
                    operation: member_span,
                })
            }
            lvalue => nested_step(lvalue, step, ty.clone()),
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
                target: Box::new(ty),
            },
            span(at.start(), getter.span.end()),
        );
        value.lvalue = Some(lvalue);
        Ok(value)
    }

    /// The call of `member` at `path` where several of the crate's impls
    /// give it the same inputs in Rawr's types, so the argument can't pick
    /// one as rustc does. Reported at the program's call, only when it is
    /// made.
    #[cfg(feature = "rust-import")]
    pub(super) fn pending_family_call(&self, path: &str, member: &str, at: Span) -> Option<Error> {
        let family = self.models.host.pending_family(path)?;
        Some(Error::InvalidHostOperation {
            item: path.into(),
            reason: format!(
                "calling `{member}` here, where {} impls of the crate provide it, is not \
                 supported in Rawr yet",
                family.len(),
            )
            .into(),
            operation: at,
        })
    }

    /// A call of something Rawr doesn't know, as a method an imported type
    /// has no member for, in a pass that asks the crates for every miss at
    /// once (#662): the miss is kept, the
    /// arguments the call did not read are passed over, and the call reads
    /// as `!`, which any place takes, so the body goes on to its next miss.
    /// The pass fails with the first miss it kept. Any other result is as
    /// it was. A miss `attempt` returns early from inside an inference
    /// checkpoint leaves that checkpoint open; the pass goes on, so the
    /// checkpoints the attempt opened are closed with what they bound.
    #[cfg(feature = "rust-import")]
    pub(super) fn recover_miss(
        &mut self,
        start: Position,
        attempt: impl FnOnce(&mut Self) -> Result<ParsedExpr, Error>,
    ) -> Result<ParsedExpr, Error> {
        let depth = self.inference.depth;
        let error = match attempt(self) {
            Err(error) => error,
            result => return result,
        };
        let missed = match &error {
            Error::UnknownMember { .. }
            | Error::NotInModule { .. }
            | Error::UnknownType { .. }
            | Error::UnknownImport { .. } => true,
            Error::ImportedApplicationRequired { miss, .. } => miss.is_some(),
            _ => false,
        };
        if !self.recover_misses || !missed || self.pending_arguments.is_some() {
            return Err(error);
        }
        let mut end = self.position();
        if matches!(self.peek(), Token::Unit) {
            end = self.next().1;
        } else if matches!(self.peek(), Token::LeftParen) {
            let mut depth = 0usize;
            loop {
                match self.next() {
                    (
                        Token::LeftBrace | Token::LeftParen | Token::LeftBracket | Token::MapStart,
                        _,
                    ) => {
                        depth += 1;
                    }
                    (Token::RightBrace | Token::RightParen | Token::RightBracket, position) => {
                        depth = depth.saturating_sub(1);
                        if depth == 0 {
                            end = position;
                            break;
                        }
                    }
                    (Token::EOF, _) => return Err(error),
                    _ => {}
                }
            }
        }
        self.inference.depth = depth;
        if let Error::UnknownType { name, .. } = &error {
            self.recovered_types.push((start, name.clone()));
        }
        self.recovered.push(error);
        Ok(self.operator_expr(origin::Fragment::from("()"), Ty::Never, span(start, end)))
    }

    /// `value.method(..)` of an imported type that only a proof-backed
    /// application settles: the call's receiver, arguments and lexical
    /// scope name the application, which the registry holds once it is
    /// proved, on the host and in a generated runner alike.
    #[cfg(feature = "rust-import")]
    pub(super) fn parse_applied_method(
        &mut self,
        receiver: ParsedExpr,
        definition: &crate::host::type_def::Definition,
        member: ImmutableString,
        path: &str,
        member_span: Span,
        miss: Option<Error>,
    ) -> Result<ParsedExpr, Error> {
        // A call outside the closed rules is the ordinary miss, when it is
        // one; otherwise the rule it is outside of.
        let unsupported = |reason: &str| {
            miss.clone().unwrap_or_else(|| Error::InvalidHostOperation {
                item: path.into(),
                reason: reason.into(),
                operation: member_span,
            })
        };
        let identity = definition.identity().imported().ok_or_else(|| {
            unsupported(&format!(
                "no method named `{member}` found for `{}` (E0599)",
                definition.path()
            ))
        })?;
        if !definition.parameters().is_empty() {
            return Err(unsupported(&format!(
                "calling `{member}`, which a trait impl provides, on a generic type is not \
                 supported in Rawr yet"
            )));
        }
        if self.pending_arguments.is_none()
            && !matches!(self.peek(), Token::LeftParen | Token::Unit)
        {
            return Err(unsupported(&method_without_call(&member)));
        }
        // Without the method's signature, a closure argument has no
        // parameter types, as `with_translation(v, |renderer| ..)` lends
        // the receiver as the trait's `&mut Self`. An argument that can't be
        // read so is the miss: demanded, the member's signature gives the
        // closure its types, as rustc's selection does before checking it.
        let (arguments, end) = match self.parse_call_arguments("host method arguments") {
            Ok(parsed) => parsed,
            Err(error) => return Err(miss.unwrap_or(error)),
        };
        let call_span = span(receiver.span.start(), end);
        let mut types = Vec::with_capacity(arguments.len());
        for argument in &arguments {
            types.push(
                instance_surface_type(&self.inference, &self.models, &argument.ty).ok_or_else(
                    || {
                        unsupported(&format!(
                            "a `{}` passed to `{member}`, which a trait impl provides, is not \
                             supported in Rawr yet",
                            argument.ty.name(&self.models)
                        ))
                    },
                )?,
            );
        }
        let scope = match self.lexical_scope(member_span) {
            Ok(scope) => scope,
            Err(error) => return Err(miss.unwrap_or(error)),
        };
        let call = rawr_import::surface::application::Call {
            receiver: rawr_import::surface::identity::Definition::authenticated(
                *identity.as_bytes(),
            ),
            receiver_arguments: Box::new([]),
            name: member.as_str().into(),
            // Rust selects a `&mut self` method of any receiver and checks
            // its mutability afterwards, as `finish_host_method` does.
            mutable: true,
            arguments: types.into_boxed_slice(),
            scope,
        }
        .canonical();
        let Some((item, owner)) = self.models.host.applied(&call.key()) else {
            // Only an imported compilation can have the call proved; any
            // other reports the miss as it is.
            if self.imported.is_none()
                && let Some(miss) = miss
            {
                return Err(miss);
            }
            return Err(Error::ImportedApplicationRequired {
                item: path.into(),
                call: serde_json::to_vec(&call)
                    .expect("an application call encodes as JSON")
                    .into_boxed_slice(),
                miss: miss.map(Box::new),
                operation: member_span,
            });
        };
        let (item, owner) = (item.clone(), owner.clone());
        self.finish_host_method(
            receiver,
            definition,
            &[item],
            Some(owner),
            false,
            member_span,
            arguments,
            call_span,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn finish_host_method(
        &mut self,
        receiver: ParsedExpr,
        definition: &crate::host::type_def::Definition,
        items: &[crate::host::item::Definition],
        owner: Option<crate::host::facade::Dependency>,
        scoped: bool,
        member_span: Span,
        mut arguments: Vec<ParsedExpr>,
        call: Span,
    ) -> Result<ParsedExpr, Error> {
        self.deref_facade_strings(items, &mut arguments)?;
        let (item, signature) =
            self.select_host_signature(items, Some(&receiver.ty), &mut arguments, false, call)?;
        // `reader.config_mut().trim_text(true)`: a `&mut self` method of an
        // imported value reached through a method's `&mut` output, which
        // is read and stored as a borrow guard's place (D554, D558 slice 2).
        let through_guard = matches!(
            &receiver.lvalue,
            Some(Lvalue::Nested(nested)) if nested
                .steps
                .iter()
                .any(|step| matches!(step, PlaceStep::Cell { native: true, .. }))
        );
        if definition.identity().imported().is_some() && !scoped {
            match item.receiver() {
                Some(receiver::Mode::Shared) => {}
                Some(receiver::Mode::Mutable) if through_guard => {}
                Some(receiver::Mode::Mutable) => {
                    self.require_mutable_host_receiver(&receiver, member_span, &item)?;
                }
                Some(receiver::Mode::Owned) => {
                    self.consume_host_receiver(&receiver, member_span, &item)?;
                }
                None => {
                    return Err(Error::InvalidHostOperation {
                        item: item.path().into(),
                        reason: format!(
                            "`{name}` is an associated function, not a method; call it as \
                             `{path}(..)` (E0599)",
                            name = item.path_ref().name(),
                            path = item.path()
                        )
                        .into(),
                        operation: member_span,
                    });
                }
            }
        }
        if signature.inputs.len() != arguments.len() {
            return Err(Error::argument_count(
                Callee::Method,
                item.path(),
                signature.inputs.len(),
                arguments.len(),
                span(member_span.start(), call.end()),
            ));
        }
        for (expected, argument) in signature.inputs.iter().zip(arguments.iter_mut()) {
            // An array where the method takes a slice or an iterable, as
            // iced's `fonts([..])` takes `impl IntoIterator`, as a
            // function's argument is (`call_host_function`).
            if let (
                Ty::Array {
                    kind: sequence::Kind::Vec,
                    ..
                },
                Ty::Array {
                    element,
                    kind: sequence::Kind::Array(_),
                },
            ) = (
                self.inference.resolve(expected),
                self.inference.resolve(&argument.ty),
            ) {
                argument.ty = Ty::Array {
                    element,
                    kind: sequence::Kind::Vec,
                };
            }
            self.collect_iterable(expected, argument);
            self.fill_fn_bound(expected, argument);
            self.coerce_expr(expected, argument)?;
            self.lend_guarded_place(expected, argument);
            if item.imported_identity().is_some() {
                self.consume_imported_value(argument, &item.path(), call)?;
            }
            self.move_local(argument)?;
        }
        let output = self.inference.resolve(&signature.output);
        self.reject_invocation(&output, call, "returned by a method")?;
        let outcome = item.outcome();
        let evaluation = item.evaluation();
        let ty = output.clone();
        let types = arguments
            .iter()
            .map(|argument| argument.ty.clone())
            .collect::<Vec<_>>();
        let summary = self.host_summary(
            &item,
            Some(&receiver.ty),
            &self.inference.resolve(&ty),
            &types,
        );
        self.note_call(call.end(), summary);
        let mutable = item.receiver() == Some(receiver::Mode::Mutable);
        let shared = item.receiver() == Some(receiver::Mode::Shared);
        let guard = item.guard();
        let opens = item.opens();
        let places = copied_places(&self.inference, &signature.inputs, &arguments);
        move_into_call(&self.inference, &signature.inputs, &mut arguments);
        let helper = self.push_intrinsic(
            item,
            owner,
            Some(&receiver),
            &mut arguments,
            output,
            signature.requirements,
            call,
        )?;
        // `animation.go_mut(..)` of a facade value in a place: a local is
        // changed where it is. Any other place is changed through a copy,
        // which is stored back, as a script `&mut self` method's receiver is.
        if mutable
            && (definition.identity().imported().is_none() || through_guard)
            && !scoped
            && self.host_witnesses.is_empty()
            && let Some(lvalue) = receiver.lvalue.clone()
        {
            self.require_mutable_binding(lvalue.binding(), member_span)?;
            if let Some(mut source) = in_place(&receiver) {
                let (mut prelude, operands) = self.private_operands(&helper, arguments);
                source.push_text(format!(".{helper}("));
                source.push(origin::Fragment::join(operands, ", "));
                source.push_text(")");
                prelude.push(source);
                let value = ParsedExpr {
                    prelude: origin::Fragment::default(),
                    source: prelude.surrounded("{ ", " }"),
                    ty,
                    span: call,
                    lvalue: None,
                    outcome,
                    evaluation,
                }
                .retain_origin();
                return self.copied_places_back(value, &places);
            }
            let place = self.temporary("receiver");
            let result = self.temporary("result");
            let mut prelude = origin::Fragment::default();
            let operands = arguments
                .into_iter()
                .enumerate()
                .map(|(index, argument)| {
                    self.materialize(argument, &format!("{helper}_arg_{index}"), &mut prelude)
                })
                .collect::<Vec<_>>();
            let mut source = origin::Fragment::plain("{ ");
            source.push(prelude);
            source.push_text(format!("let {place} = "));
            source.push(receiver.rendered_source());
            source.push_text(format!("; let {result} = {place}.{helper}("));
            source.push(origin::Fragment::join(operands, ", "));
            source.push_text("); ");
            let store = self.store_lvalue(&lvalue, origin::Fragment::plain(place))?;
            source.push(store);
            source.push_text(format!(" {result} }}"));
            let value = ParsedExpr {
                prelude: origin::Fragment::default(),
                source,
                ty,
                span: call,
                lvalue: None,
                outcome,
                evaluation,
            }
            .retain_origin();
            return self.copied_places_back(value, &places);
        }
        // A `&self` receiver that is a local is lent where it is. It is
        // read after the arguments, which Rust's borrow of it keeps from
        // changing it. A local's field or element is lent where it is too,
        // as the receiver of a method call on that place.
        let place = in_place(&receiver).filter(|_| self.host_witnesses.is_empty());
        let (prelude, source) = if shared && self.normalize_private_calls && receiver.is_local() {
            let (prelude, operands) = self.private_operands(&helper, arguments);
            let source = self.op_call(&helper, std::iter::once(receiver.source).chain(operands));
            (prelude, source)
        } else if shared
            && self.normalize_private_calls
            && let Some(mut source) = place
        {
            let (prelude, operands) = self.private_operands(&helper, arguments);
            source.push_text(format!(".{helper}("));
            source.push(origin::Fragment::join(operands, ", "));
            source.push_text(")");
            (prelude, source)
        } else {
            self.private_call(&helper, std::iter::once(receiver).chain(arguments))
        };
        let value = ParsedExpr {
            prelude,
            source,
            ty,
            span: call,
            lvalue: None,
            outcome,
            evaluation,
        }
        .retain_origin();
        let value = self.scoped_call(value, opens, &places)?;
        match guard {
            None => Ok(value),
            Some(guard) => Ok(self.guarded_output(value, guard)),
        }
    }

    /// An imported method's borrow guard output, as `state.read()` and
    /// `state.write()` give: `&T` of the copy a `Deref` guard's call gives,
    /// or `&mut T` of the place a `DerefMut` guard's call gives, read and
    /// stored as a `RefCell`'s `borrow_mut()` is. So `*state.write() += 1`
    /// reads the target and stores it back in one statement, and `let mut
    /// v = state.write();` keeps the place for `v`'s block.
    pub(super) fn guarded_output(
        &mut self,
        mut value: ParsedExpr,
        guard: crate::host::item::Guard,
    ) -> ParsedExpr {
        let inner = value.ty.clone();
        match guard {
            crate::host::item::Guard::OptionShared => {
                // `Some` of a copy of the target, typed as Rust's `Option<&T>`.
                if let Ty::Option(target) = self.inference.resolve(&inner) {
                    value.ty = Ty::Option(Box::new(Ty::Reference {
                        mutable: false,
                        target,
                    }));
                }
                return value;
            }
            crate::host::item::Guard::OptionMutable => return self.optional_guard(value),
            crate::host::item::Guard::Shared | crate::host::item::Guard::Mutable => {}
        }
        if guard == crate::host::item::Guard::Shared {
            value.ty = Ty::Reference {
                mutable: false,
                target: Box::new(inner),
            };
            return value;
        }
        let call = value.span;
        let borrow = value.rendered_source();
        let source = self.prelude_op(PreludeOp::CellGet, [borrow.clone()]);
        let mut expression = self.operator_expr(
            source,
            Ty::Reference {
                mutable: true,
                target: Box::new(inner.clone()),
            },
            call,
        );
        expression.outcome = value.outcome;
        expression.evaluation = value.evaluation;
        let place = self.temporary("cell");
        self.bindings.insert(
            place.as_str().into(),
            Binding {
                ty: Ty::Unknown,
                mutable: true,
                available: true,
                declaration: call,
                kind: BindingKind::Local,
            },
        );
        expression.lvalue = Some(Lvalue::Nested(Box::new(lvalue::Nested {
            binding: place.as_str().into(),
            base: borrow.clone(),
            steps: vec![PlaceStep::Cell {
                release: true,
                native: true,
            }],
            ty: inner.clone(),
        })));
        self.last_cell_borrow = Some(cell::CellBorrow {
            span: call,
            borrow,
            mutable: true,
            inner,
            native: true,
        });
        expression
    }

    /// A `&mut T` argument of a host call that is a place a crate method
    /// lent `&mut` (`guarded_output`), as `fill(buffer.get_mut(0), 7)`
    /// (#1318): its value is read out of the place before the statement,
    /// lent to the call, and set back into the place after it, so what
    /// the call writes reaches the crate's own value.
    pub(super) fn lend_guarded_place(&mut self, expected: &Ty, argument: &mut ParsedExpr) {
        if !matches!(
            self.inference.resolve(expected),
            Ty::Reference { mutable: true, .. }
        ) {
            return;
        }
        let Some(Lvalue::Nested(nested)) = &argument.lvalue else {
            return;
        };
        if !nested
            .steps
            .iter()
            .any(|step| matches!(step, PlaceStep::Cell { native: true, .. }))
        {
            return;
        }
        let nested = nested.clone();
        argument.source = self.nested_place(&nested);
        argument.prelude = origin::Fragment::default();
        argument.lvalue = None;
    }

    /// An imported method's `Option<&mut T>` lent from its receiver, as
    /// pane_grid's `state.get_mut(pane)`: the call gives `Some` of a place
    /// (`island::place::Place`) or `None`. The value is `Some` of a copy of
    /// the target, whose place a pattern's `&mut` binding reaches through
    /// the place at each use (`pattern_place`).
    pub(super) fn optional_guard(&mut self, value: ParsedExpr) -> ParsedExpr {
        let call = value.span;
        let target = match self.inference.resolve(&value.ty) {
            Ty::Option(target) => *target,
            other => other,
        };
        let slot = self.temporary("borrowed");
        self.field_entries.push((
            origin::Fragment::plain(format!("let {slot} = (); ")),
            origin::Fragment::default(),
        ));
        self.bindings.insert(
            slot.as_str().into(),
            Binding {
                ty: Ty::Unknown,
                mutable: true,
                available: true,
                declaration: call,
                kind: BindingKind::Local,
            },
        );
        let held = origin::Fragment::plain(slot.as_str());
        let present = self.prelude_op(PreludeOp::IsSome, [held.clone()]);
        let place = self.prelude_op(PreludeOp::Unwrap, [held]);
        let read = self.prelude_op(PreludeOp::CellGet, [place.clone()]);
        let some = self.prelude_op(PreludeOp::Some, [read]);
        let none = self.prelude_op(PreludeOp::None, []);
        // A pattern matching the value makes the call and reaches the place.
        let mut find = origin::Fragment::plain(format!("{slot} = "));
        find.push(value.rendered_source());
        find.push_text("; ");
        let missing = present.clone().surrounded("!(", ")");
        self.optional_subjects.insert(call, (find, missing));
        let mut source = origin::Fragment::plain(format!("{{ {slot} = "));
        source.push(value.rendered_source());
        source.push_text("; if ");
        source.push(present);
        source.push_text(" { ");
        source.push(some);
        source.push_text(" } else { ");
        source.push(none);
        source.push_text(" } }");
        let ty = Ty::Option(Box::new(Ty::Reference {
            mutable: true,
            target: Box::new(target.clone()),
        }));
        let mut parsed = self.operator_expr(source.surrounded("(", ")"), ty, call);
        parsed.outcome = value.outcome;
        parsed.evaluation = value.evaluation;
        parsed.lvalue = Some(Lvalue::Nested(Box::new(lvalue::Nested {
            binding: slot.as_str().into(),
            base: place,
            steps: vec![PlaceStep::Cell {
                release: false,
                native: true,
            }],
            ty: target,
        })));
        parsed
    }

    #[cfg(feature = "rust-import")]
    pub(super) fn ambiguous_imported_member(
        &mut self,
        ty: &Ty,
        member: &str,
        member_span: Span,
        method: bool,
    ) -> Option<Error> {
        let (identity, owner, alias) = ty.imported_alias_context()?;
        if alias.is_some() {
            return None;
        }
        // `to_string()` is Rust's blanket `ToString`, through `Display`
        // (`parse_host_method`).
        if method && member == "to_string" && self.imported_member_owner(ty, member).is_none() {
            return None;
        }
        // `into()` is std's blanket `Into` over the target's `From`, which
        // the use site picks (`parse_host_into`), whatever alias names the
        // type.
        if method && member == "into" && matches!(self.peek(), Token::Unit) {
            return None;
        }
        // Every alias names the same Rust type: a method any alias has is
        // the type's own, and so is a field, as `frame.area().x` reads
        // `Rect::x` whichever path `Frame::area` names `Rect` by.
        if method && self.imported_member_owner(ty, member).is_some() {
            return None;
        }
        if !method && self.imported_field_owner(ty, member).is_some() {
            return None;
        }
        // An adaptor of std's `Iterator` on a crate's iterator, as `map` of
        // itertools' `Product`, is the program's own chain over its `next`
        // (`iterator_receiver`), and a method of a trait in scope that every
        // iterator has, as `cartesian_product`, is the trait's.
        if method
            && (self.iterator_receiver(ty, member)
                || self.type_implements(ty, "Iterator")
                    && std_method::iterator(member, |_| true).is_none()
                    && self.scoped_trait_member(member).is_some())
        {
            return None;
        }
        let paths = self
            .models
            .host
            .imported_type_paths(identity, owner)?
            .collect::<Vec<_>>();
        let first = paths.first()?;
        // `Trait::method(value)` the trait answers over its `Self`
        // (`spliced_trait_call`) needs no member of the type's own.
        if method && self.spliced_trait_member(member, member_span).is_some() {
            return None;
        }
        // A method no alias has yet may come from a trait the type
        // implements: demand it through the first alias.
        if method
            && paths.iter().all(|path| {
                self.models
                    .host
                    .entry(&format!("{path}::{member}"))
                    .is_none()
            })
        {
            let error = Error::UnknownMember {
                owner: first.as_str().into(),
                member: member.into(),
                reference: member_span,
            };
            return Some(self.native_miss(true, error));
        }
        let item = if method {
            format!("{first}::{member}")
        } else {
            first.clone()
        };
        Some(Error::InvalidHostOperation {
            item: item.into(),
            reason: format!(
                "`{member}` is reachable from this value's type by several paths; name it by \
                 one of: {}",
                paths.join(", ")
            )
            .into(),
            operation: member_span,
        })
    }

    /// `Preset::to_string` of an imported type that has no `to_string` of
    /// its own: Rust's blanket `ToString` through its `Display`, as a
    /// function value `|value: &Preset| value.to_string()`. `None` for any
    /// other member, or a call.
    #[cfg(feature = "rust-import")]
    pub(super) fn imported_to_string_value(
        &mut self,
        definition: &crate::host::type_def::Definition,
        facade: Option<crate::host::facade::Dependency>,
        member: &str,
        owner_span: Span,
        member_span: Span,
    ) -> Result<Option<ParsedExpr>, Error> {
        if member != "to_string"
            || !definition.parameters().is_empty()
            || matches!(self.peek(), Token::Unit)
        {
            return Ok(None);
        }
        let ty = Ty::Host {
            proof: HostTyProof::new(definition.clone(), Some(definition.clone()), facade),
            arguments: Box::default(),
        };
        if self.imported_member_owner(&ty, member).is_some() {
            return Ok(None);
        }
        // `Look::to_string(&look)`: `look.to_string()`.
        if matches!(self.peek(), Token::LeftParen) {
            let Some(tokens) = self.path_method_call(member, member_span, None) else {
                return Err(Error::argument_count(
                    Callee::AssociatedFunction,
                    format!("{}::{member}", definition.path()),
                    1,
                    0,
                    span(owner_span.start(), member_span.end()),
                ));
            };
            self.splice_tokens(tokens);
            return self.parse_primary().map(Some);
        }
        let receiver = Ty::Reference {
            mutable: false,
            target: Box::new(ty),
        };
        self.method_value(member.into(), Box::new([receiver]), owner_span, member_span)
            .map(Some)
    }

    /// The first alias of an imported value's type that has `member`, for a
    /// value no alias names (a function's result). Every alias is the same
    /// Rust type, so the method is the same through each.
    #[cfg(feature = "rust-import")]
    pub(super) fn imported_member_owner(
        &self,
        ty: &Ty,
        member: &str,
    ) -> Option<crate::host::type_def::Definition> {
        let (identity, owner, None) = ty.imported_alias_context()? else {
            return None;
        };
        self.models
            .host
            .imported_type_paths(identity, owner)?
            .find(|path| {
                self.models
                    .host_items(&format!("{path}::{member}"))
                    .is_some()
            })
            .and_then(|path| self.models.host.type_(&path).cloned())
    }

    /// The imported type `ty` as another crate's facade names it, when only
    /// that facade has `member`: a trait of that crate gives it by an impl
    /// for this crate's type, as ratzilla's `impl<B> WebRenderer for
    /// Terminal<B>` gives ratatui's terminal `draw_web` (#1450).
    #[cfg(feature = "rust-import")]
    fn imported_member_elsewhere(
        &self,
        ty: &Ty,
        member: &str,
    ) -> Option<crate::host::type_def::Definition> {
        let (identity, _, _) = ty.imported_alias_context()?;
        self.models
            .host
            .imported_type_paths_anywhere(identity)
            .find(|path| {
                self.models
                    .host_items(&format!("{path}::{member}"))
                    .is_some()
            })
            .and_then(|path| self.models.host.type_(&path).cloned())
    }

    /// The path to demand an imported type's `member` at, as its `from`,
    /// when none of its paths has one yet: its crate's public path, as
    /// `events::conversions::Background`, before the hidden one.
    #[cfg(feature = "rust-import")]
    pub(super) fn imported_public_path(&self, ty: &Ty, member: &str) -> Option<ImmutableString> {
        let (identity, owner, _) = ty.imported_alias_context()?;
        let paths = self
            .models
            .host
            .imported_type_paths(identity, owner)?
            .collect::<Vec<_>>();
        if paths.iter().any(|path| {
            self.models
                .host_items(&format!("{path}::{member}"))
                .is_some()
        }) {
            return None;
        }
        paths
            .iter()
            .find(|path| !path.contains("::__rawr_crate_"))
            .or(paths.first())
            .map(|path| path.as_str().into())
    }

    /// The inputs and output of a crate value whose type is an opaque
    /// `impl Fn(A..) -> B`, as iced's `Function::with` gives: its type's
    /// arguments the classifier marked as the bound's (`Definition::callable`).
    #[cfg(feature = "rust-import")]
    pub(super) fn native_callable(&self, ty: &Ty) -> Option<(Vec<Ty>, Ty)> {
        let Ty::Host { proof, arguments } = self.inference.resolve(ty) else {
            return None;
        };
        let callable = *rawr_import::surface::identity::Definition::callable().as_bytes();
        let argument = |name: &str| {
            proof
                .definition
                .associated(&callable, name)
                .and_then(|position| arguments.get(position).cloned())
        };
        let output = argument("Output")?;
        let inputs = (0..)
            .map_while(|index| argument(&format!("Input{index}")))
            .collect();
        Some((inputs, output))
    }

    /// Whether the crate gives the host type `ty` (or the type it refers
    /// to) a member `member` of its own.
    #[cfg(feature = "rust-import")]
    pub(super) fn host_has_member(&self, ty: &Ty, member: &str) -> bool {
        let resolved = match self.inference.resolve(ty) {
            Ty::Reference { target, .. } => self.inference.resolve(&target),
            ty => ty,
        };
        let member = crate::tokenizer::unraw(member);
        resolved
            .host_alias()
            .cloned()
            .or_else(|| self.imported_member_owner(&resolved, member))
            .is_some_and(|definition| {
                self.models
                    .host_items(&format!("{}::{member}", definition.path()))
                    .is_some()
            })
    }

    /// The first alias of an imported type with no alias of its own whose
    /// struct has the field `member`.
    #[cfg(feature = "rust-import")]
    pub(super) fn imported_field_owner(
        &self,
        ty: &Ty,
        member: &str,
    ) -> Option<crate::host::type_def::Definition> {
        let (identity, owner, None) = ty.imported_alias_context()? else {
            return None;
        };
        self.models
            .host
            .imported_type_paths(identity, owner)?
            .filter_map(|path| self.models.host.type_(&path))
            .find(|definition| {
                matches!(definition.shape(), crate::host::type_def::Shape::Struct { fields, .. }
                    if fields.iter().any(|field| field.name() == member))
            })
            .cloned()
    }

    /// Whether a call whose output borrows what it takes besides a
    /// receiver (`frame`) takes only text lent at `'static` and scalars: a
    /// string literal is `&'static str`, so rustc types
    /// `Slot::from("Verified")` a `Slot<'static, _>` that borrows nothing of
    /// the program's. Each such
    /// text is then marked (`PreludeOp::StaticText`), so the call pins it
    /// for the runner's life and gives a plain value, and no scope frame
    /// opens for it. A borrow of a local (`name.as_str()`) keeps its frame.
    pub(super) fn lend_static_texts(
        &mut self,
        frame: Option<crate::host::item::Frame>,
        arguments: &mut [ParsedExpr],
    ) -> bool {
        if frame != Some(crate::host::item::Frame::Inputs) {
            return false;
        }
        let texts = arguments
            .iter()
            .map(|argument| self.static_text(argument))
            .collect::<Vec<_>>();
        let scalar = |parser: &Self, argument: &ParsedExpr| {
            matches!(
                parser.inference.resolve(&argument.ty),
                Ty::Int(_) | Ty::Float(_) | Ty::Bool | Ty::Char
            )
        };
        if !texts.contains(&true)
            || arguments
                .iter()
                .zip(&texts)
                .any(|(argument, text)| !text && !scalar(self, argument))
        {
            return false;
        }
        for (argument, _) in arguments.iter_mut().zip(texts).filter(|(_, text)| *text) {
            argument.source = self.prelude_op(PreludeOp::StaticText, [argument.rendered_source()]);
            argument.prelude = origin::Fragment::default();
        }
        true
    }

    /// Whether `argument` is text that borrows nothing of the body, as a
    /// string literal or a `const` of `&str` is: `&'static str`.
    pub(super) fn static_text(&self, argument: &ParsedExpr) -> bool {
        matches!(self.inference.resolve(&argument.ty), Ty::Str)
            && self
                .flow()
                .origins(argument.span)
                .is_some_and(<[_]>::is_empty)
    }

    /// `value`, a call of a `&mut self` method changing its receiver where
    /// it is, with each of its `&mut` array or string arguments (`places`)
    /// stored back from what it gives, as std's `Cursor::read_line(&mut
    /// self, buf: &mut String)` changes both.
    fn copied_places_back(
        &mut self,
        mut value: ParsedExpr,
        places: &[ParsedExpr],
    ) -> Result<ParsedExpr, Error> {
        if !places.is_empty() {
            value.source = self.copied_call(value.rendered_source(), places)?;
            value.prelude = origin::Fragment::default();
        }
        Ok(value)
    }

    /// A host call whose value borrows what it takes (`opens`, D558 tier
    /// 1), or that copies `&mut` arrays in and back out (`places`). A call
    /// that doesn't open a scope frame gives its value and then each copy.
    /// One that does is bound by a `let` at the start of the statement it
    /// is in, so the statement runs inside the frame. Its copies come back
    /// when the frame ends, and are stored after the statement, where its
    /// value no longer borrows them, as rustc's borrow of them ends.
    pub(super) fn scoped_call(
        &mut self,
        mut value: ParsedExpr,
        opens: bool,
        places: &[ParsedExpr],
    ) -> Result<ParsedExpr, Error> {
        if !opens {
            if places.is_empty() {
                return Ok(value);
            }
            value.source = self.copied_call(value.rendered_source(), places)?;
            value.prelude = origin::Fragment::default();
            return Ok(value);
        }
        if self.frames.is_empty() {
            return Err(
                self.unsupported("a call whose value borrows what it takes, outside a statement")
            );
        }
        // The frame opens ahead of the statement: a call or assignment
        // written before this call in it, outside it, would run after it.
        let start = value.span.start();
        if let Some((statement, effects)) = self.frame_effects.last_mut() {
            effects.retain(|(at, _)| *at < start);
            // The calls of an array's earlier elements, bound ahead of the
            // frame (`FrameArray`).
            let depth = self.frames.len();
            if let Some(array) = self
                .frame_arrays
                .last_mut()
                .filter(|array| array.depth == depth && array.start >= *statement)
            {
                let (element, index, mark) = array.element;
                if !effects.is_empty()
                    && effects
                        .iter()
                        .all(|(_, noted)| (array.noted..element).contains(noted))
                {
                    effects.clear();
                    if array.bound.last().is_none_or(|(last, _)| *last != index) {
                        array.bound.push((index, mark));
                    }
                }
            }
            if let Some(&(earlier, _)) = effects.first() {
                let line = statement.line().unwrap_or_default();
                return Err(Error::UnsupportedNominalSyntax {
                    construct: format!(
                        "a call or assignment run before a call whose value borrows a local \
                         it takes, in the statement at line {line}, is not supported in Rawr \
                         yet"
                    )
                    .into(),
                    source: point_span(earlier),
                });
            }
        }
        let made = self.temporary("frame");
        let mut frame = Frame::default();
        let bound = if places.is_empty() {
            frame.inner.push_text(format!("let {made} = "));
            frame.inner.push(value.rendered_source());
            frame.inner.push_text("; ");
            made.to_string()
        } else {
            // `made` keeps the frame the call made, and `frame` opens it.
            let opened = self.temporary("opened");
            frame.outer.push_text(format!("let {made} = (); "));
            frame.inner.push_text(format!("{made} = "));
            frame.inner.push(value.rendered_source());
            frame.inner.push_text(format!("; let {opened} = {made}; "));
            let helper = self.push_op(Op::Prelude(PreludeOp::FrameCopy));
            for (index, place) in places.iter().enumerate() {
                let stored = self.store_into(place, &format!("{helper}({made}, {index})"))?;
                frame.after.push(stored);
                frame.after.push_text(" ");
            }
            opened.to_string()
        };
        if let Some(frames) = self.frames.last_mut() {
            frames.push(frame);
        }
        self.bindings.insert(
            bound.as_str().into(),
            Binding {
                ty: value.ty.clone(),
                mutable: true,
                available: true,
                declaration: value.span,
                kind: BindingKind::Local,
            },
        );
        value.prelude = origin::Fragment::default();
        value.source = origin::Fragment::plain(bound.clone());
        value.lvalue = Some(Lvalue::Binding {
            binding: bound.as_str().into(),
            ty: value.ty.clone(),
        });
        Ok(value.retain_origin())
    }

    pub(super) fn consume_imported_value(
        &mut self,
        value: &ParsedExpr,
        operation: &str,
        span: Span,
    ) -> Result<(), Error> {
        // A borrow moves nothing: `&mut list` lends `list` to the call.
        if matches!(self.inference.resolve(&value.ty), Ty::Reference { .. }) {
            return Ok(());
        }
        if self
            .inference
            .resolve(&value.ty)
            .host()
            .and_then(|definition| definition.identity().imported())
            .is_none()
        {
            return Ok(());
        }
        if self.type_provides(&value.ty, capability::Kind::Copy) {
            return Ok(());
        }
        let Some(name) = variable_name(value.source.as_str()) else {
            return Ok(());
        };
        let Some(binding) = self.bindings.get_mut(name.as_str()) else {
            return Ok(());
        };
        if !binding.available {
            return Err(Error::InvalidHostOperation {
                item: operation.into(),
                reason: format!("use of moved value: `{name}` (E0382)").into(),
                operation: span,
            });
        }
        binding.available = false;
        let declaration = binding.declaration;
        self.moved_at.insert(declaration, value.span);
        Ok(())
    }

    /// `local.field` by value of a crate struct whose every field the
    /// program can name, as inboard's `action.task` of a `common::Action`:
    /// only the field moves, as rustc moves it, and the struct's other
    /// fields stay for the program (#834), apart in the runner's slot. A
    /// struct behind a reference, or with fields the program can't name,
    /// still moves whole.
    #[cfg(feature = "rust-import")]
    fn moves_field_out(
        &mut self,
        receiver: &ParsedExpr,
        definition: &crate::host::type_def::Definition,
    ) -> bool {
        let crate::host::type_def::Shape::Struct {
            constructible: true,
            ..
        } = definition.shape()
        else {
            return false;
        };
        if receiver.lvalue.is_none()
            || !receiver.prelude.is_empty()
            || matches!(self.inference.resolve(&receiver.ty), Ty::Reference { .. })
        {
            return false;
        }
        let local = variable_name(receiver.source.as_str())
            .and_then(|name| self.bindings.get(name.as_str()))
            .is_some_and(|binding| matches!(binding.kind, BindingKind::Local));
        if local {
            self.flow().moved(receiver.span.start());
        }
        local
    }

    /// `value.field` of an imported struct: a read, or, before `=`, the
    /// field's store (`store_imported_field`).
    #[cfg(feature = "rust-import")]
    pub(super) fn parse_imported_field(
        &mut self,
        receiver: ParsedExpr,
        definition: crate::host::type_def::Definition,
        member: ImmutableString,
        member_span: Span,
    ) -> Result<ParsedExpr, Error> {
        // `a.b = v` or `a.b.c = v`: the fields after this one up to a plain
        // `=`, each a field of the one before. `a.b += v` and its kin store
        // the operator's value, `a.b = a.b + v` as Rust's `Copy` fields do.
        let mut ahead = self.input.clone();
        let mut after = Vec::new();
        let mut operator = None;
        let stored = loop {
            match ahead.next() {
                Some((Token::Equals, _)) => break true,
                Some((token, _)) if after.is_empty() && compound_operator(&token).is_some() => {
                    operator = compound_operator(&token);
                    break true;
                }
                Some((Token::Period, _)) => match ahead.next() {
                    Some((Token::Identifier(name), position)) => {
                        let name = ImmutableString::from(name.as_str());
                        let at = Span::new(position, text_end(position, &name));
                        after.push((name, at));
                    }
                    _ => break false,
                },
                _ => break false,
            }
        };
        if !stored || self.split_angle.is_some() {
            return self.read_imported_field(receiver, definition, member, member_span);
        }
        for _ in 0..after.len() * 2 + 1 {
            self.next();
        }
        let mut fields = vec![(member.clone(), member_span)];
        fields.extend(after);
        // Rust evaluates the right side before evaluating the assigned
        // place, including before reading any intermediate Copy field.
        let mut value = self.parse_expr()?;
        let temporary = self.temporary("assigned");
        let mut source = origin::Fragment::plain(format!("let {temporary} = "));
        source.push(value.rendered_source());
        source.push_text("; ");
        value.prelude = origin::Fragment::default();
        value.source = origin::Fragment::plain(temporary);
        if let Some(operator) = operator {
            let current = self.read_imported_field(
                receiver.clone(),
                definition.clone(),
                member,
                member_span,
            )?;
            value = self.compound_assignment(operator, current, value)?;
        }
        let mut stored = self.store_imported_field(receiver, definition, &fields, false, value)?;
        source.push(stored.rendered_source());
        stored.source = source.surrounded("{ ", " }");
        stored.prelude = origin::Fragment::default();
        Ok(stored)
    }

    /// `place.a.b = value` of an imported struct's fields `a`, `b`: the
    /// value set in place, through the place's handle or a value native
    /// code lends `&mut` (custom_widget's `tree.size = Size::new(..)`), as
    /// a `&mut self` method changes it. Rust sets a field below another
    /// where it is: Rawr reads each field on the way, a `Copy` value,
    /// sets the next one in it and sets it back, as a script place is
    /// changed (`nested_parts`). The `=` is read.
    #[cfg(feature = "rust-import")]
    pub(super) fn store_imported_field(
        &mut self,
        receiver: ParsedExpr,
        definition: crate::host::type_def::Definition,
        fields: &[(ImmutableString, Span)],
        below: bool,
        mut assigned: ParsedExpr,
    ) -> Result<ParsedExpr, Error> {
        let (member, member_span) = fields[0].clone();
        let start = receiver.span.start();
        // The place must be mutable, as Rust's assignment needs.
        let resolved = self.inference.resolve(&receiver.ty);
        match &resolved {
            // A field read on the way is this store's own local.
            _ if below => {}
            Ty::Reference { mutable: true, .. } => {}
            Ty::Reference { mutable: false, .. } => {
                return Err(Error::InvalidHostOperation {
                    item: definition.path().into(),
                    reason: format!(
                        "`{member}` is behind a `&` reference, so it can't be assigned"
                    )
                    .into(),
                    operation: member_span,
                });
            }
            _ => {
                let root = match &receiver.lvalue {
                    Some(lvalue) => Some(lvalue.binding().clone()),
                    None => variable_name(receiver.source.as_str()),
                };
                let Some(root) = root else {
                    return Err(self.unsupported("assignment target"));
                };
                match self.mutation_binding(root.as_str()) {
                    Some(binding) if binding.mutable => {}
                    Some(binding) => {
                        return Err(match &binding.kind {
                            BindingKind::Receiver { .. } => self.receiver_assignment(
                                &binding.ty,
                                span(receiver.span.start(), member_span.end()),
                            ),
                            BindingKind::Local | BindingKind::Invocation => {
                                let place = self
                                    .written(span(receiver.span.start(), member_span.end()))
                                    .unwrap_or_else(|| root.to_string());
                                Error::assign_immutable(&place, &root, member_span)
                            }
                        });
                    }
                    None => return Err(self.unsupported("assignment through an unknown binding")),
                }
                self.mutable_use(&receiver, member_span);
            }
        }
        // A place a crate method lends `&mut`, as `buffer.get_mut(0).0 = v`
        // (#1318): its value is read out of the method's place, changed,
        // and set back into it, so the crate's own value changes.
        if let Some(Lvalue::Nested(nested)) = &receiver.lvalue
            && nested
                .steps
                .iter()
                .any(|step| matches!(step, PlaceStep::Cell { native: true, .. }))
        {
            let (read, place, store) = self.nested_parts(nested);
            let target = nested.ty.clone();
            let local = self.operator_expr(place, target, receiver.span);
            let stored = self.store_imported_field(local, definition, fields, true, assigned)?;
            let operation = stored.span;
            let mut source = read;
            source.push(stored.rendered_source());
            source.push_text("; ");
            source.push(store);
            return Ok(self.operator_expr(source.surrounded("{ ", " }"), Ty::Unit, operation));
        }
        let path: ImmutableString = definition.path().into();
        let crate::host::type_def::Shape::Struct {
            fields: declared, ..
        } = definition.shape()
        else {
            return Err(Error::UnknownMember {
                owner: path,
                member,
                reference: member_span,
            });
        };
        let Some(field) = declared.iter().find(|field| field.name() == member) else {
            return Err(Error::UnknownMember {
                owner: path,
                member,
                reference: member_span,
            });
        };
        let selector = field.imported_identity().clone();
        // The receiver once, in a local the reads and stores share.
        let place = self.temporary("place");
        let mut prelude = origin::Fragment::plain(format!("let {place} = "));
        prelude.push(receiver.rendered_source());
        prelude.push_text("; ");
        let local = self.operator_expr(
            origin::Fragment::plain(place.clone()),
            receiver.ty.clone(),
            receiver.span,
        );
        let value = if fields.len() == 1 {
            // The field's type at the receiver's instance, as a read takes it.
            let read = self.operator_expr(
                origin::Fragment::plain(place.clone()),
                receiver.ty.clone(),
                receiver.span,
            );
            let ty = self.imported_field_type(&read, &definition, field.ty(), &path)?;
            self.coerce_expr(&ty, &mut assigned)?;
            self.consume_imported_value(&assigned, &path, assigned.span)?;
            self.move_local(&mut assigned)?;
            assigned
        } else {
            let inner_ty = self.imported_field_type(&local, &definition, field.ty(), &path)?;
            let inner_ty = self.inference.resolve(&inner_ty);
            let Ty::Host { proof, .. } = &inner_ty else {
                return Err(
                    self.unsupported("assigning a field of a field that isn't a crate's struct")
                );
            };
            let inner = proof.definition.clone();
            if !self.type_provides(&inner_ty, capability::Kind::Copy) {
                // `theme.colors.accent = error`: the field below is lent
                // `&mut` for the store, which changes it where it is, as
                // rustc's place `theme.colors.accent` is (#1893).
                if !definition.parameters().is_empty() {
                    return Err(self.unsupported(
                        "assigning a field of a field that isn't `Copy`, of a generic struct",
                    ));
                }
                let lend = ImportedOperation {
                    kind: crate::host::structural::Kind::LendMut,
                    operands: vec![(local.ty.clone(), local.span, true)].into_boxed_slice(),
                    output: inner_ty.clone(),
                    receiver: Some(crate::receiver::Mode::Mutable),
                };
                let helper = self.push_imported_op(
                    &path,
                    &selector,
                    Some(lend),
                    span(start, member_span.end()),
                )?;
                let lent = self.temporary("lent");
                prelude.push_text(format!("let {lent} = "));
                prelude.push(origin::Fragment::call(&helper, [local.source]));
                prelude.push_text("; ");
                let lent = self.operator_expr(origin::Fragment::plain(lent), inner_ty, member_span);
                let changed =
                    self.store_imported_field(lent, inner, &fields[1..], true, assigned)?;
                let operation = span(start, changed.span.end());
                prelude.push(changed.rendered_source());
                return Ok(self.operator_expr(prelude.surrounded("{ ", " }"), Ty::Unit, operation));
            }
            // The field below, a copy: read, changed in it, and set back.
            let read = self.read_imported_field(
                local.clone(),
                definition.clone(),
                member.clone(),
                member_span,
            )?;
            let below = self.temporary("field");
            prelude.push_text(format!("let {below} = "));
            prelude.push(read.rendered_source());
            prelude.push_text("; ");
            let below_local = self.operator_expr(
                origin::Fragment::plain(below.clone()),
                inner_ty.clone(),
                member_span,
            );
            let changed =
                self.store_imported_field(below_local, inner, &fields[1..], true, assigned)?;
            prelude.push(changed.rendered_source());
            prelude.push_text("; ");
            self.operator_expr(origin::Fragment::plain(below), inner_ty, member_span)
        };
        let end = value.span.end();
        let operation = span(start, end);
        let imported = ImportedOperation {
            kind: crate::host::structural::Kind::Store,
            operands: vec![
                (local.ty.clone(), local.span, true),
                (value.ty.clone(), value.span, false),
            ]
            .into_boxed_slice(),
            output: Ty::Unit,
            receiver: Some(crate::receiver::Mode::Mutable),
        };
        let helper = self.push_imported_op(&path, &selector, Some(imported), operation)?;
        prelude.push(origin::Fragment::call(
            &helper,
            [local.source, value.rendered_source()],
        ));
        Ok(self.operator_expr(prelude.surrounded("{ ", " }"), Ty::Unit, operation))
    }

    /// The type of a field declared `field_type` of an imported struct
    /// `definition`, read from `receiver`, at the receiver's instance.
    #[cfg(feature = "rust-import")]
    pub(super) fn imported_field_type(
        &mut self,
        receiver: &ParsedExpr,
        definition: &crate::host::type_def::Definition,
        field_type: &crate::host::type_ref::Type,
        path: &str,
    ) -> Result<Ty, Error> {
        let resolved = self.inference.resolve(&receiver.ty);
        let held = without_references(&resolved);
        let selected_owner = match held {
            Ty::Host { proof, .. } => proof.owner.clone(),
            _ => None,
        }
        .or_else(|| self.models.host.type_owner(path).cloned());
        let mut parameters = definition
            .parameters()
            .iter()
            .zip(match held {
                Ty::Host { arguments, .. } => arguments.iter(),
                _ => [].iter(),
            })
            .map(|(name, argument)| (name.clone(), argument.clone()))
            .collect();
        self.instantiate_host_type(
            field_type,
            &mut parameters,
            path,
            receiver.span,
            selected_owner.as_ref(),
            Some(definition),
        )
    }

    /// Whether the field of type `ty` read from `receiver` is borrowed
    /// where it is read, as rustc borrows it: the whole operand of a `&`,
    /// the receiver of a method that takes it `&self`, its own or its
    /// `Deref` target's, or the place a `Copy` field of it is copied out of
    /// (`copied_below`).
    #[cfg(feature = "rust-import")]
    pub(super) fn borrows_field(&mut self, receiver: &ParsedExpr, ty: &Ty) -> bool {
        match self.peek() {
            Token::Period => {
                let mut ahead = self.input.clone();
                ahead.next();
                let (
                    Some((Token::Identifier(member), _)),
                    Some((Token::LeftParen | Token::Unit, _)),
                ) = (ahead.next(), ahead.next())
                else {
                    return self.copied_below(ty).is_some();
                };
                self.borrows_for_method(ty, member.as_str())
            }
            Token::LeftParen | Token::Unit | Token::LeftBracket => false,
            _ => self.lent_field == Some(receiver.span.start()),
        }
    }

    /// The `Copy` field of a crate struct of type `ty` the next tokens read,
    /// as `.error` of `theme.colors.error`, with its struct's definition:
    /// rustc copies it out of the place, and `theme.colors` stays whole
    /// (#1894).
    #[cfg(feature = "rust-import")]
    fn copied_below(
        &mut self,
        ty: &Ty,
    ) -> Option<(ImmutableString, Span, crate::host::type_def::Definition)> {
        let mut ahead = self.input.clone();
        let (Some((Token::Period, _)), Some((Token::Identifier(member), position))) =
            (ahead.next(), ahead.next())
        else {
            return None;
        };
        if matches!(ahead.peek(), Some((Token::LeftParen | Token::Unit, _))) {
            return None;
        }
        let Ty::Host { proof, .. } = self.inference.resolve(ty) else {
            return None;
        };
        let definition = proof.definition.clone();
        let crate::host::type_def::Shape::Struct { fields, .. } = definition.shape() else {
            return None;
        };
        let field = fields
            .iter()
            .find(|field| field.name() == member.as_str())?;
        let held = self.operator_expr(origin::Fragment::default(), ty.clone(), Span::NONE);
        let path = definition.path();
        let below = self
            .imported_field_type(&held, &definition, field.ty(), &path)
            .ok()?;
        if !self.type_provides(&below, capability::Kind::Copy) {
            return None;
        }
        // `theme.colors.error.darken()` through `&mut self` changes the
        // field where it is, not a copy of it.
        if let (Some((Token::Period, _)), Some((Token::Identifier(method), _))) =
            (ahead.next(), ahead.next())
            && self.member_receiver(&below, method.as_str()) == Some(crate::receiver::Mode::Mutable)
        {
            return None;
        }
        let member = ImmutableString::from(member.as_str());
        let at = Span::new(position, text_end(position, &member));
        Some((member, at, definition))
    }

    #[cfg(feature = "rust-import")]
    pub(super) fn read_imported_field(
        &mut self,
        receiver: ParsedExpr,
        definition: crate::host::type_def::Definition,
        member: ImmutableString,
        member_span: Span,
    ) -> Result<ParsedExpr, Error> {
        let path: ImmutableString = definition.path().into();
        let crate::host::type_def::Shape::Struct { fields, .. } = definition.shape() else {
            return Err(Error::UnknownMember {
                owner: path,
                member,
                reference: member_span,
            });
        };
        let Some(field) = fields.iter().find(|field| field.name() == member) else {
            let error = Error::UnknownMember {
                owner: path,
                member,
                reference: member_span,
            };
            return Err(self.native_miss(definition.identity().imported().is_some(), error));
        };
        let field_type = field.ty().clone();
        // A value native code lends, as `&mut widget::Tree`, is read through
        // its reference, and its facade is the one that declares its type.
        let resolved = self.inference.resolve(&receiver.ty);
        let held = without_references(&resolved);
        let selected_owner = match held {
            Ty::Host { proof, .. } => proof.owner.clone(),
            _ => None,
        }
        .or_else(|| self.models.host.type_owner(&path).cloned());
        let mut parameters = definition
            .parameters()
            .iter()
            .zip(match held {
                Ty::Host { arguments, .. } => arguments.iter(),
                _ => [].iter(),
            })
            .map(|(name, argument)| (name.clone(), argument.clone()))
            .collect();
        let ty = self.instantiate_host_type(
            &field_type,
            &mut parameters,
            &path,
            member_span,
            selected_owner.as_ref(),
            Some(&definition),
        )?;
        let operation = span(receiver.span.start(), member_span.end());
        // A Copy field, or one Rawr holds as its own value (a `String`), is
        // read where it is and leaves the struct whole, as Rust reads
        // `font.family`; a by-value use moves only the field (`move_local`).
        // Any other field takes the struct.
        let shared = self.type_provides(&ty, capability::Kind::Copy) || self.plain_value(&ty);
        // `&shot.rgba`, and `shot.rgba.len()` through a `&self` method: the
        // field lent where it is, for the statement, or for the rest of the
        // block of a `let`, as Rust borrows it, and the struct stays whole
        // (D683.1). A crate value field is lent by its own type; a `Vec` or
        // a box of one is still read by the reader.
        if !shared
            && matches!(self.inference.resolve(&ty), Ty::Host { .. })
            && definition.parameters().is_empty()
            && receiver.prelude.is_empty()
            && variable_name(receiver.source.as_str()).is_some()
            && !self.frames.is_empty()
            && self.borrows_field(&receiver, &ty)
        {
            let lend = ImportedOperation {
                kind: crate::host::structural::Kind::Borrow { field: 0 },
                operands: vec![(receiver.ty.clone(), receiver.span, true)].into_boxed_slice(),
                output: ty.clone(),
                receiver: Some(crate::receiver::Mode::Shared),
            };
            let helper =
                self.push_imported_op(&path, field.imported_identity(), Some(lend), operation)?;
            let lent = ParsedExpr {
                prelude: receiver.prelude,
                source: origin::Fragment::call(&helper, [receiver.source]),
                ty,
                span: operation,
                lvalue: None,
                outcome: crate::host::usage::Outcome::Conservative,
                evaluation: crate::host::usage::Evaluation::Required,
            };
            // `theme.colors.error`: the `Copy` field below is copied out of
            // the lent field in a block the lend ends with, as rustc's
            // borrow of the place ends with the copy (#1893, #1894).
            if let Some((below, below_span, inner)) = self.copied_below(&lent.ty) {
                self.next();
                self.next();
                let place = self.temporary("lent");
                let local = self.operator_expr(
                    origin::Fragment::plain(place.clone()),
                    lent.ty.clone(),
                    operation,
                );
                let read = self.read_imported_field(local, inner, below, below_span)?;
                let mut source = origin::Fragment::plain(format!("{{ let {place} = "));
                source.push(lent.source);
                source.push_text("; ");
                source.push(read.rendered_source());
                source.push_text(" }");
                return Ok(self.operator_expr(
                    source,
                    read.ty,
                    span(receiver.span.start(), below_span.end()),
                ));
            }
            return self.scoped_call(lent, true, &[]);
        }
        if shared {
            self.shared_field_reads.insert(operation);
            // `&theme.name` borrows the field where it is, as Rust's place
            // `theme.name` is, not a copy of it: a `fn name(&self) -> &str`
            // may return it (#1468).
            if receiver.lvalue.is_some() || self.flow().is_place(receiver.span) {
                self.flow().mark_place(operation);
            }
        } else if !self.moves_field_out(&receiver, &definition) {
            self.consume_imported_value(&receiver, &path, operation)?;
        }
        let imported = ImportedOperation {
            kind: crate::host::structural::Kind::Field { copy: shared },
            operands: vec![(receiver.ty.clone(), receiver.span, true)].into_boxed_slice(),
            output: ty.clone(),
            receiver: Some(if shared {
                crate::receiver::Mode::Shared
            } else {
                crate::receiver::Mode::Owned
            }),
        };
        let helper =
            self.push_imported_op(&path, field.imported_identity(), Some(imported), operation)?;
        // `item.name.push('!')`: a field Rawr holds as its own value, of a
        // place, is a place a method changes, read out and stored back as
        // `item.name = name` (#1139). So is an element of one, as
        // `pixel.0[3] += 1` through a `&mut Pixel` (#1318), and the field
        // a `&mut` lends, as `fill(&mut pixel.0, 7)` writes (#1330).
        let lvalue = match &receiver.lvalue {
            Some(lvalue)
                if shared
                    && (!self.type_provides(&ty, capability::Kind::Copy)
                        && self.method_follows()
                        || matches!(self.peek(), Token::LeftBracket)
                        || self.lent_mut_field == Some(receiver.span.start())) =>
            {
                let place = self.temporary("place");
                let stored = self.temporary("stored");
                let local = self.operator_expr(
                    origin::Fragment::plain(place.clone()),
                    receiver.ty.clone(),
                    receiver.span,
                );
                let value = self.operator_expr(
                    origin::Fragment::plain(stored.clone()),
                    ty.clone(),
                    member_span,
                );
                let store = self.store_imported_field(
                    local,
                    definition.clone(),
                    &[(member.clone(), member_span)],
                    true,
                    value,
                )?;
                // The store changes the value where it is and gives it, so
                // the step before it sets it back (a borrow guard's place).
                let mut store = store.rendered_source();
                store.push_text(format!("; {place}"));
                let step = PlaceStep::Accessor {
                    place: place.clone(),
                    stored,
                    read: origin::Fragment::call(&helper, [origin::Fragment::plain(place)])
                        .surrounded("{ ", " }"),
                    store: store.surrounded("{ ", " }"),
                };
                match lvalue {
                    Lvalue::Binding { binding, .. } => {
                        Some(Lvalue::Nested(Box::new(lvalue::Nested {
                            binding: binding.clone(),
                            base: origin::Fragment::plain(binding.to_string()),
                            steps: vec![step],
                            ty: ty.clone(),
                        })))
                    }
                    Lvalue::Slice(_) => None,
                    lvalue => Some(nested_step(lvalue, step, ty.clone())),
                }
            }
            _ => None,
        };
        Ok(ParsedExpr {
            prelude: receiver.prelude,
            source: origin::Fragment::call(&helper, [receiver.source]),
            ty,
            span: operation,
            lvalue,
            outcome: crate::host::usage::Outcome::Conservative,
            evaluation: crate::host::usage::Evaluation::Required,
        }
        .retain_origin())
    }
}

/// Why a crate call can't take the program's function that builds its
/// message, as `Message::Changed` given to iced's `on_input`, here.
#[cfg(feature = "rust-import")]
pub(super) const UNSUPPORTED_MESSAGE_FUNCTION: &str =
    "passing the program's function that builds its message is not supported here in Rawr yet";

/// A path naming a free value and free functions, as a Rust `fn` item: the
/// value where it is not called, the functions where it is.
/// A facade path may name a macro and a value independently, as in Rust:
/// `column![..]` takes the macro and `column(..)` the function. A path in
/// one namespace keeps its items, so a wrong call syntax still reaches the
/// diagnostic that names the right one.
pub(super) fn macro_or_value(
    items: Vec<crate::host::item::Definition>,
    bang: bool,
) -> Vec<crate::host::item::Definition> {
    let is_macro = |item: &crate::host::item::Definition| item.kind().is_macro();
    if items.iter().all(is_macro) || !items.iter().any(is_macro) {
        return items;
    }
    items
        .into_iter()
        .filter(|item| is_macro(item) == bang)
        .collect()
}

pub(super) fn value_or_call(
    items: Vec<crate::host::item::Definition>,
    called: bool,
) -> Vec<crate::host::item::Definition> {
    let value = |item: &crate::host::item::Definition| {
        matches!(item.kind(), crate::host::item::Kind::Value)
    };
    // A tuple variant's constructor, beside the reader a pattern uses.
    let items = if items
        .iter()
        .any(|item| item.kind() == crate::host::item::Kind::AssociatedFunction)
    {
        items
            .into_iter()
            .filter(|item| item.kind() != crate::host::item::Kind::Method)
            .collect()
    } else {
        items
    };
    if items.iter().all(value) || !items.iter().any(value) {
        return items;
    }
    items
        .into_iter()
        .filter(|item| value(item) != called)
        .collect()
}

/// A facade field may share its name with builder methods, as iced's
/// `style.border` and `style.border(..)`: the field where it is read, the
/// builders where it is called.
pub(super) fn field_or_builder(
    items: Vec<crate::host::item::Definition>,
    field: bool,
) -> Vec<crate::host::item::Definition> {
    let reader = |item: &crate::host::item::Definition| {
        item.receiver() == Some(receiver::Mode::Shared) && item.signature().parameters().is_empty()
    };
    if items.iter().all(reader) || !items.iter().any(reader) {
        return items;
    }
    items
        .into_iter()
        .filter(|item| reader(item) == field)
        .collect()
}

/// Whether a library function's numeric `parameter` takes a `source`
/// number: an integer or a float, as Rust's literal settles.
pub(super) fn host_number_accepts(parameter: &crate::host::type_ref::Type, source: &Ty) -> bool {
    use crate::host::type_ref::{Builtin, Shape};
    match parameter.shape() {
        Shape::Builtin(Builtin::I64) => matches!(source, Ty::Int(_)),
        Shape::Builtin(Builtin::F64 | Builtin::F32) => matches!(source, Ty::Float(_)),
        _ => false,
    }
}

/// Whether `member` of the imported enum `definition` is one of its
/// variants. Any other member is an associated item, as torin's
/// `Size::fill()` is beside its variant `Size::Fill`.
#[cfg(feature = "rust-import")]
pub(super) fn names_variant(definition: &crate::host::type_def::Definition, member: &str) -> bool {
    matches!(
        definition.shape(),
        crate::host::type_def::Shape::Enum { variants, .. }
            if variants.iter().any(|variant| variant.name() == member)
    )
}

#[cfg(feature = "rust-import")]
pub(super) struct ImportedContext<'a> {
    pub(super) native: Option<&'a crate::resolve::native::Capture>,
    pub(super) allocation: &'a mut dyn crate::host::ImportAllocation,
    pub(super) failure: &'a mut Option<rawr_import::surface::Error>,
    pub(super) source: crate::program::source::Id,
    pub(super) uses: &'a mut Vec<ImportedUse>,
    pub(super) dependencies: &'a mut BTreeSet<crate::host::facade::Dependency>,
}

#[cfg(feature = "rust-import")]
impl ImportedContext<'_> {
    pub(super) fn allocation_failure(&mut self, error: rawr_import::surface::Error) -> Error {
        self.failure.replace(error);
        Error::LoweringUnavailable {
            item: "<imported allocation>".into(),
            body: Span::NONE,
        }
    }

    pub(super) fn retain_checked(
        &mut self,
        items: Option<usize>,
        bytes: Option<usize>,
    ) -> Result<(), Error> {
        self.allocation
            .retain_checked(items, bytes)
            .map_err(|error| self.allocation_failure(error))
    }

    pub(super) fn entries<T>(&mut self, count: usize) -> Result<Vec<T>, Error> {
        self.retain_checked(Some(count), count.checked_mul(std::mem::size_of::<T>()))?;
        let mut entries = Vec::new();
        entries
            .try_reserve_exact(count)
            .map_err(|_| self.allocation_failure(rawr_import::surface::Error::Allocation))?;
        Ok(entries)
    }

    pub(super) fn for_source(&mut self, source: crate::program::source::Id) -> ImportedContext<'_> {
        ImportedContext {
            native: self.native,
            allocation: self.allocation,
            failure: self.failure,
            source,
            uses: self.uses,
            dependencies: self.dependencies,
        }
    }
}

/// A facade pattern met below a script pattern: the hidden name its value
/// is bound to, the pattern, the value's type and the binding mode.
pub(super) type HostCheck = (ImmutableString, pattern::Syntax, Ty, Option<bool>);

/// Why a literal can't build an imported value whose public field rawr
/// can't hold yet: the field and its type, as the crate writes them.
#[cfg(feature = "rust-import")]
pub(super) fn unsupported_field(unsupported: &crate::host::type_def::Unsupported) -> String {
    format!(
        "field `{}` has type `{}`, which rawr can't hold yet",
        unsupported.field, unsupported.ty
    )
}
