//! Native implementations bound to exact compiler facade proofs.

mod call;
#[cfg(feature = "rust-import")]
pub mod event;
pub mod invocation;
pub mod operation;
pub mod stream;
pub(crate) mod type_ref;
mod unwitnessed;
pub(crate) mod widget;
mod witness;

pub use call::{Call, Equal, Error as WitnessError};
pub use type_ref::Error as ValueError;
pub use unwitnessed::{Deferred, Unwitnessed};
pub use witness::Witness;

use crate::func::SendSync;
use crate::{host, Dynamic, EvalAltResult, ImmutableString, Shared};
use core::future;
use std::collections::BTreeMap;
use std::fmt;

trait Callback: SendSync {
    fn invoke(
        &self,
        call: &Call<'_>,
        args: &mut [&mut Dynamic],
    ) -> Result<Dynamic, Box<EvalAltResult>>;
}

trait TypeAttestor: SendSync {
    fn attest(&self, value: &Dynamic, expected: &Instantiation<'_>) -> Option<Attested>;
}

impl<F> TypeAttestor for F
where
    F: Fn(&Dynamic, &Instantiation<'_>) -> Option<Attested> + SendSync,
{
    fn attest(&self, value: &Dynamic, expected: &Instantiation<'_>) -> Option<Attested> {
        self(value, expected)
    }
}

/// Sealed evidence that a facade accepted one exact generic instantiation.
#[must_use]
pub struct Attested {
    private: (),
}

/// Opaque proof that one imported identity installed its complete alias cohort.
#[cfg(feature = "rust-import")]
#[doc(hidden)]
#[must_use]
pub struct ImportedTypeAttestation {
    selector: [u8; 32],
    rows: usize,
}

#[cfg(feature = "rust-import")]
impl ImportedTypeAttestation {
    /// Borrow the exact canonical imported identity that was bound.
    #[doc(hidden)]
    #[must_use]
    pub const fn selector(&self) -> &[u8; 32] {
        &self.selector
    }

    /// Return the complete number of alias paths installed for this identity.
    #[doc(hidden)]
    #[must_use]
    pub const fn rows(&self) -> usize {
        self.rows
    }
}

/// Opaque generated structural adapter authenticated against one manifest.
#[doc(hidden)]
#[derive(Clone)]
pub struct StructuralAdapter {
    pub(crate) definition: host::structural::Definition,
}

/// Read-only access to one compiler-proven generic host instantiation.
pub struct Instantiation<'a> {
    ty: &'a type_ref::Type,
    model: &'a crate::program::model::Model,
    registry: &'a Registry,
}

/// The namespace a runtime binding targets where a path names both.
#[derive(Clone, Copy, Eq, PartialEq)]
enum Namespace {
    Value,
    Macro,
    /// The reader of a tuple variant's field, beside its constructor.
    Reader,
}

struct Plain<F>(F);

impl<F> Callback for Plain<F>
where
    F: Fn(&mut [&mut Dynamic]) -> Result<Dynamic, Box<EvalAltResult>> + SendSync,
{
    fn invoke(
        &self,
        _call: &Call<'_>,
        args: &mut [&mut Dynamic],
    ) -> Result<Dynamic, Box<EvalAltResult>> {
        (self.0)(args)
    }
}

struct Contextual<F>(F);

impl<F> Callback for Contextual<F>
where
    F: Fn(&Call<'_>, &mut [&mut Dynamic]) -> Result<Dynamic, Box<EvalAltResult>> + SendSync,
{
    fn invoke(
        &self,
        call: &Call<'_>,
        args: &mut [&mut Dynamic],
    ) -> Result<Dynamic, Box<EvalAltResult>> {
        (self.0)(call, args)
    }
}

/// Collects native implementations for one finished compiler facade.
pub struct Builder {
    definition: host::facade::Definition,
    bindings: BTreeMap<host::item::Fingerprint, Shared<dyn Callback>>,
    /// One binding per instance of a generic imported item, keyed by the
    /// item and its instance's types (`rand::random::<f64>` and `::<u8>`).
    #[cfg(feature = "rust-import")]
    instances: BTreeMap<(host::item::Fingerprint, [u8; 32]), Shared<dyn Callback>>,
    structural: BTreeMap<host::structural::Definition, Shared<dyn Callback>>,
    #[cfg(feature = "rust-import")]
    events: BTreeMap<event::Key, Shared<dyn Callback>>,
    operations: BTreeMap<host::item::Fingerprint, operation::Binding>,
    streams: BTreeMap<host::item::Fingerprint, stream::Binding>,
    projections: BTreeMap<host::projection::Key, Shared<dyn Callback>>,
    attestations: BTreeMap<ImmutableString, Shared<dyn TypeAttestor>>,
    widget: Option<widget::Binding>,
}

/// One finished runtime implementation of an exact compiler facade.
pub struct Facade {
    dependency: host::facade::Dependency,
    dependencies: Box<[host::facade::Dependency]>,
    bindings: BTreeMap<host::item::Fingerprint, Shared<dyn Callback>>,
    #[cfg(feature = "rust-import")]
    instances: BTreeMap<(host::item::Fingerprint, [u8; 32]), Shared<dyn Callback>>,
    structural: BTreeMap<host::structural::Definition, Shared<dyn Callback>>,
    #[cfg(feature = "rust-import")]
    events: BTreeMap<event::Key, Shared<dyn Callback>>,
    operations: BTreeMap<host::item::Fingerprint, operation::Binding>,
    streams: BTreeMap<host::item::Fingerprint, stream::Binding>,
    projections: BTreeMap<host::projection::Key, Shared<dyn Callback>>,
    attestations: BTreeMap<ImmutableString, Shared<dyn TypeAttestor>>,
    widget: Option<widget::Binding>,
}

/// Runtime facade implementations available to one application launch.
///
/// A runner builds its registry once. Each script generation then takes
/// [`Registry::next_generation`], which shares these bindings and this
/// runner's witness authority.
pub struct Registry {
    facades: Shared<BTreeMap<ImmutableString, Facade>>,
    witness: witness::Authority,
    #[cfg(feature = "rust-import")]
    imported_receipt: Option<Shared<crate::package::import::runner::account::Receipt>>,
}

/// One exact static operation required by an executable's private lowering.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct Requirement {
    owner: host::facade::Dependency,
    binding: Binding,
    path: ImmutableString,
    operands: Box<[type_ref::Type]>,
    output: type_ref::Type,
    receiver: Option<crate::receiver::Mode>,
    /// The instance of a generic imported item this call uses.
    #[cfg(feature = "rust-import")]
    instance: Option<[u8; 32]>,
    bound: Bound,
}

/// The binding a registry resolved for one requirement, remembered so repeat
/// calls skip the facade lookup. Registries only add facade roots and never
/// replace them, and every generation of one runner shares them, so a binding
/// found under one runner's authority stays exact.
/// A clone starts empty and the cache never takes part in comparisons.
#[derive(Default)]
pub(crate) struct Bound(OnceCell<(witness::Authority, Shared<dyn Callback>)>);

type OnceCell<T> = once_cell::sync::OnceCell<T>;

impl Bound {
    pub(crate) const fn new() -> Self {
        Self(OnceCell::new())
    }

    fn get(&self, authority: witness::Authority) -> Option<&Shared<dyn Callback>> {
        self.0
            .get()
            .filter(|(owner, _)| owner.same_runner(authority))
            .map(|(_, binding)| binding)
    }

    fn remember(&self, authority: witness::Authority, binding: &Shared<dyn Callback>) {
        let _ = self.0.set((authority, binding.clone()));
    }
}

impl Clone for Bound {
    fn clone(&self) -> Self {
        Self::default()
    }
}

impl fmt::Debug for Bound {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Bound")
    }
}

