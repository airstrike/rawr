//! The crate uses a body collects: imported operations and calls, the
//! instances they need, and the script functions that fill their callable
//! slots.

use super::*;

impl<'a> Parser<'a> {
    #[cfg(feature = "rust-import")]
    pub(super) fn retain_imported(
        &mut self,
        items: Option<usize>,
        bytes: Option<usize>,
    ) -> Result<(), Error> {
        self.imported
            .as_mut()
            .expect("imported evidence retains its allocation context")
            .retain_checked(items, bytes)
    }

    #[cfg(feature = "rust-import")]
    pub(super) fn imported_entries<T>(&mut self, count: usize) -> Result<Vec<T>, Error> {
        self.imported
            .as_mut()
            .expect("imported evidence retains its allocation context")
            .entries(count)
    }

    #[cfg(feature = "rust-import")]
    pub(super) fn reserve_imported_use(
        &mut self,
        path_bytes: usize,
        selector: &rawr_import::surface::identity::Selector,
    ) -> Result<usize, Error> {
        let retained = std::mem::size_of::<ImportedUse>()
            .checked_add(path_bytes)
            .and_then(|bytes| bytes.checked_add(selector.retained_bytes()?));
        let result = self
            .imported
            .as_mut()
            .expect("only an imported compilation reserves imported uses")
            .allocation
            .retain_checked(Some(1), retained);
        if let Err(error) = result {
            self.imported
                .as_mut()
                .expect("guard proves an imported allocation context")
                .failure
                .replace(error);
            return Err(Error::LoweringUnavailable {
                item: "<imported allocation>".into(),
                body: Span::NONE,
            });
        }
        let context = self
            .imported
            .as_mut()
            .expect("guard proves an imported allocation context");
        if context.uses.try_reserve_exact(1).is_err() {
            context
                .failure
                .replace(rawr_import::surface::Error::Allocation);
            return Err(Error::LoweringUnavailable {
                item: "<imported allocation>".into(),
                body: Span::NONE,
            });
        }
        Ok(context.uses.len())
    }

    #[cfg(feature = "rust-import")]
    pub(super) fn push_imported_op(
        &mut self,
        path: &str,
        selector: &rawr_import::surface::identity::Selector,
        operation: Option<ImportedOperation>,
        span: Span,
    ) -> Result<String, Error> {
        if self.imported.is_some() {
            let helper = self.collect_imported_op(path, selector, operation.as_ref(), span)?;
            if self.symbolic_only {
                return Ok(helper);
            }
        }
        let Some(operation) = operation else {
            return Ok(String::new());
        };
        Ok(self.push_op(Op::Imported {
            path: path.into(),
            selector: selector.clone(),
            operation,
            span,
        }))
    }

    #[cfg(feature = "rust-import")]
    pub(super) fn collect_imported_op(
        &mut self,
        path: &str,
        selector: &rawr_import::surface::identity::Selector,
        operation: Option<&ImportedOperation>,
        span: Span,
    ) -> Result<String, Error> {
        if let Some(operation) = operation {
            operation.collect_dependencies(
                self.imported
                    .as_mut()
                    .expect("imported lowering retains its dependency ledger")
                    .dependencies,
            );
        }
        // A use whose selector names no item, as a type's variant or field,
        // is told apart from another item's by its path, which a runner's
        // build must hold byte for byte. The compile a runner is built for
        // and the runner's own compile reach a re-exported item through
        // different registries, which can each take another route to it,
        // so each names the type by its one path: `iced::theme::Mode::
        // Light` is the variant of the one `Mode` whatever route it took.
        let path: ImmutableString = match selector {
            rawr_import::surface::identity::Selector::Declaration
            | rawr_import::surface::identity::Selector::StructConstructor(_)
            | rawr_import::surface::identity::Selector::StructField(_)
            | rawr_import::surface::identity::Selector::EnumVariant { .. } => self
                .models
                .host
                .imported_type_path(path)
                .map_or_else(|| path.into(), |route| route.to_string().into()),
            _ => path.into(),
        };
        let index = self.reserve_imported_use(path.len(), selector)?;
        let context = self
            .imported
            .as_mut()
            .expect("imported lowering retains its allocation context");
        if let Some(operation) = operation {
            let types = operation.instance(&self.inference);
            if !types.is_empty() {
                self.pending_structural.push((index, types, span));
            }
        }
        let selector = match selector.try_clone() {
            Ok(selector) => selector,
            Err(error) => {
                context.failure.replace(error);
                return Err(Error::LoweringUnavailable {
                    item: "<imported allocation>".into(),
                    body: Span::NONE,
                });
            }
        };
        context.uses.push(ImportedUse {
            source: context.source,
            path,
            selector,
            types: Box::default(),
            constants: Box::default(),
            functions: Box::default(),
            mappers: Box::default(),
            callable: false,
            slots: Box::default(),
            fillers: Box::default(),
            application: None,
            span,
        });
        Ok(format!("{SENTINEL}{index}"))
    }

