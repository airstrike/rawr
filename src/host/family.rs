//! Imported method candidates awaiting call-specific applicability evidence.

use crate::host;

/// Resolved, distinctly identified imported methods at one path. These are
/// candidates, not ordinary host overloads and not a proof of Rust ambiguity.
#[derive(Clone, Debug)]
pub(crate) struct Pending {
    path: host::path::Path,
    methods: Box<[host::item::Definition]>,
}

pub(crate) enum Admission {
    Ordinary(Vec<host::item::Definition>),
    Pending(Pending),
}

/// Two distinct imported callable definitions of one kind can require
/// contextual lookup: two impls' methods, or two impls' associated
/// functions, as `From<A>::from` and `From<B>::from` when Rawr's types of
/// `A` and `B` are one. Repeated selectors, ordinary host items and
/// non-callables keep the ordinary facade rules, including malformed
/// duplicate declaration errors.
pub(crate) fn distinct(left: &host::item::Definition, right: &host::item::Definition) -> bool {
    left.kind() == right.kind()
        && matches!((callable(left), callable(right)), (Some(left), Some(right)) if left != right)
}

/// The definition an imported method or function stands for.
fn callable(
    definition: &host::item::Definition,
) -> Option<&rawr_import::surface::identity::Definition> {
    use rawr_import::surface::identity::Selector;
    match (definition.kind(), definition.imported_identity()) {
        (host::item::Kind::Method, Some(Selector::Method { item, .. }))
        | (host::item::Kind::AssociatedFunction, Some(Selector::AssociatedFunction { item }))
        | (host::item::Kind::Function, Some(Selector::Function { item })) => Some(item),
        _ => None,
    }
}

impl Pending {
    /// Partition one fully resolved path group. Only a group whose every
    /// member has distinct imported method provenance can leave the ordinary
    /// overload lane. No declaration, selector or signature is discarded.
    pub(crate) fn partition<A: host::ImportAllocation + ?Sized>(
        methods: Vec<host::item::Definition>,
        allocation: &mut A,
    ) -> Result<Admission, host::Error> {
        let mut conflict = None;
        let mut distinct_methods = true;
        for (index, method) in methods.iter().enumerate() {
            for prior in &methods[..index] {
                distinct_methods &= distinct(prior, method);
                if prior.kind().is_macro() != method.kind().is_macro() {
                    continue;
                }
                if prior.signature().same_inputs(method.signature())? {
                    conflict = Some(host::Error::DuplicateFacadeOverloadInputs {
                        item: method.path().into(),
                    });
                } else if !host::facade::compatible_overload(prior, method) {
                    conflict = Some(host::Error::IncompatibleFacadeOverload {
                        item: method.path().into(),
                        existing_kind: prior.kind(),
                        existing_receiver: prior.receiver(),
                        incoming_kind: method.kind(),
                        incoming_receiver: method.receiver(),
                    });
                }
            }
        }
        match conflict {
            None => Ok(Admission::Ordinary(methods)),
            Some(_) if distinct_methods => {
                let first = methods.first().ok_or(host::Error::Allocation)?;
                let header_bytes = std::mem::size_of::<Self>().checked_add(
                    first
                        .path_ref()
                        .retained_bytes()
                        .and_then(|bytes| {
                            bytes.checked_sub(std::mem::size_of::<host::path::Path>())
                        })
                        .ok_or(host::Error::Allocation)?,
                );
                allocation
                    .retain_checked(Some(1), header_bytes)
                    .map_err(|_| host::Error::Allocation)?;
                let path = first
                    .path_ref()
                    .try_clone()
                    .map_err(|_| host::Error::Allocation)?;
                Ok(Admission::Pending(Self {
                    path,
                    methods: methods.into_boxed_slice(),
                }))
            }
            Some(error) => Err(error),
        }
    }

    pub(crate) fn path(&self) -> &host::path::Path {
        &self.path
    }

    pub(crate) fn len(&self) -> usize {
        self.methods.len()
    }

    /// The candidates, each with its own selector and signature.
    pub(crate) fn methods(&self) -> &[host::item::Definition] {
        &self.methods
    }

    pub(crate) fn retained_bytes(&self) -> Option<usize> {
        self.methods.iter().try_fold(
            std::mem::size_of::<Self>().checked_add(
                self.path
                    .retained_bytes()?
                    .checked_sub(std::mem::size_of::<host::path::Path>())?,
            )?,
            |bytes, method| bytes.checked_add(method.retained_bytes()?),
        )
    }

