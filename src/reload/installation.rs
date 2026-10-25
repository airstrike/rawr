//! One-shot native binding for bootstrap and compatibility-proven replacement.

use std::fmt;
use std::sync::Arc;

use crate::{application, host, item, preview, program, reload, Dynamic, Engine, Shared};

pub mod current;
pub mod installed;
pub mod operation;

/// One native-bound generation prepared for initial host bootstrap.
pub struct Bootstrap {
    generation: Arc<reload::Generation>,
    binding: Binding,
}

/// One compatibility-proven native-bound generation prepared for replacement.
///
/// The private binding cannot be paired with different compatibility evidence.
/// Installation still compares the private retained proof by pointer with the
/// live host immediately before consuming this value.
///
/// ```compile_fail
/// # use rawr::reload::installation::Replacement;
/// fn escape(replacement: Replacement) {
///     let _catalog = replacement.binding();
/// }
/// ```
pub struct Replacement {
    retained: Arc<reload::Generation>,
    generation: Arc<reload::Generation>,
    binding: Binding,
    evidence: Box<[reload::compatibility::Evidence]>,
}

/// One non-cloneable replacement tied to the exact current slot it observed.
#[doc(hidden)]
pub struct Preparing<'a, T> {
    replacement: Replacement,
    current: &'a current::Slot<T>,
    proof: current::Proof,
}

/// Capability-attenuated catalog authority usable by one sealed host payload.
///
/// This value exposes only exact catalog operations. It cannot yield its
/// underlying bound catalog or be dereferenced into raw evaluator authority.
#[doc(hidden)]
pub struct Catalog<'a>(&'a preview::BoundCatalog);

/// Capability-attenuated application authority for one sealed host payload.
#[doc(hidden)]
pub struct Application<'a>(&'a application::BoundLaunch);

pub(super) struct Binding(pub(super) Arc<preview::BoundCatalog>);

/// A candidate-binding view whose result remains sealed until admission.
#[doc(hidden)]
pub struct Preparation<'a> {
    binding: &'a Binding,
}

/// A prepared payload no longer matches the exact live host identity or epoch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Stale;

/// A compiled candidate could not cross exact engine-origin or runtime binding.
#[derive(Debug)]
pub enum Error {
    /// The candidate did not originate from the supplied compiler engine.
    Origin(program::Error),
    /// Native facade implementations did not satisfy the candidate contract.
    Runtime(host::runtime::Error),
}

impl reload::Candidate {
    /// Consume this candidate into the distinct initial-host transformation.
    pub fn bind(
        self,
        engine: &Shared<Engine>,
        runtime: host::runtime::Registry,
    ) -> Result<Bootstrap, Error> {
        let (generation, binding) = bind(self, engine, runtime)?;
        Ok(Bootstrap {
            generation,
            binding,
        })
    }
}

impl reload::compatibility::Plan {
    /// Consume this compatibility proof and bind its candidate exactly once.
    ///
    /// Engine-origin and runtime validation finish before a replacement value
    /// exists, so failure cannot expose a partially bound installation.
    pub fn bind(
        self,
        engine: &Shared<Engine>,
        runtime: host::runtime::Registry,
    ) -> Result<Replacement, Error> {
        let (retained, candidate, evidence) = self.into_parts();
        let (generation, binding) = bind(candidate, engine, runtime)?;
        Ok(Replacement {
            retained,
            generation,
            binding,
            evidence,
        })
    }
}

impl Bootstrap {
    /// Return the immutable metadata proof for this initial generation.
    #[must_use]
    pub const fn generation(&self) -> &Arc<reload::Generation> {
        &self.generation
    }

    /// Borrow the candidate authority while constructing initial host state.
    #[must_use]
    pub fn preparation(&self) -> Preparation<'_> {
        Preparation {
            binding: &self.binding,
        }
    }

    /// Call the root package's `fn main` once, for a program that is not an
    /// application. `None` when the package has no `main`.
    #[doc(hidden)]
    pub fn invoke_main(&self) -> Option<Result<Dynamic, program::Error>> {
        self.binding.0.invoke_main()
    }

    /// The bound catalog native islands call into.
    #[cfg(feature = "rust-import")]
    pub(crate) fn catalog(&self) -> &preview::BoundCatalog {
        &self.binding.0
    }

    /// Consume this bootstrap and install one complete authority-free state.
    #[must_use]
    pub fn install<T>(self, state: T) -> current::Slot<T> {
        current::Slot::new(self.generation, self.binding, state)
    }

    /// Consume a runtime-bound bootstrap into an owned, capability-attenuated
    /// catalog for one-shot deterministic snapshot execution.
    #[doc(hidden)]
    #[must_use]
    pub fn into_snapshot_catalog(self) -> installed::Catalog {
        installed::Catalog::from_bound(self.binding.0)
    }
}

