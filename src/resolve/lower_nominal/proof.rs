//! Proofs of the executable operations a body pushes: each operation's
//! types proven against what the runtime provides.

use super::*;

impl<'a> Parser<'a> {
    pub(super) fn prove_ops(&mut self, ops: Vec<Op>) -> Result<Vec<ExecutableOp>, Error> {
        ops.into_iter()
            .enumerate()
            .map(|(index, op)| {
                let witnesses = self.witnessed_ops.get(&index).cloned();
                if let Some(names) = &witnesses {
                    *self.template_params.borrow_mut() = names.clone();
                }
                let op = self.prove_executable_op(op);
                let Some(names) = witnesses else {
                    return op;
                };
                *self.template_params.borrow_mut() = Box::default();
                Ok(ExecutableOp::Witnessed(Box::new(WitnessedOp::new(
                    op?,
                    names.len(),
                ))))
            })
            .collect()
    }

    pub(super) fn finish_ops(&mut self) -> Result<Vec<ExecutableOp>, Error> {
        #[cfg(feature = "rust-import")]
        self.finish_structural_instances()?;
        let ops = std::mem::take(&mut self.ops);
        #[cfg(feature = "rust-import")]
        self.collect_imported_ops(&ops)?;
        #[cfg(feature = "rust-import")]
        self.refuse_open_crate_values(&ops)?;
        if self.symbolic_only {
            Ok(Vec::new())
        } else {
            self.prove_ops(ops)
        }
    }

