//! Generated-owner bridge from exact imported iced evidence to retained semantics.

use std::cell::Cell;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use rawr::host;
use rawr_import::surface::facade;
use sha2::{Digest as _, Sha256};

const ROOT: &str = "iced";
const SEMANTIC_TYPES: &[&str] = &[
    "Application",
    "Element",
    "Length",
    "Result",
    "Subscription",
    "Task",
    "Theme",
    "task::Handle",
    "task::Sipper",
];
const SEMANTIC_ITEMS: &[&str] = &["Length::Fill", "Length::Fixed"];
const SEMANTIC_ITEM_ROOTS: &[&str] = &[
    "Application::",
    "Element::",
    "Subscription::",
    "Task::",
    "Theme::",
];
const COMPATIBILITY_POLICY: &[u8] = b"rawr.iced.compatibility-composition/v2";
const COMPATIBILITY_SEMANTIC_POLICY: &[u8] = b"rawr.iced.compatibility-application-cycle/v3";
const COMPATIBILITY_RUNTIME_BINDING: &[u8] = b"rawr.iced.compatibility-runtime-binding/v2";
const TEXT_ENDPOINT_DISPOSITION: &[u8] =
    b"rawr.iced.compatibility-text-endpoint/suppressed-by-D271/v1";

/// One exact target-argument substitution sealed into a generated projection table.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Argument {
    /// Reuse one zero-based source type parameter.
    Source(u32),
    /// Infer a fresh target parameter at the checked use site.
    Fresh,
}

/// Closed generated Rust namespace for one support-only alias proof.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Namespace {
    /// Type namespace.
    Type,
    /// Value namespace.
    Value,
    /// Macro namespace.
    Macro,
}

/// Closed generic-parameter class in one alias normalization mapping.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParameterKind {
    /// Lifetime parameter.
    Lifetime,
    /// Type parameter.
    Type,
    /// Const parameter.
    Const,
}

/// One ordered alias-formal to target-formal parameter mapping.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AliasParameter {
    kind: ParameterKind,
    alias: u32,
    target: u32,
}

/// One total source-alias substitution relation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceArgument {
    /// Bind one alias/source lifetime pair to the same fresh view region.
    Region { alias: u32, source: u32 },
    /// Preserve one type or const parameter through normalization.
    Parameter {
        kind: ParameterKind,
        alias: u32,
        source: u32,
    },
}

/// Closed iced extension ABI role.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ExtensionAbiRole {
    Color,
    Length,
    Rectangle,
    Vector,
}

/// One explicit closed ABI specialization argument.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionAbiArgument {
    /// Apply the compiler-proved `f32` default to one exact type formal.
    DefaultF32 { parameter: u32 },
}

/// One exact compiler formal retained by a generated operation site.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OperationFormal {
    owner: [u8; 32],
    index: u32,
    kind: ParameterKind,
}

/// One total profile-owned operation specialization.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EndpointArgument {
    Region {
        parameter: u32,
    },
    Definition {
        parameter: u32,
        definition: [u8; 32],
    },
    OwnedString {
        parameter: u32,
        lifetime: u32,
    },
}

/// One generated support-only source alias and normalized target proof.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AliasTarget<'parameters> {
    schema: u32,
    root: &'parameters str,
    path: &'parameters str,
    namespace: Namespace,
    export: [u8; 32],
    alias: [u8; 32],
    target: [u8; 32],
    parameters: &'parameters [AliasParameter],
    site: [u8; 32],
    selector: [u8; 32],
}

/// One generated support-only source site and its complete compiler proof.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProjectionSourceSite<'proof> {
    schema: u32,
    root: &'proof str,
    path: &'proof str,
    namespace: Namespace,
    export: [u8; 32],
    alias: [u8; 32],
    source: [u8; 32],
    parameters: &'proof [AliasParameter],
    arguments: &'proof [SourceArgument],
    site: [u8; 32],
    widget_trait: [u8; 32],
    widget: [u8; 32],
    target: [u8; 32],
    selector: [u8; 32],
}

/// One generated support-only linked extension ABI proof.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExtensionAbi<'proof> {
    schema: u32,
    role: ExtensionAbiRole,
    root: &'proof str,
    path: &'proof str,
    namespace: Namespace,
    export: [u8; 32],
    definition: [u8; 32],
    shape: [u8; 32],
    site: [u8; 32],
    linked: [u8; 32],
    arguments: &'proof [ExtensionAbiArgument],
    selector: [u8; 32],
}

/// One generated support-only operation endpoint proof.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OperationEndpoint<'proof> {
    schema: u32,
    root: &'proof str,
    path: &'proof str,
    namespace: Namespace,
    export: [u8; 32],
    definition: [u8; 32],
    formals: &'proof [OperationFormal],
    shape: [u8; 32],
    site: [u8; 32],
    source: [u8; 32],
    projection: [u8; 32],
    arguments: &'proof [EndpointArgument],
    linked: [u8; 32],
    selector: [u8; 32],
}

/// One generated direct-projection proof consumed by the linked iced host.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Projection<'arguments> {
    schema: u32,
    source: [u8; 32],
    alias: [u8; 32],
    selector: [u8; 32],
    arguments: &'arguments [Argument],
}

/// Compiler facades reconstructed from exact Rust evidence and linked semantics.
pub struct Compiler {
    profile: [u8; 32],
    definitions: Box<[host::facade::Definition]>,
    application_cycle: host::facade::ProfileCycle,
}

/// One authenticated generated-profile disposition replacing the baseline
/// `iced::Element` attestor with its exact composed authority.
pub(crate) struct ElementReplacement(());

/// One compiler/runtime-bound generation ready for iced lifecycle ownership.
pub struct BoundGeneration {
    profile: [u8; 32],
    host: Generation,
    /// The runner's one registry. Each later script generation takes its
    /// next generation, so native values stay valid across edits.
    runtime: host::runtime::Registry,
    snapshot: rawr::reload::source::Snapshot,
    observation: Option<Startup>,
}

// One per run, built once and moved into the run loop; boxing a variant
// would add an allocation and save nothing.
#[allow(clippy::large_enum_variant)]
enum Generation {
    Application(crate::program::Host),
    /// A plain program: no application, a `main` run once.
    Main(rawr::reload::installation::Bootstrap),
    Preview(crate::control::Host),
    Snapshot {
        suite: crate::snapshot::Suite,
        mode: crate::snapshot::Mode,
    },
}

/// One closed child-side lifecycle selected before generation binding.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Mode {
    Check,
    Run,
    Preview,
    SnapshotCheck,
    SnapshotUpdate,
}

/// Sole main-thread native lifecycle after the socket owner has split off.
pub struct Running {
    live: LiveGeneration,
    _observer: Option<Arc<rawr_watch::Control>>,
}

/// Bounded protocol-side installation and graceful-exit transform.
pub struct Transport {
    gate: Arc<rawr::reload::scheduler::Gate>,
    publication: Arc<rawr::reload::scheduler::publication::Slot>,
    sender: crate::reload::Sender,
    feedback: std::sync::mpsc::Receiver<crate::reload::Feedback>,
    retry: crate::reload::Retry,
    // Serial transport cannot reuse uncorrelated feedback after a lost reply.
    feedback_uncertain: Cell<bool>,
    observer: Option<rawr_watch::Adapter>,
    native_baseline: Option<rawr_watch::native::Baseline>,
    exporter: crate::reload::export::Exporter,
}

/// Single-use authority retained from preparation through actual UI admission.
#[must_use = "a pending installation must be installed or cancelled by dropping it"]
pub struct PendingInstall {
    request: rawr::reload::scheduler::Request,
    gate: Arc<rawr::reload::scheduler::Gate>,
    observation: Option<rawr::reload::scheduler::publication::Observed>,
    activity: Option<activity::Activity>,
}

mod activity;

impl PendingInstall {
    fn begin(gate: &Arc<rawr::reload::scheduler::Gate>) -> Result<Self, InstallError> {
        let now = Instant::now();
        gate.observe_and_start(now)
            .map_err(|error| {
                InstallError::NotInstalled(Error::Reload(error.to_string().into_boxed_str()))
            })?
            .map(|request| Self {
                request,
                gate: Arc::clone(gate),
                observation: None,
                activity: activity::Activity::begin(),
            })
            .ok_or_else(|| {
                InstallError::NotInstalled(Error::Reload(
                    "reload request did not become ready".into(),
                ))
            })
    }

    fn observe(
        &mut self,
        gate: &Arc<rawr::reload::scheduler::Gate>,
        publication: &Arc<rawr::reload::scheduler::publication::Slot>,
        bytes: &[u8],
    ) -> Result<(), InstallError> {
        if !Arc::ptr_eq(&self.gate, gate) || self.observation.is_some() {
            return Err(InstallError::NotInstalled(Error::Reload(
                "foreign or repeated reload observation".into(),
            )));
        }
        let decoded = rawr::reload::source::observation::decode(bytes).map_err(|error| {
            InstallError::NotInstalled(Error::Reload(error.to_string().into_boxed_str()))
        })?;
        self.observation = Some(publication.register_decoded(decoded));
        Ok(())
    }

    fn complete<T>(
        self,
        gate: &rawr::reload::scheduler::Gate,
        value: T,
    ) -> Result<rawr::reload::scheduler::Candidate<T>, InstallError> {
        match gate.retain_current(self.request.complete(value)) {
            rawr::reload::scheduler::Admission::Current(candidate) => Ok(candidate),
            rawr::reload::scheduler::Admission::Obsolete(value) => {
                drop(value);
                let error = Error::Reload(
                    "reload preparation became obsolete or belongs to another owner".into(),
                );
                // Suspension and foreign owners are failures, not newer edits.
                Err(
                    if std::ptr::eq(gate, self.gate.as_ref()) && gate.checkpoint().is_ok() {
                        InstallError::Superseded(error)
                    } else {
                        InstallError::NotInstalled(error)
                    },
                )
            }
        }
    }
}

struct Startup {
    gate: Arc<rawr::reload::scheduler::Gate>,
    publication: Arc<rawr::reload::scheduler::publication::Slot>,
    observer: rawr_watch::Adapter,
    native_baseline: rawr_watch::native::Baseline,
    request: PendingInstall,
}

impl Startup {
    fn attach(expected: &rawr::reload::source::Inputs, bytes: &[u8]) -> Result<Self, Error> {
        let inputs = rawr::reload::source::observation::decode(bytes)
            .map_err(|error| Error::Reload(error.to_string().into_boxed_str()))?
            .into_inputs();
        if inputs
            .source_paths()
            .iter()
            .any(|path| !expected.is_source_only(path))
        {
            return Err(Error::Reload(
                "initial observation claims an uncompiled Rawr source role".into(),
            ));
        }
        if !expected
            .paths()
            .iter()
            .all(|path| inputs.paths().binary_search(path).is_ok())
            || !expected
                .trees()
                .iter()
                .all(|path| inputs.trees().binary_search(path).is_ok())
        {
            return Err(Error::Reload(
                "initial observation omits an exact Rawr input".into(),
            ));
        }
        let gate = Arc::new(rawr::reload::scheduler::Gate::new(Duration::ZERO));
        let publication = Arc::new(rawr::reload::scheduler::publication::Slot::new(inputs));
        let observer = rawr_watch::Adapter::attach(Arc::clone(&gate), Arc::clone(&publication))
            .map_err(|error| Error::Reload(error.to_string().into_boxed_str()))?;
        // Capture before the parent's full final validation and child startup
        // admission. Later adoption must never reset an intervening event.
        let native_baseline = observer.native_baseline();
        let request = PendingInstall::begin(&gate)
            .map_err(|error| Error::Reload(error.to_string().into_boxed_str()))?;
        Ok(Self {
            gate,
            publication,
            observer,
            native_baseline,
            request,
        })
    }
}

fn admit_startup<T>(
    gate: &rawr::reload::scheduler::Gate,
    request: PendingInstall,
    value: T,
) -> Result<T, Error> {
    let candidate = request
        .complete(gate, value)
        .map_err(|error| Error::Reload(error.to_string().into_boxed_str()))?;
    match gate.admit_current(candidate, |value| value) {
        rawr::reload::scheduler::Admission::Current(value) => Ok(value),
        rawr::reload::scheduler::Admission::Obsolete(value) => {
            drop(value);
            Err(Error::Reload(
                "initial generation became obsolete before startup".into(),
            ))
        }
    }
}

const FEEDBACK_TIMEOUT: Duration = Duration::from_secs(10);
/// The iced owner answers a State export between two messages.
const EXPORT_TIMEOUT: Duration = Duration::from_secs(2);

/// Attest a profile-owned support element only by its launch-bound widget witness.
#[doc(hidden)]
pub fn attest_support_element(
    value: &rawr::Dynamic,
    expected: &host::runtime::Instantiation<'_>,
) -> Option<host::runtime::Attested> {
    let value = value.read_lock::<crate::widget::Value>()?;
    expected.attest_witness(value.witness())
}

