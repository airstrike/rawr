//! Executor-neutral factories for exact symbolic host operations.

use crate::func::SendSync;
use crate::{host, Dynamic, ImmutableString, Shared};
use core::future;
use core::pin::Pin;
use std::fmt;

#[cfg(feature = "sync")]
pub(crate) type Pending = Pin<Box<dyn future::Future<Output = Dynamic> + Send + 'static>>;
#[cfg(not(feature = "sync"))]
pub(crate) type Pending = Pin<Box<dyn future::Future<Output = Dynamic> + 'static>>;

#[cfg(feature = "sync")]
type Factory = dyn Fn(host::operation::Arguments) -> Pending + Send + Sync + 'static;
#[cfg(not(feature = "sync"))]
type Factory = dyn Fn(host::operation::Arguments) -> Pending + 'static;

#[cfg(feature = "sync")]
pub(crate) trait Await<E>:
    future::Future<Output = Result<Dynamic, E>> + Send + 'static
{
}

#[cfg(feature = "sync")]
impl<T, E> Await<E> for T where T: future::Future<Output = Result<Dynamic, E>> + Send + 'static {}

#[cfg(not(feature = "sync"))]
pub(crate) trait Await<E>: future::Future<Output = Result<Dynamic, E>> + 'static {}

#[cfg(not(feature = "sync"))]
impl<T, E> Await<E> for T where T: future::Future<Output = Result<Dynamic, E>> + 'static {}

/// One native factory bound to an exact facade operation declaration.
pub(crate) struct Binding {
    definition: host::item::Definition,
    fingerprint: host::item::Fingerprint,
    factory: Shared<Factory>,
}

/// One runtime binding joined to a compiler-selected operation proof.
pub struct Selected {
    owner: host::facade::Dependency,
    item: host::item::Fingerprint,
    path: ImmutableString,
    inputs: Box<[host::runtime::type_ref::Type]>,
    output: host::runtime::type_ref::Type,
    factory: Shared<Factory>,
}

impl Binding {
    pub(crate) fn new<F, N, E, Fut>(
        definition: host::item::Definition,
        factory: F,
        normalizer: N,
    ) -> Self
    where
        F: Fn(host::operation::Arguments) -> Fut + SendSync + 'static,
        N: Fn(E) -> Dynamic + SendSync + 'static,
        Fut: Await<E>,
        E: 'static,
    {
        let fingerprint = definition.fingerprint();
        let normalizer = Shared::new(normalizer);
        let factory: Shared<Factory> = Shared::new(move |arguments| {
            let pending = factory(arguments);
            let normalizer = normalizer.clone();
            Box::pin(async move {
                match pending.await {
                    Ok(value) => value,
                    Err(error) => normalizer(error),
                }
            })
        });
        Self {
            definition,
            fingerprint,
            factory,
        }
    }

    pub(crate) fn select(
        &self,
        owner: &host::facade::Dependency,
        proof: &host::operation::Proof,
    ) -> Option<Selected> {
        let (inputs, output) = proof.types_for(owner, &self.definition)?;
        Some(Selected {
            owner: owner.clone(),
            item: self.fingerprint.clone(),
            path: proof.path().into(),
            inputs: inputs.into(),
            output: output.clone(),
            factory: self.factory.clone(),
        })
    }
}

impl Selected {
    /// Return the exact owning facade root.
    #[must_use]
    pub fn facade(&self) -> &str {
        self.owner.root()
    }

    /// Return the exact selected item fingerprint.
    #[must_use]
    pub const fn fingerprint(&self) -> &host::item::Fingerprint {
        &self.item
    }

    /// Return the canonical qualified operation path.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Format the exact compiler-proven input types.
    #[must_use]
    pub fn inputs(&self) -> Box<[String]> {
        self.inputs
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .into_boxed_slice()
    }

    /// Format the exact compiler-proven output type.
    #[must_use]
    pub fn output(&self) -> String {
        self.output.to_string()
    }

    pub(crate) fn start(self, arguments: host::operation::Arguments) -> Pending {
        (self.factory)(arguments)
    }
}

impl fmt::Debug for Binding {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Binding(<private>)")
    }
}

impl fmt::Debug for Selected {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Selected(<private>)")
    }
}

#[cfg(test)]
mod tests {
    use super::Binding;
    use crate::{host, Dynamic, ImmutableString};
    use std::any::TypeId;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::task::{Context, Poll, Waker};

