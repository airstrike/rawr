//! Coercions at a use site: an expression to its expected type, branches
//! to one type, `&String` to `&str`, and script types to the facade types
//! they erase to.

use super::*;

impl<'a> Parser<'a> {
    /// A call's argument for a parameter declared `parameter`: an `impl
    /// IntoIterator` parameter takes any iterable (a `Vec`, `&Vec`, map,
    /// range) as the iterator over it.
    pub(super) fn coerce_argument(
        &mut self,
        parameter: &type_ref::Type,
        expected: &Ty,
        argument: &mut ParsedExpr,
    ) -> Result<(), Error> {
        // `I: IntoIterator<Item = X>` takes what Rust's `IntoIterator`
        // walks; `I: Iterator<Item = X>` a crate or imported type that
        // implements `Iterator` too, whose items its `next()` gives (T380).
        let iterator = |this: &Self, ty: &Ty| {
            let ty = this.inference.resolve(ty);
            matches!(ty, Ty::Script(..) | Ty::Host { .. }) && this.type_implements(&ty, "Iterator")
        };
        if parameter
            .iterator_item()
            .is_some_and(|(_, into)| into || iterator(self, &argument.ty))
            && !matches!(self.inference.resolve(&argument.ty), Ty::Iter(..))
            && let Some(iterator) = self.iterator_of(argument)
        {
            *argument = iterator;
        }
        self.coerce_expr(expected, argument)?;
        // A `u64` or a `usize` passed where the parameter's type names a
        // generic (`T`, `&[T]`, `Option<T>`), which the body formats and
        // orders without its type: tagged, so it stays the unsigned number
        // (`compare_value`, `Dynamic`'s `Display`).
        let mutable = matches!(parameter, type_ref::Type::Reference { mutable: true, .. });
        if !mutable
            && mentions_parameter(parameter)
            && !Shape::of(&argument.ty, &|ty| self.inference.resolve(ty)).is_plain()
        {
            let helper = self.push_op(Op::Shaped {
                op: PreludeOp::Unsigned,
                ty: argument.ty.clone(),
                flags: 0,
            });
            let value = argument.rendered_source();
            argument.prelude = origin::Fragment::default();
            argument.source = origin::Fragment::call(&helper, [value]);
            argument.source.cover(argument.span);
        }
        Ok(())
    }

    /// `&String` reads as `&str` where Rust takes a string slice.
    pub(super) fn strip_string_reference(&mut self, value: &mut ParsedExpr) {
        if let Ty::Reference { target, .. } = self.inference.resolve(&value.ty)
            && self.inference.resolve(&target).is_text()
        {
            value.ty = Ty::Str;
        }
    }

