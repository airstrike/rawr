//! A crate item's signature at a call: the form that fits the arguments,
//! its type parameters fixed by the arguments, bounds and impls, and the
//! crate types it names instantiated.

use super::*;

impl<'a> Parser<'a> {
    pub(super) fn select_host_signature(
        &mut self,
        items: &[crate::host::item::Definition],
        receiver: Option<&Ty>,
        arguments: &mut [ParsedExpr],
        repeated: bool,
        call: Span,
    ) -> Result<(crate::host::item::Definition, InstantiatedHostSignature), Error> {
        let (item, signature) =
            self.select_host_signature_form(items, receiver, arguments, repeated, call)?;
        // A number where the library takes `impl Into<T>` of its own type
        // `T`, through that type's `from` of it (`step(0.01)` on a slider
        // over `Radians`).
        if !repeated && item.imported_identity().is_none() {
            for (index, (expected, argument)) in signature
                .inputs
                .iter()
                .zip(arguments.iter_mut())
                .enumerate()
            {
                if let Some(path) = self.number_into_parameter(&item, index, expected, &argument.ty)
                {
                    let from = self
                        .models
                        .host_items(&path)
                        .map(<[_]>::to_vec)
                        .unwrap_or_default();
                    let owner = self.models.item_owner(&path).cloned();
                    let span = argument.span;
                    *argument =
                        self.call_host_function(&from, owner, vec![argument.clone()], span)?;
                }
            }
        }
        Ok((item, signature))
    }

    /// `T::from`'s path when the parameter at `index` of the library `item`
    /// is its type parameter, which takes `expected`, a library type `T`
    /// whose `from` takes the number `argument`: iced's `impl Into<T>`.
    pub(super) fn number_into_parameter(
        &self,
        item: &crate::host::item::Definition,
        index: usize,
        expected: &Ty,
        argument: &Ty,
    ) -> Option<String> {
        let parameter = item.signature().parameters().get(index)?;
        if !matches!(
            parameter.shape(),
            crate::host::type_ref::Shape::Parameter(_)
        ) {
            return None;
        }
        let argument = self.inference.resolve(argument);
        if !matches!(argument, Ty::Int(_) | Ty::Float(_)) {
            return None;
        }
        let Ty::Host { proof, .. } = self.inference.resolve(expected) else {
            return None;
        };
        let path = format!("{}::from", proof.definition.path());
        self.models
            .host_items(&path)?
            .iter()
            .any(|from| {
                from.kind() == crate::host::item::Kind::AssociatedFunction
                    && matches!(from.signature().parameters(), [parameter]
                        if host_number_accepts(parameter, &argument))
            })
            .then_some(path)
    }

    /// Whether `argument`, as written, fits the callable parameter at
    /// `index` of the one item, with the receiver's types. Nothing is bound.
    pub(super) fn callable_fits(
        &mut self,
        items: &[crate::host::item::Definition],
        receiver: Option<&Ty>,
        index: usize,
        argument: &ParsedExpr,
    ) -> bool {
        let [item] = items else {
            return false;
        };
        let checkpoint = self.inference.checkpoint();
        let fits = self
            .instantiate_host_signature(item, receiver, None, Span::NONE)
            .is_ok_and(|signature| {
                signature
                    .inputs
                    .get(index)
                    .is_some_and(|expected| self.unify_type(expected, &argument.ty))
            });
        self.inference.rollback(checkpoint);
        fits
    }

    /// `argument` where an imported item takes a type parameter at its input
    /// `index`. An adaptor chain is the native iterator that pulls from it
    /// (`step_iterator`). So is a range or a script iterator where the
    /// parameter is bounded by `Iterator`, and an array or a collection where
    /// the item names the parameter's `IntoIter`, as `Product<Self,
    /// J::IntoIter>` does. An array where the bound is `Iterator` is rustc's
    /// E0277. Any other value, as a crate's own iterator, fills it as itself.
    #[cfg(feature = "rust-import")]
    pub(super) fn iterator_argument(
        &mut self,
        items: &[crate::host::item::Definition],
        index: usize,
        argument: &mut ParsedExpr,
    ) -> Result<(), Error> {
        let mut iterator = false;
        let mut into_iter = false;
        let mut taken = false;
        for item in items
            .iter()
            .filter(|item| item.imported_identity().is_some())
        {
            let signature = item.signature();
            let Some(crate::host::type_ref::Shape::Parameter(name)) = signature
                .parameters()
                .get(index)
                .map(crate::host::type_ref::Type::shape)
            else {
                continue;
            };
            let name = name.to_string();
            taken = true;
            iterator |= signature
                .requirements(&name)
                .is_some_and(|set| set.contains(capability::Kind::Iterator));
            into_iter |= signature.relations().iter().any(|relation| {
                relation.parameter() == name
                    && relation
                        .associated()
                        .is_some_and(|(_, associated)| associated.as_str() == "IntoIter")
            });
        }
        if !taken {
            return Ok(());
        }
        let ty = self.inference.resolve(&argument.ty);
        let steps = match &ty {
            Ty::Iter(..) => true,
            Ty::InclusiveRange(_) | Ty::Script(..) => iterator && self.program_iterator(&ty),
            Ty::Array { .. } | Ty::Keyed(..) | Ty::Reference { .. } if iterator => {
                return Err(Error::MissingCapability {
                    owner: ty.name(&self.models),
                    capability: "Iterator".into(),
                    operation: argument.span,
                    derive: false,
                    required_by: None,
                });
            }
            Ty::Array { .. } | Ty::Keyed(..) | Ty::Reference { .. } => into_iter,
            _ => false,
        };
        if steps {
            self.step_iterator(argument)?;
        }
        Ok(())
    }

    /// Whether an item's parameter at `index` is a type parameter that a
    /// bound fills with a `Range<T>`, as rand's `random_range` has
    /// `R: SampleRange<T>` implemented for `Range<T>`.
    pub(super) fn takes_range(
        &self,
        items: &[crate::host::item::Definition],
        index: usize,
    ) -> bool {
        items.iter().any(|item| {
            let signature = item.signature();
            let name = match signature
                .parameters()
                .get(index)
                .map(crate::host::type_ref::Type::shape)
            {
                // A `Range<usize>` the item takes itself (#707).
                Some(crate::host::type_ref::Shape::Range(_)) => return true,
                Some(crate::host::type_ref::Shape::Parameter(name)) => name,
                _ => return false,
            };
            signature.relations().iter().any(|relation| {
                relation.parameter() == name.to_string()
                    && relation.cases().iter().any(|case| {
                        matches!(case.this(), crate::host::signature::Pattern::Range(_))
                    })
            })
        })
    }