    #[cfg(feature = "rust-import")]
    pub(super) fn finish_structural_instances(&mut self) -> Result<(), Error> {
        self.finish_projections();
        for (parameter, default, span) in std::mem::take(&mut self.structural_defaults) {
            if matches!(self.inference.resolve(&parameter), Ty::Infer(_)) {
                self.require_type(&parameter, &default, span)?;
            }
        }
        let Some(context) = self.imported.as_mut() else {
            return Ok(());
        };
        for (index, instance, span) in std::mem::take(&mut self.pending_structural) {
            let mut types = Vec::with_capacity(instance.len());
            for ty in instance.iter() {
                types.push(imported_surface_type(
                    &self.inference,
                    &self.models,
                    ty,
                    span,
                    context,
                )?);
            }
            context.uses[index].types = types.into_boxed_slice();
        }
        Ok(())
    }

    #[cfg(feature = "rust-import")]
    pub(super) fn validate_profile_mapper_output(
        &self,
        invocation: &crate::host::item::Definition,
        owner: &crate::host::facade::Dependency,
        mapper_output: &Ty,
        output: &Ty,
        span: Span,
    ) -> Result<(), Error> {
        // The call gives a crate type of the message the program's function
        // builds, as iced's `TextInput<Message>` from `on_input`; any other
        // shape is a use Rawr can't carry yet.
        let carried = matches!(mapper_output, Ty::Script(..))
            && matches!(
                self.inference.resolve(output),
                Ty::Host { proof, arguments }
                    if proof.owner.as_ref() == Some(owner)
                        && matches!(
                            arguments.as_ref(),
                            [Ty::ProfileMessage { script, seal, .. }]
                                if script.as_ref() == mapper_output && *seal != [0; 16]
                        )
            );
        if carried {
            Ok(())
        } else {
            Err(Error::InvalidHostOperation {
                item: invocation.path().into(),
                reason: UNSUPPORTED_MESSAGE_FUNCTION.into(),
                operation: span,
            })
        }
    }

    #[cfg(feature = "rust-import")]
    pub(super) fn collect_imported_ops(&mut self, ops: &[Op]) -> Result<(), Error> {
        if self.imported.is_some() {
            for op in ops {
                if let Op::Intrinsic { operation } = op {
                    if operation.item.imported_identity().is_some() {
                        self.collect_imported_call(operation)?;
                    } else if self.symbolic_only {
                        self.collect_symbolic_call(operation);
                    }
                }
            }
        }
        Ok(())
    }