    pub(super) fn coerce_expr(
        &mut self,
        expected: &Ty,
        value: &mut ParsedExpr,
    ) -> Result<Option<String>, Error> {
        self.reject_invocation(&value.ty, value.span, "used except to call its methods")?;
        let mut source = self.inference.resolve(&value.ty);
        let target = self.inference.resolve(expected);
        // A named function passed where a host callable is expected.
        #[cfg(feature = "rust-import")]
        let imported_callable = matches!(
            target,
            Ty::ImportedFunctionItem { .. } | Ty::ProfileMapper { .. }
        );
        #[cfg(not(feature = "rust-import"))]
        let imported_callable = false;
        // A crate's own `impl Fn(A..) -> B` value where a crate's callable
        // is taken, as `Task::map(Message::Loaded.with(id))`: the value
        // itself, whose inputs and output are the callable's.
        #[cfg(feature = "rust-import")]
        if let Ty::Callable { inputs, output, .. } = &target
            && let Some((given, gives)) = self.native_callable(&source)
            && given.len() == inputs.len()
        {
            for (input, given) in inputs.iter().zip(&given) {
                self.require_type(input, given, value.span)?;
            }
            self.require_type(output, &gives, value.span)?;
            return Ok(None);
        }
        if imported_callable || matches!(target, Ty::Callable { .. }) {
            self.host_callable(value)?;
            source = self.inference.resolve(&value.ty);
        }
        if let Ty::Option(target_element) = &target
            && matches!(**target_element, Ty::Callable { .. })
        {
            self.option_host_callable(value)?;
            source = self.inference.resolve(&value.ty);
        }
        // A tuple expression coerces each field to its expected type, as
        // `pair((&name, &catalog))` passes a `&String` field as `&str`.
        if let (Ty::Tuple(to), Ty::Tuple(from)) = (&target, &source)
            && to.len() == from.len()
            && let Some(fields) = self.tuple_fields.get(&value.span).cloned()
            && fields.len() == to.len()
        {
            let tuple = self.temporary("tuple");
            let mut changed = false;
            let mut members = Vec::with_capacity(to.len());
            for (index, ((to, from), field)) in to.iter().zip(from.iter()).zip(fields).enumerate() {
                let read = format!("{tuple}[{index}]");
                let mut member =
                    self.operator_expr(origin::Fragment::plain(read.as_str()), from.clone(), field);
                self.coerce_expr(to, &mut member)?;
                let rendered = member.rendered_source();
                changed |= rendered.as_str() != read;
                members.push(rendered);
            }
            if changed {
                let mut rendered = origin::Fragment::plain(format!("{{ let {tuple} = "));
                rendered.push(value.rendered_source());
                rendered.push_text("; ");
                rendered.push(origin::Fragment::join(members, ", ").surrounded("[", "]"));
                rendered.push_text(" }");
                value.prelude = origin::Fragment::default();
                value.source = rendered;
                value.lvalue = None;
                value.source.cover(value.span);
            }
            value.ty = target;
            return Ok(None);
        }
        // A crate function where a `fn` pointer is expected, as the array
        // `[primary as fn(&Theme, _) -> _, secondary]` holds two: the
        // closure that calls it, a function value at runtime.
        #[cfg(feature = "rust-import")]
        if matches!(target, Ty::Fn { .. })
            && let Ty::ImportedFunctionItem {
                item: Some(proof), ..
            } = &source
        {
            let at = value.span;
            *value = self.host_function_value(proof.item.clone(), at)?;
            value.span = at;
            source = self.inference.resolve(&value.ty);
        }
        // `&F` where `F: Fn` is itself `Fn`: a borrowed closure passed as a
        // function, as `.map(self.on_change.as_ref())`.
        if matches!(target, Ty::Fn { .. })
            && let Ty::Reference {
                target: function, ..
            } = &source
            && matches!(
                self.inference.resolve(function),
                Ty::Fn { .. } | Ty::FnItem { .. }
            )
        {
            value.ty = self.inference.resolve(function);
            source = value.ty.clone();
        }
        // A crate type where a facade type is expected, through the crate's
        // `impl From<T> for` it: a facade's `impl Into<Color>` parameter.
        if let (Ty::Host { .. }, Ty::Script(..)) = (&target, &source)
            && let Some(id) = owner_id(&target)
            && self.models.conversions.contains_key(&id)
            && let Some((conversion, ..)) =
                self.select_conversion(id, Some(&target), &source, value.span)?
        {
            value.source =
                origin::Fragment::call(method_name(conversion.id()), [value.rendered_source()]);
            value.prelude = origin::Fragment::default();
            value.ty = target.clone();
            value.lvalue = None;
            value.source.cover(value.span);
            source = target.clone();
        }
        // A function item taking its place in a `fn` pointer choice.
        if let (Ty::FnChoice { keys, .. }, Ty::FnItem { key, .. }) = (&target, &source)
            && let Some(index) = keys.iter().position(|candidate| candidate == &**key)
        {
            let mut rendered = origin::Fragment::plain(format!("[{index}, "));
            rendered.push(value.rendered_source());
            rendered.push_text("]");
            value.prelude = origin::Fragment::default();
            value.source = rendered;
            value.ty = target.clone();
            value.lvalue = None;
            value.source.cover(value.span);
            return Ok(None);
        }
        // A variant constructor (`Shape::Circle`) used as a Rust function.
        if matches!(target, Ty::Fn { .. })
            && let Some(function) = self.constructor_function(&source, value.span)
        {
            value.prelude = origin::Fragment::default();
            value.source = function.source;
            value.ty = function.ty;
            value.lvalue = None;
            source = value.ty.clone();
        }
        // A `fn` pointer choice used as a plain `fn` pointer.
        if let (Ty::Fn { .. }, Ty::FnChoice { inputs, output, .. }) = (&target, &source) {
            let mut rendered = value.rendered_source();
            rendered.push_text("[1]");
            value.prelude = origin::Fragment::default();
            value.source = rendered;
            value.ty = Ty::Fn {
                inputs: inputs.clone(),
                output: output.clone(),
            };
            value.lvalue = None;
            value.source.cover(value.span);
            source = value.ty.clone();
        }
        // Unsized coercion: `&[T; N]` where `&[T]` is expected, or where a
        // host's slice, which crosses as a `Vec`, is.
        if let (
            Ty::Reference { target: to, .. },
            Ty::Reference {
                mutable,
                target: from,
            },
        ) = (&target, &source)
            && let (
                Ty::Array {
                    kind: kind @ (sequence::Kind::Slice | sequence::Kind::Vec),
                    ..
                },
                Ty::Array {
                    element,
                    kind: sequence::Kind::Array(_),
                },
            ) = (self.inference.resolve(to), self.inference.resolve(from))
        {
            source = Ty::Reference {
                mutable: *mutable,
                target: Box::new(Ty::Array { element, kind }),
            };
            value.ty = source.clone();
        }
        // Deref coercion (D711 step 3), through std's `Deref` impls and the
        // program's: `&String` or `&Arc<String>` where `&str` is expected,
        // `&&T` where `&T` is (as `b"aeiou".contains(b)` takes a filter's
        // `&&u8`), `&Box<T>` where `&T` is, a program's `&Wrapper` where
        // `&Target` is. std's steps are the value itself at runtime; a
        // program's calls its `deref`.
        if let Some((ty, calls)) = self.deref_coercion(&target, &source) {
            source = ty.clone();
            self.apply_derefs(value, calls, ty)?;
        }
        // A reborrow: `&mut T` where `&T` is expected, as a `&mut Game`
        // parameter passed on to a function that only reads it. The
        // reference is the same value at runtime, so only the type changes.
        if let Ty::Reference {
            mutable: false,
            target: to,
        } = &target
            && !matches!(self.inference.resolve(to), Ty::Infer(_))
            && let Ty::Reference {
                mutable: true,
                target: from,
            } = &source
        {
            source = Ty::Reference {
                mutable: false,
                target: from.clone(),
            };
            value.ty = source.clone();
        }
        // `Err(Box::new(e))` or `Err("text".into())` where a `Result<T,
        // Box<dyn Error>>` is expected: its error boxed as below. The error
        // of an `Err(error)` that is no box is rustc's E0308 at the error.
        if let (Ty::Result(to_ok, to_error), Ty::Result(from_ok, from_error)) = (&target, &source)
            && self.boxed_std_error(to_error)
            && !self.boxed_std_error(from_error)
            && !matches!(self.inference.resolve(from_error), Ty::Infer(_) | Ty::Never)
        {
            if let Some(payload) = self.err_payloads.get(&value.span.start()).copied()
                && !self.text_into_at(payload.start())
                && let Some(error) = self.unboxed_error(to_error, from_error, payload)
            {
                return Err(error);
            }
            self.require_type(to_ok, from_ok, value.span)?;
            let result = self.temporary("result");
            let error = self.prelude_op(PreludeOp::UnwrapErr, [result.as_str().into()]);
            let Some(object) = self.error_object(from_error, error, true, value.span)? else {
                return Err(Error::MissingCapability {
                    owner: from_error.name(&self.models),
                    capability: "std::error::Error".into(),
                    operation: value.span,
                    derive: matches!(from_error.as_ref(), Ty::Script(..)),
                    required_by: None,
                });
            };
            let mut boxed = origin::Fragment::plain(format!("{{ let {result} = "));
            boxed.push(value.rendered_source());
            boxed.push_text("; if ");
            boxed.push(self.prelude_op(PreludeOp::IsOk, [result.as_str().into()]));
            boxed.push_text(format!(" {{ {result} }} else {{ "));
            boxed.push(self.prelude_op(PreludeOp::Err, [object]));
            boxed.push_text(" } }");
            *value = self.operator_expr(boxed, target, value.span);
            return Ok(None);
        }
        // A box of an error, or `.into()` of a `&str` or a `String`, where a
        // `Box<dyn Error>` is expected: boxed as std's `dyn Error`, as
        // `Box`'s unsizing and its `From` impls make one
        // (`error_object.rs`). Any other value is rustc's E0308.
        if self.boxed_std_error(&target)
            && !self.boxed_std_error(&source)
            && !matches!(source, Ty::Infer(_) | Ty::Never)
        {
            if !self.text_into_at(value.span.start())
                && let Some(error) = self.unboxed_error(&target, &source, value.span)
            {
                return Err(error);
            }
            let Some(object) =
                self.error_object(&source, value.rendered_source(), true, value.span)?
            else {
                return Err(Error::MissingCapability {
                    owner: source.name(&self.models),
                    capability: "std::error::Error".into(),
                    operation: value.span,
                    derive: matches!(source, Ty::Script(..)),
                    required_by: None,
                });
            };
            *value = self.operator_expr(object, target, value.span);
            return Ok(None);
        }
        // Unsized coercion: `&T` to `&dyn Trait` and `Box<T>` to
        // `Box<dyn Trait>` for a `T: Trait`; the value is unchanged.
        let unsizing = match (&target, &source) {
            (
                Ty::Reference {
                    mutable: to_mutable,
                    target: to,
                },
                Ty::Reference {
                    mutable: from_mutable,
                    target: from,
                },
            ) if !*to_mutable || *from_mutable => Some((to, from)),
            (Ty::Boxed(to), Ty::Boxed(from)) => Some((to, from)),
            (Ty::Counted(to_counter, to), Ty::Counted(from_counter, from))
                if to_counter == from_counter =>
            {
                Some((to, from))
            }
            _ => None,
        }
        .and_then(|(to, from)| {
            match (self.inference.resolve(to), self.inference.resolve(from)) {
                (Ty::Dyn(trait_name, bindings), from)
                    if !matches!(from, Ty::Dyn(..) | Ty::Infer(_) | Ty::Never) =>
                {
                    Some((trait_name, bindings, from))
                }
                // Trait upcasting: `&dyn Named` to `&dyn Shape` for a
                // supertrait `Shape` (Rust 1.86); the value is unchanged.
                (Ty::Dyn(trait_name, bindings), Ty::Dyn(sub, sub_bindings))
                    if supertraits(&self.models, &sub).contains(&trait_name) =>
                {
                    Some((trait_name, bindings, Ty::Dyn(sub, sub_bindings)))
                }
                _ => None,
            }
        });
        if let Some((trait_name, bindings, concrete)) = unsizing {
            // A box of the program's own type that implements a native
            // trait becomes that trait's object: the stand-in rawr's copy of
            // the crate declares for the type, which holds the value's
            // handle and calls the impl's functions through it (#1264).
            if type_ref::is_native_trait(&trait_name)
                && matches!(target, Ty::Boxed(_))
                && let Some(owner) = self.native_object_owner(&concrete, &trait_name)
            {
                let source = std::mem::take(&mut value.source);
                value.source = self.prelude_op(
                    PreludeOp::NativeObject,
                    [
                        source,
                        format!("{owner:?}").into(),
                        format!("{trait_name:?}").into(),
                    ],
                );
                value.source.cover(value.span);
                value.ty = target;
                value.lvalue = None;
                return Ok(None);
            }
            // Making a native trait's object of anything else, a borrow or
            // a counted pointer of the program's type, is not done yet.
            if type_ref::is_native_trait(&trait_name) {
                return Err(Error::UnsupportedNominalSyntax {
                    construct: format!(
                        "coercing `{}` to the native trait object `{}` is not supported yet",
                        concrete.name(&self.models),
                        Ty::Dyn(trait_name.clone(), bindings.clone()).name(&self.models)
                    )
                    .into(),
                    source: value.span,
                });
            }
            let positional = dyn_arguments(&bindings);
            // A capability's std trait (`dyn Debug` of an `i32` or a
            // derive) holds by the capability.
            let capable = positional.is_empty()
                && capability::Kind::from_derive(&trait_name)
                    .is_some_and(|kind| self.type_provides(&concrete, kind));
            if !capable && !self.type_implements_with(&concrete, &trait_name, &positional) {
                return Err(Error::MissingCapability {
                    owner: concrete.name(&self.models),
                    capability: trait_name,
                    operation: value.span,
                    derive: matches!(concrete, Ty::Script(..)),
                    required_by: None,
                });
            }
            // `dyn Shape<Output = i64>`: the type's `Output` is `i64`.
            for (name, bound) in bindings
                .iter()
                .filter(|(name, _)| name.parse::<usize>().is_err())
            {
                let projected =
                    Ty::Projection(Box::new(concrete.clone()), trait_name.clone(), name.clone());
                self.require_type(bound, &projected, value.span)?;
            }
            value.ty = target;
            return Ok(None);
        }
        if self.coerce_implemented_trait(&source, &target, value)? {
            return self.coerce_expr(&target, value);
        }
        if self.coerce_children(&source, &target, value)? {
            return Ok(None);
        }
        // A native value whose opaque type is a future, whatever bound
        // makes it one, as a program's own `impl Straw<..>` is through
        // `Core`, goes where a future is taken, as rustc passes it to
        // iced's `Task::future(impl Future<Output = T>)`: its `Output` is
        // the future's.
        #[cfg(feature = "rust-import")]
        if let Some(expected) = self.task_output(&target)
            && let Ty::Host { proof, arguments } = &source
            && let Some(given) = proof
                .definition
                .associated(
                    rawr_import::surface::identity::Definition::future().as_bytes(),
                    "Output",
                )
                .and_then(|position| arguments.get(position))
        {
            let given = given.clone();
            self.require_type(&expected, &given, value.span)?;
            return Ok(None);
        }
        let checkpoint = self.inference.checkpoint();
        if let Some((key, projected)) = self.selected_projection(&source, &target)
            && self.unify_type(&target, &projected)
        {
            let target = self.inference.resolve(&target);
            let helper = self.push_op(Op::Projection {
                operation: Box::new(ProjectionOperation {
                    key,
                    source,
                    target: target.clone(),
                    span: value.span,
                }),
            });
            if self.normalize_private_calls {
                let operand = self.materialize_in_place(value, &format!("{helper}_arg_0"));
                value.source = self.op_call(&helper, [operand]);
            } else {
                debug_assert!(value.prelude.is_empty());
                value.source = self.op_call(&helper, [std::mem::take(&mut value.source)]);
            }
            value.ty = target;
            value.lvalue = None;
            value.source.cover(value.span);
            self.inference.commit(checkpoint);
            return Ok(Some(helper));
        }
        self.inference.rollback(checkpoint);
        self.require_type(&target, &source, value.span)?;
        Ok(None)
    }