impl PartialEq for Bound {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Eq for Bound {}

impl PartialOrd for Bound {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Bound {
    fn cmp(&self, _: &Self) -> std::cmp::Ordering {
        std::cmp::Ordering::Equal
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum Binding {
    Item(host::item::Fingerprint),
    // Only the Rust importer admits generated structural adapters.
    #[cfg_attr(not(feature = "rust-import"), allow(dead_code))]
    Structural(host::structural::Definition),
    #[cfg(feature = "rust-import")]
    Event(Box<event::Key>),
}

/// A runtime facade proof could not be built or bound to an application.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Error {
    /// Shared generated-child metadata accounting refused this runtime.
    #[cfg(feature = "rust-import")]
    ImportedAllocation,
    /// A linked profile could not contribute its retained runtime bindings.
    Profile {
        /// Stable profile-owned reason.
        reason: ImmutableString,
    },
    /// A binding names no item in its finished compiler facade.
    UnknownItem {
        /// Exact facade root.
        facade: ImmutableString,
        /// Requested qualified item path.
        item: ImmutableString,
    },
    /// A type attestation names no generic type in its finished facade.
    UnknownGenericType {
        /// Exact facade root.
        facade: ImmutableString,
        /// Requested qualified type path.
        ty: ImmutableString,
    },
    /// An imported type attestation selector names no exact imported definition.
    #[cfg(feature = "rust-import")]
    UnknownImportedType {
        /// Exact facade root.
        facade: ImmutableString,
        /// Rejected compiler-derived imported identity.
        selector: [u8; 32],
    },
    /// A projection binding names no direct edge owned by its facade.
    UnknownProjection {
        /// Exact facade root.
        facade: ImmutableString,
        /// Requested source type path.
        source: ImmutableString,
        /// Requested target type path.
        target: ImmutableString,
    },
    /// One facade builder tried to bind the same exact item twice.
    DuplicateBinding {
        /// Repeated qualified item path.
        item: ImmutableString,
    },
    /// A generated structural adapter belongs to another exact facade.
    StructuralFacadeMismatch {
        /// Rejected structural owner path.
        operation: ImmutableString,
    },
    /// One generated structural adapter was bound more than once.
    DuplicateStructuralBinding {
        /// Repeated structural owner path.
        operation: ImmutableString,
    },
    /// One facade builder tried to bind the same exact async operation twice.
    DuplicateOperationBinding {
        /// Repeated qualified operation path.
        item: ImmutableString,
    },
    /// One facade builder tried to bind the same exact stream operation twice.
    DuplicateStreamBinding {
        /// Repeated qualified stream operation path.
        item: ImmutableString,
    },
    /// A selected operation names no item in this exact compiler facade.
    UnknownOperation {
        /// Exact facade root.
        facade: ImmutableString,
        /// Requested qualified operation path.
        item: ImmutableString,
    },
    /// A selected runtime operation belongs to a different facade contract.
    OperationFacadeMismatch {
        /// Exact runtime facade root.
        facade: ImmutableString,
        /// Rejected qualified operation path.
        item: ImmutableString,
    },
    /// A selected facade item is not a symbolic async operation.
    InvalidOperationKind {
        /// Rejected qualified item path.
        item: ImmutableString,
        /// Actual static facade item kind.
        actual: host::item::Kind,
    },
    /// A selected operation does not match an exact declared overload.
    OperationSignatureMismatch {
        /// Rejected qualified operation path.
        item: ImmutableString,
    },
    /// A selected stream names no item in this exact compiler facade.
    UnknownStream {
        /// Exact facade root.
        facade: ImmutableString,
        /// Requested qualified stream operation path.
        item: ImmutableString,
    },
    /// A selected runtime stream belongs to a different facade contract.
    StreamFacadeMismatch {
        /// Exact runtime facade root.
        facade: ImmutableString,
        /// Rejected qualified stream operation path.
        item: ImmutableString,
    },
    /// A selected facade item is not a symbolic stream operation.
    InvalidStreamKind {
        /// Rejected qualified item path.
        item: ImmutableString,
        /// Actual static facade item kind.
        actual: host::item::Kind,
    },
    /// A selected stream does not match an exact declared overload.
    StreamSignatureMismatch {
        /// Rejected qualified stream operation path.
        item: ImmutableString,
    },
    /// One facade builder tried to attest the same generic type twice.
    DuplicateTypeAttestation {
        /// Repeated qualified type path.
        ty: ImmutableString,
    },
    /// One facade builder tried to bind the same projection twice.
    DuplicateProjectionBinding {
        /// Repeated semantic projection edge.
        projection: ImmutableString,
    },
    /// One runtime registry contains two implementations for the same root.
    DuplicateFacade {
        /// Colliding facade root.
        facade: ImmutableString,
    },
    /// The application requires a facade absent from the runtime registry.
    MissingFacade {
        /// Required facade root.
        facade: ImmutableString,
    },
    /// A runtime facade root exists but its exact compiler proof differs.
    FacadeMismatch {
        /// Conflicting facade root.
        facade: ImmutableString,
    },
    /// The matching runtime facade does not implement a retained operation.
    MissingBinding {
        /// Required qualified item path.
        item: ImmutableString,
    },
    /// The matching runtime facade lacks one private generated structural adapter.
    MissingStructuralBinding {
        /// Required structural owner path.
        operation: ImmutableString,
    },
    /// The matching runtime facade does not bind a selected async operation.
    MissingOperationBinding {
        /// Required qualified operation path.
        item: ImmutableString,
    },
    /// A runtime operation binding disagrees with its opaque compiler proof.
    OperationProofMismatch {
        /// Rejected qualified operation path.
        item: ImmutableString,
    },
    /// The matching runtime facade does not bind a selected stream operation.
    MissingStreamBinding {
        /// Required qualified stream operation path.
        item: ImmutableString,
    },
    /// A runtime stream binding disagrees with its opaque compiler proof.
    StreamProofMismatch {
        /// Rejected qualified stream operation path.
        item: ImmutableString,
    },
    /// The matching runtime facade does not implement a retained projection.
    MissingProjectionBinding {
        /// Required semantic projection edge.
        projection: ImmutableString,
    },
    /// An exact generic host type has no facade-owned attestor.
    MissingTypeAttestation {
        /// Required exact generic host type.
        ty: ImmutableString,
    },
    /// The facade has no owned one-parameter type with this Rust identity.
    UnknownWidgetType {
        /// Exact facade root.
        facade: ImmutableString,
        /// Requested Rust representation.
        ty: ImmutableString,
    },
    /// More than one facade class uses the requested widget representation.
    AmbiguousWidgetType {
        /// Exact facade root.
        facade: ImmutableString,
        /// Ambiguous Rust representation.
        ty: ImmutableString,
    },
    /// One facade builder tried to bind its private widget activator twice.
    DuplicateWidgetBinding {
        /// Exact facade root.
        facade: ImmutableString,
    },
    /// The matching runtime facade has no activator for the exact widget target.
    MissingWidgetBinding {
        /// Required exact open widget target.
        ty: ImmutableString,
    },
}

impl Error {
    /// Normalize one linked profile binding failure at the generic runtime seam.
    #[doc(hidden)]
    pub fn profile(error: impl fmt::Display) -> Self {
        Self::Profile {
            reason: error.to_string().into(),
        }
    }
}

impl Builder {
    /// Start implementing one exact finished compiler facade.
    #[must_use]
    pub fn new(definition: host::facade::Definition) -> Self {
        Self {
            definition,
            bindings: BTreeMap::new(),
            #[cfg(feature = "rust-import")]
            instances: BTreeMap::new(),
            structural: BTreeMap::new(),
            #[cfg(feature = "rust-import")]
            events: BTreeMap::new(),
            operations: BTreeMap::new(),
            streams: BTreeMap::new(),
            projections: BTreeMap::new(),
            attestations: BTreeMap::new(),
            widget: None,
        }
    }

    /// Return whether this exact reconstructed facade retains an imported
    /// selector that is not bound yet. An item reached through two paths
    /// (imported by name and called by its full path) is bound once.
    #[cfg(feature = "rust-import")]
    #[doc(hidden)]
    #[must_use]
    pub fn accepts_imported(&self, selector: &[u8; 32]) -> bool {
        self.definition.items().any(|item| {
            item.imported_identity()
                .is_some_and(|identity| identity.fingerprint() == *selector)
                && !self.bindings.contains_key(&item.fingerprint())
        })
    }

    /// Return whether a linked profile already owns this generic type attestation.
    #[doc(hidden)]
    #[must_use]
    pub fn has_type_attestation(&self, path: &str) -> bool {
        self.attestations.contains_key(path)
    }

    /// Bind one exact symbolic operation to an executor-neutral async factory.
    ///
    /// The native error never crosses the binding. `normalize` transforms it
    /// into the operation's declared output representation; the resulting
    /// value remains untrusted until the selected output proof validates it.
    #[cfg(feature = "sync")]
    pub fn bind_operation<F, N, E, Fut>(
        &mut self,
        selected: &host::item::Definition,
        factory: F,
        normalize: N,
    ) -> Result<&mut Self, Error>
    where
        F: Fn(host::operation::Arguments) -> Fut + SendSync + 'static,
        N: Fn(E) -> Dynamic + SendSync + 'static,
        Fut: future::Future<Output = Result<Dynamic, E>> + Send + 'static,
        E: 'static,
    {
        self.bind_operation_inner(selected, factory, normalize)
    }

    /// Bind one exact symbolic operation to an executor-neutral async factory.
    ///
    /// The native error never crosses the binding. `normalize` transforms it
    /// into the operation's declared output representation; the resulting
    /// value remains untrusted until the selected output proof validates it.
    #[cfg(not(feature = "sync"))]
    pub fn bind_operation<F, N, E, Fut>(
        &mut self,
        selected: &host::item::Definition,
        factory: F,
        normalize: N,
    ) -> Result<&mut Self, Error>
    where
        F: Fn(host::operation::Arguments) -> Fut + SendSync + 'static,
        N: Fn(E) -> Dynamic + SendSync + 'static,
        Fut: future::Future<Output = Result<Dynamic, E>> + 'static,
        E: 'static,
    {
        self.bind_operation_inner(selected, factory, normalize)
    }

    fn bind_operation_inner<F, N, E, Fut>(
        &mut self,
        selected: &host::item::Definition,
        factory: F,
        normalize: N,
    ) -> Result<&mut Self, Error>
    where
        F: Fn(host::operation::Arguments) -> Fut + SendSync + 'static,
        N: Fn(E) -> Dynamic + SendSync + 'static,
        Fut: operation::Await<E>,
        E: 'static,
    {
        let item: ImmutableString = selected.path().into();
        if selected
            .path_ref()
            .segments()
            .first()
            .is_none_or(|root| root.as_str() != self.definition.root())
        {
            return Err(Error::OperationFacadeMismatch {
                facade: self.definition.root().into(),
                item,
            });
        }
        let Some(overloads) = self.definition.overloads(&item) else {
            return Err(Error::UnknownOperation {
                facade: self.definition.root().into(),
                item,
            });
        };
        if selected.kind() != host::item::Kind::Operation {
            return Err(Error::InvalidOperationKind {
                item,
                actual: selected.kind(),
            });
        }
        let fingerprint = selected.fingerprint();
        let Some(definition) = overloads
            .iter()
            .find(|definition| definition.fingerprint() == fingerprint)
            .cloned()
        else {
            return Err(Error::OperationSignatureMismatch { item });
        };
        if self.operations.contains_key(&fingerprint) {
            return Err(Error::DuplicateOperationBinding { item });
        }
        self.operations.insert(
            fingerprint,
            operation::Binding::new(definition, factory, normalize),
        );
        Ok(self)
    }

    /// Bind one exact symbolic stream operation to canonical argument identity
    /// and an executor-neutral native stream factory.
    ///
    /// `encode` must be deterministic and side-effect-free. It runs once per
    /// validated argument before the factory can start.
    /// `normalize` turns each native error item into the declared output
    /// representation; that value remains untrusted until bound issuance.
    pub fn bind_stream_operation<F, I, N, E, S>(
        &mut self,
        selected: &host::item::Definition,
        encode: I,
        factory: F,
        normalize: N,
    ) -> Result<&mut Self, Error>
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
        S: stream::Native<E>,
        E: 'static,
    {
        let item: ImmutableString = selected.path().into();
        if selected
            .path_ref()
            .segments()
            .first()
            .is_none_or(|root| root.as_str() != self.definition.root())
        {
            return Err(Error::StreamFacadeMismatch {
                facade: self.definition.root().into(),
                item,
            });
        }
        let Some(overloads) = self.definition.overloads(&item) else {
            return Err(Error::UnknownStream {
                facade: self.definition.root().into(),
                item,
            });
        };
        if selected.kind() != host::item::Kind::StreamOperation {
            return Err(Error::InvalidStreamKind {
                item,
                actual: selected.kind(),
            });
        }
        let fingerprint = selected.fingerprint();
        let Some(definition) = overloads
            .iter()
            .find(|definition| definition.fingerprint() == fingerprint)
            .cloned()
        else {
            return Err(Error::StreamSignatureMismatch { item });
        };
        if self.streams.contains_key(&fingerprint) {
            return Err(Error::DuplicateStreamBinding { item });
        }
        self.streams.insert(
            fingerprint,
            stream::Binding::new(definition, encode, factory, normalize),
        );
        Ok(self)
    }

    /// Bind one exact source-owned direct projection to a native conversion.
    pub fn bind_projection(
        &mut self,
        source: impl Into<ImmutableString>,
        target: impl Into<ImmutableString>,
        callback: impl Fn(&mut [&mut Dynamic]) -> Result<Dynamic, Box<EvalAltResult>>
            + SendSync
            + 'static,
    ) -> Result<&mut Self, Error> {
        let source = source.into();
        let target = target.into();
        let Some(definition) = self.definition.types().find(|ty| ty.path() == source) else {
            return Err(Error::UnknownProjection {
                facade: self.definition.root().into(),
                source,
                target,
            });
        };
        let Some(projection) = definition
            .projections()
            .iter()
            .find(|projection| projection.target() == target)
            .cloned()
        else {
            return Err(Error::UnknownProjection {
                facade: self.definition.root().into(),
                source,
                target,
            });
        };
        let owner = host::facade::Dependency::new(
            self.definition.root_path(),
            self.definition.fingerprint().clone(),
        );
        let key = host::projection::Key::new(owner, definition.contract(), projection);
        if self.projections.contains_key(&key) {
            return Err(Error::DuplicateProjectionBinding {
                projection: key.diagnostic_name(),
            });
        }
        self.projections.insert(key, Shared::new(Plain(callback)));
        Ok(self)
    }