    pub(super) fn select_host_signature_form(
        &mut self,
        items: &[crate::host::item::Definition],
        receiver: Option<&Ty>,
        arguments: &mut [ParsedExpr],
        repeated: bool,
        call: Span,
    ) -> Result<(crate::host::item::Definition, InstantiatedHostSignature), Error> {
        // A call takes the functions of a path that also holds a macro
        // (`text` and `text!`).
        let functions;
        let items = if !repeated && items.iter().any(|item| !item.kind().is_macro()) {
            functions = items
                .iter()
                .filter(|item| !item.kind().is_macro())
                .cloned()
                .collect::<Vec<_>>();
            &functions[..]
        } else {
            items
        };
        // A named function passed to the host is its symbolic callable.
        // One taking a typed number, as `Message::Changed(u8)` for a
        // slider's `i64`, takes it as its runtime kind, as the facade's
        // number inputs do (`deref_facade_strings`).
        let facade = items.iter().all(|item| item.imported_identity().is_none());
        // A callable whose input is the item's type parameter, as
        // `Task::map`'s, keeps the function's own input type when the
        // receiver's type takes it, as a `Task<u32>` does. A facade's own
        // `Task<f64>` stands for iced's `f32`, so it takes the runtime kind.
        let generic_input = |index: usize| {
            items.iter().any(|item| {
                item.signature().parameters().get(index).is_some_and(|parameter| {
                    matches!(
                        parameter.shape(),
                        crate::host::type_ref::Shape::Callable { input, .. }
                            if matches!(input.shape(), crate::host::type_ref::Shape::Parameter(_))
                    )
                })
            })
        };
        for (index, argument) in arguments.iter_mut().enumerate() {
            // `0..n` or `'a'..'z'` where an imported item's type parameter
            // takes a `Range<T>` (rand's `R: SampleRange<T>`): the range
            // itself, not an iterator over it.
            if !facade
                && self.takes_range(items, index)
                && let Some((value, ty)) = self.range_values.get(argument.source.as_str()).cloned()
            {
                argument.source = value;
                argument.prelude = origin::Fragment::default();
                argument.ty = ty;
            }
            // A program's iterator where the crate takes a type parameter,
            // as itertools' `cartesian_product(self, other: J)` takes `Self:
            // Iterator` and `J: IntoIterator`: a native iterator that pulls
            // from it (`step_iterator`). A crate's own iterator fills it as
            // itself.
            #[cfg(feature = "rust-import")]
            self.iterator_argument(items, index, argument)?;
            self.host_callable_of(argument, !facade)?;
            self.option_host_callable(argument)?;
            // A `T: Into<Length>` passed where the library takes a `Length`:
            // the caller converted it (`convert_into_host`), so it is one.
            if facade
                && let ty @ (Ty::Param(_) | Ty::Projection(..)) =
                    self.inference.resolve(&argument.ty)
                && let Some(target) = self
                    .bound_trait_arguments(&ty, "Into")
                    .first()
                    .and_then(|arguments| arguments.first())
                    .map(|target| self.inference.resolve(target))
                && let Ty::Host { proof, .. } = &target
                && self
                    .models
                    .host_items(&format!("{}::__into", proof.definition.path()))
                    .is_some()
            {
                argument.ty = target;
            }
            if facade
                && !(generic_input(index) && self.callable_fits(items, receiver, index, argument))
                && let Ty::Callable {
                    inputs,
                    output,
                    item,
                } = self.inference.resolve(&argument.ty)
            {
                let inputs = inputs
                    .iter()
                    .map(|input| match input {
                        Ty::Float(Float::F32) => Ty::Float(Float::F64),
                        Ty::Int(kind) if !matches!(kind, Int::I64 | Int::Var(_)) => {
                            Ty::Int(Int::I64)
                        }
                        input => input.clone(),
                    })
                    .collect();
                argument.ty = Ty::Callable {
                    inputs,
                    output,
                    item,
                };
            }
        }
        let arguments = &*arguments;
        let first = items
            .first()
            .expect("a host item has at least one declaration");
        // A script function or closure fills an imported item's callable
        // parameter, not a type parameter another form of it takes a value
        // at: `.theme(App::theme)` takes the form of iced's `impl
        // ThemeFn<State, Theme>` read as `Fn(&State)`, and
        // `.theme(Theme::Dark)` the one that keeps the bound
        // (`callable::Item::values`).
        let callable_forms;
        let items = if items.len() > 1 && !facade {
            callable_forms = items
                .iter()
                .filter(|item| {
                    !arguments.iter().enumerate().any(|(index, argument)| {
                        matches!(
                            self.inference.resolve(&argument.ty),
                            Ty::Callable { .. }
                                | Ty::Fn { .. }
                                | Ty::FnItem { .. }
                                | Ty::FnChoice { .. }
                        ) && matches!(
                            item.signature()
                                .parameters()
                                .get(index)
                                .map(crate::host::type_ref::Type::shape),
                            Some(crate::host::type_ref::Shape::Parameter(_))
                        )
                    })
                })
                .cloned()
                .collect::<Vec<_>>();
            if callable_forms.is_empty() {
                items
            } else {
                &callable_forms[..]
            }
        } else {
            items
        };
        let first = items.first().unwrap_or(first);
        if items.len() == 1 {
            let signature =
                self.instantiate_host_signature(first, receiver, Some(arguments), call)?;
            return Ok((first.clone(), signature));
        }

        let mut matched = None;
        let mut matched_stringy = false;
        let mut match_count = 0;
        for item in items {
            let checkpoint = self.inference.checkpoint();
            let signature =
                match self.instantiate_host_signature(item, receiver, Some(arguments), call) {
                    Ok(signature) => signature,
                    Err(_) => {
                        self.inference.rollback(checkpoint);
                        continue;
                    }
                };
            let matches_inputs = if repeated {
                let [expected] = signature.inputs.as_slice() else {
                    self.inference.rollback(checkpoint);
                    continue;
                };
                arguments
                    .iter()
                    .all(|argument| self.unify_type(expected, &argument.ty))
            } else {
                signature.inputs.len() == arguments.len()
                    && signature.inputs.iter().zip(arguments).enumerate().all(
                        |(index, (expected, argument))| {
                            self.unify_type(expected, &argument.ty)
                                || facade
                                    && self
                                        .number_into_parameter(item, index, expected, &argument.ty)
                                        .is_some()
                        },
                    )
            };
            if matches_inputs && self.host_requirements_hold(&signature) {
                match_count += 1;
                // A form taking a string comes first for an argument whose
                // type is not known yet.
                let stringy = signature
                    .inputs
                    .iter()
                    .any(|input| self.inference.resolve(input).is_text());
                if matched.is_none() || stringy && !matched_stringy {
                    matched_stringy = stringy;
                    matched = Some((item.clone(), signature, self.inference.patch(checkpoint)));
                }
            }
            self.inference.rollback(checkpoint);
        }

        match match_count {
            0 => {
                // What each form takes, for the mismatch the error names.
                let mut forms = Vec::<String>::new();
                for item in items {
                    let checkpoint = self.inference.checkpoint();
                    if let Ok(signature) =
                        self.instantiate_host_signature(item, receiver, None, Span::NONE)
                        && signature.inputs.len() == arguments.len()
                    {
                        let form = signature
                            .inputs
                            .iter()
                            .map(|input| {
                                self.inference.resolve(input).name(&self.models).to_string()
                            })
                            .collect::<Vec<_>>()
                            .join(", ");
                        let form = format!("({form})");
                        if !forms.contains(&form) {
                            forms.push(form);
                        }
                    }
                    self.inference.rollback(checkpoint);
                }
                let takes = match forms.as_slice() {
                    [] => String::new(),
                    [form] => format!("; it takes {form}"),
                    forms => format!("; its forms take {}", forms.join(", ")),
                };
                Err(Error::InvalidHostOperation {
                    item: first.path().into(),
                    reason: format!(
                        "no form of it takes ({}){takes}",
                        arguments
                            .iter()
                            .map(|argument| self.inference.resolve(&argument.ty).name(&self.models))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                    .into(),
                    operation: call,
                })
            }
            1 => {
                let (item, signature, patch) =
                    matched.expect("the one form that matched is kept as the match");
                self.inference.apply(patch);
                Ok((item, signature))
            }
            // An argument whose type is not known yet (a closure's
            // parameter, which Rust infers from a later call) takes the
            // first form, as the later uses then must.
            _ if arguments
                .iter()
                .any(|argument| matches!(self.inference.resolve(&argument.ty), Ty::Infer(_)))
                && let Some((item, signature, patch)) = matched =>
            {
                self.inference.apply(patch);
                Ok((item, signature))
            }
            _ => Err(Error::InvalidHostOperation {
                item: first.path().into(),
                reason: format!(
                    "{} forms of it take these arguments; annotate a type to pick one",
                    match_count
                )
                .into(),
                operation: call,
            }),
        }
    }

    /// A facade function whose last input is `impl Fn(T) -> String`, called
    /// without it, has Rust's `T: Display` bound there (iced's
    /// `combo_box::State::new(options: Vec<T>)` labels its options by it): the
    /// compiler passes `T::to_string`, `T` the element of the first argument.
    pub(super) fn supply_display_labeller(
        &mut self,
        items: &[crate::host::item::Definition],
        arguments: &mut Vec<ParsedExpr>,
        call: Span,
    ) -> Result<(), Error> {
        use crate::host::type_ref::{Builtin, Shape};
        let labelled = |item: &crate::host::item::Definition| {
            let parameters = item.signature().parameters();
            parameters.len() == arguments.len() + 1
                && matches!(
                    parameters.last().map(|parameter| parameter.shape()),
                    Some(Shape::Callable { input, output })
                        if matches!(input.shape(), Shape::Parameter(_))
                            && matches!(output.shape(), Shape::Builtin(Builtin::String))
                )
        };
        if items.is_empty() || !items.iter().all(labelled) {
            return Ok(());
        }
        let first = arguments
            .first()
            .map(|first| self.inference.resolve(&first.ty));
        let element = match first {
            Some(Ty::Array { element, .. }) => *element,
            Some(Ty::Reference { target, .. }) => match *target {
                Ty::Array { element, .. } => *element,
                _ => return Ok(()),
            },
            _ => return Ok(()),
        };
        let input = Ty::Reference {
            mutable: false,
            target: Box::new(self.inference.resolve(&element)),
        };
        let labeller = self.method_value("to_string".into(), Box::new([input]), call, call)?;
        arguments.push(labeller);
        Ok(())
    }

    /// Rust's `&String` to `&str` deref coercion at a facade call: a facade
    /// `String` input stands for a `&str`, and a
    /// shared borrow is passed by value, and a typed number as its runtime kind.
    /// Imported Rust items keep exact types.
    pub(super) fn deref_facade_strings(
        &mut self,
        items: &[crate::host::item::Definition],
        arguments: &mut [ParsedExpr],
    ) -> Result<(), Error> {
        if items.iter().any(|item| item.imported_identity().is_some()) {
            return Ok(());
        }
        // The sequence a facade's parameter at `index` takes, other than
        // an array of a length.
        let sequence = |index: usize| {
            items.iter().find_map(
                |item| match item.signature().parameters().get(index)?.shape() {
                    crate::host::type_ref::Shape::Array {
                        sequence: crate::host::type_ref::Sequence::Vec,
                        ..
                    } => Some(sequence::Kind::Vec),
                    crate::host::type_ref::Shape::Array {
                        sequence: crate::host::type_ref::Sequence::Slice,
                        ..
                    } => Some(sequence::Kind::Slice),
                    _ => None,
                },
            )
        };
        // A number passed as an item's type parameter, as `Task::done`'s,
        // keeps its type, as a function passed to it does.
        let generic = |index: usize| {
            items.iter().any(|item| {
                item.signature()
                    .parameters()
                    .get(index)
                    .is_some_and(|parameter| {
                        matches!(
                            parameter.shape(),
                            crate::host::type_ref::Shape::Parameter(_)
                        )
                    })
            })
        };
        // A number std's own signature types, as `process::exit`'s `i32`:
        // the argument keeps its type.
        let typed = |index: usize| {
            items.iter().any(|item| {
                item.signature()
                    .parameters()
                    .get(index)
                    .is_some_and(|parameter| {
                        matches!(
                            parameter.shape(),
                            crate::host::type_ref::Shape::Builtin(builtin)
                                if builtin.name().is_some_and(|name| {
                                    numeric_type(name).is_some() && !matches!(name, "i64" | "f64")
                                })
                        )
                    })
            })
        };
        // A facade takes `&str`, slices and borrowed facade values, such as
        // iced's `&Theme`, as their owned types. Its `f64` and `i64` inputs stand
        // for iced's `f32` and `u16`/`u32` (`impl Into<Pixels>`, `Padding`):
        // a typed `f32` or integer, which is an `f64` or `i64` at runtime,
        // is passed as that.
        for (index, argument) in arguments.iter_mut().enumerate() {
            match self.inference.resolve(&argument.ty) {
                Ty::Reference {
                    mutable: false,
                    target,
                } if matches!(
                    *target,
                    Ty::String | Ty::Str | Ty::Host { .. } | Ty::Array { .. }
                ) =>
                {
                    argument.ty = match *target {
                        // `&[T; N]`, as `b"…"`, `&Vec<T>` or `&[T]` where
                        // the facade takes the sequence its elements copy
                        // to.
                        Ty::Array {
                            element,
                            kind:
                                sequence::Kind::Array(_) | sequence::Kind::Vec | sequence::Kind::Slice,
                        } if let Some(kind) = sequence(index) => Ty::Array { element, kind },
                        target => target,
                    };
                }
                // iced's `Option<&T>` (a combo box's selection) is an
                // `Option<T>` in the facade.
                Ty::Option(ref inner)
                    if let Ty::Reference {
                        mutable: false,
                        target,
                    } = &**inner =>
                {
                    argument.ty = Ty::Option(target.clone());
                }
                Ty::Float(Float::F32) if !generic(index) && !typed(index) => {
                    argument.ty = Ty::Float(Float::F64);
                }
                Ty::Int(kind)
                    if !matches!(kind, Int::I64 | Int::Var(_))
                        && !generic(index)
                        && !typed(index) =>
                {
                    argument.ty = Ty::Int(Int::I64);
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Whether a form's bounds hold of the types its arguments fixed. A type
    /// the arguments leave to inference doesn't rule the form out, as
    /// `run((), view)`'s `Message` isn't known until the crate's `impl
    /// UpdateFn<State, Never> for ()` fixes it, as it is in rustc.
    pub(super) fn host_requirements_hold(&self, signature: &InstantiatedHostSignature) -> bool {
        signature.requirements.iter().all(|(_, ty, required)| {
            matches!(self.inference.resolve(ty), Ty::Infer(_))
                || required
                    .iter()
                    .all(|capability| self.type_provides(ty, capability))
        })
    }

    pub(super) fn instantiate_host_signature(
        &mut self,
        item: &crate::host::item::Definition,
        receiver: Option<&Ty>,
        arguments: Option<&[ParsedExpr]>,
        operation: Span,
    ) -> Result<InstantiatedHostSignature, Error> {
        let mut parameters = BTreeMap::<ImmutableString, Ty>::new();
        if let Some(types) = self.host_turbofish.clone() {
            let names = item
                .signature()
                .declared_type_parameters()
                .collect::<Vec<_>>();
            if names.len() != types.len() {
                return Err(Error::InvalidTypeArguments {
                    ty: item.path().into(),
                    expected: names.len(),
                    actual: types.len(),
                    reference: operation,
                });
            }
            parameters.extend(names.into_iter().zip(types));
        }
        let resolved_receiver = receiver.map(|ty| self.inference.resolve(ty));
        let host_receiver = resolved_receiver.as_ref().map(|ty| match ty {
            Ty::Reference { target, .. } => target.as_ref(),
            ty => ty,
        });
        if let Some(Ty::Host { proof, arguments }) = host_receiver {
            let definition = match proof.definition.identity().imported() {
                Some(_) => proof.alias.as_ref().unwrap_or(&proof.definition),
                None => &proof.definition,
            };
            let owner = item
                .path()
                .rsplit_once("::")
                .map(|(owner, _)| owner.to_string())
                .unwrap_or_default();
            // Another alias of the same imported Rust type is the same
            // receiver.
            let same_import = definition.identity().imported().is_some_and(|identity| {
                self.models
                    .host
                    .type_(&owner)
                    .and_then(|aliased| aliased.identity().imported())
                    == Some(identity)
            });
            if definition.path() != owner && !same_import {
                return Err(Error::InvalidHostOperation {
                    item: item.path().into(),
                    reason: format!(
                        "receiver has type `{}`, expected `{owner}`",
                        definition.path()
                    )
                    .into(),
                    operation,
                });
            }
            // A method whose impl fixes its receiver's type names the impl's
            // parameters, not the type's: the instance gives them below.
            #[cfg(feature = "rust-import")]
            let fixed = item.signature().instance().is_some();
            #[cfg(not(feature = "rust-import"))]
            let fixed = false;
            if !fixed {
                parameters.extend(
                    definition
                        .parameters()
                        .iter()
                        .zip(arguments)
                        .map(|(name, argument)| (name.clone(), argument.clone())),
                );
            }
        }
        let item_path = item.path();
        let receiver_origin = host_receiver.and_then(Ty::host_alias).cloned();
        let argument_origins = arguments
            .unwrap_or_default()
            .iter()
            .map(|argument| self.inference.resolve(&argument.ty).host_alias().cloned())
            .collect::<Vec<_>>();
        let associated_origin = receiver_origin.clone().or_else(|| {
            item_path
                .rsplit_once("::")
                .and_then(|(path, _)| self.models.host.type_(path))
                .cloned()
        });
        let item_owner = associated_origin
            .as_ref()
            .and_then(|definition| self.models.host.type_owner(&definition.path()))
            .or_else(|| self.models.host.item_owner(&item_path))
            .cloned();
        // A method whose impl fixes its receiver's type, as `and_then` of
        // `impl<T> Task<Option<T>>`, takes only a receiver of that type, and
        // the receiver gives the impl's parameters inside it.
        #[cfg(feature = "rust-import")]
        if let (Some(instance), Some(actual)) = (item.signature().instance(), host_receiver) {
            let instance = self.instantiate_host_type(
                instance,
                &mut parameters,
                &item_path,
                operation,
                item_owner.as_ref(),
                associated_origin.as_ref(),
            )?;
            if !self.unify_type(&instance, actual) {
                return Err(Error::InvalidHostOperation {
                    item: item_path.as_str().into(),
                    reason: format!(
                        "receiver has type `{}`, expected `{}`",
                        self.inference.resolve(actual).name(&self.models),
                        self.inference.resolve(&instance).name(&self.models)
                    )
                    .into(),
                    operation,
                });
            }
        }
        // A `RangeInclusive<T>` whose argument is not known yet, as a
        // closure's parameter, and whose `T` the other arguments fix.
        let mut open_ranges = Vec::<(usize, Ty)>::new();
        let mut inputs =
            item.signature()
                .parameters()
                .iter()
                .enumerate()
                .map(|(index, ty)| {
                    let originating = argument_origins
                        .get(index)
                        .and_then(Option::as_ref)
                        .or(associated_origin.as_ref());
                    if let Some(element) = range_inclusive_element(ty)
                        && arguments
                            .and_then(|arguments| arguments.get(index))
                            .is_some_and(|argument| {
                                matches!(self.inference.resolve(&argument.ty), Ty::Infer(_))
                            })
                    {
                        let element = self.instantiate_host_type(
                            element,
                            &mut parameters,
                            &item_path,
                            operation,
                            item_owner.as_ref(),
                            originating,
                        )?;
                        if matches!(self.inference.resolve(&element), Ty::Infer(_)) {
                            open_ranges.push((index, element));
                            return Ok(Ty::Unknown);
                        }
                        return Ok(self.inclusive_range_of(&element, None));
                    }
                    // A crate's `RangeInclusive<T>` takes the argument's range,
                    // of floats or of integers, and `T` that kind of number, as
                    // rustc infers `T` from `1.0..=30.0`. A range of a crate's
                    // type, as iced's `Radians::RANGE`, makes `T` that type.
                    if let Some(element) = range_inclusive_element(ty)
                        && let Some(argument) = arguments
                            .and_then(|arguments| arguments.get(index))
                            .map(|argument| self.inference.resolve(&argument.ty))
                        && matches!(
                            argument,
                            Ty::FloatRange | Ty::InclusiveRange(_) | Ty::InclusiveOf(_)
                        )
                    {
                        let element = self.instantiate_host_type(
                            element,
                            &mut parameters,
                            &item_path,
                            operation,
                            item_owner.as_ref(),
                            originating,
                        )?;
                        return Ok(match argument {
                            Ty::InclusiveOf(bound) => {
                                let _ = self.unify_type(&element, &bound);
                                Ty::InclusiveOf(Box::new(element))
                            }
                            argument => self.inclusive_range_of(
                                &element,
                                Some(matches!(argument, Ty::FloatRange)),
                            ),
                        });
                    }
                    // A crate's `impl IntoIterator<Item = T>`, which Rawr takes
                    // as an array of `T`, takes a shared borrow of an array too,
                    // as `table(columns, &self.events)` passes `&Vec<Event>`,
                    // whose items the columns' `|event: &Event|` read.
                    #[cfg(feature = "rust-import")]
                    if let crate::host::type_ref::Shape::Array {
                        element,
                        sequence:
                            crate::host::type_ref::Sequence::Vec
                            | crate::host::type_ref::Sequence::Slice,
                    } = ty.shape()
                        && matches!(element.shape(), crate::host::type_ref::Shape::Parameter(_))
                        && item.imported_identity().is_some()
                        && arguments
                            .and_then(|arguments| arguments.get(index))
                            .is_some_and(|argument| {
                                matches!(
                                    self.inference.resolve(&argument.ty),
                                    Ty::Reference { mutable: false, target }
                                        if matches!(
                                            *target,
                                            Ty::Array {
                                                kind: sequence::Kind::Vec | sequence::Kind::Slice,
                                                ..
                                            }
                                        )
                                )
                            })
                    {
                        let item = self.instantiate_host_type(
                            element,
                            &mut parameters,
                            &item_path,
                            operation,
                            item_owner.as_ref(),
                            originating,
                        )?;
                        let borrowed = arguments
                            .and_then(|arguments| arguments.get(index))
                            .map(|argument| self.inference.resolve(&argument.ty))
                            .unwrap_or(Ty::Unknown);
                        // Its items are borrows of the array's elements; one
                        // that can't be `T` is the mismatch rustc reports.
                        if let Ty::Reference { target, .. } = &borrowed
                            && let Ty::Array { element, .. } = target.as_ref()
                            && self.unify_type(
                                &item,
                                &Ty::Reference {
                                    mutable: false,
                                    target: element.clone(),
                                },
                            )
                        {
                            return Ok(borrowed);
                        }
                        return Ok(Ty::sequence(sequence::Kind::Vec, item));
                    }
                    // `[T; N]` or `&[T; N]` of a const parameter `N`, as
                    // `with_children<const N: usize>([Slot; N])` takes: the
                    // argument's array gives `N` its length, as rustc infers
                    // it (#1330). `N` holds an array of that length.
                    #[cfg(feature = "rust-import")]
                    if let Some(name) = length_parameter(ty)
                        && !parameters.contains_key(name)
                        && let Some(length) = arguments
                            .and_then(|arguments| arguments.get(index))
                            .and_then(|argument| {
                                array_length(&self.inference.resolve(&argument.ty))
                            })
                    {
                        parameters.insert(
                            name.clone(),
                            Ty::sequence(sequence::Kind::Array(length), Ty::Unit),
                        );
                    }
                    self.instantiate_host_type(
                        ty,
                        &mut parameters,
                        &item_path,
                        operation,
                        item_owner.as_ref(),
                        originating,
                    )
                })
                .collect::<Result<Vec<_>, _>>()?;
        if let Some(arguments) = arguments {
            for (index, element) in open_ranges {
                inputs[index] = self.range_fixed_by_arguments(&inputs, arguments, index, &element);
            }
        }
        #[cfg(feature = "rust-import")]
        let output_origin = match item.signature().output().shape() {
            crate::host::type_ref::Shape::Imported { identity, .. } => receiver_origin
                .as_ref()
                .filter(|definition| definition.identity().imported() == Some(identity))
                .or_else(|| {
                    argument_origins.iter().find_map(|originating| {
                        originating
                            .as_ref()
                            .filter(|definition| definition.identity().imported() == Some(identity))
                    })
                })
                .or(associated_origin.as_ref()),
            _ => associated_origin.as_ref(),
        };
        #[cfg(not(feature = "rust-import"))]
        let output_origin = associated_origin.as_ref();
        let output = self.instantiate_host_type(
            item.signature().output(),
            &mut parameters,
            &item_path,
            operation,
            item_owner.as_ref(),
            output_origin,
        )?;
        // Without arguments, as a closure's hint is made, the receiver
        // still fixes its projections, as `P::State` of `.subscription(..)`.
        self.apply_relations(
            item,
            arguments.unwrap_or_default(),
            &mut parameters,
            operation,
        )?;
        let requirements = item
            .signature()
            .type_parameters()
            .map(|(name, required)| {
                let ty = parameters
                    .entry(name.clone())
                    .or_insert_with(|| self.inference.fresh())
                    .clone();
                (name, ty, required)
            })
            .collect();
        Ok(InstantiatedHostSignature {
            inputs,
            output,
            requirements,
        })
    }

    /// What an imported item's bounds tie between its type parameters. rand's
    /// `random_range<T, R>(range: R) -> T` has `R: SampleRange<T>`, which rand
    /// implements for `Range<T>`: an argument `0.5..1.0` fills `R` with a
    /// range of floats, and `T` is that float, as rustc's trait selection
    /// infers it. Only a bound whose argument matches exactly one
    /// implementation ties anything.
    /// The length of the Rust array that fills the imported item's type
    /// parameter `name`, where the item's bound on it has a case of an
    /// array of that length and `ty` is an array of it: `padding([10, 20])`
    /// fills `impl Into<Padding>` with the `[u16; 2]` of `Padding`'s
    /// `From<[u16; 2]>`, as rustc infers it, not with a `Vec`.
    #[cfg(feature = "rust-import")]
    pub(super) fn related_array_length(
        &self,
        item: &crate::host::item::Definition,
        name: &str,
        ty: &Ty,
    ) -> Option<u32> {
        let Ty::Array {
            kind: sequence::Kind::Array(length),
            ..
        } = self.inference.resolved_ref(ty)
        else {
            return None;
        };
        item.signature()
            .relations()
            .iter()
            .filter(|relation| relation.parameter() == name)
            .flat_map(|relation| relation.cases())
            .any(|case| {
                matches!(
                    case.this(),
                    crate::host::signature::Pattern::Type(this)
                        if matches!(
                            this.shape(),
                            crate::host::type_ref::Shape::Array {
                                sequence: crate::host::type_ref::Sequence::Array(case_length),
                                ..
                            } if case_length == length
                        )
                )
            })
            .then(|| u32::try_from(*length).ok())
            .flatten()
    }

    /// The `RangeInclusive<T>` input at `index`, whose argument is not known
    /// yet, of the kind the call's other arguments make `element`: `T` of
    /// `slider(range, value, message)` is the `f32` of a closure's
    /// `message: fn(f32) -> _`, as rustc infers `T` from every argument
    /// before the closure's call gives `range` its `2.0..=80.0`.
    pub(super) fn range_fixed_by_arguments(
        &mut self,
        inputs: &[Ty],
        arguments: &[ParsedExpr],
        index: usize,
        element: &Ty,
    ) -> Ty {
        let checkpoint = self.inference.checkpoint();
        for (other, (input, argument)) in inputs.iter().zip(arguments).enumerate() {
            if other != index && *input != Ty::Unknown {
                let _ = self.unify_type(input, &argument.ty);
            }
        }
        let fixed = self.inference.resolve(element);
        self.inference.rollback(checkpoint);
        let float = match fixed {
            Ty::Float(_) => Some(true),
            Ty::Int(_) => Some(false),
            Ty::Host { .. } => {
                let _ = self.unify_type(element, &fixed);
                None
            }
            _ => None,
        };
        self.inclusive_range_of(element, float)
    }

    /// Rawr's inclusive range where a crate takes `RangeInclusive<T>` of
    /// `element`: `RangeInclusive<f64>` where `float` says the argument is
    /// a range of floats, or with no argument where `T` is a float, and
    /// `RangeInclusive<i64>` otherwise. `T` takes the range's kind of
    /// number, as rustc's `{float}` or `{integer}`.
    pub(super) fn inclusive_range_of(&mut self, element: &Ty, float: Option<bool>) -> Ty {
        // With no argument to read, as a closure's hint is made, an open
        // `T` stays open for the call's own arguments to settle. A range of
        // a crate's type holds its two bounds.
        let float = match (float, self.inference.resolve(element)) {
            (Some(float), _) => float,
            (None, Ty::Float(_)) => true,
            (None, Ty::Int(_)) => false,
            (None, Ty::Host { .. }) => return Ty::InclusiveOf(Box::new(element.clone())),
            (None, _) => return Ty::InclusiveRange(Int::Var(self.inference.fresh_id())),
        };
        let id = self.inference.fresh_id();
        let number = if float {
            Ty::Float(Float::Var(id))
        } else {
            Ty::Int(Int::Var(id))
        };
        let _ = self.unify_type(element, &number);
        if float {
            Ty::FloatRange
        } else {
            Ty::InclusiveRange(Int::Var(id))
        }
    }

    pub(super) fn apply_relations(
        &mut self,
        item: &crate::host::item::Definition,
        arguments: &[ParsedExpr],
        parameters: &mut BTreeMap<ImmutableString, Ty>,
        span: Span,
    ) -> Result<(), Error> {
        let signature = item.signature();
        // The facade a case's imported type is read through: the item's
        // type's, as `instantiate_host_signature` reads its own types.
        let item_path = item.path();
        let owner = item_path
            .rsplit_once("::")
            .and_then(|(path, _)| self.models.host.type_owner(path))
            .or_else(|| self.models.host.item_owner(&item_path))
            .cloned();
        for relation in signature.relations() {
            let argument =
                signature
                    .parameters()
                    .iter()
                    .zip(arguments)
                    .find_map(|(declared, argument)| match declared.shape() {
                        crate::host::type_ref::Shape::Parameter(name)
                            if name.to_string() == relation.parameter() =>
                        {
                            Some(argument.ty.clone())
                        }
                        _ => None,
                    });
            // A projection's parameter no argument is, as `Renderer` of
            // `Cache<Renderer>` that `cache.draw(..)`'s receiver fixes, is
            // what the call has inferred for it (T318).
            #[cfg(feature = "rust-import")]
            let projection = relation.trait_identity().is_none();
            #[cfg(not(feature = "rust-import"))]
            let projection = false;
            // One the call leaves open, as the `Renderer` of `picture(h)
            // .boxed()` that only the element's later use fixes, waits for
            // it, as rustc's projection obligation does: the parameter it
            // names then takes the associated type, as `Handle` the
            // renderer's (#1632).
            #[cfg(feature = "rust-import")]
            if argument.is_none()
                && projection
                && let Some(open) = parameters
                    .get(relation.parameter())
                    .filter(|ty| matches!(self.inference.resolve(ty), Ty::Infer(_)))
                    .cloned()
            {
                self.defer_projection(
                    relation,
                    open,
                    parameters,
                    (&item_path, span),
                    owner.as_ref(),
                )?;
                continue;
            }
            let Some(argument) = argument.or_else(|| {
                parameters
                    .get(relation.parameter())
                    .filter(|_| projection)
                    .filter(|ty| !matches!(self.inference.resolve(ty), Ty::Infer(_)))
                    .cloned()
            }) else {
                continue;
            };
            let actual = self.inference.resolve(&argument);
            let owner = owner.as_ref();
            // `P: Into<T>` of the item's own `T`, which core's `impl<T>
            // From<T> for T` ties to `P`: while `T` is open, the impls of
            // `From<P>` for any type are as many candidates, and rustc
            // fixes `T` from a later bound, as `image(&handle)`'s `Handle`
            // is the renderer's. Only a `T` nothing else fixes by the
            // body's end is `P`.
            #[cfg(feature = "rust-import")]
            if let [case] = relation.cases()
                && matches!(case.this(), crate::host::signature::Pattern::Variable(_))
                && let [crate::host::signature::Pattern::Parameter(target)] = relation.arguments()
                && let Some(target) = parameters.get(target)
                && matches!(self.inference.resolve(target), Ty::Infer(_))
                && !matches!(actual, Ty::Int(_) | Ty::Float(_))
            {
                self.structural_defaults
                    .push((target.clone(), actual.clone(), span));
                continue;
            }
            let mut matched = None;
            let mut ambiguous = false;
            for case in relation.cases() {
                let mut bindings = BTreeMap::new();
                if self.pattern_matches(case.this(), &actual, &mut bindings, owner)
                    && self.case_arguments_fit(
                        case,
                        relation,
                        &bindings,
                        parameters,
                        (&item_path, owner),
                    )
                {
                    ambiguous |= matched.is_some();
                    matched = Some((case, bindings));
                }
            }
            #[cfg(feature = "rust-import")]
            let unmatched = matched.is_none();
            let (Some((case, mut bindings)), false) = (matched, ambiguous) else {
                // A std trait's own impls, which no crate's graph lists, as
                // `impl<T> AsRef<[T]> for Vec<T>`, which gives `L` of `rich(
                // impl AsRef<[Span<'a, L>]>)` from a `Vec<Span<'static,
                // ()>>`, as rustc selects one. A bound whose argument is
                // still open would fit many, and fixes nothing.
                #[cfg(feature = "rust-import")]
                if unmatched
                    && let Some(std_trait) = relation.std_trait()
                    && !matches!(actual, Ty::Infer(_))
                {
                    let mut declared = Vec::with_capacity(relation.arguments().len());
                    for argument in relation.arguments() {
                        declared.push(self.pattern_type(
                            argument,
                            &mut BTreeMap::new(),
                            parameters,
                            (&item_path, span),
                            owner,
                        )?);
                    }
                    if !declared
                        .iter()
                        .any(|argument| matches!(self.inference.resolve(argument), Ty::Infer(_)))
                    {
                        let checkpoint = self.inference.checkpoint();
                        match std_impl::select(self, std_trait, &actual, &declared, false, 0) {
                            Ok(_) => self.inference.commit(checkpoint),
                            Err(_) => self.inference.rollback(checkpoint),
                        }
                    }
                }
                // An opaque type that fills the parameter gives the
                // associated type from its own bound, as `application(..)`'s
                // `impl Program<State = S>` gives `P::State`, so a closure
                // `.subscription(|state| ..)` takes `&S`, as rustc reads it.
                #[cfg(feature = "rust-import")]
                if let Some((identity, name)) = relation.associated()
                    && let Ty::Host { proof, arguments } = &actual
                    && let Some(value) = proof
                        .definition
                        .associated(identity, name)
                        .and_then(|position| arguments.get(position))
                    && let [declared] = relation.arguments()
                {
                    let value = value.clone();
                    let declared = self.pattern_type(
                        declared,
                        &mut BTreeMap::new(),
                        parameters,
                        (&item_path, span),
                        owner,
                    )?;
                    // A mismatch is rustc's to report where it compiles the call.
                    let _ = self.unify_type(&declared, &value);
                }
                // A program's iterator that fills the parameter gives its
                // items as `I::Item`, and an iterator is its own `IntoIter`,
                // as the `Product<Self, J::IntoIter>` of itertools'
                // `cartesian_product` holds two. An integer range a crate
                // takes by value is one too, of its own kind.
                #[cfg(feature = "rust-import")]
                if let Some((_, name)) = relation.associated()
                    && let [declared] = relation.arguments()
                    && let Some(item) = match (name.as_str(), &actual) {
                        ("Item", Ty::Iter(item, _)) => Some((**item).clone()),
                        // An array or a vector a crate takes by value gives
                        // its elements, as rand's `WeightedIndex::new([10,
                        // 4, 2, 1])` reads `I::Item` (#1084).
                        (
                            "Item",
                            Ty::Array {
                                element,
                                kind:
                                    sequence::Kind::Array(_)
                                    | sequence::Kind::Vec
                                    | sequence::Kind::Deque,
                            },
                        ) => Some((**element).clone()),
                        ("Item", Ty::InclusiveRange(kind)) => Some(Ty::Int(*kind)),
                        ("Item", Ty::Range(element)) if matches!(**element, Ty::Int(_)) => {
                            Some((**element).clone())
                        }
                        ("IntoIter", Ty::InclusiveRange(_)) => Some(actual.clone()),
                        ("IntoIter", Ty::Range(element)) if matches!(**element, Ty::Int(_)) => {
                            Some(actual.clone())
                        }
                        ("Item", Ty::Host { .. }) => self.host_iterator_item(&actual, span),
                        ("IntoIter", Ty::Iter(..)) => Some(actual.clone()),
                        ("IntoIter", Ty::Host { .. })
                            if self.type_implements(&actual, "Iterator") =>
                        {
                            Some(actual.clone())
                        }
                        _ => None,
                    }
                {
                    let declared = self.pattern_type(
                        declared,
                        &mut BTreeMap::new(),
                        parameters,
                        (&item_path, span),
                        owner,
                    )?;
                    let _ = self.unify_type(&declared, &item);
                }
                continue;
            };
            // The one case the argument fits is its type, so its literals
            // take the case's kinds: `(35, 35, 35)` is `(u8, u8, u8)` where
            // the only three-tuple `Fill` comes from is.
            let this = self.pattern_type(
                case.this(),
                &mut bindings,
                parameters,
                (&item_path, span),
                owner,
            )?;
            let _ = self.unify_type(&actual, &this);
            for (implemented, declared) in case.arguments().iter().zip(relation.arguments()) {
                let implemented = self.pattern_type(
                    implemented,
                    &mut bindings,
                    parameters,
                    (&item_path, span),
                    owner,
                )?;
                let declared = self.pattern_type(
                    declared,
                    &mut BTreeMap::new(),
                    parameters,
                    (&item_path, span),
                    owner,
                )?;
                // A mismatch is rustc's to report where it compiles the call.
                let _ = self.unify_type(&declared, &implemented);
            }
            // The case's bounds that fix an associated type, as iced's
            // `From<Image<Handle>> for Element` fixes `Handle` once the
            // element's renderer is known, now or by a later use.
            #[cfg(feature = "rust-import")]
            for projection in case.projections() {
                let mut at = |this: &mut Self, pattern, bindings: &mut BTreeMap<u32, Ty>| {
                    this.pattern_type(pattern, bindings, parameters, (&item_path, span), owner)
                };
                let (Ok(ty), Ok(value)) = (
                    at(self, &projection.ty, &mut bindings),
                    at(self, &projection.value, &mut bindings),
                ) else {
                    continue;
                };
                // A case of a type this program doesn't reach, as a backend
                // only an alias names, fits no type it has.
                let mut cases = Vec::with_capacity(projection.cases.len());
                for (this, given) in projection.cases.iter() {
                    let mut own = BTreeMap::new();
                    if let (Ok(this), Ok(given)) =
                        (at(self, this, &mut own), at(self, given, &mut own))
                    {
                        cases.push((this, given));
                    }
                }
                let open = self
                    .project_impl(vec![ImplProjection {
                        ty,
                        value,
                        cases: cases.into_boxed_slice(),
                    }])
                    .unwrap_or_default();
                self.inference.projections.extend(open);
            }
        }
        Ok(())
    }

    /// The projection `relation` that ties `<open as Trait>::Name` to the
    /// parameter its one argument names, with each of its cases, pending in
    /// `Inference::projections` for `open` to be known
    /// (`finish_projections`).
    #[cfg(feature = "rust-import")]
    fn defer_projection(
        &mut self,
        relation: &crate::host::signature::Relation,
        open: Ty,
        parameters: &mut BTreeMap<ImmutableString, Ty>,
        (operation, span): (&str, Span),
        owner: Option<&crate::host::facade::Dependency>,
    ) -> Result<(), Error> {
        let [declared] = relation.arguments() else {
            return Ok(());
        };
        let value = self.pattern_type(
            declared,
            &mut BTreeMap::new(),
            parameters,
            (operation, span),
            owner,
        )?;
        let mut cases = Vec::with_capacity(relation.cases().len());
        for case in relation.cases() {
            let [given] = case.arguments() else {
                continue;
            };
            let mut own = BTreeMap::new();
            // A case of a type this program doesn't reach fits no type it has.
            if let (Ok(this), Ok(given)) = (
                self.pattern_type(case.this(), &mut own, parameters, (operation, span), owner),
                self.pattern_type(given, &mut own, parameters, (operation, span), owner),
            ) {
                cases.push((this, given));
            }
        }
        self.inference.projections.push(ImplProjection {
            ty: open,
            value,
            cases: cases.into_boxed_slice(),
        });
        Ok(())
    }

    /// Whether `case` of `relation` fits by its trait arguments as well as
    /// by its self type, `bindings` what the self type bound. A case whose
    /// self type is a bare variable, as core's reflexive `impl<T> From<T>
    /// for T` behind `impl Into<Slot<'a, Message>>`, fits every argument by
    /// its self type, so rustc selects it by the whole trait reference: it
    /// fits only an argument that unifies with the bound's (#1404). Any
    /// other case fits by its self type unless an argument it implements
    /// is one the bound's can't be, as a component crate's `impl Into<
    /// Element<..>> for Slot<..>` is no case of `impl Into<Slot<'a,
    /// Message>>` for a slot: rustc matches the whole trait reference of
    /// every impl, and only the reflexive one ties the slot's message to
    /// the bound's (#1422).
    pub(super) fn case_arguments_fit(
        &mut self,
        case: &crate::host::signature::Case,
        relation: &crate::host::signature::Relation,
        bindings: &BTreeMap<u32, Ty>,
        parameters: &BTreeMap<ImmutableString, Ty>,
        (item_path, owner): (&str, Option<&crate::host::facade::Dependency>),
    ) -> bool {
        // A case whose self type fits every argument needs its arguments
        // read to fit; any other only needs them not to contradict.
        let variable = matches!(case.this(), crate::host::signature::Pattern::Variable(_));
        if case.arguments().len() != relation.arguments().len() {
            return !variable;
        }
        let checkpoint = self.inference.checkpoint();
        let mut bindings = bindings.clone();
        let mut parameters = parameters.clone();
        let mut fits = true;
        for (implemented, declared) in case.arguments().iter().zip(relation.arguments()) {
            let at = (item_path, Span::NONE);
            let implemented =
                self.pattern_type(implemented, &mut bindings, &mut parameters, at, owner);
            let declared =
                self.pattern_type(declared, &mut BTreeMap::new(), &mut parameters, at, owner);
            fits = match (implemented, declared) {
                (Ok(implemented), Ok(declared)) => self.unify_type(&declared, &implemented),
                _ => !variable,
            };
            if !fits {
                break;
            }
        }
        self.inference.rollback(checkpoint);
        fits
    }

    /// The type parameters an imported item's bounds leave open once its
    /// call is checked, fixed by the bound trait's impl headers as D604
    /// fixes an `impl Trait`'s: iced's `application(.., view)` has `W:
    /// Widget<Message, Theme, Renderer>` for the `W` `view` gives, and when
    /// that is `Column<Element<Message, Theme, Renderer>>` the impl for
    /// `Column<W>` and its own `W: Widget<M, T, R>` fix `Theme` and
    /// `Renderer`, as rustc's trait selection does. An argument the bound
    /// leaves open stays rustc's to infer (D594).
    #[cfg(feature = "rust-import")]
    pub(super) fn fix_bounds_by_impls(
        &mut self,
        item: &crate::host::item::Definition,
        requirements: &[(ImmutableString, Ty, capability::Set)],
        operands: &[(Ty, Span, bool)],
        span: Span,
    ) -> Result<(), Error> {
        let signature = item.signature();
        if item.imported_identity().is_none()
            || signature
                .relations()
                .iter()
                .all(|relation| relation.trait_identity().is_none())
        {
            return Ok(());
        }
        let item_path = item.path();
        let owner = item_path
            .rsplit_once("::")
            .and_then(|(path, _)| self.models.host.type_owner(path))
            .or_else(|| self.models.host.item_owner(&item_path))
            .cloned();
        let mut parameters = requirements
            .iter()
            .map(|(name, ty, _)| (name.clone(), ty.clone()))
            .collect::<BTreeMap<_, _>>();
        for relation in signature.relations() {
            let Some(identity) = relation.trait_identity() else {
                continue;
            };
            let Some(bounded) = parameters.get(relation.parameter()).cloned() else {
                continue;
            };
            let bounded = self.inference.resolve(&bounded);
            // A value still to be inferred is known by its bound only when
            // a call of a function written `-> impl Trait` gave it.
            if matches!(bounded, Ty::Infer(_))
                && !self
                    .opaque_values
                    .iter()
                    .any(|value| self.inference.resolve(&value.value) == bounded)
            {
                continue;
            }
            let mut arguments = Vec::with_capacity(relation.arguments().len());
            for argument in relation.arguments() {
                arguments.push(self.pattern_type(
                    argument,
                    &mut BTreeMap::new(),
                    &mut parameters,
                    (&item_path, span),
                    owner.as_ref(),
                )?);
            }
            self.take_opaque_arguments(&bounded, identity, &arguments, operands);
            if matches!(bounded, Ty::Infer(_)) {
                continue;
            }
            // A script type in the bound is followed even when nothing is
            // open, for the script impls it uses (D629). So is a type the
            // bound's arguments leave open in the bounded value, as the
            // `Message` of a crate's `plaque::<_>()` passed as `pressing`'s
            // `impl Widget<Message>`: the opaque type's impl fixes it, as
            // rustc infers it from the expected type (#1769).
            if !contains_inference(&bounded)
                && !arguments
                    .iter()
                    .any(|argument| contains_inference(&self.inference.resolve(argument)))
                && !mentions_script(&bounded)
            {
                continue;
            }
            // No fit, or more than one, leaves them to rustc.
            let checkpoint = self.inference.checkpoint();
            match self.select_impl(&bounded, identity, &arguments, owner.as_ref(), 0) {
                Ok(()) => self.inference.commit(checkpoint),
                Err(_) => self.inference.rollback(checkpoint),
            }
        }
        Ok(())
    }

    /// Keeps `value`, what a call of a program's function gives, when the
    /// function's output is written `impl Trait` (`opaque_values`). The
    /// written output is the one kept where the hidden type is revealed,
    /// else the function's output, which is still the written one. Its
    /// generic parameters are at the types `substitution` gives them. The value
    /// is the one the call gives, or, while a generic function compiled per
    /// instantiation (D629) has no instance for the call yet, a type to be
    /// inferred that rustc knows by the bound alone: `new_icon::<_>()` of
    /// `fn new_icon<M>() -> impl Widget<M>` takes its `M` from the bound of
    /// the parameter it is passed to (#1633).
    #[cfg(feature = "rust-import")]
    pub(super) fn note_opaque_value(
        &mut self,
        value: Ty,
        written: Option<&type_ref::Type>,
        substitution: &BTreeMap<ImmutableString, Ty>,
    ) -> Ty {
        let Some(written) = written.filter(|written| written.opaque_trait().is_some()) else {
            return value;
        };
        let value = match self.inference.resolve(&value) {
            Ty::Unknown if self.collecting_instances => self.inference.fresh(),
            _ => value,
        };
        self.opaque_values.push(OpaqueValue {
            value: value.clone(),
            written: written.clone(),
            substitution: substitution.clone(),
        });
        value
    }

    /// A bound `bounded: Trait<arguments>` on what a script function of the
    /// call gives, where its signature writes `impl Trait<..>`: rustc knows
    /// the value by that bound alone, so its arguments, the trait's
    /// defaults filled in, are the bound's. url_handler's `view(&self) ->
    /// impl Widget<Message>` gives a `Container` that is a widget of every
    /// renderer, and the `impl Widget<Message>` fixes iced's (#1072).
    #[cfg(feature = "rust-import")]
    pub(super) fn take_opaque_arguments(
        &mut self,
        bounded: &Ty,
        identity: [u8; 32],
        arguments: &[Ty],
        operands: &[(Ty, Span, bool)],
    ) {
        let substitution = BTreeMap::new();
        let written = operands.iter().find_map(|(ty, _, _)| {
            let Ty::Callable {
                output,
                item: Some(value),
                ..
            } = self.inference.resolve(ty)
            else {
                return None;
            };
            if self.inference.resolve(&output) != *bounded {
                return None;
            }
            let key = value.key();
            if let Some(written) = self.path_outputs.get(key) {
                return Some(written.clone());
            }
            let lifetimes = match self.models.functions_by_key.get(key) {
                Some(function) => &function.definition.lifetimes,
                None => self
                    .models
                    .methods
                    .values()
                    .find(|method| method.key() == key)?
                    .lifetimes(),
            };
            lifetimes.written_output.as_deref().cloned()
        });
        // A value a call of such a function gave, as `counter.view()`.
        let (written, substitution) = match written {
            Some(written) => (written, &substitution),
            None => {
                let Some(value) = self
                    .opaque_values
                    .iter()
                    .find(|value| self.inference.resolve(&value.value) == *bounded)
                else {
                    return;
                };
                (value.written.clone(), &value.substitution.clone())
            }
        };
        let type_ref::Type::Path(opaque) = written else {
            return;
        };
        let Some(trait_name) = type_ref::Type::Path(opaque.clone())
            .opaque_trait()
            .map(String::from)
        else {
            return;
        };
        let Ok(path) = crate::host::path::Path::parse(&trait_name) else {
            return;
        };
        let Some(header) = self.models.host.trait_header(&path).cloned() else {
            return;
        };
        // The bound's trait is the written one, or one the written one
        // holds at the same arguments: a trait every type of the bound's
        // implements through a blanket impl that passes its arguments on,
        // as iced's `Widget` is for each `core::Widget`.
        let passes_on = |headers: &[crate::host::facade::ImplHeader]| {
            headers.iter().any(|implementation| {
                matches!(
                    implementation.self_type.shape(),
                    crate::host::type_ref::Shape::Parameter(_)
                ) && implementation.bounds.iter().any(|bound| {
                    bound.trait_identity == identity
                        && bound.ty == implementation.self_type
                        && bound.arguments == implementation.arguments
                })
            })
        };
        if header.identity != identity
            && !passes_on(
                &self
                    .models
                    .host
                    .impl_headers(header.identity)
                    .cloned()
                    .collect::<Vec<_>>(),
            )
        {
            return;
        }
        let owner = self.models.host.type_owner(&trait_name).cloned();
        let written = opaque
            .arguments
            .iter()
            .filter_map(|argument| match argument {
                type_ref::Argument::Type(ty) => {
                    Some(instantiate(&Ty::from_resolved(ty), substitution))
                }
                type_ref::Argument::Lifetime(_) => None,
            })
            .collect::<Vec<_>>();
        let mut parameters = BTreeMap::new();
        for (index, (name, argument)) in header.parameters.iter().zip(arguments).enumerate() {
            let ty = match written.get(index) {
                Some(ty) => ty.clone(),
                None => match header.defaults.get(index).and_then(Option::as_ref) {
                    Some(default) => match self.instantiate_host_type(
                        default,
                        &mut parameters,
                        &trait_name,
                        Span::NONE,
                        owner.as_ref(),
                        None,
                    ) {
                        Ok(ty) => ty,
                        Err(_) => return,
                    },
                    None => return,
                },
            };
            parameters.insert(name.clone(), ty.clone());
            // A mismatch is rustc's to report where it compiles the call.
            let _ = self.unify_type(argument, &ty);
        }
    }

    /// Whether `ty` is a type of `pattern`, binding its variables. An
    /// integer or float literal whose kind is still open is a type of each
    /// builtin of its kind, as rustc's `{integer}` is.
    pub(super) fn pattern_matches(
        &mut self,
        pattern: &crate::host::signature::Pattern,
        ty: &Ty,
        bindings: &mut BTreeMap<u32, Ty>,
        owner: Option<&crate::host::facade::Dependency>,
    ) -> bool {
        use crate::host::signature::Pattern;
        use crate::host::type_ref::Builtin;
        let ty = self.inference.resolve(ty);
        match (pattern, &ty) {
            (
                Pattern::Builtin(
                    Builtin::I8
                    | Builtin::I16
                    | Builtin::I32
                    | Builtin::I64
                    | Builtin::Isize
                    | Builtin::U8
                    | Builtin::U16
                    | Builtin::U32
                    | Builtin::U64
                    | Builtin::Usize,
                ),
                Ty::Int(Int::Var(_)),
            )
            | (Pattern::Builtin(Builtin::F32 | Builtin::F64), Ty::Float(Float::Var(_))) => true,
            (Pattern::Tuple(members), Ty::Tuple(actual)) => {
                members.len() == actual.len()
                    && members.iter().zip(actual.iter()).all(|(member, actual)| {
                        self.pattern_matches(member, actual, bindings, owner)
                    })
            }
            (Pattern::Type(expected), _) => {
                let checkpoint = self.inference.checkpoint();
                let fits = self
                    .instantiate_host_type(
                        expected,
                        &mut BTreeMap::new(),
                        "",
                        Span::NONE,
                        owner,
                        None,
                    )
                    .is_ok_and(|expected| self.unify_type(&expected, &ty));
                self.inference.rollback(checkpoint);
                fits
            }
            // Whether the type fits; its variables are bound where the
            // case is chosen, as its type is unified with the argument.
            (Pattern::Generic(expected), _) => {
                let checkpoint = self.inference.checkpoint();
                let fits = self
                    .instantiate_host_type(
                        expected,
                        &mut BTreeMap::new(),
                        "",
                        Span::NONE,
                        owner,
                        None,
                    )
                    .is_ok_and(|expected| self.unify_type(&expected, &ty));
                self.inference.rollback(checkpoint);
                fits
            }
            (Pattern::Variable(index), _) => match bindings.get(index) {
                Some(bound) => *bound == ty,
                None => {
                    bindings.insert(*index, ty);
                    true
                }
            },
            (Pattern::Unit, Ty::Unit) => true,
            (Pattern::Range(element), Ty::Range(actual))
            | (Pattern::Option(element), Ty::Option(actual)) => {
                self.pattern_matches(element, actual, bindings, owner)
            }
            (Pattern::Builtin(builtin), _) => builtin
                .name()
                .is_some_and(|name| ty.name(&self.models) == name),
            _ => false,
        }
    }

    /// The type `pattern` names, its variables from `bindings` (a fresh one
    /// for an unbound variable) and the item's parameters from `parameters`.
    pub(super) fn pattern_type(
        &mut self,
        pattern: &crate::host::signature::Pattern,
        bindings: &mut BTreeMap<u32, Ty>,
        parameters: &mut BTreeMap<ImmutableString, Ty>,
        (operation, span): (&str, Span),
        owner: Option<&crate::host::facade::Dependency>,
    ) -> Result<Ty, Error> {
        use crate::host::signature::Pattern;
        Ok(match pattern {
            Pattern::Parameter(name) => parameters
                .entry(name.clone())
                .or_insert_with(|| self.inference.fresh())
                .clone(),
            Pattern::Variable(index) => match bindings.get(index) {
                Some(ty) => ty.clone(),
                None => {
                    let ty = self.inference.fresh();
                    bindings.insert(*index, ty.clone());
                    ty
                }
            },
            Pattern::Unit => Ty::Unit,
            Pattern::Builtin(builtin) => self.instantiate_host_type(
                &crate::host::type_ref::Type::builtin(*builtin),
                parameters,
                operation,
                span,
                None,
                None,
            )?,
            Pattern::Range(element) => Ty::Range(Box::new(self.pattern_type(
                element,
                bindings,
                parameters,
                (operation, span),
                owner,
            )?)),
            Pattern::Option(element) => Ty::Option(Box::new(self.pattern_type(
                element,
                bindings,
                parameters,
                (operation, span),
                owner,
            )?)),
            Pattern::Tuple(members) => Ty::Tuple(
                members
                    .iter()
                    .map(|member| {
                        self.pattern_type(member, bindings, parameters, (operation, span), owner)
                    })
                    .collect::<Result<_, _>>()?,
            ),
            // A type that names the item's parameters, as `Element<'a,
            // Message, Theme, Renderer>` of `impl Into<Element<..>>`, names
            // the call's.
            Pattern::Type(ty) => {
                self.instantiate_host_type(ty, parameters, operation, span, owner, None)?
            }
            // The item's parameters are its own, and each of the impl's is
            // its variable's binding.
            Pattern::Generic(ty) => {
                for (index, bound) in bindings.iter() {
                    parameters.insert(
                        crate::host::signature::case_variable(*index).into(),
                        bound.clone(),
                    );
                }
                let ty = self.instantiate_host_type(ty, parameters, operation, span, owner, None);
                let variables = parameters
                    .keys()
                    .filter(|name| crate::host::signature::case_index(name).is_some())
                    .cloned()
                    .collect::<Vec<_>>();
                for name in variables {
                    if let (Some(index), Some(bound)) = (
                        crate::host::signature::case_index(&name),
                        parameters.remove(&name),
                    ) {
                        bindings.insert(index, bound);
                    }
                }
                ty?
            }
        })
    }

    /// The defaulted arguments a type is written without, as rustc fills
    /// them: `let content: text_editor::Content` is a `Content<Renderer>`.
    /// Nothing is filled unless every missing argument has a default.
    pub(super) fn default_host_arguments(
        &mut self,
        definition: &crate::host::type_def::Definition,
        owner: Option<&crate::host::facade::Dependency>,
        arguments: &mut Vec<Ty>,
    ) {
        let mut parameters = definition
            .parameters()
            .iter()
            .cloned()
            .zip(arguments.iter().cloned())
            .collect::<BTreeMap<_, _>>();
        let mut filled = Vec::new();
        for index in arguments.len()..definition.parameters().len() {
            let Some(Some(default)) = definition.defaults().get(index) else {
                return;
            };
            let Ok(ty) = self.instantiate_host_type(
                default,
                &mut parameters,
                "type",
                Span::NONE,
                owner,
                Some(definition),
            ) else {
                return;
            };
            parameters.insert(definition.parameters()[index].clone(), ty.clone());
            filled.push(ty);
        }
        arguments.extend(filled);
    }

    pub(super) fn instantiate_host_type(
        &mut self,
        ty: &crate::host::type_ref::Type,
        parameters: &mut BTreeMap<ImmutableString, Ty>,
        operation: &str,
        span: Span,
        owner: Option<&crate::host::facade::Dependency>,
        originating: Option<&crate::host::type_def::Definition>,
    ) -> Result<Ty, Error> {
        use crate::host::type_ref::{Builtin, Shape};
        #[cfg(not(feature = "rust-import"))]
        let _ = (owner, originating);
        Ok(match ty.shape() {
            Shape::Unit => Ty::Unit,
            // A tuple input is a callable of several parameters.
            Shape::Callable { input, output } => Ty::Callable {
                inputs: match input.shape() {
                    // `impl Fn() -> T`.
                    Shape::Unit => Box::new([]),
                    Shape::Tuple(members) => members
                        .iter()
                        .map(|member| {
                            self.instantiate_host_type(
                                member,
                                parameters,
                                operation,
                                span,
                                owner,
                                originating,
                            )
                        })
                        .collect::<Result<_, _>>()?,
                    _ => Box::new([self.instantiate_host_type(
                        input,
                        parameters,
                        operation,
                        span,
                        owner,
                        originating,
                    )?]),
                },
                output: Box::new(self.instantiate_host_type(
                    output,
                    parameters,
                    operation,
                    span,
                    owner,
                    originating,
                )?),
                item: None,
            },
            #[cfg(feature = "rust-import")]
            Shape::ImportedFunctionItem { input, output } => Ty::ImportedFunctionItem {
                input: Box::new(self.instantiate_host_type(
                    input,
                    parameters,
                    operation,
                    span,
                    owner,
                    originating,
                )?),
                output: Box::new(self.instantiate_host_type(
                    output,
                    parameters,
                    operation,
                    span,
                    owner,
                    originating,
                )?),
                item: None,
            },
            Shape::Builtin(Builtin::Bool) => Ty::Bool,
            Shape::Builtin(Builtin::Char) => Ty::Char,
            Shape::Builtin(Builtin::F64) => Ty::Float(Float::F64),
            Shape::Builtin(Builtin::I64) => Ty::Int(Int::I64),
            Shape::Builtin(Builtin::I8) => Ty::Int(Int::I8),
            Shape::Builtin(Builtin::I16) => Ty::Int(Int::I16),
            Shape::Builtin(Builtin::I32) => Ty::Int(Int::I32),
            Shape::Builtin(Builtin::Isize) => Ty::Int(Int::Isize),
            Shape::Builtin(Builtin::U8) => Ty::Int(Int::U8),
            Shape::Builtin(Builtin::U16) => Ty::Int(Int::U16),
            Shape::Builtin(Builtin::U32) => Ty::Int(Int::U32),
            Shape::Builtin(Builtin::U64) => Ty::Int(Int::U64),
            Shape::Builtin(Builtin::Usize) => Ty::Int(Int::Usize),
            Shape::Builtin(Builtin::F32) => Ty::Float(Float::F32),
            Shape::Builtin(Builtin::Duration) => Ty::Time(Time::Duration),
            Shape::Builtin(Builtin::Instant) => Ty::Time(Time::Instant),
            Shape::Builtin(Builtin::Never) => Ty::Never,
            Shape::Builtin(Builtin::String) => Ty::String,
            // A crate's `Cow<'_, str>` input: std's `Cow<str>`, which a
            // `String` or a `&str` doesn't fill (E0277), as rustc has it.
            Shape::Builtin(Builtin::Text) => Ty::Cow(Box::new(Ty::UnsizedStr)),
            // A crate's `fmt::Arguments<'_>`: the program's `format_args!`,
            // which a `String` doesn't fill (E0308), as rustc has it.
            Shape::Builtin(Builtin::Arguments) => Ty::Arguments,
            Shape::Builtin(Builtin::InclusiveRange) => {
                Ty::InclusiveRange(Int::Var(self.inference.fresh_id()))
            }
            Shape::Builtin(Builtin::FloatInclusiveRange) => Ty::FloatRange,
            Shape::Parameter(name) => parameters
                .entry(name.to_string().into())
                .or_insert_with(|| self.inference.fresh())
                .clone(),
            Shape::Registered { path, arguments } => {
                let path = path.to_string();
                let definition = self
                    .models
                    .host
                    .type_(&path)
                    .cloned()
                    .ok_or_else(|| unnamed_signature_type(operation, span, Some(&*path)))?;
                let alias = definition.identity().imported().map(|_| definition.clone());
                Ty::Host {
                    proof: HostTyProof::new(
                        definition.clone(),
                        alias,
                        self.models.host.type_owner(&path).cloned(),
                    ),
                    arguments: arguments
                        .iter()
                        .map(|argument| {
                            self.instantiate_host_type(
                                argument,
                                parameters,
                                operation,
                                span,
                                owner,
                                originating,
                            )
                        })
                        .collect::<Result<Vec<_>, _>>()?
                        .into_boxed_slice(),
                }
            }
            Shape::RangeInclusive(element) => {
                let element = self.instantiate_host_type(
                    element,
                    parameters,
                    operation,
                    span,
                    owner,
                    originating,
                )?;
                self.inclusive_range_of(&element, None)
            }
            #[cfg(feature = "rust-import")]
            Shape::Imported {
                identity,
                arguments,
            } => {
                // Only a crate's own items name its types by identity, and
                // overload selection tries every form, so a form naming a
                // type this program doesn't reach is one that doesn't fit.
                let imported = owner
                    .and_then(|owner| self.models.host.imported_type(identity, owner, originating))
                    .ok_or_else(|| unnamed_signature_type(operation, span, None))?;
                let definition = imported.definition.clone();
                let alias = imported.alias.cloned();
                let owner = imported.owner.cloned();
                // A native module's `&Theme` of a marked `theme` names the
                // marked type through its mirror: the type marked code has.
                if let Some(marked) = self.models.mirrored(&definition) {
                    let arguments = arguments
                        .iter()
                        .map(|argument| {
                            self.instantiate_host_type(
                                argument,
                                parameters,
                                operation,
                                span,
                                owner.as_ref(),
                                alias.as_ref(),
                            )
                        })
                        .collect::<Result<_, _>>()?;
                    return Ok(Ty::Script(marked, arguments));
                }
                Ty::Host {
                    proof: HostTyProof::new(definition, alias.clone(), owner.clone()),
                    arguments: arguments
                        .iter()
                        .map(|argument| {
                            self.instantiate_host_type(
                                argument,
                                parameters,
                                operation,
                                span,
                                owner.as_ref(),
                                alias.as_ref(),
                            )
                        })
                        .collect::<Result<Vec<_>, _>>()?
                        .into_boxed_slice(),
                }
            }
            Shape::External { export, arguments } => {
                let writer = owner;
                let path = export.path();
                let definition = self
                    .models
                    .host
                    .type_(&path)
                    .cloned()
                    .ok_or_else(|| unnamed_signature_type(operation, span, Some(&*path)))?;
                let owner = self.models.host.type_owner(&path).cloned();
                if definition.contract() != *export.contract()
                    || owner.as_ref() != Some(&export.dependency())
                {
                    return Err(unnamed_signature_type(operation, span, Some(&*path)));
                }
                Ty::Host {
                    proof: HostTyProof::new(
                        definition.clone(),
                        definition.identity().imported().map(|_| definition.clone()),
                        owner.clone(),
                    ),
                    // The arguments are written by the referencing facade, so
                    // an imported argument is that facade's: a crate's
                    // `task::Sipper<Output, Progress>` names its own types.
                    arguments: arguments
                        .iter()
                        .map(|argument| {
                            self.instantiate_host_type(
                                argument,
                                parameters,
                                operation,
                                span,
                                writer,
                                originating,
                            )
                        })
                        .collect::<Result<Vec<_>, _>>()?
                        .into_boxed_slice(),
                }
            }
            Shape::Option(element) => Ty::Option(Box::new(self.instantiate_host_type(
                element,
                parameters,
                operation,
                span,
                owner,
                originating,
            )?)),
            // A `&str` an imported method lends, as url's `Url::scheme`:
            // the `&str` rustc sees.
            Shape::Reference {
                mutable: false,
                target,
            } if matches!(target.shape(), Shape::Builtin(Builtin::String)) => Ty::Str,
            Shape::Reference { mutable, target } => Ty::Reference {
                mutable: *mutable,
                target: Box::new(self.instantiate_host_type(
                    target,
                    parameters,
                    operation,
                    span,
                    owner,
                    originating,
                )?),
            },
            Shape::Boxed(element) => Ty::Boxed(Box::new(self.instantiate_host_type(
                element,
                parameters,
                operation,
                span,
                owner,
                originating,
            )?)),
            // A crate's trait object (D872): `&dyn Trait` or `Box<dyn
            // Trait>` of the trait by its name, each type argument a
            // positional binding, as a program's `dyn Convert<i64>` has.
            #[cfg(feature = "rust-import")]
            Shape::Object {
                pointer,
                identity,
                arguments,
            } => {
                let name = self.models.host.trait_name(identity).ok_or_else(|| {
                    unnamed_signature_type(operation, span, Some("a trait object's trait"))
                })?;
                let mut bindings = Vec::new();
                for (position, argument) in arguments.iter().enumerate() {
                    bindings.push((
                        ImmutableString::from(position.to_string()),
                        self.instantiate_host_type(
                            argument,
                            parameters,
                            operation,
                            span,
                            owner,
                            originating,
                        )?,
                    ));
                }
                let object = Box::new(Ty::Dyn(name, bindings.into_boxed_slice()));
                use rawr_import::classify::representability::object::Pointer;
                match pointer {
                    Pointer::Boxed => Ty::Boxed(object),
                    Pointer::Rc => Ty::Counted(Counter::Rc, object),
                    Pointer::Arc => Ty::Counted(Counter::Arc, object),
                    Pointer::Lasting | Pointer::Shared => Ty::Reference {
                        mutable: false,
                        target: object,
                    },
                }
            }
            // std's map or set in a crate's signature: rawr's (#777).
            Shape::Keyed(name, members) => Ty::Keyed(
                Keyed::from_name(name).ok_or_else(|| {
                    self.unsupported("this map or set type in a crate's signature")
                })?,
                members
                    .iter()
                    .map(|member| {
                        self.instantiate_host_type(
                            member,
                            parameters,
                            operation,
                            span,
                            owner,
                            originating,
                        )
                    })
                    .collect::<Result<_, _>>()?,
            ),
            // A crate's `Rc<T>` or `Arc<T>`: rawr's, which stands for the
            // crate's own pointer at runtime (#775).
            Shape::Counted(arc, element) => Ty::Counted(
                if *arc { Counter::Arc } else { Counter::Rc },
                Box::new(self.instantiate_host_type(
                    element,
                    parameters,
                    operation,
                    span,
                    owner,
                    originating,
                )?),
            ),
            // A crate's `JoinHandle<T>`: rawr's, which joins the crate's
            // thread (#1266).
            Shape::JoinHandle(element) => Ty::Thread(
                thread::Kind::JoinHandle,
                Box::new(self.instantiate_host_type(
                    element,
                    parameters,
                    operation,
                    span,
                    owner,
                    originating,
                )?),
            ),
            Shape::Iter(element) => Ty::iter(self.instantiate_host_type(
                element,
                parameters,
                operation,
                span,
                owner,
                originating,
            )?),
            Shape::Range(element) => Ty::Range(Box::new(self.instantiate_host_type(
                element,
                parameters,
                operation,
                span,
                owner,
                originating,
            )?)),
            Shape::Result(ok, err) => Ty::Result(
                Box::new(self.instantiate_host_type(
                    ok,
                    parameters,
                    operation,
                    span,
                    owner,
                    originating,
                )?),
                Box::new(self.instantiate_host_type(
                    err,
                    parameters,
                    operation,
                    span,
                    owner,
                    originating,
                )?),
            ),
            Shape::Array { element, sequence } => Ty::Array {
                element: Box::new(self.instantiate_host_type(
                    element,
                    parameters,
                    operation,
                    span,
                    owner,
                    originating,
                )?),
                kind: match sequence {
                    crate::host::type_ref::Sequence::Array(length) => {
                        sequence::Kind::Array(*length)
                    }
                    crate::host::type_ref::Sequence::Vec => sequence::Kind::Vec,
                    crate::host::type_ref::Sequence::Slice => sequence::Kind::Slice,
                    // `[T; N]` of the length the call's argument gave `N`
                    // (`instantiate_host_signature`).
                    #[cfg(feature = "rust-import")]
                    crate::host::type_ref::Sequence::Length(name) => match parameters
                        .get(name)
                        .map(|length| self.inference.resolve(length))
                    {
                        Some(Ty::Array {
                            kind: sequence::Kind::Array(length),
                            ..
                        }) => sequence::Kind::Array(length),
                        _ => sequence::Kind::Slice,
                    },
                },
            },
            // A tuple is an array at runtime, as `event::listen_with`'s
            // `(Event, Status, window::Id)`.
            Shape::Tuple(members) => Ty::Tuple(
                members
                    .iter()
                    .map(|member| {
                        self.instantiate_host_type(
                            member,
                            parameters,
                            operation,
                            span,
                            owner,
                            originating,
                        )
                    })
                    .collect::<Result<Vec<_>, _>>()?
                    .into_boxed_slice(),
            ),
        })
    }

    /// The key of the instance a call of a generic imported item uses:
    /// its types as `collect_imported_call` gives them to the bridge, which
    /// keys the adapter it generates for them by them, and the crate
    /// functions `items` it passes. `None` for a call of
    /// an item with no instance, which reaches the item's one adapter.
    ///
    /// A type no instance is keyed by is an error at the call, as rustc's
    /// own is: a call never reaches another instance's adapter.
    #[cfg(feature = "rust-import")]
    pub(super) fn imported_instance_key(
        &self,
        item: &crate::host::item::Definition,
        requirements: &[(ImmutableString, Ty, capability::Set)],
        slots: &[rawr_import::surface::instance::Slot],
        items: &[(u32, [u8; 32])],
        span: Span,
    ) -> Result<Option<[u8; 32]>, Error> {
        // A proof-backed application's call reaches its own adapter.
        if let Some(identity) = item.application() {
            return Ok(Some(*identity));
        }
        // A crate function an argument passes is part of the instance, as
        // the generator keys it: `.style(text::warning)` reaches its own
        // adapter, not `.style(text::default)`'s.
        if item.imported_identity().is_none()
            || (requirements.is_empty() && slots.is_empty() && items.is_empty())
        {
            return Ok(None);
        }
        let types = instance_parameters(item, requirements)
            .into_iter()
            .map(|(name, ty, _)| {
                let convert = if fixed_parameter(item, name) {
                    fixed_instance_surface_type
                } else {
                    instance_surface_type
                };
                let mut surface = convert(&self.inference, &self.models, ty).ok_or_else(|| {
                    keyless_type_argument(&self.inference, &self.models, item, name, ty, span)
                })?;
                fix_array_length(&mut surface, self.related_array_length(item, name, ty));
                Ok(surface)
            })
            .collect::<Result<Vec<_>, Error>>()?;
        let constants = self
            .imported_constants(item, requirements)
            .ok_or_else(|| unknown_const_argument(item, span))?;
        Ok(Some(rawr_import::surface::instance::key_with_items(
            &types, &constants, slots, items,
        )))
    }

    /// The values of an imported item's const parameters, in declared
    /// order: each the length of the array its argument gave it (#1330).
    /// `None` when one is unknown.
    #[cfg(feature = "rust-import")]
    pub(super) fn imported_constants(
        &self,
        item: &crate::host::item::Definition,
        requirements: &[(ImmutableString, Ty, capability::Set)],
    ) -> Option<Box<[rawr_import::classify::representability::convert::Constant]>> {
        item.signature()
            .declared_length_parameters()
            .map(|name| {
                let (_, ty, _) = requirements
                    .iter()
                    .find(|(candidate, _, _)| *candidate == name)?;
                let Ty::Array {
                    kind: sequence::Kind::Array(length),
                    ..
                } = self.inference.resolve(ty)
                else {
                    return None;
                };
                rawr_import::classify::representability::convert::Constant::checked(
                    "usize",
                    &length.to_string(),
                )
                .ok()
            })
            .collect()
    }
}

/// What a call of a program's function written `-> impl Trait` gave
/// (`Parser::opaque_values`): the value, the written output, and the types
/// the call gave the function's generic parameters, which that output
/// names.
#[cfg(feature = "rust-import")]
pub(super) struct OpaqueValue {
    pub(super) value: Ty,
    pub(super) written: type_ref::Type,
    pub(super) substitution: BTreeMap<ImmutableString, Ty>,
}
