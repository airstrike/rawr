//! Executor-neutral factories for exact symbolic host streams.

use crate::func::SendSync;
use crate::host::stream::Source;
use crate::{host, Dynamic, ImmutableString, Shared};
use futures_core::Stream;
use std::fmt;
use std::pin::Pin;
use std::task::{Context, Poll};

#[cfg(feature = "sync")]
pub(crate) type Pending = Pin<Box<dyn Stream<Item = Dynamic> + Send + 'static>>;
#[cfg(not(feature = "sync"))]
pub(crate) type Pending = Pin<Box<dyn Stream<Item = Dynamic> + 'static>>;

#[cfg(feature = "sync")]
type Factory = dyn Fn(host::stream::Arguments) -> Pending + Send + Sync + 'static;
#[cfg(not(feature = "sync"))]
type Factory = dyn Fn(host::stream::Arguments) -> Pending + 'static;

#[cfg(feature = "sync")]
type Encoder = dyn Fn(
        usize,
        &Dynamic,
        &mut host::stream::identity::Writer,
    ) -> Result<(), host::stream::identity::Error>
    + Send
    + Sync
    + 'static;
#[cfg(not(feature = "sync"))]
type Encoder = dyn Fn(
        usize,
        &Dynamic,
        &mut host::stream::identity::Writer,
    ) -> Result<(), host::stream::identity::Error>
    + 'static;

#[cfg(feature = "sync")]
/// Internal trait alias for a sendable native stream binding.
#[doc(hidden)]
pub trait Native<E>: Source<Item = Result<Dynamic, E>> + Send + 'static {}
#[cfg(feature = "sync")]
impl<T, E> Native<E> for T where T: Source<Item = Result<Dynamic, E>> + Send + 'static {}

#[cfg(not(feature = "sync"))]
/// Internal trait alias for a native stream binding.
#[doc(hidden)]
pub trait Native<E>: Source<Item = Result<Dynamic, E>> + 'static {}
#[cfg(not(feature = "sync"))]
impl<T, E> Native<E> for T where T: Source<Item = Result<Dynamic, E>> + 'static {}

/// One native factory and encoder bound to an exact stream declaration.
pub(crate) struct Binding {
    definition: host::item::Definition,
    fingerprint: host::item::Fingerprint,
    encoder: Shared<Encoder>,
    factory: Shared<Factory>,
}

/// One runtime binding joined to a compiler-selected stream proof.
pub struct Selected {
    owner: host::facade::Dependency,
    item: host::item::Fingerprint,
    path: ImmutableString,
    version: host::stream::Version,
    inputs: Box<[host::runtime::type_ref::Type]>,
    output: host::runtime::type_ref::Type,
    encoder: Shared<Encoder>,
    factory: Shared<Factory>,
}

struct Normalized<S, N> {
    stream: Pin<Box<S>>,
    normalize: Shared<N>,
}

impl Binding {
    pub(crate) fn new<F, I, N, E, S>(
        definition: host::item::Definition,
        encoder: I,
        factory: F,
        normalize: N,
    ) -> Self
    where
        F: Fn(host::stream::Arguments) -> S + SendSync + 'static,
        I: Fn(
                usize,
                &Dynamic,
                &mut host::stream::identity::Writer,
            ) -> Result<(), host::stream::identity::Error>
            + SendSync
            + 'static,
        N: Fn(E) -> Dynamic + SendSync + 'static,
        S: Native<E>,
        E: 'static,
    {
        let fingerprint = definition.fingerprint();
        let normalize = Shared::new(normalize);
        let factory: Shared<Factory> = Shared::new(move |arguments| {
            Box::pin(Normalized {
                stream: Box::pin(factory(arguments)),
                normalize: normalize.clone(),
            })
        });
        Self {
            definition,
            fingerprint,
            encoder: Shared::new(encoder),
            factory,
        }
    }