    /// A crate value a call of the body takes or gives whose type still
    /// holds a variable now the body's types are settled, as the message of
    /// a `Row<'_, _>` no use fixes: rustc can't infer it either, so it is
    /// rustc's E0282 at the first `let` whose type holds the variable, or
    /// at the call for the item's parameter that holds it, never a runner
    /// that names `_` where cargo refuses it (#1404).
    #[cfg(feature = "rust-import")]
    pub(super) fn refuse_open_crate_values(&self, ops: &[Op]) -> Result<(), Error> {
        if self.imported.is_none() {
            return Ok(());
        }
        for op in ops {
            let Op::Intrinsic { operation } = op else {
                continue;
            };
            if operation.item.imported_identity().is_none() {
                continue;
            }
            let values = operation
                .operands
                .iter()
                .map(|(ty, at, _)| (ty, *at))
                .chain([(&operation.output, operation.span)]);
            for (ty, at) in values {
                let ty = self.inference.resolve(ty);
                if !matches!(ty, Ty::Host { .. }) {
                    continue;
                }
                let Some(unknown) = first_inference(&ty) else {
                    continue;
                };
                return Err(self.unsettled(&unknown, at, None).unwrap_or_else(|| {
                    let parameter = operation
                        .requirements
                        .iter()
                        .find(|(_, ty, _)| {
                            contains(&self.inference.resolve(ty), &|inner| *inner == unknown)
                        })
                        .map_or_else(
                            || ty.name(&self.models),
                            // As rustc spells it, as `impl Into<Size>`.
                            |(name, ..)| operation.item.signature().shown(name).into(),
                        );
                    Error::TypeAnnotationsNeeded {
                        parameter,
                        reference: at,
                    }
                }));
            }
        }
        Ok(())
    }

    /// Symbolic lowering keeps no ops, so it records the facade dependencies the
    /// executable call to a facade item would carry.
    #[cfg(feature = "rust-import")]
    pub(super) fn collect_symbolic_call(&mut self, operation: &IntrinsicOperation) {
        let dependencies = &mut *self
            .imported
            .as_mut()
            .expect("imported lowering retains its dependency ledger")
            .dependencies;
        dependencies.insert(operation.owner.clone());
        for (ty, _, _) in operation.operands.iter() {
            if !ty.is_imported_static_operand() {
                self.inference
                    .resolve(ty)
                    .collect_dependencies(dependencies);
            }
        }
        self.inference
            .resolve(&operation.output)
            .collect_dependencies(dependencies);
    }