    /// iced's `impl IntoIterator<Item = Element>`, a facade slice of
    /// Elements: an array or an iterator of Elements is taken as the slice,
    /// as `column([a, b])` takes `[Element; 2]`.
    pub(super) fn coerce_children(
        &mut self,
        source: &Ty,
        target: &Ty,
        value: &mut ParsedExpr,
    ) -> Result<bool, Error> {
        let Ty::Array {
            element: target_element,
            kind: sequence::Kind::Vec,
        } = target
        else {
            return Ok(false);
        };
        let target_element = self.inference.resolve(target_element);
        let source_element = match source {
            Ty::Array { element, .. } | Ty::Iter(element, _) => self.inference.resolve(element),
            _ => return Ok(false),
        };
        let fixed = matches!(
            source,
            Ty::Array {
                kind: sequence::Kind::Array(_),
                ..
            }
        );
        let iterator = matches!(source, Ty::Iter(..));
        // iced's `keyed_column(pairs)`: an iterator of `(key, Element)`
        // pairs, a facade slice of tuples that hold a widget.
        if iterator
            && let Ty::Tuple(members) = &target_element
            && members
                .iter()
                .any(|member| matches!(self.inference.resolve(member), Ty::Host { .. }))
        {
            let checkpoint = self.inference.checkpoint();
            if !self.unify_type(&target_element, &source_element) {
                self.inference.rollback(checkpoint);
                return Ok(false);
            }
            self.inference.commit(checkpoint);
            self.collect_children(value);
            value.ty = target.clone();
            return Ok(true);
        }
        if !matches!(target_element, Ty::Host { .. })
            || !matches!(source_element, Ty::Host { .. } | Ty::Script(..))
        {
            return Ok(false);
        }
        let same = {
            let checkpoint = self.inference.checkpoint();
            let same = match (&target_element, &source_element) {
                (
                    Ty::Host {
                        proof: expected, ..
                    },
                    Ty::Host { proof: actual, .. },
                ) => same_host_definition(
                    &expected.definition,
                    &expected.owner,
                    &actual.definition,
                    &actual.owner,
                ),
                _ => false,
            } && self.unify_type(&target_element, &source_element);
            if same {
                self.inference.commit(checkpoint);
            } else {
                self.inference.rollback(checkpoint);
            }
            same
        };
        // Items of another type are `Item = Element` unmet (E0271), as
        // rustc holds `column(texts)`: no coercion converts each one.
        if !same || (!fixed && !iterator) {
            return Ok(false);
        }
        // An array or an iterator of Elements already: only its type widens
        // to the slice.
        if iterator {
            self.collect_children(value);
        }
        value.ty = target.clone();
        Ok(true)
    }

