//! A crate trait's members on a slice through a blanket impl over a borrow
//! of its own type, as rayon's `impl<'data, I: 'data + ?Sized>
//! IntoParallelRefIterator<'data> for I where &'data I:
//! IntoParallelIterator` gives `[T]` its `par_iter` through
//! `IntoParallelIterator`'s `impl<'data, T: Sync + 'data> .. for &'data
//! [T]` (`surface::slice`).
//!
//! The member is the blanket impl's function at `I = [T]`: a function of
//! the slice's borrow, `fn par_iter<'data, T: Sync + 'data>(&'data [T]) ->
//! Iter<'data, T>`, whose `T` the call infers from the slice and whose
//! projections each impl normalizes as rustc does, the blanket impl's own
//! `Self::Iter` to `<&'data I as IntoParallelIterator>::Iter` and that to
//! the slice impl's `Iter<'data, T>`.

use super::{api, model, representability, Context, Error, Failure, OutcomeEvidence};

/// An application of a blanket impl to slices: the impl of the bound's
/// trait for `&[T]` it goes through, and the bound's lifetime.
pub(super) struct Application<'graph> {
    through: &'graph model::Implementation,
    lifetime: model::Lifetime,
}

impl Context<'_> {
    /// The application of `blanket`, an impl of the trait `target` over its
    /// own parameter `I`, to slices: its bound `&'a I: Other` and the one
    /// impl of `Other` for `&[T]` in the graph. `None` for any other impl,
    /// or when `Other` has no such impl or more than one.
    pub(super) fn slice_application(
        &self,
        blanket: &model::Implementation,
        target: &api::identity::Definition,
    ) -> Option<Application<'_>> {
        let this = blanket_parameter(blanket)?;
        if !matches!(blanket.trait_ref(), model::Trait::Trait { id, .. } if id == target)
            || blanket.polarity() != model::Polarity::Positive
        {
            return None;
        }
        let (other, lifetime) = blanket.generics().predicates().find_map(|predicate| {
            let model::Predicate::Bound {
                ty:
                    model::Type::Reference {
                        lifetime,
                        mutability: model::Mutability::Shared,
                        ty,
                    },
                bound:
                    model::Bound::Trait {
                        trait_id,
                        polarity: model::Polarity::Positive,
                        binders,
                        ..
                    },
            } = predicate
            else {
                return None;
            };
            (**ty == this && binders.is_empty()).then(|| (trait_id, lifetime.clone()))
        })?;
        let mut found = self
            .graph
            .implementations_of(other)
            .filter(|implementation| {
                implementation.polarity() == model::Polarity::Positive
                    && slice_element(implementation).is_some()
            });
        let through = found.next()?;
        found
            .next()
            .is_none()
            .then_some(Application { through, lifetime })
    }

    /// The member `definition` of the blanket impl `blanket` on slices,
    /// through `application`: a function of the slice's borrow whose
    /// instance is the slice, called through the trait, `<[_] as
    /// Trait>::member(slice)`, as `[T]` has inherent methods too.
    pub(super) fn classify_slice_member(
        &self,
        blanket: &model::Implementation,
        definition: &model::Definition,
        application: &Application<'_>,
    ) -> Result<OutcomeEvidence, Error> {
        use representability::{reject, Representability};
        let retained_before = (self.retained_items.get(), self.retained_bytes.get());
        let result = (|| -> Result<Representability, Failure> {
            let model::Kind::Function {
                signature,
                generics,
            } = definition.kind()
            else {
                return Err(reject::Kind::GenericImplementation.into());
            };
            let composed = compose(self.graph, blanket, application, signature)
                .ok_or(reject::Kind::GenericImplementation)?;
            if generics.parameters().next().is_some() {
                return Err(reject::Kind::GenericImplementation.into());
            }
            let model::Trait::Trait { id, arguments } = blanket.trait_ref() else {
                return Err(reject::Kind::GenericImplementation.into());
            };
            let arguments = arguments
                .iter()
                .skip(1)
                .filter_map(|argument| match argument {
                    model::Argument::Type(ty) => Some(self.convert(ty)),
                    _ => None,
                })
                .collect::<Result<Box<[_]>, _>>()?;
            let through = representability::convert::Type::Nominal {
                definition: self.graph.surface_definition(id),
                arguments,
            };
            // The member is generic over the slice's element, which the
            // call infers from the slice it lends, as rustc does.
            let Representability::FunctionItem(item) =
                self.function(&composed.signature, &composed.generics)?
            else {
                return Err(reject::Kind::GenericImplementation.into());
            };
            let inferred = composed
                .generics
                .parameters()
                .filter(|parameter| matches!(parameter.kind(), model::ParameterKind::Type { .. }))
                .map(model::Parameter::index)
                .collect::<Vec<_>>();
            Ok(Representability::FunctionItem(
                item.with_inferred(&inferred, 0)
                    .with_instance(crate::surface::slice::instance())
                    .with_through(Some(through)),
            ))
        })();
        match result {
            Ok(plan) => {
                self.retain_inline_representability()?;
                Ok(OutcomeEvidence::Inline(Box::new(plan)))
            }
            Err(Failure::Reject(kind)) => {
                self.retained_items.set(retained_before.0);
                self.retained_bytes.set(retained_before.1);
                self.retain_span(blanket.span())?;
                self.retain_inline_representability()?;
                Ok(OutcomeEvidence::Inline(Box::new(
                    Representability::Rejected(reject::Reason::classification(
                        kind,
                        blanket.span().clone(),
                    )),
                )))
            }
            Err(Failure::Operational(error)) => Err(error),
        }
    }
}