    #[cfg(feature = "rust-import")]
    pub(super) fn collect_imported_call(
        &mut self,
        operation: &IntrinsicOperation,
    ) -> Result<(), Error> {
        let IntrinsicOperation {
            item,
            owner,
            requirements,
            operands,
            output,
            span,
            ..
        } = operation;
        let span = *span;
        let arguments = operands.iter().filter(|(_, _, receiver)| !*receiver);
        {
            let dependencies = &mut *self
                .imported
                .as_mut()
                .expect("imported lowering retains its dependency ledger")
                .dependencies;
            dependencies.insert(owner.clone());
            for (_, ty, _) in requirements.iter() {
                self.inference
                    .resolve(ty)
                    .collect_dependencies(dependencies);
            }
            for (ty, _, _) in operands.iter() {
                self.inference
                    .resolve(ty)
                    .collect_dependencies(dependencies);
            }
            self.inference
                .resolve(output)
                .collect_dependencies(dependencies);
        }
        let path_bytes = item
            .imported_path_len()
            .expect("guard proves an imported item path");
        let selector = item
            .imported_identity()
            .expect("guard proves an imported selector");
        self.reserve_imported_use(path_bytes, selector)?;
        let mapper_count = arguments
            .clone()
            .filter(|(ty, _, _)| {
                matches!(self.inference.resolved_ref(ty), Ty::ProfileMapper { .. })
            })
            .count();
        let mut mappers = self.imported_entries(mapper_count)?;
        for (position, (ty, argument_span, _)) in arguments.clone().enumerate() {
            let Ty::ProfileMapper {
                output: mapper_output,
                seal,
                ..
            } = self.inference.resolve(ty)
            else {
                continue;
            };
            self.validate_profile_mapper_output(
                item,
                owner,
                &mapper_output,
                output,
                *argument_span,
            )?;
            mappers.push(ImportedMapperUse {
                position: u32::try_from(position).expect("a call's argument count fits in u32"),
                seal,
                span: *argument_span,
            });
        }
        if mappers.len() > 1 {
            return Err(Error::InvalidHostOperation {
                item: item.path().into(),
                reason: "passing more than one of the program's functions that build its \
                         message to one call is not supported in Rawr yet"
                    .into(),
                operation: span,
            });
        }
        // The instance's types in the item's declared parameter order, and
        // its const lengths (#1330).
        let constants = self
            .imported_constants(item, requirements)
            .ok_or_else(|| unknown_const_argument(item, span))?;
        let ordered = instance_parameters(item, requirements);
        let mut types = self.imported_entries(ordered.len())?;
        for (name, ty, _) in ordered {
            if reaches_param(&self.inference.resolve(ty)) {
                self.reaches_host = true;
            }
            let length = self.related_array_length(item, name, ty);
            let convert = if fixed_parameter(item, name) {
                fixed_surface_type
            } else {
                imported_surface_type
            };
            let mut surface = convert(
                &self.inference,
                &self.models,
                ty,
                span,
                self.imported
                    .as_mut()
                    .expect("imported lowering retains allocation authority"),
            )
            .map_err(|error| match error {
                // Which of the item's parameters it is.
                Error::InvalidHostOperation {
                    reason, operation, ..
                } => Error::InvalidHostOperation {
                    item: format!("{}'s `{name}`", item.path()).into(),
                    reason,
                    operation,
                },
                error => error,
            })?;
            fix_array_length(&mut surface, length);
            types.push(surface);
        }
        let types = types.into_boxed_slice();
        let function_count = arguments
            .clone()
            .filter(|(ty, _, _)| {
                matches!(
                    self.inference.resolved_ref(ty),
                    Ty::ImportedFunctionItem { item: Some(_), .. }
                )
            })
            .count();
        let mut functions = self.imported_entries(function_count)?;
        for (position, (ty, _, _)) in arguments.enumerate() {
            let Ty::ImportedFunctionItem {
                item: Some(function),
                ..
            } = self.inference.resolved_ref(ty)
            else {
                continue;
            };
            let selector = function
                .item
                .imported_identity()
                .expect("function-item proof retains an imported selector");
            let bytes = function
                .item
                .imported_path_len()
                .and_then(|bytes| bytes.checked_add(selector.retained_bytes()?));
            self.imported
                .as_mut()
                .expect("imported lowering retains allocation authority")
                .retain_checked(Some(0), bytes)?;
            let mut instance = Vec::new();
            if instance
                .try_reserve_exact(function.parameters.len())
                .is_err()
            {
                return Err(self
                    .imported
                    .as_mut()
                    .expect("imported lowering retains allocation authority")
                    .allocation_failure(rawr_import::surface::Error::Allocation));
            }
            for (name, ty) in &function.parameters {
                instance.push(
                    imported_surface_type(
                        &self.inference,
                        &self.models,
                        ty,
                        function.span,
                        self.imported
                            .as_mut()
                            .expect("imported lowering retains allocation authority"),
                    )
                    .map_err(|error| match error {
                        Error::InvalidHostOperation {
                            reason, operation, ..
                        } => Error::InvalidHostOperation {
                            item: format!("{}'s `{name}`", function.item.path()).into(),
                            reason,
                            operation,
                        },
                        error => error,
                    })?,
                );
            }
            functions.push(ImportedFunctionUse {
                types: instance.into_boxed_slice(),
                position: u32::try_from(position).expect("a call's argument count fits in u32"),
                path: function.item.path().into(),
                selector: function
                    .item
                    .imported_identity()
                    .expect("function-item proof retains an imported selector")
                    .try_clone()
                    .map_err(|error| {
                        self.imported
                            .as_mut()
                            .expect("imported lowering retains allocation authority")
                            .failure
                            .replace(error);
                        Error::LoweringUnavailable {
                            item: item.path().into(),
                            body: span,
                        }
                    })?,
                span: function.span,
            });
        }
        let slots = self.crossing_slots(
            item,
            operands
                .iter()
                .filter(|(_, _, receiver)| !*receiver)
                .map(|(ty, _, _)| ty.clone()),
        );
        let context = self
            .imported
            .as_mut()
            .expect("imported lowering retains its allocation context");
        let selector = selector.try_clone().map_err(|error| {
            context.failure.replace(error);
            Error::LoweringUnavailable {
                item: item.path().into(),
                body: span,
            }
        })?;
        context.uses.push(ImportedUse {
            source: context.source,
            path: item.path().into(),
            selector,
            types,
            constants,
            functions: functions.into_boxed_slice(),
            mappers: mappers.into_boxed_slice(),
            callable: item.signature().takes_callable(),
            slots: slots.iter().map(|slot| slot.served()).collect(),
            fillers: slots,
            application: item.application().copied(),
            span,
        });
        Ok(())
    }