    /// Bind one projection with sealed invocation-local witness authority.
    pub fn bind_projection_with_context(
        &mut self,
        source: impl Into<ImmutableString>,
        target: impl Into<ImmutableString>,
        callback: impl Fn(&Call<'_>, &mut [&mut Dynamic]) -> Result<Dynamic, Box<EvalAltResult>>
            + SendSync
            + 'static,
    ) -> Result<&mut Self, Error> {
        let source = source.into();
        let target = target.into();
        let Some(definition) = self.definition.types().find(|ty| ty.path() == source) else {
            return Err(Error::UnknownProjection {
                facade: self.definition.root().into(),
                source,
                target,
            });
        };
        let Some(projection) = definition
            .projections()
            .iter()
            .find(|projection| projection.target() == target)
            .cloned()
        else {
            return Err(Error::UnknownProjection {
                facade: self.definition.root().into(),
                source,
                target,
            });
        };
        let owner = host::facade::Dependency::new(
            self.definition.root_path(),
            self.definition.fingerprint().clone(),
        );
        let key = host::projection::Key::new(owner, definition.contract(), projection);
        if self.projections.contains_key(&key) {
            return Err(Error::DuplicateProjectionBinding {
                projection: key.diagnostic_name(),
            });
        }
        self.projections
            .insert(key, Shared::new(Contextual(callback)));
        Ok(self)
    }

    /// Bind one facade-owned generic type to an exact value attestor.
    pub fn bind_type_attestation(
        &mut self,
        path: impl Into<ImmutableString>,
        attestor: impl Fn(&Dynamic, &Instantiation<'_>) -> Option<Attested> + SendSync + 'static,
    ) -> Result<&mut Self, Error> {
        let path = path.into();
        let is_generic = self.definition.types().any(|ty| {
            ty.path() == path && ty.native_identity().is_some() && !ty.parameters().is_empty()
        });
        if !is_generic {
            return Err(Error::UnknownGenericType {
                facade: self.definition.root().into(),
                ty: path,
            });
        }
        if self.attestations.contains_key(&path) {
            return Err(Error::DuplicateTypeAttestation { ty: path });
        }
        self.attestations.insert(path, Shared::new(attestor));
        Ok(self)
    }

    /// Bind one exact imported type identity to a native value attestor.
    ///
    /// The compiler-derived identity selects the imported definition; its
    /// facade path is only an internal registry key after that proof succeeds.
    #[cfg(feature = "rust-import")]
    #[doc(hidden)]
    pub fn bind_imported_type_attestation(
        &mut self,
        selector: [u8; 32],
        attestor: impl Fn(&Dynamic, &Instantiation<'_>) -> Option<Attested> + SendSync + 'static,
    ) -> Result<ImportedTypeAttestation, Error> {
        let paths = self
            .definition
            .types()
            .filter(|ty| {
                ty.identity()
                    .imported()
                    .is_some_and(|identity| identity.as_bytes() == &selector)
            })
            .map(|ty| ImmutableString::from(ty.path()))
            .collect::<Vec<_>>();
        if paths.is_empty() {
            return Err(Error::UnknownImportedType {
                facade: self.definition.root().into(),
                selector,
            });
        }
        if let Some(path) = paths
            .iter()
            .find(|path| self.attestations.contains_key(path.as_str()))
        {
            return Err(Error::DuplicateTypeAttestation { ty: path.clone() });
        }
        let rows = paths.len();
        let attestor: Shared<dyn TypeAttestor> = Shared::new(attestor);
        for path in paths {
            self.attestations.insert(path, attestor.clone());
        }
        Ok(ImportedTypeAttestation { selector, rows })
    }

    /// Bind this facade's exact open widget carrier materialization.
    ///
    /// The selected carrier is the facade-owned one-parameter type with Rust
    /// identity `T`. The callback receives only the compiler-proven durable
    /// descriptor and a sealed witness for the exact target instantiation.
    pub fn bind_widget<T: 'static>(
        &mut self,
        callback: impl Fn(crate::widget::Descriptor, Witness) -> Result<Dynamic, Box<EvalAltResult>>
            + SendSync
            + 'static,
    ) -> Result<&mut Self, Error> {
        if self.widget.is_some() {
            return Err(Error::DuplicateWidgetBinding {
                facade: self.definition.root().into(),
            });
        }
        let binding = widget::Binding::bind::<T>(&self.definition, callback).map_err(|error| {
            let facade = self.definition.root().into();
            let ty = std::any::type_name::<T>().into();
            match error {
                widget::Error::Unknown => Error::UnknownWidgetType { facade, ty },
                widget::Error::Ambiguous => Error::AmbiguousWidgetType { facade, ty },
            }
        })?;
        self.widget = Some(binding);
        Ok(self)
    }

    /// Bind one exact qualified facade item to a native callback.
    ///
    /// Where a path names both a macro and a function, this binds the
    /// function; [`Self::bind_macro`] binds the macro.
    pub fn bind(
        &mut self,
        path: impl Into<ImmutableString>,
        callback: impl Fn(&mut [&mut Dynamic]) -> Result<Dynamic, Box<EvalAltResult>>
            + SendSync
            + 'static,
    ) -> Result<&mut Self, Error> {
        self.bind_callback(path.into(), Namespace::Value, Shared::new(Plain(callback)))
    }

    /// Bind one exact item with sealed invocation-local witness authority.
    ///
    /// Where a path names both a macro and a function, this binds the
    /// function; [`Self::bind_macro_with_context`] binds the macro.
    pub fn bind_with_context(
        &mut self,
        path: impl Into<ImmutableString>,
        callback: impl Fn(&Call<'_>, &mut [&mut Dynamic]) -> Result<Dynamic, Box<EvalAltResult>>
            + SendSync
            + 'static,
    ) -> Result<&mut Self, Error> {
        self.bind_callback(
            path.into(),
            Namespace::Value,
            Shared::new(Contextual(callback)),
        )
    }

    /// Bind the reader of a tuple variant's field that shares its path
    /// with the variant's constructor: `Key::Named` in a pattern.
    pub fn bind_variant_reader(
        &mut self,
        path: impl Into<ImmutableString>,
        callback: impl Fn(&mut [&mut Dynamic]) -> Result<Dynamic, Box<EvalAltResult>>
            + SendSync
            + 'static,
    ) -> Result<&mut Self, Error> {
        self.bind_callback(path.into(), Namespace::Reader, Shared::new(Plain(callback)))
    }

    /// Bind the macro a path names, beside any function of the same name.
    pub fn bind_macro(
        &mut self,
        path: impl Into<ImmutableString>,
        callback: impl Fn(&mut [&mut Dynamic]) -> Result<Dynamic, Box<EvalAltResult>>
            + SendSync
            + 'static,
    ) -> Result<&mut Self, Error> {
        self.bind_callback(path.into(), Namespace::Macro, Shared::new(Plain(callback)))
    }

    /// [`Self::bind_macro`] with invocation-local witness authority.
    pub fn bind_macro_with_context(
        &mut self,
        path: impl Into<ImmutableString>,
        callback: impl Fn(&Call<'_>, &mut [&mut Dynamic]) -> Result<Dynamic, Box<EvalAltResult>>
            + SendSync
            + 'static,
    ) -> Result<&mut Self, Error> {
        self.bind_callback(
            path.into(),
            Namespace::Macro,
            Shared::new(Contextual(callback)),
        )
    }

    fn bind_callback(
        &mut self,
        path: ImmutableString,
        namespace: Namespace,
        callback: Shared<dyn Callback>,
    ) -> Result<&mut Self, Error> {
        let items = self
            .definition
            .overloads(&path)
            .ok_or_else(|| Error::UnknownItem {
                facade: self.definition.root().into(),
                item: path.clone(),
            })?;
        // A path in one namespace binds whatever it names; a path in several
        // binds the namespace asked for.
        let constructor = items
            .iter()
            .any(|item| item.kind() == host::item::Kind::AssociatedFunction);
        let namespace_of = |item: &host::item::Definition| {
            if item.kind().is_macro() {
                Namespace::Macro
            } else if constructor && item.kind() == host::item::Kind::Method {
                Namespace::Reader
            } else {
                Namespace::Value
            }
        };
        let mixed = items
            .iter()
            .any(|item| namespace_of(item) != namespace_of(&items[0]));
        let fingerprints = items
            .iter()
            .filter(|item| !mixed || namespace_of(item) == namespace)
            .map(host::item::Definition::fingerprint)
            .collect::<Vec<_>>();
        if fingerprints.is_empty() {
            return Err(Error::UnknownItem {
                facade: self.definition.root().into(),
                item: path,
            });
        }
        if fingerprints
            .iter()
            .any(|fingerprint| self.bindings.contains_key(fingerprint))
        {
            return Err(Error::DuplicateBinding { item: path });
        }
        for fingerprint in fingerprints {
            self.bindings.insert(fingerprint, callback.clone());
        }
        Ok(self)
    }

    /// Bind one generated adapter to one instance of a generic imported
    /// item: the item's selector and its instance's types.
    #[cfg(feature = "rust-import")]
    #[doc(hidden)]
    pub fn bind_imported_instance(
        &mut self,
        selector: [u8; 32],
        instance: [u8; 32],
        callback: impl Fn(&Call<'_>, &mut [&mut Dynamic]) -> Result<Dynamic, Box<EvalAltResult>>
            + SendSync
            + 'static,
    ) -> Result<&mut Self, Error> {
        let fingerprints = self
            .definition
            .items()
            .filter(|definition| {
                definition
                    .imported_identity()
                    .is_some_and(|identity| identity.fingerprint() == selector)
            })
            .map(host::item::Definition::fingerprint)
            .collect::<Vec<_>>();
        if fingerprints.is_empty() {
            return Err(Error::UnknownItem {
                facade: self.definition.root().into(),
                item: format!("<imported selector {selector:02x?}>").into(),
            });
        }
        // An instance reached through two paths is bound once.
        let callback: Shared<dyn Callback> = Shared::new(Contextual(callback));
        for fingerprint in fingerprints {
            self.instances
                .entry((fingerprint, instance))
                .or_insert_with(|| callback.clone());
        }
        Ok(self)
    }

    /// Bind one exact generated imported selector without widening to sibling overloads.
    #[cfg(feature = "rust-import")]
    #[doc(hidden)]
    pub fn bind_imported(
        &mut self,
        selector: [u8; 32],
        callback: impl Fn(&Call<'_>, &mut [&mut Dynamic]) -> Result<Dynamic, Box<EvalAltResult>>
            + SendSync
            + 'static,
    ) -> Result<&mut Self, Error> {
        // The same Rust item under several paths (`TimeDelta::num_minutes`
        // and its alias `Duration::num_minutes`) is one binding.
        let definitions = self
            .definition
            .items()
            .filter(|definition| {
                definition
                    .imported_identity()
                    .is_some_and(|identity| identity.fingerprint() == selector)
            })
            .collect::<Vec<_>>();
        let Some(first) = definitions.first() else {
            return Err(Error::UnknownItem {
                facade: self.definition.root().into(),
                item: format!("<imported selector {selector:02x?}>").into(),
            });
        };
        if definitions
            .iter()
            .all(|definition| self.bindings.contains_key(&definition.fingerprint()))
        {
            return Err(Error::DuplicateBinding {
                item: first.path().to_string().into(),
            });
        }
        let callback: Shared<dyn Callback> = Shared::new(Contextual(callback));
        let fingerprints = definitions
            .iter()
            .map(|definition| definition.fingerprint())
            .collect::<Vec<_>>();
        for fingerprint in fingerprints {
            self.bindings
                .entry(fingerprint)
                .or_insert_with(|| callback.clone());
        }
        Ok(self)
    }

    /// Bind one manifest-authenticated generated structural adapter privately.
    #[cfg(feature = "rust-import")]
    #[doc(hidden)]
    pub fn bind_imported_structural(
        &mut self,
        adapter: &StructuralAdapter,
        callback: impl Fn(&Call<'_>, &mut [&mut Dynamic]) -> Result<Dynamic, Box<EvalAltResult>>
            + SendSync
            + 'static,
    ) -> Result<&mut Self, Error> {
        let definition = &adapter.definition;
        if definition.owner().root() != self.definition.root()
            || definition.owner().fingerprint() != self.definition.fingerprint()
        {
            return Err(Error::StructuralFacadeMismatch {
                operation: definition.path().into(),
            });
        }
        if self.structural.contains_key(definition) {
            return Err(Error::DuplicateStructuralBinding {
                operation: definition.path().into(),
            });
        }
        self.structural
            .insert(definition.clone(), Shared::new(Contextual(callback)));
        Ok(self)
    }

    /// Finish the runtime facade proof bundle.
    #[must_use]
    pub fn finish(self) -> Facade {
        let dependency = host::facade::Dependency::new(
            self.definition.root_path(),
            self.definition.fingerprint().clone(),
        );
        let dependencies = self.definition.dependencies().into_iter().collect();
        Facade {
            dependency,
            dependencies,
            bindings: self.bindings,
            #[cfg(feature = "rust-import")]
            instances: self.instances,
            structural: self.structural,
            #[cfg(feature = "rust-import")]
            events: self.events,
            operations: self.operations,
            streams: self.streams,
            projections: self.projections,
            attestations: self.attestations,
            widget: self.widget,
        }
    }
}

impl Registry {
    /// Create an empty launch-local runtime registry.
    #[must_use]
    pub fn new() -> Self {
        Self {
            facades: Shared::new(BTreeMap::new()),
            witness: witness::Authority::issue(),
            #[cfg(feature = "rust-import")]
            imported_receipt: None,
        }
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn retain_imported(
        mut self,
        receipt: crate::package::import::runner::account::Receipt,
    ) -> Result<Self, Error> {
        if self.imported_receipt.is_some() {
            return Err(Error::profile(
                "runtime already retains an imported allocation",
            ));
        }
        self.imported_receipt = Some(Shared::new(receipt));
        Ok(self)
    }

    /// A registry for this runner's next script generation.
    ///
    /// It shares these bindings, their imported allocation and this runner's
    /// witness authority, so nothing is bound again. Its witnesses carry a new
    /// generation: a value that holds script code from an earlier generation
    /// fails its exact attestation, while a native Rust value still attests.
    #[must_use]
    pub fn next_generation(&self) -> Self {
        Self {
            facades: self.facades.clone(),
            witness: self.witness.next_generation(),
            #[cfg(feature = "rust-import")]
            imported_receipt: self.imported_receipt.clone(),
        }
    }

    /// Whether a facade with this root is installed.
    pub(crate) fn contains(&self, root: &str) -> bool {
        self.facades.contains_key(root)
    }

    /// Install one finished runtime facade without replacing an existing root.
    pub fn install(&mut self, facade: Facade) -> Result<&mut Self, Error> {
        let root: ImmutableString = facade.dependency.root().into();
        let facades = Shared::get_mut(&mut self.facades)
            .ok_or_else(|| Error::profile("runtime registry is already shared by a generation"))?;
        if facades.contains_key(&root) {
            return Err(Error::DuplicateFacade { facade: root });
        }
        for dependency in &facade.dependencies {
            let Some(installed) = facades.get(dependency.root()) else {
                return Err(Error::MissingFacade {
                    facade: dependency.root().into(),
                });
            };
            if &installed.dependency != dependency {
                return Err(Error::FacadeMismatch {
                    facade: dependency.root().into(),
                });
            }
        }
        facades.insert(root, facade);
        Ok(self)
    }

    pub(crate) fn validate(
        &self,
        dependencies: &[host::facade::Dependency],
        requirements: &[Requirement],
        projections: &[host::projection::Site],
        widgets: &[widget::Requirement],
        boundary_types: &[type_ref::Type],
    ) -> Result<(), Error> {
        for dependency in dependencies {
            let Some(facade) = self.facades.get(dependency.root()) else {
                return Err(Error::MissingFacade {
                    facade: dependency.root().into(),
                });
            };
            if &facade.dependency != dependency {
                return Err(Error::FacadeMismatch {
                    facade: dependency.root().into(),
                });
            }
        }
        for requirement in requirements {
            if self.binding(requirement).is_none() {
                return Err(match requirement.binding {
                    Binding::Item(_) => Error::MissingBinding {
                        item: requirement.path.clone(),
                    },
                    #[cfg(feature = "rust-import")]
                    Binding::Event(_) => Error::Profile {
                        reason: format!(
                            "missing private event conversion for `{}`",
                            requirement.path
                        )
                        .into(),
                    },
                    Binding::Structural(_) => Error::MissingStructuralBinding {
                        operation: requirement.path.clone(),
                    },
                });
            }
            for ty in &requirement.operands {
                self.validate_type_attestations(ty)?;
            }
            self.validate_type_attestations(&requirement.output)?;
        }
        for projection in projections {
            if self.projection_binding(projection.key()).is_none() {
                return Err(Error::MissingProjectionBinding {
                    projection: projection.key().diagnostic_name(),
                });
            }
            self.validate_type_attestations(projection.source())?;
            self.validate_type_attestations(projection.target())?;
        }
        for requirement in widgets {
            if self.widget_binding(requirement).is_none() {
                return Err(Error::MissingWidgetBinding {
                    ty: requirement.target().to_string().into(),
                });
            }
            self.validate_type_attestations(requirement.target())?;
        }
        for ty in boundary_types {
            self.validate_type_attestations(ty)?;
        }
        Ok(())
    }

    pub(crate) fn invoke(
        &self,
        requirement: &Requirement,
        args: &mut [&mut Dynamic],
        invocation: Option<&invocation::Proof>,
        equal: Option<&Equal<'_>>,
    ) -> Result<Dynamic, Box<EvalAltResult>> {
        let binding = match requirement.bound.get(self.witness) {
            Some(binding) => binding,
            None => {
                let Some(binding) = self.binding(requirement) else {
                    return Err(EvalAltResult::ErrorRuntime(
                        format!("missing runtime binding for `{}`", requirement.path).into(),
                        crate::Position::NONE,
                    )
                    .into());
                };
                requirement.bound.remember(self.witness, binding);
                binding
            }
        };
        let call = match requirement.binding {
            Binding::Item(_) => Call::item(
                self.witness,
                &requirement.operands,
                &requirement.output,
                requirement.receiver,
                invocation,
            )
            .with_equal(equal),
            #[cfg(feature = "rust-import")]
            Binding::Event(ref key) => {
                let [source] = requirement.operands.as_ref() else {
                    return Err(EvalAltResult::ErrorRuntime(
                        "private event operand proof".into(),
                        crate::Position::NONE,
                    )
                    .into());
                };
                Call::event(self.witness, source, &requirement.output, key.mapper())
            }
            Binding::Structural(_) => Call::structural(
                self.witness,
                &requirement.operands,
                &requirement.output,
                requirement.receiver,
            ),
        };
        binding.invoke(&call, args)
    }

    /// Join one compiler-selected symbolic operation to its exact native binding.
    ///
    /// This transformation neither invokes the factory nor polls its future.
    pub fn select_operation(
        &self,
        proof: &host::operation::Proof,
    ) -> Result<operation::Selected, Error> {
        let Some(facade) = self.facades.get(proof.owner().root()) else {
            return Err(Error::MissingFacade {
                facade: proof.owner().root().into(),
            });
        };
        if &facade.dependency != proof.owner() {
            return Err(Error::FacadeMismatch {
                facade: proof.owner().root().into(),
            });
        }
        let Some(binding) = facade.operations.get(proof.fingerprint()) else {
            return Err(Error::MissingOperationBinding {
                item: proof.path().into(),
            });
        };
        binding
            .select(&facade.dependency, proof)
            .ok_or_else(|| Error::OperationProofMismatch {
                item: proof.path().into(),
            })
    }

    /// Join one compiler-selected symbolic stream to its exact native binding.
    ///
    /// Selection neither invokes the identity encoder nor starts the factory.
    pub fn select_stream(&self, proof: &host::stream::Proof) -> Result<stream::Selected, Error> {
        let Some(facade) = self.facades.get(proof.owner().root()) else {
            return Err(Error::MissingFacade {
                facade: proof.owner().root().into(),
            });
        };
        if &facade.dependency != proof.owner() {
            return Err(Error::FacadeMismatch {
                facade: proof.owner().root().into(),
            });
        }
        let Some(binding) = facade.streams.get(proof.fingerprint()) else {
            return Err(Error::MissingStreamBinding {
                item: proof.path().into(),
            });
        };
        binding
            .select(&facade.dependency, proof)
            .ok_or_else(|| Error::StreamProofMismatch {
                item: proof.path().into(),
            })
    }

    pub(crate) fn witness_for(&self, ty: &type_ref::Type) -> Witness {
        Witness::issue(self.witness, ty.clone())
    }

    pub(crate) fn invoke_projection(
        &self,
        proof: &host::projection::Site,
        args: &mut [&mut Dynamic],
        model: &crate::program::model::Model,
    ) -> Result<Dynamic, Box<EvalAltResult>> {
        if args.len() != 1 {
            return Err(EvalAltResult::ErrorRuntime(
                format!(
                    "runtime projection `{}` received {} operand(s), expected 1",
                    proof.key().diagnostic_name(),
                    args.len()
                )
                .into(),
                proof.span().start(),
            )
            .into());
        }
        proof
            .source()
            .validate(args[0], model, self)
            .map_err(|error| {
                Box::new(EvalAltResult::ErrorRuntime(
                    format!(
                        "runtime projection `{}` received invalid `{}`: {error}",
                        proof.key().diagnostic_name(),
                        proof.source()
                    )
                    .into(),
                    proof.span().start(),
                ))
            })?;
        let binding = match proof.bound().get(self.witness) {
            Some(binding) => binding,
            None => {
                let Some(binding) = self.projection_binding(proof.key()) else {
                    return Err(EvalAltResult::ErrorRuntime(
                        format!(
                            "missing runtime projection binding for `{}`",
                            proof.key().diagnostic_name()
                        )
                        .into(),
                        proof.span().start(),
                    )
                    .into());
                };
                proof.bound().remember(self.witness, binding);
                binding
            }
        };
        let call = Call::projection(self.witness, proof.source(), proof.target());
        let result = binding.invoke(&call, args)?;
        proof
            .target()
            .validate(&result, model, self)
            .map_err(|error| {
                Box::new(EvalAltResult::ErrorRuntime(
                    format!(
                        "runtime projection `{}` returned `{}`, expected `{}`: {error}",
                        proof.key().diagnostic_name(),
                        result.type_name(),
                        proof.target()
                    )
                    .into(),
                    proof.span().start(),
                ))
            })?;
        Ok(result)
    }

    pub(crate) fn activate_widget(
        &self,
        requirement: &widget::Requirement,
        descriptor: crate::widget::Descriptor,
        model: &crate::program::model::Model,
    ) -> Result<Dynamic, Box<EvalAltResult>> {
        let position = descriptor.promotion().location().span().start();
        let Some(binding) = self.widget_binding(requirement) else {
            return Err(EvalAltResult::ErrorRuntime(
                format!(
                    "missing scripted-widget activation for `{}`",
                    requirement.target()
                )
                .into(),
                position,
            )
            .into());
        };
        let witness = Witness::issue(self.witness, requirement.target().clone());
        let result = binding.activate(descriptor, witness)?;
        requirement
            .target()
            .validate(&result, model, self)
            .map_err(|error| {
                Box::new(EvalAltResult::ErrorRuntime(
                    format!(
                        "scripted-widget activation returned `{}`, expected `{}`: {error}",
                        result.type_name(),
                        requirement.target()
                    )
                    .into(),
                    position,
                ))
            })?;
        Ok(result)
    }

    pub(crate) fn attest(
        &self,
        owner: &host::facade::Dependency,
        path: &str,
        value: &Dynamic,
        ty: &type_ref::Type,
        model: &crate::program::model::Model,
    ) -> Result<(), type_ref::Error> {
        let attestor = self
            .facades
            .get(owner.root())
            .filter(|facade| &facade.dependency == owner)
            .and_then(|facade| facade.attestations.get(path))
            .ok_or_else(|| type_ref::Error::MissingAttestation { ty: path.into() })?;
        let expected = Instantiation {
            ty,
            model,
            registry: self,
        };
        if let Some(Attested { private }) = attestor.attest(value, &expected) {
            let () = private;
            Ok(())
        } else {
            Err(type_ref::Error::AttestationMismatch { ty: path.into() })
        }
    }

    fn binding(&self, requirement: &Requirement) -> Option<&Shared<dyn Callback>> {
        let facade = self.facades.get(requirement.owner.root())?;
        if facade.dependency != requirement.owner {
            return None;
        }
        match &requirement.binding {
            #[cfg(feature = "rust-import")]
            Binding::Item(item) => requirement
                .instance
                .and_then(|instance| facade.instances.get(&(item.clone(), instance)))
                .or_else(|| facade.bindings.get(item)),
            #[cfg(not(feature = "rust-import"))]
            Binding::Item(item) => facade.bindings.get(item),
            Binding::Structural(definition) => facade.structural.get(definition),
            #[cfg(feature = "rust-import")]
            Binding::Event(key) => facade.events.get(&**key),
        }
    }

    fn projection_binding(&self, key: &host::projection::Key) -> Option<&Shared<dyn Callback>> {
        let facade = self.facades.get(key.owner().root())?;
        if &facade.dependency != key.owner() {
            return None;
        }
        facade.projections.get(key)
    }

    fn widget_binding(&self, requirement: &widget::Requirement) -> Option<&widget::Binding> {
        let facade = self.facades.get(requirement.owner().root())?;
        if &facade.dependency != requirement.owner() {
            return None;
        }
        facade
            .widget
            .as_ref()
            .filter(|binding| binding.accepts(requirement))
    }

    fn validate_type_attestations(&self, ty: &type_ref::Type) -> Result<(), Error> {
        if let Some((path, arguments, owner, required)) = ty.host_attestation() {
            if required {
                let available = owner.is_some_and(|owner| {
                    self.facades
                        .get(owner.root())
                        .filter(|facade| &facade.dependency == owner)
                        .is_some_and(|facade| facade.attestations.contains_key(path))
                });
                if !available {
                    return Err(Error::MissingTypeAttestation {
                        ty: ty.to_string().into(),
                    });
                }
            }
            for argument in arguments.iter() {
                self.validate_type_attestations(argument)?;
            }
        }
        Ok(())
    }
}

impl Requirement {
    pub(crate) fn new(
        owner: host::facade::Dependency,
        item: host::item::Fingerprint,
        path: impl Into<ImmutableString>,
        operands: Box<[type_ref::Type]>,
        output: type_ref::Type,
        receiver: Option<crate::receiver::Mode>,
    ) -> Self {
        Self {
            owner,
            binding: Binding::Item(item),
            path: path.into(),
            operands,
            output,
            receiver,
            #[cfg(feature = "rust-import")]
            instance: None,
            bound: Bound::default(),
        }
    }

    /// The same requirement of one instance of a generic imported item.
    #[cfg(feature = "rust-import")]
    pub(crate) const fn with_instance(mut self, instance: Option<[u8; 32]>) -> Self {
        self.instance = instance;
        self
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn structural(proof: &host::structural::Proof) -> Self {
        Self {
            owner: proof.definition().owner().clone(),
            binding: Binding::Structural(proof.definition().clone()),
            path: proof.definition().path().into(),
            operands: proof
                .operands()
                .iter()
                .map(|operand| operand.ty().clone())
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            output: proof.output().clone(),
            receiver: proof.receiver(),
            #[cfg(feature = "rust-import")]
            instance: None,
            bound: Bound::default(),
        }
    }
}

impl StructuralAdapter {
    #[cfg(feature = "rust-import")]
    pub(crate) fn transform(definition: host::structural::Definition) -> Self {
        Self { definition }
    }

    #[doc(hidden)]
    #[must_use]
    pub fn identity(&self) -> &[u8; 32] {
        self.definition.identity()
    }

    /// Return the exact facade root owning this private adapter.
    #[doc(hidden)]
    #[must_use]
    pub fn root(&self) -> &str {
        self.definition.owner().root()
    }
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

impl Instantiation<'_> {
    /// Mint evidence when a native value satisfies this exact instantiation.
    #[must_use]
    pub const fn attest(&self, accepted: bool) -> Option<Attested> {
        if accepted {
            Some(Attested { private: () })
        } else {
            None
        }
    }

    /// Mint evidence only when a carried witness matches this exact launch and type.
    #[must_use]
    pub fn attest_witness(&self, witness: Option<&Witness>) -> Option<Attested> {
        self.attest(witness.is_some_and(|witness| witness.matches(self.registry.witness, self.ty)))
    }

    /// Mint evidence when a native Rust value's witness comes from this runner
    /// and names this exact type, in any of the runner's script generations.
    ///
    /// Only an imported Rust value may use this. It holds no script code, and
    /// its Rust code cannot change while one runner lives.
    #[doc(hidden)]
    #[must_use]
    pub fn attest_native_witness(&self, witness: Option<&Witness>) -> Option<Attested> {
        self.attest(
            witness.is_some_and(|witness| witness.matches_native(self.registry.witness, self.ty)),
        )
    }

    /// Check that another attested carrier has these exact logical arguments.
    ///
    /// The attestor still checks the carrier's concrete Rust representation;
    /// this comparison proves the complete canonical argument list and current
    /// launch authority without relying on names or arity alone.
    #[must_use]
    pub fn arguments_match_witness(&self, witness: Option<&Witness>) -> bool {
        witness.is_some_and(|witness| witness.arguments_match(self.registry.witness, self.ty))
    }

    /// Return the number of exact logical type arguments.
    #[must_use]
    pub fn argument_count(&self) -> usize {
        self.ty.arguments().len()
    }

    /// Validate a value against one exact logical type argument.
    #[must_use]
    pub fn argument_matches(&self, index: usize, value: &Dynamic) -> bool {
        self.ty
            .arguments()
            .get(index)
            .is_some_and(|argument| argument.validate(value, self.model, self.registry).is_ok())
    }

    /// Format one exact logical type argument for diagnostics.
    #[must_use]
    pub fn argument_name(&self, index: usize) -> Option<String> {
        self.ty.arguments().get(index).map(ToString::to_string)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            #[cfg(feature = "rust-import")]
            Self::ImportedAllocation => formatter
                .write_str("generated runtime metadata exceeds the shared allocation limit"),
            Self::Profile { reason } => {
                write!(formatter, "linked profile runtime binding failed: {reason}")
            }
            Self::UnknownItem { facade, item } => {
                write!(formatter, "runtime facade `{facade}` has no item `{item}`")
            }
            Self::UnknownGenericType { facade, ty } => {
                write!(
                    formatter,
                    "runtime facade `{facade}` has no generic type `{ty}`"
                )
            }
            #[cfg(feature = "rust-import")]
            Self::UnknownImportedType { facade, selector } => write!(
                formatter,
                "runtime facade `{facade}` has no imported type for selector {selector:02x?}"
            ),
            Self::UnknownProjection {
                facade,
                source,
                target,
            } => write!(
                formatter,
                "runtime facade `{facade}` has no direct projection `{source} -> {target}`"
            ),
            Self::DuplicateBinding { item } => {
                write!(formatter, "duplicate runtime binding for `{item}`")
            }
            Self::StructuralFacadeMismatch { operation } => write!(
                formatter,
                "generated structural operation `{operation}` belongs to another facade"
            ),
            Self::DuplicateStructuralBinding { operation } => write!(
                formatter,
                "duplicate generated structural binding for `{operation}`"
            ),
            Self::DuplicateOperationBinding { item } => {
                write!(
                    formatter,
                    "duplicate runtime operation binding for `{item}`"
                )
            }
            Self::DuplicateStreamBinding { item } => {
                write!(formatter, "duplicate runtime stream binding for `{item}`")
            }
            Self::UnknownOperation { facade, item } => {
                write!(
                    formatter,
                    "runtime facade `{facade}` has no operation `{item}`"
                )
            }
            Self::OperationFacadeMismatch { facade, item } => write!(
                formatter,
                "operation `{item}` does not belong to runtime facade `{facade}`"
            ),
            Self::InvalidOperationKind { item, actual } => write!(
                formatter,
                "runtime operation `{item}` has facade item kind {actual:?}"
            ),
            Self::OperationSignatureMismatch { item } => write!(
                formatter,
                "runtime operation `{item}` does not match an exact declared signature"
            ),
            Self::UnknownStream { facade, item } => {
                write!(
                    formatter,
                    "runtime facade `{facade}` has no stream `{item}`"
                )
            }
            Self::StreamFacadeMismatch { facade, item } => write!(
                formatter,
                "stream operation `{item}` does not belong to runtime facade `{facade}`"
            ),
            Self::InvalidStreamKind { item, actual } => write!(
                formatter,
                "runtime stream `{item}` has facade item kind {actual:?}"
            ),
            Self::StreamSignatureMismatch { item } => write!(
                formatter,
                "runtime stream `{item}` does not match an exact declared signature"
            ),
            Self::DuplicateTypeAttestation { ty } => {
                write!(formatter, "duplicate runtime type attestation for `{ty}`")
            }
            Self::DuplicateProjectionBinding { projection } => {
                write!(
                    formatter,
                    "duplicate runtime projection binding for `{projection}`"
                )
            }
            Self::DuplicateFacade { facade } => {
                write!(formatter, "duplicate runtime facade `{facade}`")
            }
            Self::MissingFacade { facade } => {
                write!(formatter, "missing runtime facade `{facade}`")
            }
            Self::FacadeMismatch { facade } => {
                write!(
                    formatter,
                    "runtime facade `{facade}` does not match its compiled ABI"
                )
            }
            Self::MissingBinding { item } => {
                write!(formatter, "missing runtime binding for `{item}`")
            }
            Self::MissingStructuralBinding { operation } => write!(
                formatter,
                "missing generated structural binding for `{operation}`"
            ),
            Self::MissingOperationBinding { item } => {
                write!(formatter, "missing runtime operation binding for `{item}`")
            }
            Self::OperationProofMismatch { item } => write!(
                formatter,
                "runtime operation binding for `{item}` does not match its compiler proof"
            ),
            Self::MissingStreamBinding { item } => {
                write!(formatter, "missing runtime stream binding for `{item}`")
            }
            Self::StreamProofMismatch { item } => write!(
                formatter,
                "runtime stream binding for `{item}` does not match its compiler proof"
            ),
            Self::MissingProjectionBinding { projection } => {
                write!(
                    formatter,
                    "missing runtime projection binding for `{projection}`"
                )
            }
            Self::MissingTypeAttestation { ty } => {
                write!(
                    formatter,
                    "missing exact runtime type attestation for `{ty}`"
                )
            }
            Self::UnknownWidgetType { facade, ty } => {
                write!(
                    formatter,
                    "runtime facade `{facade}` has no open widget carrier represented by `{ty}`"
                )
            }
            Self::AmbiguousWidgetType { facade, ty } => {
                write!(
                    formatter,
                    "runtime facade `{facade}` has multiple open widget carriers represented by `{ty}`"
                )
            }
            Self::DuplicateWidgetBinding { facade } => {
                write!(formatter, "duplicate private widget binding for `{facade}`")
            }
            Self::MissingWidgetBinding { ty } => {
                write!(formatter, "missing private widget binding for `{ty}`")
            }
        }
    }
}

#[cfg(test)]
mod witness_tests {
    use super::{invocation, type_ref, witness, Call, WitnessError};
    use crate::{host, receiver};
    use std::any::TypeId;

    #[derive(Clone)]
    struct Carrier;

    #[derive(Clone)]
    struct Scoped;

    #[test]
    fn witnesses_distinguish_exact_logical_instantiations_with_one_outer_type() {
        let authority = witness::Authority::issue();
        let first = carrier(type_ref::Type::builtin("i64", TypeId::of::<crate::INT>()));
        let second = carrier(type_ref::Type::builtin("bool", TypeId::of::<bool>()));

        let witness = Call::item(authority, &[], &first, None, None)
            .output_witness()
            .unwrap();

        assert!(witness.matches(authority, &first));
        assert!(!witness.matches(authority, &second));
    }

    #[test]
    fn projections_derive_only_from_the_authenticated_exact_source() {
        let authority = witness::Authority::issue();
        let source = carrier(type_ref::Type::builtin("i64", TypeId::of::<crate::INT>()));
        let wrong_source = carrier(type_ref::Type::builtin("bool", TypeId::of::<bool>()));
        let target = carrier(type_ref::Type::builtin(
            "String",
            TypeId::of::<crate::ImmutableString>(),
        ));
        let source_witness = Call::item(authority, &[], &source, None, None)
            .output_witness()
            .unwrap();
        let wrong_witness = Call::item(authority, &[], &wrong_source, None, None)
            .output_witness()
            .unwrap();
        let projection = Call::projection(authority, &source, &target);

        assert!(projection
            .project_witness(&source_witness)
            .unwrap()
            .matches(authority, &target));
        assert_eq!(
            projection.project_witness(&wrong_witness).unwrap_err(),
            WitnessError::ProjectionSourceMismatch
        );
    }

    #[test]
    fn owned_builders_cannot_mint_a_replacement_output_witness() {
        let authority = witness::Authority::issue();
        let ty = carrier(type_ref::Type::builtin("i64", TypeId::of::<crate::INT>()));

        assert_eq!(
            Call::item(
                authority,
                std::slice::from_ref(&ty),
                &ty,
                Some(receiver::Mode::Owned),
                None,
            )
            .output_witness()
            .unwrap_err(),
            WitnessError::OutputRequiresConstructor
        );
    }

    #[test]
    fn owned_item_transform_derives_only_its_exact_output_from_its_receiver() {
        let authority = witness::Authority::issue();
        let source = carrier(type_ref::Type::builtin("i64", TypeId::of::<crate::INT>()));
        let foreign = carrier(type_ref::Type::builtin("bool", TypeId::of::<bool>()));
        let target = carrier(type_ref::Type::builtin(
            "String",
            TypeId::of::<crate::ImmutableString>(),
        ));
        let source_witness = Call::item(authority, &[], &source, None, None)
            .output_witness()
            .unwrap();
        let foreign_witness = Call::item(authority, &[], &foreign, None, None)
            .output_witness()
            .unwrap();
        let operands = [source.clone()];
        let transform = Call::item(
            authority,
            &operands,
            &target,
            Some(receiver::Mode::Owned),
            None,
        );

        assert!(transform
            .transform_witness(&source_witness)
            .unwrap()
            .matches(authority, &target));
        assert_eq!(
            transform.transform_witness(&foreign_witness).unwrap_err(),
            WitnessError::TransformSourceMismatch
        );
        assert_eq!(
            Call::item(authority, &[source], &target, None, None)
                .transform_witness(&source_witness)
                .unwrap_err(),
            WitnessError::TransformRequiresOwnedItem
        );
    }

    #[test]
    fn invocation_value_requires_the_exact_mutable_facade_receiver() {
        let authority = witness::Authority::issue();
        let (export, receiver) = scoped(facade("scoped", 1));
        let invocation = invocation::Invocation::bind(export, 42_u64).unwrap();
        let proof = invocation.proof();
        let operands = [receiver.clone()];
        let output = type_ref::Type::unit();
        let call = Call::item(
            authority,
            &operands,
            &output,
            Some(receiver::Mode::Mutable),
            Some(&proof),
        );

        assert_eq!(*call.invocation::<u64>().unwrap(), 42);
        assert_eq!(
            call.invocation::<String>().unwrap_err(),
            WitnessError::InvocationPayloadMismatch
        );

        let (_, foreign_receiver) = scoped(facade("foreign", 1));
        let foreign_operands = [foreign_receiver];
        let foreign = Call::item(
            authority,
            &foreign_operands,
            &output,
            Some(receiver::Mode::Mutable),
            Some(&proof),
        );
        assert_eq!(
            foreign.invocation::<u64>().unwrap_err(),
            WitnessError::InvocationReceiverMismatch
        );

        let shared = Call::item(
            authority,
            &operands,
            &output,
            Some(receiver::Mode::Shared),
            Some(&proof),
        );
        assert_eq!(
            shared.invocation::<u64>().unwrap_err(),
            WitnessError::InvocationRequiresMutableReceiver
        );

        let owned = Call::item(
            authority,
            &operands,
            &output,
            Some(receiver::Mode::Owned),
            Some(&proof),
        );
        assert_eq!(
            owned.invocation::<u64>().unwrap_err(),
            WitnessError::InvocationRequiresMutableReceiver
        );

        let ordinary = Call::item(authority, &[], &output, None, Some(&proof));
        assert_eq!(
            ordinary.invocation::<u64>().unwrap_err(),
            WitnessError::InvocationRequiresMutableReceiver
        );

        let projection = Call::projection(authority, &receiver, &output);
        assert_eq!(
            projection.invocation::<u64>().unwrap_err(),
            WitnessError::InvocationRequiresMutableReceiver
        );

        let unavailable = Call::item(
            authority,
            &operands,
            &output,
            Some(receiver::Mode::Mutable),
            None,
        );
        assert_eq!(
            unavailable.invocation::<u64>().unwrap_err(),
            WitnessError::InvocationUnavailable
        );
    }

    #[test]
    fn invocation_binding_rejects_owned_facade_storage() {
        let mut facade =
            host::facade::Builder::new("owned", host::facade::Abi::new(1).unwrap()).unwrap();
        facade.type_::<Scoped>("Renderer", &[], &[]).unwrap();
        let export = facade.finish().unwrap().export_type("Renderer").unwrap();

        assert_eq!(
            invocation::Invocation::bind(export, ()).unwrap_err(),
            invocation::Error::OwnedStorage
        );
    }

    fn carrier(argument: type_ref::Type) -> type_ref::Type {
        type_ref::Type::host(
            "test::Carrier".into(),
            TypeId::of::<Carrier>(),
            vec![argument].into_boxed_slice(),
            None,
        )
    }

    fn facade(root: &str, abi: u32) -> host::facade::Definition {
        let mut facade =
            host::facade::Builder::new(root, host::facade::Abi::new(abi).unwrap()).unwrap();
        facade.invocation_type::<Scoped>("Renderer", &[]).unwrap();
        facade.finish().unwrap()
    }

    fn scoped(facade: host::facade::Definition) -> (host::type_def::Export, type_ref::Type) {
        let export = facade.export_type("Renderer").unwrap();
        let receiver = type_ref::Type::host(
            export.path().into(),
            TypeId::of::<Scoped>(),
            Box::new([]),
            Some(export.dependency()),
        );
        (export, receiver)
    }
}

impl std::error::Error for Error {}

#[cfg(test)]
mod tests {
    use super::{type_ref, widget, Builder, Registry, Witness};
    use crate::program::{model::Model, GenerationId};
    use crate::types::nominal;
    use crate::{host, resolve, type_def, Dynamic, Engine, Shared};
    use std::any::TypeId;

    #[derive(Clone)]
    struct ErasedGeneric {
        logical_argument: &'static str,
    }

    #[derive(Clone)]
    struct OpenWidget {
        witness: Option<Witness>,
    }

    #[derive(Clone)]
    struct OtherWidget;

    #[derive(Clone)]
    struct DependencyValue;

    #[derive(Clone)]
    struct DependentValue;

    #[cfg(feature = "rust-import")]
    #[derive(Clone)]
    struct ImportedState;

    #[cfg(feature = "rust-import")]
    #[derive(Clone)]
    struct ImportedScreen;

    #[cfg(feature = "rust-import")]
    #[derive(Clone)]
    struct ImportedGeneric;

    #[test]
    fn install_preflights_dependencies() {
        let alpha = dependency_facade("alpha", 1);
        let beta = dependency_facade("beta", 1);
        let dependent = dependent_facade(&alpha, &beta);
        let runtime = || Builder::new(dependent.clone()).finish();

        let mut empty = Registry::new();
        assert!(matches!(
            empty.install(runtime()),
            Err(super::Error::MissingFacade { facade }) if facade == "alpha"
        ));
        assert!(empty.facades.is_empty());

        let mut reverse = Registry::new();
        reverse
            .install(Builder::new(beta.clone()).finish())
            .unwrap();
        assert!(matches!(
            reverse.install(runtime()),
            Err(super::Error::MissingFacade { facade }) if facade == "alpha"
        ));
        assert_eq!(
            reverse
                .facades
                .keys()
                .map(|root| root.as_str())
                .collect::<Vec<_>>(),
            ["beta"]
        );

        reverse
            .install(Builder::new(alpha.clone()).finish())
            .unwrap();
        reverse.install(runtime()).unwrap();
        assert_eq!(
            reverse
                .facades
                .keys()
                .map(|root| root.as_str())
                .collect::<Vec<_>>(),
            ["alpha", "beta", "extension"]
        );

        assert!(matches!(
            reverse.install(Builder::new(dependent).finish()),
            Err(super::Error::DuplicateFacade { facade }) if facade == "extension"
        ));
        assert_eq!(reverse.facades.len(), 3);
    }

    #[test]
    fn install_rejects_stale() {
        let alpha = dependency_facade("alpha", 1);
        let beta = dependency_facade("beta", 1);
        let dependent = dependent_facade(&alpha, &beta);
        let mut registry = Registry::new();
        registry
            .install(Builder::new(dependency_facade("alpha", 2)).finish())
            .unwrap();
        registry.install(Builder::new(beta).finish()).unwrap();

        assert!(matches!(
            registry.install(Builder::new(dependent).finish()),
            Err(super::Error::FacadeMismatch { facade }) if facade == "alpha"
        ));
        assert_eq!(
            registry
                .facades
                .keys()
                .map(|root| root.as_str())
                .collect::<Vec<_>>(),
            ["alpha", "beta"]
        );
    }

    #[test]
    fn generic_host_values_require_exact_owner_bound_attestation() {
        let facade = generic_facade();
        let owner = host::facade::Dependency::new(facade.root_path(), facade.fingerprint().clone());
        let expected = type_ref::Type::host(
            "generic::Value".into(),
            TypeId::of::<ErasedGeneric>(),
            vec![type_ref::Type::builtin("i64", TypeId::of::<crate::INT>())].into_boxed_slice(),
            Some(owner),
        );
        let model = empty_model();

        let mut missing = Registry::new();
        missing
            .install(Builder::new(facade.clone()).finish())
            .unwrap();
        let value = Dynamic::from(ErasedGeneric {
            logical_argument: "i64",
        });
        assert!(matches!(
            expected.validate(&value, &model, &missing),
            Err(type_ref::Error::MissingAttestation { ref ty }) if ty == "generic::Value"
        ));

        let mut facade = Builder::new(facade);
        facade
            .bind_type_attestation("generic::Value", |value, expected| {
                expected.attest(value.read_lock::<ErasedGeneric>().is_some_and(|value| {
                    expected.argument_name(0).as_deref() == Some(value.logical_argument)
                }))
            })
            .unwrap();
        let mut registry = Registry::new();
        registry.install(facade.finish()).unwrap();

        let wrong = Dynamic::from(ErasedGeneric {
            logical_argument: "bool",
        });
        assert!(matches!(
            expected.validate(&wrong, &model, &registry),
            Err(type_ref::Error::AttestationMismatch { ref ty }) if ty == "generic::Value"
        ));
        expected.validate(&value, &model, &registry).unwrap();
    }

    #[cfg(feature = "rust-import")]
    #[test]
    fn imported_identity_attests_zero_and_parameterized_nominals() {
        const STATE: [u8; 32] = [0x41; 32];
        const SCREEN: [u8; 32] = [0x42; 32];
        const GENERIC: [u8; 32] = [0x43; 32];

        let facade = imported_attestation_facade(&[
            (
                "domain::State",
                STATE,
                ImportedShape::Struct,
                Box::default(),
            ),
            (
                "domain::Screen",
                SCREEN,
                ImportedShape::Enum,
                Box::default(),
            ),
            (
                "domain::Carrier",
                GENERIC,
                ImportedShape::Opaque,
                Box::from([crate::ImmutableString::from("T")]),
            ),
        ])
        .unwrap();
        let owner = host::facade::Dependency::new(facade.root_path(), facade.fingerprint().clone());
        let mut builder = Builder::new(facade);
        let _ = builder
            .bind_imported_type_attestation(STATE, |value, expected| {
                expected.attest(
                    value.read_lock::<ImportedState>().is_some() && expected.argument_count() == 0,
                )
            })
            .unwrap();
        let _ = builder
            .bind_imported_type_attestation(SCREEN, |value, expected| {
                expected.attest(
                    value.read_lock::<ImportedScreen>().is_some() && expected.argument_count() == 0,
                )
            })
            .unwrap();
        let _ = builder
            .bind_imported_type_attestation(GENERIC, |value, expected| {
                expected.attest(
                    value.read_lock::<ImportedGeneric>().is_some()
                        && expected.argument_count() == 1
                        && expected.argument_matches(0, &Dynamic::from(1_i64)),
                )
            })
            .unwrap();
        let mut registry = Registry::new();
        registry.install(builder.finish()).unwrap();
        let model = empty_model();

        let state = type_ref::Type::imported(
            "gallery_ui::domain::State".into(),
            host::type_def::import::Identity::new(STATE),
            Box::default(),
            owner.clone(),
        );
        let screen = type_ref::Type::imported(
            "gallery_ui::domain::Screen".into(),
            host::type_def::import::Identity::new(SCREEN),
            Box::default(),
            owner.clone(),
        );
        let generic = type_ref::Type::imported(
            "gallery_ui::domain::Carrier".into(),
            host::type_def::import::Identity::new(GENERIC),
            Box::from([type_ref::Type::builtin("i64", TypeId::of::<crate::INT>())]),
            owner,
        );

        state
            .validate(&Dynamic::from(ImportedState), &model, &registry)
            .unwrap();
        screen
            .validate(&Dynamic::from(ImportedScreen), &model, &registry)
            .unwrap();
        generic
            .validate(&Dynamic::from(ImportedGeneric), &model, &registry)
            .unwrap();
        assert!(matches!(
            state.validate(&Dynamic::from(ImportedScreen), &model, &registry),
            Err(type_ref::Error::AttestationMismatch { ref ty })
                if ty == "gallery_ui::domain::State"
        ));
    }

    #[cfg(feature = "rust-import")]
    #[test]
    fn imported_identity_rejects_unknown_duplicate_and_upstream_ambiguity() {
        const AMBIGUOUS: [u8; 32] = [0x51; 32];
        const EXACT: [u8; 32] = [0x52; 32];
        assert!(matches!(
            imported_attestation_facade(&[
                (
                    "domain::Left",
                    AMBIGUOUS,
                    ImportedShape::Struct,
                    Box::default(),
                ),
                (
                    "domain::Right",
                    AMBIGUOUS,
                    ImportedShape::Enum,
                    Box::default(),
                ),
            ]),
            Err(host::Error::ConflictingHostIdentity { .. })
        ));
        let facade = imported_attestation_facade(&[(
            "domain::Exact",
            EXACT,
            ImportedShape::Struct,
            Box::default(),
        )])
        .unwrap();
        let owner = host::facade::Dependency::new(facade.root_path(), facade.fingerprint().clone());
        let mut builder = Builder::new(facade);

        assert!(matches!(
            builder.bind_imported_type_attestation([0x99; 32], |_, expected| {
                expected.attest(true)
            }),
            Err(super::Error::UnknownImportedType { selector, .. })
                if selector == [0x99; 32]
        ));
        let _ = builder
            .bind_imported_type_attestation(EXACT, |_, expected| expected.attest(false))
            .unwrap();
        assert!(matches!(
            builder.bind_imported_type_attestation(EXACT, |_, expected| {
                expected.attest(true)
            }),
            Err(super::Error::DuplicateTypeAttestation { ref ty })
                if ty == "gallery_ui::domain::Exact"
        ));
        assert!(matches!(
            builder.bind_type_attestation("gallery_ui::domain::Exact", |_, expected| {
                expected.attest(true)
            }),
            Err(super::Error::UnknownGenericType { ref ty, .. })
                if ty == "gallery_ui::domain::Exact"
        ));

        let mut registry = Registry::new();
        registry.install(builder.finish()).unwrap();
        let exact = type_ref::Type::imported(
            "gallery_ui::domain::Exact".into(),
            host::type_def::import::Identity::new(EXACT),
            Box::default(),
            owner,
        );
        assert!(matches!(
            exact.validate(&Dynamic::from(ImportedState), &empty_model(), &registry),
            Err(type_ref::Error::AttestationMismatch { .. })
        ));
    }

    #[cfg(feature = "rust-import")]
    #[test]
    fn imported_identity_binds_complete_alias_cohorts_once() {
        const STATE: [u8; 32] = [0x61; 32];
        const SCREEN: [u8; 32] = [0x62; 32];
        let state_paths = [
            "domain::State",
            "domain::StateAliasA",
            "domain::StateAliasB",
            "domain::StateAliasC",
            "domain::StateAliasD",
            "domain::StateAliasE",
        ];
        let facade = imported_attestation_facade(&[
            (state_paths[0], STATE, ImportedShape::Struct, Box::default()),
            (state_paths[1], STATE, ImportedShape::Struct, Box::default()),
            (state_paths[2], STATE, ImportedShape::Struct, Box::default()),
            (state_paths[3], STATE, ImportedShape::Struct, Box::default()),
            (state_paths[4], STATE, ImportedShape::Struct, Box::default()),
            (state_paths[5], STATE, ImportedShape::Struct, Box::default()),
            (
                "domain::Screen",
                SCREEN,
                ImportedShape::Enum,
                Box::default(),
            ),
            (
                "domain::ScreenAlias",
                SCREEN,
                ImportedShape::Enum,
                Box::default(),
            ),
        ])
        .unwrap();
        let owner = host::facade::Dependency::new(facade.root_path(), facade.fingerprint().clone());
        let mut builder = Builder::new(facade);
        let state = builder
            .bind_imported_type_attestation(STATE, |value, expected| {
                expected.attest(value.read_lock::<ImportedState>().is_some())
            })
            .unwrap();
        assert_eq!(state.selector(), &STATE);
        assert_eq!(state.rows(), state_paths.len());
        let screen = builder
            .bind_imported_type_attestation(SCREEN, |value, expected| {
                expected.attest(value.read_lock::<ImportedScreen>().is_some())
            })
            .unwrap();
        assert_eq!(screen.selector(), &SCREEN);
        assert_eq!(screen.rows(), 2);
        assert!(matches!(
            builder.bind_imported_type_attestation(STATE, |_, expected| expected.attest(true)),
            Err(super::Error::DuplicateTypeAttestation { .. })
        ));

        let mut registry = Registry::new();
        registry.install(builder.finish()).unwrap();
        let model = empty_model();
        for path in state_paths {
            let ty = type_ref::Type::imported(
                format!("gallery_ui::{path}").into(),
                host::type_def::import::Identity::new(STATE),
                Box::default(),
                owner.clone(),
            );
            ty.validate(&Dynamic::from(ImportedState), &model, &registry)
                .unwrap();
            assert!(matches!(
                ty.validate(&Dynamic::from(ImportedScreen), &model, &registry),
                Err(type_ref::Error::AttestationMismatch { .. })
            ));
        }
        for path in [
            "gallery_ui::domain::Screen",
            "gallery_ui::domain::ScreenAlias",
        ] {
            type_ref::Type::imported(
                path.into(),
                host::type_def::import::Identity::new(SCREEN),
                Box::default(),
                owner.clone(),
            )
            .validate(&Dynamic::from(ImportedScreen), &model, &registry)
            .unwrap();
        }
    }

    #[cfg(feature = "rust-import")]
    #[test]
    fn first_middle_and_last_alias_collisions_leave_the_cohort_unmodified() {
        const STATE: [u8; 32] = [0x71; 32];
        let paths = ["domain::Alpha", "domain::Middle", "domain::Zulu"];
        for occupied in paths {
            let facade = imported_attestation_facade(&[
                (paths[0], STATE, ImportedShape::Struct, Box::default()),
                (paths[1], STATE, ImportedShape::Struct, Box::default()),
                (paths[2], STATE, ImportedShape::Struct, Box::default()),
            ])
            .unwrap();
            let owner =
                host::facade::Dependency::new(facade.root_path(), facade.fingerprint().clone());
            let mut builder = Builder::new(facade);
            let occupied = format!("gallery_ui::{occupied}");
            let existing: Shared<dyn super::TypeAttestor> =
                Shared::new(|_: &Dynamic, expected: &super::Instantiation<'_>| {
                    expected.attest(true)
                });
            builder
                .attestations
                .insert(occupied.clone().into(), existing);

            assert!(matches!(
                builder.bind_imported_type_attestation(STATE, |_, expected| {
                    expected.attest(false)
                }),
                Err(super::Error::DuplicateTypeAttestation { ref ty })
                    if ty.as_str() == occupied
            ));
            assert_eq!(builder.attestations.len(), 1);
            assert!(builder.attestations.contains_key(occupied.as_str()));

            let mut registry = Registry::new();
            registry.install(builder.finish()).unwrap();
            type_ref::Type::imported(
                occupied.into(),
                host::type_def::import::Identity::new(STATE),
                Box::default(),
                owner,
            )
            .validate(&Dynamic::from(ImportedScreen), &empty_model(), &registry)
            .unwrap();
        }
    }

    #[test]
    fn nominal_validation_rejects_the_same_identity_with_a_stale_schema() {
        let old = resolved("struct State { value: i64 }");
        let old_model = Model::new(GenerationId::issue(), &old);
        let old_definition = definition(&old, "State");
        let identity = type_def::Identity::Script(old_definition.key().clone());
        let proof = old_model
            .prove_struct(
                &identity,
                old_definition.schema(),
                vec![Dynamic::from(1_i64)].into_boxed_slice(),
            )
            .unwrap();
        let value: Dynamic =
            nominal::Struct::construct(&old_model, &identity, old_definition.schema(), proof)
                .unwrap()
                .into();

        let current = resolved("struct State { value: bool }");
        let current_model = Model::new(GenerationId::issue(), &current);
        let current_definition = definition(&current, "State");
        let expected = type_ref::Type::script(
            identity,
            current_definition.schema().clone(),
            current_definition.kind(),
            TypeId::of::<Dynamic>(),
            Box::default(),
        );
        let registry = Registry::new();
        let error = expected
            .validate(&value, &current_model, &registry)
            .unwrap_err();
        assert!(matches!(
            error,
            type_ref::Error::Nominal(crate::program::construction::Error::SchemaMismatch { .. })
        ));
    }

    #[test]
    fn widget_activation_is_required_on_the_exact_facade() {
        let facade = widget_facade("iced", 1);
        let (_descriptor, requirement, _model) = widget_requirement(&facade);
        let mut registry = Registry::new();
        registry.install(Builder::new(facade).finish()).unwrap();
        install_lifecycle(&mut registry);

        assert!(matches!(
            registry.validate(
                &[requirement.owner().clone()],
                &[],
                &[],
                &[requirement],
                &[],
            ),
            Err(super::Error::MissingWidgetBinding { .. })
        ));
    }

    #[test]
    fn widget_binding_rejects_ambiguous_runtime_carrier_identity() {
        let mut facade =
            host::facade::Builder::new("iced", host::facade::Abi::new(1).unwrap()).unwrap();
        facade.module("widget").unwrap();
        facade
            .type_::<OpenWidget>("widget::First", &["Message"], &[])
            .unwrap();
        facade
            .type_::<OpenWidget>("widget::Second", &["Message"], &[])
            .unwrap();
        let mut runtime = Builder::new(facade.finish().unwrap());

        assert!(matches!(
            runtime.bind_widget::<OpenWidget>(|_, witness| {
                Ok(Dynamic::from(OpenWidget {
                    witness: Some(witness),
                }))
            }),
            Err(super::Error::AmbiguousWidgetType { .. })
        ));
    }

    #[test]
    fn widget_activation_cannot_borrow_a_foreign_facade_binding() {
        let facade = widget_facade("iced", 1);
        let (_descriptor, requirement, _model) = widget_requirement(&facade);
        let foreign = widget_facade("foreign", 1);
        let mut foreign_runtime = Builder::new(foreign);
        foreign_runtime
            .bind_widget::<OpenWidget>(|_, witness| {
                Ok(Dynamic::from(OpenWidget {
                    witness: Some(witness),
                }))
            })
            .unwrap();
        let mut registry = Registry::new();
        registry.install(Builder::new(facade).finish()).unwrap();
        install_lifecycle(&mut registry);
        registry.install(foreign_runtime.finish()).unwrap();

        assert!(matches!(
            registry.validate(
                &[requirement.owner().clone()],
                &[],
                &[],
                &[requirement],
                &[],
            ),
            Err(super::Error::MissingWidgetBinding { .. })
        ));
    }

    #[test]
    fn widget_activation_rejects_another_target_class_in_the_same_facade() {
        let facade = widget_facade("iced", 1);
        let (_descriptor, requirement, _model) = widget_requirement(&facade);
        let owner = requirement.owner().clone();
        let target = type_ref::Type::host(
            "iced::widget::Other".into(),
            TypeId::of::<OtherWidget>(),
            requirement.target().arguments().to_vec().into_boxed_slice(),
            Some(owner.clone()),
        );
        let wrong = widget::Requirement::bind(owner, target);
        let mut runtime = Builder::new(facade);
        runtime
            .bind_widget::<OpenWidget>(|_, witness| {
                Ok(Dynamic::from(OpenWidget {
                    witness: Some(witness),
                }))
            })
            .unwrap();
        let mut registry = Registry::new();
        registry.install(runtime.finish()).unwrap();
        install_lifecycle(&mut registry);

        assert!(matches!(
            registry.validate(&[wrong.owner().clone()], &[], &[], &[wrong], &[]),
            Err(super::Error::MissingWidgetBinding { .. })
        ));
    }

    #[test]
    fn widget_activation_output_must_attest_its_exact_target() {
        let facade = widget_facade("iced", 1);
        let (descriptor, requirement, model) = widget_requirement(&facade);
        let mut runtime = Builder::new(facade);
        runtime
            .bind_type_attestation("iced::Element", |value, expected| {
                value
                    .read_lock::<OpenWidget>()
                    .and_then(|value| expected.attest_witness(value.witness.as_ref()))
            })
            .unwrap();
        runtime
            .bind_widget::<OpenWidget>(|_, _| Ok(Dynamic::from(OpenWidget { witness: None })))
            .unwrap();
        let mut registry = Registry::new();
        registry.install(runtime.finish()).unwrap();
        install_lifecycle(&mut registry);

        let error = registry
            .activate_widget(&requirement, descriptor, &model)
            .unwrap_err();
        assert!(error.to_string().contains("failed exact type attestation"));
    }

    #[test]
    fn widget_activation_issues_the_exact_target_witness() {
        let facade = widget_facade("iced", 1);
        let mut engine = Engine::new();
        engine.register_host_facade(facade.clone()).unwrap();
        engine.register_host_facade(lifecycle_facade()).unwrap();
        let engine = Shared::new(engine);
        let authority = engine.compile_authority(widget_source()).unwrap();
        let view = authority
            .items()
            .find(|item| item.key().name() == "view")
            .unwrap()
            .clone();
        let mut runtime = Builder::new(facade);
        runtime
            .bind_type_attestation("iced::Element", |value, expected| {
                value
                    .read_lock::<OpenWidget>()
                    .and_then(|value| expected.attest_witness(value.witness.as_ref()))
            })
            .unwrap();
        runtime
            .bind_widget::<OpenWidget>(|_, witness| {
                Ok(Dynamic::from(OpenWidget {
                    witness: Some(witness),
                }))
            })
            .unwrap();
        let mut registry = Registry::new();
        registry.install(runtime.finish()).unwrap();
        install_lifecycle(&mut registry);

        let value = authority
            .invoke_bound(&view, &mut [], Shared::new(registry))
            .unwrap();
        assert!(value.read_lock::<OpenWidget>().is_some());
    }

    fn generic_facade() -> host::facade::Definition {
        let mut facade =
            host::facade::Builder::new("generic", host::facade::Abi::new(1).unwrap()).unwrap();
        facade.type_::<ErasedGeneric>("Value", &["T"], &[]).unwrap();
        facade.finish().unwrap()
    }

    #[cfg(feature = "rust-import")]
    #[derive(Clone, Copy)]
    enum ImportedShape {
        Opaque,
        Struct,
        Enum,
    }

    /// An imported type to declare: its path, identity, shape and type
    /// parameters.
    #[cfg(feature = "rust-import")]
    type ImportedType<'a> = (
        &'a str,
        [u8; 32],
        ImportedShape,
        Box<[crate::ImmutableString]>,
    );

    #[cfg(feature = "rust-import")]
    fn imported_attestation_facade(
        types: &[ImportedType<'_>],
    ) -> Result<host::facade::Definition, host::Error> {
        let mut allocation = host::TestImportAllocation::default();
        let mut facade = host::facade::Builder::new_imported(
            "gallery_ui",
            host::facade::Abi::new(1).unwrap(),
            &mut allocation,
        )?;
        facade.imported_module("domain", &mut allocation)?;
        for (path, identity, shape, parameters) in types {
            let shape: Shared<host::type_def::Shape> = match shape {
                ImportedShape::Opaque => host::type_def::Shape::Opaque,
                ImportedShape::Struct => host::type_def::Shape::Struct {
                    form: host::type_def::Form::Record,
                    constructor: None,
                    fields: Box::default(),
                    constructible: false,
                },
                ImportedShape::Enum => host::type_def::Shape::Enum(Box::default()),
            }
            .into();
            facade.imported_type(
                path,
                host::type_def::import::Identity::new(*identity),
                &[],
                parameters.clone(),
                Box::default(),
                shape,
                &mut allocation,
            )?;
        }
        facade.finish()
    }

    fn dependency_facade(root: &str, abi: u32) -> host::facade::Definition {
        let mut facade =
            host::facade::Builder::new(root, host::facade::Abi::new(abi).unwrap()).unwrap();
        facade.type_::<DependencyValue>("Value", &[], &[]).unwrap();
        facade.finish().unwrap()
    }

    fn dependent_facade(
        alpha: &host::facade::Definition,
        beta: &host::facade::Definition,
    ) -> host::facade::Definition {
        let alpha = alpha.export_type("Value").unwrap();
        let beta = beta.export_type("Value").unwrap();
        let mut facade =
            host::facade::Builder::new("extension", host::facade::Abi::new(1).unwrap()).unwrap();
        facade
            .type_::<DependentValue>("Projected", &[], &[])
            .unwrap();
        facade.project_to("Projected", alpha.clone(), &[]).unwrap();
        let mut signature = host::signature::Builder::new();
        signature.output(
            host::type_ref::Type::tuple([
                host::type_ref::Type::external(alpha, []).unwrap(),
                host::type_ref::Type::callable(
                    host::type_ref::Type::builtin(host::type_ref::Builtin::Bool),
                    host::type_ref::Type::external(beta, []).unwrap(),
                ),
            ])
            .unwrap(),
        );
        facade
            .item(host::item::Definition::function("inspect", signature.finish()).unwrap())
            .unwrap();
        facade.finish().unwrap()
    }

    fn widget_facade(root: &str, abi: u32) -> host::facade::Definition {
        let mut facade =
            host::facade::Builder::new(root, host::facade::Abi::new(abi).unwrap()).unwrap();
        facade.module("widget").unwrap();
        facade
            .type_::<OpenWidget>("Element", &["Message"], &[])
            .unwrap();
        facade
            .type_::<OtherWidget>("widget::Other", &["Message"], &[])
            .unwrap();
        facade.finish().unwrap()
    }

    fn widget_requirement(
        facade: &host::facade::Definition,
    ) -> (
        crate::widget::Descriptor,
        widget::Requirement,
        crate::program::model::Model,
    ) {
        let mut engine = Engine::new();
        engine.register_host_facade(facade.clone()).unwrap();
        engine.register_host_facade(lifecycle_facade()).unwrap();
        let executable = engine.compile_executable(widget_source()).unwrap();
        let promotion = executable.widget_promotions()[0].clone();
        let requirement = executable.widget_requirements()[0].clone();
        let model = executable.model().clone();
        (
            crate::widget::Descriptor::new(promotion, Dynamic::UNIT),
            requirement,
            model,
        )
    }

    fn lifecycle_facade() -> host::facade::Definition {
        host::facade::Builder::new("rawr_iced", host::facade::Abi::new(1).unwrap())
            .unwrap()
            .finish()
            .unwrap()
    }

    fn install_lifecycle(registry: &mut Registry) {
        registry
            .install(Builder::new(lifecycle_facade()).finish())
            .unwrap();
    }

    fn widget_source() -> &'static str {
        r#"
            struct Dial;
            enum Message { Tick }
            #[widget]
            impl Dial {}
            pub fn view() -> iced::Element<Message> { Dial {} }
        "#
    }

    fn empty_model() -> Model {
        let program = resolved("");
        Model::new(GenerationId::issue(), &program)
    }

    fn resolved(source: &str) -> resolve::Program {
        let engine = Engine::new();
        resolve::resolve(engine.parse_unit(source).unwrap(), &host::Registry::new()).unwrap()
    }

    fn definition<'a>(program: &'a resolve::Program, name: &str) -> &'a type_def::Definition {
        program
            .types()
            .iter()
            .find(|definition| definition.name() == name)
            .unwrap()
    }
}