    #[derive(Clone, Copy)]
    struct NativeError;

    #[test]
    fn exact_binding_joins_the_compiler_proofs_without_exposing_callbacks() {
        let facade = facade("effects", 1);
        let definition = item(&facade, "effects::load");
        let proof = proof(&facade, &definition);
        let mut builder = host::runtime::Builder::new(facade);
        bind(&mut builder, &definition).unwrap();
        let mut registry = host::runtime::Registry::new();
        registry.install(builder.finish()).unwrap();

        let selected = registry.select_operation(&proof).unwrap();

        assert_eq!(selected.path(), "effects::load");
        assert_eq!(selected.facade(), "effects");
        assert_eq!(selected.fingerprint(), &definition.fingerprint());
        assert_eq!(selected.inputs().as_ref(), ["i64"]);
        assert_eq!(selected.output(), "String");
        assert_eq!(format!("{selected:?}"), "Selected(<private>)");
    }

    #[test]
    fn duplicate_exact_operation_binding_is_rejected() {
        let facade = facade("effects", 1);
        let definition = item(&facade, "effects::load");
        let mut builder = host::runtime::Builder::new(facade);
        bind(&mut builder, &definition).unwrap();

        assert!(matches!(
            bind(&mut builder, &definition),
            Err(host::runtime::Error::DuplicateOperationBinding { item })
                if item == "effects::load"
        ));
    }