    /// An iterator passed where the facade takes a slice: the array of its
    /// items.
    pub(super) fn collect_children(&mut self, value: &mut ParsedExpr) {
        if let Some((items, _)) = self.collected_items(value) {
            value.prelude = origin::Fragment::default();
            value.source = items;
        }
    }

    /// Arms naming different functions of one signature meet at a `fn`
    /// pointer, as in Rust; the pointer keeps which function it is.
    pub(super) fn fn_choice(&mut self, sources: &[Ty]) -> Option<Ty> {
        let Some(Ty::FnItem { inputs, output, .. }) = sources.first() else {
            return None;
        };
        let mut keys = Vec::new();
        for source in sources {
            let Ty::FnItem { key, .. } = source else {
                return None;
            };
            // A closure that captures is chosen as its value, not its item.
            if self
                .lifted
                .iter()
                .any(|closure| closure.key == **key && !closure.captures.is_empty())
            {
                return None;
            }
            if !keys.contains(&**key) {
                keys.push((**key).clone());
            }
        }
        if keys.len() < 2 {
            return None;
        }
        let pointer = Ty::Fn {
            inputs: inputs.clone(),
            output: output.clone(),
        };
        let checkpoint = self.inference.checkpoint();
        if !sources
            .iter()
            .all(|source| self.unify_type(&pointer, source))
        {
            self.inference.rollback(checkpoint);
            return None;
        }
        self.inference.commit(checkpoint);
        let Ty::Fn { inputs, output } = self.inference.resolve(&pointer) else {
            return None;
        };
        Some(Ty::FnChoice {
            keys: Rc::new(keys),
            inputs,
            output,
        })
    }