    pub(crate) fn try_clone(&self) -> Result<Self, rawr_import::surface::Error> {
        let mut methods = Vec::new();
        methods
            .try_reserve_exact(self.methods.len())
            .map_err(|_| rawr_import::surface::Error::Allocation)?;
        for method in &self.methods {
            methods.push(method.try_clone()?);
        }
        Ok(Self {
            path: self.path.try_clone()?,
            methods: methods.into_boxed_slice(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{Admission, Pending};
    use crate::{host, receiver};
    use rawr_import::classify::representability::ownership;
    use rawr_import::surface::identity;

    fn method(id: u8, output: host::type_ref::Builtin) -> host::item::Definition {
        let mut signature = host::signature::Builder::new();
        signature.output(host::type_ref::Type::builtin(output));
        host::item::Definition::method("Marker::choose", receiver::Mode::Shared, signature.finish())
            .unwrap()
            .imported(identity::Selector::Method {
                item: identity::Definition::authenticated([id; 32]),
                receiver: ownership::Receiver::Shared,
            })
    }

    fn fixture() -> (host::facade::Definition, Vec<Pending>) {
        let mut allocation = host::TestImportAllocation::default();
        let mut builder = host::facade::Builder::new_imported(
            "fixture",
            host::facade::Abi::new(1).unwrap(),
            &mut allocation,
        )
        .unwrap();
        builder.type_::<()>("Marker", &[], &[]).unwrap();
        builder.alias("Alias", "Marker").unwrap();
        builder
            .imported_item(method(1, host::type_ref::Builtin::I64))
            .unwrap();
        builder
            .imported_item(method(2, host::type_ref::Builtin::Bool))
            .unwrap();
        builder
            .finish_imported(Box::default(), &mut allocation)
            .unwrap()
    }

    #[test]
    fn pending_families_retain_every_selector_and_clone_through_aliases() {
        let (ordinary, families) = fixture();
        assert_eq!(ordinary.items().count(), 0);
        assert_eq!(families.len(), 1);
        let selectors = families[0]
            .methods
            .iter()
            .map(|method| method.imported_identity().unwrap().fingerprint())
            .collect::<Vec<_>>();
        assert_ne!(selectors[0], selectors[1]);
        let mut registry = host::Registry::new();
        registry.install(ordinary).unwrap();
        registry.defer_imported(families).unwrap();
        assert!(registry.overloads("fixture::Marker::choose").is_none());
        let mut allocation = host::TestImportAllocation::default();
        let cloned = registry.clone_for_import(&mut allocation).unwrap();
        let expected = allocation.usage();
        let family = cloned.pending_family("fixture::Alias::choose").unwrap();
        assert_eq!(family.len(), 2);
        assert_eq!(
            family
                .methods
                .iter()
                .map(|method| method.imported_identity().unwrap().fingerprint())
                .collect::<Vec<_>>(),
            selectors
        );
        let mut short = host::TestImportAllocation::new(expected.0 - 1, usize::MAX);
        assert!(registry.clone_for_import(&mut short).is_err());
        let mut short = host::TestImportAllocation::new(usize::MAX, expected.1 - 1);
        assert!(registry.clone_for_import(&mut short).is_err());
    }

    #[test]
    fn duplicate_identity_and_ordinary_groups_keep_strict_validation() {
        let first = method(1, host::type_ref::Builtin::I64);
        let duplicate = method(1, host::type_ref::Builtin::Bool);
        let other = method(2, host::type_ref::Builtin::Bool);
        assert!(matches!(
            Pending::partition(
                vec![first.clone(), duplicate, other],
                &mut host::TestImportAllocation::default()
            ),
            Err(host::Error::DuplicateFacadeOverloadInputs { .. })
        ));
        let mut signature = host::signature::Builder::new();
        signature.output(host::type_ref::Type::builtin(host::type_ref::Builtin::Bool));
        let ordinary = host::item::Definition::method(
            "Marker::choose",
            receiver::Mode::Shared,
            signature.finish(),
        )
        .unwrap();
        assert!(matches!(
            Pending::partition(
                vec![first.clone(), ordinary],
                &mut host::TestImportAllocation::default()
            ),
            Err(host::Error::DuplicateFacadeOverloadInputs { .. })
        ));

        let mut builder =
            host::facade::Builder::new("fixture", host::facade::Abi::new(1).unwrap()).unwrap();
        builder.type_::<()>("Marker", &[], &[]).unwrap();
        builder.item(first).unwrap();
        builder
            .item(method(2, host::type_ref::Builtin::Bool))
            .unwrap();
        assert!(matches!(
            builder.finish(),
            Err(host::Error::DuplicateFacadeOverloadInputs { .. })
        ));
        assert!(matches!(
            Pending::partition(
                vec![method(1, host::type_ref::Builtin::I64)],
                &mut host::TestImportAllocation::default()
            )
            .unwrap(),
            Admission::Ordinary(_)
        ));
    }

    #[test]
    fn established_receiver_instance_projection_keeps_its_existing_route() {
        for instance_first in [false, true] {
            let mut allocation = host::TestImportAllocation::default();
            let mut builder = host::facade::Builder::new_imported(
                "fixture",
                host::facade::Abi::new(1).unwrap(),
                &mut allocation,
            )
            .unwrap();
            builder.type_::<()>("Marker", &[], &[]).unwrap();
            let ordinary = method(1, host::type_ref::Builtin::I64);
            let mut signature = host::signature::Builder::new();
            signature.instance(host::type_ref::Type::registered("Marker").unwrap());
            signature.output(host::type_ref::Type::builtin(host::type_ref::Builtin::I64));
            let instance = host::item::Definition::method(
                "Marker::choose",
                receiver::Mode::Shared,
                signature.finish(),
            )
            .unwrap()
            .imported(identity::Selector::Method {
                item: identity::Definition::authenticated([2; 32]),
                receiver: ownership::Receiver::Shared,
            });
            let methods = if instance_first {
                [instance, ordinary]
            } else {
                [ordinary, instance]
            };
            for method in methods {
                builder.imported_item(method).unwrap();
            }
            let (definition, pending) = builder
                .finish_imported(Box::default(), &mut allocation)
                .unwrap();
            assert!(pending.is_empty());
            let methods = definition.items().collect::<Vec<_>>();
            assert_eq!(methods.len(), 1);
            assert!(methods[0].signature().instance().is_none());
            assert_eq!(
                methods[0].imported_identity(),
                method(1, host::type_ref::Builtin::I64).imported_identity()
            );
        }
    }

    #[test]
    fn used_family_is_explicitly_unsupported_without_selecting_by_output() {
        let (ordinary, families) = fixture();
        let mut engine = crate::Engine::new();
        engine.register_host_facade(ordinary).unwrap();
        engine.host_registry.defer_imported(families).unwrap();
        engine
            .compile_executable("fn unused(value: fixture::Marker) { 1; }")
            .unwrap();
        for output in ["i64", "bool"] {
            for receiver in ["Marker", "Alias"] {
                let source =
                    format!("fn used(value: fixture::{receiver}) -> {output} {{ value.choose() }}");
                let error = match engine.compile_executable(&source) {
                    Ok(_) => panic!("a candidate family has no established applicability"),
                    Err(error) => error,
                };
                assert!(
                    error.to_string().contains("impls of the crate provide it"),
                    "{error}"
                );
                assert!(!error.to_string().contains("return-only overloads"));
            }
        }
    }

    /// Two impls' `from` whose inputs are one type in Rawr, as two `From`
    /// impls of types Rawr doesn't tell apart: the facade keeps them, and
    /// only a call of `from` is reported, at the call.
    #[test]
    fn a_family_of_associated_functions_is_reported_only_where_it_is_called() {
        let from = |id: u8| {
            let mut signature = host::signature::Builder::new();
            signature.parameter(host::type_ref::Type::builtin(host::type_ref::Builtin::I64));
            signature.output(host::type_ref::Type::registered("Marker").unwrap());
            host::item::Definition::associated_function("Marker::from", signature.finish())
                .unwrap()
                .imported(identity::Selector::AssociatedFunction {
                    item: identity::Definition::authenticated([id; 32]),
                })
        };
        let mut allocation = host::TestImportAllocation::default();
        let mut builder = host::facade::Builder::new_imported(
            "fixture",
            host::facade::Abi::new(1).unwrap(),
            &mut allocation,
        )
        .unwrap();
        builder.type_::<()>("Marker", &[], &[]).unwrap();
        builder.imported_item(from(1)).unwrap();
        builder.imported_item(from(2)).unwrap();
        let (ordinary, families) = builder
            .finish_imported(Box::default(), &mut allocation)
            .unwrap();
        assert_eq!(families.len(), 1);
        assert_eq!(families[0].len(), 2);
        let mut engine = crate::Engine::new();
        engine.register_host_facade(ordinary).unwrap();
        engine.host_registry.defer_imported(families).unwrap();
        engine
            .compile_executable("fn unused(value: fixture::Marker) { 1; }")
            .unwrap();
        for source in [
            "fn used() -> fixture::Marker { fixture::Marker::from(1) }",
            "use fixture::Marker;\nfn used() -> Marker { Marker::from(1) }",
        ] {
            let error = match engine.compile_executable(source) {
                Ok(_) => panic!("Rawr can't pick one of the impls: {source}"),
                Err(error) => error.to_string(),
            };
            assert!(error.contains("impls of the crate provide it"), "{error}");
            assert!(!error.contains("return-only overloads"), "{error}");
        }
    }
}