/// The blanket impl's function at `I = [T]`, with the generics it is
/// generic over there.
struct Composed {
    signature: model::Signature,
    generics: model::Generics,
}

/// The self parameter `I` of a blanket impl, as a type.
fn blanket_parameter(blanket: &model::Implementation) -> Option<model::Type> {
    match blanket.self_type() {
        this @ model::Type::Parameter { owner, .. } if owner == blanket.id() => Some(this.clone()),
        _ => None,
    }
}

/// The element parameter `T` of an impl for `&[T]`, by index, and the
/// lifetime of its borrow.
fn slice_element(implementation: &model::Implementation) -> Option<(u32, &model::Lifetime)> {
    let model::Type::Reference {
        lifetime,
        mutability: model::Mutability::Shared,
        ty,
    } = implementation.self_type()
    else {
        return None;
    };
    match &**ty {
        model::Type::Slice(element) => match &**element {
            model::Type::Parameter { owner, index } if owner == implementation.id() => {
                Some((*index, lifetime))
            }
            _ => None,
        },
        _ => None,
    }
}

/// The function `signature` of `blanket` at `I = [T]`, a free function of
/// the slice's borrow, where `T` takes `I`'s place among the blanket impl's
/// parameters and `application`'s impl gives its bounds. `None` when a
/// type names what neither impl gives, or the blanket impl bounds `I` by
/// more than the bound it applies through and its sizedness.
fn compose(
    graph: &api::Graph<impl api::Phase>,
    blanket: &model::Implementation,
    application: &Application<'_>,
    signature: &model::Signature,
) -> Option<Composed> {
    let this = blanket_parameter(blanket)?;
    let model::Type::Parameter { index: slot, .. } = this else {
        return None;
    };
    let through = application.through;
    let (element, borrowed) = slice_element(through)?;
    let element_type = model::Type::Parameter {
        owner: blanket.id().clone(),
        index: slot,
    };
    let slice = model::Type::Slice(Box::new(element_type.clone()));
    // The slice impl's own parameters are its element, now `T` of the
    // blanket impl, and its borrow's lifetime, now the bound's.
    for parameter in through.generics().parameters() {
        match parameter.kind() {
            model::ParameterKind::Type { .. } if parameter.index() == element => {}
            model::ParameterKind::Lifetime { .. }
                if matches!(borrowed, model::Lifetime::Early { owner, index }
                    if owner == through.id() && *index == parameter.index()) => {}
            _ => return None,
        }
    }
    let from_through = Rewrite {
        owner: through.id(),
        types: &|index| (index == element).then(|| element_type.clone()),
        lifetimes: &|lifetime| (lifetime == borrowed).then(|| application.lifetime.clone()),
    };
    let to_slice = Rewrite {
        owner: blanket.id(),
        types: &|index| (index == slot).then(|| slice.clone()),
        lifetimes: &|_| None,
    };
    let blanket_given = given(graph, blanket);
    let through_given = given(graph, through);
    let model::Trait::Trait {
        id: blanket_trait, ..
    } = blanket.trait_ref()
    else {
        return None;
    };
    let model::Trait::Trait {
        id: through_trait, ..
    } = through.trait_ref()
    else {
        return None;
    };
    // A type of the blanket impl's function at `I = [T]`: its own trait's
    // projections on `I` are the associated types it gives, and its bound
    // trait's on `&[E]` the slice impl's at `E`.
    let normalize = |ty: &model::Type| -> Option<model::Type> {
        let own = project(graph, ty, &|trait_id, name, arguments| {
            (trait_id == blanket_trait
                && matches!(arguments.first(), Some(model::Argument::Type(first))
                    if *first == this
                        || matches!(first, model::Type::SelfType(owner) if owner == trait_id)))
            .then(|| given_named(&blanket_given, name))
            .flatten()
        })?;
        let sliced = to_slice.ty(&own)?;
        project(graph, &sliced, &|trait_id, name, arguments| {
            if trait_id != through_trait {
                return None;
            }
            let Some(model::Argument::Type(model::Type::Reference {
                lifetime,
                mutability: model::Mutability::Shared,
                ty,
            })) = arguments.first()
            else {
                return None;
            };
            let model::Type::Slice(at) = &**ty else {
                return None;
            };
            let value = given_named(&through_given, name)?;
            Rewrite {
                owner: through.id(),
                types: &|index| (index == element).then(|| (**at).clone()),
                lifetimes: &|candidate| (candidate == borrowed).then(|| lifetime.clone()),
            }
            .ty(&value)
        })
    };
    let receiver = match signature.receiver() {
        model::Receiver::Reference {
            lifetime,
            mutability: model::Mutability::Shared,
            ty,
        } if *ty == this => model::Type::Reference {
            lifetime: lifetime.clone(),
            mutability: model::Mutability::Shared,
            ty: Box::new(slice.clone()),
        },
        _ => return None,
    };
    let inputs = std::iter::once(Some(receiver))
        .chain(signature.inputs().map(&normalize))
        .collect::<Option<Box<[_]>>>()?;
    let output = normalize(signature.output())?;
    let free = model::Signature::new(
        signature.safety(),
        signature.const_function(),
        signature.async_function(),
        signature.abi().into(),
        signature.variadic(),
        signature.binders().cloned().collect(),
        model::Receiver::None,
        inputs,
        output,
    );
    // `T` in `I`'s place, named and bounded as the slice impl's element.
    let element_parameter = through
        .generics()
        .parameters()
        .find(|parameter| parameter.index() == element)?;
    let generics = blanket
        .generics()
        .parameters()
        .map(|parameter| {
            if parameter.index() == slot {
                model::Parameter::new(
                    blanket.id().clone(),
                    slot,
                    element_parameter.name().into(),
                    element_parameter.kind().clone(),
                )
            } else {
                parameter.clone()
            }
        })
        .collect::<Box<[_]>>();
    // The blanket impl's bounds of anything but `I`, and the slice impl's
    // bounds of `T`. Its bounds of `I` are the one it applies through, which
    // the slice impl proves, and `I`'s sizedness and outliving, which a
    // slice of `T: 'a` meets.
    let mut predicates = Vec::new();
    for predicate in blanket.generics().predicates() {
        match predicate {
            model::Predicate::Bound {
                ty: model::Type::Reference { ty, .. },
                bound: model::Bound::Trait { trait_id, .. },
            } if **ty == this && trait_id == through_trait => {}
            model::Predicate::Bound {
                ty,
                bound: model::Bound::Trait { trait_id, .. },
            } if *ty == this && sizedness(graph, trait_id) => {}
            model::Predicate::TypeOutlives { ty, .. } if *ty == this => {}
            predicate if names(predicate, &this) => return None,
            predicate => predicates.push(predicate.clone()),
        }
    }
    for predicate in through.generics().predicates() {
        predicates.push(from_through.predicate(predicate)?);
    }
    Some(Composed {
        signature: free,
        generics: model::Generics::new(generics, predicates.into_boxed_slice()),
    })
}