    pub(super) fn prove_executable_op(&self, op: Op) -> Result<ExecutableOp, Error> {
        Ok(match op {
            #[cfg(feature = "rust-import")]
            Op::Imported {
                path,
                selector,
                operation,
                span,
            } => {
                let types = operation
                    .instance(&self.inference)
                    .iter()
                    .map(|ty| instance_surface_type(&self.inference, &self.models, ty))
                    .collect::<Option<Vec<_>>>()
                    .ok_or_else(|| Error::InvalidHostOperation {
                        item: path.clone(),
                        reason: "type annotations needed: a type argument of it is unknown (E0282)"
                            .into(),
                        operation: span,
                    })?;
                let instance =
                    (!types.is_empty()).then(|| rawr_import::surface::instance::key(&types));
                // Whether a field is read as a copy is decided at the field's
                // type once the body has fixed it, as rustc decides it: a
                // struct update's `..base` can read a field before a later
                // use fixes its type.
                let mut operation = operation;
                if let crate::host::structural::Kind::Field { copy } = &mut operation.kind
                    && !*copy
                    && self.type_provides(&operation.output, capability::Kind::Copy)
                {
                    *copy = true;
                    operation.receiver = Some(crate::receiver::Mode::Shared);
                }
                let definition = self
                    .models
                    .host
                    .structural(&path, &selector, operation.kind, instance)
                    .cloned()
                    .ok_or_else(|| {
                        let error = Error::InvalidHostOperation {
                            item: path.clone(),
                            reason: format!(
                                "{} is not supported in Rawr yet",
                                structural_use(operation.kind)
                            )
                            .into(),
                            operation: span,
                        };
                        if let Some(site) = self.native_site() {
                            site.record(&error);
                        }
                        error
                    })?;
                assert_eq!(
                    definition.kind(),
                    operation.kind,
                    "the structural adapter found for an operation is of its kind"
                );
                let operands = operation
                    .operands
                    .iter()
                    .map(|(ty, operand, receiver)| {
                        self.proven_type(ty, definition.path(), *operand).map(|ty| {
                            crate::host::structural::Operand::new(ty, *operand, *receiver)
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?
                    .into_boxed_slice();
                let output = self.proven_type(&operation.output, definition.path(), span)?;
                let proof = crate::host::structural::Proof::transform(
                    definition,
                    operands,
                    output,
                    operation.receiver,
                    span,
                )
                .expect("a structural adapter takes the operands its operation was generated for");
                ExecutableOp::Imported { proof }
            }
            Op::Symbolic {
                value,
                captures,
                changes,
            } => ExecutableOp::Symbolic {
                value,
                captures: usize::try_from(captures).unwrap_or(usize::MAX),
                changes,
            },
            Op::Intrinsic { operation } => {
                let IntrinsicOperation {
                    facade,
                    owner,
                    item,
                    requirements,
                    operands,
                    output,
                    span,
                } = *operation;
                for (_, ty, required) in requirements.iter() {
                    let ty = self.inference.resolve(ty);
                    // A parameter no script value fixes is rustc's to infer
                    // and to check (D594). So is one a crate's own type
                    // fills, as a trait's default `Theme` does: the facade
                    // knows its derives only where a program uses them, and
                    // rustc checks the bound where it compiles the call.
                    #[cfg(feature = "rust-import")]
                    if item.imported_identity().is_some()
                        && match &ty {
                            Ty::Infer(_) => true,
                            Ty::Host { proof, .. } => {
                                proof.definition.identity().imported().is_some()
                            }
                            _ => false,
                        }
                    {
                        continue;
                    }
                    for capability in required.iter() {
                        if !self.type_provides(&ty, capability) {
                            return Err(Error::MissingCapability {
                                owner: ty.name(&self.models),
                                capability: capability.name().into(),
                                operation: span,
                                derive: matches!(ty, Ty::Script(..)),
                                declared: self.declared_name(&ty),
                                required_by: Some(crate::resolve::Required::by(
                                    crate::host::path::public(&item.path()).as_ref(),
                                )),
                                formatted: None,
                                chain: crate::resolve::Chain::default(),
                            });
                        }
                    }
                }
                let item_path = item.path();
                let output = self.proven_type(&output, &item_path, span)?;
                // A host call gives back each `&mut` array and string it
                // copies in after its value (`copied_places`).
                let copies = operands
                    .iter()
                    .filter(|(ty, _, receiver)| !receiver && copied_input(&self.inference, ty))
                    .count()
                    .try_into()
                    .unwrap_or(u8::MAX);
                #[cfg(feature = "rust-import")]
                let instance = self.imported_instance_key(
                    &item,
                    &requirements,
                    &self
                        .crossing_slots(
                            &item,
                            operands
                                .iter()
                                .filter(|(_, _, receiver)| !*receiver)
                                .map(|(ty, _, _)| ty.clone()),
                        )
                        .iter()
                        .map(|slot| slot.served())
                        .collect::<Box<[_]>>(),
                    &self.function_items(
                        operands
                            .iter()
                            .filter(|(_, _, receiver)| !*receiver)
                            .map(|(ty, _, _)| ty.clone()),
                    ),
                    span,
                )?;
                let operands = operands
                    .iter()
                    .filter(|(ty, _, _)| !ty.is_imported_static_operand())
                    .map(|(ty, operand, receiver)| {
                        self.proven_type(ty, &item_path, *operand)
                            .map(|ty| intrinsic::Operand::new(ty, *operand, *receiver))
                    })
                    .collect::<Result<Vec<_>, _>>()?
                    .into_boxed_slice();
                #[cfg(not(feature = "rust-import"))]
                let instance = None;
                let caller = self.call_site(span);
                ExecutableOp::Intrinsic {
                    proof: intrinsic::Proof::new(facade, owner, &item, operands, output, span)
                        .with_instance(instance)
                        .with_copies(copies)
                        .with_caller(caller),
                }
            }
            Op::Projection { operation } => {
                let ProjectionOperation {
                    key,
                    source,
                    target,
                    span,
                } = *operation;
                let name = key.diagnostic_name();
                let source = self.proven_type(&source, &name, span)?;
                let target = self.proven_type(&target, &name, span)?;
                ExecutableOp::Projection {
                    proof: Box::new(crate::host::projection::Site::new(
                        key, source, target, span,
                    )),
                }
            }
            Op::Struct { definition, item } => ExecutableOp::Struct {
                definition,
                item: *item,
            },
            Op::Enum {
                definition,
                item,
                variant,
            } => ExecutableOp::Enum {
                definition,
                item: *item,
                variant,
            },
            Op::Clone { definition } => ExecutableOp::Clone { definition },
            Op::Debug { definition } => ExecutableOp::Debug { definition },
            Op::Equal { definition } => ExecutableOp::Equal { definition },
            Op::Default { value, definition } => ExecutableOp::Default { value, definition },
            #[cfg(feature = "rust-import")]
            Op::Sequence { target, span } => {
                let target = self.inference.resolve(&target);
                let concrete = instance_surface_type(&self.inference, &self.models, &target);
                let proof = concrete.as_ref().and_then(|_| {
                    self.proven_type(&target, "native sequence constructor", span)
                        .ok()
                });
                ExecutableOp::Sequence(sequence::Constructor::checked(concrete.as_ref(), proof))
            }
            #[cfg(feature = "rust-import")]
            Op::CallableObject { call, span } => {
                let call = self.inference.resolve(&call);
                ExecutableOp::CallableObject(callable_object::Coercion::new(
                    self.proven_type(&call, "a callable coerced to a trait object", span)
                        .ok(),
                ))
            }
            Op::Prelude(op) => ExecutableOp::Prelude(op),
            Op::Collect { target } => match self.inference.resolve(&target) {
                Ty::String => ExecutableOp::Prelude(PreludeOp::CollectString),
                Ty::Keyed(kind, arguments) => {
                    let op = if kind.is_map() {
                        PreludeOp::MapFrom
                    } else {
                        PreludeOp::SetFrom
                    };
                    return self.prove_executable_op(self.keyed_op(op, kind, &arguments[0]));
                }
                // Into a map or a set of the payloads, once none failed.
                Ty::Result(payload, _) | Ty::Option(payload)
                    if let Ty::Keyed(kind, arguments) = self.inference.resolve(&payload) =>
                {
                    let keyed = if kind.is_map() {
                        PreludeOp::MapFrom
                    } else {
                        PreludeOp::SetFrom
                    };
                    let Op::Shaped { op, ty, flags } = self.keyed_op(keyed, kind, &arguments[0])
                    else {
                        unreachable!("a keyed operation is shaped");
                    };
                    let tried = match self.inference.resolve(&target) {
                        Ty::Result(..) => SHAPED_RESULT,
                        _ => SHAPED_OPTION,
                    };
                    return self.prove_executable_op(Op::Shaped {
                        op,
                        ty,
                        flags: flags | tried,
                    });
                }
                Ty::Result(payload, _) => ExecutableOp::Kinded {
                    op: PreludeOp::CollectTry,
                    kinds: [COLLECT_RESULT, kind_code(self.inference.resolve(&payload))],
                },
                Ty::Option(payload) => ExecutableOp::Kinded {
                    op: PreludeOp::CollectTry,
                    kinds: [COLLECT_OPTION, kind_code(self.inference.resolve(&payload))],
                },
                _ => ExecutableOp::Prelude(PreludeOp::CollectVec),
            },
            Op::CollectStop { target } => ExecutableOp::Kinded {
                op: PreludeOp::CollectStop,
                kinds: [
                    match self.inference.resolve(&target) {
                        Ty::Result(..) => COLLECT_RESULT,
                        Ty::Option(_) => COLLECT_OPTION,
                        _ => 0,
                    },
                    0,
                ],
            },
            Op::Kinded { op, kinds } => ExecutableOp::Kinded {
                op,
                kinds: kinds.map(|kind| kind_code(self.inference.resolve(&kind))),
            },
            Op::Shaped { op, ty, flags } => {
                let resolve = |ty: &Ty| self.inference.resolve(ty);
                let shape = if flags & SHAPED_TUPLES != 0 {
                    Shape::with_tuples(&ty, &resolve)
                } else {
                    Shape::of(&ty, &resolve)
                };
                let flags = flags & !SHAPED_TUPLES;
                let flags = if flags & SHAPED_HASHED != 0 && !self.hand_written_eq_inside(&ty) {
                    flags & !SHAPED_HASHED
                } else {
                    flags
                };
                if shape.is_plain() && flags == 0 {
                    ExecutableOp::Prelude(op)
                } else {
                    ExecutableOp::Shaped { op, shape, flags }
                }
            }
            Op::Validate { definition } => ExecutableOp::Validate { definition },
            Op::MatchSubject { definition, item } => ExecutableOp::MatchSubject {
                definition,
                item: *item,
            },
        })
    }

    /// Where the program makes the call at `span`, as rustc places it: a
    /// method call's name, else the call's start. A crate's
    /// `#[track_caller]` function the call runs sees it as its caller, as
    /// `file!()`, `line!()` and `column!()` there name it.
    fn call_site(&self, span: Span) -> Option<Box<crate::std_facade::panic::Site>> {
        let key = self.models.sources.key(self.models.context.index());
        let source = self.models.sources.resolve(key).ok()?;
        let (line, column) = crate::diagnostic::call_position(source.text(), span)?;
        Some(Box::new(crate::std_facade::panic::Site::new(
            self.file_name(),
            u32::try_from(line).ok()?,
            u32::try_from(column).ok()?,
        )))
    }

    /// Whether `ty` has `capability`. A std trait of a type std declares is
    /// std's impl of it, selected from the std table as rustc selects it
    /// (D711 step 3), where the table states the impl (`std_states`). The
    /// rest are rawr's facts
    /// about its own values (`Send`, `'static`), its iterators, and the
    /// program's impls, derives and bounds.
    pub(super) fn type_provides(&self, ty: &Ty, capability: capability::Kind) -> bool {
        if let Some(parameter) = self.assumed(ty)
            && self.type_provides(&parameter, capability)
        {
            return true;
        }
        let ty = self.inference.resolve(ty);
        if let Some(definition) = capability.std_trait()
            && std_states(&ty, capability)
            && std_impl::declares(&ty)
        {
            return std_impl::holds(&mut std_impl::Probe(self), definition, &ty, &[], true, 0)
                .is_ok();
        }
        match ty {
            // `()` is `Clone` and `Copy` as a tuple is, and no iterator or
            // error.
            Ty::Unit => !matches!(
                capability,
                capability::Kind::Iterator | capability::Kind::Error
            ),
            // `!` implements what core implements it for: it is `Copy`,
            // prints, compares, orders and hashes, but has no `Default`
            // and is no iterator. A type holding it is `Copy` as its other
            // parts are, as `Option<!>` is once the fallback makes it.
            Ty::Never => !matches!(
                capability,
                capability::Kind::Default | capability::Kind::Iterator
            ),
            // Function pointers and items are `Copy`, as in Rust: a host's
            // callable and an imported function are its Rust items.
            Ty::Fn { .. } | Ty::FnChoice { .. } | Ty::Callable { .. } => {
                matches!(capability, capability::Kind::Clone | capability::Kind::Copy)
            }
            #[cfg(feature = "rust-import")]
            Ty::ImportedFunctionItem { .. } => {
                matches!(capability, capability::Kind::Clone | capability::Kind::Copy)
            }
            // A closure is `Copy` unless it owns a capture that is not, or
            // captures by `&mut`.
            Ty::FnItem { key, .. } => match capability {
                capability::Kind::Clone => true,
                capability::Kind::Copy => !self.owning_closures.contains(&*key),
                _ => false,
            },
            // `[T; N]`'s `Default`, which std implements for each length
            // up to 32, never for every `N`.
            Ty::Array {
                element,
                kind: sequence::Kind::Array(length),
            } if capability == capability::Kind::Default => match length.fixed() {
                Some(0) => true,
                Some(length) => length <= 32 && self.type_provides(&element, capability),
                None => false,
            },
            Ty::Array { element, .. } => {
                !matches!(
                    capability,
                    capability::Kind::Iterator | capability::Kind::Error
                ) && self.type_provides(&element, capability)
            }
            // A tuple is `Clone` and `Copy` as rustc makes it, of members
            // that are.
            Ty::Tuple(members) => {
                !matches!(
                    capability,
                    capability::Kind::Iterator | capability::Kind::Error
                ) && members
                    .iter()
                    .all(|member| self.type_provides(member, capability))
            }
            Ty::Option(element) => {
                !matches!(
                    capability,
                    capability::Kind::Iterator | capability::Kind::Error
                ) && self.type_provides(&element, capability)
            }
            Ty::Result(ok, err) => {
                !matches!(
                    capability,
                    capability::Kind::Iterator | capability::Kind::Error
                ) && self.type_provides(&ok, capability)
                    && self.type_provides(&err, capability)
            }
            #[cfg(feature = "rust-import")]
            Ty::ProfileMapper { .. } => false,
            #[cfg(feature = "rust-import")]
            Ty::ProfileMessage { payload, .. } => self.type_provides(&payload, capability),
            Ty::Bool => crate::capability::builtin_supports("bool", capability),
            Ty::Char => crate::capability::builtin_supports("char", capability),
            Ty::Int(_) => crate::capability::builtin_supports("i64", capability),
            Ty::Float(_) => crate::capability::builtin_supports("f64", capability),
            Ty::String | Ty::UnsizedStr | Ty::Str => {
                crate::capability::builtin_supports("String", capability)
            }
            // A range is an iterator where its element steps.
            range @ (Ty::InclusiveRange(_)
            | Ty::FloatRange
            | Ty::Range(_)
            | Ty::InclusiveOf(_)
            | Ty::OpenRange(..)) => self
                .range_provides(&range, capability)
                .expect("a range type has std's range parts"),
            Ty::ParseError(error) => crate::capability::builtin_supports(error.name(), capability),
            // `fmt::Arguments<'_>` is `Copy` and writes its text.
            Ty::Arguments => matches!(
                capability,
                capability::Kind::Clone | capability::Kind::Copy | capability::Kind::Debug
            ),
            Ty::Ordering => crate::capability::builtin_supports("Ordering", capability),
            Ty::Time(time) => crate::capability::builtin_supports(time.name(), capability),
            // A borrow is neither `Send` nor `'static` by rawr's facts.
            Ty::Reference { .. } => false,
            // Maps and sets are `Send` and `'static` as their keys and
            // values are.
            Ty::Keyed(_, arguments) => {
                !matches!(
                    capability,
                    capability::Kind::Iterator | capability::Kind::Error
                ) && arguments
                    .iter()
                    .all(|argument| self.type_provides(argument, capability))
            }
            Ty::Reverse(inner) => self.type_provides(&inner, capability),
            Ty::Cow(ref borrowed) => match capability {
                capability::Kind::Send | capability::Kind::Static => {
                    self.type_provides(borrowed, capability)
                }
                _ => false,
            },
            // A `Mutex<T>`, `RefCell<T>` or `Cell<T>` is `Send` and
            // `'static` when `T` is.
            Ty::Cell(_, inner) => {
                matches!(
                    capability,
                    capability::Kind::Send | capability::Kind::Static
                ) && self.type_provides(&inner, capability)
            }
            // std's thread and channel handles are `Send` and `'static` as
            // their values are.
            Ty::Thread(kind, inner) => match capability {
                capability::Kind::Debug => true,
                capability::Kind::Clone => kind.clones(),
                capability::Kind::Send if !kind.sends() => true,
                capability::Kind::Send | capability::Kind::Static => {
                    self.type_provides(&inner, capability)
                }
                _ => false,
            },
            // Rawr's iterators are iterators, and `Clone` when their items
            // are.
            Ty::Iter(item, _, _) => {
                capability == capability::Kind::Iterator
                    || capability == capability::Kind::Clone
                        && self.type_provides(&item, capability)
            }
            // `Box<T>` is what `T` is.
            Ty::Boxed(target) => self.type_provides(&target, capability),
            // An `Rc` stays on its thread, and an `Arc<T>` is `Send` when
            // `T` is `Send` and `Sync`.
            Ty::Counted(counter, target) => match (counter, capability) {
                (Counter::Rc, capability::Kind::Send) => false,
                (Counter::Arc, capability::Kind::Send) => {
                    self.type_provides(&target, capability) && self.shared_between_threads(&target)
                }
                _ => self.type_provides(&target, capability),
            },
            Ty::Weak(counter, target) => match (counter, capability) {
                (Counter::Arc, capability::Kind::Send) => {
                    self.type_provides(&target, capability) && self.shared_between_threads(&target)
                }
                (_, capability::Kind::Static) => self.type_provides(&target, capability),
                _ => false,
            },

            // A trait object has only its trait's methods: a std trait's
            // object has the capability its trait or a supertrait is
            // (`dyn Debug`, `dyn Error`'s `Debug`), and std's `impl Debug
            // for dyn Any`.
            dynamic @ Ty::Dyn(..) => {
                capability == capability::Kind::Debug
                    && (self.std_error(&dynamic, false) || self.std_any(&dynamic, false))
                    || self.type_implements(&dynamic, capability.name())
            }
            // std's `Poll<T>` derives what it is from `T`, its `Copy`,
            // `Clone`, `Debug`, comparisons and `Hash`, as an `Option` does.
            Ty::Host { proof, arguments }
                if proof.definition.path().as_str() == crate::future::facade::POLL =>
            {
                !matches!(
                    capability,
                    capability::Kind::Default
                        | capability::Kind::Iterator
                        | capability::Kind::Error
                ) && arguments
                    .iter()
                    .all(|argument| self.type_provides(argument, capability))
            }
            Ty::Host { proof, .. } => proof.definition.capabilities().contains(capability),
            // A hand-written impl holds where its own bounds do.
            Ty::Script(id, arguments) if self.implemented_by_hand(capability.name(), id) => {
                self.impl_holds(capability.name(), id, &arguments)
            }
            // A derive on a generic type holds when its arguments provide
            // the capability too, as derive's bounds require.
            Ty::Script(id, arguments) => {
                self.models
                    .definition(id)
                    .capabilities()
                    .contains(capability)
                    && arguments
                        .iter()
                        .all(|argument| self.type_provides(argument, capability))
            }
            // A generic parameter has exactly its bounds' capabilities, and
            // an opaque output its written bounds' (D1109).
            Ty::Param(_, Parameter::Opaque(captured)) => self
                .opaque_bounds(&captured.opaque)
                .is_some_and(|generic| generic.bounds.contains(capability)),
            Ty::Param(name, _) => self
                .generics
                .get(&name)
                .is_some_and(|bounds| bounds.contains(capability)),
            // An associated type of an unknown type: only its declared
            // bounds (`type Output: Clone;`), as in Rust.
            Ty::Projection(_, trait_name, name) => self
                .associated_bounds(&trait_name, &name)
                .is_some_and(|(bounds, _)| bounds.contains(capability)),
            // A const argument is a value, which no derive's bound asks
            // anything of.
            Ty::Const(_) => true,
            Ty::Infer(_) | Ty::Unknown => false,
        }
    }

    pub(super) fn proven_type(
        &self,
        ty: &Ty,
        operation: &str,
        span: Span,
    ) -> Result<intrinsic::Type, Error> {
        let ty = self.inference.resolve(ty);
        Ok(match ty {
            // A `-> !` item gives no value, so its output's witness is
            // never checked against one.
            Ty::Unit | Ty::Never => intrinsic::Type::unit(),
            Ty::Callable {
                inputs,
                output,
                item: Some(item),
            } => intrinsic::Type::callable(
                inputs
                    .iter()
                    .map(|input| self.proven_type(input, operation, span))
                    .collect::<Result<_, _>>()?,
                self.proven_type(&output, operation, span)?,
                *item,
            ),
            Ty::Array { element, kind } => {
                intrinsic::Type::array(self.proven_type(&element, operation, span)?, kind.length())
            }
            // An iterator a host gives is the array of its items.
            Ty::Iter(element, _, _) => {
                intrinsic::Type::array(self.proven_type(&element, operation, span)?, None)
            }
            // `None` where the host takes an `Option` of a callable: no item
            // is ever held, so no `Some` passes the proof.
            Ty::Option(element)
                if matches!(
                    self.inference.resolve(&element),
                    Ty::Callable { item: None, .. }
                ) =>
            {
                intrinsic::Type::option(intrinsic::Type::unit())
            }
            Ty::Option(element) => {
                intrinsic::Type::option(self.proven_type(&element, operation, span)?)
            }
            Ty::Result(ok, err) => intrinsic::Type::result(
                self.proven_type(&ok, operation, span)?,
                self.proven_type(&err, operation, span)?,
            ),
            // A tuple the host hands over, as `event::listen_with`'s.
            Ty::Tuple(members) => intrinsic::Type::tuple(
                members
                    .iter()
                    .map(|member| self.proven_type(member, operation, span))
                    .collect::<Result<_, _>>()?,
            ),
            // A map or set a crate takes or gives (#777): its array of
            // `[key, value]` pairs, or of keys.
            Ty::Keyed(kind, ref members) => {
                let members = members
                    .iter()
                    .map(|member| self.proven_type(member, operation, span))
                    .collect::<Result<Box<[_]>, _>>()?;
                let entry = if kind.is_map() {
                    intrinsic::Type::tuple(members)
                } else {
                    members
                        .into_vec()
                        .pop()
                        .ok_or_else(|| unpassable(ty.name(&self.models), operation, span))?
                };
                intrinsic::Type::array(entry, None)
            }
            // A `Cow<str>` a crate's `Cow<'_, str>` input takes (#1390):
            // std's runtime `Cow`, whose text the binding reads.
            Ty::Cow(ref borrowed) if matches!(**borrowed, Ty::UnsizedStr) => {
                intrinsic::Type::builtin("Cow<str>", TypeId::of::<cow::Value>())
            }
            Ty::Reverse(_) | Ty::Cow(_) => {
                return Err(unpassable(ty.name(&self.models), operation, span));
            }
            // A `Box` is its value at runtime (D492.3), which a crate's
            // `Box<T>` boxes where it takes it.
            Ty::Boxed(inner) => self.proven_type(&inner, operation, span)?,
            // An `Rc` or `Arc` of a crate's trait object is the object, as
            // a `Box` of one is: native code makes it (#1304).
            Ty::Counted(_, inner)
                if matches!(self.inference.resolve(&inner),
                    Ty::Dyn(name, ..) if self.models.native_trait(&name)) =>
            {
                self.proven_type(&inner, operation, span)?
            }
            // An `Rc` or `Arc` a crate takes or gives (#775): rawr's
            // pointer, whose value the crate's own pointer holds.
            Ty::Counted(counter, inner) => {
                intrinsic::Type::counted(counter.name(), self.proven_type(&inner, operation, span)?)
            }
            // A `RefCell`, as a canvas Program's State, the host keeps and
            // hands back as it is.
            Ty::Cell(kind, _) => intrinsic::Type::builtin(kind.name(), TypeId::of::<cell::Cell>()),
            // A thread's `JoinHandle`, the program's or a crate's (#1266,
            // #1274): rawr's thread handle, whose value becomes rawr's where
            // it is joined, attested as its `T` (#1275).
            Ty::Thread(thread::Kind::JoinHandle, inner) => intrinsic::Type::joined(
                TypeId::of::<crate::eval::threads::Handle>(),
                self.proven_type(&inner, operation, span)?,
            ),
            Ty::Thread(..) => return Err(unpassable(ty.name(&self.models), operation, span)),
            Ty::Fn { .. } | Ty::FnItem { .. } | Ty::FnChoice { .. } | Ty::Callable { .. } => {
                return Err(Error::InvalidHostOperation {
                    item: operation.into(),
                    reason: "passing a closure or `fn` pointer to it here is not supported in \
                             Rawr yet"
                        .into(),
                    operation: span,
                });
            }
            #[cfg(feature = "rust-import")]
            Ty::ImportedFunctionItem { .. } | Ty::ProfileMapper { .. } => {
                return Err(Error::InvalidHostOperation {
                    item: operation.into(),
                    reason: "passing a function to it here is not supported in Rawr yet".into(),
                    operation: span,
                });
            }
            #[cfg(feature = "rust-import")]
            Ty::ProfileMessage { payload, .. } => {
                return self.proven_type(&payload, operation, span);
            }
            Ty::Bool => intrinsic::Type::builtin("bool", TypeId::of::<bool>()),
            Ty::Char => intrinsic::Type::builtin("char", TypeId::of::<char>()),
            Ty::Int(_) => intrinsic::Type::builtin("i64", TypeId::of::<INT>()),
            Ty::Float(_) => intrinsic::Type::builtin("f64", TypeId::of::<crate::FLOAT>()),
            Ty::String | Ty::UnsizedStr | Ty::Str => {
                intrinsic::Type::builtin("String", TypeId::of::<ImmutableString>())
            }
            Ty::InclusiveRange(_) => intrinsic::Type::i64_range_inclusive(),
            Ty::FloatRange => intrinsic::Type::f64_range_inclusive(),
            Ty::Range(element) => intrinsic::Type::builtin(
                if matches!(*element, Ty::Float(_)) {
                    "Range<f64>"
                } else {
                    "Range<i64>"
                },
                range_type_id(&element),
            ),
            Ty::InclusiveOf(_) => intrinsic::Type::builtin(
                "RangeInclusive<Dynamic>",
                TypeId::of::<std::ops::RangeInclusive<Dynamic>>(),
            ),
            Ty::OpenRange(open, _) => intrinsic::Type::builtin(open.name(), range::type_id(open)),
            Ty::Ordering => intrinsic::Type::builtin("i64", TypeId::of::<INT>()),
            Ty::Time(Time::Duration) => {
                intrinsic::Type::builtin("Duration", TypeId::of::<std::time::Duration>())
            }
            Ty::Time(Time::Instant) => {
                intrinsic::Type::builtin("Instant", TypeId::of::<crate::Instant>())
            }
            Ty::ParseError(_) | Ty::Arguments => {
                intrinsic::Type::builtin("String", TypeId::of::<ImmutableString>())
            }
            Ty::Reference { target, .. } => {
                return self.proven_type(&target, operation, span);
            }
            // A generic type's arguments go in its witness, so a value that
            // comes back is checked against them (review 1).
            // Arguments that are still generic parameters leave the type's
            // members unchecked, as before.
            Ty::Script(id, arguments) => {
                let arguments = arguments
                    .iter()
                    .map(|argument| self.proven_type(argument, operation, span))
                    .collect::<Result<Box<[_]>, _>>()
                    .unwrap_or_default();
                script_type(&self.models, id, arguments)
            }
            Ty::Host { proof, arguments } => {
                let definition = &proof.definition;
                let arguments = arguments
                    .iter()
                    .enumerate()
                    .map(|(index, argument)| {
                        // An argument nothing fixed whose parameter defaults
                        // to `()`, as `Span<'a, Link = ()>`'s with no
                        // `.link(..)`: its default.
                        if matches!(self.inference.resolve(argument), Ty::Infer(_))
                            && definition.defaults().get(index).is_some_and(|default| {
                                default.as_ref().is_some_and(|default| {
                                    matches!(default.shape(), crate::host::type_ref::Shape::Unit)
                                })
                            })
                        {
                            return Ok(intrinsic::Type::unit());
                        }
                        // A closure the value holds, as iced's `Map<W, F,
                        // A>` does: a function type, as a script struct's
                        // `Box<dyn Fn(A) -> B>` field is (T417).
                        if matches!(
                            self.inference.resolve(argument),
                            Ty::Fn { .. }
                                | Ty::FnItem { .. }
                                | Ty::FnChoice { .. }
                                | Ty::Callable { .. }
                        ) {
                            return Ok(intrinsic::Type::Object(
                                crate::item::type_ref::FN_TYPE.into(),
                            ));
                        }
                        self.proven_type(argument, operation, span)
                    })
                    .collect::<Result<Vec<_>, _>>()?
                    .into_boxed_slice();
                if let Some(native) = definition.native_identity() {
                    intrinsic::Type::host(
                        definition.path().into(),
                        native,
                        arguments,
                        proof.owner.clone(),
                    )
                } else {
                    #[cfg(feature = "rust-import")]
                    {
                        let unpassable =
                            || unpassable(host_type_name(&proof, &self.models), operation, span);
                        let identity = definition.identity().imported().ok_or_else(unpassable)?;
                        let owner = proof.owner.clone().ok_or_else(unpassable)?;
                        let path = proof.alias.as_ref().unwrap_or(definition).path().into();
                        intrinsic::Type::imported(path, *identity, arguments, owner)
                    }
                    #[cfg(not(feature = "rust-import"))]
                    {
                        return Err(unpassable(
                            host_type_name(&proof, &self.models),
                            operation,
                            span,
                        ));
                    }
                }
            }
            Ty::Param(name, _)
                if let Some(index) = self
                    .template_params
                    .borrow()
                    .iter()
                    .position(|parameter| *parameter == name) =>
            {
                intrinsic::Type::witness_parameter(index)
            }
            Ty::Infer(_) | Ty::Unknown => {
                return Err(Error::InvalidHostOperation {
                    item: operation.into(),
                    reason: "type annotations needed: the type of a value it takes or gives is \
                             unknown (E0282)"
                        .into(),
                    operation: span,
                });
            }
            // A crate's trait object (D872): a program's value coerced to
            // it, or one native code gave, which the adapter takes at its
            // exact type.
            Ty::Dyn(name, ..) if self.models.native_trait(&name) => intrinsic::Type::Object(name),
            // `3` of `Buf<3>`, among a type's arguments.
            Ty::Const(value) => intrinsic::Type::Constant(value),
            Ty::Param(..) | Ty::Projection(..) | Ty::Weak(..) | Ty::Dyn(..) => {
                return Err(unpassable(ty.name(&self.models), operation, span));
            }
        })
    }
}

/// Whether std's table states `ty`'s impl of the std trait `capability`.
/// rustc makes `Clone` and `Copy` of a tuple itself, `!`'s impls are of a
/// type stable Rust doesn't name, and the table writes the lengths of std's
/// per-length `Default` impls for arrays as their anonymous constants.
fn std_states(ty: &Ty, capability: capability::Kind) -> bool {
    match ty {
        Ty::Never => false,
        Ty::Unit | Ty::Tuple(_) => {
            !matches!(capability, capability::Kind::Clone | capability::Kind::Copy)
        }
        Ty::Array {
            kind: sequence::Kind::Array(_),
            ..
        } => capability != capability::Kind::Default,
        _ => true,
    }
}