    #[test]
    fn registration_and_proof_selection_never_start_the_factory() {
        let starts = Arc::new(AtomicUsize::new(0));
        let facade = facade("effects", 1);
        let definition = item(&facade, "effects::load");
        let proof = proof(&facade, &definition);
        let mut builder = host::runtime::Builder::new(facade);
        let observed = starts.clone();
        builder
            .bind_operation(
                &definition,
                move |arguments| {
                    observed.fetch_add(1, Ordering::SeqCst);
                    async move {
                        Ok::<_, NativeError>(arguments.into_values().into_vec().remove(0))
                    }
                },
                |_error| Dynamic::from("normalized"),
            )
            .unwrap();
        let mut registry = host::runtime::Registry::new();
        registry.install(builder.finish()).unwrap();

        let _selected = registry.select_operation(&proof).unwrap();

        assert_eq!(starts.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn selected_operation_starts_once_and_normalizes_native_failure() {
        let starts = Arc::new(AtomicUsize::new(0));
        let facade = facade("effects", 1);
        let definition = item(&facade, "effects::load");
        let proof = proof(&facade, &definition);
        let mut builder = host::runtime::Builder::new(facade);
        let observed = starts.clone();
        builder
            .bind_operation(
                &definition,
                move |_arguments| {
                    observed.fetch_add(1, Ordering::SeqCst);
                    core::future::ready(Err::<Dynamic, _>(NativeError))
                },
                |_error| Dynamic::from("normalized"),
            )
            .unwrap();
        let mut registry = host::runtime::Registry::new();
        registry.install(builder.finish()).unwrap();

        let selected = registry.select_operation(&proof).unwrap();
        let mut pending = selected.start(host::operation::Arguments::seal(
            vec![Dynamic::from(7_i64)].into_boxed_slice(),
        ));

        assert_eq!(starts.load(Ordering::SeqCst), 1);
        let mut context = Context::from_waker(Waker::noop());
        let Poll::Ready(output) = pending.as_mut().poll(&mut context) else {
            panic!("ready native failure must normalize without another poll");
        };
        assert_eq!(output.into_string().unwrap(), "normalized");
        assert_eq!(starts.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn missing_operation_binding_fails_before_any_factory_can_start() {
        let facade = facade("effects", 1);
        let definition = item(&facade, "effects::load");
        let proof = proof(&facade, &definition);
        let mut registry = host::runtime::Registry::new();
        registry
            .install(host::runtime::Builder::new(facade).finish())
            .unwrap();

        assert!(matches!(
            registry.select_operation(&proof),
            Err(host::runtime::Error::MissingOperationBinding { item })
                if item == "effects::load"
        ));
    }

    #[test]
    fn binding_rejects_unknown_wrong_kind_and_wrong_signature_selections() {
        let facade = facade("effects", 1);
        let function = item(&facade, "effects::ready");
        let mut builder = host::runtime::Builder::new(facade);

        assert!(matches!(
            bind(&mut builder, &function),
            Err(host::runtime::Error::InvalidOperationKind {
                item,
                actual: host::item::Kind::Function,
            }) if item == "effects::ready"
        ));

        let unknown = operation("effects::missing", host::type_ref::Builtin::I64);
        assert!(matches!(
            bind(&mut builder, &unknown),
            Err(host::runtime::Error::UnknownOperation { facade, item })
                if facade == "effects" && item == "effects::missing"
        ));

        let wrong_signature = operation("effects::load", host::type_ref::Builtin::Bool);
        assert!(matches!(
            bind(&mut builder, &wrong_signature),
            Err(host::runtime::Error::OperationSignatureMismatch { item })
                if item == "effects::load"
        ));
    }

    #[test]
    fn binding_and_selection_reject_different_exact_facades() {
        let effects = facade("effects", 1);
        let foreign = facade("foreign", 1);
        let foreign_definition = item(&foreign, "foreign::load");
        let mut builder = host::runtime::Builder::new(effects.clone());
        assert!(matches!(
            bind(&mut builder, &foreign_definition),
            Err(host::runtime::Error::OperationFacadeMismatch { facade, item })
                if facade == "effects" && item == "foreign::load"
        ));

        let definition = item(&effects, "effects::load");
        bind(&mut builder, &definition).unwrap();
        let mut registry = host::runtime::Registry::new();
        registry.install(builder.finish()).unwrap();
        let incompatible = facade("effects", 2);
        let incompatible_definition = item(&incompatible, "effects::load");
        let incompatible_proof = proof(&incompatible, &incompatible_definition);

        assert!(matches!(
            registry.select_operation(&incompatible_proof),
            Err(host::runtime::Error::FacadeMismatch { facade }) if facade == "effects"
        ));
    }

    #[test]
    fn binding_debug_never_exposes_factory_or_normalizer_state() {
        let definition = operation("effects::load", host::type_ref::Builtin::I64);
        let binding = Binding::new(
            definition,
            |arguments| async move {
                Ok::<_, NativeError>(arguments.into_values().into_vec().remove(0))
            },
            |_error| Dynamic::from("normalized-secret"),
        );

        assert_eq!(format!("{binding:?}"), "Binding(<private>)");
    }

    fn bind<'a>(
        builder: &'a mut host::runtime::Builder,
        definition: &host::item::Definition,
    ) -> Result<&'a mut host::runtime::Builder, host::runtime::Error> {
        builder.bind_operation(
            definition,
            |arguments| async move {
                Ok::<_, NativeError>(arguments.into_values().into_vec().remove(0))
            },
            |_error| Dynamic::from("normalized"),
        )
    }

    fn facade(root: &str, abi: u32) -> host::facade::Definition {
        let mut builder =
            host::facade::Builder::new(root, host::facade::Abi::new(abi).unwrap()).unwrap();
        builder
            .item(operation("load", host::type_ref::Builtin::I64))
            .unwrap();
        let mut ready = host::signature::Builder::new();
        ready.output(host::type_ref::Type::builtin(host::type_ref::Builtin::Bool));
        builder
            .item(host::item::Definition::function("ready", ready.finish()).unwrap())
            .unwrap();
        builder.finish().unwrap()
    }

    fn item(facade: &host::facade::Definition, path: &str) -> host::item::Definition {
        facade
            .items()
            .find(|definition| definition.path() == path)
            .unwrap()
            .clone()
    }

    fn proof(
        facade: &host::facade::Definition,
        definition: &host::item::Definition,
    ) -> host::operation::Proof {
        let owner = host::facade::Dependency::new(facade.root_path(), facade.fingerprint().clone());
        host::operation::Proof::new(
            owner,
            definition,
            vec![host::runtime::type_ref::Type::builtin(
                "i64",
                TypeId::of::<crate::INT>(),
            )]
            .into_boxed_slice(),
            host::runtime::type_ref::Type::builtin("String", TypeId::of::<ImmutableString>()),
        )
    }

    fn operation(path: &str, input: host::type_ref::Builtin) -> host::item::Definition {
        let mut signature = host::signature::Builder::new();
        signature
            .parameter(host::type_ref::Type::builtin(input))
            .output(host::type_ref::Type::builtin(
                host::type_ref::Builtin::String,
            ));
        host::item::Definition::operation(path, signature.finish()).unwrap()
    }
}