    pub(super) fn coerce_branches(
        &mut self,
        branches: &mut [&mut ParsedExpr],
    ) -> Result<Ty, Error> {
        let sources = branches
            .iter()
            .map(|branch| self.inference.resolve(&branch.ty))
            .filter(|ty| *ty != Ty::Never)
            .collect::<Vec<_>>();
        let Some(first) = sources.first().cloned() else {
            return Ok(Ty::Never);
        };
        if let Some(target) = self.fn_choice(&sources) {
            for branch in branches {
                self.coerce_expr(&target, branch)?;
            }
            return Ok(target);
        }

        // Rust gives every arm one type; a projection is chosen only when the
        // arms cannot unify exactly.
        let checkpoint = self.inference.checkpoint();
        let exact = sources.iter().all(|source| self.unify_type(&first, source));
        if exact {
            self.inference.commit(checkpoint);
        } else {
            self.inference.rollback(checkpoint);
        }
        let target = if exact {
            first
        } else {
            let candidates = sources
                .iter()
                .cloned()
                .chain(
                    sources
                        .iter()
                        .flat_map(|source| self.projected_host_types(source)),
                )
                .chain(sources.iter().filter_map(|source| match source {
                    Ty::FnItem { inputs, output, .. } => Some(Ty::Fn {
                        inputs: inputs.clone(),
                        output: output.clone(),
                    }),
                    _ => None,
                }))
                .collect::<Vec<_>>();
            let mut selected = None::<(Ty, Patch)>;
            for candidate in candidates {
                let checkpoint = self.inference.checkpoint();
                if !sources
                    .iter()
                    .all(|source| self.unify_type(&candidate, source))
                {
                    self.inference.rollback(checkpoint);
                    continue;
                }
                let candidate = self.inference.resolve(&candidate);
                if selected.as_ref().is_some_and(|(current, _)| {
                    !same_head(current, &candidate) || !same_inferred(current, &candidate)
                }) {
                    self.inference.rollback(checkpoint);
                    return Err(Error::UnsupportedNominalSyntax {
                        construct:
                            "control-flow branches have more than one common host projection".into(),
                        source: branches
                            .iter()
                            .find(|branch| self.inference.resolve(&branch.ty) != first)
                            .map_or(Span::NONE, |branch| branch.span),
                    });
                }
                if selected.is_none() {
                    selected = Some((candidate, self.inference.patch(checkpoint)));
                }
                self.inference.rollback(checkpoint);
            }
            let Some((target, patch)) = selected else {
                let (actual, expression) = branches
                    .iter()
                    .find(|branch| {
                        let source = self.inference.resolve(&branch.ty);
                        source != Ty::Never && source != first
                    })
                    .map_or((Ty::Unit, Span::NONE), |branch| {
                        (self.inference.resolve(&branch.ty), branch.span)
                    });
                return Err(Error::TypeMismatch {
                    expected: first.name(&self.models),
                    actual: actual.name(&self.models),
                    expression,
                });
            };
            self.inference.apply(patch);
            target
        };

        for branch in branches {
            self.coerce_expr(&target, branch)?;
        }
        Ok(self.inference.resolve(&target))
    }