    /// The script functions that fill `item`'s callable parameters as
    /// slots (D594): each named function or closure that captures nothing,
    /// by its position among the call's arguments. A closure that captures
    /// crosses as a Rust closure that holds what it captured, in a slot of
    /// its own kind ([`Slot::closure`]).
    ///
    /// [`Slot::closure`]: rawr_import::surface::instance::Slot::closure
    #[cfg(feature = "rust-import")]
    pub(super) fn crossing_slots(
        &self,
        item: &crate::host::item::Definition,
        arguments: impl Iterator<Item = Ty>,
    ) -> Box<[rawr_import::surface::instance::Slot]> {
        let crossing = item.signature().crossing();
        // An item of one callable parameter takes a closure that captures
        // in its one closure class; one of several, as iced's `Task::sip`,
        // in a slot of its own kind at each parameter, as the classifier
        // has no one class for it.
        let several = item
            .signature()
            .parameters()
            .iter()
            .filter(|parameter| {
                matches!(
                    parameter.shape(),
                    crate::host::type_ref::Shape::Callable { .. }
                )
            })
            .count()
            > 1;
        arguments
            .enumerate()
            .filter_map(|(position, ty)| {
                // A crate's own callable value fills the parameter as
                // itself, boxed (`Slot::native`).
                if self.native_callable(&ty).is_some()
                    && item
                        .signature()
                        .parameters()
                        .get(position)
                        .is_some_and(|parameter| {
                            matches!(
                                parameter.shape(),
                                crate::host::type_ref::Shape::Callable { .. }
                            )
                        })
                {
                    return Some(rawr_import::surface::instance::Slot::native(
                        u32::try_from(position).unwrap_or(u32::MAX),
                    ));
                }
                if !crossing {
                    return None;
                }
                let Ty::Callable {
                    item: Some(function),
                    ..
                } = self.inference.resolve(&ty)
                else {
                    return None;
                };
                let captures = self
                    .lifted
                    .iter()
                    .find(|closure| closure.key == *function.key())
                    .is_some_and(|closure| !closure.captures.is_empty());
                if captures && !several {
                    return None;
                }
                let position = u32::try_from(position).unwrap_or(u32::MAX);
                Some(if captures {
                    rawr_import::surface::instance::Slot::closure(position)
                } else {
                    rawr_import::surface::instance::Slot::new(
                        position,
                        &format!("{:?}", function.key()),
                    )
                })
            })
            .collect()
    }

    /// The crate functions `arguments` pass, each by its input's position
    /// and its selector's fingerprint, as `.style(theme::text::base)`
    /// passes one: its instance's own, as the generated adapter keys it
    /// (`instance::key_with_items`).
    #[cfg(feature = "rust-import")]
    pub(super) fn function_items(
        &self,
        arguments: impl Iterator<Item = Ty>,
    ) -> Box<[(u32, [u8; 32])]> {
        arguments
            .enumerate()
            .filter_map(|(position, ty)| {
                let Ty::ImportedFunctionItem {
                    item: Some(function),
                    ..
                } = self.inference.resolve(&ty)
                else {
                    return None;
                };
                let selector = function.item.imported_identity()?.fingerprint();
                Some((u32::try_from(position).unwrap_or(u32::MAX), selector))
            })
            .collect()
    }
}