/// Exact profile reconstruction failed before any compiler facade was installed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A neutral proof cannot authorize the iced semantic host.
    #[error("neutral profile proof cannot authorize the iced runner host")]
    NeutralProfile,
    /// The exact decoded plan does not contain one iced facade owner.
    #[error("generated imported plan must contain exactly one iced facade, found {0}")]
    IcedOwners(usize),
    /// A generated projection table is forged, unordered, duplicated, or foreign.
    #[error("generated iced projection table is not an exact sealed proof")]
    ProjectionProof,
    /// A generated extension ABI family is missing, duplicated, stale, or foreign.
    #[error("generated iced extension ABI family is not an exact sealed proof")]
    ExtensionAbiProof,
    /// A generated operation endpoint is missing, duplicated, stale, or foreign.
    #[error("generated iced operation endpoint is not an exact sealed proof")]
    OperationEndpointProof,
    /// The generated profile cannot prove the sole Element replacement.
    #[error("generated iced Element runtime replacement is not exact")]
    ElementReplacement,
    /// The imported facade reconstruction or registration failed.
    #[error(transparent)]
    Import(#[from] rawr::package::import::runner::Error),
    /// The linked semantic facade could not be built.
    #[error(transparent)]
    Host(#[from] host::Error),
    /// The exact compiled generation could not bind its runtime registry.
    #[error(transparent)]
    Binding(#[from] rawr::reload::installation::Error),
    /// The bound application could not boot under retained iced semantics.
    #[error(transparent)]
    Application(#[from] crate::program::install::Error),
    /// The bound preview host could not boot under retained iced semantics.
    #[error("{0}")]
    Preview(Box<rawr::diagnostic::Report>),
    /// The requested command does not match the pre-admission lifecycle.
    #[error("generated iced lifecycle command does not match its bound mode")]
    Lifecycle,
    /// Previews and snapshots need an application, and a plain program has none.
    #[error("this program has no application, so it has no previews or snapshots. Its `main` does not end in `iced::run` or `iced::application`.")]
    PlainProgram,
    /// A live candidate failed or timed out before iced installation feedback.
    #[error("generated iced reload transaction failed: {0}")]
    Reload(Box<str>),
    /// The bounded facade plan could not be decoded.
    #[error(transparent)]
    Surface(#[from] rawr_import::surface::Error),
}

/// The child consumed its hot preparation, but only correlated feedback can
/// prove whether the iced owner left its current generation unchanged.
#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    /// Correlated Failed or Settled feedback proves no slot publication.
    #[error(transparent)]
    NotInstalled(Error),
    /// Missing, inconsistent, or uncorrelated acknowledgement is not rollback.
    #[error(transparent)]
    Uncertain(Error),
    /// A newer event on this owner's gate revoked the request; nothing installed.
    #[error(transparent)]
    Superseded(Error),
}

impl InstallError {
    /// Only this outcome permits a consumed-but-not-installed wire response.
    #[must_use]
    pub fn is_not_installed(&self) -> bool {
        matches!(self, Self::NotInstalled(_) | Self::Superseded(_))
    }

    /// Only a newer event on this owner's own gate, never a real failure.
    #[must_use]
    pub const fn is_superseded(&self) -> bool {
        matches!(self, Self::Superseded(_))
    }

    /// Borrow the definite rejection text before allocating its wire payload.
    /// An unexpected error family cannot become no-install wire evidence.
    #[must_use]
    pub fn not_installed_diagnostic(&self) -> Option<&str> {
        match self {
            Self::NotInstalled(Error::Reload(reason)) | Self::Superseded(Error::Reload(reason)) => {
                Some(reason)
            }
            _ => None,
        }
    }
}

impl Argument {
    /// Reuse one zero-based source type parameter.
    #[must_use]
    pub const fn source(index: u32) -> Self {
        Self::Source(index)
    }

    /// Infer a fresh target parameter at the checked use site.
    #[must_use]
    pub const fn fresh() -> Self {
        Self::Fresh
    }
}

impl AliasParameter {
    /// Construct one ordered alias normalization parameter.
    #[must_use]
    pub const fn new(kind: ParameterKind, alias: u32, target: u32) -> Self {
        Self {
            kind,
            alias,
            target,
        }
    }
}

impl OperationFormal {
    /// Construct one exact generated compiler formal.
    #[must_use]
    pub const fn new(owner: [u8; 32], index: u32, kind: ParameterKind) -> Self {
        Self { owner, index, kind }
    }
}

impl<'parameters> AliasTarget<'parameters> {
    /// Construct one generated alias row from its sealed proof fields.
    #[allow(
        clippy::too_many_arguments,
        reason = "every alias proof field is inseparable"
    )]
    #[must_use]
    pub const fn new(
        root: &'parameters str,
        path: &'parameters str,
        namespace: Namespace,
        export: [u8; 32],
        alias: [u8; 32],
        target: [u8; 32],
        parameters: &'parameters [AliasParameter],
        site: [u8; 32],
        selector: [u8; 32],
    ) -> Self {
        Self {
            schema: 2,
            root,
            path,
            namespace,
            export,
            alias,
            target,
            parameters,
            site,
            selector,
        }
    }

    /// Seal one test/provisional row while deriving both selectors.
    #[doc(hidden)]
    #[must_use]
    pub fn checked(
        root: &'parameters str,
        path: &'parameters str,
        namespace: Namespace,
        export: [u8; 32],
        alias: [u8; 32],
        target: [u8; 32],
        parameters: &'parameters [AliasParameter],
    ) -> Self {
        let mut proof = Self::new(
            root, path, namespace, export, alias, target, parameters, [0; 32], [0; 32],
        );
        proof.site = alias_site_selector(&proof);
        proof.selector = alias_target_selector(&proof);
        proof
    }

    /// Borrow the profile alias-target selector.
    #[must_use]
    pub const fn selector(&self) -> &[u8; 32] {
        &self.selector
    }
}

impl<'proof> ProjectionSourceSite<'proof> {
    /// Construct one generated source-site row from sealed proof fields.
    #[allow(
        clippy::too_many_arguments,
        reason = "every source proof field is inseparable"
    )]
    #[must_use]
    pub const fn new(
        root: &'proof str,
        path: &'proof str,
        namespace: Namespace,
        export: [u8; 32],
        alias: [u8; 32],
        source: [u8; 32],
        parameters: &'proof [AliasParameter],
        arguments: &'proof [SourceArgument],
        site: [u8; 32],
        widget_trait: [u8; 32],
        widget: [u8; 32],
        target: [u8; 32],
        selector: [u8; 32],
    ) -> Self {
        Self {
            schema: 3,
            root,
            path,
            namespace,
            export,
            alias,
            source,
            parameters,
            arguments,
            site,
            widget_trait,
            widget,
            target,
            selector,
        }
    }

    /// Borrow the canonical source-site selector.
    #[must_use]
    pub const fn selector(&self) -> &[u8; 32] {
        &self.selector
    }

    /// Seal one test/provisional source row while deriving both selectors.
    #[doc(hidden)]
    #[allow(
        clippy::too_many_arguments,
        reason = "every source proof field is inseparable"
    )]
    #[must_use]
    pub fn checked(
        root: &'proof str,
        path: &'proof str,
        namespace: Namespace,
        export: [u8; 32],
        alias: [u8; 32],
        source: [u8; 32],
        parameters: &'proof [AliasParameter],
        arguments: &'proof [SourceArgument],
        widget_trait: [u8; 32],
        widget: [u8; 32],
        target: [u8; 32],
    ) -> Self {
        let mut proof = Self::new(
            root,
            path,
            namespace,
            export,
            alias,
            source,
            parameters,
            arguments,
            [0; 32],
            widget_trait,
            widget,
            target,
            [0; 32],
        );
        proof.site = projection_source_site_selector(&proof);
        proof.selector = projection_source_selector(&proof);
        proof
    }
}

impl<'proof> ExtensionAbi<'proof> {
    /// Construct one generated ABI row from sealed profile evidence.
    #[allow(
        clippy::too_many_arguments,
        reason = "every extension ABI proof field is inseparable"
    )]
    #[must_use]
    pub const fn new(
        role: ExtensionAbiRole,
        root: &'proof str,
        path: &'proof str,
        namespace: Namespace,
        export: [u8; 32],
        definition: [u8; 32],
        shape: [u8; 32],
        site: [u8; 32],
        linked: [u8; 32],
        arguments: &'proof [ExtensionAbiArgument],
        selector: [u8; 32],
    ) -> Self {
        Self {
            schema: 2,
            role,
            root,
            path,
            namespace,
            export,
            definition,
            shape,
            site,
            linked,
            arguments,
            selector,
        }
    }

    /// Seal one test row by deriving its generic-site and profile selectors.
    #[doc(hidden)]
    #[allow(
        clippy::too_many_arguments,
        reason = "every extension ABI proof field is inseparable"
    )]
    #[must_use]
    pub fn checked(
        role: ExtensionAbiRole,
        root: &'proof str,
        path: &'proof str,
        namespace: Namespace,
        export: [u8; 32],
        definition: [u8; 32],
        shape: [u8; 32],
        arguments: &'proof [ExtensionAbiArgument],
    ) -> Self {
        let linked = extension_linked_fingerprint(role);
        let mut proof = Self::new(
            role, root, path, namespace, export, definition, shape, [0; 32], linked, arguments,
            [0; 32],
        );
        proof.site = extension_abi_site_selector(&proof);
        proof.selector = extension_abi_selector(&proof);
        proof
    }

    /// Borrow the exact closed role.
    pub const fn role(&self) -> ExtensionAbiRole {
        self.role
    }

    /// Borrow the exact public path.
    pub const fn path(&self) -> &str {
        self.path
    }

    /// Borrow the canonical profile ABI selector.
    pub const fn selector(&self) -> &[u8; 32] {
        &self.selector
    }
}

impl<'proof> OperationEndpoint<'proof> {
    /// Construct one generated operation endpoint from sealed proof fields.
    #[allow(
        clippy::too_many_arguments,
        reason = "every operation endpoint proof field is inseparable"
    )]
    #[must_use]
    pub const fn new(
        root: &'proof str,
        path: &'proof str,
        namespace: Namespace,
        export: [u8; 32],
        definition: [u8; 32],
        formals: &'proof [OperationFormal],
        shape: [u8; 32],
        site: [u8; 32],
        source: [u8; 32],
        projection: [u8; 32],
        arguments: &'proof [EndpointArgument],
        linked: [u8; 32],
        selector: [u8; 32],
    ) -> Self {
        Self {
            schema: 2,
            root,
            path,
            namespace,
            export,
            definition,
            formals,
            shape,
            site,
            source,
            projection,
            arguments,
            linked,
            selector,
        }
    }

    /// Seal one test row by deriving its generic-site, linked-host, and profile selectors.
    #[doc(hidden)]
    #[allow(
        clippy::too_many_arguments,
        reason = "every operation endpoint proof field is inseparable"
    )]
    #[must_use]
    pub fn checked(
        root: &'proof str,
        path: &'proof str,
        namespace: Namespace,
        export: [u8; 32],
        definition: [u8; 32],
        formals: &'proof [OperationFormal],
        shape: [u8; 32],
        source: [u8; 32],
        projection: [u8; 32],
        arguments: &'proof [EndpointArgument],
    ) -> Self {
        let mut proof = Self::new(
            root,
            path,
            namespace,
            export,
            definition,
            formals,
            shape,
            [0; 32],
            source,
            projection,
            arguments,
            super::evidence::operation_linked_fingerprint(),
            [0; 32],
        );
        proof.site = operation_site_selector(&proof);
        proof.selector = operation_endpoint_selector(&proof);
        proof
    }

    /// Borrow the profile-owned endpoint selector.
    #[must_use]
    pub const fn selector(&self) -> &[u8; 32] {
        &self.selector
    }
}

impl<'arguments> Projection<'arguments> {
    /// Construct one generated table row from already-sealed profile evidence.
    #[must_use]
    pub const fn new(
        source: [u8; 32],
        alias: [u8; 32],
        selector: [u8; 32],
        arguments: &'arguments [Argument],
    ) -> Self {
        Self {
            schema: 3,
            source,
            alias,
            selector,
            arguments,
        }
    }

    /// Seal one table row from exact evidence while deriving its canonical selector.
    #[doc(hidden)]
    #[must_use]
    pub fn checked(source: [u8; 32], alias: [u8; 32], arguments: &'arguments [Argument]) -> Self {
        let mut projection = Self::new(source, alias, [0; 32], arguments);
        projection.selector = selector(&projection);
        projection
    }

    /// Borrow the canonical generated selector.
    #[must_use]
    pub const fn selector(&self) -> &[u8; 32] {
        &self.selector
    }
}

impl Compiler {
    /// Borrow the exact profile proof sealed into this generated owner.
    #[must_use]
    pub const fn profile_fingerprint(&self) -> &[u8; 32] {
        &self.profile
    }

    /// Borrow the exact compiler facades installed for this generation.
    #[must_use]
    pub const fn definitions(&self) -> &[host::facade::Definition] {
        &self.definitions
    }

    /// Borrow the non-authoritative application self-cycle proof.
    #[doc(hidden)]
    #[must_use]
    pub const fn application_cycle(&self) -> &host::facade::ProfileCycle {
        &self.application_cycle
    }

    /// Consume the profile proof bundle and retain its compiler definitions.
    #[must_use]
    pub fn into_definitions(self) -> Box<[host::facade::Definition]> {
        self.definitions
    }
}

impl BoundGeneration {
    /// Borrow the exact profile proof that authorized this host.
    #[must_use]
    pub const fn profile_fingerprint(&self) -> &[u8; 32] {
        &self.profile
    }

    /// Borrow the bound host for product gates that must not open a window.
    #[must_use]
    pub const fn host(&self) -> Option<&crate::program::Host> {
        match &self.host {
            Generation::Application(host) => Some(host),
            Generation::Main(_) | Generation::Preview(_) | Generation::Snapshot { .. } => None,
        }
    }

    /// Start from the State a previous runner process exported;
    /// `false` keeps the booted State.
    pub fn seed_state(&mut self, state: &[u8]) -> bool {
        match &mut self.host {
            Generation::Application(host) => host.seed_state(state),
            Generation::Preview(host) => host.seed_state(state),
            Generation::Main(_) | Generation::Snapshot { .. } => false,
        }
    }

    /// Establish child coverage before the parent performs final validation.
    pub fn observe(&mut self, bytes: &[u8]) -> Result<(), Error> {
        if self.observation.is_some()
            || !matches!(
                &self.host,
                Generation::Application(_) | Generation::Preview(_) | Generation::Main(_)
            )
        {
            return Err(Error::Lifecycle);
        }
        self.observation = Some(Startup::attach(self.snapshot.inputs(), bytes)?);
        Ok(())
    }

    /// Split one live generation into the child-main native owner and its sole
    /// authenticated protocol-side reload transport.
    pub fn start(self, engine: rawr::Shared<rawr::Engine>) -> Result<(Running, Transport), Error> {
        if !matches!(
            &self.host,
            Generation::Application(_) | Generation::Preview(_) | Generation::Main(_)
        ) {
            return Err(Error::Lifecycle);
        }
        let (gate, publication, observer, native_baseline, request) = match self.observation {
            Some(startup) => {
                self.snapshot
                    .revalidate()
                    .map_err(|error| Error::Reload(error.to_string().into_boxed_str()))?;
                (
                    startup.gate,
                    startup.publication,
                    Some(startup.observer),
                    Some(startup.native_baseline),
                    Some(startup.request),
                )
            }
            None => (
                Arc::new(rawr::reload::scheduler::Gate::new(Duration::ZERO)),
                Arc::new(rawr::reload::scheduler::publication::Slot::new(
                    self.snapshot.inputs().clone(),
                )),
                None,
                None,
                None,
            ),
        };
        let (transport, service) = crate::reload::channel_generated(
            Arc::clone(&gate),
            Arc::clone(&publication),
            engine,
            self.runtime,
        );
        let exporter = transport.exporter();
        let (sender, feedback, retry) = transport.into_parts();
        let live = running_with_service(self.host, service);
        let live = match request {
            Some(request) => admit_startup(&gate, request, live)?,
            None => live,
        };
        let control = observer.as_ref().map(rawr_watch::Adapter::control);
        Ok((
            Running {
                live,
                _observer: control,
            },
            Transport {
                gate,
                publication,
                sender,
                feedback,
                retry,
                feedback_uncertain: Cell::new(false),
                observer,
                native_baseline,
                exporter,
            },
        ))
    }