/// Whether `trait_id` is one of the sizedness traits rustc gives every
/// parameter, as `MetaSized` of a `?Sized` one.
fn sizedness(graph: &api::Graph<impl api::Phase>, trait_id: &api::identity::Definition) -> bool {
    matches!(
        super::definition_name(graph, trait_id),
        Some("Sized" | "MetaSized" | "PointeeSized")
    )
}

/// Whether `predicate` names `ty` anywhere.
fn names(predicate: &model::Predicate, ty: &model::Type) -> bool {
    match predicate {
        model::Predicate::Bound { ty: bounded, bound } => {
            contains(bounded, ty)
                || matches!(bound, model::Bound::Trait { arguments, .. }
                    if arguments.iter().any(|argument| argument_contains(argument, ty)))
        }
        model::Predicate::ProjectionEquality {
            projection,
            ty: equal,
        } => contains(projection, ty) || contains(equal, ty),
        model::Predicate::TypeOutlives { ty: outliving, .. } => contains(outliving, ty),
        model::Predicate::LifetimeOutlives { .. } | model::Predicate::ConstEvaluatable(_) => false,
    }
}

fn argument_contains(argument: &model::Argument, needle: &model::Type) -> bool {
    matches!(argument, model::Argument::Type(ty) if contains(ty, needle))
}