impl Replacement {
    /// Return the immutable metadata proof prepared for installation.
    #[must_use]
    pub const fn generation(&self) -> &Arc<reload::Generation> {
        &self.generation
    }

    /// Return structured compatibility evidence for the exact live-owner set.
    #[must_use]
    pub const fn evidence(&self) -> &[reload::compatibility::Evidence] {
        &self.evidence
    }

    /// Return structured compatibility evidence for one exact live owner.
    #[must_use]
    pub fn owner(
        &self,
        owner: &reload::compatibility::Owner,
    ) -> Option<&reload::compatibility::Evidence> {
        self.evidence
            .binary_search_by(|evidence| evidence.owner().cmp(owner))
            .ok()
            .map(|index| &self.evidence[index])
    }

    /// Tie this replacement to one exact current slot before evaluation.
    pub fn prepare<T>(self, current: &current::Slot<T>) -> Result<Preparing<'_, T>, Stale> {
        if !current.retains(&self.retained) {
            return Err(Stale);
        }
        Ok(Preparing {
            replacement: self,
            current,
            proof: current.proof(),
        })
    }
}

impl<T> Preparing<'_, T> {
    /// Borrow candidate authority while constructing replacement host state.
    #[must_use]
    pub fn preparation(&self) -> Preparation<'_> {
        Preparation {
            binding: &self.replacement.binding,
        }
    }

    /// Borrow the exact current authority-free state observed by this proof.
    #[must_use]
    pub fn state(&self) -> &T {
        self.current.state()
    }

    /// Consume this proof and seal one complete authority-free replacement.
    #[must_use]
    pub fn seal(self, state: T) -> current::Prepared<T> {
        let Self {
            replacement,
            current,
            proof,
        } = self;
        current.seal(
            proof,
            replacement.retained,
            replacement.generation,
            replacement.binding,
            replacement.evidence,
            state,
        )
    }
}

impl Preparation<'_> {
    /// Mint a capability-attenuated catalog for the still-sealed host payload.
    #[must_use]
    pub fn catalog(&self) -> Catalog<'_> {
        Catalog(&self.binding.0)
    }
}

impl Catalog<'_> {
    /// Clone the optional application launch under attenuated catalog authority.
    #[must_use]
    pub fn application(&self) -> Option<Application<'_>> {
        self.0.application().map(Application)
    }

    /// Return compiler-validated previews in stable key order.
    #[must_use]
    pub fn previews(&self) -> &[preview::Definition] {
        self.0.previews()
    }

    /// Return the immutable executable generation identifier.
    #[must_use]
    pub fn generation(&self) -> program::GenerationId {
        self.0.generation()
    }

    /// Return the compiler-selected root package name.
    #[must_use]
    pub fn package(&self) -> &str {
        self.0.package()
    }

    /// Materialize one exact compiler-proven preview fixture.
    pub fn seed(&self, key: &preview::Key) -> Result<Option<Dynamic>, preview::RuntimeError> {
        self.0.seed(key)
    }

    /// Invoke and normalize one exact preview subject.
    pub fn subject(
        &self,
        key: &preview::Key,
        fixture: Option<&Dynamic>,
    ) -> Result<Dynamic, preview::RuntimeError> {
        self.0.subject(key, fixture)
    }

    /// Invoke one preview update and retain its durable task value.
    pub fn update(
        &self,
        key: &preview::Key,
        state: &Dynamic,
        message: Dynamic,
    ) -> Result<preview::Update, preview::RuntimeError> {
        self.0.update(key, state, message)
    }

    /// Evaluate one explicit preview theme choice.
    pub fn theme(
        &self,
        key: &preview::Key,
        index: usize,
        state: Option<&Dynamic>,
    ) -> Result<Dynamic, preview::RuntimeError> {
        self.0.theme(key, index, state)
    }

    /// Resolve one preview source location exactly.
    pub fn source(
        &self,
        location: preview::Location,
    ) -> Result<crate::diagnostic::Source, preview::RuntimeError> {
        self.0.source(location)
    }
}