    /// Execute the one-shot bound snapshot owner.
    pub fn snapshot(self) -> Result<crate::snapshot::Outcome, Error> {
        match self.host {
            Generation::Snapshot { suite, mode } => suite
                .execute(mode)
                .map_err(|error| Error::Reload(error.to_string().into_boxed_str())),
            Generation::Application(_) | Generation::Preview(_) | Generation::Main(_) => {
                Err(Error::Lifecycle)
            }
        }
    }
}

// As `Generation`: one per run.
#[allow(clippy::large_enum_variant)]
enum LiveGeneration {
    Application(crate::program::Host, crate::reload::Service),
    Preview(crate::control::Host, crate::reload::Service),
    Main(rawr::reload::installation::Bootstrap),
}

fn running_with_service(generation: Generation, service: crate::reload::Service) -> LiveGeneration {
    match generation {
        Generation::Application(host) => LiveGeneration::Application(host, service),
        Generation::Preview(host) => LiveGeneration::Preview(host, service),
        Generation::Main(bootstrap) => LiveGeneration::Main(bootstrap),
        Generation::Snapshot { .. } => unreachable!("snapshot generations never become live"),
    }
}

impl Running {
    /// Run the sole bound child lifecycle on the calling main thread.
    pub fn run(self) -> iced::Result {
        let _observer = self._observer;
        match self.live {
            LiveGeneration::Application(host, service) => host.run(service),
            LiveGeneration::Preview(host, service) => host.run_live(service),
            LiveGeneration::Main(bootstrap) => run_main(&bootstrap),
        }
    }
}

/// Run a plain program's `main` once and end the process with its outcome,
/// as for a Rust program. A `main` that returns `Err` prints
/// `Error: {err:?}` on stderr and exits with 1. A panic exits with 101.
fn run_main(bootstrap: &rawr::reload::installation::Bootstrap) -> ! {
    use std::io::Write as _;
    let code = match bootstrap.invoke_main() {
        None => 0,
        Some(Ok(value)) => match value.try_cast::<Result<rawr::Dynamic, rawr::Dynamic>>() {
            Some(Err(error)) => {
                eprintln!("Error: {error:?}");
                1
            }
            _ => 0,
        },
        // A run-time failure is a panic: Rust exits with 101.
        Some(Err(error)) => {
            eprintln!("error: {error}");
            101
        }
    };
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    std::process::exit(code)
}

impl Transport {
    /// The live State as carried bytes for the runner that replaces this one;
    /// `None` when it holds anything but plain script data.
    #[must_use]
    pub fn export_state(&self) -> Option<Vec<u8>> {
        self.exporter.export(EXPORT_TIMEOUT)
    }

    /// Drain one or more coalesced native refresh requests without starting work.
    #[must_use]
    pub fn take_refresh(&self) -> bool {
        self.retry.take()
    }

    /// Capture one original request before preparing an off-live candidate.
    pub fn begin_install(&self) -> Result<PendingInstall, InstallError> {
        check_install_feedback(&self.feedback, &self.feedback_uncertain)?;
        PendingInstall::begin(&self.gate)
    }

    /// Inspect native invalidation only after capturing the original request.
    /// This is eligibility, never publication authority: every later event
    /// still revokes this exact request at the existing UI admission gate.
    pub fn resident_eligible(&self, pending: &PendingInstall) -> bool {
        Arc::ptr_eq(&self.gate, &pending.gate)
            && self
                .observer
                .as_ref()
                .zip(self.native_baseline.as_ref())
                .is_some_and(|(observer, baseline)| observer.resident_eligible(baseline))
    }

    /// Borrow the original observed request around resident compilation.
    /// This neither refreshes its revision nor grants installation authority.
    pub fn request_is_current(&self, pending: &PendingInstall) -> Result<bool, InstallError> {
        check_install_feedback(&self.feedback, &self.feedback_uncertain)?;
        if !Arc::ptr_eq(&self.gate, &pending.gate) || pending.observation.is_none() {
            return Err(InstallError::Uncertain(Error::Reload(
                "resident compilation requires this owner's observed request".into(),
            )));
        }
        Ok(self.gate.request_is_current(&pending.request))
    }

    /// Register the full input union on the original request before parent
    /// validation. This neither refreshes its revision nor proves freshness.
    pub fn observe_install(
        &self,
        pending: &mut PendingInstall,
        bytes: &[u8],
    ) -> Result<(), InstallError> {
        check_install_feedback(&self.feedback, &self.feedback_uncertain)?;
        pending.observe(&self.gate, &self.publication, bytes)?;
        if let Some(observer) = &self.observer
            && let Some(observation) = &pending.observation
            && let Err(error) = observer.control().prepare(observation.inputs())
        {
            self.gate.suspend();
            drop(pending.observation.take());
            let _ = self.reconcile_observation();
            return Err(InstallError::NotInstalled(Error::Reload(
                error.to_string().into_boxed_str(),
            )));
        }
        Ok(())
    }

    /// Cancel off-live state and reconcile roots outside the admission gate.
    pub fn cancel_install(&self, pending: PendingInstall) -> Result<(), Error> {
        drop(pending);
        self.reconcile_observation()
    }

    fn reconcile_observation(&self) -> Result<(), Error> {
        if let Some(observer) = &self.observer
            && let Some(error) = observer.control().reconcile().into_iter().next()
        {
            self.gate.suspend();
            return Err(Error::Reload(error.to_string().into_boxed_str()));
        }
        Ok(())
    }

    /// Consume retained preparation authority. Success is returned only after
    /// the iced owner commits its state and input slot; this never refreshes it.
    pub fn install(
        &self,
        pending: PendingInstall,
        candidate: rawr::reload::Candidate,
    ) -> Result<(), InstallError> {
        let result = self.install_pending(pending, candidate);
        self.reconcile_observation()
            .map_err(InstallError::Uncertain)?;
        result
    }

    fn install_pending(
        &self,
        mut pending: PendingInstall,
        candidate: rawr::reload::Candidate,
    ) -> Result<(), InstallError> {
        // This receiver has one serial protocol caller. Report/exit consume
        // their own replies before the next install; after any lost reply the
        // channel can never prove which operation a later feedback belongs to.
        check_install_feedback(&self.feedback, &self.feedback_uncertain)?;
        // Stay awake until the owner confirms the installed generation.
        let _activity = pending.activity.take();
        let snapshot = rawr::reload::source::Snapshot::from_graph(candidate.source());
        let candidate = match pending.observation.take() {
            Some(observation) => {
                let inputs = observation.bind(snapshot).map_err(|error| {
                    InstallError::NotInstalled(Error::Reload(error.to_string().into_boxed_str()))
                })?;
                pending.complete(&self.gate, (candidate, inputs))?
            }
            None if self.observer.is_some() => {
                return Err(InstallError::NotInstalled(Error::Reload(
                    "observed owner requires staged reload inputs".into(),
                )))
            }
            None => pending
                .complete(&self.gate, (candidate, snapshot))?
                .stage_inputs(&self.publication),
        };
        let completion = crate::reload::Completion::new(candidate);
        let result = match self.sender.send(&self.gate, completion) {
            Ok(true) => install_feedback(self.feedback.recv_timeout(FEEDBACK_TIMEOUT)),
            Ok(false) => Err(InstallError::Superseded(Error::Reload(
                "reload candidate became obsolete".into(),
            ))),
            Err(completion) => {
                drop(completion);
                Err(InstallError::Uncertain(Error::Reload(
                    "reload owner closed before installation".into(),
                )))
            }
        };
        if matches!(&result, Err(InstallError::Uncertain(_))) {
            self.feedback_uncertain.set(true);
        }
        result
    }

    /// Display one parent-side compile failure without changing the live
    /// generation or its exact publication.
    pub fn report(&self, message: &str) -> Result<(), Error> {
        let now = Instant::now();
        let request = self
            .gate
            .observe_and_start(now)
            .map_err(|error| Error::Reload(error.to_string().into_boxed_str()))?
            .ok_or_else(|| Error::Reload("reload report did not become ready".into()))?;
        let report = rawr::diagnostic::Report::error(
            rawr::diagnostic::code::FRAMEWORK_NATIVE,
            message.to_owned(),
        )
        .finish();
        let completion = crate::reload::Completion::failed(request.complete(report));
        self.send_report(completion)
    }

    fn send_report(&self, completion: crate::reload::Completion) -> Result<(), Error> {
        match self.sender.send(&self.gate, completion) {
            Ok(true) => self.feedback(ExpectedFeedback::Failed),
            // A newer edit revoked this diagnostic before submission. There
            // is no feedback to consume and the last-good owner stays live.
            Ok(false) => Ok(()),
            Err(completion) => {
                drop(completion);
                Err(Error::Reload("reload owner closed before reporting".into()))
            }
        }
    }

    /// Ask iced to exit and wait for its explicit owner acknowledgement.
    pub fn exit(&self) -> Result<(), Error> {
        self.sender.exit().map_err(|completion| {
            drop(completion);
            Error::Reload("reload owner closed before exit".into())
        })?;
        self.feedback(ExpectedFeedback::Exiting)
    }

    /// Retire the UI owner and transfer its final coalesced refresh request.
    /// The exit acknowledgement closes UI acceptance before the flag is read.
    pub fn retire(&self) -> Result<bool, Error> {
        self.exit()?;
        Ok(self.retry.take())
    }

    fn feedback(&self, expected: ExpectedFeedback) -> Result<(), Error> {
        let result = match (expected, self.feedback.recv_timeout(FEEDBACK_TIMEOUT)) {
            (ExpectedFeedback::Failed, Ok(crate::reload::Feedback::Failed(_)))
            // Revocation after submission is acknowledged by failed_candidate
            // as Settled. This consumes the report's one correlated reply.
            | (ExpectedFeedback::Failed, Ok(crate::reload::Feedback::Settled))
            | (ExpectedFeedback::Exiting, Ok(crate::reload::Feedback::Exiting)) => Ok(()),
            (_, Ok(crate::reload::Feedback::Installed(retired))) => {
                retired.release();
                Err(Error::Reload(
                    "unexpected generated-owner installation".into(),
                ))
            }
            (
                _,
                Ok(
                    crate::reload::Feedback::Failed(_)
                    | crate::reload::Feedback::Settled
                    | crate::reload::Feedback::Exiting,
                ),
            ) => Err(Error::Reload("unexpected generated-owner feedback".into())),
            (_, Err(error)) => Err(Error::Reload(error.to_string().into_boxed_str())),
        };
        if result.is_err() {
            self.feedback_uncertain.set(true);
        }
        result
    }
}

/// Called only for the single submitted install on an unpoisoned serial
/// transport. Application/preview hosts send Failed/Settled only before slot
/// publication; a successful slot change sends Installed. Lost feedback must
/// not be translated into a no-install promise.
fn install_feedback(
    feedback: Result<crate::reload::Feedback, std::sync::mpsc::RecvTimeoutError>,
) -> Result<(), InstallError> {
    match feedback {
        Ok(crate::reload::Feedback::Installed(retired)) => {
            retired.release();
            Ok(())
        }
        Ok(crate::reload::Feedback::Failed(report)) => Err(InstallError::NotInstalled(
            Error::Reload(report.to_string().into_boxed_str()),
        )),
        Ok(crate::reload::Feedback::Settled) => Err(InstallError::Superseded(Error::Reload(
            "reload candidate settled without installation".into(),
        ))),
        Ok(crate::reload::Feedback::Exiting) => Err(InstallError::Uncertain(Error::Reload(
            "unexpected generated-owner exit during installation".into(),
        ))),
        Err(error) => Err(InstallError::Uncertain(Error::Reload(
            error.to_string().into_boxed_str(),
        ))),
    }
}

fn check_install_feedback(
    feedback: &std::sync::mpsc::Receiver<crate::reload::Feedback>,
    uncertain: &Cell<bool>,
) -> Result<(), InstallError> {
    if uncertain.get() {
        return Err(InstallError::Uncertain(Error::Reload(
            "generated-owner feedback correlation was lost".into(),
        )));
    }
    match feedback.try_recv() {
        Err(std::sync::mpsc::TryRecvError::Empty) => Ok(()),
        pending => {
            uncertain.set(true);
            if let Ok(crate::reload::Feedback::Installed(retired)) = pending {
                retired.release();
            }
            Err(InstallError::Uncertain(Error::Reload(
                "unexpected feedback before generated-owner installation".into(),
            )))
        }
    }
}

#[derive(Clone, Copy)]
enum ExpectedFeedback {
    Failed,
    Exiting,
}