/// Whether `ty` is `needle` or holds it.
fn contains(ty: &model::Type, needle: &model::Type) -> bool {
    if ty == needle {
        return true;
    }
    match ty {
        model::Type::Definition { arguments, .. } | model::Type::Projection { arguments, .. } => {
            arguments
                .iter()
                .any(|argument| argument_contains(argument, needle))
        }
        model::Type::Tuple(members) => members.iter().any(|member| contains(member, needle)),
        model::Type::Slice(element)
        | model::Type::Array { element, .. }
        | model::Type::Reference { ty: element, .. }
        | model::Type::RawPointer { ty: element, .. } => contains(element, needle),
        _ => false,
    }
}

/// The associated types `implementation` gives, by name.
fn given<'graph>(
    graph: &'graph api::Graph<impl api::Phase>,
    implementation: &model::Implementation,
) -> Vec<(&'graph str, &'graph model::Type)> {
    implementation
        .associated()
        .filter_map(|associated| {
            let definition = graph
                .definition_index(associated)
                .and_then(|index| graph.definition_at(index))?;
            match definition.kind() {
                model::Kind::AssociatedType {
                    default: model::Default::Present(ty),
                    ..
                } => Some((definition.display_path().last()?, ty)),
                _ => None,
            }
        })
        .collect()
}

fn given_named(given: &[(&str, &model::Type)], name: &str) -> Option<model::Type> {
    given
        .iter()
        .find(|(candidate, _)| *candidate == name)
        .map(|(_, ty)| (*ty).clone())
}

/// What a projection reads as, by its trait, its associated type's name and
/// its arguments, when the impl it is read through gives it.
type Resolve<'a> =
    dyn Fn(&api::identity::Definition, &str, &[model::Argument]) -> Option<model::Type> + 'a;

/// `ty` with each projection `resolve` gives a type for, by its trait, its
/// associated type's name and its arguments, read as that type, itself
/// projected again. `None` when an associated type has no name.
fn project(
    graph: &api::Graph<impl api::Phase>,
    ty: &model::Type,
    resolve: &Resolve<'_>,
) -> Option<model::Type> {
    let each = |ty: &model::Type| project(graph, ty, resolve);
    let arguments = |arguments: &[model::Argument]| {
        arguments
            .iter()
            .map(|argument| match argument {
                model::Argument::Type(ty) => each(ty).map(model::Argument::Type),
                other => Some(other.clone()),
            })
            .collect::<Option<Box<[_]>>>()
    };
    Some(match ty {
        model::Type::Projection {
            trait_id,
            associated,
            arguments: given,
        } => {
            let given = arguments(given)?;
            let name = super::definition_name(graph, associated)?;
            match resolve(trait_id, name, &given) {
                Some(value) => each(&value)?,
                None => model::Type::Projection {
                    trait_id: trait_id.clone(),
                    associated: associated.clone(),
                    arguments: given,
                },
            }
        }
        model::Type::Definition {
            id,
            arguments: given,
        } => model::Type::Definition {
            id: id.clone(),
            arguments: arguments(given)?,
        },
        model::Type::Tuple(members) => {
            model::Type::Tuple(members.iter().map(each).collect::<Option<_>>()?)
        }
        model::Type::Slice(element) => model::Type::Slice(Box::new(each(element)?)),
        model::Type::Array { element, length } => model::Type::Array {
            element: Box::new(each(element)?),
            length: length.clone(),
        },
        model::Type::Reference {
            lifetime,
            mutability,
            ty,
        } => model::Type::Reference {
            lifetime: lifetime.clone(),
            mutability: *mutability,
            ty: Box::new(each(ty)?),
        },
        other => other.clone(),
    })
}