impl Application<'_> {
    /// Return the normalized boot contract.
    #[must_use]
    pub fn boot(&self) -> &application::Boot {
        self.0.boot()
    }

    /// Return `main`'s statements before the entry, when present.
    #[must_use]
    pub fn prelude(&self) -> Option<&application::Prelude> {
        self.0.prelude()
    }

    /// Return the normalized update callback.
    #[must_use]
    pub fn update(&self) -> &application::Update {
        self.0.update()
    }

    /// Return the normalized view callback.
    #[must_use]
    pub fn view(&self) -> &application::View {
        self.0.view()
    }

    /// Return the optional state-derived title callback.
    #[must_use]
    pub fn title(&self) -> Option<&application::Title> {
        self.0.title()
    }

    /// Return the optional state-derived theme callback.
    #[must_use]
    pub fn theme(&self) -> Option<&application::Theme> {
        self.0.theme()
    }

    /// Return the optional state-derived subscription callback.
    #[must_use]
    pub fn subscription(&self) -> Option<&application::Subscription> {
        self.0.subscription()
    }

    /// Return whether the application requests centered initial placement.
    #[must_use]
    pub fn centered(&self) -> bool {
        self.0.centered()
    }

    /// Return the immutable executable generation identifier.
    #[must_use]
    pub fn generation(&self) -> program::GenerationId {
        self.0.generation()
    }

    /// Resolve exact retained declaration evidence for one callback.
    #[must_use]
    pub fn callback_evidence(
        &self,
        callback: &application::Callback,
    ) -> Option<application::callback::Evidence<'_>> {
        self.0.callback_evidence(callback)
    }

    /// Resolve the retained evidence of the callable `error` came from, for
    /// a failure no callback owns (a Task's async body).
    #[must_use]
    pub fn error_evidence(
        &self,
        error: &program::Error,
    ) -> Option<application::callback::Evidence<'_>> {
        self.0.error_evidence(error)
    }

    /// Materialize the default application State.
    pub fn boot_default(&self) -> Result<Dynamic, application::RuntimeError> {
        self.0.boot_default()
    }

    /// Run `main`'s statements before the entry, when there are any.
    pub fn invoke_prelude(&self) -> Result<(), application::RuntimeError> {
        self.0.invoke_prelude()
    }

    /// Invoke and validate the explicit application boot callback.
    pub fn invoke_boot(&self) -> Result<application::Startup, application::RuntimeError> {
        self.0.invoke_boot()
    }

    /// Invoke and normalize the application view callback.
    pub fn invoke_view(&self, arguments: &mut [Dynamic]) -> Result<Dynamic, program::Error> {
        self.0.invoke_view(arguments)
    }

    /// Invoke and normalize the optional application title callback.
    pub fn invoke_title(
        &self,
        arguments: &mut [Dynamic],
    ) -> Result<Option<Dynamic>, program::Error> {
        self.0.invoke_title(arguments)
    }

    /// Invoke and normalize the optional application theme callback.
    pub fn invoke_theme(
        &self,
        arguments: &mut [Dynamic],
    ) -> Result<Option<Dynamic>, program::Error> {
        self.0.invoke_theme(arguments)
    }

    /// Invoke and normalize the optional application subscription callback.
    pub fn invoke_subscription(
        &self,
        arguments: &mut [Dynamic],
    ) -> Result<Option<Dynamic>, program::Error> {
        self.0.invoke_subscription(arguments)
    }

    /// Validate one exact application State value.
    pub fn validate_state(&self, value: &Dynamic) -> Result<(), application::RuntimeError> {
        self.0.validate_state(value)
    }

    /// The State `value` as bytes a new runner process can seed from.
    pub fn export_state(&self, value: &Dynamic) -> Option<Vec<u8>> {
        self.0.export_state(value)
    }

    /// The State exported `bytes` carry, rebuilt in this program, when every
    /// script type in it is unchanged.
    pub fn import_state(&self, bytes: &[u8]) -> Option<Dynamic> {
        self.0.import_state(bytes)
    }

    /// Prepare one compiler-proven host operation as an inert sealed value.
    pub fn prepare_operation(
        &self,
        call: host::operation::Call,
        mapper: &item::Value,
    ) -> Result<operation::Prepared, host::operation::Error> {
        self.0
            .issue_operation(call, mapper)
            .map(operation::Prepared::new)
    }
}

impl fmt::Debug for Bootstrap {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Bootstrap")
            .field("generation", &self.generation.id())
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for Replacement {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Replacement")
            .field("evidence", &self.evidence)
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for Binding {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Binding(<private>)")
    }
}

impl fmt::Debug for Catalog<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Catalog(<private>)")
    }
}

impl fmt::Debug for Application<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Application(<private>)")
    }
}

impl fmt::Display for Stale {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("prepared reload no longer matches the exact live host")
    }
}

impl std::error::Error for Stale {}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Origin(error) => error.fmt(formatter),
            Self::Runtime(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Origin(error) => Some(error),
            Self::Runtime(error) => Some(error),
        }
    }
}

// Bound preview catalogs share one exact identity across host-owned adapters;
// they are intentionally not transferred to worker threads.
#[allow(clippy::arc_with_non_send_sync)]
fn bind(
    candidate: reload::Candidate,
    engine: &Shared<Engine>,
    runtime: host::runtime::Registry,
) -> Result<(Arc<reload::Generation>, Binding), Error> {
    let reload::Candidate {
        generation,
        catalog,
    } = candidate;
    let launch = engine.launch_previews(catalog).map_err(Error::Origin)?;
    let catalog = launch.bind(runtime).map_err(Error::Runtime)?;
    debug_assert_eq!(generation.id(), catalog.generation());
    Ok((generation, Binding(Arc::new(catalog))))
}