    pub(crate) fn select(
        &self,
        owner: &host::facade::Dependency,
        proof: &host::stream::Proof,
    ) -> Option<Selected> {
        let (inputs, output) = proof.types_for(owner, &self.definition)?;
        Some(Selected {
            owner: owner.clone(),
            item: self.fingerprint.clone(),
            path: proof.path().into(),
            version: proof.identity_version(),
            inputs: inputs.into(),
            output: output.clone(),
            encoder: self.encoder.clone(),
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

    /// Return the canonical qualified stream operation path.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Return the exact canonical identity protocol version.
    #[must_use]
    pub const fn identity_version(&self) -> host::stream::Version {
        self.version
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

    /// Format the exact compiler-proven output item type.
    #[must_use]
    pub fn output(&self) -> String {
        self.output.to_string()
    }

    pub(crate) fn identify(
        &self,
        arguments: &host::stream::Arguments,
    ) -> Result<Box<[host::stream::identity::Contribution]>, (usize, host::stream::identity::Error)>
    {
        arguments
            .values()
            .iter()
            .enumerate()
            .map(|(index, value)| {
                let mut writer = host::stream::identity::Writer::new();
                (self.encoder)(index, value, &mut writer).map_err(|error| (index, error))?;
                writer.finish().map_err(|error| (index, error))
            })
            .collect::<Result<Vec<_>, _>>()
            .map(Vec::into_boxed_slice)
    }

    pub(crate) fn start(self, arguments: host::stream::Arguments) -> Pending {
        (self.factory)(arguments)
    }
}

impl<S, N, E> Stream for Normalized<S, N>
where
    S: Source<Item = Result<Dynamic, E>>,
    N: Fn(E) -> Dynamic,
{
    type Item = Dynamic;

    fn poll_next(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.as_mut().get_mut();
        match Source::poll_next(this.stream.as_mut(), context) {
            Poll::Ready(Some(Ok(value))) => Poll::Ready(Some(value)),
            Poll::Ready(Some(Err(error))) => Poll::Ready(Some((this.normalize)(error))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
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
    use futures_core::Stream;
    use std::any::TypeId;
    use std::collections::VecDeque;
    use std::pin::Pin;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::task::{Context, Poll, Waker};

    #[derive(Clone, Copy)]
    struct NativeError;

    struct Values(VecDeque<Result<Dynamic, NativeError>>);

    impl Stream for Values {
        type Item = Result<Dynamic, NativeError>;

        fn poll_next(
            mut self: Pin<&mut Self>,
            _context: &mut Context<'_>,
        ) -> Poll<Option<Self::Item>> {
            Poll::Ready(self.0.pop_front())
        }
    }

    #[test]
    fn selection_and_identity_sealing_do_not_start_the_factory() {
        let encodes = Arc::new(AtomicUsize::new(0));
        let starts = Arc::new(AtomicUsize::new(0));
        let facade = facade(1);
        let definition = item(&facade);
        let proof = proof(&facade, &definition);
        let mut builder = host::runtime::Builder::new(facade);
        let observed_encodes = encodes.clone();
        let observed_starts = starts.clone();
        builder
            .bind_stream_operation(
                &definition,
                move |index, value, writer| {
                    observed_encodes.fetch_add(1, Ordering::SeqCst);
                    assert_eq!(index, 0);
                    writer.i64(value.as_int().map_err(host::stream::identity::Error::new)?);
                    Ok(())
                },
                move |_arguments| {
                    observed_starts.fetch_add(1, Ordering::SeqCst);
                    Values(VecDeque::new())
                },
                |_error| Dynamic::from("normalized"),
            )
            .unwrap();
        let mut registry = host::runtime::Registry::new();
        registry.install(builder.finish()).unwrap();

        let selected = registry.select_stream(&proof).unwrap();
        assert_eq!(selected.identity_version(), host::stream::Version::new(1));
        assert_eq!(starts.load(Ordering::SeqCst), 0);
        assert_eq!(encodes.load(Ordering::SeqCst), 0);

        let arguments =
            host::stream::Arguments::seal(vec![Dynamic::from(7_i64)].into_boxed_slice());
        let contributions = selected.identify(&arguments).unwrap();
        assert_eq!(contributions.len(), 1);
        assert_eq!(encodes.load(Ordering::SeqCst), 1);
        assert_eq!(starts.load(Ordering::SeqCst), 0);

        let _pending = selected.start(arguments);
        assert_eq!(starts.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn encoder_failure_is_typed_and_prevents_start() {
        let starts = Arc::new(AtomicUsize::new(0));
        let facade = facade(1);
        let definition = item(&facade);
        let proof = proof(&facade, &definition);
        let mut builder = host::runtime::Builder::new(facade);
        let observed_starts = starts.clone();
        builder
            .bind_stream_operation(
                &definition,
                |_index, _value, _writer| Err(host::stream::identity::Error::new("not canonical")),
                move |_arguments| {
                    observed_starts.fetch_add(1, Ordering::SeqCst);
                    Values(VecDeque::new())
                },
                |_error| Dynamic::from("normalized"),
            )
            .unwrap();
        let mut registry = host::runtime::Registry::new();
        registry.install(builder.finish()).unwrap();
        let selected = registry.select_stream(&proof).unwrap();
        let arguments =
            host::stream::Arguments::seal(vec![Dynamic::from(7_i64)].into_boxed_slice());

        let (index, error) = selected.identify(&arguments).unwrap_err();
        assert_eq!(index, 0);
        assert_eq!(error.to_string(), "not canonical");
        assert_eq!(starts.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn omitted_encoder_contribution_is_typed_and_prevents_start() {
        let starts = Arc::new(AtomicUsize::new(0));
        let facade = facade(1);
        let definition = item(&facade);
        let proof = proof(&facade, &definition);
        let mut builder = host::runtime::Builder::new(facade);
        let observed_starts = starts.clone();
        builder
            .bind_stream_operation(
                &definition,
                |_index, _value, _writer| Ok(()),
                move |_arguments| {
                    observed_starts.fetch_add(1, Ordering::SeqCst);
                    Values(VecDeque::new())
                },
                |_error| Dynamic::from("normalized"),
            )
            .unwrap();
        let mut registry = host::runtime::Registry::new();
        registry.install(builder.finish()).unwrap();
        let selected = registry.select_stream(&proof).unwrap();
        let arguments =
            host::stream::Arguments::seal(vec![Dynamic::from(7_i64)].into_boxed_slice());

        let (index, error) = selected.identify(&arguments).unwrap_err();
        assert_eq!(index, 0);
        assert_eq!(
            error.to_string(),
            "stream identity encoder emitted no canonical atoms"
        );
        assert_eq!(starts.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn native_failures_normalize_inside_the_binding() {
        let facade = facade(1);
        let definition = item(&facade);
        let proof = proof(&facade, &definition);
        let mut builder = host::runtime::Builder::new(facade);
        builder
            .bind_stream_operation(
                &definition,
                |_index, value, writer| {
                    writer.i64(value.as_int().map_err(host::stream::identity::Error::new)?);
                    Ok(())
                },
                |_arguments| Values(VecDeque::from([Err(NativeError)])),
                |_error| Dynamic::from("normalized"),
            )
            .unwrap();
        let mut registry = host::runtime::Registry::new();
        registry.install(builder.finish()).unwrap();
        let selected = registry.select_stream(&proof).unwrap();
        let arguments =
            host::stream::Arguments::seal(vec![Dynamic::from(7_i64)].into_boxed_slice());
        let mut pending = selected.start(arguments);
        let mut context = Context::from_waker(Waker::noop());

        let Poll::Ready(Some(value)) = pending.as_mut().poll_next(&mut context) else {
            panic!("normalized stream item should be immediately ready");
        };
        assert_eq!(value.into_string().unwrap(), "normalized");
    }

    #[test]
    fn identity_protocol_version_participates_in_exact_fingerprint() {
        let first = stream("ticks", 1);
        let second = stream("ticks", 2);

        assert_ne!(first.fingerprint(), second.fingerprint());
    }

    #[test]
    fn canonical_identity_covers_order_type_version_and_exact_equivalence() {
        let first_facade = generic_facade(1);
        let first_definition = item(&first_facade);
        let first_owner = host::facade::Dependency::new(
            first_facade.root_path(),
            first_facade.fingerprint().clone(),
        );
        let i64_type = host::runtime::type_ref::Type::builtin("i64", TypeId::of::<crate::INT>());
        let string_type =
            host::runtime::type_ref::Type::builtin("String", TypeId::of::<ImmutableString>());
        let ordered = host::stream::Proof::new(
            first_owner.clone(),
            &first_definition,
            vec![i64_type.clone(), i64_type.clone()].into_boxed_slice(),
            string_type.clone(),
        )
        .unwrap();
        let typed = host::stream::Proof::new(
            first_owner,
            &first_definition,
            vec![i64_type.clone(), string_type.clone()].into_boxed_slice(),
            string_type.clone(),
        )
        .unwrap();
        let mut one = host::stream::identity::Writer::new();
        one.i64(1);
        let one = one.finish().unwrap();
        let mut two = host::stream::identity::Writer::new();
        two.i64(2);
        let two = two.finish().unwrap();
        let contributions = [one.clone(), two.clone()];
        let identity = ordered.identity(&contributions, &[], &[]).unwrap();

        assert_eq!(
            identity,
            ordered.identity(&contributions, &[], &[]).unwrap()
        );
        assert_ne!(
            identity,
            ordered
                .identity(&[two.clone(), one.clone()], &[], &[])
                .unwrap()
        );
        assert_ne!(identity, typed.identity(&contributions, &[], &[]).unwrap());

        let second_facade = generic_facade(2);
        let second_definition = item(&second_facade);
        let second_owner = host::facade::Dependency::new(
            second_facade.root_path(),
            second_facade.fingerprint().clone(),
        );
        let versioned = host::stream::Proof::new(
            second_owner,
            &second_definition,
            vec![i64_type.clone(), i64_type].into_boxed_slice(),
            string_type,
        )
        .unwrap();
        assert_ne!(
            identity,
            versioned.identity(&contributions, &[], &[]).unwrap()
        );
    }

    fn facade(version: u32) -> host::facade::Definition {
        let mut builder =
            host::facade::Builder::new("effects", host::facade::Abi::new(1).unwrap()).unwrap();
        builder.item(stream("ticks", version)).unwrap();
        builder.finish().unwrap()
    }

    fn generic_facade(version: u32) -> host::facade::Definition {
        let mut builder =
            host::facade::Builder::new("effects", host::facade::Abi::new(1).unwrap()).unwrap();
        let mut signature = host::signature::Builder::new();
        signature.type_parameter("First", &[]).unwrap();
        signature.type_parameter("Second", &[]).unwrap();
        signature.parameter(host::type_ref::Type::parameter("First").unwrap());
        signature.parameter(host::type_ref::Type::parameter("Second").unwrap());
        signature.output(host::type_ref::Type::builtin(
            host::type_ref::Builtin::String,
        ));
        builder
            .item(
                host::item::Definition::stream_operation(
                    "ticks",
                    signature.finish(),
                    host::stream::Version::new(version),
                )
                .unwrap(),
            )
            .unwrap();
        builder.finish().unwrap()
    }

    fn item(facade: &host::facade::Definition) -> host::item::Definition {
        facade.items().next().unwrap().clone()
    }

    fn proof(
        facade: &host::facade::Definition,
        definition: &host::item::Definition,
    ) -> host::stream::Proof {
        let owner = host::facade::Dependency::new(facade.root_path(), facade.fingerprint().clone());
        host::stream::Proof::new(
            owner,
            definition,
            vec![host::runtime::type_ref::Type::builtin(
                "i64",
                TypeId::of::<crate::INT>(),
            )]
            .into_boxed_slice(),
            host::runtime::type_ref::Type::builtin("String", TypeId::of::<ImmutableString>()),
        )
        .unwrap()
    }

    fn stream(path: &str, version: u32) -> host::item::Definition {
        let mut signature = host::signature::Builder::new();
        signature
            .parameter(host::type_ref::Type::builtin(host::type_ref::Builtin::I64))
            .output(host::type_ref::Type::builtin(
                host::type_ref::Builtin::String,
            ));
        host::item::Definition::stream_operation(
            path,
            signature.finish(),
            host::stream::Version::new(version),
        )
        .unwrap()
    }

    #[test]
    fn binding_debug_redacts_encoder_factory_and_normalizer() {
        let binding = Binding::new(
            stream("effects::ticks", 1),
            |_index, _value, writer| {
                writer.bool(true);
                Ok(())
            },
            |_arguments| Values(VecDeque::new()),
            |_error| Dynamic::from("normalized-secret"),
        );

        assert_eq!(format!("{binding:?}"), "Binding(<private>)");
    }
}