/// A renaming of the parameters of the impl `owner`: each type parameter
/// by index to the type `types` gives, and each lifetime to the one
/// `lifetimes` gives. A parameter either leaves out stays as it is.
struct Rewrite<'a> {
    owner: &'a api::identity::Definition,
    types: &'a dyn Fn(u32) -> Option<model::Type>,
    lifetimes: &'a dyn Fn(&model::Lifetime) -> Option<model::Lifetime>,
}

impl Rewrite<'_> {
    fn lifetime(&self, lifetime: &model::Lifetime) -> model::Lifetime {
        (self.lifetimes)(lifetime).unwrap_or_else(|| lifetime.clone())
    }

    fn argument(&self, argument: &model::Argument) -> Option<model::Argument> {
        Some(match argument {
            model::Argument::Type(ty) => model::Argument::Type(self.ty(ty)?),
            model::Argument::Lifetime(lifetime) => {
                model::Argument::Lifetime(self.lifetime(lifetime))
            }
            model::Argument::Const(_) => argument.clone(),
        })
    }

    fn arguments(&self, arguments: &[model::Argument]) -> Option<Box<[model::Argument]>> {
        arguments
            .iter()
            .map(|argument| self.argument(argument))
            .collect()
    }

    /// `ty` renamed. `None` for a function pointer, a trait object or an
    /// opaque type, whose binders and bounds it doesn't read.
    fn ty(&self, ty: &model::Type) -> Option<model::Type> {
        let inner = |ty: &model::Type| self.ty(ty).map(Box::new);
        Some(match ty {
            model::Type::Parameter { owner, index } if owner == self.owner => {
                (self.types)(*index).unwrap_or_else(|| ty.clone())
            }
            model::Type::Definition { id, arguments } => model::Type::Definition {
                id: id.clone(),
                arguments: self.arguments(arguments)?,
            },
            model::Type::Projection {
                trait_id,
                associated,
                arguments,
            } => model::Type::Projection {
                trait_id: trait_id.clone(),
                associated: associated.clone(),
                arguments: self.arguments(arguments)?,
            },
            model::Type::Tuple(members) => model::Type::Tuple(
                members
                    .iter()
                    .map(|member| self.ty(member))
                    .collect::<Option<_>>()?,
            ),
            model::Type::Slice(element) => model::Type::Slice(inner(element)?),
            model::Type::Array { element, length } => model::Type::Array {
                element: inner(element)?,
                length: length.clone(),
            },
            model::Type::Reference {
                lifetime,
                mutability,
                ty,
            } => model::Type::Reference {
                lifetime: self.lifetime(lifetime),
                mutability: *mutability,
                ty: inner(ty)?,
            },
            model::Type::RawPointer { mutability, ty } => model::Type::RawPointer {
                mutability: *mutability,
                ty: inner(ty)?,
            },
            model::Type::FunctionPointer(_)
            | model::Type::TraitObject { .. }
            | model::Type::ImplTrait { .. } => return None,
            other => other.clone(),
        })
    }

    fn bound(&self, bound: &model::Bound) -> Option<model::Bound> {
        Some(match bound {
            model::Bound::Trait {
                trait_id,
                arguments,
                polarity,
                binders,
            } => model::Bound::Trait {
                trait_id: trait_id.clone(),
                arguments: self.arguments(arguments)?,
                polarity: *polarity,
                binders: binders.clone(),
            },
            model::Bound::MaybeSized => model::Bound::MaybeSized,
        })
    }

    fn predicate(&self, predicate: &model::Predicate) -> Option<model::Predicate> {
        Some(match predicate {
            model::Predicate::Bound { ty, bound } => model::Predicate::Bound {
                ty: self.ty(ty)?,
                bound: self.bound(bound)?,
            },
            model::Predicate::ProjectionEquality { projection, ty } => {
                model::Predicate::ProjectionEquality {
                    projection: self.ty(projection)?,
                    ty: self.ty(ty)?,
                }
            }
            model::Predicate::TypeOutlives { ty, lifetime } => model::Predicate::TypeOutlives {
                ty: self.ty(ty)?,
                lifetime: self.lifetime(lifetime),
            },
            model::Predicate::LifetimeOutlives { longer, shorter } => {
                model::Predicate::LifetimeOutlives {
                    longer: self.lifetime(longer),
                    shorter: self.lifetime(shorter),
                }
            }
            model::Predicate::ConstEvaluatable(_) => predicate.clone(),
        })
    }
}