    /// The type a widget erases to (iced's `Element`), when `source` is a
    /// widget.
    pub(super) fn widget_erasure(&mut self, source: &Ty) -> Option<Ty> {
        match self.projected_host_types(source).into_iter().next() {
            None if self.erased_widget(source) => self.erasure_types().into_iter().next(),
            target => target,
        }
    }

    /// A widget's `.into()` or `Element::from(widget)`, which rustc rejects
    /// (E0277) and Rawr rejects with the `.boxed()` fix.
    pub(super) fn no_widget_conversion(
        &self,
        source: &Ty,
        target: &Ty,
        method: &str,
        call: Span,
    ) -> Error {
        let (source, target) = (source.name(&self.models), target.name(&self.models));
        Error::InvalidMethodCall {
            owner: if method == "from" {
                target.clone()
            } else {
                source.clone()
            },
            method: method.into(),
            reason: format!(
                "the trait bound `{target}: From<{source}>` is not satisfied; pass the widget \
                 as it is, or call `.boxed()`"
            )
            .into(),
            call,
        }
    }

    /// The types facade traits erase to (iced's `Element`), each with fresh
    /// arguments. A trait script types implement (`implemented_trait`)
    /// erases nothing.
    pub(super) fn erasure_types(&mut self) -> Vec<Ty> {
        let traits = self
            .models
            .host
            .trait_types()
            .filter(|(definition, _)| {
                !matches!(definition.shape(), crate::host::type_def::Shape::Trait(_))
            })
            .map(|(definition, owner)| (definition.clone(), owner.clone()))
            .collect::<Vec<_>>();
        let mut erased = Vec::<Ty>::new();
        for (definition, owner) in traits {
            let arguments = definition
                .parameters()
                .iter()
                .map(|_| self.inference.fresh())
                .collect();
            let source = Ty::Host {
                proof: HostTyProof::new(definition, None, Some(owner)),
                arguments,
            };
            for target in self.projected_host_types(&source) {
                if !erased.iter().any(|known| same_head(known, &target)) {
                    erased.push(target);
                }
            }
        }
        erased
    }