/// Install the exact generated iced facade with only genuine semantic anchors replaced.
#[allow(clippy::too_many_arguments)]
pub fn install_compiler(
    engine: &mut rawr::Engine,
    bytes: &[u8],
    profile: [u8; 32],
    rules: [u8; 32],
    aliases: &[AliasTarget<'_>],
    sources: &[ProjectionSourceSite<'_>],
    abis: &[ExtensionAbi<'_>],
    endpoints: &[OperationEndpoint<'_>],
    projections: &[Projection<'_>],
) -> Result<Compiler, Error> {
    install_compiler_accounted(
        engine,
        bytes,
        profile,
        rules,
        aliases,
        sources,
        abis,
        endpoints,
        projections,
        None,
    )
}

/// Reconstruct a generated compiler under its retained native metadata account.
#[doc(hidden)]
#[allow(clippy::too_many_arguments)]
pub fn install_compiler_retained(
    engine: &mut rawr::Engine,
    bytes: &[u8],
    profile: [u8; 32],
    rules: [u8; 32],
    aliases: &[AliasTarget<'_>],
    sources: &[ProjectionSourceSite<'_>],
    abis: &[ExtensionAbi<'_>],
    endpoints: &[OperationEndpoint<'_>],
    projections: &[Projection<'_>],
    native: &mut rawr::package::import::runner::account::Native,
) -> Result<Compiler, Error> {
    install_compiler_accounted(
        engine,
        bytes,
        profile,
        rules,
        aliases,
        sources,
        abis,
        endpoints,
        projections,
        Some(native),
    )
}

#[allow(clippy::too_many_arguments)]
fn install_compiler_accounted(
    engine: &mut rawr::Engine,
    bytes: &[u8],
    profile: [u8; 32],
    rules: [u8; 32],
    aliases: &[AliasTarget<'_>],
    sources: &[ProjectionSourceSite<'_>],
    abis: &[ExtensionAbi<'_>],
    endpoints: &[OperationEndpoint<'_>],
    projections: &[Projection<'_>],
    mut native: Option<&mut rawr::package::import::runner::account::Native>,
) -> Result<Compiler, Error> {
    if profile == *rawr_import::profile::Proof::neutral().fingerprint() {
        return Err(Error::NeutralProfile);
    }
    let limits = rawr_import::cargo::Limits::standard();
    let plan = facade::Plan::decode(bytes, &limits)?;
    let owners = plan
        .facades()
        .filter(|facade| facade.root() == ROOT)
        .count();
    if owners != 1 {
        return Err(Error::IcedOwners(owners));
    }
    validate_projections(&plan, aliases, sources, projections)?;
    validate_extension_abis(&plan, abis)?;
    validate_operation_endpoints(&plan, sources, endpoints, projections)?;
    let semantic = crate::facade()?;
    let [endpoint] = endpoints else {
        return Err(Error::OperationEndpointProof);
    };
    let compatibility = compatibility_seal(&semantic, bytes, profile, rules, endpoint);
    let decoration = Decoration::new(
        semantic,
        compatibility.exact,
        aliases,
        sources,
        abis,
        endpoints,
        projections,
        &plan,
    );
    let definitions = match native.as_deref_mut() {
        Some(native) => rawr::package::import::runner::install_decoded_profile_retained(
            engine,
            &plan,
            &decoration,
            native,
        )?,
        None => rawr::package::import::runner::install_decoded_profile(engine, &plan, &decoration)?,
    };
    let mut definitions = definitions.into_vec();
    let iced = definitions
        .iter()
        .find(|definition| definition.root() == ROOT)
        .ok_or(Error::IcedOwners(0))?;
    decoration.extension_exports(iced)?;
    let extension = crate::extension_profile(iced, compatibility.exact)?;
    let application_cycle =
        host::facade::ProfileCycle::transform(iced, compatibility.exact, compatibility.semantic)?;
    match native {
        Some(native) => rawr::package::import::runner::install_profile_facade_retained(
            engine,
            extension.clone(),
            native,
        )?,
        None => {
            engine.register_host_facade(extension.clone())?;
        }
    }
    definitions
        .try_reserve_exact(1)
        .map_err(|_| host::Error::Allocation)?;
    definitions.push(extension);
    Ok(Compiler {
        profile,
        definitions: definitions.into_boxed_slice(),
        application_cycle,
    })
}

#[derive(Clone, Copy)]
struct CompatibilitySeal {
    exact: [u8; 32],
    semantic: [u8; 32],
}

fn compatibility_seal(
    semantic: &host::facade::Definition,
    plan: &[u8],
    profile: [u8; 32],
    rules: [u8; 32],
    endpoint: &OperationEndpoint<'_>,
) -> CompatibilitySeal {
    let mut digest = Sha256::new();
    compatibility_preimage(&mut digest, semantic, plan, endpoint);
    digest.update(profile);
    let exact = digest.finalize().into();

    let mut digest = Sha256::new();
    digest.update(COMPATIBILITY_SEMANTIC_POLICY);
    compatibility_preimage(&mut digest, semantic, plan, endpoint);
    digest.update(b"rawr.framework.iced");
    digest.update(12_u32.to_le_bytes());
    digest.update(rules);
    CompatibilitySeal {
        exact,
        semantic: digest.finalize().into(),
    }
}

fn compatibility_preimage(
    digest: &mut Sha256,
    semantic: &host::facade::Definition,
    plan: &[u8],
    endpoint: &OperationEndpoint<'_>,
) {
    digest.update(COMPATIBILITY_POLICY);
    digest.update(COMPATIBILITY_RUNTIME_BINDING);
    digest.update(TEXT_ENDPOINT_DISPOSITION);
    digest.update(endpoint.selector);
    digest.update(semantic.abi().get().to_le_bytes());
    digest.update(semantic.fingerprint().compatibility_digest());
    framed_bytes(digest, plan);
}

/// Install compiler semantics directly from one exact classified profile contract.
#[doc(hidden)]
pub fn install_compiler_contract(
    engine: &mut rawr::Engine,
    bytes: &[u8],
    profile: [u8; 32],
    contract: &rawr_import::profile::Contract,
) -> Result<Compiler, Error> {
    with_contract_rows(
        contract,
        |aliases, sources, abis, endpoints, projections| {
            install_compiler(
                engine,
                bytes,
                profile,
                *contract.rules_digest(),
                aliases,
                sources,
                abis,
                endpoints,
                projections,
            )
        },
    )
}

fn with_contract_rows<T>(
    contract: &rawr_import::profile::Contract,
    action: impl FnOnce(
        &[AliasTarget<'_>],
        &[ProjectionSourceSite<'_>],
        &[ExtensionAbi<'_>],
        &[OperationEndpoint<'_>],
        &[Projection<'_>],
    ) -> Result<T, Error>,
) -> Result<T, Error> {
    let mut parameter_rows = Vec::new();
    parameter_rows
        .try_reserve_exact(contract.alias_targets().len())
        .map_err(|_| host::Error::Allocation)?;
    for alias in contract.alias_targets() {
        let mut parameters = Vec::new();
        parameters
            .try_reserve_exact(alias.parameters().len())
            .map_err(|_| host::Error::Allocation)?;
        parameters.extend(alias.parameters().iter().map(|parameter| {
            AliasParameter::new(
                match parameter.kind() {
                    facade::ParameterKind::Lifetime => ParameterKind::Lifetime,
                    facade::ParameterKind::Type => ParameterKind::Type,
                    facade::ParameterKind::Const => ParameterKind::Const,
                },
                parameter.alias_index(),
                parameter.target_index(),
            )
        }));
        parameter_rows.push(parameters.into_boxed_slice());
    }
    let mut aliases = Vec::new();
    aliases
        .try_reserve_exact(contract.alias_targets().len())
        .map_err(|_| host::Error::Allocation)?;
    for (alias, parameters) in contract.alias_targets().zip(&parameter_rows) {
        aliases.push(AliasTarget::checked(
            alias.root(),
            alias.path(),
            match alias.namespace() {
                facade::Namespace::Type => Namespace::Type,
                facade::Namespace::Value => Namespace::Value,
                facade::Namespace::Macro => Namespace::Macro,
            },
            *alias.export_identity(),
            *alias.alias_identity(),
            *alias.target_identity(),
            parameters,
        ));
    }
    let mut source_parameter_rows = Vec::new();
    let mut source_argument_rows = Vec::new();
    source_parameter_rows
        .try_reserve_exact(contract.projection_sources().len())
        .map_err(|_| host::Error::Allocation)?;
    source_argument_rows
        .try_reserve_exact(contract.projection_sources().len())
        .map_err(|_| host::Error::Allocation)?;
    for source in contract.projection_sources() {
        let mut parameters = Vec::new();
        parameters
            .try_reserve_exact(source.parameters().len())
            .map_err(|_| host::Error::Allocation)?;
        parameters.extend(source.parameters().iter().map(|parameter| {
            AliasParameter::new(
                match parameter.kind() {
                    facade::ParameterKind::Lifetime => ParameterKind::Lifetime,
                    facade::ParameterKind::Type => ParameterKind::Type,
                    facade::ParameterKind::Const => ParameterKind::Const,
                },
                parameter.alias_index(),
                parameter.target_index(),
            )
        }));
        source_parameter_rows.push(parameters.into_boxed_slice());
        let mut relations = Vec::new();
        relations
            .try_reserve_exact(source.arguments().len())
            .map_err(|_| host::Error::Allocation)?;
        relations.extend(source.arguments().iter().map(|argument| match argument {
            rawr_import::profile::contract::SourceArgument::Region { alias, source } => {
                SourceArgument::Region {
                    alias: *alias,
                    source: *source,
                }
            }
            rawr_import::profile::contract::SourceArgument::Parameter {
                kind,
                alias,
                source,
            } => SourceArgument::Parameter {
                kind: match kind {
                    facade::ParameterKind::Lifetime => ParameterKind::Lifetime,
                    facade::ParameterKind::Type => ParameterKind::Type,
                    facade::ParameterKind::Const => ParameterKind::Const,
                },
                alias: *alias,
                source: *source,
            },
        }));
        source_argument_rows.push(relations.into_boxed_slice());
    }
    let mut sources = Vec::new();
    sources
        .try_reserve_exact(contract.projection_sources().len())
        .map_err(|_| host::Error::Allocation)?;
    for ((source, parameters), arguments) in contract
        .projection_sources()
        .zip(&source_parameter_rows)
        .zip(&source_argument_rows)
    {
        sources.push(ProjectionSourceSite::new(
            source.root(),
            source.path(),
            Namespace::Type,
            *source.export_identity(),
            *source.alias_identity(),
            *source.source_identity(),
            parameters,
            arguments,
            *source.site_selector(),
            *source.widget_trait_evidence(),
            *source.widget_evidence(),
            *source.target_selector(),
            *source.selector(),
        ));
    }
    let mut arguments = Vec::new();
    arguments
        .try_reserve_exact(contract.projections().len())
        .map_err(|_| host::Error::Allocation)?;
    for projection in contract.projections() {
        let mut row = Vec::new();
        row.try_reserve_exact(projection.arguments().len())
            .map_err(|_| host::Error::Allocation)?;
        row.extend(projection.arguments().iter().map(|argument| {
            argument
                .source_index()
                .map_or(Argument::Fresh, Argument::Source)
        }));
        arguments.push(row.into_boxed_slice());
    }
    let mut projections = Vec::new();
    projections
        .try_reserve_exact(contract.projections().len())
        .map_err(|_| host::Error::Allocation)?;
    for (projection, arguments) in contract.projections().zip(&arguments) {
        projections.push(Projection::checked(
            *projection.source_selector(),
            *projection.alias_selector(),
            arguments,
        ));
    }
    let mut abi_argument_rows = Vec::new();
    abi_argument_rows
        .try_reserve_exact(contract.extension_abis().len())
        .map_err(|_| host::Error::Allocation)?;
    for abi in contract.extension_abis() {
        let mut row = Vec::new();
        row.try_reserve_exact(abi.arguments().len())
            .map_err(|_| host::Error::Allocation)?;
        row.extend(abi.arguments().iter().map(|argument| match argument {
            rawr_import::profile::contract::AbiArgument::DefaultF32 { parameter } => {
                ExtensionAbiArgument::DefaultF32 {
                    parameter: *parameter,
                }
            }
        }));
        abi_argument_rows.push(row.into_boxed_slice());
    }
    let mut abis = Vec::new();
    abis.try_reserve_exact(contract.extension_abis().len())
        .map_err(|_| host::Error::Allocation)?;
    for (abi, arguments) in contract.extension_abis().zip(&abi_argument_rows) {
        let role = match abi.role() {
            "Color" => ExtensionAbiRole::Color,
            "Length" => ExtensionAbiRole::Length,
            "Rectangle" => ExtensionAbiRole::Rectangle,
            "Vector" => ExtensionAbiRole::Vector,
            _ => return Err(Error::ExtensionAbiProof),
        };
        abis.push(ExtensionAbi::new(
            role,
            abi.root(),
            abi.path(),
            Namespace::Type,
            *abi.export_identity(),
            *abi.definition_identity(),
            *abi.shape_digest(),
            *abi.site_selector(),
            *abi.linked_fingerprint(),
            arguments,
            *abi.selector(),
        ));
    }
    let mut endpoint_formals = Vec::new();
    let mut endpoint_arguments = Vec::new();
    endpoint_formals
        .try_reserve_exact(contract.operation_endpoints().len())
        .map_err(|_| host::Error::Allocation)?;
    endpoint_arguments
        .try_reserve_exact(contract.operation_endpoints().len())
        .map_err(|_| host::Error::Allocation)?;
    for endpoint in contract.operation_endpoints() {
        let mut formals = Vec::new();
        formals
            .try_reserve_exact(endpoint.site().formals().len())
            .map_err(|_| host::Error::Allocation)?;
        formals.extend(endpoint.site().formals().iter().map(|formal| {
            OperationFormal::new(
                *formal.owner(),
                formal.index(),
                match formal.kind() {
                    facade::ParameterKind::Lifetime => ParameterKind::Lifetime,
                    facade::ParameterKind::Type => ParameterKind::Type,
                    facade::ParameterKind::Const => ParameterKind::Const,
                },
            )
        }));
        endpoint_formals.push(formals.into_boxed_slice());
        let mut arguments = Vec::new();
        arguments
            .try_reserve_exact(endpoint.arguments().len())
            .map_err(|_| host::Error::Allocation)?;
        arguments.extend(endpoint.arguments().iter().map(|argument| match argument {
            rawr_import::profile::contract::EndpointArgument::Region { parameter } => {
                EndpointArgument::Region {
                    parameter: *parameter,
                }
            }
            rawr_import::profile::contract::EndpointArgument::Definition {
                parameter,
                definition,
            } => EndpointArgument::Definition {
                parameter: *parameter,
                definition: *definition.digest(),
            },
            rawr_import::profile::contract::EndpointArgument::OwnedString {
                parameter,
                lifetime,
            } => EndpointArgument::OwnedString {
                parameter: *parameter,
                lifetime: *lifetime,
            },
        }));
        endpoint_arguments.push(arguments.into_boxed_slice());
    }
    let mut endpoints = Vec::new();
    endpoints
        .try_reserve_exact(contract.operation_endpoints().len())
        .map_err(|_| host::Error::Allocation)?;
    for ((endpoint, formals), arguments) in contract
        .operation_endpoints()
        .zip(&endpoint_formals)
        .zip(&endpoint_arguments)
    {
        let site = endpoint.site();
        endpoints.push(OperationEndpoint::new(
            site.root(),
            site.path(),
            match site.namespace() {
                facade::Namespace::Type => Namespace::Type,
                facade::Namespace::Value => Namespace::Value,
                facade::Namespace::Macro => Namespace::Macro,
            },
            *site.export_identity(),
            *site.definition_identity(),
            formals,
            *site.shape_digest(),
            *site.selector(),
            *endpoint.source_selector(),
            *endpoint.projection_selector(),
            arguments,
            *endpoint.linked(),
            *endpoint.selector(),
        ));
    }
    action(&aliases, &sources, &abis, &endpoints, &projections)
}

/// Add retained iced lifecycle bindings to generated ordinary runtime builders.
pub fn bind_runtime(
    definitions: &[host::facade::Definition],
    builders: &mut [host::runtime::Builder],
    projections: &[Projection<'_>],
) -> Result<(), Error> {
    let replacement = ElementReplacement::transform(definitions, projections)?;
    let iced = definitions
        .iter()
        .position(|definition| definition.root() == ROOT)
        .ok_or(Error::IcedOwners(0))?;
    let semantic = definitions
        .iter()
        .position(|definition| definition.root() == "rawr_iced")
        .ok_or(Error::IcedOwners(0))?;
    let (iced, semantic) = two_mut(builders, iced, semantic).ok_or(Error::IcedOwners(0))?;
    crate::widget::bind_profile(iced, semantic, replacement);
    crate::subscription::bind(iced, semantic);
    crate::task::bind(iced, semantic);
    crate::backend::bind(iced);
    crate::theme::bind(iced, semantic);
    Ok(())
}

impl ElementReplacement {
    fn transform(
        definitions: &[host::facade::Definition],
        projections: &[Projection<'_>],
    ) -> Result<Self, Error> {
        let [projection] = projections else {
            return Err(Error::ElementReplacement);
        };
        if projection.schema != 3
            || projection.selector != selector(projection)
            || !matches!(projection.arguments, [Argument::Fresh])
        {
            return Err(Error::ElementReplacement);
        }

        let mut owners = definitions
            .iter()
            .filter(|definition| definition.root() == ROOT);
        let Some(iced) = owners.next() else {
            return Err(Error::ElementReplacement);
        };
        if owners.next().is_some() {
            return Err(Error::ElementReplacement);
        }

        let mut elements = iced
            .types()
            .filter(|definition| definition.path() == "iced::Element");
        let Some(element) = elements.next() else {
            return Err(Error::ElementReplacement);
        };
        if elements.next().is_some() || element.parameters().len() != 1 {
            return Err(Error::ElementReplacement);
        }

        let exact_text_projection = iced.types().any(|definition| {
            definition.path() == "iced::widget::Text"
                && definition.parameters().is_empty()
                && matches!(
                    definition.projections(),
                    [projection]
                        if projection.target() == "iced::Element"
                            && projection.arguments() == [host::type_def::Argument::fresh()]
                )
        });
        exact_text_projection
            .then_some(Self(()))
            .ok_or(Error::ElementReplacement)
    }
}

/// Bind one exact compiled snapshot generation to its sole runtime and iced host.
pub fn bind_generation(
    candidate: rawr::reload::Candidate,
    engine: &rawr::Shared<rawr::Engine>,
    runtime: host::runtime::Registry,
    profile: [u8; 32],
    mode: Mode,
    root: PathBuf,
) -> Result<BoundGeneration, Error> {
    if profile == *rawr_import::profile::Proof::neutral().fingerprint() {
        return Err(Error::NeutralProfile);
    }
    let snapshot = rawr::reload::source::Snapshot::from_graph(candidate.source());
    let source = candidate.source().entry_source().diagnostic().clone();
    let bootstrap = candidate.bind(engine, runtime.next_generation())?;
    let (plain, previews) = {
        let preparation = bootstrap.preparation();
        let catalog = preparation.catalog();
        let application = catalog.application().is_some();
        (!application, application && !catalog.previews().is_empty())
    };
    let host = match mode {
        // A plain program checks by binding, and runs its `main` once.
        Mode::Check | Mode::Run if plain => Generation::Main(bootstrap),
        Mode::Preview | Mode::SnapshotCheck | Mode::SnapshotUpdate if plain => {
            return Err(Error::PlainProgram);
        }
        // `rawr run` of an app that declares previews opens the app under the
        // preview footer so any preview is one pick away.
        Mode::Run if previews => Generation::Preview(
            crate::control::Host::bootstrap(bootstrap, source)
                .map_err(|report| Error::Preview(Box::new(report)))?
                .into_run(),
        ),
        Mode::Check | Mode::Run => {
            Generation::Application(crate::program::Host::bootstrap(bootstrap, Some(source))?)
        }
        Mode::Preview => Generation::Preview(
            crate::control::Host::bootstrap(bootstrap, source)
                .map_err(|report| Error::Preview(Box::new(report)))?
                .into_preview(&root),
        ),
        Mode::SnapshotCheck | Mode::SnapshotUpdate => Generation::Snapshot {
            suite: crate::snapshot::Suite::from_installation(
                bootstrap.into_snapshot_catalog(),
                root,
            ),
            mode: match mode {
                Mode::SnapshotCheck => crate::snapshot::Mode::Check,
                Mode::SnapshotUpdate => crate::snapshot::Mode::Update,
                Mode::Check | Mode::Run | Mode::Preview => unreachable!(),
            },
        },
    };
    Ok(BoundGeneration {
        profile,
        host,
        runtime,
        snapshot,
        observation: None,
    })
}

fn two_mut<T>(values: &mut [T], left: usize, right: usize) -> Option<(&mut T, &mut T)> {
    if left == right || left >= values.len() || right >= values.len() {
        return None;
    }
    if left < right {
        let (before, after) = values.split_at_mut(right);
        Some((&mut before[left], &mut after[0]))
    } else {
        let (before, after) = values.split_at_mut(left);
        Some((&mut after[0], &mut before[right]))
    }
}

fn validate_projections(
    plan: &facade::Plan,
    aliases: &[AliasTarget<'_>],
    sources: &[ProjectionSourceSite<'_>],
    projections: &[Projection<'_>],
) -> Result<(), Error> {
    if aliases.is_empty() || sources.is_empty() || projections.is_empty() {
        return Err(Error::ProjectionProof);
    }
    let mut previous_alias = None;
    for alias in aliases {
        if alias.schema != 2
            || previous_alias.is_some_and(|selector| selector >= alias.selector)
            || alias.site != alias_site_selector(alias)
            || alias.selector != alias_target_selector(alias)
            || alias.namespace != Namespace::Type
            || alias.root.is_empty()
            || alias.path.is_empty()
            || alias.export == [0; 32]
            || alias.alias == [0; 32]
            || alias.target == [0; 32]
            || !alias
                .parameters
                .windows(2)
                .all(|pair| pair[0].target < pair[1].target)
        {
            return Err(Error::ProjectionProof);
        }
        let mut sites = plan.aliases().filter(|site| site.selector() == &alias.site);
        let Some(site) = sites.next() else {
            return Err(Error::ProjectionProof);
        };
        if sites.next().is_some()
            || site.root() != alias.root
            || site.path() != alias.path
            || site.namespace() != facade::Namespace::Type
            || site.export_identity() != &alias.export
            || site.alias_identity() != &alias.alias
            || site.target_identity() != &alias.target
            || site.parameters().len() != alias.parameters.len()
            || !site
                .parameters()
                .iter()
                .zip(alias.parameters)
                .all(|(left, right)| {
                    left.alias_index() == right.alias
                        && left.target_index() == right.target
                        && matches!(
                            (left.kind(), right.kind),
                            (facade::ParameterKind::Lifetime, ParameterKind::Lifetime)
                                | (facade::ParameterKind::Type, ParameterKind::Type)
                                | (facade::ParameterKind::Const, ParameterKind::Const)
                        )
                })
        {
            return Err(Error::ProjectionProof);
        }
        previous_alias = Some(alias.selector);
    }
    let mut previous_source = None;
    for source in sources {
        if source.schema != 3
            || previous_source.is_some_and(|selector| selector >= source.selector)
            || projection_source_selector(source) != source.selector
            || source.namespace != Namespace::Type
            || source.export == [0; 32]
            || source.alias == [0; 32]
            || source.source == [0; 32]
            || source.widget == [0; 32]
            || source.target == [0; 32]
            || source.arguments.len() != source.parameters.len()
            || source
                .arguments
                .iter()
                .filter(|argument| matches!(argument, SourceArgument::Region { .. }))
                .count()
                != 1
        {
            return Err(Error::ProjectionProof);
        }
        let mut sites = plan
            .projection_sources()
            .filter(|site| site.selector() == &source.site);
        let Some(site) = sites.next() else {
            return Err(Error::ProjectionProof);
        };
        if sites.next().is_some()
            || site.root() != source.root
            || site.path() != source.path
            || site.namespace() != facade::Namespace::Type
            || site.export_identity() != &source.export
            || site.alias_identity() != &source.alias
            || site.source_identity() != &source.source
            || site.shape() != facade::SourceShape::Struct
            || site.parameters().len() != source.parameters.len()
            || !site
                .parameters()
                .iter()
                .zip(source.parameters)
                .all(|(left, right)| {
                    left.alias_index() == right.alias
                        && left.target_index() == right.target
                        && parameter_kind_matches(left.kind(), right.kind)
                })
            || !source
                .parameters
                .iter()
                .zip(source.arguments)
                .all(|(parameter, argument)| match (parameter.kind, argument) {
                    (ParameterKind::Lifetime, SourceArgument::Region { alias, source }) => {
                        parameter.alias == *alias && parameter.target == *source
                    }
                    (
                        kind,
                        SourceArgument::Parameter {
                            kind: relation,
                            alias,
                            source,
                        },
                    ) => {
                        kind == *relation
                            && parameter.alias == *alias
                            && parameter.target == *source
                    }
                    _ => false,
                })
            || aliases
                .iter()
                .filter(|alias| alias.selector == source.target)
                .count()
                != 1
        {
            return Err(Error::ProjectionProof);
        }
        previous_source = Some(source.selector);
    }
    let mut previous = None;
    for projection in projections {
        if projection.schema != 3
            || previous.is_some_and(|selector| selector >= projection.selector)
            || selector(projection) != projection.selector
        {
            return Err(Error::ProjectionProof);
        }
        let sources = plan
            .projection_sources()
            .filter(|site| {
                sources.iter().any(|source| {
                    source.selector == projection.source && source.site == *site.selector()
                })
            })
            .count();
        let targets = aliases
            .iter()
            .filter(|alias| alias.selector == projection.alias)
            .count();
        if sources != 1 || targets != 1 {
            return Err(Error::ProjectionProof);
        }
        previous = Some(projection.selector);
    }
    Ok(())
}

fn validate_extension_abis(plan: &facade::Plan, abis: &[ExtensionAbi<'_>]) -> Result<(), Error> {
    if abis.len() != 4 {
        return Err(Error::ExtensionAbiProof);
    }
    let mut previous = None;
    for abi in abis {
        let expected_arguments: &[ExtensionAbiArgument] = match abi.role {
            ExtensionAbiRole::Color | ExtensionAbiRole::Length => &[],
            ExtensionAbiRole::Rectangle | ExtensionAbiRole::Vector => {
                &[ExtensionAbiArgument::DefaultF32 { parameter: 0 }]
            }
        };
        if abi.schema != 2
            || previous.is_some_and(|selector| selector >= abi.selector)
            || abi.root != ROOT
            || abi.path != extension_abi_name(abi.role)
            || abi.namespace != Namespace::Type
            || abi.export == [0; 32]
            || abi.definition == [0; 32]
            || abi.shape == [0; 32]
            || abi.linked != extension_linked_fingerprint(abi.role)
            || abi.arguments != expected_arguments
            || abi.site != extension_abi_site_selector(abi)
            || abi.selector != extension_abi_selector(abi)
        {
            return Err(Error::ExtensionAbiProof);
        }
        let mut sites = plan.abi_sites().filter(|site| site.selector() == &abi.site);
        let Some(site) = sites.next() else {
            return Err(Error::ExtensionAbiProof);
        };
        if sites.next().is_some()
            || site.root() != abi.root
            || site.path() != abi.path
            || site.namespace() != facade::Namespace::Type
            || site.export_identity() != &abi.export
            || site.definition_identity() != &abi.definition
            || site.shape_digest() != &abi.shape
        {
            return Err(Error::ExtensionAbiProof);
        }
        previous = Some(abi.selector);
    }
    for role in [
        ExtensionAbiRole::Color,
        ExtensionAbiRole::Length,
        ExtensionAbiRole::Rectangle,
        ExtensionAbiRole::Vector,
    ] {
        if abis.iter().filter(|abi| abi.role == role).count() != 1 {
            return Err(Error::ExtensionAbiProof);
        }
    }
    Ok(())
}

fn validate_operation_endpoints(
    plan: &facade::Plan,
    sources: &[ProjectionSourceSite<'_>],
    endpoints: &[OperationEndpoint<'_>],
    projections: &[Projection<'_>],
) -> Result<(), Error> {
    let [endpoint] = endpoints else {
        return Err(Error::OperationEndpointProof);
    };
    if endpoint.schema != 2
        || endpoint.root != ROOT
        || endpoint.path != "widget::text"
        || endpoint.namespace != Namespace::Value
        || endpoint.export == [0; 32]
        || endpoint.definition == [0; 32]
        || endpoint.shape == [0; 32]
        || endpoint.linked != super::evidence::operation_linked_fingerprint()
        || endpoint.site != operation_site_selector(endpoint)
        || endpoint.selector != operation_endpoint_selector(endpoint)
        || endpoint.formals.len() != 3
        || endpoint.arguments.len() != 3
        || !endpoint
            .formals
            .windows(2)
            .all(|pair| pair[0].index < pair[1].index)
        || !endpoint
            .formals
            .iter()
            .all(|formal| formal.owner == endpoint.definition)
        || !matches!(
            endpoint.formals,
            [
                OperationFormal {
                    index: 0,
                    kind: ParameterKind::Lifetime,
                    ..
                },
                OperationFormal {
                    index: 1,
                    kind: ParameterKind::Type,
                    ..
                },
                OperationFormal {
                    index: 2,
                    kind: ParameterKind::Type,
                    ..
                },
            ]
        )
        || !matches!(
            endpoint.arguments,
            [
                EndpointArgument::Region { parameter: 0 },
                EndpointArgument::Definition { parameter: 1, definition: theme },
                EndpointArgument::OwnedString { parameter: 2, lifetime: 0 },
            ] if *theme != [0; 32]
        )
        || sources
            .iter()
            .filter(|source| source.selector == endpoint.source)
            .count()
            != 1
        || projections
            .iter()
            .filter(|projection| {
                projection.selector == endpoint.projection && projection.source == endpoint.source
            })
            .count()
            != 1
    {
        return Err(Error::OperationEndpointProof);
    }
    let mut sites = plan
        .operation_sites()
        .filter(|site| site.selector() == &endpoint.site);
    let Some(site) = sites.next() else {
        return Err(Error::OperationEndpointProof);
    };
    if sites.next().is_some()
        || site.root() != endpoint.root
        || site.path() != endpoint.path
        || site.namespace() != facade::Namespace::Value
        || site.export_identity() != &endpoint.export
        || site.definition_identity() != &endpoint.definition
        || site.shape_digest() != &endpoint.shape
        || site.formals().len() != endpoint.formals.len()
        || !site
            .formals()
            .iter()
            .zip(endpoint.formals)
            .all(|(site, generated)| {
                site.owner() == &generated.owner
                    && site.index() == generated.index
                    && parameter_kind_matches(site.kind(), generated.kind)
            })
    {
        return Err(Error::OperationEndpointProof);
    }
    Ok(())
}

const fn extension_abi_name(role: ExtensionAbiRole) -> &'static str {
    match role {
        ExtensionAbiRole::Color => "Color",
        ExtensionAbiRole::Length => "Length",
        ExtensionAbiRole::Rectangle => "Rectangle",
        ExtensionAbiRole::Vector => "Vector",
    }
}

fn extension_linked_fingerprint(role: ExtensionAbiRole) -> [u8; 32] {
    let role = match role {
        ExtensionAbiRole::Color => super::evidence::AbiRole::Color,
        ExtensionAbiRole::Length => super::evidence::AbiRole::Length,
        ExtensionAbiRole::Rectangle => super::evidence::AbiRole::Rectangle,
        ExtensionAbiRole::Vector => super::evidence::AbiRole::Vector,
    };
    role.linked_fingerprint()
}

fn extension_abi_site_selector(abi: &ExtensionAbi<'_>) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(b"rawr.import.surface.facade.abi-site/v1");
    framed_bytes(&mut digest, abi.root.as_bytes());
    framed_bytes(&mut digest, abi.path.as_bytes());
    digest.update([match abi.namespace {
        Namespace::Type => 0,
        Namespace::Value => 1,
        Namespace::Macro => 2,
    }]);
    digest.update(abi.export);
    digest.update(abi.definition);
    digest.update(abi.shape);
    digest.finalize().into()
}

fn extension_abi_selector(abi: &ExtensionAbi<'_>) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(b"rawr.import.profile.extension-abi/v1");
    digest.update(abi.site);
    framed_bytes(&mut digest, extension_abi_name(abi.role).as_bytes());
    digest.update(abi.linked);
    for argument in abi.arguments {
        match argument {
            ExtensionAbiArgument::DefaultF32 { parameter } => {
                digest.update([0]);
                digest.update(parameter.to_le_bytes());
            }
        }
    }
    digest.finalize().into()
}

fn operation_site_selector(endpoint: &OperationEndpoint<'_>) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(b"rawr.import.surface.facade.operation-site/v1");
    framed_bytes(&mut digest, endpoint.root.as_bytes());
    framed_bytes(&mut digest, endpoint.path.as_bytes());
    digest.update([match endpoint.namespace {
        Namespace::Type => 0,
        Namespace::Value => 1,
        Namespace::Macro => 2,
    }]);
    digest.update(endpoint.export);
    digest.update(endpoint.definition);
    digest.update(
        u64::try_from(endpoint.formals.len())
            .unwrap_or(u64::MAX)
            .to_le_bytes(),
    );
    for formal in endpoint.formals {
        digest.update(formal.owner);
        digest.update(formal.index.to_le_bytes());
        digest.update([match formal.kind {
            ParameterKind::Lifetime => 0,
            ParameterKind::Type => 1,
            ParameterKind::Const => 2,
        }]);
    }
    digest.update(endpoint.shape);
    digest.finalize().into()
}

fn operation_endpoint_selector(endpoint: &OperationEndpoint<'_>) -> [u8; 32] {
    let mut identity = Sha256::new();
    identity.update(b"rawr.import.profile.identity/v1");
    framed_bytes(&mut identity, b"rawr.framework.iced");
    let identity: [u8; 32] = identity.finalize().into();

    let mut digest = Sha256::new();
    digest.update(b"rawr.import.profile.operation-endpoint/v1");
    digest.update(identity);
    digest.update(super::definition::VERSION.to_le_bytes());
    digest.update(endpoint.site);
    digest.update(endpoint.source);
    digest.update(endpoint.projection);
    digest.update(endpoint.linked);
    for argument in endpoint.arguments {
        match argument {
            EndpointArgument::Region { parameter } => {
                digest.update([0]);
                digest.update(parameter.to_le_bytes());
            }
            EndpointArgument::Definition {
                parameter,
                definition,
            } => {
                digest.update([1]);
                digest.update(parameter.to_le_bytes());
                digest.update(definition);
            }
            EndpointArgument::OwnedString {
                parameter,
                lifetime,
            } => {
                digest.update([2]);
                digest.update(parameter.to_le_bytes());
                digest.update(lifetime.to_le_bytes());
            }
        }
    }
    digest.finalize().into()
}

fn selector(projection: &Projection<'_>) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(b"rawr.import.profile.projection/v3");
    digest.update(projection.source);
    digest.update(projection.alias);
    for argument in projection.arguments {
        match argument {
            Argument::Source(index) => {
                digest.update([0]);
                digest.update(index.to_le_bytes());
            }
            Argument::Fresh => digest.update([1]),
        }
    }
    digest.finalize().into()
}

fn alias_site_selector(alias: &AliasTarget<'_>) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(b"rawr.import.surface.facade.alias-site/v2");
    framed_bytes(&mut digest, alias.root.as_bytes());
    framed_bytes(&mut digest, alias.path.as_bytes());
    digest.update([match alias.namespace {
        Namespace::Type => 0,
        Namespace::Value => 1,
        Namespace::Macro => 2,
    }]);
    digest.update(alias.export);
    digest.update(alias.alias);
    digest.update(alias.target);
    digest.update(
        u64::try_from(alias.parameters.len())
            .unwrap_or(u64::MAX)
            .to_le_bytes(),
    );
    for parameter in alias.parameters {
        digest.update([match parameter.kind {
            ParameterKind::Lifetime => 0,
            ParameterKind::Type => 1,
            ParameterKind::Const => 2,
        }]);
        digest.update(parameter.alias.to_le_bytes());
        digest.update(parameter.target.to_le_bytes());
    }
    digest.finalize().into()
}

fn alias_target_selector(alias: &AliasTarget<'_>) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(b"rawr.import.profile.alias-target/v2");
    digest.update(alias.site);
    digest.update(alias.export);
    digest.update(alias.alias);
    digest.update(alias.target);
    digest.finalize().into()
}

fn parameter_kind_matches(left: facade::ParameterKind, right: ParameterKind) -> bool {
    matches!(
        (left, right),
        (facade::ParameterKind::Lifetime, ParameterKind::Lifetime)
            | (facade::ParameterKind::Type, ParameterKind::Type)
            | (facade::ParameterKind::Const, ParameterKind::Const)
    )
}

fn projection_source_selector(source: &ProjectionSourceSite<'_>) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(b"rawr.import.profile.projection-source-site/v3");
    digest.update(source.site);
    digest.update(source.widget_trait);
    digest.update(source.widget);
    digest.update(source.target);
    for argument in source.arguments {
        match argument {
            SourceArgument::Region { alias, source } => {
                digest.update([0]);
                digest.update(alias.to_le_bytes());
                digest.update(source.to_le_bytes());
            }
            SourceArgument::Parameter {
                kind,
                alias,
                source,
            } => {
                digest.update([match kind {
                    ParameterKind::Lifetime => 1,
                    ParameterKind::Type => 2,
                    ParameterKind::Const => 3,
                }]);
                digest.update(alias.to_le_bytes());
                digest.update(source.to_le_bytes());
            }
        }
    }
    digest.finalize().into()
}

fn projection_source_site_selector(source: &ProjectionSourceSite<'_>) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(b"rawr.import.surface.facade.projection-source-site/v1");
    framed_bytes(&mut digest, source.root.as_bytes());
    framed_bytes(&mut digest, source.path.as_bytes());
    digest.update([match source.namespace {
        Namespace::Type => 0,
        Namespace::Value => 1,
        Namespace::Macro => 2,
    }]);
    digest.update(source.export);
    digest.update(source.alias);
    digest.update(source.source);
    digest.update([0]);
    digest.update(
        u64::try_from(source.parameters.len())
            .unwrap_or(u64::MAX)
            .to_le_bytes(),
    );
    for parameter in source.parameters {
        digest.update([match parameter.kind {
            ParameterKind::Lifetime => 0,
            ParameterKind::Type => 1,
            ParameterKind::Const => 2,
        }]);
        digest.update(parameter.alias.to_le_bytes());
        digest.update(parameter.target.to_le_bytes());
    }
    digest.finalize().into()
}

fn framed_bytes(digest: &mut Sha256, bytes: &[u8]) {
    digest.update(u64::try_from(bytes.len()).unwrap_or(u64::MAX).to_le_bytes());
    digest.update(bytes);
}

struct Decoration<'proof> {
    semantic: host::facade::Definition,
    compatibility: [u8; 32],
    aliases: &'proof [AliasTarget<'proof>],
    sources: &'proof [ProjectionSourceSite<'proof>],
    abis: &'proof [ExtensionAbi<'proof>],
    endpoints: &'proof [OperationEndpoint<'proof>],
    projections: &'proof [Projection<'proof>],
    plan: &'proof facade::Plan,
}

impl<'proof> Decoration<'proof> {
    #[allow(clippy::too_many_arguments)]
    const fn new(
        semantic: host::facade::Definition,
        compatibility: [u8; 32],
        aliases: &'proof [AliasTarget<'proof>],
        sources: &'proof [ProjectionSourceSite<'proof>],
        abis: &'proof [ExtensionAbi<'proof>],
        endpoints: &'proof [OperationEndpoint<'proof>],
        projections: &'proof [Projection<'proof>],
        plan: &'proof facade::Plan,
    ) -> Self {
        Self {
            semantic,
            compatibility,
            aliases,
            sources,
            abis,
            endpoints,
            projections,
            plan,
        }
    }

    fn semantic_type_path<'a>(
        &self,
        facade: &'a facade::Facade,
        identity: &[u8; 32],
    ) -> Option<&'a str> {
        if facade.root() != ROOT {
            return None;
        }
        facade
            .types()
            .find(|ty| ty.identity() == identity && SEMANTIC_TYPES.contains(&ty.path()))
            .map(facade::TypeDefinition::path)
    }

    fn semantic_item(path: &str) -> bool {
        matches!(path, "run" | "application" | "application::timed")
            || SEMANTIC_ITEMS.contains(&path)
            || SEMANTIC_ITEM_ROOTS
                .iter()
                .any(|prefix| path.starts_with(prefix))
    }

    fn compatibility_type(&self, path: &str) -> bool {
        let qualified = format!("iced::{path}");
        self.semantic
            .types()
            .any(|definition| definition.path() == qualified)
    }

    fn compatibility_item(&self, path: &str) -> bool {
        let qualified = format!("iced::{path}");
        self.semantic.items().any(|item| item.path() == qualified)
    }

    fn source_site(
        &self,
        selector: &[u8; 32],
    ) -> Option<(&ProjectionSourceSite<'_>, &facade::ProjectionSourceSite)> {
        let mut selected = self
            .sources
            .iter()
            .filter(|source| &source.selector == selector);
        let source = selected.next()?;
        if selected.next().is_some() {
            return None;
        }
        let mut sites = self
            .plan
            .projection_sources()
            .filter(|site| site.selector() == &source.site);
        let site = sites.next()?;
        sites.next().is_none().then_some((source, site))
    }

    fn abi_site(&self, abi: &ExtensionAbi<'_>) -> Option<&facade::AbiSite> {
        let mut sites = self
            .plan
            .abi_sites()
            .filter(|site| site.selector() == &abi.site);
        let site = sites.next()?;
        if sites.next().is_some()
            || site.root() != abi.root
            || site.path() != abi.path
            || site.namespace() != facade::Namespace::Type
            || site.export_identity() != &abi.export
            || site.definition_identity() != &abi.definition
            || site.shape_digest() != &abi.shape
        {
            return None;
        }
        Some(site)
    }

    fn operation_site(&self, endpoint: &OperationEndpoint<'_>) -> Option<&facade::OperationSite> {
        let mut sites = self
            .plan
            .operation_sites()
            .filter(|site| site.selector() == &endpoint.site);
        let site = sites.next()?;
        if sites.next().is_some()
            || site.root() != endpoint.root
            || site.path() != endpoint.path
            || site.namespace() != facade::Namespace::Value
            || site.export_identity() != &endpoint.export
            || site.definition_identity() != &endpoint.definition
        {
            return None;
        }
        Some(site)
    }

    fn alias(&self, projection: &Projection<'_>) -> Option<&AliasTarget<'_>> {
        let mut selected = self
            .aliases
            .iter()
            .filter(|alias| alias.selector == projection.alias);
        let alias = selected.next()?;
        selected.next().is_none().then_some(alias)
    }

    /// Prove each extension ABI type's ordinary `iced` type is the linked
    /// semantic one, so the extension can use it.
    fn extension_exports(&self, iced: &host::facade::Definition) -> Result<(), host::Error> {
        let export = |role| {
            let path = format!("iced::{}", extension_abi_name(role));
            let mut rows = self.abis.iter().filter(|abi| abi.role == role);
            let row = rows.next().ok_or_else(|| host::Error::UnknownFacadeType {
                item: "profile extension ABI".into(),
                ty: path.clone().into(),
            })?;
            if rows.next().is_some()
                || row.root != ROOT
                || row.path != extension_abi_name(role)
                || self.abi_site(row).is_none()
            {
                return Err(host::Error::UnknownFacadeType {
                    item: "profile extension ABI".into(),
                    ty: path.into(),
                });
            }
            let mut ordinary_rows = iced.types().filter(|definition| definition.path() == path);
            let ordinary = ordinary_rows.next();
            if ordinary_rows.next().is_some() {
                return Err(host::Error::UnknownFacadeType {
                    item: "profile extension ABI".into(),
                    ty: path.into(),
                });
            }
            let mut semantic = self
                .semantic
                .types()
                .filter(|definition| definition.path() == path);
            let linked = semantic.next();
            if semantic.next().is_some() {
                return Err(host::Error::UnknownFacadeType {
                    item: "profile extension ABI".into(),
                    ty: path.into(),
                });
            }
            let exact = ordinary
                .zip(linked)
                .is_some_and(|(ordinary, linked)| ordinary == linked);
            if !exact {
                return Err(host::Error::UnknownFacadeType {
                    item: "profile extension ABI".into(),
                    ty: path.into(),
                });
            }
            iced.support_type(extension_abi_name(role)).map(drop)
        };
        for role in [
            ExtensionAbiRole::Color,
            ExtensionAbiRole::Length,
            ExtensionAbiRole::Rectangle,
            ExtensionAbiRole::Vector,
        ] {
            export(role)?;
        }
        Ok(())
    }
}

impl rawr::package::import::runner::Profile for Decoration<'_> {
    fn ready(&self, facade: &facade::Facade, installed: &[host::facade::Definition]) -> bool {
        // A crate that returns a Rust future or sipper names iced's types,
        // so it waits for the iced facade.
        if facade.root() != ROOT
            && !installed.iter().any(|definition| definition.root() == ROOT)
            && facade.items().any(|item| {
                item.signature().inputs().iter().any(names_projected)
                    || names_projected(item.signature().output())
            })
        {
            return false;
        }
        self.projections.iter().all(|projection| {
            let Some((source, _)) = self.source_site(&projection.source) else {
                return false;
            };
            if source.root != facade.root() {
                return true;
            }
            let Some(alias) = self.alias(projection) else {
                return false;
            };
            alias.root == facade.root()
                || installed
                    .iter()
                    .any(|definition| definition.root() == alias.root)
        })
    }

    fn compatibility<'a>(
        &'a self,
        facade: &facade::Facade,
    ) -> Option<(&'a host::facade::Definition, [u8; 32])> {
        (facade.root() == ROOT).then_some((&self.semantic, self.compatibility))
    }

    fn owns_type(&self, facade: &facade::Facade, ty: &facade::TypeDefinition) -> bool {
        facade.root() == ROOT && self.compatibility_type(ty.path())
    }

    fn install_types(
        &self,
        facade: &facade::Facade,
        builder: &mut host::facade::Builder,
    ) -> Result<(), host::Error> {
        if facade.root() != ROOT {
            return Ok(());
        }
        for definition in self.semantic.types().filter(|definition| {
            definition
                .path()
                .strip_prefix("iced::")
                .is_some_and(|path| path != "Length" && SEMANTIC_TYPES.contains(&path))
        }) {
            builder.profile_type(definition)?;
        }
        for abi in self.abis {
            let path = format!("iced::{}", extension_abi_name(abi.role));
            if let Some(imported) = facade.types().find(|ty| ty.path() == abi.path)
                && imported.identity() != &abi.definition
            {
                return Err(host::Error::ConflictingHostIdentity {
                    existing: path.clone().into(),
                    incoming: path.clone().into(),
                });
            }
            let site = self
                .abi_site(abi)
                .ok_or_else(|| host::Error::UnknownFacadeType {
                    item: "profile extension ABI".into(),
                    ty: path.clone().into(),
                })?;
            let mut linked = self
                .semantic
                .types()
                .filter(|definition| definition.path() == path);
            let definition = linked
                .next()
                .ok_or_else(|| host::Error::UnknownFacadeType {
                    item: "linked extension ABI".into(),
                    ty: path.clone().into(),
                })?;
            if linked.next().is_some() || !definition.parameters().is_empty() {
                return Err(host::Error::UnknownFacadeType {
                    item: "linked extension ABI".into(),
                    ty: path.into(),
                });
            }
            builder.profile_semantic_support_type(
                definition,
                extension_abi_name(abi.role),
                site,
                abi.selector,
                abi.linked,
            )?;
        }
        Ok(())
    }

    fn semantic_type(
        &self,
        facade: &facade::Facade,
        installed: &[host::facade::Definition],
        identity: &[u8; 32],
        arguments: Box<[host::type_ref::Type]>,
    ) -> Result<Option<host::type_ref::Type>, host::Error> {
        // A Rust future or sipper has no Rust type to name, and any crate
        // may return one. The importer names it; iced's facade declares it.
        let projected = if identity
            == rawr_import::surface::identity::Definition::future().as_bytes()
        {
            Some("Task")
        } else if identity == rawr_import::surface::identity::Definition::sipper().as_bytes() {
            Some("task::Sipper")
        } else if identity == rawr_import::surface::identity::Definition::duration().as_bytes() {
            Some("time::Duration")
        } else {
            None
        };
        if let Some(path) = projected {
            if facade.root() == ROOT {
                return if arguments.is_empty() {
                    host::type_ref::Type::registered(path).map(Some)
                } else {
                    host::type_ref::Type::applied(path, arguments).map(Some)
                };
            }
            // Another crate's facade names the installed iced facade's type.
            let iced = installed
                .iter()
                .find(|definition| definition.root() == ROOT)
                .ok_or_else(|| host::Error::UnknownFacadeType {
                    item: "a Rust future or sipper".into(),
                    ty: format!("iced::{path}").into(),
                })?;
            return host::type_ref::Type::external(iced.export_type(path)?, arguments).map(Some);
        }
        let Some(path) = self.semantic_type_path(facade, identity) else {
            return Ok(None);
        };
        if arguments.is_empty() {
            host::type_ref::Type::registered(path).map(Some)
        } else {
            host::type_ref::Type::applied(path, arguments).map(Some)
        }
    }

    fn owns_item(&self, facade: &facade::Facade, item: &facade::Item) -> bool {
        // An imported associated item of a type the profile owns names that
        // imported type, which `owns_type` replaces with the profile's own; on
        // iced master `Column<W>` and its builders import, so they go with it.
        facade.root() == ROOT
            && (self.compatibility_item(item.path())
                || item
                    .path()
                    .rsplit_once("::")
                    .is_some_and(|(owner, _)| self.compatibility_type(owner)))
    }

    fn install_items(
        &self,
        facade: &facade::Facade,
        builder: &mut host::facade::Builder,
    ) -> Result<(), host::Error> {
        if facade.root() != ROOT {
            return Ok(());
        }
        for item in self.semantic.items().filter(|item| {
            item.path()
                .strip_prefix("iced::")
                .is_some_and(Self::semantic_item)
        }) {
            builder.profile_item(item)?;
        }
        Ok(())
    }

    fn install_endpoints(
        &self,
        facade: &facade::Facade,
        _builder: &mut host::facade::Builder,
    ) -> Result<(), host::Error> {
        if facade.root() != ROOT || self.endpoints.len() != 1 {
            return if facade.root() == ROOT {
                Err(host::Error::UnknownFacadeType {
                    item: "profile operation endpoint".into(),
                    ty: "iced::widget::text".into(),
                })
            } else {
                Ok(())
            };
        }
        let endpoint = &self.endpoints[0];
        let site = self
            .operation_site(endpoint)
            .ok_or_else(|| host::Error::UnknownFacadeType {
                item: "profile operation endpoint".into(),
                ty: "iced::widget::text".into(),
            })?;
        let projection = self
            .projections
            .iter()
            .find(|projection| projection.selector == endpoint.projection)
            .ok_or_else(|| host::Error::UnknownFacadeType {
                item: "profile operation endpoint".into(),
                ty: "profile projection".into(),
            })?;
        let alias = self
            .aliases
            .iter()
            .find(|alias| alias.selector == projection.alias)
            .ok_or_else(|| host::Error::UnknownFacadeType {
                item: "profile operation endpoint".into(),
                ty: "profile alias target".into(),
            })?;
        if site.path() != "widget::text" || alias.path != "Element" {
            return Err(host::Error::UnknownFacadeType {
                item: "profile operation endpoint".into(),
                ty: "complete iced::widget::Text compatibility cohort".into(),
            });
        }

        // The endpoint proof remains validated and fingerprinted, but the
        // facade's own `text` owns the sole final callable meaning.
        Ok(())
    }

    fn install_projections(
        &self,
        facade: &facade::Facade,
        installed: &[host::facade::Definition],
        builder: &mut host::facade::Builder,
    ) -> Result<(), host::Error> {
        for projection in self.projections {
            let Some((source_proof, _source_site)) = self.source_site(&projection.source) else {
                return Err(host::Error::UnknownFacadeType {
                    item: "profile source site".into(),
                    ty: "profile projection source".into(),
                });
            };
            if source_proof.root != facade.root() {
                continue;
            }
            let alias = self
                .alias(projection)
                .ok_or_else(|| host::Error::UnknownFacadeType {
                    item: source_proof.path.into(),
                    ty: "profile projection alias".into(),
                })?;
            let Some(source) = facade.types().find(|ty| ty.path() == source_proof.path) else {
                // A support-only lifetime-bearing source is intentionally not installed as
                // an ordinary facade type. Its operation projection is sealed separately.
                continue;
            };
            let parameters = source.parameters().collect::<Vec<_>>();
            let mut arguments = Vec::new();
            arguments
                .try_reserve_exact(projection.arguments.len())
                .map_err(|_| host::Error::Allocation)?;
            for argument in projection.arguments {
                arguments.push(match argument {
                    Argument::Source(index) => {
                        let index = usize::try_from(*index).map_err(|_| {
                            host::Error::InvalidProjectionParameter {
                                source: source.path().into(),
                                target: alias.path.into(),
                                parameter: "profile source index".into(),
                            }
                        })?;
                        let parameter = parameters.get(index).ok_or_else(|| {
                            host::Error::InvalidProjectionParameter {
                                source: source.path().into(),
                                target: alias.path.into(),
                                parameter: "profile source index".into(),
                            }
                        })?;
                        host::type_def::Argument::source(*parameter)?
                    }
                    Argument::Fresh => host::type_def::Argument::fresh(),
                });
            }
            if alias.root == facade.root() {
                builder.project_with(source.path(), alias.path, &arguments)?;
            } else {
                let target = installed
                    .iter()
                    .find(|definition| definition.root() == alias.root)
                    .ok_or_else(|| host::Error::UnknownFacadeType {
                        item: source.path().into(),
                        ty: alias.path.into(),
                    })?
                    .export_type(alias.path)?;
                builder.project_to_with(source.path(), target, &arguments)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod alias_proof_tests {
    use super::{
        alias_site_selector, alias_target_selector, bind_runtime, projection_source_selector,
        projection_source_site_selector, selector, validate_extension_abis,
        validate_operation_endpoints, validate_projections, with_contract_rows, AliasParameter,
        AliasTarget, Argument, Namespace, ParameterKind, Projection, ProjectionSourceSite,
        SourceArgument,
    };
    use sha2::{Digest as _, Sha256};

    static PARAMETERS: [AliasParameter; 1] = [AliasParameter::new(ParameterKind::Type, 0, 0)];
    static MISMAPPED: [AliasParameter; 1] = [AliasParameter::new(ParameterKind::Type, 1, 0)];
    static ARGUMENTS: [Argument; 1] = [Argument::fresh()];
    static SOURCE_PARAMETERS: [AliasParameter; 2] = [
        AliasParameter::new(ParameterKind::Lifetime, 0, 0),
        AliasParameter::new(ParameterKind::Type, 1, 1),
    ];
    static SOURCE_ARGUMENTS: [SourceArgument; 2] = [
        SourceArgument::Region {
            alias: 0,
            source: 0,
        },
        SourceArgument::Parameter {
            kind: ParameterKind::Type,
            alias: 1,
            source: 1,
        },
    ];

    fn alias() -> AliasTarget<'static> {
        AliasTarget::checked(
            "iced",
            "Element",
            Namespace::Type,
            [0xe1; 32],
            [0xa1; 32],
            [3; 32],
            &PARAMETERS,
        )
    }

    #[test]
    fn runtime_replacement_is_proven_before_the_profile_inventory_changes() {
        const SIBLINGS: [&str; 9] = [
            "iced::widget::TextInput",
            "iced::widget::Checkbox",
            "iced::widget::Toggler",
            "iced::widget::Slider",
            "iced::widget::Button",
            "iced::widget::Column",
            "iced::widget::Row",
            "iced::widget::Container",
            "iced::widget::Scrollable",
        ];
        static WRONG_ARGUMENTS: [Argument; 1] = [Argument::Source(0)];

        let limits = rawr_import::cargo::Limits::standard();
        let definition = crate::profile::Definition::new(&limits).unwrap();
        let graph = rawr_import::test_fixture::iced_profile();
        let contract = rawr_import::profile::classify(&definition, &graph, &limits).unwrap();
        let classified = graph
            .classify(&rawr_import::classify::Policy::standard(), &limits)
            .unwrap();
        let bytes = rawr_import::surface::facade::Plan::encode(&classified, &limits).unwrap();
        let bytes = rawr_import::test_fixture::iced_text_module_facade_plan(&bytes);
        let mut engine = rawr::Engine::new();
        let compiler = super::install_compiler_contract(&mut engine, &bytes, [0xd3; 32], &contract)
            .expect("the exact generated profile compiler proof installs");

        with_contract_rows(
            &contract,
            |_aliases, _sources, _abis, _endpoints, projections| {
                let definitions = compiler.definitions();
                let iced = definitions
                    .iter()
                    .position(|definition| definition.root() == "iced")
                    .expect("one iced runtime owner");
                let fresh = || {
                    definitions
                        .iter()
                        .cloned()
                        .map(rawr::host::runtime::Builder::new)
                        .collect::<Vec<_>>()
                };

                let mut builders = fresh();
                bind_runtime(definitions, &mut builders, projections)?;
                assert!(!builders[iced].has_type_attestation("iced::Element"));
                assert!(SIBLINGS
                    .iter()
                    .all(|path| builders[iced].has_type_attestation(path)));
                builders[iced]
                    .bind_type_attestation("iced::Element", super::attest_support_element)
                    .expect("the exact generated replacement owns the empty slot");
                assert!(matches!(
                    builders[iced].bind_type_attestation("iced::Element", |_, _| None),
                    Err(rawr::host::runtime::Error::DuplicateTypeAttestation { .. })
                ));

                let exact = projections[0];
                let mut stale = exact;
                stale.schema = 2;
                let mut wrong_arguments = exact;
                wrong_arguments.arguments = &WRONG_ARGUMENTS;
                wrong_arguments.selector = selector(&wrong_arguments);
                let cases = [
                    Vec::new(),
                    vec![exact, exact],
                    vec![stale],
                    vec![wrong_arguments],
                ];
                for forged in cases {
                    let mut builders = fresh();
                    assert!(matches!(
                        bind_runtime(definitions, &mut builders, &forged),
                        Err(super::Error::ElementReplacement)
                    ));
                    assert!(!builders[iced].has_type_attestation("iced::Element"));
                    assert!(!builders[iced].has_type_attestation(SIBLINGS[0]));
                }
                Ok(())
            },
        )
        .expect("only the exact atomic replacement proof is accepted");
    }

    fn source(alias: &AliasTarget<'_>) -> ProjectionSourceSite<'static> {
        ProjectionSourceSite::checked(
            "iced",
            "Text",
            Namespace::Type,
            [0xe3; 32],
            [0xa3; 32],
            [0x22; 32],
            &SOURCE_PARAMETERS,
            &SOURCE_ARGUMENTS,
            [0x80; 32],
            [0x81; 32],
            alias.selector,
        )
    }

    fn projection(
        alias: &AliasTarget<'_>,
        source: &ProjectionSourceSite<'_>,
    ) -> Projection<'static> {
        Projection::checked(source.selector, alias.selector, &ARGUMENTS)
    }

    fn resign(alias: &mut AliasTarget<'_>) {
        alias.site = alias_site_selector(alias);
        alias.selector = alias_target_selector(alias);
    }

    fn resign_source(source: &mut ProjectionSourceSite<'_>) {
        source.site = projection_source_site_selector(source);
        source.selector = projection_source_selector(source);
    }

    #[test]
    fn alias_site_fields_and_generated_versions_fail_closed_independently() {
        let limits = rawr_import::cargo::Limits::standard();
        let bytes = rawr_import::test_fixture::iced_semantic_facade_plan();
        let plan = rawr_import::surface::facade::Plan::decode(&bytes, &limits).unwrap();
        let exact = alias();
        let exact_source = source(&exact);
        let row = projection(&exact, &exact_source);
        validate_projections(&plan, &[exact], &[exact_source], &[row])
            .expect("the exact fixture proof validates");

        let mut cases = Vec::new();
        let mut stale = exact;
        stale.schema = 0;
        cases.push(stale);
        let mut path = exact;
        path.path = "Other";
        resign(&mut path);
        cases.push(path);
        let mut export = exact;
        export.export = [0xe2; 32];
        resign(&mut export);
        cases.push(export);
        let mut definition = exact;
        definition.alias = [0xa2; 32];
        resign(&mut definition);
        cases.push(definition);
        let mut target = exact;
        target.target = [4; 32];
        resign(&mut target);
        cases.push(target);
        let mut namespace = exact;
        namespace.namespace = Namespace::Value;
        resign(&mut namespace);
        cases.push(namespace);
        let mut mapping = exact;
        mapping.parameters = &MISMAPPED;
        resign(&mut mapping);
        cases.push(mapping);
        for forged in cases {
            let source = source(&exact);
            let projection = projection(&forged, &source);
            assert!(validate_projections(&plan, &[forged], &[source], &[projection]).is_err());
        }

        assert!(validate_projections(&plan, &[exact, exact], &[exact_source], &[row]).is_err());
        assert!(validate_projections(&plan, &[], &[exact_source], &[row]).is_err());
        assert!(validate_projections(&plan, &[exact], &[], &[row]).is_err());
        assert!(
            validate_projections(&plan, &[exact], &[exact_source, exact_source], &[row]).is_err()
        );
        let mut stale_source = exact_source;
        stale_source.schema = 0;
        assert!(validate_projections(&plan, &[exact], &[stale_source], &[row]).is_err());
        let mut wrong_source = exact_source;
        wrong_source.source = [0x23; 32];
        resign_source(&mut wrong_source);
        let wrong_row = projection(&exact, &wrong_source);
        assert!(validate_projections(&plan, &[exact], &[wrong_source], &[wrong_row]).is_err());
        let mut stale_row = row;
        stale_row.schema = 2;
        assert!(validate_projections(&plan, &[exact], &[exact_source], &[stale_row]).is_err());
        let mut zero_source = row;
        zero_source.source = [0; 32];
        zero_source.selector = selector(&zero_source);
        assert!(validate_projections(&plan, &[exact], &[exact_source], &[zero_source]).is_err());
        let mut missing_alias = row;
        missing_alias.alias = [0; 32];
        missing_alias.selector = selector(&missing_alias);
        assert!(validate_projections(&plan, &[exact], &[exact_source], &[missing_alias]).is_err());
    }

    #[test]
    fn compiler_authentic_alias_site_validates_without_a_target_facade_type() {
        let limits = rawr_import::cargo::Limits::standard();
        let definition = crate::profile::Definition::new(&limits).unwrap();
        let graph = rawr_import::test_fixture::iced_profile();
        let contract = rawr_import::profile::classify(&definition, &graph, &limits).unwrap();
        let alias = contract.alias_targets().next().expect("one Element alias");
        assert_eq!(contract.alias_targets().len(), 1);
        assert_eq!(alias.root(), "iced");
        assert_eq!(alias.path(), "Element");
        assert_eq!(
            alias.namespace(),
            rawr_import::surface::facade::Namespace::Type
        );
        assert_eq!(
            alias
                .parameters()
                .iter()
                .map(|parameter| (
                    parameter.kind(),
                    parameter.alias_index(),
                    parameter.target_index(),
                ))
                .collect::<Vec<_>>(),
            [
                (rawr_import::surface::facade::ParameterKind::Lifetime, 0, 0),
                (rawr_import::surface::facade::ParameterKind::Type, 1, 1),
                (rawr_import::surface::facade::ParameterKind::Type, 2, 2),
                (rawr_import::surface::facade::ParameterKind::Type, 3, 3),
            ]
        );
        let projection = contract
            .projections()
            .next()
            .expect("one direct projection");
        assert_eq!(projection.alias_selector(), alias.selector());
        let target = *alias.target_identity();

        let classified = graph
            .classify(&rawr_import::classify::Policy::standard(), &limits)
            .unwrap();
        let mut rejected_target_paths = classified
            .outcomes()
            .filter(|outcome| {
                outcome
                    .identity()
                    .definition()
                    .is_some_and(|id| id.as_bytes() == &target)
            })
            .map(|outcome| {
                let rawr_import::classify::representability::Representability::Rejected(reason) =
                    outcome.representability()
                else {
                    panic!("the compiler-authentic lifetime target remains rejected")
                };
                assert_eq!(
                    reason.kind(),
                    rawr_import::classify::representability::reject::Kind::LifetimeOrConstGeneric
                );
                outcome.export().path().collect::<Vec<_>>().join("::")
            })
            .collect::<Vec<_>>();
        rejected_target_paths.sort_unstable();
        assert_eq!(
            rejected_target_paths,
            [
                "advanced::graphics::core::Element",
                "advanced::graphics::futures::core::Element",
            ]
        );
        let bytes = rawr_import::surface::facade::Plan::encode(&classified, &limits).unwrap();
        let plan = rawr_import::surface::facade::Plan::decode(&bytes, &limits).unwrap();
        let mut alias_sites = plan
            .aliases()
            .filter(|site| site.selector() == alias.site_selector());
        let site = alias_sites.next().expect("one exact Element alias site");
        assert!(alias_sites.next().is_none());
        assert_eq!(site.root(), alias.root());
        assert_eq!(site.path(), alias.path());
        assert_eq!(site.selector(), alias.site_selector());
        assert_eq!(site.export_identity(), alias.export_identity());
        assert_eq!(site.alias_identity(), alias.alias_identity());
        assert_eq!(site.target_identity(), alias.target_identity());
        assert_eq!(site.parameters(), alias.parameters());
        let source = contract
            .projection_sources()
            .next()
            .expect("one selected Text source site");
        assert_eq!(contract.projection_sources().len(), 1);
        let mut source_sites = plan
            .projection_sources()
            .filter(|site| site.selector() == source.site_selector());
        let source_site = source_sites.next().expect("one exact Text source site");
        assert!(source_sites.next().is_none());
        assert_eq!(source_site.root(), source.root());
        assert_eq!(source_site.path(), source.path());
        assert_eq!(source_site.export_identity(), source.export_identity());
        assert_eq!(source_site.alias_identity(), source.alias_identity());
        assert_eq!(source_site.source_identity(), source.source_identity());
        assert_eq!(source_site.parameters(), source.parameters());
        assert_eq!(
            plan.facades()
                .flat_map(rawr_import::surface::facade::Facade::types)
                .filter(|ty| ty.identity() == &target)
                .count(),
            0
        );
        with_contract_rows(
            &contract,
            |aliases, sources, abis, endpoints, projections| {
                validate_projections(&plan, aliases, sources, projections)?;
                validate_extension_abis(&plan, abis)?;
                validate_operation_endpoints(&plan, sources, endpoints, projections)?;
                let [endpoint] = endpoints else {
                    panic!("one exact operation endpoint row")
                };
                assert_eq!(endpoint.schema, 2);
                assert_eq!(
                    endpoint.linked,
                    super::super::evidence::operation_linked_fingerprint()
                );

                let mut stale_endpoint = endpoints.to_vec();
                stale_endpoint[0].schema = 1;
                assert!(validate_projections(&plan, aliases, sources, projections).is_ok());
                assert!(validate_extension_abis(&plan, abis).is_ok());
                assert!(
                    validate_operation_endpoints(&plan, sources, &stale_endpoint, projections)
                        .is_err()
                );

                let mut old_linked = Sha256::new();
                old_linked.update(b"rawr.iced.operation-endpoint.linked-host/v1");
                old_linked.update(7_u32.to_le_bytes());
                old_linked.update(b"iced::widget::text");
                old_linked.update(
                    b"owned String -> &'view str -> iced::widget::Text<'view, Theme, Renderer>",
                );
                let mut stale_linked = endpoints.to_vec();
                stale_linked[0].linked = old_linked.finalize().into();
                stale_linked[0].selector = super::operation_endpoint_selector(&stale_linked[0]);
                assert!(validate_projections(&plan, aliases, sources, projections).is_ok());
                assert!(validate_extension_abis(&plan, abis).is_ok());
                assert!(
                    validate_operation_endpoints(&plan, sources, &stale_linked, projections)
                        .is_err()
                );
                let rectangle = abis
                    .iter()
                    .position(|abi| abi.role == super::ExtensionAbiRole::Rectangle)
                    .expect("one Rectangle ABI row");
                let mut cases = Vec::new();
                let mut stale = abis.to_vec();
                stale[rectangle].schema = 1;
                cases.push(stale);
                let mut role = abis.to_vec();
                role[rectangle].role = super::ExtensionAbiRole::Vector;
                cases.push(role);
                let mut path = abis.to_vec();
                path[rectangle].path = "Other";
                cases.push(path);
                let mut namespace = abis.to_vec();
                namespace[rectangle].namespace = Namespace::Value;
                cases.push(namespace);
                let mut export = abis.to_vec();
                export[rectangle].export = [0x41; 32];
                cases.push(export);
                let mut definition = abis.to_vec();
                definition[rectangle].definition = [0x42; 32];
                cases.push(definition);
                let mut shape = abis.to_vec();
                shape[rectangle].shape = [0x43; 32];
                cases.push(shape);
                let mut site = abis.to_vec();
                site[rectangle].site = [0x44; 32];
                cases.push(site);
                let mut linked = abis.to_vec();
                linked[rectangle].linked = [0x45; 32];
                cases.push(linked);
                let length = abis
                    .iter()
                    .position(|abi| abi.role == super::ExtensionAbiRole::Length)
                    .expect("one Length ABI row");
                let mut old_linked = Sha256::new();
                old_linked.update(b"rawr.iced.extension-abi.linked-host/v1");
                old_linked.update(13_u32.to_le_bytes());
                old_linked.update(b"Length");
                old_linked.update(b"iced::Length");
                let mut stale_length = abis.to_vec();
                stale_length[length].linked = old_linked.finalize().into();
                stale_length[length].selector =
                    super::extension_abi_selector(&stale_length[length]);
                cases.push(stale_length);
                let mut specialization = abis.to_vec();
                specialization[rectangle].arguments = &[];
                cases.push(specialization);
                let mut profile = abis.to_vec();
                profile[rectangle].selector = [0x46; 32];
                cases.push(profile);
                for forged in cases {
                    assert!(validate_extension_abis(&plan, &forged).is_err());
                }
                assert!(validate_extension_abis(&plan, &abis[..3]).is_err());
                let mut duplicate = abis.to_vec();
                duplicate.push(abis[rectangle]);
                assert!(validate_extension_abis(&plan, &duplicate).is_err());
                let mut shuffled = abis.to_vec();
                shuffled.reverse();
                assert!(validate_extension_abis(&plan, &shuffled).is_err());
                Ok(())
            },
        )
        .expect("the exact proof rows validate while stale support rows fail closed");

        let shuffled = rawr_import::test_fixture::iced_profile_element_case(
            rawr_import::test_fixture::IcedElementCase::Shuffled,
        )
        .unwrap();
        let shuffled_contract =
            rawr_import::profile::classify(&definition, &shuffled, &limits).unwrap();
        let shuffled_classified = shuffled
            .classify(&rawr_import::classify::Policy::standard(), &limits)
            .unwrap();
        let shuffled_bytes =
            rawr_import::surface::facade::Plan::encode(&shuffled_classified, &limits).unwrap();
        assert_eq!(bytes, shuffled_bytes);
        assert_eq!(contract.rules_digest(), shuffled_contract.rules_digest());
    }

    #[test]
    fn the_facade_text_cohort_suppresses_the_profile_endpoint() {
        let limits = rawr_import::cargo::Limits::standard();
        let definition = crate::profile::Definition::new(&limits).unwrap();
        let graph = rawr_import::test_fixture::iced_profile();
        let contract = rawr_import::profile::classify(&definition, &graph, &limits).unwrap();
        let classified = graph
            .classify(&rawr_import::classify::Policy::standard(), &limits)
            .unwrap();
        let bytes = rawr_import::surface::facade::Plan::encode(&classified, &limits).unwrap();
        let bytes = rawr_import::test_fixture::iced_text_module_facade_plan(&bytes);
        let mut engine = rawr::Engine::new();
        super::install_compiler_contract(&mut engine, &bytes, [0xd2; 32], &contract)
            .expect("the endpoint proof is validated under the complete Text cohort");

        engine
            .compile_executable(
                r#"
use iced::Element;
use iced::widget::text;
enum Message { Tick }
pub fn style() { let _style = text::primary; }
pub fn string() -> Element<Message> { text("generated") }
pub fn integer() -> Element<Message> { text(24) }
pub fn boolean() -> Element<Message> { text(true) }
pub fn character() -> Element<Message> { text('R') }
pub fn float() -> Element<Message> { text(2.5) }
pub fn chained() -> Element<Message> { text("generated").size(24) }
"#,
            )
            .expect("all ordinary Text callables, its projection, and its builders survive");

        let absent = engine
            .compile_executable(
                r#"
use iced::Element;
use iced::widget::label;
enum Message { Tick }
pub fn view() -> Element<Message> { label("generated") }
"#,
            )
            .expect_err("a module without an endpoint remains an unresolved call");
        assert!(
            absent.to_string().contains("nknown function 'label'"),
            "{absent}"
        );

        let wrong_arity =
            engine.compile(r#"use iced::widget::text; pub fn invalid() { text("one", "two") }"#);
        assert!(
            wrong_arity.is_err(),
            "the ordinary overload set remains exact"
        );
    }

    #[test]
    fn imported_facade_keeps_the_compatibility_seed_re_exports() {
        let limits = rawr_import::cargo::Limits::standard();
        let definition = crate::profile::Definition::new(&limits).unwrap();
        let graph = rawr_import::test_fixture::iced_profile();
        let contract = rawr_import::profile::classify(&definition, &graph, &limits).unwrap();
        let classified = graph
            .classify(&rawr_import::classify::Policy::standard(), &limits)
            .unwrap();
        let bytes = rawr_import::surface::facade::Plan::encode(&classified, &limits).unwrap();
        let bytes = rawr_import::test_fixture::iced_text_module_facade_plan(&bytes);
        let mut engine = rawr::Engine::new();
        super::install_compiler_contract(&mut engine, &bytes, [0xd2; 32], &contract)
            .expect("the imported iced facade installs");

        engine
            .compile_executable(
                r#"
use iced::Renderer;
use iced::widget::canvas::Event;
use iced::widget::canvas::stroke::Stroke;
pub fn renderer(_renderer: &Renderer) {}
pub fn event(_event: Event) {}
pub fn stroke() -> Stroke<'static> { Stroke::default() }
"#,
            )
            .expect("the facade's re-exports resolve beside imported iced");
    }
}

#[cfg(test)]
mod hot_outcome_tests;

#[cfg(all(test, unix))]
mod observation_tests;

/// Whether `ty` names a Rust future or sipper, which iced's facade declares.
fn names_projected(ty: &facade::Type) -> bool {
    match ty {
        facade::Type::Nominal {
            identity,
            arguments,
        } => {
            rawr_import::surface::identity::Definition::is_projected(identity)
                || arguments.iter().any(names_projected)
        }
        facade::Type::FunctionItem { input, output } | facade::Type::Result(input, output) => {
            names_projected(input) || names_projected(output)
        }
        facade::Type::Option(element) => names_projected(element),
        _ => false,
    }
}