    /// A script type implementing a facade trait it may implement (iced's
    /// `component::Component`) where its projection is expected: `value`
    /// becomes that projection's `new` call, which the caller coerces.
    #[inline(never)]
    pub(super) fn coerce_implemented_trait(
        &mut self,
        source: &Ty,
        target: &Ty,
        value: &mut ParsedExpr,
    ) -> Result<bool, Error> {
        // `canvas(self)` with `self: &Clock`: the program's value.
        let source = &match self.inference.resolve(source) {
            Ty::Reference { target, .. } if matches!(*target, Ty::Script(..)) => *target,
            source => source,
        };
        if let Ty::Script(id, _) = source
            && let Ty::Host { proof, .. } = target
            && let Some((trait_name, path)) = self.implemented_trait(*id, &proof.definition.path())
            && let Some(items) = self.models.host_items(&path).map(<[_]>::to_vec)
        {
            let call = value.span;
            let owner = self.models.definition(*id).definition().clone();
            let (_, declared) = self.models.traits[&trait_name].clone();
            let mut arguments = Vec::new();
            // `impl<Message> canvas::Program<Message> for C`: the impl's
            // parameters are fresh, shared by its methods.
            let mut substitution = None::<BTreeMap<ImmutableString, Ty>>;
            for function in declared.functions.iter() {
                let Some(method) = self
                    .models
                    .trait_methods
                    .get(&(*id, function.name.text.clone()))
                    .and_then(|methods| {
                        methods
                            .iter()
                            .find(|method| method.trait_name() == Some(&trait_name))
                    })
                    .cloned()
                else {
                    return Err(Error::MissingCapability {
                        owner: owner.name().into(),
                        capability: trait_name,
                        operation: call,
                        derive: true,
                        required_by: None,
                    });
                };
                let item = crate::item::Value::new(
                    method.key().clone(),
                    crate::item::Kind::Method,
                    Some(method.owner_identity().clone()),
                    method.receiver(),
                    method.fingerprint().clone(),
                    owner.capabilities().fingerprint(),
                );
                let substitution = match &substitution {
                    Some(substitution) => substitution.clone(),
                    None => {
                        let fresh = self.method_substitution(&method, None, call)?;
                        substitution = Some(fresh.clone());
                        fresh
                    }
                };
                let inputs = std::iter::once(source.clone())
                    .chain(method.parameters().iter().map(|parameter| {
                        let ty = instantiate(&Ty::from_resolved(&parameter.ty), &substitution);
                        match self.inference.resolve(&ty) {
                            Ty::Reference { target, .. } => *target,
                            ty => ty,
                        }
                    }))
                    .collect();
                let output = self.inference.resolve(&instantiate(
                    &Ty::from_resolved(method.output()),
                    &substitution,
                ));
                arguments.push(self.symbolic_expr(item, inputs, output, call, Vec::new()));
            }
            for (name, (bounds, _)) in declared
                .associated
                .iter()
                .zip(declared.associated_bounds.iter())
            {
                if !bounds.contains(capability::Kind::Default) {
                    continue;
                }
                let ty = self.inference.resolve(&Ty::Projection(
                    Box::new(source.clone()),
                    trait_name.clone(),
                    name.text.clone(),
                ));
                let default = self.default_source(&ty, call)?;
                arguments.push(self.operator_expr(default, ty, call));
            }
            // A projection that takes one more argument is told which
            // methods the impl writes itself, so it runs the trait's
            // default natively for the rest (bit `n` for function `n`).
            let takes = items
                .first()
                .map_or(0, |item| item.signature().parameters().len());
            if takes == arguments.len() + 2 {
                let written = declared
                    .functions
                    .iter()
                    .enumerate()
                    .filter(|(_, function)| {
                        self.models
                            .trait_methods
                            .get(&(*id, function.name.text.clone()))
                            .is_some_and(|methods| {
                                methods.iter().any(|method| {
                                    method.trait_name() == Some(&trait_name)
                                        && method.trait_default().is_none()
                                })
                            })
                    })
                    .fold(0_i64, |mask, (index, _)| mask | (1 << index));
                let ty = Ty::Int(Int::Var(self.inference.fresh_id()));
                arguments.push(self.operator_expr(
                    origin::Fragment::plain(written.to_string()),
                    ty,
                    call,
                ));
            }
            let placeholder = self.operator_expr(origin::Fragment::default(), Ty::Unit, call);
            let mut program = std::mem::replace(value, placeholder);
            // A reference is its value at runtime.
            program.ty = source.clone();
            arguments.insert(0, program);
            let owner = self.models.item_owner(&path).cloned();
            *value = self.call_host_function(&items, owner, arguments, call)?;
            return Ok(true);
        }
        Ok(false)
    }

    /// The facade trait script types may implement whose projection is
    /// `target`, when the script type `id` implements it: its name and its
    /// projection's `new`.
    pub(super) fn implemented_trait(
        &self,
        id: type_def::Id,
        target: &str,
    ) -> Option<(ImmutableString, String)> {
        self.models.host.trait_types().find_map(|(definition, _)| {
            if !matches!(definition.shape(), crate::host::type_def::Shape::Trait(_)) {
                return None;
            }
            // The trait's projection, or the type it projects to when that
            // type projects to `target` in turn: iced's `advanced::Widget`
            // makes an `Instance`, which is an `Element`.
            let projected = definition.projections().iter().find_map(|projection| {
                let projected = projection.target();
                (projected == target
                    || self.models.host.type_(&projected).is_some_and(|instance| {
                        instance
                            .projections()
                            .iter()
                            .any(|projection| projection.target() == target)
                    }))
                .then_some(projected)
            })?;
            let path = definition.path();
            let name: ImmutableString = path.rsplit("::").next()?.into();
            self.models
                .impls
                .contains(&(name.clone(), id))
                .then(|| (name, format!("{projected}::new")))
        })
    }

    /// A facade trait's implementor has the methods of the type the trait
    /// erases to, as iced's `Widget` gives every widget `map` and `boxed`:
    /// that type, when `source` converts into it and it has `member`.
    pub(super) fn erasure_with(&mut self, source: &Ty, member: &str) -> Option<Ty> {
        let erased = self.erasure_types();
        let candidates = if self.erased_widget(source) {
            erased
        } else {
            self.projected_host_types(source)
                .into_iter()
                .filter(|target| erased.iter().any(|known| same_head(known, target)))
                .collect()
        };
        candidates.into_iter().find(|target| {
            target.host().is_some_and(|definition| {
                self.models
                    .host_items(&format!("{}::{member}", definition.path()))
                    .is_some()
            })
        })
    }

    /// A widget that converts into an erased type by its widget protocol
    /// rather than a facade projection: an imported native widget.
    pub(super) fn erased_widget(&self, source: &Ty) -> bool {
        match source {
            #[cfg(feature = "rust-import")]
            Ty::Host { arguments, .. } => matches!(arguments.as_ref(), [Ty::ProfileMessage { .. }]),
            _ => false,
        }
    }

    pub(super) fn projected_host_types(&mut self, source: &Ty) -> Vec<Ty> {
        let Ty::Host { proof, arguments } = source else {
            return Vec::new();
        };
        if proof.owner.is_none() {
            return Vec::new();
        }
        let definition = &proof.definition;
        let substitutions = definition
            .parameters()
            .iter()
            .zip(arguments.iter())
            .map(|(name, argument)| (name.as_str(), argument.clone()))
            .collect::<BTreeMap<_, _>>();

        let projected = definition
            .projections()
            .iter()
            .filter_map(|projection| {
                let target_path = projection.target();
                let target = self.models.host.type_(&target_path)?.clone();
                let owner = self.models.host.type_owner(&target_path)?.clone();
                let arguments = projection
                    .arguments()
                    .iter()
                    .map(|argument| match argument {
                        crate::host::type_def::Argument::Source(name) => {
                            substitutions.get(name.as_str()).cloned()
                        }
                        crate::host::type_def::Argument::Fresh => Some(self.inference.fresh()),
                    })
                    .collect::<Option<Box<[_]>>>()?;
                let alias = target.identity().imported().map(|_| target.clone());
                Some(Ty::Host {
                    proof: HostTyProof::new(target, alias, Some(owner)),
                    arguments,
                })
            })
            .collect::<Vec<_>>();
        projected
            .iter()
            .map(|target| self.inference.resolve(target))
            .collect()
    }

    pub(super) fn coerce_tail(&mut self, expected: &Ty, body: Span) -> Result<(), Error> {
        let Some((start, span)) = self.last_expression else {
            return self.require_type(expected, &self.last_type.clone(), body);
        };
        let mut value = self.take_tail(start, span);
        if value.lvalue.is_some() {
            self.move_local(&mut value)?;
        }
        value.lvalue = None;
        let result = self.coerce_expr(expected, &mut value);
        result?;
        self.returned(value.span, &value.ty);
        self.write_output(value.rendered_source(), value.span);
        self.last_type = value.ty;
        Ok(())
    }

    /// The tail written from `start`, taken out of the output. A local the
    /// body ends with is its lvalue: it moves out to the caller, as in Rust,
    /// rather than being copied as the body's locals end.
    pub(super) fn take_tail(&mut self, start: usize, span: Span) -> ParsedExpr {
        let mut source = self.output.split_off(start);
        source.trim_end();
        let lvalue = variable_name(source.as_str())
            .filter(|name| {
                self.bindings
                    .get(name.as_str())
                    .is_some_and(|binding| matches!(binding.kind, BindingKind::Local))
            })
            .map(|binding| Lvalue::Binding {
                binding,
                ty: self.last_type.clone(),
            });
        ParsedExpr {
            prelude: origin::Fragment::default(),
            source,
            ty: self.last_type.clone(),
            span,
            lvalue,
            outcome: crate::host::usage::Outcome::Conservative,
            evaluation: crate::host::usage::Evaluation::Required,
        }
    }
}
