//! Closed, bounded classification of authoritative Rust API evidence.

use serde::ser::{SerializeSeq as _, SerializeStruct as _};
use serde::{Deserialize, Serialize, Serializer};
use sha2::{Digest as _, Sha256};
use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::{self, Write};

use crate::api::{self, ledger, model};
use crate::contract::{self, Classified, Contract};

#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod cached;
pub mod representability;
pub mod std_binding;

use std_binding::Std;

const CONTRACT_SCHEMA: u32 = 7;
const CLASSIFIER_VERSION: u32 = 152;

/// The contract schema and classifier version every contract fingerprint
/// carries.
pub(crate) const fn contract_versions() -> [u32; 2] {
    [CONTRACT_SCHEMA, CLASSIFIER_VERSION]
}
const COMPILER_ABI: u32 = 1;

/// Frozen classifier policy; callers cannot choose semantic versions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Policy {
    schema: u32,
    classifier: u32,
    compiler_abi: u32,
}

struct FunctionParameters {
    parameters: Box<[representability::signature::Parameter]>,
    callable: Option<crate::surface::callable::Class>,
    binders: Box<[crate::surface::instance::Binder]>,
    predicates: Box<[representability::predicate::Bound]>,
    /// The callable parameter, its `Fn*` inputs and its `Output`.
    call: Option<(u32, Box<[model::Type]>, model::Type)>,
    /// Every callable parameter, its `Fn*` inputs, its `Output`, and
    /// whether the output borrows at the bound's own lifetime: what a
    /// script function crosses as when it fills the parameter (D594).
    crossings: Vec<Crossing>,
}

/// A callable parameter's index, its `Fn*` inputs, its `Output`, and
/// whether that output borrows at the bound's own lifetime.
type Crossing = (u32, Box<[model::Type]>, model::Type, bool);

#[derive(Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct OutcomeRecord {
    export: api::identity::Export,
    identity: crate::surface::identity::Identity,
    name: Box<str>,
    origin: Option<representability::origin::Set>,
    evidence: OutcomeEvidence,
    implementations: Box<[ImplementationRecord]>,
    /// The variant of its enum this path re-exports, as `pub use
    /// Alignment::Center;` does.
    variant: Option<Box<str>>,
    /// The trait's own method an impl's member is an instance of, as each
    /// `into` of `Degrees` that core's blanket `Into` gives through a
    /// crate's `From<Degrees>` is `Into::into`, whatever its selector.
    member: Option<crate::surface::identity::Definition>,
}

#[derive(Debug, Eq, Ord, PartialEq, PartialOrd)]
enum OutcomeEvidence {
    Plan {
        definition: crate::surface::identity::Definition,
        index: u32,
    },
    Inline(Box<representability::Representability>),
}

/// One Contract-borrowed classified outcome and its exact representability proof.
///
/// ```compile_fail
/// let _ = rawr_import::classify::Outcome {};
/// ```
#[derive(Clone, Copy, Debug)]
pub struct Outcome<'contract> {
    record: &'contract OutcomeRecord,
    representability: &'contract representability::Representability,
    definitions: &'contract [DefinitionPlan],
}

/// One alias-neutral representability proof shared by every export of a definition.
///
/// ```compile_fail
/// let _ = rawr_import::classify::DefinitionPlan {};
/// ```
#[derive(Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
pub struct DefinitionPlan {
    definition: crate::surface::identity::Definition,
    representability: representability::Representability,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct SupportLedger {
    groups: Box<[SupportGroup]>,
    edges: Box<[SupportEdge]>,
    rejections: Box<[SupportRejection]>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct SupportGroup {
    provenance: SupportProvenance,
    count: usize,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct SupportEdge {
    definition: crate::surface::identity::Definition,
    group: u32,
    rejection: Option<std::num::NonZeroU32>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct SupportRejection {
    reason: ledger::Reason,
    span: model::Span,
}

/// One borrowed exact recursive support outcome from a shared definition plan.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum SupportOutcome<'a> {
    /// The support definition and its recursive closure are complete.
    Complete {
        definition: crate::surface::identity::Definition,
        provenance: SupportProvenance,
    },
    /// The support edge has stable positioned Rust rejection evidence.
    Rejected {
        definition: crate::surface::identity::Definition,
        provenance: SupportProvenance,
        reason: &'a ledger::Reason,
        span: &'a model::Span,
    },
}

/// Exact semantic provenance that made one recursive support edge necessary.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum SupportProvenance {
    /// A retained definition's public shape requires the edge.
    Definition(crate::surface::identity::Definition),
    /// One exact domain-separated implementation requires the edge.
    Implementation(crate::surface::identity::Implementation),
}

/// A borrowing iterator over one plan's closed recursive support outcomes.
pub struct SupportOutcomes<'a> {
    ledger: &'a SupportLedger,
    group: Option<u32>,
    edge: usize,
    remaining: usize,
}

struct CanonicalSupport<'a>(&'a SupportLedger);

/// One exact implementation ledger nested under the export that retained it.
#[derive(Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct ImplementationRecord {
    identity: crate::surface::identity::Implementation,
    span: model::Span,
    associated: Box<[OutcomeRecord]>,
}

/// One Contract-borrowed implementation ledger nested under an exact export.
#[derive(Clone, Copy, Debug)]
pub struct ImplementationOutcome<'contract> {
    record: &'contract ImplementationRecord,
    definitions: &'contract [DefinitionPlan],
}

#[derive(Serialize)]
struct Canonical<'a> {
    semantic_api: &'a [u8; 32],
    schema: u32,
    classifier: u32,
    compiler_abi: u32,
    support: CanonicalSupport<'a>,
    definitions: &'a [DefinitionPlan],
    outcomes: &'a [OutcomeRecord],
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredCanonical {
    semantic_api: [u8; 32],
    schema: u32,
    classifier: u32,
    compiler_abi: u32,
    support: Box<[StoredSupportOutcome]>,
    definitions: Box<[DefinitionPlan]>,
    outcomes: Box<[StoredOutcome]>,
}

#[derive(Deserialize, Serialize)]
// A decoded record that is converted at once; boxing would allocate per record.
#[allow(clippy::large_enum_variant)]
enum StoredSupportOutcome {
    Complete {
        definition: crate::surface::identity::Definition,
        provenance: SupportProvenance,
    },
    Rejected {
        definition: crate::surface::identity::Definition,
        provenance: SupportProvenance,
        reason: ledger::Reason,
        span: model::Span,
    },
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredOutcome {
    export: api::identity::Export,
    identity: crate::surface::identity::Identity,
    name: Box<str>,
    origin: Option<representability::origin::Set>,
    definition_plan: Option<crate::surface::identity::Definition>,
    representability: Option<representability::Representability>,
    implementations: Box<[StoredImplementation]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    variant: Option<Box<str>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    member: Option<crate::surface::identity::Definition>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredImplementation {
    identity: crate::surface::identity::Implementation,
    span: model::Span,
    associated: Box<[StoredOutcome]>,
}

pub(crate) struct ClassifiedParts {
    definitions: Box<[DefinitionPlan]>,
    outcomes: Box<[OutcomeRecord]>,
    support: SupportLedger,
    canonical: Box<[u8]>,
    pub(crate) fingerprint: contract::fingerprint::Fingerprint,
    allocation: contract::Usage,
}

/// Counts the encoded canonical contract bytes; only checked-arithmetic
/// overflow stops the measurement.
struct Measured {
    bytes: usize,
    overflow: bool,
}

/// Checked-arithmetic overflow is a genuine failure and stays terminal.
#[cfg(not(target_arch = "wasm32"))]
fn arithmetic() -> Error {
    Error::Node(Box::new(crate::node::Error::Allocation))
}

/// Checked-arithmetic overflow is a genuine failure and stays terminal. A
/// runner on wasm has no node store to report it through.
#[cfg(target_arch = "wasm32")]
fn arithmetic() -> Error {
    Error::Allocation
}

struct Context<'graph, P: api::Phase = api::Extracted> {
    graph: &'graph api::Graph<P>,
    support: SupportLedger,
    item_limit: usize,
    byte_limit: usize,
    retained_items: Cell<usize>,
    retained_bytes: Cell<usize>,
    scratch_bytes: Cell<usize>,
    /// The implementation whose self type a trait's `Self` stands for, while
    /// a default method of that trait is classified as its member.
    trait_self: Cell<Option<usize>>,
    /// The signature being converted opens a scope frame (D558 tier 1):
    /// its output's late-bound lifetimes are the frame's, as its early
    /// ones are `'static`, and are written by neither.
    scope_lifetimes: Cell<bool>,
    /// The crate type aliases that are the only name of the type they
    /// name, by definition index (`instance_aliases`).
    instance_aliases: std::cell::OnceCell<Box<[usize]>>,
    /// Each type whose only name is a passing alias, with that alias, by
    /// definition index (`sole_passing_aliases`).
    sole_aliases: std::cell::OnceCell<Box<[(usize, usize)]>>,
    /// While a function's output is converted: the type parameters a
    /// script callable or future fills, which an opaque `impl Trait` type
    /// it gives does not take as arguments. `None` elsewhere, where an
    /// opaque type is refused.
    opaque_output: std::cell::RefCell<Option<Box<[u32]>>>,
    /// The positions of the captures each opaque type a function gives
    /// leaves out (`opaque_output`), by its definition, so the opaque type
    /// has the same arguments wherever it is named.
    opaque_excluded: std::cell::RefCell<BTreeMap<api::identity::Definition, Box<[u32]>>>,
    /// While a trait's method is classified as a member of an opaque type a
    /// function gives: that `impl Trait` type, which the method's `Self` is.
    opaque_self: std::cell::RefCell<Option<model::Type>>,
    /// The opaque types being converted, each by its definition at its
    /// captures, so one their own bounds reach again is left as its
    /// projection, not converted without end: a projection
    /// `opaque_projection` gives, as `A: From<Self::B>` and `B:
    /// From<Self::A>` of one trait reach, or the opaque type itself, as
    /// `impl Number` binds `Add::Output` to it through `Number: Add<Output
    /// = Self>` (#1262). The same definition at other captures is another
    /// type, as `explain`'s output is where its `Self` is `explain`'s
    /// output (#1270).
    opaque_converting: std::cell::RefCell<Vec<(api::identity::Definition, Box<[model::Argument]>)>>,
    /// While a function's output is converted: the associated types each
    /// opaque `impl Trait` type it gives binds, as arguments of that type.
    opaque_associated: std::cell::RefCell<Vec<representability::signature::Opaque>>,
    /// The implementations of each `core` trait, by the trait's name, in
    /// graph order (`core_implementations`).
    core_traits: std::cell::OnceCell<HashMap<&'graph str, Box<[usize]>>>,
    /// `callable_traits`, read once.
    callable_traits: std::cell::OnceCell<Option<CallableTraits>>,
}

/// `core`'s `Fn` and `FnOnce` traits and `FnOnce::Output`.
type CallableTraits = (
    api::identity::Definition,
    api::identity::Definition,
    api::identity::Definition,
);

#[cfg(feature = "rustc-driver")]
pub(crate) const EVIDENCE_RETAINED_ITEMS: usize = 6_000_000;
#[cfg(feature = "rustc-driver")]
pub(crate) const EVIDENCE_RETAINED_BYTES: usize = 3_221_225_472;

#[derive(Clone, Copy)]
struct Authority {
    items: usize,
    bytes: usize,
}

impl Authority {
    const fn production(limits: &crate::limits::Limits) -> Self {
        Self {
            items: limits.classified_contract_items,
            bytes: limits.classified_contract_bytes,
        }
    }

    #[cfg(feature = "rustc-driver")]
    const fn evidence() -> Self {
        Self {
            items: EVIDENCE_RETAINED_ITEMS,
            bytes: EVIDENCE_RETAINED_BYTES,
        }
    }
}

/// Private move-only authority for a scalar-only, nonpublishing classification attempt.
#[cfg(feature = "rustc-driver")]
#[allow(
    dead_code,
    reason = "the private capability is consumed only by explicitly governed evidence attempts"
)]
pub(crate) struct EvidenceCapability {
    graph: api::Graph<api::Extracted>,
}

/// Allocation-free scalar decomposition of one completed classification transaction.
#[cfg(feature = "rustc-driver")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(
    dead_code,
    reason = "scalar fields are read only by the private evidence reporter and focused gates"
)]
pub(crate) struct ClassificationEvidence {
    pub(crate) graph_retained: api::RetainedEvidence,
    pub(crate) graph_items: usize,
    pub(crate) graph_bytes: usize,
    pub(crate) transaction_items: usize,
    pub(crate) transaction_bytes: usize,
    pub(crate) definitions: usize,
    pub(crate) implementations: usize,
    pub(crate) exports: usize,
    pub(crate) support_groups: usize,
    pub(crate) support_edges: usize,
    pub(crate) support_rejections: usize,
    pub(crate) implementation_ledgers: usize,
    pub(crate) associated: usize,
    pub(crate) classifier_new_items: usize,
    pub(crate) final_items: usize,
    pub(crate) final_bytes: usize,
    pub(crate) bytes: ClassificationBytes,
    pub(crate) scratch_high_water_bytes: usize,
    pub(crate) canonical_contract_bytes: usize,
    pub(crate) elapsed: std::time::Duration,
}

/// Exact deep retained byte categories; scratch is deliberately separate.
#[cfg(feature = "rustc-driver")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(
    dead_code,
    reason = "deep byte categories remain private diagnostic scalars"
)]
pub(crate) struct ClassificationBytes {
    pub(crate) graph: usize,
    pub(crate) contract_inline: usize,
    pub(crate) definition_plans: usize,
    pub(crate) outcomes: usize,
    pub(crate) support: usize,
    pub(crate) canonical: usize,
}

#[cfg(feature = "rustc-driver")]
#[derive(Debug, thiserror::Error)]
#[allow(
    dead_code,
    reason = "the private evidence lane preserves process and classifier failure classes"
)]
pub(crate) enum EvidenceError {
    #[error(transparent)]
    Process(#[from] crate::Error),
    #[error(transparent)]
    Classification(#[from] Error),
    #[error("classification scalar evidence does not match retained ownership")]
    Cardinality,
}

enum Failure {
    Reject(representability::reject::Kind),
    Operational(Error),
}

fn argument_late_input(
    argument: &model::Argument,
    binders: &[model::Binder],
) -> Result<bool, representability::reject::Kind> {
    match argument {
        model::Argument::Lifetime(lifetime) => lifetime_late_input(lifetime, binders),
        model::Argument::Type(ty) => type_late_input(ty, binders),
        model::Argument::Const(_) => Ok(false),
    }
}

fn lifetime_late_input(
    lifetime: &model::Lifetime,
    binders: &[model::Binder],
) -> Result<bool, representability::reject::Kind> {
    let model::Lifetime::Late { binder, index } = lifetime else {
        return Ok(false);
    };
    if binders.iter().any(|declared| {
        declared.depth() == binder.depth()
            && declared.index() == binder.index()
            && *index == binder.index()
    }) {
        Ok(true)
    } else {
        Err(representability::reject::Kind::HigherRankedBinder)
    }
}

fn type_late_input(
    ty: &model::Type,
    binders: &[model::Binder],
) -> Result<bool, representability::reject::Kind> {
    match ty {
        model::Type::Reference { lifetime, ty, .. } => {
            Ok(lifetime_late_input(lifetime, binders)? || type_late_input(ty, binders)?)
        }
        model::Type::Definition { arguments, .. } | model::Type::Projection { arguments, .. } => {
            arguments.iter().try_fold(false, |late, argument| {
                argument_late_input(argument, binders).map(|actual| late || actual)
            })
        }
        model::Type::Tuple(types) => types.iter().try_fold(false, |late, ty| {
            type_late_input(ty, binders).map(|actual| late || actual)
        }),
        model::Type::Slice(ty) => type_late_input(ty, binders),
        model::Type::Array { element, .. } | model::Type::RawPointer { ty: element, .. } => {
            type_late_input(element, binders)
        }
        model::Type::Primitive(_)
        | model::Type::Parameter { .. }
        | model::Type::SelfType(_)
        | model::Type::Never => Ok(false),
        model::Type::FunctionPointer(_)
        | model::Type::TraitObject { .. }
        | model::Type::ImplTrait { .. } => Err(representability::reject::Kind::CallableProtocol),
    }
}

/// Whether `argument` is `'static` or a type that names it, as
/// `Cow<'static, str>` does.
fn argument_names_static(argument: &model::Argument) -> bool {
    match argument {
        model::Argument::Lifetime(lifetime) => *lifetime == model::Lifetime::Static,
        model::Argument::Type(ty) => type_names_static(ty),
        model::Argument::Const(_) => false,
    }
}

/// Whether `ty` names `'static`, in a reference or in a type's arguments.
fn type_names_static(ty: &model::Type) -> bool {
    match ty {
        model::Type::Reference { lifetime, ty, .. } => {
            *lifetime == model::Lifetime::Static || type_names_static(ty)
        }
        model::Type::Definition { arguments, .. } | model::Type::Projection { arguments, .. } => {
            arguments.iter().any(argument_names_static)
        }
        model::Type::Tuple(members) => members.iter().any(type_names_static),
        model::Type::Slice(element) | model::Type::Array { element, .. } => {
            type_names_static(element)
        }
        _ => false,
    }
}

/// `ty` with each late-bound lifetime as `'static`: the type a callable's
/// output names, whose borrow the slot that makes it gives its own
/// lifetime (D594).
fn late_as_static(ty: &model::Type) -> model::Type {
    let argument = |argument: &model::Argument| match argument {
        model::Argument::Lifetime(model::Lifetime::Late { .. }) => {
            model::Argument::Lifetime(model::Lifetime::Static)
        }
        model::Argument::Type(ty) => model::Argument::Type(late_as_static(ty)),
        other => other.clone(),
    };
    match ty {
        model::Type::Definition { id, arguments } => model::Type::Definition {
            id: id.clone(),
            arguments: arguments.iter().map(argument).collect(),
        },
        model::Type::Reference {
            lifetime,
            mutability,
            ty,
        } => model::Type::Reference {
            lifetime: match lifetime {
                model::Lifetime::Late { .. } => model::Lifetime::Static,
                other => other.clone(),
            },
            mutability: *mutability,
            ty: Box::new(late_as_static(ty)),
        },
        model::Type::Tuple(members) => {
            model::Type::Tuple(members.iter().map(late_as_static).collect())
        }
        model::Type::Slice(element) => model::Type::Slice(Box::new(late_as_static(element))),
        other => other.clone(),
    }
}

fn callable_projection<'a>(
    projection: &model::Type,
    parameter: u32,
    mut name: impl FnMut(&api::identity::Definition) -> Option<&'a str>,
) -> bool {
    let model::Type::Projection {
        trait_id,
        associated,
        arguments,
    } = projection
    else {
        return false;
    };
    name(trait_id) == Some("FnOnce")
        && name(associated) == Some("Output")
        && matches!(
            arguments.first(),
            Some(model::Argument::Type(model::Type::Parameter { index, .. }))
                if *index == parameter
        )
}

fn type_has_late_lifetime(ty: &model::Type) -> bool {
    fn argument(argument: &model::Argument) -> bool {
        match argument {
            model::Argument::Lifetime(model::Lifetime::Late { .. }) => true,
            model::Argument::Lifetime(_) | model::Argument::Const(_) => false,
            model::Argument::Type(ty) => type_has_late_lifetime(ty),
        }
    }

    match ty {
        model::Type::Definition { arguments, .. } => arguments.iter().any(argument),
        model::Type::Tuple(types) => types.iter().any(type_has_late_lifetime),
        model::Type::Slice(ty) => type_has_late_lifetime(ty),
        model::Type::Array { element, .. } | model::Type::RawPointer { ty: element, .. } => {
            type_has_late_lifetime(element)
        }
        model::Type::Reference { lifetime, ty, .. } => {
            matches!(lifetime, model::Lifetime::Late { .. }) || type_has_late_lifetime(ty)
        }
        model::Type::FunctionPointer(signature) => {
            signature.inputs().any(type_has_late_lifetime)
                || type_has_late_lifetime(signature.output())
        }
        // A projection borrows where its arguments do: iced's
        // `Subscription<P::Message>` borrows nothing.
        model::Type::Projection { arguments, .. } => arguments.iter().any(argument),
        model::Type::TraitObject { .. } | model::Type::ImplTrait { .. } => true,
        model::Type::Primitive(_)
        | model::Type::Parameter { .. }
        | model::Type::SelfType(_)
        | model::Type::Never => false,
    }
}

impl From<representability::reject::Kind> for Failure {
    fn from(kind: representability::reject::Kind) -> Self {
        Self::Reject(kind)
    }
}

impl From<Error> for Failure {
    fn from(error: Error) -> Self {
        Self::Operational(error)
    }
}

impl<'graph, P: api::Phase> Context<'graph, P> {
    fn new(graph: &'graph api::Graph<P>, authority: Authority) -> Result<Self, Error> {
        let mut context = Self {
            graph,
            support: SupportLedger {
                groups: Box::default(),
                edges: Box::default(),
                rejections: Box::default(),
            },
            item_limit: authority.items,
            byte_limit: authority.bytes,
            retained_items: Cell::new(graph.allocation().items),
            retained_bytes: Cell::new(graph.allocation().bytes),
            scratch_bytes: Cell::new(0),
            trait_self: Cell::new(None),
            scope_lifetimes: Cell::new(false),
            instance_aliases: std::cell::OnceCell::new(),
            sole_aliases: std::cell::OnceCell::new(),
            opaque_output: std::cell::RefCell::new(None),
            opaque_excluded: std::cell::RefCell::new(BTreeMap::new()),
            opaque_self: std::cell::RefCell::new(None),
            opaque_converting: std::cell::RefCell::new(Vec::new()),
            opaque_associated: std::cell::RefCell::new(Vec::new()),
            core_traits: std::cell::OnceCell::new(),
            callable_traits: std::cell::OnceCell::new(),
        };
        let initialized = (|| {
            context.retain(
                1,
                std::mem::size_of::<Contract<Classified>>()
                    .checked_add(std::mem::size_of::<SupportLedger>())
                    .ok_or_else(arithmetic)?,
            )?;
            context.retain_items(graph.exports().len())?;
            context.support = context.support_ledger()?;
            Ok(())
        })();
        if initialized.is_err() {
            context.observe_result(&initialized);
        }
        initialized?;
        Ok(context)
    }

    fn observe_result<T>(&self, _result: &Result<T, Error>) {}

    /// Charge classified retained items and bytes; only checked overflow is
    /// terminal.
    fn retain(&self, items: usize, bytes: usize) -> Result<(), Error> {
        let actual_items = self
            .retained_items
            .get()
            .checked_add(items)
            .ok_or_else(arithmetic)?;
        let actual_bytes = self
            .retained_bytes
            .get()
            .checked_add(bytes)
            .ok_or_else(arithmetic)?;
        self.retained_items.set(actual_items);
        self.retained_bytes.set(actual_bytes);
        Ok(())
    }

    fn retain_items(&self, additional: usize) -> Result<(), Error> {
        self.retain(additional, 0)
    }

    fn retain_array<T>(&self, count: usize) -> Result<(), Error> {
        let bytes = std::mem::size_of::<T>()
            .checked_mul(count)
            .ok_or_else(arithmetic)?;
        self.retain(0, bytes)
    }

    /// Keep the scratch peak.
    fn scratch_arrays<T>(&self, count: usize, arrays: usize) -> Result<(), Error> {
        let bytes = std::mem::size_of::<T>()
            .checked_mul(count)
            .and_then(|bytes| bytes.checked_mul(arrays))
            .ok_or_else(arithmetic)?;
        self.scratch_bytes.set(self.scratch_bytes.get().max(bytes));
        Ok(())
    }

    fn retain_str(&self, value: &str) -> Result<(), Error> {
        self.retain(0, value.len())
    }

    fn retain_span(&self, span: &model::Span) -> Result<(), Error> {
        self.retain(0, span.retained_bytes())
    }

    fn retain_export(&self, export: &api::identity::Export) -> Result<(), Error> {
        let bytes = export.retained_bytes().ok_or(Error::Limit {
            resource: "classified retained bytes",
            limit: self.byte_limit,
            actual: usize::MAX,
        })?;
        self.retain(0, bytes)
    }

    fn retain_selector(&self, selector: &crate::surface::identity::Selector) -> Result<(), Error> {
        let bytes = selector.retained_bytes().ok_or(Error::Limit {
            resource: "classified retained bytes",
            limit: self.byte_limit,
            actual: usize::MAX,
        })?;
        self.retain(0, bytes)
    }

    fn retain_api_definition(&self, definition: &api::identity::Definition) -> Result<(), Error> {
        let bytes = definition.retained_bytes().ok_or(Error::Limit {
            resource: "classified retained bytes",
            limit: self.byte_limit,
            actual: usize::MAX,
        })?;
        self.retain(0, bytes)
    }

    fn retain_ledger_reason(&self, reason: &ledger::Reason) -> Result<(), Error> {
        match reason {
            ledger::Reason::ExternalAuthorityUnavailable {
                crate_name,
                stable_crate_id,
            } => self.retain(
                0,
                crate_name
                    .len()
                    .checked_add(stable_crate_id.len())
                    .ok_or(Error::Limit {
                        resource: "classified retained bytes",
                        limit: self.byte_limit,
                        actual: usize::MAX,
                    })?,
            ),
            ledger::Reason::UnnameablePrivate(definition)
            | ledger::Reason::UnresolvedDefinition(definition) => {
                self.retain_api_definition(definition)
            }
            ledger::Reason::UnknownCompilerShape {
                definition,
                compiler_kind,
            } => {
                self.retain_api_definition(definition)?;
                self.retain_str(compiler_kind)
            }
            ledger::Reason::UnsupportedPublicPath { compiler_kind } => {
                self.retain_str(compiler_kind)
            }
            ledger::Reason::InvalidAliasChain | ledger::Reason::ConflictingIdentity => Ok(()),
        }
    }

    fn retain_inline_representability(&self) -> Result<(), Error> {
        self.retain(0, std::mem::size_of::<representability::Representability>())
    }

    fn retain_outcome_common(
        &self,
        export: &api::identity::Export,
        name: &str,
    ) -> Result<(), Error> {
        self.retain_export(export)?;
        self.retain_str(name)
    }

    fn retain_form(&self, fields: &model::FieldSet) -> Result<(), Error> {
        if !matches!(fields.form(), model::FieldForm::Record) {
            return Ok(());
        }
        self.retain_array::<crate::surface::identity::Field>(fields.public_fields().len())?;
        for field in fields.public_fields() {
            self.retain_str(field.name())?;
        }
        Ok(())
    }

    /// A type's generic parameters. Its bounds (`Tz: TimeZone`) hold or
    /// fail where rustc compiles the bridge at the concrete types.
    fn parameters(
        &self,
        generics: &model::Generics,
    ) -> Result<Box<[representability::signature::Parameter]>, Failure> {
        self.declared_parameters(generics, false)
    }

    /// The generic parameters. `function` admits a function's argument
    /// `impl Trait`, which no type has.
    fn declared_parameters(
        &self,
        generics: &model::Generics,
        function: bool,
    ) -> Result<Box<[representability::signature::Parameter]>, Failure> {
        // A lifetime parameter is instantiated at `'static` (D558 tier 0):
        // it is not declared, and `function` refuses a signature whose
        // inputs borrow at it.
        for parameter in generics
            .parameters()
            .filter(|parameter| !is_lifetime_parameter(parameter))
        {
            // An argument's `impl Fn(..)` takes a script closure, and no
            // signature names it. Any other argument's `impl Trait` is a
            // type parameter the call's argument fills, as `<T: Trait>` is
            // (`argument_impl_name`).
            if !facade_segment_nameable(parameter.name())
                && !(function && parameter.name().starts_with("impl "))
            {
                return Err(representability::reject::Kind::UnnameableType.into());
            }
        }
        self.retain_array::<representability::signature::Parameter>(generics.parameters().len())?;
        for parameter in generics.parameters() {
            self.retain_str(parameter.name())?;
        }
        let mut parameters = Vec::new();
        parameters
            .try_reserve_exact(generics.parameters().len())
            .map_err(|_| Error::Allocation)?;
        for parameter in generics
            .parameters()
            .filter(|parameter| !is_lifetime_parameter(parameter))
        {
            let (kind, default) = match parameter.kind() {
                model::ParameterKind::Type { default, .. } => {
                    (representability::signature::Kind::Type, default)
                }
                model::ParameterKind::Const { .. } => (
                    representability::signature::Kind::Constant,
                    &model::Default::Absent,
                ),
                model::ParameterKind::Lifetime { .. } => {
                    unreachable!("lifetime parameters are filtered above")
                }
            };
            // A default Rawr cannot hold leaves the argument required.
            let default = match default {
                model::Default::Present(ty) => match self.convert(ty) {
                    Ok(ty) => Some(ty),
                    Err(Failure::Reject(_)) => None,
                    Err(error) => return Err(error),
                },
                model::Default::Absent => None,
            };
            let declared = representability::signature::Parameter::new(
                parameter.index(),
                parameter.name().into(),
                kind,
            );
            let declared = if parameter.name().starts_with("impl ")
                && !self.callable_bound(generics, parameter.index())
            {
                declared.into_argument_impl()
            } else {
                declared
            };
            parameters.push(declared.with_default(default));
        }
        Ok(parameters.into_boxed_slice())
    }

    /// Whether `generics` bound the parameter at `index` by `Fn`, `FnMut`
    /// or `FnOnce`.
    fn callable_bound(&self, generics: &model::Generics, index: u32) -> bool {
        generics.predicates().any(|predicate| {
            matches!(
                predicate,
                model::Predicate::Bound {
                    ty: model::Type::Parameter { index: bounded, .. },
                    bound: model::Bound::Trait { trait_id, .. },
                } if *bounded == index
                    && matches!(self.definition_name(trait_id), Some("Fn" | "FnMut" | "FnOnce"))
            )
        })
    }

    fn function_parameters(
        &self,
        generics: &model::Generics,
    ) -> Result<FunctionParameters, Failure> {
        let parameters = self.declared_parameters(generics, true)?;
        if generics.predicates().len() == 0 {
            return Ok(FunctionParameters {
                parameters,
                callable: None,
                binders: Box::default(),
                predicates: Box::default(),
                call: None,
                crossings: Vec::new(),
            });
        }
        let mut bounds = Vec::new();
        for predicate in generics.predicates() {
            let model::Predicate::Bound {
                ty: model::Type::Parameter { index, .. },
                bound:
                    model::Bound::Trait {
                        trait_id,
                        arguments,
                        polarity: model::Polarity::Positive,
                        binders,
                    },
            } = predicate
            else {
                continue;
            };
            if matches!(
                self.definition_name(trait_id),
                Some("Fn" | "FnMut" | "FnOnce")
            ) {
                bounds.push((*index, arguments.as_ref(), binders.as_ref()));
            }
        }
        // A script function crosses into every callable parameter as a
        // slot (D594): several callables, a `&mut` input, or an output that
        // borrows an input, as `iced::run(update, view)` takes them.
        let crossings = self.crossings(generics, &bounds);
        let crossed = |parameters: Box<[representability::signature::Parameter]>,
                       crossings: Vec<Crossing>|
         -> Option<FunctionParameters> {
            let mut parameters = parameters;
            for (index, ..) in &crossings {
                let parameter = parameters
                    .iter_mut()
                    .find(|candidate| candidate.index() == *index)?;
                if parameter.kind() != representability::signature::Kind::Type {
                    return None;
                }
                parameter.mark_callable();
            }
            Some(FunctionParameters {
                parameters,
                callable: None,
                binders: Box::default(),
                predicates: Box::default(),
                call: None,
                crossings,
            })
        };
        if bounds.len() > 1 {
            return crossed(parameters, crossings?)
                .ok_or_else(|| representability::reject::Kind::CallableProtocol.into());
        }
        match self.single_callable(generics, parameters.clone()) {
            Ok(mut single) => {
                single.crossings = crossings.unwrap_or_default();
                Ok(single)
            }
            Err(Failure::Reject(kind)) => crossings
                .ok()
                .filter(|crossings| !crossings.is_empty())
                .and_then(|crossings| crossed(parameters, crossings))
                .ok_or(Failure::Reject(kind)),
            Err(error) => Err(error),
        }
    }

    /// Each callable parameter's inputs and output, as a script function
    /// fills it by a slot (D594): its inputs may borrow, shared or `&mut`,
    /// at the bound's own lifetimes, and its output may borrow at them, as
    /// `for<'a> Fn(&'a S) -> Element<'a, M>` does. Any other predicate
    /// holds or fails where rustc compiles the call.
    fn crossings(
        &self,
        generics: &model::Generics,
        bounds: &[(u32, &[model::Argument], &[model::Binder])],
    ) -> Result<Vec<Crossing>, Failure> {
        let mut crossings = Vec::new();
        for (parameter, arguments, binders) in bounds {
            let [model::Argument::Type(model::Type::Tuple(inputs))] = arguments else {
                return Err(representability::reject::Kind::CallableProtocol.into());
            };
            for argument in arguments.iter() {
                argument_late_input(argument, binders).map_err(Failure::Reject)?;
            }
            let returned = generics.predicates().find_map(|predicate| match predicate {
                model::Predicate::ProjectionEquality { projection, ty }
                    if callable_projection(projection, *parameter, |trait_id| {
                        self.definition_name(trait_id)
                    }) =>
                {
                    Some(ty)
                }
                _ => None,
            });
            let Some(returned) = returned else {
                return Err(representability::reject::Kind::CallableProtocol.into());
            };
            // A script function that returns a future or a sipper, as a
            // closure that does (`single_callable`, #545).
            if self.future_bounded(generics, returned) {
                return Err(representability::reject::Kind::FutureClosure.into());
            }
            // An output borrows only at a lifetime its own bound declares,
            // which an input takes.
            let borrows = type_has_late_lifetime(returned);
            if borrows && type_late_input(returned, binders).is_err() {
                return Err(representability::reject::Kind::EscapingBorrow.into());
            }
            crossings.push((*parameter, inputs.clone(), returned.clone(), borrows));
        }
        Ok(crossings)
    }

    fn single_callable(
        &self,
        generics: &model::Generics,
        mut parameters: Box<[representability::signature::Parameter]>,
    ) -> Result<FunctionParameters, Failure> {
        let mut selected = None;
        for predicate in generics.predicates() {
            let model::Predicate::Bound {
                ty: model::Type::Parameter { index, .. },
                bound:
                    model::Bound::Trait {
                        trait_id,
                        arguments,
                        polarity: model::Polarity::Positive,
                        binders,
                    },
            } = predicate
            else {
                continue;
            };
            let Some(name @ ("Fn" | "FnMut" | "FnOnce")) = self.definition_name(trait_id) else {
                continue;
            };
            if selected.is_some() {
                return Err(representability::reject::Kind::CallableProtocol.into());
            }
            selected = Some((*index, name, arguments.as_ref(), binders.as_ref()));
        }
        let Some((parameter, class, arguments, declared_binders)) = selected else {
            let mut predicates = Vec::new();
            self.retain_array::<representability::predicate::Bound>(generics.predicates().len())?;
            predicates
                .try_reserve_exact(generics.predicates().len())
                .map_err(|_| Error::Allocation)?;
            for predicate in generics.predicates() {
                // Any other bound (`R: SampleRange<T>`, `StandardUniform:
                // Distribution<T>`, an outlives bound) holds or fails where
                // rustc compiles the bridge's call at the concrete types.
                let model::Predicate::Bound {
                    ty: model::Type::Parameter { index, .. },
                    bound:
                        model::Bound::Trait {
                            trait_id,
                            arguments,
                            polarity: model::Polarity::Positive,
                            binders,
                        },
                } = predicate
                else {
                    continue;
                };
                if !binders.is_empty()
                    || arguments.iter().any(|argument| {
                        !matches!(
                            argument,
                            model::Argument::Type(model::Type::Parameter {
                                index: argument,
                                ..
                            }) if argument == index
                        )
                    })
                {
                    continue;
                }
                if self.graph.definition_index(trait_id).is_none() {
                    continue;
                }
                predicates.push(representability::predicate::Bound::new(
                    *index,
                    crate::surface::identity::Definition::of(trait_id),
                ));
            }
            return Ok(FunctionParameters {
                parameters,
                callable: None,
                binders: Box::default(),
                predicates: predicates.into_boxed_slice(),
                call: None,
                crossings: Vec::new(),
            });
        };
        let callable_parameter = parameters
            .iter_mut()
            .find(|candidate| candidate.index() == parameter)
            .ok_or(representability::reject::Kind::CallableProtocol)?;
        if callable_parameter.kind() != representability::signature::Kind::Type {
            return Err(representability::reject::Kind::CallableProtocol.into());
        }
        callable_parameter.mark_callable();
        let late_bound = arguments
            .iter()
            .try_fold(false, |late, argument| {
                argument_late_input(argument, declared_binders).map(|actual| late || actual)
            })
            .map_err(Failure::Reject)?;
        let mut binders = Vec::new();
        self.retain_array::<crate::surface::instance::Binder>(declared_binders.len())?;
        binders
            .try_reserve_exact(declared_binders.len())
            .map_err(|_| Error::Allocation)?;
        for binder in declared_binders {
            binders.push(crate::surface::instance::Binder::normalized(
                binder.depth(),
                binder.index(),
            ));
        }
        let mut static_owned = false;
        let mut output = false;
        let mut returned = None;
        for predicate in generics.predicates() {
            match predicate {
                model::Predicate::Bound {
                    ty: model::Type::Parameter { .. },
                    bound:
                        model::Bound::Trait {
                            polarity: model::Polarity::Positive,
                            binders,
                            ..
                        },
                } if binders.is_empty() => {}
                model::Predicate::Bound {
                    ty: model::Type::Parameter { index, .. },
                    bound:
                        model::Bound::Trait {
                            trait_id,
                            polarity: model::Polarity::Positive,
                            ..
                        },
                } if *index == parameter
                    && matches!(
                        self.definition_name(trait_id),
                        Some("Fn" | "FnMut" | "FnOnce")
                    ) => {}
                model::Predicate::ProjectionEquality { projection, ty }
                    if callable_projection(projection, parameter, |trait_id| {
                        self.definition_name(trait_id)
                    }) =>
                {
                    if type_has_late_lifetime(ty) {
                        return Err(representability::reject::Kind::EscapingBorrow.into());
                    }
                    output = true;
                    returned = Some(ty.clone());
                }
                // `F: 'a` of an early `'a`, which is `'static` (D558).
                model::Predicate::TypeOutlives {
                    ty: model::Type::Parameter { index, .. },
                    lifetime: model::Lifetime::Static | model::Lifetime::Early { .. },
                } if *index == parameter => static_owned = true,
                // `R: 'static` of another parameter, as `spawn_blocking`'s
                // output has, or `Message: 'a` of an early `'a`, which is
                // `'static` (D558): rustc checks it where the bridge names
                // `R`.
                model::Predicate::TypeOutlives {
                    ty: model::Type::Parameter { .. },
                    lifetime: model::Lifetime::Static | model::Lifetime::Early { .. },
                } => {}
                // A default method's bound on `Self`, as Freya's `on_press`
                // has `where Self: AccessibilityExt`: rustc checks it where
                // the bridge calls the method on its type.
                model::Predicate::Bound {
                    ty: model::Type::SelfType(_),
                    ..
                } => {}
                // That bound once an alias's instance is `Self`, as
                // `iced::Renderer` is the `Self: Renderer` of
                // `with_translation`: a bound on a concrete type, which
                // rustc checks where the bridge names it.
                model::Predicate::Bound {
                    ty: ty @ model::Type::Definition { .. },
                    ..
                } if !names_type_parameter(ty) => {}
                // A bound on an associated type, as iced's `style` has
                // `where Theme::Class<'a>: From<StyleFn<'a, Theme>>`:
                // rustc checks it where the bridge calls the method.
                model::Predicate::Bound {
                    ty: model::Type::Projection { .. },
                    ..
                } => {}
                _ => return Err(representability::reject::Kind::CallableProtocol.into()),
            }
        }
        if !output {
            return Err(representability::reject::Kind::CallableProtocol.into());
        }
        // A closure that returns a future or a sipper, as `sipper(|sender|
        // async move { .. })` takes: the script's `async` block has no Rust
        // type the runner can write (#545).
        if returned
            .as_ref()
            .is_some_and(|returned| self.future_bounded(generics, returned))
        {
            return Err(representability::reject::Kind::FutureClosure.into());
        }
        // A callable with no `'static` bound is borrowed for the call, which
        // ends before it can be called again: an `FnMut` one, as image's
        // `ImageBuffer::from_fn(w, h, |x, y| ..)` takes, is called through
        // the same borrow as an `Fn` one, and may change what it captures.
        let class = match (class, static_owned) {
            ("Fn" | "FnMut", false) => crate::surface::callable::Class::Borrowed(
                crate::surface::callable::borrowed::Proof::input_only(late_bound),
            ),
            ("Fn", true) => crate::surface::callable::Class::Shared(
                crate::surface::callable::shared::Proof::static_owned(
                    crate::surface::callable::shared::Scope::Epoch,
                ),
            ),
            ("FnMut", true) => crate::surface::callable::Class::Mutable(
                crate::surface::callable::mutable::Proof::static_owned(),
            ),
            ("FnOnce", _) => crate::surface::callable::Class::Once(
                crate::surface::callable::once::Proof::static_owned(),
            ),
            _ => return Err(representability::reject::Kind::CallableProtocol.into()),
        };
        // `Fn(A, B)` is written `Fn<(A, B)>`: its one argument is the tuple
        // of the callable's inputs.
        let inputs = match arguments {
            [model::Argument::Type(model::Type::Tuple(inputs))] => Some(inputs.clone()),
            _ => None,
        };
        Ok(FunctionParameters {
            parameters,
            callable: Some(class),
            binders: binders.into_boxed_slice(),
            predicates: Box::default(),
            call: inputs
                .zip(returned)
                .map(|(inputs, returned)| (parameter, inputs, returned)),
            crossings: Vec::new(),
        })
    }

    /// `generics` with an `impl Into<T>` parameter read as the callable that
    /// `T`'s one callable `From` implementation converts from, as Freya's
    /// `WindowConfig::new(impl Into<AppComponent>)` with `impl<F: Fn() -> E +
    /// 'static, E: IntoElement> From<F> for AppComponent`, or its `on_press(impl
    /// Into<EventHandler<Event<PressEventData>>>)` with `impl<H: FnMut(D) +
    /// 'static, D> From<H> for EventHandler<D>`. The parameter takes that
    /// implementation's bounds. Its parameters that `T` fixes, as `D`, are
    /// `T`'s arguments, and one more, as `E`, is a synthetic parameter the
    /// instance infers, as a future's `F::Output` is (D527). `None` when
    /// nothing is read so.
    /// `generics` with each parameter `P: Trait<A..>` of a crate's trait
    /// that one blanket implementation `impl<T: Fn(..) -> R, ..> Trait<..>
    /// for T` gives to callables read as that callable: `P` takes the
    /// implementation's `Fn` bound and its other bounds, at the arguments
    /// `A..`, and the one other parameter the implementation declares, as
    /// `R` or a `W` its output is, is a synthetic parameter the call infers
    /// (D594). iced's `UpdateFn` and `ViewFn` are read so. `None` when no
    /// parameter is.
    ///
    /// Also the parameters whose trait the crate implements for a type of
    /// its own too, as iced's `ThemeFn` is for `Theme`. With `values`, such
    /// a parameter keeps its bound, so a value of a type that implements the
    /// trait fills it, as rustc fills it.
    fn blanket_callables(
        &self,
        generics: &model::Generics,
        values: bool,
    ) -> (Option<model::Generics>, Vec<u32>) {
        let mut parameters = generics.parameters().cloned().collect::<Vec<_>>();
        let mut predicates = Vec::new();
        let mut changed = false;
        let mut valued = Vec::new();
        for predicate in generics.predicates() {
            let model::Predicate::Bound {
                ty:
                    model::Type::Parameter {
                        owner,
                        index: parameter,
                    },
                bound:
                    model::Bound::Trait {
                        trait_id,
                        arguments,
                        polarity: model::Polarity::Positive,
                        ..
                    },
            } = predicate
            else {
                predicates.push(predicate.clone());
                continue;
            };
            if self.graph.definition_index(trait_id).is_none()
                || matches!(
                    self.definition_name(trait_id),
                    Some("Fn" | "FnMut" | "FnOnce")
                )
            {
                predicates.push(predicate.clone());
                continue;
            }
            let Some(implementation) = self.callable_blanket(trait_id) else {
                predicates.push(predicate.clone());
                continue;
            };
            let implemented_for_types = self.graph.implementations_of(trait_id).any(|other| {
                matches!(other.trait_ref(), model::Trait::Trait { id, .. } if id == trait_id)
                    && !matches!(other.self_type(),
                        model::Type::Parameter { owner, .. } if owner == other.id())
            });
            if values && implemented_for_types {
                valued.push(*parameter);
                predicates.push(predicate.clone());
                continue;
            }
            let model::Trait::Trait {
                arguments: implemented,
                ..
            } = implementation.trait_ref()
            else {
                predicates.push(predicate.clone());
                continue;
            };
            let model::Type::Parameter { index: this, .. } = implementation.self_type() else {
                predicates.push(predicate.clone());
                continue;
            };
            // What the bound gives each of the implementation's parameters.
            let mut given = vec![(
                *this,
                model::Type::Parameter {
                    owner: owner.clone(),
                    index: *parameter,
                },
            )];
            for (implemented, argument) in implemented.iter().skip(1).zip(arguments.iter()) {
                if let (
                    model::Argument::Type(model::Type::Parameter { index, .. }),
                    model::Argument::Type(argument),
                ) = (implemented, argument)
                {
                    given.push((*index, argument.clone()));
                }
            }
            let mut others = implementation.generics().parameters().filter(|candidate| {
                matches!(candidate.kind(), model::ParameterKind::Type { .. })
                    && !given.iter().any(|(index, _)| *index == candidate.index())
            });
            let other = others.next();
            if others.next().is_some() {
                predicates.push(predicate.clone());
                continue;
            }
            let synthetic = representability::signature::future_output(*parameter);
            let renamed = |index: u32| {
                given
                    .iter()
                    .find(|(given, _)| *given == index)
                    .map(|(_, ty)| ty.clone())
                    .or_else(|| {
                        other.is_some_and(|other| other.index() == index).then(|| {
                            model::Type::Parameter {
                                owner: owner.clone(),
                                index: synthetic,
                            }
                        })
                    })
            };
            let relabeled = implementation
                .generics()
                .predicates()
                .map(|predicate| relabel_predicate(predicate, implementation.id(), &renamed))
                .collect::<Option<Vec<_>>>();
            let Some(relabeled) = relabeled else {
                predicates.push(predicate.clone());
                continue;
            };
            predicates.extend(relabeled);
            if let Some(other) = other {
                // Two traits' blanket impls may each name theirs `C`, as
                // iced's `BootFn` and `UpdateFn` do: each is its own.
                let mut name = other.name().to_owned();
                let mut suffix = 1_u32;
                while parameters.iter().any(|parameter| parameter.name() == name) {
                    suffix += 1;
                    name = format!("{}{suffix}", other.name());
                }
                parameters.push(model::Parameter::new(
                    owner.clone(),
                    synthetic,
                    name.into(),
                    model::ParameterKind::Type {
                        bounds: Box::default(),
                        default: model::Default::Absent,
                    },
                ));
            }
            changed = true;
            if implemented_for_types {
                valued.push(*parameter);
            }
        }
        (
            changed.then(|| {
                model::Generics::new(parameters.into_boxed_slice(), predicates.into_boxed_slice())
            }),
            valued,
        )
    }

    /// The inputs and output of an opaque type's `Fn(A..) -> B` bound, as
    /// `impl Fn(B) -> O` binds them: `None` without one, or when one of
    /// them doesn't convert.
    fn opaque_call(
        &self,
        bounds: &[model::Predicate],
    ) -> Option<(
        Vec<representability::convert::Type>,
        representability::convert::Type,
    )> {
        let inputs = bounds.iter().find_map(|predicate| match predicate {
            model::Predicate::Bound {
                bound:
                    model::Bound::Trait {
                        trait_id,
                        arguments,
                        polarity: model::Polarity::Positive,
                        binders,
                    },
                ..
            } if binders.is_empty() && self.is_core_trait(trait_id, "Fn") => {
                match arguments.first() {
                    Some(model::Argument::Type(model::Type::Tuple(inputs))) => Some(inputs),
                    _ => None,
                }
            }
            _ => None,
        })?;
        let output = bounds.iter().find_map(|predicate| match predicate {
            model::Predicate::ProjectionEquality {
                projection: model::Type::Projection { trait_id, .. },
                ty,
            } if self.is_core_trait(trait_id, "FnOnce") => Some(ty),
            _ => None,
        })?;
        let inputs = inputs
            .iter()
            .map(|input| self.convert(input).ok())
            .collect::<Option<Vec<_>>>()?;
        Some((inputs, self.convert(output).ok()?))
    }

    /// The one implementation of the crate trait `trait_id` for every
    /// callable, as iced's `impl<F, A, B, O> Function<A, B, O> for F where
    /// F: Fn(A, B) -> O`.
    fn callable_blanket(
        &self,
        trait_id: &api::identity::Definition,
    ) -> Option<&model::Implementation> {
        let mut found = self
            .graph
            .implementations_of(trait_id)
            .filter(|implementation| {
                let model::Trait::Trait { id, .. } = implementation.trait_ref() else {
                    return false;
                };
                let model::Type::Parameter { owner, index } = implementation.self_type() else {
                    return false;
                };
                id == trait_id
                    && owner == implementation.id()
                    && implementation.generics().predicates().any(|predicate| {
                        matches!(
                            predicate,
                            model::Predicate::Bound {
                                ty: model::Type::Parameter { index: bounded, .. },
                                bound: model::Bound::Trait { trait_id, .. },
                            } if bounded == index
                                && matches!(
                                    self.definition_name(trait_id),
                                    Some("Fn" | "FnMut" | "FnOnce")
                                )
                        )
                    })
            });
        let (Some(implementation), None) = (found.next(), found.next()) else {
            return None;
        };
        Some(implementation)
    }

    /// `core::iter::Iterator`, when the crate trait `trait_id` is one only
    /// iterators have, as itertools' `trait Itertools: Iterator`, or has an
    /// implementation for every type parameter bounded by it, as its
    /// `impl<T: Iterator + ?Sized> Itertools for T`. rustc holds the
    /// program's iterator to the trait where the runner calls its method.
    fn iterator_blanket(
        &self,
        trait_id: &api::identity::Definition,
    ) -> Option<api::identity::Definition> {
        let supertrait = self
            .graph
            .definition_index(trait_id)
            .and_then(|index| self.graph.definitions().nth(index))
            .and_then(|definition| match definition.kind() {
                model::Kind::Trait { supertraits, .. } => {
                    supertraits.iter().find_map(|predicate| match predicate {
                        model::Predicate::Bound {
                            bound:
                                model::Bound::Trait {
                                    trait_id: bound,
                                    polarity: model::Polarity::Positive,
                                    ..
                                },
                            ..
                        } if self.is_core_trait(bound, "Iterator") => Some(bound.clone()),
                        _ => None,
                    })
                }
                _ => None,
            });
        if supertrait.is_some() {
            return supertrait;
        }
        self.graph
            .implementations_of(trait_id)
            .find_map(|implementation| {
                let model::Trait::Trait { id, .. } = implementation.trait_ref() else {
                    return None;
                };
                let model::Type::Parameter { owner, index } = implementation.self_type() else {
                    return None;
                };
                if id != trait_id
                    || owner != implementation.id()
                    || implementation.polarity() != model::Polarity::Positive
                {
                    return None;
                }
                implementation
                    .generics()
                    .predicates()
                    .find_map(|predicate| match predicate {
                        model::Predicate::Bound {
                            ty: model::Type::Parameter { index: bounded, .. },
                            bound:
                                model::Bound::Trait {
                                    trait_id: bound,
                                    polarity: model::Polarity::Positive,
                                    ..
                                },
                        } if bounded == index && self.is_core_trait(bound, "Iterator") => {
                            Some(bound.clone())
                        }
                        _ => None,
                    })
            })
    }

    /// `<T as Trait<..>>::Name` as the trait's one implementation for every
    /// callable sets it (`callable_blanket`), at the projection's self type
    /// and arguments: the opaque `impl Fn(B) -> O` iced's `Function::with`
    /// returns, where the call fills its `Self` with a callable. `None`
    /// when the trait has no such implementation.
    fn through_callable_blanket(&self, ty: &model::Type) -> Option<model::Type> {
        let model::Type::Projection {
            trait_id,
            associated,
            arguments,
        } = ty
        else {
            return None;
        };
        let implementation = self.callable_blanket(trait_id)?;
        let name = self
            .graph
            .definition_index(associated)
            .and_then(|index| self.graph.definitions().nth(index))
            .and_then(|definition| definition.display_path().last())?;
        let value = self.associated_type(implementation, name)?;
        let model::Trait::Trait {
            arguments: implemented,
            ..
        } = implementation.trait_ref()
        else {
            return None;
        };
        let given = implemented
            .iter()
            .zip(arguments.iter())
            .filter_map(|pair| match pair {
                (
                    model::Argument::Type(model::Type::Parameter { index, .. }),
                    model::Argument::Type(argument),
                ) => Some((*index, argument.clone())),
                _ => None,
            })
            .collect::<Vec<_>>();
        relabel_type(value, implementation.id(), &|index| {
            given
                .iter()
                .find(|(given, _)| *given == index)
                .map(|(_, ty)| ty.clone())
        })
    }

    fn callable_conversion(&self, generics: &model::Generics) -> Option<model::Generics> {
        let callable = |predicate: &model::Predicate| {
            matches!(
                predicate,
                model::Predicate::Bound {
                    bound: model::Bound::Trait { trait_id, .. },
                    ..
                } if matches!(self.definition_name(trait_id), Some("Fn" | "FnMut" | "FnOnce"))
            )
        };
        if generics.predicates().any(callable) {
            return None;
        }
        let (into, owner, parameter, target) =
            generics
                .predicates()
                .enumerate()
                .find_map(|(position, predicate)| {
                    let model::Predicate::Bound {
                        ty: model::Type::Parameter { owner, index },
                        bound:
                            model::Bound::Trait {
                                trait_id,
                                arguments,
                                polarity: model::Polarity::Positive,
                                binders,
                            },
                    } = predicate
                    else {
                        return None;
                    };
                    let [model::Argument::Type(target @ model::Type::Definition { .. })] =
                        arguments.as_ref()
                    else {
                        return None;
                    };
                    (binders.is_empty() && self.is_core_trait(trait_id, "Into"))
                        .then_some((position, owner, *index, target))
                })?;
        // The implementations of `From<F>` for `T` whose `F` is a callable,
        // with what `T` gives their parameters.
        let mut found = self
            .core_implementations("From")
            .filter_map(|implementation| {
                let model::Trait::Trait { id, arguments } = implementation.trait_ref() else {
                    return None;
                };
                let [_, model::Argument::Type(model::Type::Parameter { index: source, .. })] =
                    arguments.as_ref()
                else {
                    return None;
                };
                if !self.is_core_trait(id, "From")
                    || !implementation.generics().predicates().any(|predicate| {
                        matches!(
                            predicate,
                            model::Predicate::Bound {
                                ty: model::Type::Parameter { index: bound, .. },
                                ..
                            } if bound == source
                        ) && callable(predicate)
                    })
                {
                    return None;
                }
                let mut fixed = Vec::new();
                match_self_type(
                    implementation.self_type(),
                    target,
                    implementation.id(),
                    &mut fixed,
                )
                .then_some((implementation, *source, fixed))
            });
        let (implementation, source, fixed) = found.next()?;
        if found.next().is_some() {
            return None;
        }
        // `F` is the parameter, `T` fixes others, and at most one more, `E`,
        // is synthetic.
        let mut others = implementation.generics().parameters().filter(|candidate| {
            candidate.index() != source
                && !fixed.iter().any(|(index, _)| *index == candidate.index())
        });
        let other = others.next();
        if others.next().is_some()
            || other.is_some_and(|other| !matches!(other.kind(), model::ParameterKind::Type { .. }))
        {
            return None;
        }
        let synthetic = representability::signature::future_output(parameter);
        let renamed = |index: u32| {
            if index == source {
                Some(model::Type::Parameter {
                    owner: owner.clone(),
                    index: parameter,
                })
            } else if other.is_some_and(|other| other.index() == index) {
                Some(model::Type::Parameter {
                    owner: owner.clone(),
                    index: synthetic,
                })
            } else {
                fixed
                    .iter()
                    .find(|(fixed, _)| *fixed == index)
                    .map(|(_, ty)| ty.clone())
            }
        };
        let mut predicates = generics
            .predicates()
            .enumerate()
            .filter(|(position, _)| *position != into)
            .map(|(_, predicate)| predicate.clone())
            .collect::<Vec<_>>();
        for predicate in implementation.generics().predicates() {
            predicates.push(relabel_predicate(predicate, implementation.id(), &renamed)?);
        }
        let mut parameters = generics.parameters().cloned().collect::<Vec<_>>();
        if let Some(other) = other {
            parameters.push(model::Parameter::new(
                owner.clone(),
                synthetic,
                other.name().into(),
                model::ParameterKind::Type {
                    bounds: Box::default(),
                    default: model::Default::Absent,
                },
            ));
        }
        Some(model::Generics::new(
            parameters.into_boxed_slice(),
            predicates.into_boxed_slice(),
        ))
    }

    /// Each projection `W::Assoc` of a type parameter `W` an input names,
    /// other than a future's `Output`, as `render_stateful_widget`'s `&mut
    /// W::State`, or a callable parameter's bound names in its inputs or
    /// output, as iced's `Application<P>::subscription(f: impl Fn(&P::State)
    /// -> Subscription<P::Message>)`, or what a parameter's bound binds, as
    /// `presets(impl IntoIterator<Item = Preset<P::State, P::Message>>)`:
    /// each with the synthetic type parameter that stands for it, numbered
    /// per parameter.
    fn projections(
        &self,
        generics: &model::Generics,
        signature: &model::Signature,
    ) -> Vec<Projected> {
        use representability::signature;
        /// `deep`: also inside a type's arguments, as a callable's
        /// `Subscription<P::Message>` names it.
        fn find(ty: &model::Type, deep: bool, found: &mut Vec<(u32, Chain)>) {
            match ty {
                // A projection of a parameter, or of a projection of one,
                // as palette's `<<S as RgbStandard>::Space as RgbSpace>::
                // WhitePoint`.
                model::Type::Projection { .. } => {
                    if let Some((index, chain)) = projection_chain(ty)
                        && !found.iter().any(|(known, known_chain)| {
                            *known == index
                                && known_chain.len() == chain.len()
                                && known_chain
                                    .iter()
                                    .zip(&chain)
                                    .all(|((_, named), (_, associated))| named == associated)
                        })
                    {
                        found.push((index, chain));
                    }
                }
                model::Type::Reference { ty, .. } => find(ty, deep, found),
                model::Type::Definition { arguments, .. } if deep => {
                    for argument in arguments.iter() {
                        if let model::Argument::Type(ty) = argument {
                            find(ty, deep, found);
                        }
                    }
                }
                model::Type::Tuple(members) if deep => {
                    for member in members.iter() {
                        find(member, deep, found);
                    }
                }
                model::Type::Slice(element) | model::Type::Array { element, .. } if deep => {
                    find(element, deep, found);
                }
                // What an opaque type's bounds bind, as `.subscription(..)`
                // gives `impl Program<State = P::State>`.
                model::Type::ImplTrait { bounds, .. } if deep => {
                    for predicate in bounds.iter() {
                        if let model::Predicate::ProjectionEquality { ty, .. } = predicate {
                            find(ty, deep, found);
                        }
                    }
                }
                _ => {}
            }
        }
        let mut found = Vec::new();
        for input in signature.inputs() {
            find(input, false, &mut found);
        }
        // The output, as `Cache<Renderer>::draw` gives `Renderer::Geometry`:
        // the call infers it from the parameter's own type (T318). So is one
        // inside an input, as palette's `FromColorUnclamped<Lab<<S::Space
        // as RgbSpace>::WhitePoint, T>> for Rgb<S, T>` takes a `Lab` at the
        // white point of `S`. A std trait's, as an iterator's `Self::Item`,
        // keeps its own reading.
        let mut deep = Vec::new();
        find(signature.output(), true, &mut deep);
        for input in signature.inputs() {
            find(input, true, &mut deep);
        }
        // So is one a bound ties to another parameter, as rand's
        // `WeightedIndex::new(weights: I)` has `I::Item: SampleBorrow<X>`:
        // the type that fills `I` gives it, and the trait's impls for it
        // give `X`, as rustc infers `X` (#1084).
        for predicate in generics.predicates() {
            if let model::Predicate::Bound {
                ty: projection @ model::Type::Projection { .. },
                bound: model::Bound::Trait { arguments, .. },
            } = predicate
                && arguments.iter().any(|argument| {
                    matches!(argument, model::Argument::Type(ty) if names_type_parameter(ty))
                })
            {
                find(projection, false, &mut deep);
            }
        }
        // So is an iterator's `I::Item` and `J::IntoIter`, as
        // `Product<I, J>::next` gives `Option<(I::Item, J::Item)>` and
        // itertools' `cartesian_product` gives `Product<Self, J::IntoIter>`:
        // the type that fills the parameter gives it.
        let item = |chain: &Chain| {
            matches!(&chain[..], [(trait_id, _)]
                if self.is_core_trait(trait_id, "Iterator")
                    || self.is_core_trait(trait_id, "IntoIterator"))
        };
        for (index, chain) in deep {
            if (chain
                .iter()
                .all(|(trait_id, _)| trait_id.unit().authority().sysroot().is_none())
                || item(&chain))
                && chain.iter().all(|(_, associated)| {
                    self.definition_name(associated)
                        .is_some_and(|name| name.chars().all(|c| c.is_alphanumeric() || c == '_'))
                })
                && !found.iter().any(|(known, known_chain)| {
                    *known == index
                        && known_chain.len() == chain.len()
                        && known_chain
                            .iter()
                            .zip(&chain)
                            .all(|((_, named), (_, associated))| named == associated)
                })
            {
                found.push((index, chain));
            }
        }
        // A callable parameter's inputs and output, which a script function
        // that fills it names as the types the projections are.
        let declared = found.len();
        for predicate in generics.predicates() {
            match predicate {
                model::Predicate::Bound {
                    ty: model::Type::Parameter { .. },
                    bound:
                        model::Bound::Trait {
                            trait_id,
                            arguments,
                            ..
                        },
                } if matches!(
                    self.definition_name(trait_id),
                    Some("Fn" | "FnMut" | "FnOnce")
                ) =>
                {
                    for argument in arguments.iter() {
                        if let model::Argument::Type(ty) = argument {
                            find(ty, true, &mut found);
                        }
                    }
                }
                // What a parameter's bound binds, as iced's
                // `Application<P>::presets(impl IntoIterator<Item =
                // Preset<P::State, P::Message>>)` binds `Item` (#1050).
                model::Predicate::ProjectionEquality {
                    projection:
                        model::Type::Projection {
                            trait_id,
                            arguments,
                            ..
                        },
                    ty,
                } if self.definition_name(trait_id) == Some("FnOnce")
                    || matches!(
                        arguments.first(),
                        Some(model::Argument::Type(model::Type::Parameter { .. }))
                    ) =>
                {
                    find(ty, true, &mut found);
                }
                _ => {}
            }
        }
        let mut projected: Vec<Projected> = Vec::new();
        for (position, (index, chain)) in found.into_iter().enumerate() {
            let Some((trait_id, _)) = chain.last().cloned() else {
                continue;
            };
            // A future's `Output` an input or the output names is its own
            // synthetic parameter (D527), and a callable's is its output. A
            // callable's input is read as any other projection is, as iced's
            // `Task::sip` takes `impl FnOnce(<S as Future>::Output) -> T`
            // with `S: sipper::Core`.
            let called = position >= declared;
            if (!called
                && (self.is_core_trait(&trait_id, "Future")
                    || generics.predicates().any(|predicate| {
                        matches!(predicate, model::Predicate::Bound {
                            ty: model::Type::Parameter { index: bounded, .. },
                            bound: model::Bound::Trait { trait_id, .. },
                        } if *bounded == index && self.is_core_trait(trait_id, "Future"))
                    })))
                || matches!(
                    self.definition_name(&trait_id),
                    Some("Fn" | "FnMut" | "FnOnce")
                )
                || !generics
                    .parameters()
                    .any(|parameter| parameter.index() == index)
            {
                continue;
            }
            let Some(hops) = chain
                .into_iter()
                .map(|(trait_id, associated)| {
                    let name = self.definition_name(&associated)?.to_owned();
                    Some(Hop {
                        trait_id,
                        associated,
                        name,
                    })
                })
                .collect::<Option<Box<[Hop]>>>()
            else {
                continue;
            };
            let ordinal = projected
                .iter()
                .filter(|known| known.parameter == index)
                .count();
            let Ok(ordinal) = u32::try_from(ordinal) else {
                continue;
            };
            projected.push(Projected {
                parameter: index,
                hops,
                synthetic: signature::projection_output(index, ordinal),
            });
        }
        projected
    }

    /// `generics` with each projection of `projected` its bounds name
    /// written as its synthetic parameter, as a callable's `Fn(&P::State)`
    /// is `Fn(&PState)`. `None` when there is none.
    fn projected_generics(
        generics: &model::Generics,
        projected: &[Projected],
    ) -> Option<model::Generics> {
        if projected.is_empty() {
            return None;
        }
        let argument = |argument: &model::Argument| match argument {
            model::Argument::Type(ty) => model::Argument::Type(projected_parameters(ty, projected)),
            other => other.clone(),
        };
        let predicates = generics
            .predicates()
            .map(|predicate| match predicate {
                // A projection a bound names as its self type is its
                // synthetic parameter too, as `I::Item: SampleBorrow<X>` is
                // `IItem: SampleBorrow<X>` (#1084).
                model::Predicate::Bound {
                    ty,
                    bound:
                        model::Bound::Trait {
                            trait_id,
                            arguments,
                            polarity,
                            binders,
                        },
                } => model::Predicate::Bound {
                    ty: projected_parameters(ty, projected),
                    bound: model::Bound::Trait {
                        trait_id: trait_id.clone(),
                        arguments: arguments.iter().map(argument).collect(),
                        polarity: *polarity,
                        binders: binders.clone(),
                    },
                },
                model::Predicate::ProjectionEquality { projection, ty } => {
                    model::Predicate::ProjectionEquality {
                        projection: projection.clone(),
                        ty: projected_parameters(ty, projected),
                    }
                }
                other => other.clone(),
            })
            .collect();
        Some(model::Generics::new(
            generics.parameters().cloned().collect(),
            predicates,
        ))
    }

    /// `parameters` with the synthetic type parameter of each projection
    /// appended, named after its parameter and associated type (`PState`).
    fn declare_projected(
        projected: &[Projected],
        parameters: &mut Box<[representability::signature::Parameter]>,
    ) {
        use representability::signature;
        let mut extended = parameters.to_vec();
        for projection in projected {
            let Some(owner) = extended
                .iter()
                .find(|parameter| parameter.index() == projection.parameter)
            else {
                continue;
            };
            let mut declared = owner.name().to_owned();
            for hop in &projection.hops {
                declared.push_str(&hop.name);
            }
            extended.push(signature::Parameter::new(
                projection.synthetic,
                declared.into_boxed_str(),
                signature::Kind::Type,
            ));
        }
        *parameters = extended.into_boxed_slice();
    }

    /// For each projection, the relation that infers its synthetic
    /// parameter: each implementation of the trait whose self type and
    /// associated type Rawr can name, as `impl StatefulWidget for List<'_>
    /// { type State = ListState; }`.
    fn projection_relations(
        &self,
        projected: &[Projected],
    ) -> Result<Vec<representability::signature::Relation>, Failure> {
        use representability::signature::{Case, Relation};
        let mut relations = Vec::new();
        for projection in projected {
            let Some((first, rest)) = projection.hops.split_first() else {
                continue;
            };
            let mut cases = Vec::new();
            for implementation in self.graph.implementations_of(&first.trait_id) {
                let model::Trait::Trait { id, .. } = implementation.trait_ref() else {
                    continue;
                };
                if *id != first.trait_id || implementation.polarity() != model::Polarity::Positive {
                    continue;
                }
                let Ok(this) = self.convert(implementation.self_type()) else {
                    continue;
                };
                let value = implementation.associated().find_map(|associated| {
                    let definition = self
                        .graph
                        .definition_index(associated)
                        .and_then(|index| self.graph.definitions().nth(index))?;
                    match definition.kind() {
                        model::Kind::AssociatedType {
                            default: model::Default::Present(ty),
                            ..
                        } if definition.display_path().last() == Some(first.name.as_str()) => {
                            Some(ty)
                        }
                        _ => None,
                    }
                });
                // A projection of a projection reads each further one at
                // the type the one before gives, as `Srgb`'s `Space` is
                // `Srgb`, whose `WhitePoint` is `D65`.
                let value = value.and_then(|value| self.associated_through(rest, value.clone()));
                let Some(Ok(value)) = value.map(|value| self.convert(&value)) else {
                    continue;
                };
                if names_other_parameter(&this, u32::MAX) || names_other_parameter(&value, u32::MAX)
                {
                    continue;
                }
                cases.push(Case::new(this, Box::new([value])));
            }
            // A generic implementation at each type only a crate's alias
            // names, as `impl<A, B> geometry::Renderer for Renderer<A, B>`
            // is at iced's `Renderer`, whose `Geometry` is then canvas's
            // `Geometry` (T318).
            for index in self.instance_aliases() {
                let Some(model::Kind::TypeAlias { ty: instance, .. }) = self
                    .graph
                    .definitions()
                    .nth(*index)
                    .map(model::Definition::kind)
                else {
                    continue;
                };
                let Some(value) = self
                    .associated_at(&first.trait_id, &first.name, instance, 0)
                    .and_then(|value| self.associated_through(rest, value))
                else {
                    continue;
                };
                let (Ok(this), Ok(value)) = (self.convert(instance), self.convert(&value)) else {
                    continue;
                };
                if names_other_parameter(&this, u32::MAX)
                    || names_other_parameter(&value, u32::MAX)
                    || cases.iter().any(|case: &Case| case.this() == &this)
                {
                    continue;
                }
                cases.push(Case::new(this, Box::new([value])));
            }
            // Without a case, the relation still names the associated type,
            // which an opaque type that fills the parameter gives from its
            // own bound, as `application(..)`'s `impl Program<State = S>`.
            // A chain names no one associated type of the parameter.
            self.retain_array::<Case>(cases.len())?;
            let relation = Relation::new(
                projection.parameter,
                Box::new([representability::convert::Type::Parameter(
                    projection.synthetic,
                )]),
                cases.into_boxed_slice(),
            );
            relations.push(if rest.is_empty() {
                self.retain_str(&first.name)?;
                relation.of_associated(representability::signature::Associated::new(
                    *crate::surface::identity::Definition::of(&first.trait_id).as_bytes(),
                    first.name.as_str().into(),
                ))
            } else {
                relation
            });
        }
        Ok(relations)
    }

    /// The type `ty` gives through each projection of `hops` in turn, read
    /// as [`Self::associated_at`] reads one.
    fn associated_through(&self, hops: &[Hop], ty: model::Type) -> Option<model::Type> {
        hops.iter().try_fold(ty, |ty, hop| {
            self.associated_at(&hop.trait_id, &hop.name, &ty, 0)
        })
    }

    /// What the associated type `name` of `trait_id` is for the concrete
    /// type `this`, read from the one implementation whose self type
    /// `this` is an instance of, with the projections its value names read
    /// the same way: `<fallback::Renderer<wgpu::Renderer,
    /// tiny_skia::Renderer> as geometry::Renderer>::Geometry` is
    /// `fallback::Geometry<wgpu::Geometry, tiny_skia::Geometry>`. `None`
    /// when no implementation, or more than one, fits, or the trait takes
    /// arguments besides its self type.
    fn associated_at(
        &self,
        trait_id: &api::identity::Definition,
        name: &str,
        this: &model::Type,
        depth: usize,
    ) -> Option<model::Type> {
        associated_at(self.graph, trait_id, name, this, depth)
    }

    /// Each associated type an opaque type's `bounds` bind, as `(trait,
    /// associated type, value)`: its own, as `impl Program<State = S>`
    /// binds, then each one a bound's supertraits bind at the bound's
    /// arguments, as rustc elaborates them. `impl Straw<(), Progress,
    /// Error>` binds `Future::Output` to `Result<(), Error>` and
    /// `Stream::Item` to `Progress` through `Sipper` and `Core`.
    fn bound_associated(
        &self,
        bounds: &[model::Predicate],
    ) -> Vec<(
        api::identity::Definition,
        api::identity::Definition,
        model::Type,
    )> {
        self.elaborate(bounds).0
    }

    /// Each trait an opaque type's `bounds` name, then each of their
    /// supertraits, at the bound's arguments after `Self`, as rustc
    /// elaborates them: `impl Stocked` is a `Named` too, when `Named` is a
    /// supertrait of `Stocked`.
    fn bound_traits(
        &self,
        bounds: &[model::Predicate],
    ) -> Vec<(api::identity::Definition, Vec<model::Argument>)> {
        self.elaborate(bounds).1
    }

    /// The associated types `bounds` bind (`bound_associated`) and the
    /// traits they name (`bound_traits`), each through the supertraits.
    #[allow(clippy::type_complexity)]
    fn elaborate(
        &self,
        bounds: &[model::Predicate],
    ) -> (
        Vec<(
            api::identity::Definition,
            api::identity::Definition,
            model::Type,
        )>,
        Vec<(api::identity::Definition, Vec<model::Argument>)>,
    ) {
        /// Supertraits are read this deep.
        const DEPTH: usize = 8;
        fn is_self(ty: &model::Type) -> bool {
            matches!(
                ty,
                model::Type::SelfType(_) | model::Type::Parameter { index: 0, .. }
            )
        }
        let mut found = Vec::new();
        let mut pending = Vec::new();
        // A supertrait's predicates name the trait's `Self`, which is the
        // bound's self type: `Number: Add<Output = Self>` binds `Output` to
        // the opaque type `impl Number` is, not to the self type of the
        // impl a member is classified in (#1278).
        let mut read = |predicates: &[model::Predicate],
                        bindings: &BTreeMap<u32, model::Type>,
                        this: Option<(&api::identity::Definition, &model::Type)>,
                        depth: usize,
                        pending: &mut Vec<_>| {
            let at = |ty: &model::Type| {
                let ty = substitute_bindings(ty, bindings);
                match this {
                    Some((trait_id, this)) => {
                        type_replaced(&ty, &model::Type::SelfType(trait_id.clone()), this)
                    }
                    None => ty,
                }
            };
            for predicate in predicates {
                match predicate {
                    model::Predicate::ProjectionEquality {
                        projection:
                            model::Type::Projection {
                                trait_id,
                                associated,
                                arguments,
                            },
                        ty,
                    } if depth == 0
                        || matches!(arguments.first(),
                            Some(model::Argument::Type(this)) if is_self(this)) =>
                    {
                        found.push((trait_id.clone(), associated.clone(), at(ty)));
                    }
                    model::Predicate::Bound {
                        ty,
                        bound:
                            model::Bound::Trait {
                                trait_id,
                                arguments,
                                polarity: model::Polarity::Positive,
                                binders,
                            },
                    } if (depth == 0 || is_self(ty)) && binders.is_empty() => {
                        let arguments = arguments
                            .iter()
                            .map(|argument| match argument {
                                model::Argument::Type(ty) => model::Argument::Type(at(ty)),
                                other => other.clone(),
                            })
                            .collect::<Vec<_>>();
                        let this = this.map_or_else(|| ty.clone(), |(_, this)| this.clone());
                        pending.push((trait_id.clone(), arguments, this, depth));
                    }
                    _ => {}
                }
            }
        };
        read(bounds, &BTreeMap::new(), None, 0, &mut pending);
        let mut seen = Vec::new();
        while let Some((trait_id, arguments, this, depth)) = pending.pop() {
            if depth >= DEPTH || seen.contains(&(trait_id.clone(), arguments.clone())) {
                continue;
            }
            let Some(model::Kind::Trait {
                supertraits,
                generics,
                ..
            }) = self
                .graph
                .definition_index(&trait_id)
                .and_then(|index| self.graph.definitions().nth(index))
                .map(model::Definition::kind)
            else {
                continue;
            };
            // The trait's own parameters after `Self` take the bound's
            // arguments, in order, and `Self` its self type.
            let bindings = generics
                .parameters()
                .filter(|parameter| parameter.index() != 0)
                .zip(&arguments)
                .filter_map(|(parameter, argument)| match argument {
                    model::Argument::Type(ty) => Some((parameter.index(), ty.clone())),
                    _ => None,
                })
                .chain([(0, this.clone())])
                .collect();
            read(
                supertraits,
                &bindings,
                Some((&trait_id, &this)),
                depth + 1,
                &mut pending,
            );
            seen.push((trait_id, arguments));
        }
        (found, seen)
    }

    /// The type parameters bounded by `core::future::Future`, marked as
    /// future parameters, each with a synthetic `F::Output` parameter
    /// appended: `(index, owner)`.
    fn future_parameters(
        &self,
        generics: &model::Generics,
        parameters: &mut Box<[representability::signature::Parameter]>,
        callable: bool,
    ) -> Vec<(u32, api::identity::Definition)> {
        use representability::signature;
        if callable {
            return Vec::new();
        }
        let mut futures = Vec::new();
        for predicate in generics.predicates() {
            let model::Predicate::Bound {
                ty: model::Type::Parameter { owner, index },
                bound:
                    model::Bound::Trait {
                        trait_id,
                        polarity: model::Polarity::Positive,
                        ..
                    },
            } = predicate
            else {
                continue;
            };
            if !self.is_core_trait(trait_id, "Future")
                || futures.iter().any(|(known, _)| known == index)
            {
                continue;
            }
            futures.push((*index, owner.clone()));
        }
        if futures.is_empty() {
            return futures;
        }
        let mut extended = parameters.to_vec();
        for (index, _) in &futures {
            let Some(parameter) = extended.iter_mut().find(|parameter| {
                parameter.index() == *index && parameter.kind() == signature::Kind::Type
            }) else {
                continue;
            };
            parameter.mark_future();
            let name = format!("{}Output", parameter.name());
            extended.push(signature::Parameter::new(
                signature::future_output(*index),
                name.into_boxed_str(),
                signature::Kind::Type,
            ));
        }
        *parameters = extended.into_boxed_slice();
        futures
    }

    /// The type each future parameter's `Output` is bound to, as `T` of
    /// iced's `Task::future(impl Future<Output = T>)`, by the future's
    /// synthetic output parameter: `(synthetic, type)`.
    fn future_outputs(
        &self,
        generics: &model::Generics,
        futures: &[(u32, api::identity::Definition)],
        rewrite: impl Fn(&model::Type) -> model::Type,
    ) -> Vec<(u32, representability::convert::Type)> {
        let mut outputs = Vec::new();
        for predicate in generics.predicates() {
            let model::Predicate::ProjectionEquality {
                projection:
                    model::Type::Projection {
                        trait_id,
                        associated,
                        arguments,
                    },
                ty,
            } = predicate
            else {
                continue;
            };
            let Some(model::Argument::Type(model::Type::Parameter { index, .. })) =
                arguments.first()
            else {
                continue;
            };
            if !self.is_core_trait(trait_id, "Future")
                || self.definition_name(associated) != Some("Output")
                || !futures.iter().any(|(future, _)| future == index)
            {
                continue;
            }
            let synthetic = representability::signature::future_output(*index);
            if let Ok(fixed) = self.convert_input(&rewrite(ty))
                && !names_parameter(&fixed, synthetic)
            {
                outputs.push((synthetic, fixed));
            }
        }
        outputs
    }

    /// Each bound `R: Trait<A..>` that names another type parameter in
    /// `A..`, with the trait's implementations in this crate whose self
    /// type and arguments Rawr can name. rand's `random_range` has `R:
    /// SampleRange<T>`, implemented for `Range<T>`: a call with a
    /// `Range<f32>` fills `T` with `f32`, as rustc infers it.
    fn relations(
        &self,
        generics: &model::Generics,
    ) -> Result<Box<[representability::signature::Relation]>, Failure> {
        use representability::convert::Type;
        use representability::signature::{Case, Relation, IMPL_PARAMETER};
        /// `ty` with each parameter marked as the impl's own.
        fn marked(ty: Type) -> Type {
            match ty {
                Type::Parameter(index) => Type::Parameter(IMPL_PARAMETER | index),
                Type::Range(element) => Type::Range(Box::new(marked(*element))),
                Type::Option(element) => Type::Option(Box::new(marked(*element))),
                Type::Nominal {
                    definition,
                    arguments,
                } => Type::Nominal {
                    definition,
                    arguments: arguments.into_vec().into_iter().map(marked).collect(),
                },
                other => other,
            }
        }
        let mut relations = Vec::new();
        for predicate in generics.predicates() {
            let model::Predicate::Bound {
                ty: model::Type::Parameter { index, .. },
                bound:
                    model::Bound::Trait {
                        trait_id,
                        arguments,
                        polarity: model::Polarity::Positive,
                        binders,
                    },
            } = predicate
            else {
                continue;
            };
            // A lifetime argument, as `'a` of iced's `C: Component<'a,
            // Message, Theme, Renderer>`, ties no type.
            let arguments = arguments
                .iter()
                .filter(|argument| !matches!(argument, model::Argument::Lifetime(_)))
                .collect::<Vec<_>>();
            if !binders.is_empty() {
                continue;
            }
            // `T: Float` with no arguments, as iced's `Animation<T: Float>`:
            // its implementations for number types are the cases, so a
            // literal whose kind only one of them fits is that type, as
            // rustc selects lilt's `impl Float for f32` for `new(0.)` (#1028).
            // The driver brings a crate trait's impls for numbers; a std
            // trait's, as `Copy`'s, are only those something else used.
            if arguments.is_empty() {
                if trait_id.unit().authority().sysroot().is_some() {
                    continue;
                }
                let cases = self
                    .graph
                    .implementations_of(trait_id)
                    .filter(|implementation| implementation.polarity() == model::Polarity::Positive)
                    .filter_map(|implementation| match implementation.self_type() {
                        // The Rust number itself: `convert` gives the
                        // value Rawr holds, an `f64` for an `f32`.
                        model::Type::Primitive(name) => {
                            representability::convert::Builtin::primitive(name)
                                .filter(|builtin| builtin.is_number())
                                .map(|builtin| Case::new(Type::Builtin(builtin), Box::default()))
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                if !cases.is_empty() {
                    self.retain_array::<Case>(cases.len())?;
                    relations.push(Relation::new(*index, Box::default(), cases.into()));
                }
                continue;
            }
            let Some(converted) = arguments
                .iter()
                .map(|argument| match argument {
                    model::Argument::Type(ty) => self.convert(ty).ok(),
                    _ => None,
                })
                .collect::<Option<Box<[_]>>>()
            else {
                continue;
            };
            // `P: Into<T>` of a concrete `T`, as Freya's `background(impl
            // Into<Fill>)`: `T`'s implementations of `From<S>` for a
            // concrete `S` are the cases, each `S` a self type that gives
            // `T`. An argument of one case's shape is that `S`, so
            // `(35, 35, 35)` is `(u8, u8, u8)` and `12.` an `f32`, as
            // rustc's trait selection infers it.
            if let [target] = &*converted
                && !names_other_parameter(target, u32::MAX)
                && self.is_core_trait(trait_id, "Into")
            {
                let cases = self.conversion_cases(target)?;
                if !cases.is_empty() {
                    self.retain_array::<Case>(cases.len())?;
                    relations.push(Relation::new(*index, converted, cases));
                }
                continue;
            }
            // `P: Into<T>` of another type parameter, as iced's `Slider::
            // default(impl Into<T>)`: core's `impl<T> From<T> for T` ties
            // `P` to `T`, so a number literal is `T`'s number, as rustc
            // infers `default(50)` a `u8` where `T` is one.
            if let [Type::Parameter(_)] = &*converted
                && self.is_core_trait(trait_id, "Into")
            {
                let reflexive = Type::Parameter(IMPL_PARAMETER);
                self.retain_array::<Case>(1)?;
                relations.push(Relation::new(
                    *index,
                    converted,
                    Box::new([Case::new(reflexive.clone(), Box::new([reflexive]))]),
                ));
                continue;
            }
            // `T: Mul<T>` ties nothing new; `R: SampleRange<T>` ties `T`.
            if !converted.iter().any(|ty| names_other_parameter(ty, *index)) {
                continue;
            }
            let mut cases = Vec::new();
            // `P: Into<T>` of a generic `T`, as iced's `center(impl
            // Into<Element<'a, Message, Theme, Renderer>>)`: core's `impl<T,
            // U: From<T>> Into<U> for T` makes each crate impl of `From<S>`
            // for a crate type a case, `S` the self type that gives it, as
            // `From<Text<'a, Theme, Renderer>> for Element<'a, Message,
            // Theme, Renderer>` ties the text's theme and renderer to the
            // element's. A crate impl whose `S` is a bare parameter fits
            // every argument and so ties nothing. Core's reflexive `impl<T>
            // From<T> for T` is the first case: it fits only an argument of
            // the target's own type, and ties that argument's parameters to
            // the target's, as a `Slot<'_, _>` passed to `impl Into<Slot<'a,
            // Message>>` is the slot of `Message` (#1404).
            if let [Type::Nominal {
                definition: into, ..
            }] = &*converted
                && self.is_core_trait(trait_id, "Into")
            {
                let reflexive = Type::Parameter(IMPL_PARAMETER);
                cases.push(Case::new(reflexive.clone(), Box::new([reflexive])));
                for implementation in self.core_implementations("From") {
                    let model::Trait::Trait { id, arguments } = implementation.trait_ref() else {
                        continue;
                    };
                    let [_, model::Argument::Type(source)] = arguments.as_ref() else {
                        continue;
                    };
                    if implementation.polarity() != model::Polarity::Positive
                        || !self.is_core_trait(id, "From")
                    {
                        continue;
                    }
                    let (Ok(target @ Type::Nominal { .. }), Ok(source)) = (
                        self.convert(implementation.self_type()),
                        self.convert(source),
                    ) else {
                        continue;
                    };
                    if !matches!(&target, Type::Nominal { definition, .. } if definition == into)
                        || matches!(source, Type::Parameter(_))
                    {
                        continue;
                    }
                    cases.push(
                        Case::new(marked(source), Box::new([marked(target)]))
                            .with_projections(self.case_projections(implementation, marked)),
                    );
                }
            }
            for implementation in self.graph.implementations_of(trait_id) {
                let model::Trait::Trait {
                    id,
                    arguments: implemented,
                } = implementation.trait_ref()
                else {
                    continue;
                };
                // An impl's trait reference names `Self` first; a bound's
                // arguments start after it.
                let Some(implemented) = implemented.get(1..) else {
                    continue;
                };
                let implemented = implemented
                    .iter()
                    .filter(|argument| !matches!(argument, model::Argument::Lifetime(_)))
                    .collect::<Vec<_>>();
                if id != trait_id
                    || implementation.polarity() != model::Polarity::Positive
                    || implemented.len() != arguments.len()
                {
                    continue;
                }
                let Ok(this) = self.convert(implementation.self_type()) else {
                    continue;
                };
                let Some(implemented) = implemented
                    .iter()
                    .map(|argument| match argument {
                        model::Argument::Type(ty) => self.convert(ty).ok().map(marked),
                        _ => None,
                    })
                    .collect::<Option<Box<[_]>>>()
                else {
                    continue;
                };
                cases.push(
                    Case::new(marked(this), implemented)
                        .with_projections(self.case_projections(implementation, marked)),
                );
            }
            // Every impl is a case, however many the crate has, as rustc
            // never forgets a bound: icedxdy_widget's hundred `From<_> for
            // Slot` impls still leave the reflexive one to tie a slot's
            // message to `badge(impl Into<Slot<'a, Message>>)`'s (#1435).
            // With no case, the trait's impl headers may still fix the
            // bound's arguments (D604).
            self.retain_array::<Case>(cases.len())?;
            relations.push(
                Relation::new(*index, converted, cases.into_boxed_slice())
                    .of_trait(*crate::surface::identity::Definition::of(trait_id).as_bytes()),
            );
        }
        self.retain_array::<Relation>(relations.len())?;
        Ok(relations.into_boxed_slice())
    }

    /// The bounds `<ty as Trait>::Name == value` of `implementation`'s
    /// where-clause, its parameters `marked` as a case's, each with a case
    /// for every type `ty` may be that Rawr can name: the trait's impls for
    /// a concrete type, and each type only a crate's alias names, at the
    /// associated type rustc's impls give it there. A bound of a trait that
    /// takes arguments besides `Self`, or of a type Rawr can't name, is left
    /// out.
    fn case_projections(
        &self,
        implementation: &model::Implementation,
        marked: fn(representability::convert::Type) -> representability::convert::Type,
    ) -> Box<[representability::signature::Projection]> {
        let generics = implementation.generics();
        generics
            .predicates()
            .filter_map(|predicate| {
                let model::Predicate::ProjectionEquality {
                    projection:
                        model::Type::Projection {
                            trait_id,
                            associated,
                            arguments,
                        },
                    ty: value,
                } = predicate
                else {
                    return None;
                };
                let [model::Argument::Type(this)] = arguments
                    .iter()
                    .filter(|argument| !matches!(argument, model::Argument::Lifetime(_)))
                    .collect::<Vec<_>>()[..]
                else {
                    return None;
                };
                let name = self.definition_name(associated)?;
                let candidates =
                    projection_candidates(self.graph, trait_id, self.instance_aliases());
                let mut cases = Vec::new();
                for candidate in &candidates {
                    let Some(at) = self.associated_at(trait_id, name, candidate, 0) else {
                        continue;
                    };
                    let (Ok(case), Ok(at)) = (self.convert(candidate), self.convert(&at)) else {
                        continue;
                    };
                    if names_other_parameter(&case, u32::MAX)
                        || names_other_parameter(&at, u32::MAX)
                        || cases.iter().any(|(known, _)| *known == case)
                    {
                        continue;
                    }
                    cases.push((case, at));
                }
                Some(representability::signature::Projection::new(
                    marked(self.convert(this).ok()?),
                    marked(self.convert(value).ok()?),
                    cases.into_boxed_slice(),
                ))
            })
            .collect()
    }

    /// The cases of `P: Into<target>`: each implementation of `From<S>`
    /// for `target` whose `S` names no parameter, as a self type `S` with
    /// the trait argument `target`, once each. rustc's reflexive `From<T>
    /// for T` is none of them: an argument that is already `target` needs
    /// nothing inferred.
    fn conversion_cases(
        &self,
        target: &representability::convert::Type,
    ) -> Result<Box<[representability::signature::Case]>, Failure> {
        use representability::signature::Case;
        let mut cases = Vec::<Case>::new();
        for implementation in self.core_implementations("From") {
            let model::Trait::Trait { id, arguments } = implementation.trait_ref() else {
                continue;
            };
            let [_, model::Argument::Type(source)] = arguments.as_ref() else {
                continue;
            };
            if implementation.polarity() != model::Polarity::Positive
                || !self.is_core_trait(id, "From")
                || self.convert(implementation.self_type()).ok().as_ref() != Some(target)
            {
                continue;
            }
            // A tuple, as `(u8, u8, u8)`, is a value Rawr holds, and so is
            // an array of a known length, as `Padding`'s `From<[u16; 2]>`
            // takes.
            let source = match source {
                model::Type::Array { element, length } => {
                    array_length(length).ok_or(()).and_then(|length| {
                        let element = self.convert_value(element).map_err(|_| ())?;
                        Ok(representability::convert::Type::Sequence {
                            element: Box::new(element),
                            owned: true,
                            length: Some(length),
                        })
                    })
                }
                source => self.convert_value(source).map_err(|_| ()),
            };
            let Ok(source) = source else {
                continue;
            };
            if names_other_parameter(&source, u32::MAX) {
                continue;
            }
            let case = Case::new(source, Box::new([target.clone()]));
            if !cases.contains(&case) {
                cases.try_reserve(1).map_err(|_| Error::Allocation)?;
                cases.push(case);
            }
        }
        Ok(cases.into_boxed_slice())
    }

    fn definition_name(&self, definition: &api::identity::Definition) -> Option<&str> {
        self.graph
            .definition_index(definition)
            .and_then(|index| self.graph.definitions().nth(index))
            .and_then(|definition| definition.display_path().last())
    }

    /// The implementations of the `core` trait `name`, in graph order.
    fn core_implementations(
        &self,
        name: &str,
    ) -> impl Iterator<Item = &'graph model::Implementation> + use<'graph, '_, P> {
        let graph = self.graph;
        let traits = self.core_traits.get_or_init(|| {
            let mut traits = HashMap::<&'graph str, Vec<usize>>::new();
            for (index, implementation) in graph.implementations().enumerate() {
                let model::Trait::Trait { id, .. } = implementation.trait_ref() else {
                    continue;
                };
                if id.unit().authority().sysroot().is_none() || id.unit().crate_name() != "core" {
                    continue;
                }
                let Some(name) = graph
                    .definition_index(id)
                    .and_then(|index| graph.definitions().nth(index))
                    .and_then(|definition| definition.display_path().last())
                else {
                    continue;
                };
                traits.entry(name).or_default().push(index);
            }
            traits
                .into_iter()
                .map(|(name, indices)| (name, indices.into_boxed_slice()))
                .collect()
        });
        traits
            .get(name)
            .map_or(&[][..], AsRef::as_ref)
            .iter()
            .filter_map(move |index| graph.implementations().nth(*index))
    }
}

impl Policy {
    /// Return the sole Phase 2 classifier policy.
    #[must_use]
    pub const fn standard() -> Self {
        Self {
            schema: CONTRACT_SCHEMA,
            classifier: CLASSIFIER_VERSION,
            compiler_abi: COMPILER_ABI,
        }
    }

    #[cfg(test)]
    const fn with_versions(schema: u32, classifier: u32) -> Self {
        Self {
            schema,
            classifier,
            ..Self::standard()
        }
    }
}

impl Default for Policy {
    fn default() -> Self {
        Self::standard()
    }
}

impl<'contract> Outcome<'contract> {
    /// Borrow the exact reversible export path proved by this classified entry.
    #[must_use]
    pub const fn export(&self) -> &'contract api::identity::Export {
        &self.record.export
    }

    /// Borrow the stable symbolic entry identity.
    #[must_use]
    pub const fn identity(&self) -> &'contract crate::surface::identity::Identity {
        &self.record.identity
    }

    /// Return the unchanged Rust declaration name.
    #[must_use]
    pub fn name(&self) -> &'contract str {
        &self.record.name
    }

    /// Borrow the entry's sole closed representability result.
    #[must_use]
    pub const fn representability(&self) -> &'contract representability::Representability {
        self.representability
    }

    /// The variant of its enum this path re-exports, as `pub use
    /// Alignment::Center;` does: the outcome is otherwise its enum's.
    #[must_use]
    pub fn variant(&self) -> Option<&'contract str> {
        self.record.variant.as_deref()
    }

    /// The trait's own method this impl member is an instance of, when the
    /// trait declares it: each `into` core's blanket `Into` gives a type
    /// through a crate's `From` impls is `Into::into`.
    #[must_use]
    pub const fn member(&self) -> Option<crate::surface::identity::Definition> {
        self.record.member
    }

    /// Return the export/declaration positions for a definition-backed outcome.
    #[must_use]
    pub const fn origin(&self) -> Option<&'contract representability::origin::Set> {
        self.record.origin.as_ref()
    }

    /// Iterate over every implementation retained for this exact export.
    pub fn implementations(
        self,
    ) -> impl ExactSizeIterator<Item = ImplementationOutcome<'contract>> + 'contract {
        self.record
            .implementations
            .iter()
            .map(|record| ImplementationOutcome {
                record,
                definitions: self.definitions,
            })
    }
}

impl<'contract> Outcome<'contract> {
    pub(crate) fn new(
        record: &'contract OutcomeRecord,
        definitions: &'contract [DefinitionPlan],
    ) -> Self {
        Self {
            record,
            representability: record.representability(definitions),
            definitions,
        }
    }
}

impl OutcomeRecord {
    /// The implementation ledgers and associated outcomes the outcome
    /// retains, with each associated outcome's own, as the opaque type a
    /// trait's method gives has its members under that method
    /// (`method_opaque_implementations`).
    pub(crate) fn ledgers(&self) -> Option<(usize, usize)> {
        self.implementations
            .iter()
            .flat_map(|implementation| implementation.associated.iter())
            .try_fold(
                (self.implementations.len(), 0_usize),
                |(implementations, associated), member| {
                    let (inner, below) = member.ledgers()?;
                    Some((
                        implementations.checked_add(inner)?,
                        associated.checked_add(below)?.checked_add(1)?,
                    ))
                },
            )
    }

    /// The items the outcome retains: itself and its `ledgers`.
    pub(crate) fn retained_items(&self) -> Option<usize> {
        let (implementations, associated) = self.ledgers()?;
        implementations.checked_add(associated)?.checked_add(1)
    }
}

impl DefinitionPlan {
    /// Return the semantic Rust definition proved exactly once by this plan.
    #[must_use]
    pub const fn definition(&self) -> crate::surface::identity::Definition {
        self.definition
    }

    /// Borrow the alias-neutral representability proof.
    #[must_use]
    pub fn representability(&self) -> &representability::Representability {
        &self.representability
    }
}

fn recount_retained_usage(
    graph: &api::Graph<impl api::Phase>,
    definitions: &[DefinitionPlan],
    outcomes: &[OutcomeRecord],
    support: &SupportLedger,
    canonical_bytes: usize,
) -> Option<(usize, usize)> {
    let seed = graph.allocation();
    let mut audit = DriverAudit {
        items: seed.items,
        bytes: seed.bytes,
    };
    audit.add(1, std::mem::size_of::<Contract<Classified>>())?;
    audit.add(0, std::mem::size_of::<SupportLedger>())?;
    audit.add(outcomes.len(), std::mem::size_of_val(outcomes))?;
    audit.add(definitions.len(), std::mem::size_of_val(definitions))?;
    for definition in definitions {
        audit.representability(&definition.representability)?;
    }
    for outcome in outcomes {
        audit.outcome(outcome)?;
    }
    audit.support(support)?;
    audit.add(0, canonical_bytes)?;
    Some((audit.items, audit.bytes))
}

#[cfg(feature = "rustc-driver")]
#[allow(
    dead_code,
    clippy::too_many_arguments,
    reason = "the scalar evidence recount receives each immutable Contract ledger separately"
)]
pub(crate) fn classification_evidence(
    graph: &api::Graph<impl api::Phase>,
    definitions: &[DefinitionPlan],
    outcomes: &[OutcomeRecord],
    support: &SupportLedger,
    canonical_bytes: usize,
    final_usage: contract::Usage,
    scratch_bytes: usize,
    stage: crate::process::EvidenceStage,
) -> Option<ClassificationEvidence> {
    let graph_usage = graph.allocation();
    let graph_retained = graph.retained_evidence()?;
    if graph_retained.category_sum()? != graph_retained.total || graph_retained.total != graph_usage
    {
        return None;
    }
    let transaction = graph.transaction();
    let (implementation_ledgers, associated) = outcomes.iter().try_fold(
        (0_usize, 0_usize),
        |(implementations, associated), outcome| {
            let (inner, below) = outcome.ledgers()?;
            Some((
                implementations.checked_add(inner)?,
                associated.checked_add(below)?,
            ))
        },
    )?;

    let classifier_new_items = 1_usize
        .checked_add(definitions.len())?
        .checked_add(support.groups.len())?
        .checked_add(support.edges.len())?
        .checked_add(support.rejections.len())?
        .checked_add(outcomes.len())?
        .checked_add(implementation_ledgers)?
        .checked_add(associated)?;
    if support.groups.len()
        != definitions
            .len()
            .checked_add(graph.implementations().len())?
    {
        return None;
    }
    let expected_final_items = graph_usage.items.checked_add(classifier_new_items)?;
    if expected_final_items != final_usage.items {
        return None;
    }

    let contract_inline = std::mem::size_of::<Contract<Classified>>()
        .checked_add(std::mem::size_of::<SupportLedger>())?;
    let mut definition_audit = DriverAudit { items: 0, bytes: 0 };
    definition_audit.add(0, std::mem::size_of_val(definitions))?;
    for definition in definitions {
        definition_audit.representability(&definition.representability)?;
    }
    let mut outcome_audit = DriverAudit { items: 0, bytes: 0 };
    outcome_audit.add(0, std::mem::size_of_val(outcomes))?;
    for outcome in outcomes {
        outcome_audit.outcome(outcome)?;
    }
    let mut support_audit = DriverAudit { items: 0, bytes: 0 };
    support_audit.support(support)?;
    let expected_final_bytes = graph_usage
        .bytes
        .checked_add(contract_inline)?
        .checked_add(definition_audit.bytes)?
        .checked_add(outcome_audit.bytes)?
        .checked_add(support_audit.bytes)?
        .checked_add(canonical_bytes)?;
    if expected_final_bytes != final_usage.bytes {
        return None;
    }

    Some(ClassificationEvidence {
        graph_retained,
        graph_items: graph_usage.items,
        graph_bytes: graph_usage.bytes,
        transaction_items: transaction.items,
        transaction_bytes: transaction.bytes,
        definitions: definitions.len(),
        implementations: graph.implementations().len(),
        exports: outcomes.len(),
        support_groups: support.groups.len(),
        support_edges: support.edges.len(),
        support_rejections: support.rejections.len(),
        implementation_ledgers,
        associated,
        classifier_new_items,
        final_items: final_usage.items,
        final_bytes: final_usage.bytes,
        bytes: ClassificationBytes {
            graph: graph_usage.bytes,
            contract_inline,
            definition_plans: definition_audit.bytes,
            outcomes: outcome_audit.bytes,
            support: support_audit.bytes,
            canonical: canonical_bytes,
        },
        scratch_high_water_bytes: scratch_bytes,
        canonical_contract_bytes: canonical_bytes,
        elapsed: stage.elapsed(),
    })
}

struct DriverAudit {
    items: usize,
    bytes: usize,
}

impl DriverAudit {
    fn add(&mut self, items: usize, bytes: usize) -> Option<()> {
        self.items = self.items.checked_add(items)?;
        self.bytes = self.bytes.checked_add(bytes)?;
        Some(())
    }

    fn span(&mut self, span: &model::Span) -> Option<()> {
        self.add(0, span.retained_bytes())
    }

    fn reason(&mut self, reason: &ledger::Reason) -> Option<()> {
        use ledger::Reason;
        let bytes = match reason {
            Reason::ExternalAuthorityUnavailable {
                crate_name,
                stable_crate_id,
            } => crate_name.len().checked_add(stable_crate_id.len())?,
            Reason::UnnameablePrivate(definition) | Reason::UnresolvedDefinition(definition) => {
                definition.retained_bytes()?
            }
            Reason::UnknownCompilerShape {
                definition,
                compiler_kind,
            } => definition
                .retained_bytes()?
                .checked_add(compiler_kind.len())?,
            Reason::UnsupportedPublicPath { compiler_kind } => compiler_kind.len(),
            Reason::InvalidAliasChain | Reason::ConflictingIdentity => 0,
        };
        self.add(0, bytes)
    }

    fn parameters(&mut self, parameters: &[representability::signature::Parameter]) -> Option<()> {
        self.add(0, std::mem::size_of_val(parameters))?;
        for parameter in parameters {
            self.add(0, parameter.name().len())?;
            if let Some(default) = parameter.default() {
                self.ty(default)?;
            }
        }
        Some(())
    }

    fn ty(&mut self, ty: &representability::convert::Type) -> Option<()> {
        if let representability::convert::Type::Nominal { arguments, .. } = ty {
            self.add(0, std::mem::size_of_val(arguments.as_ref()))?;
            for argument in arguments {
                self.ty(argument)?;
            }
        }
        Some(())
    }

    fn signature(&mut self, signature: &representability::signature::Signature) -> Option<()> {
        self.parameters(signature.parameters())?;
        self.add(0, std::mem::size_of_val(signature.inputs()))?;
        for input in signature.inputs() {
            self.ty(input)?;
        }
        self.ty(signature.output())
    }

    fn field(&mut self, field: &representability::structure::Field) -> Option<()> {
        self.add(0, field.selector().retained_bytes()?)?;
        self.add(0, field.name().len())?;
        self.ty(field.ty())?;
        self.span(field.span())
    }

    fn representability(
        &mut self,
        representability: &representability::Representability,
    ) -> Option<()> {
        use representability::Representability as R;
        match representability {
            R::Value(plan) => {
                self.ty(plan.ty())?;
                self.parameters(plan.parameters())?;
            }
            R::Struct(plan) => {
                self.parameters(plan.parameters())?;
                self.add(0, std::mem::size_of_val(plan.fields()))?;
                if matches!(plan.form(), model::FieldForm::Record) {
                    let form_bytes = plan.fields().iter().try_fold(
                        plan.fields()
                            .len()
                            .checked_mul(std::mem::size_of::<crate::surface::identity::Field>())?,
                        |bytes, field| bytes.checked_add(field.name().len()),
                    )?;
                    self.add(0, form_bytes)?;
                }
                for field in plan.fields() {
                    self.field(field)?;
                }
            }
            R::Enum(plan) => {
                self.parameters(plan.parameters())?;
                self.add(0, std::mem::size_of_val(plan.variants()))?;
                for variant in plan.variants() {
                    self.add(0, variant.selector().retained_bytes()?)?;
                    self.add(0, variant.name().len())?;
                    self.add(0, std::mem::size_of_val(variant.fields()))?;
                    for field in variant.fields() {
                        self.field(field)?;
                    }
                    self.span(variant.span())?;
                }
            }
            R::Function(plan) => self.signature(plan.signature())?,
            R::Method(plan) => self.signature(plan.signature())?,
            R::Constant(plan) => {
                self.ty(plan.ty())?;
                self.add(
                    0,
                    plan.value().map_or(Some(0), |value| {
                        value.ty().len().checked_add(value.value().len())
                    })?,
                )?;
            }
            R::FunctionItem(item) => {
                self.signature(item.signature())?;
                self.add(0, std::mem::size_of_val(item.binders()))?;
                self.add(0, std::mem::size_of_val(item.predicates()))?;
                if let Some(call) = item.call() {
                    self.add(0, std::mem::size_of_val(call.inputs()))?;
                    for input in call.inputs() {
                        self.ty(input)?;
                    }
                    self.ty(call.output())?;
                }
            }
            R::Invocation(item) => {
                self.signature(item.signature())?;
                self.add(0, std::mem::size_of_val(item.binders()))?;
                self.add(0, std::mem::size_of_val(item.predicates()))?;
            }
            R::Excluded(reason) => {
                self.add(0, reason.reason().len())?;
                self.span(reason.policy_span())?;
                self.span(reason.source_span())?;
            }
            R::Rejected(reason) => {
                if let Some(extraction) = reason.extraction_reason() {
                    self.reason(extraction)?;
                }
                self.span(reason.span())?;
            }
        }
        Some(())
    }

    fn outcome(&mut self, outcome: &OutcomeRecord) -> Option<()> {
        self.add(0, outcome.export.retained_bytes()?)?;
        self.add(0, outcome.name.len())?;
        if let Some(variant) = &outcome.variant {
            self.add(0, variant.len())?;
        }
        if let Some(origin) = &outcome.origin {
            self.add(0, outcome.identity.selector().retained_bytes()?)?;
            self.span(origin.source())?;
            self.span(origin.declaration())?;
        }
        if let OutcomeEvidence::Inline(representability) = &outcome.evidence {
            self.add(0, std::mem::size_of::<representability::Representability>())?;
            self.representability(representability)?;
        }
        self.add(
            outcome.implementations.len(),
            std::mem::size_of_val(outcome.implementations.as_ref()),
        )?;
        for implementation in &outcome.implementations {
            self.span(&implementation.span)?;
            self.add(
                implementation.associated.len(),
                std::mem::size_of_val(implementation.associated.as_ref()),
            )?;
            for associated in &implementation.associated {
                self.outcome(associated)?;
            }
        }
        Some(())
    }

    fn support(&mut self, support: &SupportLedger) -> Option<()> {
        self.add(
            support.groups.len(),
            std::mem::size_of_val(support.groups.as_ref()),
        )?;
        self.add(
            support.edges.len(),
            std::mem::size_of_val(support.edges.as_ref()),
        )?;
        self.add(
            support.rejections.len(),
            std::mem::size_of_val(support.rejections.as_ref()),
        )?;
        for rejection in &support.rejections {
            self.reason(&rejection.reason)?;
            self.span(&rejection.span)?;
        }
        Some(())
    }
}

impl SupportLedger {
    pub(crate) fn outcomes(&self) -> SupportOutcomes<'_> {
        SupportOutcomes {
            ledger: self,
            group: None,
            edge: 0,
            remaining: self.edges.len(),
        }
    }

    /// No support, for an implementation that retained none.
    pub(crate) fn no_outcomes(&self) -> SupportOutcomes<'_> {
        SupportOutcomes {
            ledger: self,
            group: None,
            edge: self.edges.len(),
            remaining: 0,
        }
    }

    pub(crate) fn definition_outcomes(
        &self,
        definition: crate::surface::identity::Definition,
    ) -> Option<SupportOutcomes<'_>> {
        self.group_outcomes(SupportProvenance::Definition(definition))
    }

    pub(crate) fn implementation_outcomes(
        &self,
        implementation: crate::surface::identity::Implementation,
    ) -> Option<SupportOutcomes<'_>> {
        self.group_outcomes(SupportProvenance::Implementation(implementation))
    }

    fn group_outcomes(&self, provenance: SupportProvenance) -> Option<SupportOutcomes<'_>> {
        let group = self
            .groups
            .binary_search_by_key(&provenance, |group| group.provenance)
            .ok()?;
        let group = u32::try_from(group).ok()?;
        Some(SupportOutcomes {
            ledger: self,
            group: Some(group),
            edge: 0,
            remaining: self.groups[group as usize].count,
        })
    }

    pub(crate) const fn len(&self) -> usize {
        self.edges.len()
    }

    pub(crate) const fn groups_len(&self) -> usize {
        self.groups.len()
    }

    pub(crate) const fn rejections_len(&self) -> usize {
        self.rejections.len()
    }
}

impl<'a> Iterator for SupportOutcomes<'a> {
    type Item = SupportOutcome<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(edge) = self.ledger.edges.get(self.edge) {
            self.edge += 1;
            if self.group.is_some_and(|group| edge.group != group) {
                continue;
            }
            debug_assert!(self.remaining > 0);
            self.remaining -= 1;
            let provenance = self.ledger.groups.get(edge.group as usize)?.provenance;
            let Some(rejection) = edge.rejection else {
                return Some(SupportOutcome::Complete {
                    definition: edge.definition,
                    provenance,
                });
            };
            let rejection = self
                .ledger
                .rejections
                .get(rejection.get().checked_sub(1)? as usize)?;
            return Some(SupportOutcome::Rejected {
                definition: edge.definition,
                provenance,
                reason: &rejection.reason,
                span: &rejection.span,
            });
        }
        None
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl ExactSizeIterator for SupportOutcomes<'_> {
    fn len(&self) -> usize {
        self.remaining
    }
}

impl Serialize for CanonicalSupport<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut sequence = serializer.serialize_seq(Some(self.0.edges.len()))?;
        for outcome in self.0.outcomes() {
            sequence.serialize_element(&outcome)?;
        }
        sequence.end()
    }
}

impl Serialize for DefinitionPlan {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("DefinitionPlan", 2)?;
        state.serialize_field("definition", &self.definition)?;
        state.serialize_field("representability", &self.representability)?;
        state.end()
    }
}

impl Serialize for OutcomeRecord {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("Outcome", 9)?;
        state.serialize_field("export", &self.export)?;
        state.serialize_field("identity", &self.identity)?;
        state.serialize_field("name", &self.name)?;
        state.serialize_field("origin", &self.origin)?;
        let definition = match &self.evidence {
            OutcomeEvidence::Plan { definition, .. } => Some(definition),
            OutcomeEvidence::Inline(_) => None,
        };
        state.serialize_field("definition_plan", &definition)?;
        let inline = match &self.evidence {
            OutcomeEvidence::Plan { .. } => None,
            OutcomeEvidence::Inline(representability) => Some(representability.as_ref()),
        };
        state.serialize_field("representability", &inline)?;
        state.serialize_field("implementations", &self.implementations)?;
        match &self.variant {
            Some(variant) => state.serialize_field("variant", variant)?,
            None => state.skip_field("variant")?,
        }
        match &self.member {
            Some(member) => state.serialize_field("member", member)?,
            None => state.skip_field("member")?,
        }
        state.end()
    }
}

impl<'contract> ImplementationOutcome<'contract> {
    /// Return the semantic implementation identity.
    #[must_use]
    pub const fn identity(&self) -> crate::surface::identity::Implementation {
        self.record.identity
    }

    /// Return the exact Rust implementation position.
    #[must_use]
    pub const fn span(&self) -> &'contract model::Span {
        &self.record.span
    }

    /// Iterate every associated item outcome retained by this implementation.
    pub fn associated(self) -> impl ExactSizeIterator<Item = Outcome<'contract>> + 'contract {
        self.record.associated.iter().map(|record| Outcome {
            record,
            representability: record.representability(self.definitions),
            definitions: self.definitions,
        })
    }
}

impl Serialize for ImplementationRecord {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("ImplementationOutcome", 3)?;
        state.serialize_field("identity", &self.identity)?;
        state.serialize_field("span", &self.span)?;
        state.serialize_field("associated", &self.associated)?;
        state.end()
    }
}

impl api::Graph<api::Extracted> {
    /// Classify and select the facade as one pure batch before publishing their
    /// current DAG receipts. Legacy graphs retain their existing validation path.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn classify_with_facade(
        self,
        policy: &Policy,
        limits: &crate::limits::Limits,
    ) -> Result<(Contract<Classified>, crate::surface::facade::Cached), Error> {
        cached::classify_facade(self, policy, limits)
    }

    /// Consume an authenticated Graph into the private scalar-only evidence lane.
    #[cfg(feature = "rustc-driver")]
    #[allow(
        dead_code,
        reason = "minted only by an explicitly selected private evidence attempt"
    )]
    pub(crate) const fn into_classification_evidence(self) -> EvidenceCapability {
        EvidenceCapability { graph: self }
    }

    /// Consume complete raw API evidence and classify every selected path once.
    pub fn classify(
        self,
        policy: &Policy,
        limits: &crate::limits::Limits,
    ) -> Result<Contract<Classified>, Error> {
        #[cfg(not(target_arch = "wasm32"))]
        if self.node_chain().is_some() {
            return cached::classify(self, policy, limits);
        }
        self.classify_with_scratch(policy, Authority::production(limits))
            .map(|(contract, _scratch_bytes)| contract)
    }

    fn classify_with_scratch(
        self,
        policy: &Policy,
        authority: Authority,
    ) -> Result<(Contract<Classified>, usize), Error> {
        let (parts, scratch_bytes) = self.classify_parts(policy, authority)?;
        Ok((parts.into_contract(self, None), scratch_bytes))
    }
}

impl<P: api::Phase> api::Graph<P> {
    fn classify_parts(
        &self,
        policy: &Policy,
        authority: Authority,
    ) -> Result<(ClassifiedParts, usize), Error> {
        let semantic_api = *self.semantic_fingerprint().as_bytes();
        let context = Context::new(self, authority)?;
        let result = (|| {
            let mut outcomes = Vec::new();
            context.retain_array::<OutcomeRecord>(self.exports().len())?;
            outcomes
                .try_reserve_exact(self.exports().len())
                .map_err(|_| Error::Allocation)?;
            context.retain_array::<DefinitionPlan>(self.definitions().len())?;
            let mut definitions = Vec::new();
            definitions
                .try_reserve_exact(self.definitions().len())
                .map_err(|_| Error::Allocation)?;
            for (definition_index, definition) in self.definitions().enumerate() {
                context.retain_items(1)?;
                definitions.push(DefinitionPlan {
                    definition: crate::surface::identity::Definition::of(definition.id()),
                    representability: context.classify_definition(definition_index, definition)?,
                });
            }
            if definitions.len() != self.definitions().len() {
                return Err(Error::Cardinality);
            }
            definitions.sort_unstable();
            drop_unnameable_fields(self, &mut definitions);
            for path in self.exports() {
                outcomes.push(context.classify(path, &definitions)?);
            }
            if outcomes.len() != self.exports().len() {
                return Err(Error::Cardinality);
            }
            enforce_nameability(&context, &definitions, &mut outcomes)?;
            let canonical_value = Canonical {
                semantic_api: &semantic_api,
                schema: policy.schema,
                classifier: policy.classifier,
                compiler_abi: policy.compiler_abi,
                support: CanonicalSupport(&context.support),
                definitions: &definitions,
                outcomes: &outcomes,
            };
            let mut measured = Measured {
                bytes: 0,
                overflow: false,
            };
            let result = serde_json::to_writer(&mut measured, &canonical_value);
            if measured.overflow {
                return Err(arithmetic());
            }
            result.map_err(|_| Error::Encoding)?;
            context.retain(0, measured.bytes)?;
            // The retained targets were measured above; the cardinality proof
            // alone remains a gate.
            let allocation = contract::Usage::classified(
                self,
                &definitions,
                &outcomes,
                &context.support,
                measured.bytes,
                (context.retained_items.get(), context.retained_bytes.get()),
                usize::MAX,
                usize::MAX,
            )?;
            let mut canonical = Vec::new();
            canonical
                .try_reserve_exact(measured.bytes)
                .map_err(|_| Error::Allocation)?;
            serde_json::to_writer(&mut canonical, &canonical_value).map_err(|_| Error::Encoding)?;
            if canonical.len() != measured.bytes {
                return Err(Error::Encoding);
            }
            let fingerprint = contract::fingerprint::Fingerprint(Sha256::digest(&canonical).into());
            Ok((
                definitions.into_boxed_slice(),
                outcomes.into_boxed_slice(),
                canonical.into_boxed_slice(),
                fingerprint,
                allocation,
            ))
        })();
        context.observe_result(&result);
        let (definitions, outcomes, canonical, fingerprint, allocation) = result?;
        let support = context.support;
        let scratch_bytes = context.scratch_bytes.get();
        Ok((
            ClassifiedParts {
                definitions,
                outcomes,
                support,
                canonical,
                fingerprint,
                allocation,
            },
            scratch_bytes,
        ))
    }
}

impl ClassifiedParts {
    pub(crate) fn outcomes(&self) -> impl ExactSizeIterator<Item = Outcome<'_>> {
        self.outcomes
            .iter()
            .map(|outcome| Outcome::new(outcome, &self.definitions))
    }

    pub(crate) fn into_contract(
        self,
        graph: api::Graph<api::Extracted>,
        chain: Option<crate::node_parts::Chain>,
    ) -> Contract<Classified> {
        Contract::seal(
            graph,
            self.definitions,
            self.outcomes,
            self.support,
            self.canonical,
            self.fingerprint,
            self.allocation,
            chain,
        )
    }

    pub(super) fn decode(
        bytes: &[u8],
        graph: &api::Graph<impl api::Phase>,
        policy: &Policy,
    ) -> Result<Self, Error> {
        (|| {
            let stored: StoredCanonical =
                serde_json::from_slice(bytes).map_err(|_| Error::Canonical)?;
            let exact = serde_json::to_vec(&stored).map_err(|_| Error::Encoding)?;
            if exact.as_slice() != bytes
                || stored.semantic_api != *graph.semantic_fingerprint().as_bytes()
                || stored.schema != policy.schema
                || stored.classifier != policy.classifier
                || stored.compiler_abi != policy.compiler_abi
                || stored.definitions.windows(2).any(|pair| pair[0] >= pair[1])
                || stored.outcomes.len() != graph.exports().len()
            {
                return Err(Error::Canonical);
            }

            let definitions = stored.definitions;
            let mut outcomes = Vec::new();
            outcomes
                .try_reserve_exact(stored.outcomes.len())
                .map_err(|_| Error::Allocation)?;
            for outcome in stored.outcomes {
                outcomes.push(outcome.into_record(&definitions)?);
            }
            let outcomes = outcomes.into_boxed_slice();
            let support = SupportLedger::decode(stored.support, graph)?;
            let rebuilt = serde_json::to_vec(&Canonical {
                semantic_api: graph.semantic_fingerprint().as_bytes(),
                schema: policy.schema,
                classifier: policy.classifier,
                compiler_abi: policy.compiler_abi,
                support: CanonicalSupport(&support),
                definitions: &definitions,
                outcomes: &outcomes,
            })
            .map_err(|_| Error::Encoding)?;
            if rebuilt.as_slice() != bytes {
                return Err(Error::Canonical);
            }
            let retained =
                recount_retained_usage(graph, &definitions, &outcomes, &support, bytes.len())
                    .ok_or(Error::Cardinality)?;
            let allocation = contract::Usage::classified(
                graph,
                &definitions,
                &outcomes,
                &support,
                bytes.len(),
                retained,
                usize::MAX,
                usize::MAX,
            )?;
            Ok(Self {
                definitions,
                outcomes,
                support,
                canonical: rebuilt.into_boxed_slice(),
                fingerprint: contract::fingerprint::Fingerprint(Sha256::digest(bytes).into()),
                allocation,
            })
        })()
    }
}

impl StoredOutcome {
    fn into_record(self, definitions: &[DefinitionPlan]) -> Result<OutcomeRecord, Error> {
        let evidence = match (self.definition_plan, self.representability) {
            (Some(definition), None) => {
                let index = definitions
                    .binary_search_by_key(&definition, DefinitionPlan::definition)
                    .map_err(|_| Error::Canonical)?;
                OutcomeEvidence::Plan {
                    definition,
                    index: u32::try_from(index).map_err(|_| Error::IndexOverflow)?,
                }
            }
            (None, Some(representability)) => OutcomeEvidence::Inline(Box::new(representability)),
            _ => return Err(Error::Canonical),
        };
        let mut implementations = Vec::new();
        implementations
            .try_reserve_exact(self.implementations.len())
            .map_err(|_| Error::Allocation)?;
        for implementation in self.implementations {
            implementations.push(implementation.into_record(definitions)?);
        }
        Ok(OutcomeRecord {
            export: self.export,
            identity: self.identity,
            name: self.name,
            origin: self.origin,
            evidence,
            implementations: implementations.into_boxed_slice(),
            variant: self.variant,
            member: self.member,
        })
    }
}

impl StoredImplementation {
    fn into_record(self, definitions: &[DefinitionPlan]) -> Result<ImplementationRecord, Error> {
        let mut associated = Vec::new();
        associated
            .try_reserve_exact(self.associated.len())
            .map_err(|_| Error::Allocation)?;
        for outcome in self.associated {
            associated.push(outcome.into_record(definitions)?);
        }
        Ok(ImplementationRecord {
            identity: self.identity,
            span: self.span,
            associated: associated.into_boxed_slice(),
        })
    }
}

impl SupportLedger {
    fn decode(
        stored: Box<[StoredSupportOutcome]>,
        graph: &api::Graph<impl api::Phase>,
    ) -> Result<Self, Error> {
        if stored.len() != graph.support().len() {
            return Err(Error::Canonical);
        }
        let group_count = graph
            .definitions()
            .len()
            .checked_add(graph.implementations().len())
            .ok_or(Error::IndexOverflow)?;
        let mut groups: Vec<SupportGroup> = Vec::new();
        let mut edges = Vec::new();
        let mut rejections = Vec::new();
        groups
            .try_reserve_exact(group_count)
            .map_err(|_| Error::Allocation)?;
        groups.extend(graph.definitions().map(|definition| SupportGroup {
            provenance: SupportProvenance::Definition(crate::surface::identity::Definition::of(
                definition.id(),
            )),
            count: 0,
        }));
        for implementation in graph.implementations() {
            groups.push(SupportGroup {
                provenance: SupportProvenance::Implementation(
                    crate::surface::identity::Implementation::of(implementation)?,
                ),
                count: 0,
            });
        }
        groups.sort_unstable_by_key(|group| group.provenance);
        if groups.len() != group_count
            || groups
                .windows(2)
                .any(|groups| groups[0].provenance == groups[1].provenance)
        {
            return Err(Error::Canonical);
        }
        edges
            .try_reserve_exact(stored.len())
            .map_err(|_| Error::Allocation)?;
        rejections
            .try_reserve_exact(stored.len())
            .map_err(|_| Error::Allocation)?;
        for (outcome, expected) in stored.into_vec().into_iter().zip(graph.support()) {
            let expected_definition = crate::surface::identity::Definition::of(expected.id());
            let expected_provenance = match expected.role() {
                ledger::SupportRole::Definition => SupportProvenance::Definition(
                    crate::surface::identity::Definition::of(expected.provenance()),
                ),
                ledger::SupportRole::Implementation => {
                    let index = graph
                        .implementation_index(expected.provenance())
                        .ok_or(Error::Canonical)?;
                    let implementation =
                        graph.implementations().nth(index).ok_or(Error::Canonical)?;
                    SupportProvenance::Implementation(crate::surface::identity::Implementation::of(
                        implementation,
                    )?)
                }
            };
            let (definition, provenance, rejection) = match (outcome, expected) {
                (
                    StoredSupportOutcome::Complete {
                        definition,
                        provenance,
                    },
                    ledger::Support::Complete { .. },
                ) if definition == expected_definition && provenance == expected_provenance => {
                    (definition, provenance, None)
                }
                (
                    StoredSupportOutcome::Rejected {
                        definition,
                        provenance,
                        reason,
                        span,
                    },
                    ledger::Support::Rejected {
                        reason: expected_reason,
                        span: expected_span,
                        ..
                    },
                ) if definition == expected_definition
                    && provenance == expected_provenance
                    && reason == *expected_reason
                    && span.same_place(expected_span) =>
                {
                    (
                        definition,
                        provenance,
                        Some(SupportRejection { reason, span }),
                    )
                }
                _ => {
                    return Err(Error::Canonical);
                }
            };
            let group_index = groups
                .binary_search_by_key(&provenance, |group| group.provenance)
                .map_err(|_| Error::Canonical)?;
            let group = groups.get_mut(group_index).ok_or(Error::Canonical)?;
            group.count = group.count.checked_add(1).ok_or(Error::IndexOverflow)?;
            let group_index = u32::try_from(group_index).map_err(|_| Error::IndexOverflow)?;
            let rejection = if let Some(rejection) = rejection {
                rejections.push(rejection);
                let index = u32::try_from(rejections.len()).map_err(|_| Error::IndexOverflow)?;
                std::num::NonZeroU32::new(index)
                    .ok_or(Error::IndexOverflow)?
                    .into()
            } else {
                None
            };
            edges.push(SupportEdge {
                definition,
                group: group_index,
                rejection,
            });
        }
        Ok(Self {
            groups: groups.into_boxed_slice(),
            edges: edges.into_boxed_slice(),
            rejections: rejections.into_boxed_slice(),
        })
    }
}

#[cfg(feature = "rustc-driver")]
#[allow(
    dead_code,
    reason = "called only by an explicitly selected private evidence attempt"
)]
impl EvidenceCapability {
    pub(crate) fn measure(self, policy: &Policy) -> Result<ClassificationEvidence, EvidenceError> {
        let session = crate::process::EvidenceSession::new();
        let mut classified = None;
        session.observe_inline(crate::process::EvidenceInlineStage::Classification, || {
            classified = Some(
                self.graph
                    .classify_with_scratch(policy, Authority::evidence()),
            );
            Ok(())
        })?;
        let (contract, scratch_bytes) = classified.ok_or(EvidenceError::Cardinality)??;
        let stage = session.classification();
        let evidence = contract
            .classification_evidence(scratch_bytes, stage)
            .ok_or(EvidenceError::Cardinality)?;
        for bytes in [
            evidence.bytes.graph,
            evidence.bytes.contract_inline,
            evidence.bytes.definition_plans,
            evidence.bytes.outcomes,
            evidence.bytes.support,
            evidence.bytes.canonical,
            evidence.final_bytes,
        ] {
            if bytes > EVIDENCE_RETAINED_BYTES {
                return Err(Error::Limit {
                    resource: "classification evidence byte component",
                    limit: EVIDENCE_RETAINED_BYTES,
                    actual: bytes,
                }
                .into());
            }
        }
        if evidence.final_items > EVIDENCE_RETAINED_ITEMS {
            return Err(Error::Limit {
                resource: "classification evidence retained items",
                limit: EVIDENCE_RETAINED_ITEMS,
                actual: evidence.final_items,
            }
            .into());
        }
        drop(contract);
        Ok(evidence)
    }
}

/// Nameability facts read once per classification: definitions by
/// identity, the `(root, definition)` pairs some import exports, and each
/// settled verdict, so every outcome after the first reuses them instead of
/// re-scanning (and re-hashing) the whole graph per type it mentions.
struct Names<'graph> {
    plans: &'graph [DefinitionPlan],
    models:
        std::collections::BTreeMap<crate::surface::identity::Definition, &'graph model::Definition>,
    exported: std::collections::BTreeMap<
        &'graph str,
        std::collections::BTreeSet<crate::surface::identity::Definition>,
    >,
    /// Verdicts that assumed no definition on the walk's stack, so they hold
    /// for every later outcome under the same root.
    settled: std::collections::BTreeMap<
        Box<str>,
        std::collections::BTreeMap<crate::surface::identity::Definition, bool>,
    >,
    /// Per-outcome walk state, exactly as before: the stack and the verdicts
    /// that leaned on it.
    visiting: Vec<crate::surface::identity::Definition>,
    resolved: Vec<crate::surface::identity::Definition>,
    /// The opaque `impl Trait` types functions give, which a facade
    /// declares under names of its own.
    opaque: std::collections::BTreeSet<[u8; 32]>,
}

impl<'graph> Names<'graph> {
    fn new(graph: &'graph api::Graph<impl api::Phase>, plans: &'graph [DefinitionPlan]) -> Self {
        let models = graph
            .definitions()
            .map(|model| (crate::surface::identity::Definition::of(model.id()), model))
            .collect();
        let mut exported = exported_by_root(graph);
        // A type whose only name is a passing alias is named through it.
        for (root, _, target) in sole_passing_aliases(graph) {
            exported
                .entry(root)
                .or_default()
                .insert(crate::surface::identity::Definition::of(target.id()));
        }
        Self {
            plans,
            models,
            exported,
            settled: std::collections::BTreeMap::new(),
            visiting: Vec::new(),
            resolved: Vec::new(),
            opaque: crate::surface::opaque::identities(graph),
        }
    }
}

/// A public field whose type no import names (`Arc<reqwest::Error>` in an
/// error enum) is unavailable by itself, as a field Rawr cannot hold is. The
/// struct or enum stays, and a struct or variant that lost a field cannot be
/// built. A field's type is nameable when every definition in it is an
/// exported struct or enum, so the verdict does not lean on fields.
fn drop_unnameable_fields(graph: &api::Graph<impl api::Phase>, plans: &mut [DefinitionPlan]) {
    use representability::Representability as R;
    let mut exported = std::collections::BTreeSet::new();
    for path in graph.exports() {
        if let ledger::Path::Imported(export) = path {
            exported.insert(crate::surface::identity::Definition::of(export.target()));
        }
    }
    for (_, _, target) in sole_passing_aliases(graph) {
        exported.insert(crate::surface::identity::Definition::of(target.id()));
    }
    let nominal = graph
        .definitions()
        .map(|model| (crate::surface::identity::Definition::of(model.id()), model))
        .filter(|(definition, model)| {
            exported.contains(definition)
                && model.display_path().all(facade_segment_nameable)
                && matches!(
                    shared_plan(plans, *definition),
                    Some(R::Struct(_) | R::Enum(_))
                )
        })
        .map(|(definition, _)| definition)
        .collect::<std::collections::BTreeSet<_>>();
    let models = graph
        .definitions()
        .map(|model| (crate::surface::identity::Definition::of(model.id()), model))
        .collect::<std::collections::BTreeMap<_, _>>();
    for plan in plans {
        let keep =
            |field: &representability::structure::Field| field_type_nameable(field.ty(), &nominal);
        let model = models.get(&plan.definition).copied();
        // The field as the crate writes it, which a literal's message names.
        let lost = |field: &representability::structure::Field| {
            let ty = model.and_then(|model| match (model.kind(), field.selector()) {
                (model::Kind::Struct { fields, generics }, _) => fields
                    .public_fields()
                    .iter()
                    .find(|declared| declared.index() == field.index())
                    .map(|declared| written(graph, generics, declared.ty())),
                (
                    model::Kind::Enum { variants, generics },
                    crate::surface::identity::Selector::EnumVariant { index, .. },
                ) => variants
                    .get(*index as usize)?
                    .fields()
                    .find(|declared| declared.index() == field.index())
                    .map(|declared| written(graph, generics, declared.ty())),
                _ => None,
            });
            representability::structure::Unsupported::new(
                field.name().into(),
                ty.unwrap_or_default().into(),
            )
        };
        match &mut plan.representability {
            R::Struct(structure) => structure.retain_fields(keep, lost),
            R::Enum(enumeration) => enumeration.retain_fields(keep, lost),
            _ => {}
        }
    }
}

/// `ty` as Rust writes it in a message: each definition by its own name,
/// each parameter by the name `generics` gives it, and lifetimes elided.
fn written(
    graph: &api::Graph<impl api::Phase>,
    generics: &model::Generics,
    ty: &model::Type,
) -> String {
    let mut text = String::new();
    write_type(graph, generics, ty, &mut text);
    text
}

fn write_type(
    graph: &api::Graph<impl api::Phase>,
    generics: &model::Generics,
    ty: &model::Type,
    text: &mut String,
) {
    let name = |id: &api::identity::Definition| {
        graph
            .definition_index(id)
            .and_then(|index| graph.definitions().nth(index))
            .and_then(|definition| definition.display_path().last())
            .unwrap_or("_")
            .to_owned()
    };
    let parameter = |index: u32| {
        generics
            .parameters()
            .find(|parameter| parameter.index() == index)
            .map_or("_", model::Parameter::name)
            .to_owned()
    };
    let arguments = |arguments: &[model::Argument], text: &mut String| {
        let mut first = true;
        for argument in arguments {
            let before = text.len();
            text.push_str(if first { "<" } else { ", " });
            match argument {
                model::Argument::Type(ty) => write_type(graph, generics, ty, text),
                model::Argument::Const(value) => write_const(generics, value, text),
                model::Argument::Lifetime(_) => {
                    text.truncate(before);
                    continue;
                }
            }
            first = false;
        }
        if !first {
            text.push('>');
        }
    };
    match ty {
        model::Type::Primitive(primitive) => text.push_str(primitive),
        model::Type::Definition {
            id,
            arguments: given,
        } => {
            text.push_str(&name(id));
            arguments(given, text);
        }
        model::Type::Parameter { index, .. } => text.push_str(&parameter(*index)),
        model::Type::SelfType(_) => text.push_str("Self"),
        model::Type::Tuple(members) => {
            text.push('(');
            for (position, member) in members.iter().enumerate() {
                if position > 0 {
                    text.push_str(", ");
                }
                write_type(graph, generics, member, text);
            }
            if members.len() == 1 {
                text.push(',');
            }
            text.push(')');
        }
        model::Type::Slice(element) => {
            text.push('[');
            write_type(graph, generics, element, text);
            text.push(']');
        }
        model::Type::Array { element, length } => {
            text.push('[');
            write_type(graph, generics, element, text);
            text.push_str("; ");
            write_const(generics, length, text);
            text.push(']');
        }
        model::Type::Reference { mutability, ty, .. } => {
            text.push_str(match mutability {
                model::Mutability::Shared => "&",
                model::Mutability::Mutable => "&mut ",
            });
            write_type(graph, generics, ty, text);
        }
        model::Type::RawPointer { mutability, ty } => {
            text.push_str(match mutability {
                model::Mutability::Shared => "*const ",
                model::Mutability::Mutable => "*mut ",
            });
            write_type(graph, generics, ty, text);
        }
        model::Type::FunctionPointer(signature) => {
            text.push_str("fn(");
            for (position, input) in signature.inputs().enumerate() {
                if position > 0 {
                    text.push_str(", ");
                }
                write_type(graph, generics, input, text);
            }
            text.push(')');
            if !matches!(signature.output(), model::Type::Tuple(members) if members.is_empty()) {
                text.push_str(" -> ");
                write_type(graph, generics, signature.output(), text);
            }
        }
        model::Type::TraitObject { bounds, .. } => {
            text.push_str("dyn ");
            let mut first = true;
            for bound in bounds {
                if let model::ObjectBound::Trait(model::Bound::Trait {
                    trait_id,
                    arguments: given,
                    ..
                }) = bound
                {
                    if !first {
                        text.push_str(" + ");
                    }
                    text.push_str(&name(trait_id));
                    arguments(given, text);
                    first = false;
                }
            }
        }
        model::Type::ImplTrait { bounds, .. } => {
            text.push_str("impl ");
            let mut first = true;
            for bound in bounds {
                if let model::Predicate::Bound {
                    bound: model::Bound::Trait { trait_id, .. },
                    ..
                } = bound
                {
                    if !first {
                        text.push_str(" + ");
                    }
                    text.push_str(&name(trait_id));
                    first = false;
                }
            }
        }
        model::Type::Projection {
            trait_id,
            associated,
            arguments: given,
        } => {
            text.push('<');
            match given.first() {
                Some(model::Argument::Type(ty)) => write_type(graph, generics, ty, text),
                _ => text.push_str("Self"),
            }
            text.push_str(" as ");
            text.push_str(&name(trait_id));
            text.push_str(">::");
            text.push_str(&name(associated));
        }
        model::Type::Never => text.push('!'),
    }
}

fn write_const(generics: &model::Generics, value: &model::Const, text: &mut String) {
    match value {
        model::Const::Literal { value, .. } => {
            text.push_str(value.trim_end_matches("_usize").trim_end_matches("usize"))
        }
        model::Const::Parameter { index, .. } => text.push_str(
            generics
                .parameters()
                .find(|parameter| parameter.index() == *index)
                .map_or("_", model::Parameter::name),
        ),
        model::Const::Unevaluated { .. } => text.push('_'),
    }
}

fn field_type_nameable(
    ty: &representability::convert::Type,
    nominal: &std::collections::BTreeSet<crate::surface::identity::Definition>,
) -> bool {
    use representability::convert::Type;
    match ty {
        Type::Nominal {
            definition,
            arguments,
        } => {
            // Or a type Rawr declares itself, as std's `PathBuf` and
            // `Duration` are.
            (nominal.contains(definition)
                || crate::surface::identity::Definition::is_projected(definition.as_bytes()))
                && arguments
                    .iter()
                    .all(|argument| field_type_nameable(argument, nominal))
        }
        Type::FunctionItem { input, output } => {
            field_type_nameable(input, nominal) && field_type_nameable(output, nominal)
        }
        Type::ProfileMessage { payload, .. }
        | Type::Option(payload)
        | Type::Borrowed { ty: payload, .. }
        | Type::Range(payload)
        | Type::Sequence {
            element: payload, ..
        }
        | Type::ConstArray {
            element: payload, ..
        } => field_type_nameable(payload, nominal),
        Type::Result(ok, err) => {
            field_type_nameable(ok, nominal) && field_type_nameable(err, nominal)
        }
        Type::Tuple(members)
        | Type::Script {
            arguments: members, ..
        } => members
            .iter()
            .all(|member| field_type_nameable(member, nominal)),
        Type::Inferred => true,
        Type::Unit | Type::Never | Type::Builtin(_) | Type::Parameter(_) => true,
    }
}

fn enforce_nameability(
    context: &Context<'_, impl api::Phase>,
    plans: &[DefinitionPlan],
    outcomes: &mut [OutcomeRecord],
) -> Result<(), Error> {
    context.scratch_arrays::<crate::surface::identity::Definition>(plans.len(), 2)?;
    let mut names = Names::new(context.graph, plans);
    names
        .visiting
        .try_reserve_exact(plans.len())
        .map_err(|_| Error::Allocation)?;
    names
        .resolved
        .try_reserve_exact(plans.len())
        .map_err(|_| Error::Allocation)?;
    for outcome in outcomes {
        enforce_outcome_nameability(context, &mut names, outcome)?;
    }
    Ok(())
}

fn enforce_outcome_nameability(
    context: &Context<'_, impl api::Phase>,
    names: &mut Names<'_>,
    outcome: &mut OutcomeRecord,
) -> Result<(), Error> {
    names.visiting.clear();
    names.resolved.clear();
    let exported_path = facade_segment_nameable(outcome.export.root_alias())
        && outcome.export.path().all(facade_segment_nameable);
    let nameable = exported_path
        && (!matches!(outcome.evidence, OutcomeEvidence::Plan { .. })
            || representability_nameable(
                outcome.representability(names.plans),
                outcome.export.root_alias(),
                names,
            )
            .0);
    if !nameable {
        let span = outcome
            .origin
            .as_ref()
            .ok_or(Error::Cardinality)?
            .declaration();
        context.retain_span(span)?;
        context.retain_inline_representability()?;
        let span = span.clone();
        outcome.evidence =
            OutcomeEvidence::Inline(Box::new(representability::Representability::Rejected(
                representability::reject::Reason::classification(
                    representability::reject::Kind::UnnameableType,
                    span,
                ),
            )));
    }
    for implementation in &mut outcome.implementations {
        for associated in &mut implementation.associated {
            enforce_outcome_nameability(context, names, associated)?;
        }
    }
    Ok(())
}

/// A verdict and whether it leaned on a definition still on the stack (a
/// cycle assumed nameable), which keeps it out of the settled table.
type Verdict = (bool, bool);

fn all_nameable<'t>(
    types: impl IntoIterator<Item = &'t representability::convert::Type>,
    root: &str,
    names: &mut Names<'_>,
) -> Verdict {
    let mut assumed = false;
    for ty in types {
        let (nameable, leaned) = type_nameable(ty, root, names);
        assumed |= leaned;
        if !nameable {
            return (false, assumed);
        }
    }
    (true, assumed)
}

fn representability_nameable(
    representability: &representability::Representability,
    root: &str,
    names: &mut Names<'_>,
) -> Verdict {
    use representability::Representability as R;
    match representability {
        R::Value(plan) => type_nameable(plan.ty(), root, names),
        R::Struct(plan) => all_nameable(plan.fields().iter().map(|field| field.ty()), root, names),
        R::Enum(plan) => all_nameable(
            plan.variants()
                .iter()
                .flat_map(|variant| variant.fields().iter().map(|field| field.ty())),
            root,
            names,
        ),
        R::Function(plan) => signature_nameable(plan.signature(), root, names),
        R::Method(plan) => signature_nameable(plan.signature(), root, names),
        R::Constant(plan) => type_nameable(plan.ty(), root, names),
        R::FunctionItem(plan) => signature_nameable(plan.signature(), root, names),
        R::Invocation(plan) => signature_nameable(plan.signature(), root, names),
        R::Excluded(_) | R::Rejected(_) => (true, false),
    }
}

fn signature_nameable(
    signature: &representability::signature::Signature,
    root: &str,
    names: &mut Names<'_>,
) -> Verdict {
    all_nameable(
        signature
            .inputs()
            .iter()
            .chain(std::iter::once(signature.output())),
        root,
        names,
    )
}

fn type_nameable(
    ty: &representability::convert::Type,
    root: &str,
    names: &mut Names<'_>,
) -> Verdict {
    match ty {
        representability::convert::Type::Nominal {
            definition,
            arguments,
        } => {
            let (nameable, assumed) = all_nameable(arguments.iter(), root, names);
            if !nameable {
                return (false, assumed);
            }
            // Rawr declares a future, a duration, an instant and the like
            // itself; no Rust definition names them.
            if crate::surface::identity::Definition::is_projected(definition.as_bytes())
                || names.opaque.contains(definition.as_bytes())
            {
                return (true, assumed);
            }
            let (nameable, leaned) = definition_nameable(*definition, root, names);
            (nameable, assumed | leaned)
        }
        representability::convert::Type::FunctionItem { input, output } => {
            all_nameable([&**input, &**output], root, names)
        }
        representability::convert::Type::ProfileMessage { payload, .. }
        | representability::convert::Type::Option(payload)
        | representability::convert::Type::Borrowed { ty: payload, .. }
        | representability::convert::Type::Range(payload)
        | representability::convert::Type::Sequence {
            element: payload, ..
        }
        | representability::convert::Type::ConstArray {
            element: payload, ..
        } => type_nameable(payload, root, names),
        representability::convert::Type::Result(ok, err) => {
            all_nameable([&**ok, &**err], root, names)
        }
        representability::convert::Type::Tuple(members)
        | representability::convert::Type::Script {
            arguments: members, ..
        } => all_nameable(members.iter(), root, names),
        representability::convert::Type::Inferred => (true, false),
        representability::convert::Type::Unit
        | representability::convert::Type::Never
        | representability::convert::Type::Builtin(_)
        | representability::convert::Type::Parameter(_) => (true, false),
    }
}

fn definition_nameable(
    definition: crate::surface::identity::Definition,
    root: &str,
    names: &mut Names<'_>,
) -> Verdict {
    if let Some(nameable) = names
        .settled
        .get(root)
        .and_then(|settled| settled.get(&definition))
    {
        return (*nameable, false);
    }
    if names.visiting.contains(&definition) {
        return (true, true);
    }
    if names.resolved.contains(&definition) {
        return (true, true);
    }
    let Some(model) = names.models.get(&definition).copied() else {
        return settle(names, root, definition, false);
    };
    if !model.display_path().all(facade_segment_nameable) {
        return settle(names, root, definition, false);
    }
    // A type is nameable through any import that exports it. A function of
    // one crate may take or return a type another declared crate exports.
    let exported = names
        .exported
        .values()
        .any(|exported| exported.contains(&definition));
    let Some(plan) = shared_plan(names.plans, definition) else {
        return settle(names, root, definition, false);
    };
    if !exported
        || !matches!(
            plan,
            representability::Representability::Struct(_)
                | representability::Representability::Enum(_)
        )
    {
        return settle(names, root, definition, false);
    }
    names.visiting.push(definition);
    let (nameable, leaned) = representability_nameable(plan, root, names);
    names.visiting.pop();
    // Only the outermost assumption on the stack can lean on itself; once this
    // definition leaves the stack, a verdict that leaned only on it is exact
    // for no other walk, so any lean keeps it per-outcome.
    if leaned {
        if nameable {
            names.resolved.push(definition);
        }
        return (nameable, true);
    }
    settle(names, root, definition, nameable)
}

fn settle(
    names: &mut Names<'_>,
    root: &str,
    definition: crate::surface::identity::Definition,
    nameable: bool,
) -> Verdict {
    match names.settled.get_mut(root) {
        Some(settled) => {
            settled.insert(definition, nameable);
        }
        None => {
            names.settled.insert(
                root.into(),
                std::collections::BTreeMap::from([(definition, nameable)]),
            );
        }
    }
    (nameable, false)
}

fn shared_plan(
    plans: &[DefinitionPlan],
    definition: crate::surface::identity::Definition,
) -> Option<&representability::Representability> {
    plans
        .binary_search_by_key(&definition, DefinitionPlan::definition)
        .ok()
        .map(|index| plans[index].representability())
}

fn plan_evidence(
    definitions: &[DefinitionPlan],
    definition: &model::Definition,
) -> Result<OutcomeEvidence, Error> {
    let definition = crate::surface::identity::Definition::of(definition.id());
    let index = definitions
        .binary_search_by_key(&definition, DefinitionPlan::definition)
        .map_err(|_| Error::Cardinality)?;
    let index = u32::try_from(index).map_err(|_| Error::Limit {
        resource: "classification definition plan indices",
        limit: u32::MAX as usize,
        actual: definitions.len(),
    })?;
    Ok(OutcomeEvidence::Plan { definition, index })
}

impl OutcomeRecord {
    fn representability<'a>(
        &'a self,
        definitions: &'a [DefinitionPlan],
    ) -> &'a representability::Representability {
        match &self.evidence {
            OutcomeEvidence::Plan { definition, index } => {
                let plan = &definitions[*index as usize];
                debug_assert_eq!(plan.definition(), *definition);
                plan.representability()
            }
            OutcomeEvidence::Inline(representability) => representability,
        }
    }
}

impl<P: api::Phase> Context<'_, P> {
    fn classify(
        &self,
        path: &ledger::Path,
        definitions: &[DefinitionPlan],
    ) -> Result<OutcomeRecord, Error> {
        match path {
            ledger::Path::Excluded {
                reason,
                policy_span,
                source_span,
                ..
            } => {
                let name = export_name_ref(path.key());
                self.retain_outcome_common(path.key(), name)?;
                self.retain_str(reason)?;
                self.retain_span(policy_span)?;
                self.retain_span(source_span)?;
                self.retain_inline_representability()?;
                Ok(OutcomeRecord {
                    export: path.key().clone(),
                    identity: crate::surface::identity::Identity::unavailable(path.key())?,
                    name: name.into(),
                    origin: None,
                    evidence: OutcomeEvidence::Inline(Box::new(
                        representability::Representability::Excluded(
                            representability::exclude::Reason::policy(
                                reason.clone(),
                                policy_span.clone(),
                                source_span.clone(),
                            ),
                        ),
                    )),
                    implementations: Box::default(),
                    variant: None,
                    member: None,
                })
            }
            ledger::Path::Rejected { reason, span, .. } => {
                let name = export_name_ref(path.key());
                self.retain_outcome_common(path.key(), name)?;
                self.retain_ledger_reason(reason)?;
                self.retain_span(span)?;
                self.retain_inline_representability()?;
                Ok(OutcomeRecord {
                    export: path.key().clone(),
                    identity: crate::surface::identity::Identity::unavailable(path.key())?,
                    name: name.into(),
                    origin: None,
                    evidence: OutcomeEvidence::Inline(Box::new(
                        representability::Representability::Rejected(
                            representability::reject::Reason::extraction(reason, span.clone()),
                        ),
                    )),
                    implementations: Box::default(),
                    variant: None,
                    member: None,
                })
            }
            ledger::Path::Imported(export) => {
                let definition_index = self
                    .graph
                    .definition_index(export.target())
                    .ok_or(Error::Cardinality)?;
                let definition = self
                    .graph
                    .definitions()
                    .nth(definition_index)
                    .ok_or(Error::Cardinality)?;
                // `pub type Duration = TimeDelta;` is another path to the
                // type it names, with that type's members. So is an alias
                // that passes its parameters on to a type no import names
                // otherwise, as iced's `widget::Text` is
                // `core::widget::Text`'s only name.
                let aliased = self
                    .alias_target(definition)
                    .or_else(|| self.sole_alias_target(definition));
                let retargeted = aliased.map(|target| export.retargeted(target.id().clone()));
                let export = retargeted.as_ref().unwrap_or(export);
                let definition = aliased.unwrap_or(definition);
                let implementation = match definition.owner() {
                    model::Owner::Implementation(owner) => {
                        let index = self
                            .graph
                            .implementation_index(owner)
                            .ok_or(Error::Cardinality)?;
                        Some(
                            self.graph
                                .implementations()
                                .nth(index)
                                .ok_or(Error::Cardinality)?,
                        )
                    }
                    model::Owner::Free | model::Owner::Definition(_) => None,
                };
                let selector = if self.instance_aliases().contains(&definition_index) {
                    crate::surface::identity::Selector::Declaration
                } else {
                    selector(definition)?
                };
                self.retain_selector(&selector)?;
                let identity =
                    crate::surface::identity::Identity::imported(export, implementation, selector)?;
                let name = definition.display_path().last().unwrap_or("<unnamed>");
                self.retain_outcome_common(export.key(), name)?;
                self.retain_span(export.span())?;
                self.retain_span(definition.span())?;
                let origin = representability::origin::Set::new(
                    export.span().clone(),
                    definition.span().clone(),
                );
                let evidence = plan_evidence(definitions, definition)?;
                // A re-exported variant is its enum's variant, whose members
                // are the enum's.
                let variant = match export.variant() {
                    Some(variant) => {
                        self.retain_str(variant)?;
                        Some(Box::from(variant))
                    }
                    None => None,
                };
                // An alias that passes its parameters on to a type, as
                // iced's `widget::Text<'a, Theme = crate::Theme>` is
                // `core::widget::Text<'a, Theme>`, stays an alias, with its
                // defaults, and has that type's members: `$crate::Text::new`
                // in `text!` is `core::widget::Text::new`.
                let passed_on = aliased
                    .is_none()
                    .then(|| self.passed_on_target(definition))
                    .flatten()
                    .map(|target| export.retargeted(target.id().clone()));
                let implementations = if variant.is_some() {
                    Box::default()
                } else if let Some(passed_on) = &passed_on {
                    self.implementations(passed_on, definitions)?
                } else {
                    let mut records = self.implementations(export, definitions)?.into_vec();
                    records.extend(self.opaque_implementations(export, definition)?);
                    records.extend(self.self_member_implementations(export, definition)?);
                    records.into_boxed_slice()
                };
                Ok(OutcomeRecord {
                    export: export.key().clone(),
                    identity,
                    name: name.into(),
                    origin: Some(origin),
                    evidence,
                    implementations,
                    variant,
                    member: None,
                })
            }
        }
    }

    /// The type a non-generic alias of a non-generic struct or enum names.
    fn alias_target(&self, definition: &model::Definition) -> Option<&model::Definition> {
        let model::Kind::TypeAlias { ty, generics } = definition.kind() else {
            return None;
        };
        if generics.parameters().len() != 0 || generics.predicates().len() != 0 {
            return None;
        }
        let model::Type::Definition { id, arguments } = ty else {
            return None;
        };
        if !arguments.is_empty() {
            return None;
        }
        let target = self
            .graph
            .definitions()
            .nth(self.graph.definition_index(id)?)?;
        matches!(
            target.kind(),
            model::Kind::Struct { .. } | model::Kind::Enum { .. }
        )
        .then_some(target)
    }

    /// The struct or enum a generic alias names with each of its own type
    /// parameters, in order, and nothing else (`passing_target`).
    fn passed_on_target(&self, definition: &model::Definition) -> Option<&model::Definition> {
        passing_target(self.graph, definition)
    }

    /// The struct or enum whose only name is the passing alias
    /// `definition` (`sole_passing_aliases`).
    fn sole_alias_target(&self, definition: &model::Definition) -> Option<&model::Definition> {
        let target = passing_target(self.graph, definition)?;
        let index = self.graph.definition_index(target.id())?;
        self.sole_aliases()
            .binary_search_by_key(&index, |(target, _)| *target)
            .is_ok()
            .then_some(target)
    }

    /// Each type whose only name is a passing alias, with that alias, by
    /// definition index (`sole_passing_aliases`).
    fn sole_aliases(&self) -> &[(usize, usize)] {
        self.sole_aliases.get_or_init(|| {
            let mut found = sole_passing_aliases(self.graph)
                .into_iter()
                .filter_map(|(_, alias, target)| {
                    Some((
                        self.graph.definition_index(target.id())?,
                        self.graph.definition_index(alias.id())?,
                    ))
                })
                .collect::<Vec<_>>();
            // The first alias in export order names the type, as the
            // driver's walk takes the first.
            found.sort_by_key(|(target, _)| *target);
            found.dedup_by_key(|(target, _)| *target);
            found.into_boxed_slice()
        })
    }

    /// A type's parameters with the defaults of the passing alias that is
    /// its only name: Rust code writes the type through the alias, so
    /// `widget::Text` takes `Theme = crate::Theme` though
    /// `core::widget::Text` has no default.
    fn sole_alias_defaults(
        &self,
        definition: usize,
        parameters: Box<[representability::signature::Parameter]>,
    ) -> Result<Box<[representability::signature::Parameter]>, Failure> {
        let Ok(found) = self
            .sole_aliases()
            .binary_search_by_key(&definition, |(target, _)| *target)
        else {
            return Ok(parameters);
        };
        let alias = self.sole_aliases()[found].1;
        let Some(model::Kind::TypeAlias { generics, .. }) = self
            .graph
            .definitions()
            .nth(alias)
            .map(model::Definition::kind)
        else {
            return Ok(parameters);
        };
        let defaults = generics
            .parameters()
            .filter_map(|parameter| match parameter.kind() {
                model::ParameterKind::Type { default, .. } => Some(default),
                _ => None,
            })
            .collect::<Vec<_>>();
        if defaults.len() != parameters.len() {
            return Ok(parameters);
        }
        let mut parameters = parameters.into_vec();
        for (parameter, default) in parameters.iter_mut().zip(defaults) {
            // A default Rawr cannot hold leaves the argument required.
            let default = match default {
                model::Default::Present(ty) => match self.convert(ty) {
                    Ok(ty) => Some(ty),
                    Err(Failure::Reject(_)) => None,
                    Err(error) => return Err(error),
                },
                model::Default::Absent => None,
            };
            *parameter = parameter.clone().with_default(default);
        }
        Ok(parameters.into_boxed_slice())
    }

    fn implementations(
        &self,
        export: &ledger::Export,
        definitions: &[DefinitionPlan],
    ) -> Result<Box<[ImplementationRecord]>, Error> {
        let mut outcomes = Vec::new();
        let alias = self
            .graph
            .definition_index(export.target())
            .and_then(|index| self.instance_aliases().contains(&index).then_some(index));
        let alias = alias.and_then(|index| self.graph.definitions().nth(index));
        let instance = alias.and_then(|alias| match alias.kind() {
            model::Kind::TypeAlias { ty, .. } => Some(ty),
            _ => None,
        });
        let target = match instance {
            Some(model::Type::Definition { id, .. }) => id,
            _ => export.target(),
        };
        for implementation in self.graph.relevant_implementations(target) {
            let substitutions = match instance {
                Some(instance) => {
                    let Some(substitutions) = instance_substitutions(
                        implementation.self_type(),
                        instance,
                        implementation.id(),
                    ) else {
                        continue;
                    };
                    Some(substitutions)
                }
                None => None,
            };
            self.retain_items(1)?;
            self.retain_items(implementation.associated().len())?;
            self.retain_array::<ImplementationRecord>(1)?;
            outcomes.try_reserve(1).map_err(|_| Error::Allocation)?;
            let identity = crate::surface::identity::Implementation::of(implementation)?;
            let mut associated = Vec::new();
            self.retain_array::<OutcomeRecord>(implementation.associated().len())?;
            associated
                .try_reserve_exact(implementation.associated().len())
                .map_err(|_| Error::Allocation)?;
            for id in implementation.associated() {
                let definition_index = self.graph.definition_index(id).ok_or(Error::Cardinality)?;
                let definition = self
                    .graph
                    .definitions()
                    .nth(definition_index)
                    .ok_or(Error::Cardinality)?;
                let selector = match selector(definition)? {
                    // A method of a trait's impl for `&str` is that impl's
                    // own (`surface::text`).
                    crate::surface::identity::Selector::Method { receiver, .. }
                        if is_str_reference(implementation.self_type())
                            && matches!(implementation.trait_ref(), model::Trait::Trait { id, .. } if id == export.target()) =>
                    {
                        crate::surface::identity::Selector::Method {
                            item: crate::surface::identity::Definition::text_member(
                                implementation.id(),
                                definition.id(),
                            ),
                            receiver,
                        }
                    }
                    // A trait's provided method that two of the type's own
                    // impls of the trait share, as `PartialOrd::lt` of
                    // `Degrees`'s `PartialOrd` and `PartialOrd<f32>` (T412):
                    // each impl's own, so a call selects one by its argument.
                    crate::surface::identity::Selector::Method { receiver, .. }
                        if self.shared_provided(implementation, definition) =>
                    {
                        crate::surface::identity::Selector::Method {
                            item: crate::surface::identity::Definition::text_member(
                                implementation.id(),
                                definition.id(),
                            ),
                            receiver,
                        }
                    }
                    // An associated function the trait declares, as the
                    // member of a blanket impl's application is: no
                    // receiver tells one type's from another's, as
                    // `Hsl::from_color` and `Rgb::from_color` of palette's
                    // `impl<T, U> FromColor<T> for U` are, so each impl's
                    // is its own.
                    crate::surface::identity::Selector::AssociatedFunction { .. }
                        if trait_member(implementation, definition) =>
                    {
                        crate::surface::identity::Selector::AssociatedFunction {
                            item: crate::surface::identity::Definition::text_member(
                                implementation.id(),
                                definition.id(),
                            ),
                        }
                    }
                    selector => selector,
                };
                self.retain_selector(&selector)?;
                let item_identity = crate::surface::identity::Identity::imported(
                    export,
                    Some(implementation),
                    selector,
                )?;
                let name = definition.display_path().last().unwrap_or("<unnamed>");
                self.retain_outcome_common(export.key(), name)?;
                self.retain_span(export.span())?;
                self.retain_span(definition.span())?;
                let origin = representability::origin::Set::new(
                    export.span().clone(),
                    definition.span().clone(),
                );
                let evidence = match &substitutions {
                    Some(substitutions) => {
                        self.classify_alias_member(implementation, definition, substitutions)?
                    }
                    None => self.classify_associated(
                        implementation,
                        definition,
                        definitions,
                        export.target(),
                    )?,
                };
                let implementations = match &substitutions {
                    Some(_) => Box::default(),
                    None => {
                        self.inherent_opaque_implementations(export, implementation, definition)?
                    }
                };
                associated.push(OutcomeRecord {
                    export: export.key().clone(),
                    identity: item_identity,
                    name: name.into(),
                    origin: Some(origin),
                    evidence,
                    implementations,
                    variant: None,
                    member: trait_member(implementation, definition)
                        .then(|| crate::surface::identity::Definition::of(definition.id())),
                });
            }
            self.retain_span(implementation.span())?;
            outcomes.push(ImplementationRecord {
                identity,
                span: implementation.span().clone(),
                associated: associated.into_boxed_slice(),
            });
        }
        Ok(outcomes.into_boxed_slice())
    }

    /// An opaque alias names one exact instance of an otherwise unnameable
    /// type. Its methods use that instance's arguments, while the bridge
    /// calls through the public alias and rustc checks the impl's bounds.
    fn classify_alias_member(
        &self,
        implementation: &model::Implementation,
        definition: &model::Definition,
        substitutions: &[((api::identity::Definition, u32), model::Type)],
    ) -> Result<OutcomeEvidence, Error> {
        use representability::{callable, reject, Representability};
        let retained_before = (self.retained_items.get(), self.retained_bytes.get());
        let result = (|| -> Result<Representability, Failure> {
            if implementation.safety() != model::Safety::Safe
                || implementation.polarity() != model::Polarity::Positive
                || implementation.const_impl()
                || implementation.default_impl()
            {
                return Err(reject::Kind::GenericImplementation.into());
            }
            let model::Kind::Function {
                signature,
                generics,
            } = definition.kind()
            else {
                return Err(reject::Kind::GenericImplementation.into());
            };
            let provided = provided_signature(implementation, definition, signature, generics)?;
            let (signature, generics) = provided
                .as_ref()
                .map_or((signature, generics), |(signature, generics)| {
                    (signature, generics)
                });
            // A trait's provided method takes `Self` as the alias's
            // instance, as iced's `with_translation(&mut self, v, f: impl
            // FnOnce(&mut Self))` lends `&mut Renderer` to its closure.
            let instance = |ty: &model::Type| {
                let own = match implementation.trait_ref() {
                    model::Trait::Trait { id, .. } => {
                        self_replaced(ty, id, implementation.self_type())
                    }
                    _ => ty.clone(),
                };
                substitute_parameters(&own, substitutions)
            };
            let signature = map_signature_types(signature, instance);
            let generics = map_generics_types(generics, instance);
            match self.function(&signature, &generics)? {
                Representability::FunctionItem(item) => Ok(Representability::Invocation(
                    callable::Invocation::new(item, 0),
                )),
                plan => Ok(plan),
            }
        })();
        let plan = match result {
            Ok(plan) => plan,
            Err(Failure::Reject(kind)) => {
                self.retained_items.set(retained_before.0);
                self.retained_bytes.set(retained_before.1);
                self.retain_span(implementation.span())?;
                Representability::Rejected(reject::Reason::classification(
                    kind,
                    implementation.span().clone(),
                ))
            }
            Err(Failure::Operational(error)) => return Err(error),
        };
        self.retain_inline_representability()?;
        Ok(OutcomeEvidence::Inline(Box::new(plan)))
    }

    /// The methods of each trait an opaque `impl Trait` type the function
    /// `definition` gives implements, as members of that type: iced's
    /// `iced(..) -> impl Widget<..>` gives a value whose `Widget::_boxed`
    /// `column!` calls. Each is the trait's method with the trait's
    /// arguments the bound's, `Self` the opaque type, and the function's
    /// type parameters, which the opaque type's arguments are, its own.
    fn opaque_implementations(
        &self,
        export: &ledger::Export,
        definition: &model::Definition,
    ) -> Result<Vec<ImplementationRecord>, Error> {
        let model::Kind::Function {
            signature,
            generics,
        } = definition.kind()
        else {
            return Ok(Vec::new());
        };
        let mut opaques = Vec::new();
        impl_traits(signature.output(), &mut opaques);
        self.opaque_records(export, definition, generics, &opaques)
    }

    /// The methods of each trait an opaque `impl Trait` type the inherent
    /// method `method` of `implementation` gives implements, as
    /// `opaque_implementations` gives a function's: polars'
    /// `ChunkedArray::downcast_iter_mut` gives an `impl Iterator` whose
    /// `next` a program calls. The impl's parameters come first, then the
    /// method's own, as the method's signature declares them.
    fn inherent_opaque_implementations(
        &self,
        export: &ledger::Export,
        implementation: &model::Implementation,
        method: &model::Definition,
    ) -> Result<Box<[ImplementationRecord]>, Error> {
        let model::Kind::Function {
            signature,
            generics,
        } = method.kind()
        else {
            return Ok(Box::default());
        };
        if !matches!(implementation.trait_ref(), model::Trait::Inherent) {
            return Ok(Box::default());
        }
        let mut opaques = Vec::new();
        impl_traits(signature.output(), &mut opaques);
        if opaques.is_empty() {
            return Ok(Box::default());
        }
        let Ok(generics) = model::Generics::inherited(implementation.generics(), generics) else {
            return Ok(Box::default());
        };
        Ok(self
            .opaque_records(export, method, &generics, &opaques)?
            .into_boxed_slice())
    }

    /// The methods of each trait an opaque `impl Trait` type the method
    /// `method` of the trait `definition` gives implements, as
    /// `opaque_implementations` gives a function's: iced's `Widget::explain`
    /// gives a value whose `Widget::boxed` layout calls. The method is a
    /// function of the trait's `Self`, its parameter 0, as
    /// `classify_self_member` has it, and of the trait's parameters and its
    /// own, so the members are its self member's (`surface::member`).
    fn method_opaque_implementations(
        &self,
        export: &ledger::Export,
        definition: &model::Definition,
        method: &model::Definition,
    ) -> Result<Box<[ImplementationRecord]>, Error> {
        let (
            model::Kind::Trait {
                generics: container,
                ..
            },
            model::Kind::Function {
                signature,
                generics,
            },
        ) = (definition.kind(), method.kind())
        else {
            return Ok(Box::default());
        };
        let trait_id = definition.id();
        let output = self_as_parameter(signature.output(), trait_id);
        let mut opaques = Vec::new();
        impl_traits(&output, &mut opaques);
        if opaques.is_empty() {
            return Ok(Box::default());
        }
        // `Self` is the trait's parameter 0, a parameter of its own here,
        // named as the self member's signature names it, where the call's
        // receiver fills it (`classify_self_member`): the opaque type's
        // parameters take the names of that signature (`surface::facade`),
        // and a member's parameter is its owner's by name.
        let parameters = std::iter::once(model::Parameter::new(
            trait_id.clone(),
            0,
            representability::signature::argument_impl_name(0),
            model::ParameterKind::Type {
                bounds: Box::default(),
                default: model::Default::Absent,
            },
        ))
        .chain(
            container
                .parameters()
                .filter(|parameter| parameter.name() != "Self")
                .cloned(),
        )
        .chain(generics.parameters().cloned())
        .collect::<Box<[_]>>();
        let predicates = container
            .predicates()
            .chain(generics.predicates())
            .map(|predicate| self_predicate_as_parameter(predicate, trait_id))
            .collect::<Box<[_]>>();
        let generics = model::Generics::new(parameters, predicates);
        Ok(self
            .opaque_records(export, method, &generics, &opaques)?
            .into_boxed_slice())
    }

    /// The members of each opaque type in `opaques`, which the function
    /// `definition`, whose generics are `generics`, gives
    /// (`opaque_implementations`).
    fn opaque_records(
        &self,
        export: &ledger::Export,
        definition: &model::Definition,
        generics: &model::Generics,
        opaques: &[&model::Type],
    ) -> Result<Vec<ImplementationRecord>, Error> {
        let mut records = Vec::new();
        for &opaque in opaques {
            let model::Type::ImplTrait { id, bounds, .. } = opaque else {
                continue;
            };
            // Each trait the bounds name, and their supertraits, whose
            // methods rustc reaches on the value as well, then each trait a
            // blanket impl over them gives it (`blanket_extensions`).
            let bound = self.bound_traits(bounds);
            let extensions = self.blanket_extensions(&bound);
            let traits = bound
                .into_iter()
                .map(|bound| (bound, false))
                .chain(extensions.into_iter().map(|extension| (extension, true)));
            // A method an extension shares a name with is the bound's, so
            // each name is one member.
            let mut names = Vec::<Box<str>>::new();
            for ((trait_id, arguments), extension) in traits {
                let trait_id = &trait_id;
                let arguments = &arguments[..];
                let Some(trait_definition) = self
                    .graph
                    .definition_index(trait_id)
                    .and_then(|index| self.graph.definitions().nth(index))
                else {
                    continue;
                };
                let model::Kind::Trait { associated, .. } = trait_definition.kind() else {
                    continue;
                };
                let mut outcomes = Vec::new();
                for method_id in associated.iter() {
                    let Some(method) = self
                        .graph
                        .definition_index(method_id)
                        .and_then(|index| self.graph.definitions().nth(index))
                    else {
                        continue;
                    };
                    let model::Kind::Function {
                        signature: method_signature,
                        generics: method_generics,
                    } = method.kind()
                    else {
                        continue;
                    };
                    // An associated function is a member where its signature
                    // names the trait's `Self`, as `Error::custom(..) ->
                    // Self` does, which a type parameter's opaque type
                    // reaches through its frame (#593).
                    if matches!(method_signature.receiver(), model::Receiver::None)
                        && !(parameter_opaque_id(id) && names_self(method_signature, trait_id))
                    {
                        continue;
                    }
                    let name = method.display_path().last().unwrap_or("<unnamed>");
                    if extension && names.iter().any(|known| **known == *name) {
                        continue;
                    }
                    names.push(name.into());
                    // The trait's type parameters share its export, so each
                    // one's member is its own by the opaque type's
                    // definition.
                    let selector = match selector(method)? {
                        crate::surface::identity::Selector::Method { receiver, .. }
                            if parameter_opaque_id(id) =>
                        {
                            crate::surface::identity::Selector::Method {
                                item: crate::surface::identity::Definition::self_member(
                                    id,
                                    method.id(),
                                ),
                                receiver,
                            }
                        }
                        crate::surface::identity::Selector::AssociatedFunction { .. }
                            if parameter_opaque_id(id) =>
                        {
                            crate::surface::identity::Selector::AssociatedFunction {
                                item: crate::surface::identity::Definition::self_member(
                                    id,
                                    method.id(),
                                ),
                            }
                        }
                        selector => selector,
                    };
                    self.retain_selector(&selector)?;
                    let identity = crate::surface::identity::Identity::opaque_member(
                        export,
                        crate::surface::identity::Implementation::opaque(id, trait_id),
                        selector,
                    );
                    self.retain_outcome_common(export.key(), name)?;
                    self.retain_span(export.span())?;
                    self.retain_span(method.span())?;
                    let origin = representability::origin::Set::new(
                        export.span().clone(),
                        method.span().clone(),
                    );
                    let evidence = self.classify_opaque_method(
                        opaque,
                        trait_id,
                        arguments,
                        generics,
                        method,
                        method_signature,
                        method_generics,
                    )?;
                    outcomes.push(OutcomeRecord {
                        export: export.key().clone(),
                        identity,
                        name: name.into(),
                        origin: Some(origin),
                        evidence,
                        implementations: Box::default(),
                        variant: None,
                        member: None,
                    });
                }
                // Charged as `implementations` charges a crate's own.
                self.retain_items(1)?;
                self.retain_items(outcomes.len())?;
                self.retain_array::<ImplementationRecord>(1)?;
                self.retain_array::<OutcomeRecord>(outcomes.len())?;
                self.retain_span(definition.span())?;
                records.push(ImplementationRecord {
                    identity: crate::surface::identity::Implementation::opaque(id, trait_id),
                    span: definition.span().clone(),
                    associated: outcomes.into_boxed_slice(),
                });
            }
        }
        Ok(records)
    }

    /// The traits a blanket impl over a parameter gives a value whose type
    /// implements the traits `bound` names, each with its arguments: iced's
    /// `widget::Widget`, implemented for every `T: core::Widget`, gives the
    /// opaque type `Widget::explain` gives its `boxed`. An impl applies when
    /// each of its predicates is a bound of its self type that `bound` holds,
    /// at arguments that fix the impl's parameters the trait names, or a
    /// `Sized` bound.
    fn blanket_extensions(
        &self,
        bound: &[(api::identity::Definition, Vec<model::Argument>)],
    ) -> Vec<(api::identity::Definition, Vec<model::Argument>)> {
        let mut found = Vec::<(api::identity::Definition, Vec<model::Argument>)>::new();
        for implementation in self.graph.implementations() {
            let model::Trait::Trait {
                id,
                arguments: implemented,
            } = implementation.trait_ref()
            else {
                continue;
            };
            let model::Type::Parameter { owner, index: this } = implementation.self_type() else {
                continue;
            };
            if owner != implementation.id()
                || implementation.polarity() != model::Polarity::Positive
                || bound.iter().chain(&found).any(|(known, _)| known == id)
            {
                continue;
            }
            let mut given = BTreeMap::<u32, model::Type>::new();
            let mut through = false;
            let applies = implementation.generics().predicates().all(|predicate| {
                let model::Predicate::Bound { ty, bound: bounded } = predicate else {
                    return false;
                };
                let model::Bound::Trait {
                    trait_id,
                    arguments,
                    polarity: model::Polarity::Positive,
                    ..
                } = bounded
                else {
                    return matches!(bounded, model::Bound::MaybeSized);
                };
                if self.definition_name(trait_id) == Some("Sized") {
                    return true;
                }
                if !matches!(ty, model::Type::Parameter { owner, index }
                    if owner == implementation.id() && index == this)
                {
                    return false;
                }
                let Some((_, held)) = bound.iter().find(|(known, _)| known == trait_id) else {
                    return false;
                };
                through = true;
                arguments.len() == held.len()
                    && arguments.iter().zip(held).all(|pair| match pair {
                        (
                            model::Argument::Type(model::Type::Parameter { owner, index }),
                            model::Argument::Type(held),
                        ) if owner == implementation.id() => match given.get(index) {
                            Some(known) => known == held,
                            None => {
                                given.insert(*index, held.clone());
                                true
                            }
                        },
                        (argument, held) => argument == held,
                    })
            });
            if !applies || !through {
                continue;
            }
            // The trait's arguments leave out `Self`, as a bound's do.
            let Some(arguments) = relabel_arguments(
                implemented.get(1..).unwrap_or_default(),
                implementation.id(),
                &|index| given.get(&index).cloned(),
            ) else {
                continue;
            };
            found.push((id.clone(), arguments.into_vec()));
        }
        found
    }

    /// The methods the trait `definition` declares that take `self` by
    /// value, each a function generic over the trait's `Self`
    /// (`surface::member`): a program's own type that implements the trait
    /// calls them, as `column!`'s `Widget::_boxed(circle)` calls the
    /// provided `_boxed` of a script's `impl Widget<..> for Circle`. No
    /// impl of the crate's has the script type as its `Self`.
    fn self_member_implementations(
        &self,
        export: &ledger::Export,
        definition: &model::Definition,
    ) -> Result<Vec<ImplementationRecord>, Error> {
        let model::Kind::Trait {
            associated,
            generics,
            ..
        } = definition.kind()
        else {
            return Ok(Vec::new());
        };
        let mut outcomes = Vec::new();
        for method_id in associated.iter() {
            let Some(method) = self
                .graph
                .definition_index(method_id)
                .and_then(|index| self.graph.definitions().nth(index))
            else {
                continue;
            };
            let model::Kind::Function {
                signature: method_signature,
                generics: method_generics,
            } = method.kind()
            else {
                continue;
            };
            if !matches!(method_signature.receiver(), model::Receiver::Value { .. }) {
                continue;
            }
            let selector = crate::surface::identity::Selector::AssociatedFunction {
                item: crate::surface::identity::Definition::self_member(
                    definition.id(),
                    method.id(),
                ),
            };
            self.retain_selector(&selector)?;
            let identity = crate::surface::identity::Identity::imported(export, None, selector)?;
            let name = method.display_path().last().unwrap_or("<unnamed>");
            self.retain_outcome_common(export.key(), name)?;
            self.retain_span(export.span())?;
            self.retain_span(method.span())?;
            let origin =
                representability::origin::Set::new(export.span().clone(), method.span().clone());
            let evidence = self.classify_self_member(
                definition,
                generics,
                method,
                method_signature,
                method_generics,
            )?;
            let implementations = self.method_opaque_implementations(export, definition, method)?;
            outcomes.push(OutcomeRecord {
                export: export.key().clone(),
                identity,
                name: name.into(),
                origin: Some(origin),
                evidence,
                implementations,
                variant: None,
                member: None,
            });
        }
        outcomes.extend(self.parameter_members(export, definition)?);
        if outcomes.is_empty() {
            return Ok(Vec::new());
        }
        // Charged as `implementations` charges a crate's own.
        self.retain_items(1)?;
        self.retain_items(outcomes.len())?;
        self.retain_array::<ImplementationRecord>(1)?;
        self.retain_array::<OutcomeRecord>(outcomes.len())?;
        self.retain_span(definition.span())?;
        Ok(vec![ImplementationRecord {
            identity: crate::surface::identity::Implementation::self_members(
                &crate::surface::identity::Definition::of(definition.id()),
            ),
            span: definition.span().clone(),
            associated: outcomes.into_boxed_slice(),
        }])
    }

    /// The opaque type each type parameter of the methods of the trait
    /// `definition` is where native code calls a program's impl of the
    /// method, as `S` of serde's `serialize<S: Serializer>`, and each
    /// associated type of its bounds they leave open, as `S::Ok` (#593):
    /// each a function of no inputs that gives the type, beside the trait's
    /// methods over its `Self` (`surface::opaque::parameter_name`), whose
    /// members are the methods its bounds give it, as an opaque output's
    /// are. No program calls the function; the program's impl names the
    /// type.
    fn parameter_members(
        &self,
        export: &ledger::Export,
        definition: &model::Definition,
    ) -> Result<Vec<OutcomeRecord>, Error> {
        let model::Kind::Trait { associated, .. } = definition.kind() else {
            return Ok(Vec::new());
        };
        let mut outcomes = Vec::new();
        for method_id in associated.iter() {
            let Some(method) = self
                .graph
                .definition_index(method_id)
                .and_then(|index| self.graph.definitions().nth(index))
            else {
                continue;
            };
            let model::Kind::Function { generics, .. } = method.kind() else {
                continue;
            };
            let Some(name) = method.display_path().last() else {
                continue;
            };
            let parameters = generics
                .parameters()
                .filter(|parameter| matches!(parameter.kind(), model::ParameterKind::Type { .. }));
            for (position, parameter) in parameters.enumerate() {
                let opaque = parameter_opaque(method, parameter, generics);
                let model::Type::ImplTrait { bounds, .. } = &opaque else {
                    continue;
                };
                if self.bound_traits(bounds).is_empty() {
                    continue;
                }
                let mut types = vec![(None, opaque.clone())];
                for (trait_id, _) in self.bound_traits(bounds) {
                    let Some(model::Kind::Trait { associated, .. }) = self
                        .graph
                        .definition_index(&trait_id)
                        .and_then(|index| self.graph.definitions().nth(index))
                        .map(model::Definition::kind)
                    else {
                        continue;
                    };
                    for item in associated.iter() {
                        let Some(found) = self
                            .graph
                            .definition_index(item)
                            .and_then(|index| self.graph.definitions().nth(index))
                        else {
                            continue;
                        };
                        if !matches!(found.kind(), model::Kind::AssociatedType { .. }) {
                            continue;
                        }
                        let Some(associated_name) = found.display_path().last() else {
                            continue;
                        };
                        if types
                            .iter()
                            .any(|(known, _)| known.as_deref() == Some(associated_name))
                        {
                            continue;
                        }
                        let projected = self.opaque_projection(&opaque, item);
                        if matches!(projected, model::Type::ImplTrait { .. }) {
                            types.push((Some(associated_name.to_owned()), projected));
                        }
                    }
                }
                for (projection, ty) in types {
                    outcomes.extend(self.parameter_member(
                        export,
                        definition,
                        method,
                        &crate::surface::opaque::parameter_name(
                            name,
                            position,
                            projection.as_deref(),
                        ),
                        &ty,
                    )?);
                }
            }
        }
        Ok(outcomes)
    }

    /// One function of [`Self::parameter_members`]: `name`, which gives the
    /// opaque type `opaque` that a type parameter of the trait
    /// `definition`'s method `method` is, or one of its projections. `None`
    /// when Rawr reads the type as a value of its own kind rather than as
    /// that opaque type, as iced's `Executor::spawn`'s `impl Future<Output =
    /// ()>` is the core future: the function would name the core future's
    /// identity as the parameter's, and every call that gives or takes a
    /// future would run in a frame of the parameter.
    fn parameter_member(
        &self,
        export: &ledger::Export,
        definition: &model::Definition,
        method: &model::Definition,
        name: &str,
        opaque: &model::Type,
    ) -> Result<Option<OutcomeRecord>, Error> {
        let model::Type::ImplTrait { id, .. } = opaque else {
            return Err(Error::Cardinality);
        };
        let signature = model::Signature::new(
            model::Safety::Safe,
            false,
            false,
            "\"Rust\"".into(),
            false,
            Box::default(),
            model::Receiver::None,
            Box::default(),
            opaque.clone(),
        );
        let generics = model::Generics::new(Box::default(), Box::default());
        let converted = match self.function(&signature, &generics) {
            Ok(plan) => {
                let own = crate::surface::identity::Definition::of(id);
                let gives_own = matches!(&plan, representability::Representability::Function(function)
                    if matches!(function.signature().output(),
                        representability::convert::Type::Nominal { definition, .. }
                            if *definition == own));
                if !gives_own {
                    return Ok(None);
                }
                Ok(plan)
            }
            Err(Failure::Reject(kind)) => Err(kind),
            Err(Failure::Operational(error)) => return Err(error),
        };
        let selector = crate::surface::identity::Selector::AssociatedFunction {
            item: crate::surface::identity::Definition::self_member(definition.id(), id),
        };
        self.retain_selector(&selector)?;
        let identity = crate::surface::identity::Identity::imported(export, None, selector)?;
        self.retain_outcome_common(export.key(), name)?;
        self.retain_span(export.span())?;
        self.retain_span(method.span())?;
        let origin =
            representability::origin::Set::new(export.span().clone(), method.span().clone());
        self.retain_inline_representability()?;
        // A type no function gives, as one that names the trait's own
        // parameter (#1032), has no members a facade can declare.
        let implementations = match converted {
            Ok(_) => self
                .opaque_records(export, method, &generics, &[opaque])?
                .into_boxed_slice(),
            Err(_) => Box::default(),
        };
        let evidence = OutcomeEvidence::Inline(Box::new(converted.unwrap_or_else(|kind| {
            representability::Representability::Rejected(
                representability::reject::Reason::classification(kind, method.span().clone()),
            )
        })));
        Ok(Some(OutcomeRecord {
            export: export.key().clone(),
            identity,
            name: name.into(),
            origin: Some(origin),
            evidence,
            implementations,
            variant: None,
            member: None,
        }))
    }

    /// The type `<opaque as Trait>::associated` is, for an opaque type
    /// `opaque`: the type its bounds bind it to, as `impl Program<State =
    /// S>` binds `State`, or else an opaque type of its own, bounded as the
    /// trait declares the associated type, with the trait's `Self` the
    /// opaque type, as serde's `S::SerializeStruct` is a `SerializeStruct<
    /// Ok = S::Ok, ..>` (#593).
    fn opaque_projection(
        &self,
        opaque: &model::Type,
        associated: &api::identity::Definition,
    ) -> model::Type {
        let model::Type::ImplTrait { id, bounds, .. } = opaque else {
            return opaque.clone();
        };
        if let Some((_, _, ty)) = self
            .bound_associated(bounds)
            .into_iter()
            .find(|(_, item, _)| item == associated)
        {
            return ty;
        }
        let declared = self
            .graph
            .definition_index(associated)
            .and_then(|index| self.graph.definitions().nth(index));
        let (owner, declared) = match declared.map(|found| (found, found.kind())) {
            Some((found, model::Kind::AssociatedType { bounds, .. })) => (
                match found.owner() {
                    model::Owner::Definition(owner) => Some(owner.clone()),
                    _ => None,
                },
                bounds.clone(),
            ),
            _ => (None, Box::default()),
        };
        let id = api::identity::Definition::new(
            id.unit().clone(),
            format!("{}::{}", id.def_path_hash(), associated.def_path_hash()).into(),
            api::identity::Namespace::Type,
        );
        // The declaration's `Self` is the opaque type, and so is the
        // projection a bound of its own names it by. The projection itself
        // is the new type's `Self`, as `PartialEq<Self>` of `type Font:
        // PartialEq` is the font's own (rustc's item bounds of the type).
        let this = owner.as_ref().map(|owner| model::Type::Projection {
            trait_id: owner.clone(),
            associated: associated.clone(),
            arguments: Box::new([model::Argument::Type(opaque.clone())]),
        });
        let own = model::Type::SelfType(id.clone());
        let bounds = declared
            .iter()
            .map(|predicate| {
                map_predicate_types(predicate, |ty| match (&owner, &this) {
                    (Some(owner), Some(this)) => type_replaced(
                        &substitute_parameters(
                            &self_replaced(ty, owner, opaque),
                            &[((owner.clone(), 0), opaque.clone())],
                        ),
                        this,
                        &own,
                    ),
                    _ => ty.clone(),
                })
            })
            .collect();
        model::Type::ImplTrait {
            id,
            bounds,
            captures: Box::default(),
        }
    }

    /// `projected`, the type `opaque_projection` gives, converted as a
    /// function's output is. An opaque type of its own that its own bounds
    /// reach again while it is converted is refused (`opaque_converting`).
    fn convert_projected(
        &self,
        projected: &model::Type,
    ) -> Result<representability::convert::Type, Failure> {
        let outer = self.opaque_output.replace(Some(Box::default()));
        let converted = self.convert(projected);
        *self.opaque_output.borrow_mut() = outer;
        converted
    }

    /// `method` of the trait `definition`, whose generics are `container`,
    /// as a function of its `Self` and the trait's parameters: `self` its
    /// first input, `Self` a parameter the call's receiver fills and no
    /// turbofish names, and the trait's parameters the trait's path takes,
    /// as `Widget::<Message, Theme, Renderer>::_boxed(circle)`.
    fn classify_self_member(
        &self,
        definition: &model::Definition,
        container: &model::Generics,
        method: &model::Definition,
        signature: &model::Signature,
        generics: &model::Generics,
    ) -> Result<OutcomeEvidence, Error> {
        use representability::{callable, reject, Representability};
        let retained_before = (self.retained_items.get(), self.retained_bytes.get());
        let trait_id = definition.id();
        let result = (|| -> Result<Representability, Failure> {
            let model::Receiver::Value { ty: receiver } = signature.receiver() else {
                return Err(reject::Kind::GenericImplementation.into());
            };
            let this = |ty: &model::Type| self_as_parameter(ty, trait_id);
            // `Self` is the trait's parameter 0, a parameter of its own here.
            let mut parameters = vec![model::Parameter::new(
                trait_id.clone(),
                0,
                "This".into(),
                model::ParameterKind::Type {
                    bounds: Box::default(),
                    default: model::Default::Absent,
                },
            )];
            let mut container_parameters = 0_usize;
            for parameter in container.parameters() {
                if parameter.name() == "Self" || is_lifetime_parameter(parameter) {
                    continue;
                }
                if !matches!(parameter.kind(), model::ParameterKind::Type { .. }) {
                    return Err(reject::Kind::GenericImplementation.into());
                }
                container_parameters += 1;
                parameters.push(parameter.clone());
            }
            parameters.extend(generics.parameters().cloned());
            let trait_arguments = container
                .parameters()
                .filter(|parameter| parameter.name() != "Self")
                .map(|parameter| match parameter.kind() {
                    model::ParameterKind::Lifetime { .. } => {
                        model::Argument::Lifetime(model::Lifetime::Static)
                    }
                    _ => model::Argument::Type(model::Type::Parameter {
                        owner: trait_id.clone(),
                        index: parameter.index(),
                    }),
                })
                .collect::<Box<[_]>>();
            let own_bound = model::Predicate::Bound {
                ty: model::Type::Parameter {
                    owner: trait_id.clone(),
                    index: 0,
                },
                bound: model::Bound::Trait {
                    trait_id: trait_id.clone(),
                    arguments: trait_arguments,
                    polarity: model::Polarity::Positive,
                    binders: Box::default(),
                },
            };
            // The trait's own predicates and the method's, `Self` the
            // parameter: `Self: Sized` holds of any value a call passes.
            let mut predicates = Vec::new();
            for predicate in container.predicates().chain(generics.predicates()) {
                if let model::Predicate::Bound {
                    ty: model::Type::SelfType(owner),
                    bound:
                        model::Bound::Trait {
                            trait_id: bound, ..
                        },
                } = predicate
                    && owner == trait_id
                    && self.definition_name(bound) == Some("Sized")
                {
                    continue;
                }
                let predicate = self_predicate_as_parameter(predicate, trait_id);
                if !predicates.contains(&predicate) {
                    predicates.push(predicate);
                }
            }
            let bounded = predicates.iter().any(|predicate| {
                matches!(
                    predicate,
                    model::Predicate::Bound {
                        ty: model::Type::Parameter { owner, index: 0 },
                        bound: model::Bound::Trait { trait_id: bound, .. },
                    } if owner == trait_id && bound == trait_id
                )
            });
            if !bounded {
                predicates.insert(0, own_bound);
            }
            // A trait every iterator has through a blanket impl, as
            // itertools' `impl<T: Iterator + ?Sized> Itertools for T`: a
            // program's iterator is its `Self`, as a type of the call's.
            if let Some(iterator) = self.iterator_blanket(trait_id)
                && !predicates.iter().any(|predicate| {
                    matches!(
                        predicate,
                        model::Predicate::Bound {
                            ty: model::Type::Parameter { owner, index: 0 },
                            bound: model::Bound::Trait { trait_id: bound, .. },
                        } if owner == trait_id && *bound == iterator
                    )
                })
            {
                predicates.push(model::Predicate::Bound {
                    ty: model::Type::Parameter {
                        owner: trait_id.clone(),
                        index: 0,
                    },
                    bound: model::Bound::Trait {
                        trait_id: iterator,
                        arguments: Box::default(),
                        polarity: model::Polarity::Positive,
                        binders: Box::default(),
                    },
                });
            }
            let mut inputs = vec![this(receiver)];
            inputs.extend(signature.inputs().map(this));
            // What the trait's implementation for every callable returns,
            // as iced's `Function::with` returns `impl Fn(B) -> O`.
            let output = this(signature.output());
            let output = self.through_callable_blanket(&output).unwrap_or(output);
            let rewritten = model::Signature::new(
                signature.safety(),
                signature.const_function(),
                signature.async_function(),
                signature.abi().into(),
                signature.variadic(),
                signature.binders().cloned().collect(),
                model::Receiver::None,
                inputs.into_boxed_slice(),
                output,
            )
            .with_guard(signature.guard().cloned());
            let generics =
                model::Generics::new(parameters.into_boxed_slice(), predicates.into_boxed_slice());
            let Representability::FunctionItem(item) = self.function(&rewritten, &generics)? else {
                return Err(reject::Kind::GenericImplementation.into());
            };
            let item = item.with_inferred(&[0], container_parameters + 1);
            // A closure the output names is boxed, as iced's `Widget::map`
            // gives `Map<Self, F, Message>` (T417): the call makes it as any
            // generic method's call does.
            let boxed = item.call().is_some()
                && item.crossings().is_empty()
                && output_boxes(item.signature().output());
            // A callable that is the receiver, as a function is of iced's
            // `Function::with` through the trait's impl for every `Fn`,
            // fills `Self` as any callable parameter's slot.
            let callable_self = !item.crossings().is_empty()
                && item.crossings().iter().all(|call| call.parameter() == 0);
            if !boxed && !callable_self && (item.call().is_some() || !item.crossings().is_empty()) {
                return Err(reject::Kind::CallableProtocol.into());
            }
            Ok(Representability::Invocation(callable::Invocation::new(
                item,
                container_parameters,
            )))
        })();
        match result {
            Ok(plan) => {
                self.retain_inline_representability()?;
                Ok(OutcomeEvidence::Inline(Box::new(plan)))
            }
            Err(Failure::Reject(kind)) => {
                self.retained_items.set(retained_before.0);
                self.retained_bytes.set(retained_before.1);
                self.retain_span(method.span())?;
                self.retain_inline_representability()?;
                Ok(OutcomeEvidence::Inline(Box::new(
                    Representability::Rejected(reject::Reason::classification(
                        kind,
                        method.span().clone(),
                    )),
                )))
            }
            Err(Failure::Operational(error)) => Err(error),
        }
    }

    /// `container` without each type parameter the opaque type `opaque`
    /// leaves out of its arguments (`opaque_excluded`), and the bounds on
    /// it, or `None` when it leaves none out.
    fn without_left_out(
        &self,
        opaque: &model::Type,
        container: &model::Generics,
    ) -> Option<model::Generics> {
        let model::Type::ImplTrait { id, captures, .. } = opaque else {
            return None;
        };
        let positions = self.opaque_excluded.borrow().get(id).cloned()?;
        let left_out = captures
            .iter()
            .enumerate()
            .filter(|(position, _)| {
                u32::try_from(*position).is_ok_and(|position| positions.contains(&position))
            })
            .filter_map(|(_, capture)| match capture {
                model::Argument::Type(model::Type::Parameter { index, .. }) => Some(*index),
                _ => None,
            })
            .collect::<Vec<_>>();
        if left_out.is_empty() {
            return None;
        }
        let bounded = |ty: &model::Type| matches!(ty, model::Type::Parameter { index, .. } if left_out.contains(index));
        let parameters = container
            .parameters()
            .filter(|parameter| !left_out.contains(&parameter.index()))
            .cloned()
            .collect();
        let predicates = container
            .predicates()
            .filter(|predicate| match predicate {
                model::Predicate::Bound { ty, .. } | model::Predicate::TypeOutlives { ty, .. } => {
                    !bounded(ty)
                }
                model::Predicate::ProjectionEquality {
                    projection: model::Type::Projection { arguments, .. },
                    ..
                } => !matches!(arguments.first(), Some(model::Argument::Type(ty)) if bounded(ty)),
                _ => true,
            })
            .cloned()
            .collect();
        Some(model::Generics::new(parameters, predicates))
    }

    /// `method` of the trait `trait_id` as a member of the opaque type
    /// `opaque`, whose bound gives the trait `arguments`, within the
    /// function whose generics are `container`.
    #[allow(clippy::too_many_arguments)]
    fn classify_opaque_method(
        &self,
        opaque: &model::Type,
        trait_id: &api::identity::Definition,
        arguments: &[model::Argument],
        container: &model::Generics,
        method: &model::Definition,
        signature: &model::Signature,
        generics: &model::Generics,
    ) -> Result<OutcomeEvidence, Error> {
        use representability::{reject, Representability};
        let retained_before = (self.retained_items.get(), self.retained_bytes.get());
        // A bound's arguments leave out `Self`, the trait's parameter 0.
        let mut substitutions = arguments
            .iter()
            .enumerate()
            .filter_map(|(index, argument)| match argument {
                model::Argument::Type(ty) => Some((
                    (trait_id.clone(), u32::try_from(index).ok()?.checked_add(1)?),
                    ty.clone(),
                )),
                _ => None,
            })
            .collect::<Vec<_>>();
        let first = container
            .parameters()
            .map(|parameter| parameter.index().saturating_add(1))
            .max()
            .unwrap_or(0);
        // A parameter the opaque type leaves out (`opaque_excluded`), as
        // the callable `code_block`'s `impl Fn(Uri) -> Message` fills, is
        // none of the member's: no argument of the type gives it.
        let pruned = self.without_left_out(opaque, container);
        let container = pruned.as_ref().unwrap_or(container);
        // The method's parameters join the function's, and a name both
        // use, as warp's `form<T>()` and `Filter::untuple_one<T>` do, would
        // be declared twice: the method's is renamed `T1`, `T2`, ...
        let mut names = container
            .parameters()
            .map(|parameter| Box::<str>::from(parameter.name()))
            .collect::<Vec<_>>();
        let mut own = Vec::new();
        for (position, parameter) in generics.parameters().enumerate() {
            let index = first
                .checked_add(u32::try_from(position).map_err(|_| Error::IndexOverflow)?)
                .ok_or(Error::IndexOverflow)?;
            if matches!(parameter.kind(), model::ParameterKind::Type { .. }) {
                substitutions.push((
                    (parameter.owner().clone(), parameter.index()),
                    model::Type::Parameter {
                        owner: parameter.owner().clone(),
                        index,
                    },
                ));
            }
            let mut name = Box::<str>::from(parameter.name());
            let mut suffix = 1_u32;
            while names.contains(&name) {
                name = format!("{}{suffix}", parameter.name()).into();
                suffix = suffix.saturating_add(1);
            }
            names.push(name.clone());
            own.push(model::Parameter::new(
                parameter.owner().clone(),
                index,
                name,
                parameter.kind().clone(),
            ));
        }
        // Its bounds name its parameters at their new indices, as
        // `tag<T: Display>`'s does.
        let own = model::Generics::new(
            own.into_boxed_slice(),
            generics
                .predicates()
                .map(|predicate| substituted_predicate(predicate, &substitutions))
                .collect(),
        );
        let signature = substituted_signature(signature, &substitutions);
        let inherited =
            model::Generics::inherited(container, &own).map_err(|_| Error::Allocation)?;
        let outer = self.opaque_self.replace(Some(opaque.clone()));
        let classified = self.function(&signature, &inherited).and_then(|plan| {
            let instance = self.convert(&model::Type::SelfType(trait_id.clone()))?;
            match plan {
                Representability::FunctionItem(item) => {
                    Ok(Representability::FunctionItem(item.with_instance(instance)))
                }
                // An associated function of the trait, as `Error::custom`.
                Representability::Function(plan) => Ok(Representability::FunctionItem(
                    representability::callable::Item::new(
                        plan.signature().clone(),
                        None,
                        None,
                        Box::default(),
                        Box::default(),
                    )
                    .with_instance(instance),
                )),
                // A method of a trait without parameters, on an opaque type
                // of a function without type parameters, as `fn view<'a>(..)
                // -> impl Show + 'a` gives: an item of no parameters, whose
                // `Self` the opaque type's witness names.
                Representability::Method(plan) => Ok(Representability::FunctionItem(
                    representability::callable::Item::new(
                        plan.signature().clone(),
                        Some(plan.receiver()),
                        None,
                        Box::default(),
                        Box::default(),
                    )
                    .with_instance(instance),
                )),
                _ => Err(Failure::from(reject::Kind::GenericImplementation)),
            }
        });
        *self.opaque_self.borrow_mut() = outer;
        match classified {
            Ok(plan) => {
                self.retain_inline_representability()?;
                Ok(OutcomeEvidence::Inline(Box::new(plan)))
            }
            Err(Failure::Reject(kind)) => {
                self.retained_items.set(retained_before.0);
                self.retained_bytes.set(retained_before.1);
                self.retain_span(method.span())?;
                self.retain_inline_representability()?;
                Ok(OutcomeEvidence::Inline(Box::new(
                    Representability::Rejected(reject::Reason::classification(
                        kind,
                        method.span().clone(),
                    )),
                )))
            }
            Err(Failure::Operational(error)) => Err(error),
        }
    }

    fn classify_associated(
        &self,
        implementation: &model::Implementation,
        definition: &model::Definition,
        definitions: &[DefinitionPlan],
        target: &api::identity::Definition,
    ) -> Result<OutcomeEvidence, Error> {
        use representability::{reject, Representability};
        let rejected = match (
            implementation.safety(),
            implementation.polarity(),
            implementation.const_impl(),
            implementation.default_impl(),
        ) {
            (model::Safety::Unsafe, _, _, _) => Some(reject::Kind::UnsafeImplementation),
            (_, model::Polarity::Negative, _, _) => Some(reject::Kind::NegativeImplementation),
            (_, _, true, _) => Some(reject::Kind::ConstImplementation),
            (_, _, _, true) => Some(reject::Kind::DefaultImplementation),
            // An implementation that declares only lifetimes is this type's
            // own (D558 tier 0) only when its self type is the type with
            // nothing but lifetimes filled in. One for another type that
            // names this one, as cosmic-text's `impl<'a>
            // BorrowedWithFontSystem<'a, Buffer>` names `Buffer`, has no
            // members of this type: its `hit(&mut self)` is not
            // `Buffer::hit(&self)`. One that fixes a type argument, as
            // `impl BorrowedWithFontSystem<'_, Buffer>` and `impl
            // BorrowedWithFontSystem<'_, Editor>` each do, is no member of
            // the generic type: both declare `set_tab_width(&mut self, u16)`.
            // A fixed argument that is its parameter's declared default is
            // the type as written without it, as rustc resolves
            // `Parser::new` in `impl<'input> Parser<'input,
            // DefaultBrokenLinkCallback>` for `Parser`. So is a member of
            // one instance that no other implementation of the type
            // declares, as quick-xml's `Reader::from_str` in `impl<'a>
            // Reader<&'a [u8]>`: no overload can repeat it (D558 slice 2).
            // A trait's impl for `&str`, whose only parameter is the
            // string's lifetime, as `impl Label for &str`, is the trait's
            // member on Rawr's string, below.
            _ if implementation.generics().parameters().next().is_some()
                && lifetime_only(implementation.generics())
                && !is_str_reference(implementation.self_type())
                && !self.own_form(implementation.self_type(), target)
                && !self.sole_instance_member(implementation, definition, target) =>
            {
                Some(reject::Kind::GenericImplementation)
            }
            // An impl for another type that names this one has no members
            // of this type. Crossterm's `impl<D> AsRef<ContentStyle> for
            // StyledContent<D>` names `ContentStyle`, and its `as_ref` is
            // `StyledContent`'s. Palette's inherent `impl<T> Hsl<Srgb, T>`
            // names `Srgb`, and its `new_srgb` is `Hsl`'s.
            _ if matches!(implementation.self_type(), model::Type::Definition { id, .. } if id != target) => {
                Some(reject::Kind::GenericImplementation)
            }
            // `as_ref` of one of several `AsRef<T>` impls, as a type that is
            // `AsRef<str>` and `AsRef<[u8]>`: rustc picks one by the type
            // the caller asks for, which a Rawr call doesn't name, and
            // they'd repeat one input signature (D558 slice 2).
            _ if self.ambiguous_as_ref(implementation) => Some(reject::Kind::GenericImplementation),
            _ => None,
        };
        // A trait's default method, as Freya's `ContainerSizeExt::expanded(self)
        // -> Self`, is one definition every implementation shares. As a
        // member of this one, its `Self` is this implementation's type.
        let default_method = matches!(definition.owner(), model::Owner::Definition(_))
            && matches!(definition.kind(), model::Kind::Function { .. });
        match rejected {
            Some(kind) => {
                self.retain_span(implementation.span())?;
                self.retain_inline_representability()?;
                Ok(OutcomeEvidence::Inline(Box::new(
                    Representability::Rejected(reject::Reason::classification(
                        kind,
                        implementation.span().clone(),
                    )),
                )))
            }
            // A trait's impl for `&str`, as iced's `impl Widget<..> for
            // &str`: its methods take Rawr's string as the impl's `&str`.
            None if is_str_reference(implementation.self_type())
                && matches!(implementation.trait_ref(), model::Trait::Trait { id, .. } if id == target) =>
            {
                let index = self
                    .graph
                    .implementation_index(implementation.id())
                    .ok_or(Error::Cardinality)?;
                let outer = self.trait_self.replace(Some(index));
                let evidence = self.classify_str_member(implementation, definition);
                self.trait_self.set(outer);
                evidence
            }
            // A crate's impl of an operator for a number that takes this
            // type, as iced's `impl Mul<Radians> for f32`: the member is
            // this type's function of the number and the operand.
            None if is_number(implementation.self_type())
                && matches!(implementation.trait_ref(), model::Trait::Trait { .. }) =>
            {
                self.classify_operand_member(implementation, definition)
            }
            None if default_method => {
                let index = self
                    .graph
                    .implementation_index(implementation.id())
                    .ok_or(Error::Cardinality)?;
                let outer = self.trait_self.replace(Some(index));
                let evidence = if lifetime_only(implementation.generics()) {
                    self.classify_default_method(implementation, definition)
                } else {
                    self.classify_inherent_invocation(implementation, definition)
                };
                self.trait_self.set(outer);
                evidence
            }
            None if lifetime_only(implementation.generics()) => {
                plan_evidence(definitions, definition)
            }
            None => self.classify_inherent_invocation(implementation, definition),
        }
    }

    /// The method of a trait's impl for a number that takes another type,
    /// as `mul` of `impl Mul<Radians> for f32`: a function of the number
    /// and the operand whose instance is the number, so the bridge calls
    /// `<f32>::mul(number, operand)` and rustc selects the impl.
    fn classify_operand_member(
        &self,
        implementation: &model::Implementation,
        definition: &model::Definition,
    ) -> Result<OutcomeEvidence, Error> {
        use representability::{function, reject, Representability};
        let retained_before = (self.retained_items.get(), self.retained_bytes.get());
        let result = (|| -> Result<Representability, Failure> {
            let model::Kind::Function {
                signature,
                generics,
            } = definition.kind()
            else {
                return Err(reject::Kind::GenericImplementation.into());
            };
            // A number's `self` is its type written out: the function's
            // first input.
            let by_value = matches!(signature.receiver(),
                model::Receiver::Explicit { ty } if ty == implementation.self_type());
            if !by_value
                || implementation.generics().parameters().next().is_some()
                || generics.parameters().next().is_some()
            {
                return Err(reject::Kind::GenericImplementation.into());
            }
            let instance = self
                .convert(implementation.self_type())
                .map_err(|_| reject::Kind::GenericImplementation)?;
            let free = model::Signature::new(
                signature.safety(),
                signature.const_function(),
                signature.async_function(),
                signature.abi().into(),
                signature.variadic(),
                signature.binders().cloned().collect(),
                model::Receiver::None,
                std::iter::once(implementation.self_type().clone())
                    .chain(signature.inputs().cloned())
                    .collect(),
                signature.output().clone(),
            );
            match self.function(&free, generics)? {
                Representability::Function(plan) => Ok(Representability::Function(
                    function::Plan::new(plan.signature().clone().with_instance(Some(instance))),
                )),
                _ => Err(reject::Kind::GenericImplementation.into()),
            }
        })();
        match result {
            Ok(plan) => {
                self.retain_inline_representability()?;
                Ok(OutcomeEvidence::Inline(Box::new(plan)))
            }
            Err(Failure::Reject(kind)) => {
                self.retained_items.set(retained_before.0);
                self.retained_bytes.set(retained_before.1);
                self.retain_span(implementation.span())?;
                self.retain_inline_representability()?;
                Ok(OutcomeEvidence::Inline(Box::new(
                    Representability::Rejected(reject::Reason::classification(
                        kind,
                        implementation.span().clone(),
                    )),
                )))
            }
            Err(Failure::Operational(error)) => Err(error),
        }
    }

    /// Whether `definition` is a provided method of the trait
    /// `implementation` implements that another impl of the trait for the
    /// same type also has, as `PartialOrd::lt` is of both of `Degrees`'s
    /// `PartialOrd` and `PartialOrd<f32>`, or as a blanket impl's method is
    /// of each of its applications to one type through that type's impls
    /// of its bound. Each impl's parameters are its own, so the self types
    /// match by the type they name.
    fn shared_provided(
        &self,
        implementation: &model::Implementation,
        definition: &model::Definition,
    ) -> bool {
        let (model::Owner::Definition(owner), model::Trait::Trait { id, .. }) =
            (definition.owner(), implementation.trait_ref())
        else {
            return false;
        };
        let same_type = |left: &model::Type, right: &model::Type| match (left, right) {
            (
                model::Type::Definition { id: left, .. },
                model::Type::Definition { id: right, .. },
            ) => left == right,
            _ => left == right,
        };
        owner == id
            && self.graph.implementations_of(id).any(|other| {
                other.id() != implementation.id()
                    && matches!(other.trait_ref(), model::Trait::Trait { id: other_id, .. } if other_id == id)
                    && same_type(other.self_type(), implementation.self_type())
                    && other.associated().any(|associated| associated == definition.id())
            })
    }

    /// A trait's default method as a member of the non-generic
    /// `implementation`, with the trait's `Self` as its type.
    fn classify_default_method(
        &self,
        implementation: &model::Implementation,
        definition: &model::Definition,
    ) -> Result<OutcomeEvidence, Error> {
        use representability::{reject, Representability};
        let retained_before = (self.retained_items.get(), self.retained_bytes.get());
        let model::Kind::Function {
            signature,
            generics,
        } = definition.kind()
        else {
            return Err(Error::Cardinality);
        };
        // A concrete implementation still fixes the trait's parameters,
        // even when the implementation itself declares none.
        let provided = provided_signature(implementation, definition, signature, generics)?;
        let (signature, generics) = provided
            .as_ref()
            .map_or((signature, generics), |(signature, generics)| {
                (signature, generics)
            });
        match self.function(signature, generics) {
            Ok(plan) => {
                self.retain_inline_representability()?;
                Ok(OutcomeEvidence::Inline(Box::new(plan)))
            }
            Err(Failure::Reject(kind)) => {
                self.retained_items.set(retained_before.0);
                self.retained_bytes.set(retained_before.1);
                self.retain_span(implementation.span())?;
                self.retain_inline_representability()?;
                Ok(OutcomeEvidence::Inline(Box::new(
                    Representability::Rejected(reject::Reason::classification(
                        kind,
                        implementation.span().clone(),
                    )),
                )))
            }
            Err(Failure::Operational(error)) => Err(error),
        }
    }

    /// The trait a method of `implementation` is called through when the
    /// impl has parameters its type doesn't name, the `inferred` ones: the
    /// trait at the impl's arguments other than `Self`, which name them,
    /// as rustc reads `Renderer` from the `impl Widget<Part>` a value is
    /// (T428). `None` for an inherent impl, or one whose arguments Rawr
    /// can't write.
    fn through(
        &self,
        implementation: &model::Implementation,
        inferred: &[u32],
    ) -> Option<representability::convert::Type> {
        let model::Trait::Trait { id, arguments } = implementation.trait_ref() else {
            return None;
        };
        if inferred.is_empty() {
            return None;
        }
        let arguments = arguments
            .iter()
            .skip(1)
            .filter_map(|argument| match argument {
                model::Argument::Type(ty) => Some(self.convert(ty).ok()),
                _ => None,
            })
            .collect::<Option<Box<[_]>>>()?;
        Some(representability::convert::Type::Nominal {
            definition: crate::surface::identity::Definition::of(id),
            arguments,
        })
    }

    /// A method of a trait's impl for `&str`, as iced's `impl<Message,
    /// Theme, Renderer> Widget<Message, Theme, Renderer> for &str` gives
    /// `_boxed`: an invocation whose instance is the `&str`, or the
    /// `&'static str` of an impl for that alone, with every parameter of
    /// the impl the call's to infer, as rustc infers them.
    fn classify_str_member(
        &self,
        implementation: &model::Implementation,
        definition: &model::Definition,
    ) -> Result<OutcomeEvidence, Error> {
        use representability::{callable, reject, Representability};

        let retained_before = (self.retained_items.get(), self.retained_bytes.get());
        let result = (|| -> Result<Representability, Failure> {
            let model::Kind::Function {
                signature,
                generics,
            } = definition.kind()
            else {
                return Err(reject::Kind::GenericImplementation.into());
            };
            // Only a method that takes the string by value: `&self` would
            // borrow a `&&str`. A method the impl defines reads its `self`,
            // the impl's own `&str`, as a borrow of `str`.
            let by_value = match signature.receiver() {
                model::Receiver::Value { .. } => true,
                model::Receiver::Reference {
                    mutability: model::Mutability::Shared,
                    ty: model::Type::Primitive(name),
                    ..
                } => &**name == "str",
                _ => false,
            };
            if !by_value
                || generics
                    .parameters()
                    .any(|parameter| parameter.owner() != definition.id())
            {
                return Err(reject::Kind::GenericImplementation.into());
            }
            let parameters = implementation
                .generics()
                .parameters()
                .filter(|parameter| !is_lifetime_parameter(parameter))
                .collect::<Vec<_>>();
            if parameters
                .iter()
                .any(|parameter| !matches!(parameter.kind(), model::ParameterKind::Type { .. }))
            {
                return Err(reject::Kind::GenericImplementation.into());
            }
            let inferred = parameters
                .iter()
                .map(|parameter| parameter.index())
                .collect::<Vec<_>>();
            let provided = provided_signature(implementation, definition, signature, generics)?;
            let (signature, generics) = match &provided {
                Some((signature, generics)) => (signature, generics),
                None => (signature, generics),
            };
            let inherited = model::Generics::inherited(implementation.generics(), generics)
                .map_err(|_| Error::Allocation)?;
            // A `self` the impl defines is its own `&str` taken by value,
            // as a provided method's `self` is.
            let owned;
            let signature = if matches!(signature.receiver(), model::Receiver::Reference { .. }) {
                owned = model::Signature::new(
                    signature.safety(),
                    signature.const_function(),
                    signature.async_function(),
                    signature.abi().into(),
                    signature.variadic(),
                    signature.binders().cloned().collect(),
                    model::Receiver::Value {
                        ty: implementation.self_type().clone(),
                    },
                    signature.inputs().cloned().collect(),
                    signature.output().clone(),
                )
                .with_guard(signature.guard().cloned());
                &owned
            } else {
                signature
            };
            // A method of an impl and trait with no type parameters, as
            // `impl Label for &str`, classifies as a plain method of the
            // string, which takes it by value all the same.
            let item = match self.function(signature, &inherited)? {
                Representability::FunctionItem(item) => item,
                Representability::Method(plan)
                    if plan.receiver() == representability::ownership::Receiver::Owned =>
                {
                    callable::Item::new(
                        plan.signature().clone(),
                        Some(plan.receiver()),
                        None,
                        Box::default(),
                        Box::default(),
                    )
                }
                _ => return Err(reject::Kind::GenericImplementation.into()),
            };
            let item = item
                .with_inferred(&inferred, parameters.len())
                .with_through(self.through(implementation, &inferred));
            if item.call().is_some() || !item.crossings().is_empty() {
                return Err(reject::Kind::CallableProtocol.into());
            }
            // The string at the lifetime the impl names: `&'static str`
            // when the impl is for that alone, as `impl Title<State> for
            // &'static str` is, and otherwise a `&str` the call borrows at
            // any lifetime, as rustc does (#717).
            let instance = if matches!(
                implementation.self_type(),
                model::Type::Reference {
                    lifetime: model::Lifetime::Static,
                    ..
                }
            ) {
                representability::convert::Builtin::StaticStr
            } else {
                representability::convert::Builtin::Str
            };
            let item = item.with_instance(representability::convert::Type::Builtin(instance));
            Ok(Representability::Invocation(callable::Invocation::new(
                item, 0,
            )))
        })();
        match result {
            Ok(plan) => {
                self.retain_inline_representability()?;
                Ok(OutcomeEvidence::Inline(Box::new(plan)))
            }
            Err(Failure::Reject(kind)) => {
                self.retained_items.set(retained_before.0);
                self.retained_bytes.set(retained_before.1);
                self.retain_span(implementation.span())?;
                self.retain_inline_representability()?;
                Ok(OutcomeEvidence::Inline(Box::new(
                    Representability::Rejected(reject::Reason::classification(
                        kind,
                        implementation.span().clone(),
                    )),
                )))
            }
            Err(Failure::Operational(error)) => Err(error),
        }
    }

    fn classify_inherent_invocation(
        &self,
        implementation: &model::Implementation,
        definition: &model::Definition,
    ) -> Result<OutcomeEvidence, Error> {
        use representability::{callable, reject, Representability};

        let retained_before = (self.retained_items.get(), self.retained_bytes.get());
        // An inherent or trait implementation for the generic type itself
        // (`impl<Tz: TimeZone> Datelike for DateTime<Tz>`): its methods take
        // the type's arguments from the receiver.
        let result = (|| -> Result<Representability, Failure> {
            let model::Type::Definition { id, arguments } = implementation.self_type() else {
                return Err(reject::Kind::GenericImplementation.into());
            };
            let parameters = implementation
                .generics()
                .parameters()
                .filter(|parameter| !is_lifetime_parameter(parameter))
                .collect::<Vec<_>>();
            let arguments = arguments
                .iter()
                .filter(|argument| {
                    !matches!(
                        argument,
                        model::Argument::Lifetime(
                            model::Lifetime::Static | model::Lifetime::Early { .. }
                        )
                    )
                })
                .collect::<Vec<_>>();
            if parameters.is_empty() {
                return Err(reject::Kind::GenericImplementation.into());
            }
            // An impl may fix some of its type's arguments, as pane_grid's
            // `impl<'a, Message, W, ..> Content<'a, Message, Void, W, ..>`
            // does: its own parameters stand in order among them, and the
            // bridge reaches the type at that instance.
            // An implementation's parameters its type doesn't name, as
            // `Renderer` of iced's `impl<'a, Message, W, Theme, Renderer>
            // Widget<Message, Theme, Renderer> for Button<'a, Message, W,
            // Theme>` and `Message` of its `impl<Message, Theme, Renderer>
            // Widget<Message, Theme, Renderer> for Text<'_, Theme,
            // Renderer>`, are the call's to infer, from what it gives and
            // takes, as rustc infers them. A trait's arguments name them.
            let trait_impl = matches!(implementation.trait_ref(), model::Trait::Trait { .. });
            let mut inferred = Vec::new();
            let mut infer = |parameter: &model::Parameter| {
                if trait_impl && matches!(parameter.kind(), model::ParameterKind::Type { .. }) {
                    inferred.push(parameter.index());
                    Ok(())
                } else {
                    Err(reject::Kind::GenericImplementation)
                }
            };
            // A parameter inside an argument the impl fixes, as `T` of
            // iced's `impl<T> Task<Option<T>>`, is the type's too: the
            // receiver's instance gives it.
            let mut nested = Vec::new();
            for argument in &arguments {
                if let model::Argument::Type(ty) = argument
                    && !matches!(ty, model::Type::Parameter { .. })
                {
                    named_parameters(ty, implementation.id(), &mut nested);
                }
            }
            // The type's arguments may name the impl's parameters in another
            // order than it declares them, as iced's `impl<W, A, B, F, Theme,
            // Renderer> Widget<B, Theme, Renderer> for Map<W, F, A>` does,
            // or name one twice. Then the bridge reaches the type at the
            // impl's own instance, which names each parameter where it
            // stands, as it does an impl that fixes an argument.
            let mut named = Vec::new();
            let mut fixed = false;
            for argument in arguments {
                match argument {
                    model::Argument::Type(model::Type::Parameter { owner, index })
                        if owner == implementation.id()
                            && parameters.iter().any(|parameter| {
                                parameter.index() == *index
                                    && matches!(parameter.kind(), model::ParameterKind::Type { .. })
                            }) =>
                    {
                        if named.last().is_some_and(|last| last >= index) {
                            fixed = true;
                        }
                        named.push(*index);
                    }
                    model::Argument::Type(model::Type::Parameter { .. }) => {
                        return Err(reject::Kind::GenericImplementation.into());
                    }
                    model::Argument::Type(_) => fixed = true,
                    _ => return Err(reject::Kind::GenericImplementation.into()),
                }
            }
            for parameter in &parameters {
                if !named.contains(&parameter.index()) && !nested.contains(&parameter.index()) {
                    infer(parameter)?;
                }
            }
            if !implementation.relevance().any(|relevant| relevant == id)
                || !implementation.retains_associated_owner(definition)
                || !matches!(definition.visibility(), model::Visibility::Public)
            {
                return Err(reject::Kind::GenericImplementation.into());
            }
            let model::Kind::Function {
                signature,
                generics,
            } = definition.kind()
            else {
                return Err(reject::Kind::GenericImplementation.into());
            };
            if generics
                .parameters()
                .any(|parameter| parameter.owner() != definition.id())
            {
                return Err(reject::Kind::GenericImplementation.into());
            }
            let container_parameters = parameters.len() - inferred.len();
            let provided = provided_signature(implementation, definition, signature, generics)?;
            let (signature, generics) = match &provided {
                Some((signature, generics)) => (signature, generics),
                None => (signature, generics),
            };
            let inherited = model::Generics::inherited(implementation.generics(), generics)
                .map_err(|_| Error::Allocation)?;
            // A method's receiver gives each parameter its type names, as a
            // `Map<W, F, A>` gives `F: Fn(A) -> B`: such a parameter is the
            // type the receiver holds, never a closure the call makes, so
            // its `Fn` bound, which rustc checks, takes no script function.
            let inherited = if matches!(signature.receiver(), model::Receiver::None) {
                inherited
            } else {
                let held = |index: &u32| named.contains(index) || nested.contains(index);
                model::Generics::new(
                    inherited.parameters().cloned().collect(),
                    inherited
                        .predicates()
                        .filter(|predicate| {
                            !matches!(
                                predicate,
                                model::Predicate::Bound {
                                    ty: model::Type::Parameter { owner, index },
                                    bound: model::Bound::Trait { trait_id, .. },
                                } if owner == implementation.id()
                                    && held(index)
                                    && matches!(
                                        self.definition_name(trait_id),
                                        Some("Fn" | "FnMut" | "FnOnce")
                                    )
                            )
                        })
                        .cloned()
                        .collect(),
                )
            };
            let Representability::FunctionItem(item) = self.function(signature, &inherited)? else {
                return Err(reject::Kind::GenericImplementation.into());
            };
            let item = item
                .with_inferred(&inferred, parameters.len())
                .with_through(self.through(implementation, &inferred));
            // An invocation's signature declares each of its parameters, and
            // an argument's `impl Fn(..)` has no name to declare, as in
            // Freya's `Callback<A, R>::new(impl FnMut(A) -> R + 'static)`,
            // unless Rawr makes its closure, as pane_grid's `PaneGrid::new(
            // &state, impl Fn(Pane, &S, bool) -> Content)`, or a slot fills
            // it, as `Cache<Renderer>::draw` lends its closure a `&mut
            // Frame<Renderer>` (T318).
            if item.call().is_none()
                && item.crossings().is_empty()
                && item
                    .signature()
                    .parameters()
                    .iter()
                    .any(|parameter| parameter.name().starts_with("impl "))
            {
                return Err(reject::Kind::CallableProtocol.into());
            }
            let item = if fixed {
                let instance = self
                    .convert(implementation.self_type())
                    .map_err(|_| reject::Kind::GenericImplementation)?;
                item.with_instance(instance)
            } else {
                item
            };
            Ok(Representability::Invocation(callable::Invocation::new(
                item,
                container_parameters,
            )))
        })();

        match result {
            Ok(plan) => {
                self.retain_inline_representability()?;
                Ok(OutcomeEvidence::Inline(Box::new(plan)))
            }
            Err(Failure::Reject(kind)) => {
                self.retained_items.set(retained_before.0);
                self.retained_bytes.set(retained_before.1);
                self.retain_span(implementation.span())?;
                self.retain_inline_representability()?;
                Ok(OutcomeEvidence::Inline(Box::new(
                    Representability::Rejected(reject::Reason::classification(
                        kind,
                        implementation.span().clone(),
                    )),
                )))
            }
            Err(Failure::Operational(error)) => Err(error),
        }
    }

    fn support_ledger(&self) -> Result<SupportLedger, Error> {
        let count = self.graph.support().len();
        let rejected = self
            .graph
            .support()
            .filter(|outcome| matches!(outcome, ledger::Support::Rejected { .. }))
            .count();
        let group_count = self
            .graph
            .definitions()
            .len()
            .checked_add(self.graph.implementations().len())
            .ok_or(Error::Limit {
                resource: "classified retained items",
                limit: self.item_limit,
                actual: usize::MAX,
            })?;
        self.retain_items(group_count)?;
        self.retain_items(count)?;
        self.retain_items(rejected)?;
        self.retain_array::<SupportGroup>(group_count)?;
        self.retain_array::<SupportEdge>(count)?;
        self.retain_array::<SupportRejection>(rejected)?;
        let mut groups = Vec::new();
        groups
            .try_reserve_exact(group_count)
            .map_err(|_| Error::Allocation)?;
        groups.extend(self.graph.definitions().map(|definition| SupportGroup {
            provenance: SupportProvenance::Definition(crate::surface::identity::Definition::of(
                definition.id(),
            )),
            count: 0,
        }));
        for implementation in self.graph.implementations() {
            groups.push(SupportGroup {
                provenance: SupportProvenance::Implementation(
                    crate::surface::identity::Implementation::of(implementation)?,
                ),
                count: 0,
            });
        }
        groups.sort_unstable_by_key(|group| group.provenance);
        if groups.len() != group_count
            || groups
                .windows(2)
                .any(|groups| groups[0].provenance == groups[1].provenance)
        {
            return Err(Error::Cardinality);
        }
        let mut edges = Vec::new();
        edges
            .try_reserve_exact(count)
            .map_err(|_| Error::Allocation)?;
        let mut rejections = Vec::new();
        rejections
            .try_reserve_exact(rejected)
            .map_err(|_| Error::Allocation)?;
        for outcome in self.graph.support() {
            let provenance = match outcome.role() {
                ledger::SupportRole::Definition => SupportProvenance::Definition(
                    crate::surface::identity::Definition::of(outcome.provenance()),
                ),
                ledger::SupportRole::Implementation => {
                    let index = self
                        .graph
                        .implementation_index(outcome.provenance())
                        .ok_or(Error::Cardinality)?;
                    let implementation = self
                        .graph
                        .implementations()
                        .nth(index)
                        .ok_or(Error::Cardinality)?;
                    SupportProvenance::Implementation(crate::surface::identity::Implementation::of(
                        implementation,
                    )?)
                }
            };
            let group = groups
                .binary_search_by_key(&provenance, |group| group.provenance)
                .map_err(|_| Error::Cardinality)?;
            groups[group].count = groups[group].count.checked_add(1).ok_or(Error::Limit {
                resource: "classified retained items",
                limit: self.item_limit,
                actual: usize::MAX,
            })?;
            let group = u32::try_from(group).map_err(|_| Error::IndexOverflow)?;
            let edge = match outcome {
                ledger::Support::Complete { id, .. } => SupportEdge {
                    definition: crate::surface::identity::Definition::of(id),
                    group,
                    rejection: None,
                },
                ledger::Support::Rejected {
                    id, reason, span, ..
                } => {
                    self.retain_ledger_reason(reason)?;
                    self.retain_span(span)?;
                    let rejection = rejections
                        .len()
                        .checked_add(1)
                        .and_then(|index| u32::try_from(index).ok())
                        .and_then(std::num::NonZeroU32::new)
                        .ok_or(Error::IndexOverflow)?;
                    rejections.push(SupportRejection {
                        reason: reason.clone(),
                        span: span.clone(),
                    });
                    SupportEdge {
                        definition: crate::surface::identity::Definition::of(id),
                        group,
                        rejection: Some(rejection),
                    }
                }
            };
            edges.push(edge);
        }
        if edges.len() != count || rejections.len() != rejected {
            return Err(Error::Cardinality);
        }
        Ok(SupportLedger {
            groups: groups.into_boxed_slice(),
            edges: edges.into_boxed_slice(),
            rejections: rejections.into_boxed_slice(),
        })
    }

    fn classify_definition(
        &self,
        definition_index: usize,
        definition: &model::Definition,
    ) -> Result<representability::Representability, Error> {
        use representability::{reject, Representability};
        let retained_before_attempt = (self.retained_items.get(), self.retained_bytes.get());
        let result = match definition.kind() {
            model::Kind::Rejected { reason, .. } => {
                self.retain_ledger_reason(reason)?;
                self.retain_span(definition.span())?;
                return Ok(Representability::Rejected(reject::Reason::extraction(
                    reason,
                    definition.span().clone(),
                )));
            }
            model::Kind::Struct { fields, generics } => self
                .structure(definition_index, fields, generics)
                .map(Representability::Struct),
            model::Kind::Enum { variants, generics } => self
                .enumeration(definition_index, variants, generics)
                .map(Representability::Enum),
            model::Kind::Const { ty, value } => {
                self.constant(ty, value).map(Representability::Constant)
            }
            // The only name of the type it names is a type of its own.
            model::Kind::TypeAlias { ty, .. }
                if self.instance_aliases().contains(&definition_index) =>
            {
                Ok(Representability::Struct(self.instance_alias_plan(ty)))
            }
            model::Kind::TypeAlias { ty, generics } => {
                self.value(ty, generics).map(Representability::Value)
            }
            model::Kind::Function {
                signature,
                generics,
            } => self.function(signature, generics),
            model::Kind::Module => Err(Failure::Reject(reject::Kind::Module)),
            model::Kind::Union { .. } => Err(Failure::Reject(reject::Kind::Union)),
            model::Kind::Static {
                ty,
                mutable: false,
                thread_local: false,
            } => self.statik(ty).map(Representability::Constant),
            model::Kind::Static { .. } => Err(Failure::Reject(reject::Kind::Static)),
            model::Kind::Trait { .. } => Err(Failure::Reject(reject::Kind::Trait)),
            model::Kind::AssociatedType { .. } => {
                Err(Failure::Reject(reject::Kind::AssociatedType))
            }
            model::Kind::TypeConst { .. } => Err(Failure::Reject(reject::Kind::TypeConstant)),
            model::Kind::Sysroot { .. } => Err(Failure::Reject(reject::Kind::SysrootDefinition)),
            model::Kind::MacroRules { .. } => Err(Failure::Reject(reject::Kind::MacroRules)),
        };
        match result {
            Ok(representability) => Ok(representability),
            Err(Failure::Reject(kind)) => {
                self.retained_items.set(retained_before_attempt.0);
                self.retained_bytes.set(retained_before_attempt.1);
                self.retain_span(definition.span())?;
                Ok(Representability::Rejected(reject::Reason::classification(
                    kind,
                    definition.span().clone(),
                )))
            }
            Err(Failure::Operational(error)) => Err(error),
        }
    }

    fn structure(
        &self,
        definition: usize,
        fields: &model::FieldSet,
        generics: &model::Generics,
    ) -> Result<representability::structure::Plan, Failure> {
        let parameters = self.sole_alias_defaults(definition, self.parameters(generics)?)?;
        // A struct whose every parameter has a default, as palette's
        // `Rgb<S = Srgb, T = f32>`, is made and read at that instance,
        // which its path names as a Rust type.
        let declaration_only = parameters
            .iter()
            .any(|parameter| parameter.default().is_none());
        let mut admitted = Vec::new();
        self.retain_array::<representability::structure::Field>(fields.public_fields().len())?;
        admitted
            .try_reserve_exact(fields.public_fields().len())
            .map_err(|_| Error::Allocation)?;
        let mut constructible = fields.public_complete() && !fields.non_exhaustive();
        let mut unsupported = None;
        for field in fields.public_fields() {
            // A public field of a type Rawr cannot hold is unavailable by
            // itself. The struct stays, without a constructor, and a
            // literal of it names the field (D664).
            let ty = match self.convert_field(field.ty()) {
                Ok(ty) => ty,
                Err(Failure::Reject(_)) => {
                    constructible = false;
                    unsupported.get_or_insert_with(|| {
                        representability::structure::Unsupported::new(
                            field.name().into(),
                            written(self.graph, generics, field.ty()).into(),
                        )
                    });
                    continue;
                }
                Err(error) => return Err(error),
            };
            self.retain_str(field.name())?;
            let selector = crate::surface::identity::Selector::StructField(
                crate::surface::identity::Field::new(field.index(), field.name().into()),
            );
            self.retain_str(field.name())?;
            self.retain_span(field.span())?;
            admitted.push(representability::structure::Field::new(
                selector,
                field.name().into(),
                field.index(),
                ty,
                field.span().clone(),
            ));
        }
        self.retain_form(fields)?;
        let form = crate::surface::identity::Form::from_fields(
            fields.form(),
            fields.public_fields().iter().map(|field| {
                crate::surface::identity::Field::new(field.index(), field.name().into())
            }),
        )?;
        // A generic constructor is available once demand supplies its
        // concrete instance; an uninstantiated use remains refused.
        let constructor =
            constructible.then_some(crate::surface::identity::Selector::StructConstructor(form));
        Ok(representability::structure::Plan::new(
            constructor,
            parameters,
            fields.form(),
            admitted.into_boxed_slice(),
            constructible,
            declaration_only,
            self.implements_for_any(definition, "Copy"),
            // A generic struct has the `Default` of an impl for any
            // arguments, as palette's `impl<S, T: Stimulus> Default for
            // Rgb<S, T>` and iced's `impl<Renderer: geometry::Renderer>
            // Default for Cache<Renderer>` are: `cargo check` holds each
            // instance to the impl's bounds, as it does for `Copy`.
            self.implements_for_any(definition, "Default"),
            self.implements_for_any(definition, "Debug"),
            self.implements_for_any(definition, "Clone"),
            self.implements_for_any(definition, "PartialEq"),
            self.implements_for_any(definition, "Eq"),
            self.implements_for_any(definition, "Hash"),
        )
        .with_order(
            self.implements_for_any(definition, "PartialOrd"),
            self.implements_for_any(definition, "Ord"),
        )
        .with_error(self.implements_for_any(definition, "Error"))
        .with_borrowing(
            lifetime_parameters(generics),
            covariant_lifetimes(generics),
            self.implements_for_any(definition, "Iterator"),
        )
        .with_given(given_parameters(generics))
        .with_unsupported(unsupported))
    }

    /// An input native code gives a script closure: a value, a copy of
    /// what a shared reference names, as pane_grid's `view(pane, &state,
    /// is_maximized)` gives `&S`, or a crate value lent `&mut` for the
    /// call, as a trait's `with_translation(v, f: impl FnOnce(&mut Self))`
    /// lends the renderer.
    fn convert_closure_input(
        &self,
        input: &model::Type,
    ) -> Option<representability::convert::Type> {
        use crate::surface::identity::Definition;
        if let Ok(input) = self.convert_value(input) {
            return Some(input);
        }
        if let Some(native) = self.native_place_input(input) {
            return Some(native);
        }
        let model::Type::Reference { mutability, ty, .. } = input else {
            return None;
        };
        let mutable = matches!(mutability, model::Mutability::Mutable);
        let target = self.convert_value(ty).ok()?;
        let admitted = match &target {
            representability::convert::Type::Parameter(_) => !mutable,
            // Rawr's own values cross by value, so only a crate value is
            // lent.
            representability::convert::Type::Nominal { definition, .. } => {
                !mutable
                    || !([
                        Definition::value(),
                        Definition::duration(),
                        Definition::instant(),
                        Definition::infallible(),
                    ]
                    .contains(definition)
                        || Definition::std_path(definition.as_bytes()).is_some())
            }
            _ => false,
        };
        admitted.then(|| representability::convert::Type::Borrowed {
            mutable,
            ty: Box::new(target),
        })
    }

    /// An input native code gives a script function that fills a callable
    /// parameter by a slot (D594): a value, or a shared or `&mut` borrow
    /// of one, which the slot lends the function for the call, as
    /// `update(&mut state, message)` takes `&mut S`.
    fn convert_crossing_input(
        &self,
        input: &model::Type,
    ) -> Option<representability::convert::Type> {
        if let Ok(input) = self.convert_value(input) {
            return Some(input);
        }
        let model::Type::Reference { mutability, ty, .. } = input else {
            return None;
        };
        let mutable = matches!(mutability, model::Mutability::Mutable);
        if mutable && let Some(native) = self.native_place_input(input) {
            return Some(native);
        }
        let target = self.convert_value(ty).ok()?;
        matches!(
            target,
            representability::convert::Type::Parameter(_)
                | representability::convert::Type::Nominal { .. }
        )
        .then(|| representability::convert::Type::Borrowed {
            mutable,
            ty: Box::new(target),
        })
    }

    /// `&mut Vec<T>` or `&mut String` that native code lends a script
    /// function or closure: a typed native place for the call (D651).
    fn native_place_input(&self, input: &model::Type) -> Option<representability::convert::Type> {
        if let Some(element) = self.copied_vec(input) {
            return Some(representability::convert::Type::Borrowed {
                mutable: true,
                ty: Box::new(representability::convert::Type::Sequence {
                    element: Box::new(self.convert(element).ok()?),
                    owned: true,
                    length: None,
                }),
            });
        }
        let model::Type::Reference {
            mutability: model::Mutability::Mutable,
            ty,
            ..
        } = input
        else {
            return None;
        };
        let string @ representability::convert::Type::Builtin(
            representability::convert::Builtin::String,
        ) = self.convert(ty).ok()?
        else {
            return None;
        };
        Some(representability::convert::Type::Borrowed {
            mutable: true,
            ty: Box::new(string),
        })
    }

    /// Whether a function's output is a struct or enum that declares a
    /// lifetime and names an early-bound one, and every input that borrows
    /// at an early lifetime borrows shared, as a widget's builder
    /// `Field<'a, ..>::value(self, value: &'a str) -> Self` does: the output
    /// keeps those inputs borrowed for as long as it lives, as rustc reads
    /// the signature. A shared borrow stays valid for as long as the region
    /// that holds the output, which pins the strings and holds the values
    /// it lends; an exclusive one would keep its place locked for that long.
    fn lends_shared(&self, signature: &model::Signature) -> bool {
        let model::Type::Definition { id, .. } = signature.output() else {
            return false;
        };
        let declares = self
            .graph
            .definition_index(id)
            .and_then(|index| self.graph.definitions().nth(index))
            .is_some_and(|definition| match definition.kind() {
                model::Kind::Struct { generics, .. } | model::Kind::Enum { generics, .. } => {
                    lifetime_parameters(generics) != 0
                }
                _ => false,
            });
        declares
            && names_early_lifetime(signature.output())
            && !signature.inputs().any(borrows_mutably_at_early_lifetime)
    }

    /// Whether a function with no receiver gives a struct or enum that
    /// declares a lifetime, and each input that borrows at an early lifetime is a
    /// shared reference to a crate type that declares none, as iced's
    /// `qr_code<'a, Theme>(data: &'a Data) -> QRCode<'a, Theme>`: its output
    /// borrows those values, which a view's region holds for as long as the
    /// output lives. The output may also be opaque and capture an early
    /// lifetime, and an input may be a shared slice of such crate values, as
    /// in iced's `markdown::view<'a>(items: &'a [Item], ..) -> impl
    /// Widget<..> + 'a`.
    fn lends_values(&self, signature: &model::Signature) -> bool {
        let lifetimes = |id: &api::identity::Definition| {
            self.graph
                .definition_index(id)
                .and_then(|index| self.graph.definitions().nth(index))
                .and_then(|definition| match definition.kind() {
                    model::Kind::Struct { generics, .. } | model::Kind::Enum { generics, .. } => {
                        Some(lifetime_parameters(generics))
                    }
                    _ => None,
                })
        };
        let output = match signature.output() {
            model::Type::Definition { id, .. } => {
                matches!(lifetimes(id), Some(count) if count != 0)
            }
            model::Type::ImplTrait { captures, .. } => captures.iter().any(|argument| {
                matches!(
                    argument,
                    model::Argument::Lifetime(model::Lifetime::Early { .. })
                )
            }),
            _ => false,
        };
        let crate_value = |ty: &model::Type| {
            matches!(ty, model::Type::Definition { id, arguments }
                if lifetimes(id) == Some(0)
                    && !arguments.iter().any(|argument| matches!(argument,
                        model::Argument::Type(ty) if borrows_at_early_lifetime(ty))))
        };
        matches!(signature.receiver(), model::Receiver::None)
            && output
            && signature
                .inputs()
                .filter(|input| borrows_at_early_lifetime(input))
                .all(|input| {
                    matches!(input, model::Type::Reference {
                        mutability: model::Mutability::Shared,
                        lifetime: model::Lifetime::Early { .. },
                        ty,
                    } if match &**ty {
                        model::Type::Slice(element) => crate_value(element),
                        ty => crate_value(ty),
                    })
                })
    }

    /// Whether a call's `output`, when it is opaque and borrows the call's
    /// inputs, has bounds Rawr can write, so it is held beside what it
    /// borrows through a proxy of them (#846, #849): each is a trait at
    /// types Rawr writes, and none is higher-ranked or negative. Any other
    /// output is held as it is.
    fn writes_opaque(&self, output: &model::Type) -> bool {
        let model::Type::ImplTrait { bounds, .. } = output else {
            return true;
        };
        let written = bounds.iter().all(|predicate| match predicate {
            model::Predicate::Bound {
                bound:
                    model::Bound::Trait {
                        polarity, binders, ..
                    },
                ..
            } => *polarity == model::Polarity::Positive && binders.is_empty(),
            _ => true,
        });
        written && self.opaque_traits(bounds).is_some()
    }

    /// The traits of an opaque type's `bounds` besides auto traits and
    /// `Sized`, at the bound's arguments after `Self`, as `Show` of `impl
    /// Show + 'a` (`signature::Opaque::traits`). `None` when Rawr can't
    /// write the arguments of one.
    fn opaque_traits(
        &self,
        bounds: &[model::Predicate],
    ) -> Option<Box<[representability::convert::Type]>> {
        let mut traits = Vec::new();
        for predicate in bounds {
            let model::Predicate::Bound {
                bound:
                    model::Bound::Trait {
                        trait_id,
                        arguments,
                        polarity: model::Polarity::Positive,
                        binders,
                    },
                ..
            } = predicate
            else {
                continue;
            };
            if !binders.is_empty()
                || self.is_core_trait(trait_id, "Sized")
                || self.sizedness(trait_id)
                || self.auto_trait(trait_id)
            {
                continue;
            }
            // `Self`, as `PartialEq<Self>` names it, is no type Rawr writes.
            let arguments = arguments
                .iter()
                .filter_map(|argument| match argument {
                    model::Argument::Type(model::Type::SelfType(_)) => Some(None),
                    model::Argument::Type(ty) => Some(self.convert(ty).ok()),
                    _ => None,
                })
                .collect::<Option<Box<[_]>>>()?;
            traits.push(representability::convert::Type::Nominal {
                definition: crate::surface::identity::Definition::of(trait_id),
                arguments,
            });
        }
        Some(traits.into_boxed_slice())
    }

    /// Each method of the one trait of an opaque type's `bounds`
    /// (`opaque_traits`), when a value held through its proxy can
    /// implement the trait by calling the method of the value inside
    /// (#856), as `wrap(s: impl Show)` takes a `view(&items)` that lives
    /// beside its items:
    /// each takes `&self` or `&mut self`, has no generics of its own, and
    /// takes and gives types Rawr writes as they are, giving none that
    /// borrows. The trait has no arguments, supertraits, associated types
    /// or methods only a `Sized` type has. `None` when it isn't so.
    fn opaque_forwards(
        &self,
        bounds: &[model::Predicate],
    ) -> Option<Box<[representability::signature::Forward]>> {
        let definition = |id: &api::identity::Definition| {
            self.graph
                .definition_index(id)
                .and_then(|index| self.graph.definitions().nth(index))
        };
        let mut principal = None;
        for predicate in bounds {
            let model::Predicate::Bound {
                bound:
                    model::Bound::Trait {
                        trait_id,
                        arguments,
                        polarity: model::Polarity::Positive,
                        binders,
                    },
                ..
            } = predicate
            else {
                continue;
            };
            if !binders.is_empty()
                || self.is_core_trait(trait_id, "Sized")
                || self.sizedness(trait_id)
                || self.auto_trait(trait_id)
            {
                continue;
            }
            // Its arguments after `Self`.
            if arguments.len() > 1 || principal.replace(trait_id).is_some() {
                return None;
            }
        }
        let id = principal?;
        let Some(model::Kind::Trait {
            supertraits,
            associated,
            ..
        }) = definition(id).map(model::Definition::kind)
        else {
            return None;
        };
        let supertrait = supertraits.iter().any(|predicate| {
            !matches!(predicate, model::Predicate::Bound {
                bound: model::Bound::Trait { trait_id, .. },
                ..
            } if self.sizedness(trait_id) || self.auto_trait(trait_id))
        });
        if supertrait {
            return None;
        }
        let mut forwards = Vec::new();
        for item in associated.iter() {
            let member = definition(item)?;
            let model::Kind::Function {
                signature,
                generics,
            } = member.kind()
            else {
                return None;
            };
            let mutable = match signature.receiver() {
                model::Receiver::Reference { mutability, .. } => {
                    matches!(mutability, model::Mutability::Mutable)
                }
                _ => return None,
            };
            let own = generics
                .parameters()
                .any(|parameter| parameter.owner() == item);
            let sized = generics.predicates().any(|predicate| {
                matches!(predicate, model::Predicate::Bound {
                    bound: model::Bound::Trait { trait_id, .. },
                    ..
                } if self.is_core_trait(trait_id, "Sized"))
            });
            if own || sized || signature.async_function() {
                return None;
            }
            let mut inputs = Vec::new();
            for input in signature.inputs() {
                if !self.forwardable(input, true) {
                    return None;
                }
                inputs.push(self.convert(input).ok()?);
            }
            if !self.forwardable(signature.output(), false) {
                return None;
            }
            let output = self.convert(signature.output()).ok()?;
            let name = self.definition_name(item)?;
            self.retain_str(name).ok()?;
            forwards.push(representability::signature::Forward::new(
                name.into(),
                mutable,
                inputs.into_boxed_slice(),
                output,
            ));
        }
        Some(forwards.into_boxed_slice())
    }

    /// Whether `ty`, in a method a value held as a trait object forwards
    /// (`opaque_forwards`), is one Rawr writes as it is: a primitive, the
    /// unit type, a `String`, an `Option` of such a type, or a crate's own
    /// type without type arguments, and, as an `input`, a reference to one
    /// at any lifetime. An output borrows nothing.
    fn forwardable(&self, ty: &model::Type, input: bool) -> bool {
        match ty {
            model::Type::Primitive(name) => name.as_ref() != "str",
            model::Type::Tuple(members) => members.is_empty(),
            model::Type::Reference { ty, .. } if input => {
                matches!(&**ty, model::Type::Primitive(name) if name.as_ref() == "str")
                    || self.forwardable(ty, false)
            }
            model::Type::Definition { id, arguments } => {
                let Some(definition) = self
                    .graph
                    .definition_index(id)
                    .and_then(|index| self.graph.definitions().nth(index))
                else {
                    return false;
                };
                match std_binding::of(definition) {
                    Some(Std::String) => true,
                    Some(Std::Option) => matches!(
                        &arguments[..],
                        [model::Argument::Type(inner)] if self.forwardable(inner, false)
                    ),
                    Some(_) => false,
                    None => arguments.iter().all(|argument| match argument {
                        model::Argument::Lifetime(_) => input,
                        _ => false,
                    }),
                }
            }
            _ => false,
        }
    }

    /// Whether `id` is one of core's traits below `Sized` that every type
    /// a trait object can be implements, as rustc's `MetaSized` is of each
    /// trait's `Self`.
    fn sizedness(&self, id: &api::identity::Definition) -> bool {
        self.is_core_trait(id, "MetaSized") || self.is_core_trait(id, "PointeeSized")
    }

    /// Whether `id` is an auto trait, as `Send` and `Sync` are.
    fn auto_trait(&self, id: &api::identity::Definition) -> bool {
        ["Send", "Sync", "Unpin"]
            .into_iter()
            .any(|name| self.is_core_trait(id, name))
            || self
                .graph
                .definition_index(id)
                .and_then(|index| self.graph.definitions().nth(index))
                .is_some_and(|definition| {
                    matches!(
                        definition.kind(),
                        model::Kind::Trait {
                            auto_trait: true,
                            ..
                        }
                    )
                })
    }

    fn enumeration(
        &self,
        definition: usize,
        variants: &[model::Variant],
        generics: &model::Generics,
    ) -> Result<representability::enumeration::Plan, Failure> {
        let parameters = self.sole_alias_defaults(definition, self.parameters(generics)?)?;
        let declaration_only = !parameters.is_empty();
        let mut admitted = Vec::new();
        self.retain_array::<representability::enumeration::Variant>(variants.len())?;
        admitted
            .try_reserve_exact(variants.len())
            .map_err(|_| Error::Allocation)?;
        for (variant_index, variant) in variants.iter().enumerate() {
            self.retain_form(variant.field_set())?;
            self.retain_str(variant.name())?;
            let form = crate::surface::identity::Form::from_fields(
                variant.field_set().form(),
                variant.fields().map(|field| {
                    crate::surface::identity::Field::new(field.index(), field.name().into())
                }),
            )?;
            let variant_selector = crate::surface::identity::Selector::EnumVariant {
                index: u32::try_from(variant_index).map_err(|_| Error::IndexOverflow)?,
                name: variant.name().into(),
                form,
            };
            let mut fields = Vec::new();
            self.retain_array::<representability::structure::Field>(variant.fields().len())?;
            fields
                .try_reserve_exact(variant.fields().len())
                .map_err(|_| Error::Allocation)?;
            let mut constructible =
                variant.field_set().public_complete() && !variant.field_set().non_exhaustive();
            let mut unsupported = None;
            for field in variant.fields() {
                if !matches!(field.visibility(), model::Visibility::Public) {
                    constructible = false;
                    continue;
                }
                // A field of a type Rawr cannot hold is unavailable by
                // itself, as in a struct. The variant stays and cannot be
                // built, and a literal of it names the field. A field that
                // borrows is held as a struct's is.
                let ty = match self.convert_field(field.ty()) {
                    Ok(ty) => ty,
                    Err(Failure::Reject(_)) => {
                        constructible = false;
                        unsupported.get_or_insert_with(|| {
                            representability::structure::Unsupported::new(
                                field.name().into(),
                                written(self.graph, generics, field.ty()).into(),
                            )
                        });
                        continue;
                    }
                    Err(error) => return Err(error),
                };
                self.retain_selector(&variant_selector)?;
                self.retain_str(field.name())?;
                self.retain_span(field.span())?;
                fields.push(representability::structure::Field::new(
                    variant_selector.clone(),
                    field.name().into(),
                    field.index(),
                    ty,
                    field.span().clone(),
                ));
            }
            self.retain_str(variant.name())?;
            self.retain_span(variant.span())?;
            admitted.push(
                representability::enumeration::Variant::new(
                    variant_selector,
                    variant.name().into(),
                    variant.field_set().form(),
                    fields.into_boxed_slice(),
                    constructible,
                    variant.span().clone(),
                )
                .with_unsupported(unsupported),
            );
        }
        Ok(representability::enumeration::Plan::new(
            parameters,
            admitted.into_boxed_slice(),
            declaration_only,
            self.implements_for_any(definition, "Copy"),
            self.implements_for_any(definition, "Default"),
            self.implements_for_any(definition, "Debug"),
            self.implements_for_any(definition, "Clone"),
            self.implements_for_any(definition, "PartialEq"),
            self.implements_for_any(definition, "Eq"),
            self.implements_for_any(definition, "Hash"),
        )
        .with_order(
            self.implements_for_any(definition, "PartialOrd"),
            self.implements_for_any(definition, "Ord"),
        )
        .with_error(self.implements_for_any(definition, "Error"))
        .with_borrowing(
            lifetime_parameters(generics),
            covariant_lifetimes(generics),
            self.implements_for_any(definition, "Iterator"),
        )
        .with_given(given_parameters(generics)))
    }

    fn constant(
        &self,
        ty: &model::Type,
        value: &model::Default<model::Const>,
    ) -> Result<representability::constant::Plan, Failure> {
        use representability::convert::Type;
        let model::Default::Present(value) = value else {
            return Err(representability::reject::Kind::MissingConstantValue.into());
        };
        if let Some(text) = static_text(ty) {
            return Ok(representability::constant::Plan::new(text, None));
        }
        if let Some(scalars) = self.static_scalars(ty) {
            return Ok(representability::constant::Plan::new(scalars, None));
        }
        let ty = self.convert_value(ty)?;
        // A constant of another type, as Freya's `Color::WHITE: Color =
        // Color::new(..)`, is read by its path where the bridge runs, as a
        // static is: each read is a new value, as Rust's is. So is one
        // whose value is its own, which rustc evaluates to no scalar
        // (`&'static str`).
        if !matches!(ty, Type::Unit | Type::Builtin(_))
            || matches!(value, model::Const::Unevaluated { arguments, .. } if arguments.is_empty())
        {
            return Ok(representability::constant::Plan::new(ty, None));
        }
        if let model::Const::Literal { ty, value } = value {
            self.retain_str(ty)?;
            self.retain_str(value)?;
        }
        Ok(representability::constant::Plan::new(
            ty,
            Some(constant(value)?),
        ))
    }

    /// An immutable `static`, read by its path where the bridge runs: a
    /// slice of values the bridge clones (game_of_life's `pub static ALL:
    /// &[Preset]`), or a primitive.
    ///
    /// A slice of crate values without `Clone` that borrow nothing, as a
    /// page's `pub static BLOCKS: &[Block]`, is lent where it is: Rawr
    /// reads its elements in place, as Rust does, and copies none (#1291).
    /// So is a crate value that borrows nothing, as a page's `pub static
    /// META: Meta`, which a `&'static Meta` result gives itself (#1442).
    fn statik(&self, ty: &model::Type) -> Result<representability::constant::Plan, Failure> {
        use representability::convert::Type;
        if let Some(text) = static_text(ty) {
            return Ok(representability::constant::Plan::new(text, None));
        }
        if let Some(scalars) = self.static_scalars(ty) {
            return Ok(representability::constant::Plan::new(scalars, None));
        }
        if !names_early_lifetime(ty)
            && !names_late_lifetime(ty)
            && let Ok(value) = self.convert(ty)
            // One of std's types Rawr holds as its own value is read as
            // that value.
            && matches!(&value, Type::Nominal { definition, .. }
                if crate::surface::identity::Definition::std_path(definition.as_bytes()).is_none())
        {
            return Ok(representability::constant::Plan::lasting(value));
        }
        if let model::Type::Reference {
            mutability: model::Mutability::Shared,
            ty: target,
            ..
        } = ty
            && let model::Type::Slice(element) = &**target
            && !names_early_lifetime(element)
            && !names_late_lifetime(element)
            // Wire values, as a pair of numbers, are copied as an array
            // (`convert_value`).
            && let Ok(element) = self.convert(element)
            && matches!(element, Type::Nominal { .. })
            && !self.clones(&element)
        {
            return Ok(representability::constant::Plan::new(
                Type::Borrowed {
                    mutable: false,
                    ty: Box::new(Type::Sequence {
                        element: Box::new(element),
                        owned: false,
                        length: None,
                    }),
                },
                None,
            ));
        }
        let ty = self.convert_value(ty)?;
        if !matches!(
            ty,
            Type::Unit | Type::Builtin(_) | Type::Sequence { owned: false, .. }
        ) {
            return Err(representability::reject::Kind::Static.into());
        }
        Ok(representability::constant::Plan::new(ty, None))
    }

    /// The type of a `const` or `static` of a slice of numbers, `bool`s or
    /// `char`s, as `pub const BYTES: &[u8]`: `&'static [T]` in either, a
    /// borrow of Rawr's array of them, so a `for` over it gives `&T` and
    /// `.len()` reads it as rustc's do (#1146).
    fn static_scalars(&self, ty: &model::Type) -> Option<representability::convert::Type> {
        use representability::convert::Type;
        let model::Type::Reference {
            mutability: model::Mutability::Shared,
            ty: target,
            ..
        } = ty
        else {
            return None;
        };
        let model::Type::Slice(element) = &**target else {
            return None;
        };
        let element = self.convert(element).ok()?;
        matches!(element, Type::Builtin(_)).then(|| Type::Borrowed {
            mutable: false,
            ty: Box::new(Type::Sequence {
                element: Box::new(element),
                owned: false,
                length: None,
            }),
        })
    }

    /// A value the bridge gives Rawr: as `convert`, and a `Vec<T>`, a
    /// borrowed slice `&[T]` of `Clone` elements, or a tuple, which
    /// cross as Rawr's arrays and tuples.
    fn convert_value(&self, ty: &model::Type) -> Result<representability::convert::Type, Failure> {
        use representability::convert::Type;
        match ty {
            model::Type::Reference {
                mutability: model::Mutability::Shared,
                ty,
                ..
            } if let model::Type::Slice(element) = ty.as_ref() => {
                let element = self.convert_value(element)?;
                if !self.clones(&element) {
                    return Err(representability::reject::Kind::SliceType.into());
                }
                Ok(Type::Sequence {
                    element: Box::new(element),
                    owned: false,
                    length: None,
                })
            }
            model::Type::Definition { id, arguments }
                if let Some(element) = self
                    .graph
                    .definition_index(id)
                    .and_then(|index| self.graph.definitions().nth(index))
                    .and_then(|definition| vec_element(definition, arguments)) =>
            {
                Ok(Type::Sequence {
                    element: Box::new(self.convert_value(element)?),
                    owned: true,
                    length: None,
                })
            }
            // A shared slice, as ratatui's `Layout::split` gives an
            // `Rc<[Rect]>`: its elements are cloned out, as a `&[T]`'s are.
            model::Type::Definition { id, arguments }
                if let Some(element) = self
                    .graph
                    .definition_index(id)
                    .and_then(|index| self.graph.definitions().nth(index))
                    .and_then(|definition| shared_slice_element(definition, arguments)) =>
            {
                let element = self.convert_value(element)?;
                if !self.clones(&element) {
                    return Err(representability::reject::Kind::SliceType.into());
                }
                Ok(Type::Sequence {
                    element: Box::new(element),
                    owned: false,
                    length: None,
                })
            }
            // An `Option` of a tuple, as pane_grid's `State::split` gives
            // `Option<(Pane, Split)>`: Rawr's `Option` of its tuple.
            model::Type::Definition { id, arguments }
                if let Some(model::Type::Tuple(members)) = self
                    .graph
                    .definition_index(id)
                    .and_then(|index| self.graph.definitions().nth(index))
                    .and_then(|definition| option_element(definition, arguments))
                    && members.len() >= 2 =>
            {
                let element = self
                    .graph
                    .definition_index(id)
                    .and_then(|index| self.graph.definitions().nth(index))
                    .and_then(|definition| option_element(definition, arguments))
                    .ok_or(Error::Cardinality)?;
                Ok(Type::Option(Box::new(self.convert_value(element)?)))
            }
            // A `Result` of values, as changelog's `async fn list()` gives
            // `Result<(Self, Vec<Contribution>), Error>`: Rawr's `Result`
            // of the tuple and `Vec` it holds.
            model::Type::Definition { id, arguments }
                if let Some((ok, err)) = self
                    .graph
                    .definition_index(id)
                    .and_then(|index| self.graph.definitions().nth(index))
                    .and_then(|definition| result_elements(definition, arguments)) =>
            {
                Ok(Type::Result(
                    Box::new(self.convert_value(ok)?),
                    Box::new(self.convert_value(err)?),
                ))
            }
            model::Type::Tuple(members) if members.len() >= 2 => {
                let mut converted = Vec::new();
                self.retain_array::<Type>(members.len())?;
                converted
                    .try_reserve_exact(members.len())
                    .map_err(|_| Error::Allocation)?;
                for member in members {
                    converted.push(self.convert_value(member)?);
                }
                Ok(Type::Tuple(converted.into_boxed_slice()))
            }
            // An array of a known length, as `corners() -> [u8; 4]` gives:
            // Rawr's array of that length (#1330).
            model::Type::Array { .. } => self.convert_held(ty),
            ty => self.convert(ty),
        }
    }

    /// Whether the crate's values of `ty` clone, so a slice of them can
    /// be copied out.
    fn clones(&self, ty: &representability::convert::Type) -> bool {
        use representability::convert::Type;
        match ty {
            Type::Inferred => false,
            Type::Unit | Type::Never | Type::Builtin(_) => true,
            Type::Nominal { definition, .. } => self
                .graph
                .definitions()
                .position(|candidate| {
                    crate::surface::identity::Definition::of(candidate.id()) == *definition
                })
                .is_some_and(|index| self.implements_for_any(index, "Clone")),
            Type::Option(element)
            | Type::Borrowed { ty: element, .. }
            | Type::Range(element)
            | Type::Sequence { element, .. }
            | Type::ConstArray { element, .. } => self.clones(element),
            Type::Result(ok, err) => self.clones(ok) && self.clones(err),
            Type::Tuple(members) => members.iter().all(|member| self.clones(member)),
            Type::FunctionItem { .. }
            | Type::ProfileMessage { .. }
            | Type::Parameter(_)
            | Type::Script { .. } => false,
        }
    }

    fn value(
        &self,
        ty: &model::Type,
        generics: &model::Generics,
    ) -> Result<representability::value::Plan, Failure> {
        let parameters = self.parameters(generics)?;
        Ok(representability::value::Plan::new(
            self.convert(ty)?,
            parameters,
        ))
    }

    fn function(
        &self,
        signature: &model::Signature,
        generics: &model::Generics,
    ) -> Result<representability::Representability, Failure> {
        if signature.safety() != model::Safety::Safe {
            return Err(representability::reject::Kind::UnsafeFunction.into());
        }
        if signature.variadic() {
            return Err(representability::reject::Kind::VariadicFunction.into());
        }
        if signature.abi() != "\"Rust\"" {
            return Err(representability::reject::Kind::ForeignAbi.into());
        }
        let pointers = self.pointer_callables(signature, generics);
        let (signature, generics) = pointers
            .as_ref()
            .map_or((signature, generics), |(signature, generics)| {
                (signature, generics)
            });
        let objects = object_inputs(signature, generics);
        let (signature, generics) = objects
            .as_ref()
            .map_or((signature, generics), |(signature, generics)| {
                (signature, generics)
            });
        let normalized = self.self_projections(signature);
        let signature = normalized.as_ref().unwrap_or(signature);
        let chosen = argument_binders_static(signature);
        let signature = chosen.as_ref().unwrap_or(signature);
        let opens_copies = self.late_binders_open_copies(signature);
        let borrows_receiver = borrows_receiver(signature, |id| {
            self.graph
                .definition_index(id)
                .and_then(|index| self.graph.definitions().nth(index))
                .is_some_and(|definition| std_binding::of(definition) == Some(Std::Option))
        });
        let outer_scope = self
            .scope_lifetimes
            .replace(opens_copies || borrows_receiver);
        let classified = self.function_with(signature, generics, opens_copies, borrows_receiver);
        self.scope_lifetimes.set(outer_scope);
        // A plan's signature names only the type parameters it declares, so
        // the facade writes each by its name (#1032).
        match classified {
            Ok(plan) if !declares_its_parameters(&plan) => {
                Err(representability::reject::Kind::UndeclaredParameter.into())
            }
            classified => classified,
        }
    }

    /// `signature` with each `<T as Trait>::Name` of a type `T` the crate
    /// implements the trait for, set to a type that projects from a type
    /// parameter, read as that type, as `Product<I, J>`'s `impl Iterator`
    /// sets `Item` to `(I::Item, J::Item)`: `next`'s `Option<Self::Item>`
    /// names `I::Item` and `J::Item`, which the call infers from the
    /// receiver (`projections`). `None` when none does.
    fn self_projections(&self, signature: &model::Signature) -> Option<model::Signature> {
        if self.opaque_self.borrow().is_some() {
            return None;
        }
        let mut changed = false;
        let inputs = signature
            .inputs()
            .map(|input| self.self_projected(input, &mut changed))
            .collect::<Box<[_]>>();
        let output = self.self_projected(signature.output(), &mut changed);
        changed.then(|| {
            model::Signature::new(
                signature.safety(),
                signature.const_function(),
                signature.async_function(),
                signature.abi().into(),
                signature.variadic(),
                signature.binders().cloned().collect(),
                signature.receiver().clone(),
                inputs,
                output,
            )
            .with_guard(signature.guard().cloned())
        })
    }

    /// `ty` with each `<T as Trait>::Name` read as `self_projections`
    /// reads it, `changed` set when one is.
    fn self_projected(&self, ty: &model::Type, changed: &mut bool) -> model::Type {
        let mut argument = |argument: &model::Argument| match argument {
            model::Argument::Type(ty) => model::Argument::Type(self.self_projected(ty, changed)),
            other => other.clone(),
        };
        match ty {
            model::Type::Projection { arguments, .. }
                if !matches!(
                    arguments.first(),
                    Some(model::Argument::Type(model::Type::Parameter { .. }))
                ) =>
            {
                match self.projected(ty) {
                    Some(value) if projects_parameter(value) => {
                        *changed = true;
                        value.clone()
                    }
                    _ => ty.clone(),
                }
            }
            model::Type::Definition { id, arguments } => model::Type::Definition {
                id: id.clone(),
                arguments: arguments.iter().map(&mut argument).collect(),
            },
            model::Type::Tuple(members) => model::Type::Tuple(
                members
                    .iter()
                    .map(|member| self.self_projected(member, changed))
                    .collect(),
            ),
            model::Type::Reference {
                lifetime,
                mutability,
                ty,
            } => model::Type::Reference {
                lifetime: lifetime.clone(),
                mutability: *mutability,
                ty: Box::new(self.self_projected(ty, changed)),
            },
            other => other.clone(),
        }
    }

    /// `signature` and `generics` with each input that is a safe Rust
    /// function pointer of other than one input, as iced's
    /// `event::listen_with(f: fn(Event, Status, window::Id) ->
    /// Option<Message>)` takes, read as a parameter `impl Fn(..) -> R` of
    /// its own. A script function or a closure that captures nothing
    /// fills it as a slot, a function item that coerces to the pointer
    /// (D594), and so does a crate function. A pointer of one input stays
    /// the function item it is (`convert_input`). `None` when there is none.
    fn pointer_callables(
        &self,
        signature: &model::Signature,
        generics: &model::Generics,
    ) -> Option<(model::Signature, model::Generics)> {
        fn pointer(ty: &model::Type) -> Option<&model::Signature> {
            match ty {
                model::Type::FunctionPointer(pointer)
                    if pointer.safety() == model::Safety::Safe
                        && !pointer.async_function()
                        && !pointer.variadic()
                        && pointer.abi() == "\"Rust\""
                        && pointer.binders().len() == 0
                        && matches!(pointer.receiver(), model::Receiver::None)
                        && pointer.inputs().len() != 1 =>
                {
                    Some(pointer)
                }
                _ => None,
            }
        }
        if !signature.inputs().any(|input| pointer(input).is_some()) {
            return None;
        }
        let (callable, once, output) = self.callable_traits()?;
        // A parameter is read by its index alone, so one an item with no
        // parameters of its own declares, as image-like `Grid::fold(&self,
        // u32, fn(u32, u8) -> u32)` or a free `combine(.., fn(i64, i64, i64)
        // -> i64)`, is owned by `Fn` itself.
        let owner = generics
            .parameters()
            .next()
            .map_or_else(|| callable.clone(), |parameter| parameter.owner().clone());
        let mut next = generics
            .parameters()
            .map(model::Parameter::index)
            .filter(|index| *index & representability::signature::FUTURE_OUTPUT == 0)
            .max()
            .map_or(0, |index| index + 1);
        let mut parameters = generics.parameters().cloned().collect::<Vec<_>>();
        let mut predicates = generics.predicates().cloned().collect::<Vec<_>>();
        let mut inputs = Vec::new();
        for input in signature.inputs() {
            let Some(pointer) = pointer(input) else {
                inputs.push(input.clone());
                continue;
            };
            let index = next;
            next += 1;
            let parameter = model::Type::Parameter {
                owner: owner.clone(),
                index,
            };
            let arguments = model::Type::Tuple(pointer.inputs().cloned().collect());
            parameters.push(model::Parameter::new(
                owner.clone(),
                index,
                format!("impl Fn#{index}").into(),
                model::ParameterKind::Type {
                    bounds: Box::default(),
                    default: model::Default::Absent,
                },
            ));
            predicates.push(model::Predicate::Bound {
                ty: parameter.clone(),
                bound: model::Bound::Trait {
                    trait_id: callable.clone(),
                    arguments: Box::from([model::Argument::Type(arguments.clone())]),
                    polarity: model::Polarity::Positive,
                    binders: Box::default(),
                },
            });
            // A function pointer holds no borrow of its own.
            predicates.push(model::Predicate::TypeOutlives {
                ty: parameter.clone(),
                lifetime: model::Lifetime::Static,
            });
            predicates.push(model::Predicate::ProjectionEquality {
                projection: model::Type::Projection {
                    trait_id: once.clone(),
                    associated: output.clone(),
                    arguments: Box::from([
                        model::Argument::Type(parameter.clone()),
                        model::Argument::Type(arguments),
                    ]),
                },
                ty: pointer.output().clone(),
            });
            inputs.push(parameter);
        }
        let rewritten = model::Signature::new(
            signature.safety(),
            signature.const_function(),
            signature.async_function(),
            signature.abi().into(),
            signature.variadic(),
            signature.binders().cloned().collect(),
            signature.receiver().clone(),
            inputs.into_boxed_slice(),
            signature.output().clone(),
        )
        .with_guard(signature.guard().cloned());
        Some((
            rewritten,
            model::Generics::new(parameters.into_boxed_slice(), predicates.into_boxed_slice()),
        ))
    }

    /// `core`'s `Fn` and `FnOnce` and `FnOnce::Output`, where the graph
    /// holds them.
    fn callable_traits(&self) -> Option<CallableTraits> {
        self.callable_traits
            .get_or_init(|| self.read_callable_traits())
            .clone()
    }

    fn read_callable_traits(&self) -> Option<CallableTraits> {
        let mut callable = None;
        let mut once = None;
        for definition in self.graph.definitions() {
            let id = definition.id();
            if self.is_core_trait(id, "Fn") {
                callable = Some(id.clone());
            } else if self.is_core_trait(id, "FnOnce") {
                once = Some(id.clone());
            }
        }
        let once = once?;
        let output = self.graph.definitions().find(|definition| {
            let path = definition.display_path().collect::<Vec<_>>();
            path.last() == Some(&"Output")
                && (matches!(definition.owner(), model::Owner::Definition(owner) if *owner == once)
                    || path.iter().rev().nth(1) == Some(&"FnOnce"))
        })?;
        Some((callable?, once, output.id().clone()))
    }

    /// The item, and where a parameter of a crate's trait over `Fn` is one
    /// the crate also implements for a type of its own, as iced's
    /// `.theme(impl ThemeFn<State, Theme>)` is for `Theme`, the item read
    /// with that bound kept beside it: a script function fills the
    /// parameter as the callable the trait's blanket impl takes, and a
    /// value of a type that implements the trait fills it as itself.
    fn function_with(
        &self,
        signature: &model::Signature,
        generics: &model::Generics,
        opens_copies: bool,
        borrows_receiver: bool,
    ) -> Result<representability::Representability, Failure> {
        use representability::Representability;
        let classified =
            self.function_form(signature, generics, opens_copies, borrows_receiver, false)?;
        let Representability::FunctionItem(item) = classified else {
            return Ok(classified);
        };
        // Only an input the parameter is makes another form: a method of the
        // opaque program `iced::application(..)` gives is bounded by its
        // `UpdateFn` and takes no update.
        let valued = self.blanket_callables(generics, false).1;
        if !signature.inputs().any(
            |input| matches!(input, model::Type::Parameter { index, .. } if valued.contains(index)),
        ) {
            return Ok(Representability::FunctionItem(item));
        }
        Ok(Representability::FunctionItem(
            match self.function_form(signature, generics, opens_copies, borrows_receiver, true) {
                Ok(Representability::FunctionItem(values)) => item.with_values(values),
                _ => item,
            },
        ))
    }

    fn function_form(
        &self,
        signature: &model::Signature,
        generics: &model::Generics,
        opens_copies: bool,
        borrows_receiver: bool,
        values: bool,
    ) -> Result<representability::Representability, Failure> {
        use representability::{callable, function, method, ownership, Representability};
        let rebound;
        let signature = if receiver_binder_is_exact(signature)
            || late_binders_lend_strings(signature)
            || late_binders_lend_inputs(signature)
            || self.async_lends_inputs(signature)
            || opens_copies
        {
            signature
        } else if let Some(early) = output_binders_early(signature, generics)
            && (receiver_binder_is_exact(&early) || late_binders_lend_inputs(&early))
        {
            // A lifetime of the function's own that its output names, as
            // iced's `markdown::view<'a>(items: &'a [Item], ..) -> impl
            // Widget + 'a`, is read as the early-bound lifetime rustc treats
            // it as at a direct call.
            rebound = early;
            &rebound
        } else {
            return Err(representability::reject::Kind::HigherRankedBinder.into());
        };
        // An early-bound lifetime is `'static` only where no input borrows
        // at it (D558 tier 0). An associated function whose output borrows
        // its `&str` inputs, as `Parser::new(text: &'input str)`, opens a
        // native scope frame that owns copies of them and holds the output
        // in its region (D558 tier 1, slice 2). Any other borrow refuses.
        if matches!(
            signature.receiver(),
            model::Receiver::Reference {
                lifetime: model::Lifetime::Early { .. },
                ..
            }
        ) {
            return Err(representability::reject::Kind::LifetimeOrConstGeneric.into());
        }
        let mut lends = Vec::new();
        let scope = if borrows_receiver {
            // The frame's output keeps a `&'a T` it takes as long as it
            // keeps the receiver, as inboard's `wizard.view(catalog, theme)`
            // keeps `theme`.
            if let model::Receiver::Reference { lifetime, .. } = signature.receiver() {
                lends = signature
                    .inputs()
                    .enumerate()
                    .filter(|(_, input)| shares_at(input, lifetime) || text_at(input, lifetime))
                    .filter_map(|(index, _)| u32::try_from(index).ok())
                    .collect();
            }
            Some(representability::signature::Scope::Borrows)
        } else if opens_copies {
            Some(representability::signature::Scope::Opens)
        } else if signature.inputs().any(borrows_at_early_lifetime) {
            // So does a method that borrows its receiver for the call alone
            // and whose output borrows a `&F` the call fills with a `&str`
            // or a value made for it, as jiff's `strftime<'f, F: 'f +
            // ?Sized>(&self, format: &'f F) -> Display<'f>`.
            if receiver_lent_for_call(signature)
                && signature
                    .inputs()
                    .filter(|input| borrows_at_early_lifetime(input))
                    .all(|input| is_str_reference(input) || is_parameter_reference(input))
                && names_early_lifetime(signature.output())
            {
                Some(representability::signature::Scope::Opens)
            } else if self.lends_values(signature) {
                // A value whose type borrows a crate value its function
                // takes, as iced's `qr_code(&'a Data) -> QRCode<'a, Theme>`
                // and `markdown::view(&'a [Item], ..) -> impl Widget + 'a`:
                // the value keeps those inputs borrowed for as long as it
                // lives, as rustc's checker reads the signature. An opaque
                // one is held beside what it borrows, through a proxy of its
                // traits.
                if !self.writes_opaque(signature.output()) {
                    return Err(representability::reject::Kind::OpaqueBounds.into());
                }
                lends = signature
                    .inputs()
                    .enumerate()
                    .filter(|(_, input)| borrows_at_early_lifetime(input))
                    .filter_map(|(index, _)| u32::try_from(index).ok())
                    .collect();
                None
            } else if self.lends_shared(signature) {
                lends = signature
                    .inputs()
                    .enumerate()
                    .filter(|(_, input)| borrows_at_early_lifetime(input))
                    .filter_map(|(index, _)| u32::try_from(index).ok())
                    .collect();
                None
            } else if !names_early_lifetime(signature.output()) {
                // A lifetime the output doesn't name, as serde_json's
                // `from_str<'a, T: Deserialize<'a>>(s: &'a str) -> Result<T>`
                // has, where a bound alone names it: the borrow ends with
                // the call, as a late-bound one's does, and rustc proves
                // the bound at the type the call gives (#593).
                None
            } else {
                return Err(representability::reject::Kind::LifetimeOrConstGeneric.into());
            }
        } else {
            None
        };
        // A parameter bounded by a crate's trait over `Fn`, as iced's
        // `impl UpdateFn<State, Message>` is, takes the callable the trait's
        // blanket implementation takes (D594).
        let (blanket, valued) = self.blanket_callables(generics, values);
        let generics = blanket.as_ref().unwrap_or(generics);
        let converted = self.callable_conversion(generics);
        let generics = converted.as_ref().unwrap_or(generics);
        // `W::State` of a type parameter `W: StatefulWidget` is a synthetic
        // type parameter the call infers from `W`'s implementation (D558.5),
        // and so is `P::State` where a callable's bound names it, and
        // `I::Item` of a type parameter a program's iterator doesn't fill.
        let projected = self.projections(generics, signature);
        let rewritten = Self::projected_generics(generics, &projected);
        let generics = rewritten.as_ref().unwrap_or(generics);
        let FunctionParameters {
            mut parameters,
            callable: callable_class,
            binders,
            mut predicates,
            call,
            crossings,
        } = self.function_parameters(generics)?;
        // A callable an `impl Into<T>` converts from crosses as a closure
        // `T`'s `From` takes (D552.3), not as a slot.
        let crossings = if converted.is_some() {
            Vec::new()
        } else {
            crossings
        };
        // `F: Future` takes a script future as `rawr::future::Script`, and
        // `F::Output` is a synthetic type parameter the script infers (D527).
        let futures = self.future_parameters(generics, &mut parameters, callable_class.is_some());
        // `T: Clone` keeps `Clone`: a script type fills `T` as `Value`,
        // which implements it, so Rawr checks the script type (review 3).
        parameters = parameters
            .into_vec()
            .into_iter()
            .map(|parameter| {
                let bounds = self.derivable_bounds(generics, parameter.index());
                let static_str = self.static_str_parameter(generics, parameter.index());
                parameter.with_bounds(bounds).with_static_str(static_str)
            })
            .collect();
        if !futures.is_empty() {
            predicates = predicates
                .iter()
                .copied()
                .filter(|bound| !futures.iter().any(|(index, _)| *index == bound.parameter()))
                .collect();
        }
        Self::declare_projected(&projected, &mut parameters);
        let rewrite = |ty: &model::Type| {
            projected_parameters(
                &future_projections(ty, &futures, |trait_id| {
                    self.is_core_trait(trait_id, "Future")
                }),
                &projected,
            )
        };
        let mut inputs = Vec::new();
        self.retain_array::<representability::convert::Type>(signature.inputs().len())?;
        inputs
            .try_reserve_exact(signature.inputs().len())
            .map_err(|_| Error::Allocation)?;
        for input in signature.inputs() {
            inputs.push(match input {
                // An argument's `impl IntoIterator<Item = X>`, as iced's
                // `Column::with_children` takes: a `Vec<X>`, which Rawr's
                // array is, so the call's `X` is its elements' type.
                model::Type::Parameter { index, .. }
                    if let Some(item) = self.iterated_item(generics, *index) =>
                {
                    representability::convert::Type::Sequence {
                        element: Box::new(self.convert_value(item)?),
                        owned: true,
                        length: None,
                    }
                }
                input if futures.is_empty() => self.convert_input(&rewrite(input))?,
                input => futurize(self.convert_input(&rewrite(input))?, &futures),
            });
        }
        // A Rust future crosses as a host's future from a free function, or
        // from a method that takes `self` by value and so owns what the
        // future holds. An `async fn` method that borrows its receiver holds
        // the borrow across its awaits: its future holds a copy of a `Clone`
        // receiver or the receiver itself, lent from Rawr until the future
        // ends (D513.6).
        let rewritten = rewrite(signature.output());
        // An opaque `impl Trait` the function gives is a type of its own,
        // at the type parameters the caller fixes (`iced::widget::iced`).
        // A parameter a value fills where another form takes a callable is
        // left out as the callable is, so both forms give the one type.
        self.opaque_associated.borrow_mut().clear();
        *self.opaque_output.borrow_mut() = Some(
            call.iter()
                .map(|(parameter, ..)| *parameter)
                .chain(crossings.iter().map(|(parameter, ..)| *parameter))
                .chain(futures.iter().map(|(parameter, _)| *parameter))
                .chain(valued.iter().copied().filter(|_| values))
                .collect(),
        );
        let output = match signature.receiver() {
            // `-> !`: the call never gives a value. `!` is no other
            // position's type on stable Rust.
            _ if matches!(rewritten, model::Type::Never) => {
                Ok(representability::convert::Type::Never)
            }
            model::Receiver::None | model::Receiver::Value { .. } => {
                self.convert_output(&rewritten)
            }
            model::Receiver::Reference { .. } if signature.async_function() => {
                self.convert_output(&rewritten)
            }
            // A `&[T]` the method lends from its receiver, typed as the
            // slice it is. Through `&self`, as iced's `combo_box::State::
            // options(&self) -> &[T]`, the slice is lent in place for as
            // long as the receiver's borrow lasts, whatever `T` is (#687).
            // Through `&mut self`, and for `T` a scalar, as QName's
            // `as_ref(&self) -> &[u8]`, it is still a copy, so a slice
            // pattern matches it (D558 slice 2).
            model::Receiver::Reference {
                lifetime: lifetime @ model::Lifetime::Late { .. },
                mutability,
                ..
            } if let model::Type::Reference {
                lifetime: lent, ty, ..
            } = &rewritten
                && lent == lifetime
                && let model::Type::Slice(element) = &**ty =>
            {
                let lent_in_place = matches!(mutability, model::Mutability::Shared)
                    && signature.inputs().len() == 0;
                let converted = if lent_in_place {
                    self.convert_value(element).map(|element| {
                        representability::convert::Type::Sequence {
                            element: Box::new(element),
                            owned: false,
                            length: None,
                        }
                    })
                } else {
                    self.convert_value(&rewritten)
                };
                converted.map(|slice| representability::convert::Type::Borrowed {
                    mutable: false,
                    ty: Box::new(slice),
                })
            }
            _ => self.convert_value(&rewritten),
        };
        *self.opaque_output.borrow_mut() = None;
        let opaques = std::mem::take(&mut *self.opaque_associated.borrow_mut());
        let mut guard = None;
        let output = match output {
            Ok(output) => output,
            // A borrow guard a method gives, as `RefCell::borrow_mut` and
            // Freya's `State::write` do, is the type it dereferences to,
            // reached only inside each call while the guard lives.
            Err(error) => match self.guard_output(signature)? {
                Some((target, mode)) => {
                    guard = Some(mode);
                    target
                }
                // A `&str` the method lends, as url's `Url::scheme(&self)`
                // gives: script code sees the `&str` rustc sees (D568), and
                // rawr's borrow checker keeps the receiver borrowed while
                // the `&str` is used (D572). At run time the text crosses as
                // a copy made at the call, as Rawr's strings can't point into
                // native memory.
                None if is_str_reference(signature.output()) => {
                    representability::convert::Type::Borrowed {
                        mutable: false,
                        ty: Box::new(representability::convert::Type::Builtin(
                            representability::convert::Builtin::Str,
                        )),
                    }
                }
                // An output Rawr cannot hold whose type implements
                // `Display`, as `DelayedFormat<StrftimeItems<'a>>`, is its
                // text.
                None => self.display_output(signature.output()).ok_or(error)?,
            },
        };
        // A script future fills `F` as a type Rawr has no name for, so a
        // signature can name `F` only as the input that takes the future
        // and through `F::Output`. tokio's `LocalKey::scope` gives back a
        // `TaskLocalFuture<T, F>`, which no facade signature can declare.
        if futures.iter().any(|(future, _)| {
            inputs
                .iter()
                .chain([&output])
                .any(|ty| names_parameter(ty, *future))
        }) {
            return Err(representability::reject::Kind::ScriptFutureType.into());
        }
        // A closure's type the output names, as iced's `Widget::map(self, f:
        // F) -> Map<Self, F, Message>` does: Rust's `Box<dyn Fn(A..) -> B>`,
        // which the bridge boxes the script closure as, so the output is a
        // type the runner can write (T417). Only a shared `Fn` the call
        // makes, which no other input names.
        let (output, callable_class) = match (&call, &callable_class) {
            (
                Some((callable, call_inputs, returned)),
                Some(
                    crate::surface::callable::Class::Borrowed(_)
                    | crate::surface::callable::Class::Shared(_),
                ),
            ) if crossings.iter().all(|(crossing, ..)| crossing == callable)
                && names_parameter(&output, *callable)
                && inputs.iter().all(|input| {
                    *input == representability::convert::Type::Parameter(*callable)
                        || !names_parameter(input, *callable)
                }) =>
            {
                let converted = call_inputs
                    .iter()
                    .map(|input| self.convert_closure_input(input))
                    .collect::<Option<Vec<_>>>();
                match (converted, self.convert_value(returned).ok()) {
                    // The boxed closure lives as long as the value that
                    // holds it, as a `'static` one does.
                    (Some(converted), Some(returned)) => (
                        replace_parameter(
                            output,
                            *callable,
                            &representability::convert::Type::Nominal {
                                definition: crate::surface::identity::Definition::callable(),
                                arguments: converted.into_iter().chain([returned]).collect(),
                            },
                        ),
                        Some(crate::surface::callable::Class::Shared(
                            crate::surface::callable::shared::Proof::static_owned(
                                crate::surface::callable::shared::Scope::Epoch,
                            ),
                        )),
                    ),
                    _ => (output, callable_class),
                }
            }
            _ => (output, callable_class),
        };
        // A boxed closure is the closure Rawr makes, not a slot.
        let crossings = match &call {
            Some((callable, ..))
                if !names_parameter(&output, *callable)
                    && crossings.iter().all(|(crossing, ..)| crossing == callable)
                    && output_boxes(&output) =>
            {
                Vec::new()
            }
            _ => crossings,
        };
        // A script closure fills a callable parameter `B` as a type Rawr
        // has no name for, as it does a future parameter, so a signature
        // can name `B` only as the input that takes the closure. Freya's
        // `VirtualScrollView<D, B>::auto_hide_scrollbars(self, bool) -> Self`
        // names it in its receiver and output.
        // A closure Rawr makes may also come as an `Option` of one, as
        // iced's `on_toggle_maybe(Option<F>)` takes it.
        let made = call.as_ref().map(|(callable, ..)| *callable);
        for callable in made
            .iter()
            .copied()
            .chain(crossings.iter().map(|(callable, ..)| *callable))
        {
            let closure = representability::convert::Type::Parameter(callable);
            if inputs
                .iter()
                .filter(|input| {
                    **input != closure
                        && !(made == Some(callable)
                            && matches!(input, representability::convert::Type::Option(inner)
                                if **inner == closure))
                })
                .chain([&output])
                .any(|ty| names_parameter(ty, callable))
            {
                return Err(representability::reject::Kind::CallableProtocol.into());
            }
        }
        // A future parameter whose `Output` a bound fixes, as iced's
        // `Task::future(impl Future<Output = T>)` does: the future's output
        // is that type wherever the signature names it, so the call's `T` is
        // what the script future gives, as rustc reads it (D683.5).
        let mut output = output;
        for (synthetic, fixed) in self.future_outputs(generics, &futures, rewrite) {
            inputs = inputs
                .into_iter()
                .map(|ty| replace_parameter(ty, synthetic, &fixed))
                .collect();
            output = replace_parameter(output, synthetic, &fixed);
        }
        let mut relations = self.relations(generics)?.into_vec();
        relations.extend(self.projection_relations(&projected)?);
        let relations = relations.into_boxed_slice();
        let signature_plan = representability::signature::Signature::new(
            parameters,
            inputs.into_boxed_slice(),
            output,
        )
        .with_relations(relations)
        .with_guard(guard)
        .with_scope(scope)
        .with_lends(lends.into_boxed_slice())
        .with_statics(static_inputs(signature))
        .with_opaques(opaques.into_boxed_slice());
        // A receiver of an impl that fixes its type's arguments, as
        // `impl<'a> Reader<&'a [u8]>`: the instance the bridge reaches.
        let signature_plan = match signature.receiver() {
            model::Receiver::Value { ty } | model::Receiver::Reference { ty, .. }
                if matches!(ty, model::Type::Definition { arguments, .. }
                    if arguments.iter().any(|argument| matches!(argument,
                        model::Argument::Type(argument)
                            if !matches!(argument, model::Type::Parameter { .. })))) =>
            {
                signature_plan.with_instance(self.convert(ty).ok())
            }
            _ => signature_plan,
        };
        let receiver = match signature.receiver() {
            model::Receiver::None => None,
            model::Receiver::Value { .. } => Some(ownership::Receiver::Owned),
            model::Receiver::Reference { mutability, .. } => Some(match mutability {
                model::Mutability::Shared => ownership::Receiver::Shared,
                model::Mutability::Mutable => ownership::Receiver::Mutable,
            }),
            model::Receiver::Explicit { .. } => {
                return Err(representability::reject::Kind::ExplicitReceiver.into())
            }
        };
        if !signature_plan.parameters().is_empty() {
            // A callable's inputs and output it can name; one it can't is
            // left unnamed, and Rawr does not call the item.
            let call = call.and_then(|(parameter, inputs, returned)| {
                let inputs = inputs
                    .iter()
                    .map(|input| self.convert_closure_input(input))
                    .collect::<Option<Box<[_]>>>()?;
                let returned = self.convert_value(&returned).ok()?;
                Some(callable::Call::new(parameter, inputs, returned))
            });
            // A closure input that is an associated type of a type
            // parameter, as `Task::sip`'s `S::Item`, has no type until the
            // call fixes the parameter.
            let projected_input = crossings.iter().any(|(_, inputs, ..)| {
                inputs.iter().any(|input| {
                    matches!(input, model::Type::Projection { .. })
                        && self.convert_crossing_input(input).is_none()
                })
            });
            // What each callable parameter takes and gives when a script
            // function fills it by a slot (D594); all or none.
            let crossings = crossings
                .iter()
                .map(|(parameter, inputs, returned, borrows)| {
                    let inputs = inputs
                        .iter()
                        .map(|input| self.convert_crossing_input(input))
                        .collect::<Option<Box<[_]>>>()?;
                    // The output's own lifetimes are the slot's, which the
                    // runner writes; the type is converted without them.
                    let returned = self.convert_value(&late_as_static(returned)).ok()?;
                    Some(callable::Call::new(*parameter, inputs, returned).borrowing(*borrows))
                })
                .collect::<Option<Box<[_]>>>()
                .unwrap_or_default();
            // An argument's `impl Fn(..)` has no name a signature can
            // declare, so only an item whose closure Rawr can make keeps it.
            if call.is_none()
                && crossings.is_empty()
                && signature_plan
                    .parameters()
                    .iter()
                    .any(|parameter| parameter.name().starts_with("impl "))
            {
                return Err(if projected_input {
                    representability::reject::Kind::ProjectedClosureInput
                } else {
                    representability::reject::Kind::CallableProtocol
                }
                .into());
            }
            return Ok(Representability::FunctionItem(
                callable::Item::new(
                    signature_plan,
                    receiver,
                    callable_class,
                    binders,
                    predicates,
                )
                .with_call(call)
                .with_crossings(crossings),
            ));
        }
        Ok(match receiver {
            Some(receiver) => Representability::Method(method::Plan::new(receiver, signature_plan)),
            None => Representability::Function(function::Plan::new(signature_plan)),
        })
    }

    /// A shared borrow of `target`, a slice of scalars or `str`, that
    /// `borrowed_argument` admits.
    fn convert_borrowed(
        &self,
        target: &model::Type,
    ) -> Result<representability::convert::Type, Failure> {
        use representability::convert::{Builtin, Type};
        Ok(Type::Borrowed {
            mutable: false,
            ty: Box::new(match target {
                model::Type::Slice(element) => Type::Sequence {
                    element: Box::new(self.convert(element)?),
                    owned: false,
                    length: None,
                },
                _ => Type::Builtin(Builtin::Str),
            }),
        })
    }

    /// A struct's public field: as `convert`, and a shared slice of
    /// scalars or a `&str` that borrows for the struct's own lifetime, as
    /// iced's `LineDash<'a>` has `segments: &'a [f32]`. A value Rawr builds
    /// keeps what it lends for as long as the runner lives, as a literal's
    /// is.
    ///
    /// A shared slice of other values that borrow nothing, as a page's
    /// `children: &'static [Block]`, borrows them the same way, and an
    /// `Option` of a `&str` or a slice of scalars, as `description:
    /// Option<&'static str>`, is Rawr's `Option` of it (#1283).
    fn convert_field(&self, ty: &model::Type) -> Result<representability::convert::Type, Failure> {
        use representability::convert::Type;
        if let Some(target) = borrowed_argument(ty) {
            return self.convert_borrowed(target);
        }
        if let model::Type::Reference {
            lifetime: model::Lifetime::Early { .. } | model::Lifetime::Static,
            mutability: model::Mutability::Shared,
            ty: target,
        } = ty
            && let model::Type::Slice(element) = &**target
            && !names_early_lifetime(element)
            && !names_late_lifetime(element)
        {
            return Ok(Type::Borrowed {
                mutable: false,
                ty: Box::new(Type::Sequence {
                    element: Box::new(self.convert(element)?),
                    owned: false,
                    length: None,
                }),
            });
        }
        if let model::Type::Definition { id, arguments } = ty
            && let Some(element) = self
                .graph
                .definition_index(id)
                .and_then(|index| self.graph.definitions().nth(index))
                .and_then(|definition| option_element(definition, arguments))
            && let Some(target) = borrowed_argument(element)
        {
            return Ok(Type::Option(Box::new(self.convert_borrowed(target)?)));
        }
        self.convert_held(ty)
    }

    /// A field's type as a struct or variant holds it: as `convert`, and an
    /// array of a known length, as iced_graphics's `SolidVertex2D` has
    /// `position: [f32; 2]`, which Rawr's array of that length fills and
    /// reads, as a `From<[u16; 2]>` case takes it.
    fn convert_held(&self, ty: &model::Type) -> Result<representability::convert::Type, Failure> {
        match ty {
            model::Type::Array { element, length } => {
                let length =
                    array_length(length).ok_or(representability::reject::Kind::ArrayType)?;
                Ok(representability::convert::Type::Sequence {
                    element: Box::new(self.convert_value(element)?),
                    owned: true,
                    length: Some(length),
                })
            }
            ty => self.convert(ty),
        }
    }

    /// An array an item takes: as `convert_held`, and `[T; N]` of a const
    /// parameter `N`, as `with_children<const N: usize>(children: [Slot;
    /// N])` takes, whose length the call's argument gives (#1330).
    fn convert_array(&self, ty: &model::Type) -> Result<representability::convert::Type, Failure> {
        match ty {
            model::Type::Array {
                element,
                length: model::Const::Parameter { index, .. },
            } => Ok(representability::convert::Type::ConstArray {
                element: Box::new(self.convert_value(element)?),
                parameter: *index,
            }),
            ty => self.convert_held(ty),
        }
    }

    fn convert(&self, ty: &model::Type) -> Result<representability::convert::Type, Failure> {
        use representability::convert::{Builtin, Type};
        Ok(match ty {
            model::Type::Tuple(members) if members.is_empty() => Type::Unit,
            // A type only a crate's alias names is the alias's own type.
            model::Type::Definition { .. } if let Some(alias) = self.instance_alias(ty) => {
                Type::Nominal {
                    definition: crate::surface::identity::Definition::of(alias.id()),
                    arguments: Box::default(),
                }
            }
            model::Type::Primitive(name) => Type::Builtin(
                Builtin::primitive(name)
                    .ok_or(representability::reject::Kind::UnsupportedPrimitive)?,
            ),
            model::Type::Definition { id, arguments } => {
                let index = self
                    .graph
                    .definition_index(id)
                    .ok_or(representability::reject::Kind::UnresolvedDefinition)?;
                let definition = self
                    .graph
                    .definitions()
                    .nth(index)
                    .ok_or(representability::reject::Kind::UnresolvedDefinition)?;
                definition_is_public(definition)?;
                match std_binding::of(definition) {
                    Some(Std::String) => Type::Builtin(Builtin::String),
                    // Only an input (`convert_input`): Rawr holds no
                    // `fmt::Arguments` a crate gives.
                    Some(Std::Arguments) => {
                        return Err(representability::reject::Kind::StdNotRun(
                            std_binding::Unrun::Other,
                        )
                        .into())
                    }
                    // `PhantomData<T>` holds nothing, and Rawr's is the unit
                    // type, as a script struct's is: palette's `Rgb` has
                    // `standard: PhantomData<S>`.
                    Some(Std::PhantomData) => Type::Unit,
                    Some(Std::Duration) => Type::Nominal {
                        definition: crate::surface::identity::Definition::duration(),
                        arguments: Box::default(),
                    },
                    Some(Std::Instant) => Type::Nominal {
                        definition: crate::surface::identity::Definition::instant(),
                        arguments: Box::default(),
                    },
                    Some(Std::Held { path, .. } | Std::Lent { owned: path }) => Type::Nominal {
                        definition: crate::surface::identity::Definition::std(path),
                        arguments: Box::default(),
                    },
                    Some(Std::RangeInclusive) => {
                        let element = std_binding::type_argument(arguments, 0)
                            .ok_or(representability::reject::Kind::UnsupportedGenericArgument)?;
                        // A range of a concrete integer: Rawr's
                        // `RangeInclusive<i64>`, converted to the integer
                        // type where the call takes it.
                        if matches!(element, model::Type::Primitive(name) if matches!(
                            name.as_ref(),
                            "i8" | "i16" | "i32" | "i64" | "isize" | "u8" | "u16" | "u32"
                                | "u64" | "usize"
                        )) {
                            Type::Builtin(Builtin::InclusiveRange)
                        } else {
                            // A range of a type parameter or of a float, as
                            // a crate's `slider(range: RangeInclusive<T>,
                            // value: T, ..)` takes: Rawr's integer or float
                            // range as `T` is an integer or a float. A
                            // range of a crate's type, as iced's
                            // `Radians::RANGE` is, holds its two bounds.
                            let element = self.convert(element)?;
                            let crate_type = matches!(
                                &element,
                                Type::Nominal { definition, .. }
                                    if !crate::surface::identity::Definition::is_projected(
                                        definition.as_bytes(),
                                    )
                            );
                            if !crate_type
                                && !matches!(
                                    element,
                                    Type::Builtin(Builtin::F32 | Builtin::F64) | Type::Parameter(_)
                                )
                            {
                                return Err(
                                    representability::reject::Kind::UnsupportedGenericArgument
                                        .into(),
                                );
                            }
                            Type::Nominal {
                                definition: crate::surface::identity::Definition::range_inclusive(),
                                arguments: Box::new([element]),
                            }
                        }
                    }
                    Some(Std::Range) => {
                        // A range value, as rand's `random_range(0.5..1.0)`
                        // or a crate's `fn take(r: Range<usize>)` takes, or
                        // the impl parameter a relation case binds: Rawr's
                        // `Range<f64>` of a float, its `Range<i64>` of an
                        // integer or a char (#707).
                        let element =
                            self.convert(std_binding::type_argument(arguments, 0).ok_or(
                                representability::reject::Kind::UnsupportedGenericArgument,
                            )?)?;
                        if !matches!(
                            element,
                            Type::Builtin(
                                Builtin::F32
                                    | Builtin::F64
                                    | Builtin::Char
                                    | Builtin::I8
                                    | Builtin::I16
                                    | Builtin::I32
                                    | Builtin::I64
                                    | Builtin::Isize
                                    | Builtin::U8
                                    | Builtin::U16
                                    | Builtin::U32
                                    | Builtin::U64
                                    | Builtin::Usize
                            ) | Type::Parameter(_)
                        ) {
                            return Err(
                                representability::reject::Kind::UnsupportedGenericArgument.into()
                            );
                        }
                        Type::Range(Box::new(element))
                    }
                    // alloc's `Cow<'a, [T]>` and `Cow<'a, str>`, as
                    // quick-xml's `Attribute::value` and `decode()` give:
                    // what they hold, copied out as Rawr's array or string,
                    // which no longer borrows (D558 slice 2).
                    Some(Std::Cow) => match std_binding::type_argument(arguments, 0) {
                        Some(model::Type::Slice(element)) => Type::Sequence {
                            element: Box::new(self.convert(element)?),
                            owned: false,
                            length: None,
                        },
                        Some(model::Type::Primitive(name)) if &**name == "str" => {
                            Type::Builtin(Builtin::Text)
                        }
                        _ => {
                            return Err(
                                representability::reject::Kind::UnsupportedGenericArgument.into()
                            )
                        }
                    },
                    Some(Std::Option) => {
                        Type::Option(Box::new(self.convert(
                            std_binding::type_argument(arguments, 0).ok_or(
                                representability::reject::Kind::UnsupportedGenericArgument,
                            )?,
                        )?))
                    }
                    Some(Std::Result) => {
                        let (Some(ok), Some(err)) = (
                            std_binding::type_argument(arguments, 0),
                            std_binding::type_argument(arguments, 1),
                        ) else {
                            return Err(
                                representability::reject::Kind::UnsupportedGenericArgument.into()
                            );
                        };
                        Type::Result(Box::new(self.convert(ok)?), Box::new(self.convert(err)?))
                    }
                    // A `Vec<T>`, as iced's `font::list()` gives inside a
                    // `Task<Result<Vec<Family>, Error>>`: Rawr's array,
                    // which generated code names as the `Vec` it is.
                    Some(Std::Vec) => Type::Sequence {
                        element: Box::new(self.convert(
                            std_binding::type_argument(arguments, 0).ok_or(
                                representability::reject::Kind::UnsupportedGenericArgument,
                            )?,
                        )?),
                        owned: true,
                        length: None,
                    },
                    // A `Box<T>`: rawr's `Box`, which holds its value as
                    // itself, boxed where the crate takes it.
                    Some(Std::Box) => Type::Nominal {
                        definition: crate::surface::identity::Definition::boxed(),
                        arguments: Box::new([self.convert(
                            std_binding::type_argument(arguments, 0).ok_or(
                                representability::reject::Kind::UnsupportedGenericArgument,
                            )?,
                        )?]),
                    },
                    // An `Rc<T>` or `Arc<T>` of a crate's type: rawr's
                    // counted pointer, whose value is the crate's own
                    // pointer, so passing it copies nothing (#775). Of any
                    // other `T` rawr's pointer holds the value itself.
                    Some(Std::Counted { arc }) => {
                        let unrun = || {
                            representability::reject::Kind::StdNotRun(if arc {
                                std_binding::Unrun::Arc
                            } else {
                                std_binding::Unrun::Rc
                            })
                        };
                        let element = self
                            .convert(std_binding::type_argument(arguments, 0).ok_or_else(unrun)?)?;
                        match &element {
                            Type::Nominal { definition, .. }
                                if !crate::surface::identity::Definition::is_projected(
                                    definition.as_bytes(),
                                ) =>
                            {
                                Type::Nominal {
                                    definition: crate::surface::identity::Definition::counted(arc),
                                    arguments: Box::new([element]),
                                }
                            }
                            _ => return Err(unrun().into()),
                        }
                    }
                    // A map or set whose keys are rawr's strings, numbers,
                    // chars or bools: rawr's collection of its kind, which
                    // the crate's is made from or made into where it
                    // crosses (#777).
                    Some(Std::Keyed(name, unrun)) => {
                        let not_run = || representability::reject::Kind::StdNotRun(unrun);
                        let map = name.ends_with("Map");
                        let mut converted = Vec::new();
                        for index in 0..if map { 2 } else { 1 } {
                            converted.push(self.convert(
                                std_binding::type_argument(arguments, index).ok_or_else(not_run)?,
                            )?);
                        }
                        let ordered = matches!(
                            converted.first(),
                            Some(Type::Builtin(builtin))
                                if *builtin == Builtin::String
                                    || *builtin == Builtin::Char
                                    || *builtin == Builtin::Bool
                                    || *builtin == Builtin::I64
                                    || builtin.is_narrow_integer()
                        );
                        if !ordered {
                            return Err(not_run().into());
                        }
                        Type::Nominal {
                            definition: crate::surface::identity::Definition::keyed(name),
                            arguments: converted.into_boxed_slice(),
                        }
                    }
                    Some(Std::Unrun(unrun)) => {
                        return Err(representability::reject::Kind::StdNotRun(unrun).into())
                    }
                    // A `JoinHandle` crosses only as a function gives one
                    // (`convert_output`).
                    Some(Std::JoinHandle) => {
                        return Err(representability::reject::Kind::StdNotRun(
                            std_binding::Unrun::JoinHandle,
                        )
                        .into())
                    }
                    None => {
                        definition_is_publishable_nominal(definition)?;
                        let mut converted = Vec::new();
                        self.retain_array::<Type>(arguments.len())?;
                        converted
                            .try_reserve_exact(arguments.len())
                            .map_err(|_| Error::Allocation)?;
                        for argument in arguments {
                            let argument =
                                match argument {
                                    model::Argument::Type(argument) => argument,
                                    // An early-bound lifetime is instantiated at
                                    // `'static` (D558 tier 0).
                                    model::Argument::Lifetime(
                                        model::Lifetime::Static | model::Lifetime::Early { .. },
                                    ) => continue,
                                    // A scope frame's lifetime (D558 tier 1).
                                    model::Argument::Lifetime(model::Lifetime::Late { .. })
                                        if self.scope_lifetimes.get() =>
                                    {
                                        continue
                                    }
                                    _ => return Err(
                                        representability::reject::Kind::UnsupportedGenericArgument
                                            .into(),
                                    ),
                                };
                            // `Reader<&'a [u8]>`: a shared slice of scalars or
                            // a `&str` a type argument names, which a scope
                            // frame's value borrows (D558 tier 1), at the
                            // frame's own lifetime too, as iced's
                            // `Key::as_ref(&self) -> Key<&str>` gives.
                            let borrowed = borrowed_argument(argument).or_else(|| {
                                self.scope_lifetimes
                                    .get()
                                    .then(|| late_borrowed_argument(argument))
                                    .flatten()
                            });
                            converted.push(match borrowed {
                                Some(target) => self.convert_borrowed(target)?,
                                None => self.convert(argument)?,
                            });
                        }
                        Type::Nominal {
                            definition: crate::surface::identity::Definition::of(id),
                            arguments: converted.into_boxed_slice(),
                        }
                    }
                }
            }
            model::Type::Parameter { index, .. } => Type::Parameter(*index),
            // A trait method's `Self` where it is a member of an opaque type
            // (`opaque_implementations`).
            model::Type::SelfType(_) if self.opaque_self.borrow().is_some() => {
                let opaque = self.opaque_self.borrow().clone();
                let outer = self.opaque_output.replace(Some(Box::default()));
                let converted = opaque.as_ref().map(|opaque| self.convert(opaque));
                *self.opaque_output.borrow_mut() = outer;
                converted.ok_or(representability::reject::Kind::OpaqueType)??
            }
            model::Type::SelfType(id) => {
                if let Some(index) = self.self_implementation(id) {
                    let implementation = self
                        .graph
                        .implementations()
                        .nth(index)
                        .ok_or(Error::Cardinality)?;
                    self.convert(implementation.self_type())?
                } else {
                    let index = self
                        .graph
                        .definition_index(id)
                        .ok_or(representability::reject::Kind::UnresolvedDefinition)?;
                    let definition = self
                        .graph
                        .definitions()
                        .nth(index)
                        .ok_or(representability::reject::Kind::UnresolvedDefinition)?;
                    definition_is_public(definition)?;
                    definition_is_publishable_nominal(definition)?;
                    Type::Nominal {
                        definition: crate::surface::identity::Definition::of(id),
                        arguments: Box::default(),
                    }
                }
            }
            model::Type::Reference { .. } => {
                return Err(representability::reject::Kind::ReferenceType.into())
            }
            model::Type::RawPointer { .. } => {
                return Err(representability::reject::Kind::RawPointer.into())
            }
            model::Type::FunctionPointer(_) => {
                return Err(representability::reject::Kind::FunctionPointer.into())
            }
            model::Type::TraitObject { .. } => {
                return Err(representability::reject::Kind::TraitObject.into())
            }
            // An opaque type a function gives, as `fn iced<Message, Theme,
            // Renderer>(..) -> impl Widget<Message, Theme, Renderer>`: a
            // type of its own, named by its compiler definition, at the
            // type arguments it captures that a caller's types fix.
            model::Type::ImplTrait {
                id,
                bounds,
                captures,
            } => {
                // An opaque type its own conversion reaches again at the
                // same captures, as `impl Number` does where `Number:
                // Add<Output = Self>` binds `Output` to the opaque type
                // itself, is left as the projection that names it: its
                // arguments would hold the type itself (#1262). The same
                // definition at other captures is another type, converted
                // as any is, as `explain`'s output is where its `Self` is
                // `explain`'s output (#1270).
                let converting = (id.clone(), captures.clone());
                if self.opaque_converting.borrow().contains(&converting) {
                    return Err(representability::reject::Kind::Projection.into());
                }
                self.opaque_converting.borrow_mut().push(converting);
                let converted = self.convert_opaque(id, bounds, captures);
                self.opaque_converting.borrow_mut().pop();
                converted?
            }
            // `<T as Trait>::Name` of an impl of the trait for `T` that sets
            // `Name`, as `next`'s `Option<Self::Item>` names an iterator's
            // `Item` (D558 slice 2).
            // `Self::Item` of a trait's method where it is a member of an
            // opaque type (`opaque_implementations`): the type the opaque
            // type's bounds bind it to, as `impl Stocked<Item = String>`.
            model::Type::Projection {
                associated,
                arguments,
                ..
            } if matches!(
                arguments.first(),
                Some(model::Argument::Type(model::Type::SelfType(_)))
            ) && self.opaque_self.borrow().is_some() =>
            {
                let opaque = self.opaque_self.borrow().clone();
                let bound = match &opaque {
                    Some(model::Type::ImplTrait { bounds, .. }) => self
                        .bound_associated(bounds)
                        .into_iter()
                        .find(|(_, item, _)| item == associated)
                        .map(|(_, _, ty)| ty),
                    _ => None,
                };
                // One its bounds leave open is an opaque type of its own
                // (`opaque_projection`), as `Visitor::Out` of `V: Visitor`.
                let ty = match (bound, &opaque) {
                    (Some(ty), _) => ty,
                    (None, Some(opaque)) => self.opaque_projection(opaque, associated),
                    (None, None) => return Err(representability::reject::Kind::Projection.into()),
                };
                return self.convert_projected(&ty);
            }
            // `<S as Serializer>::Ok` of an opaque type `S`, as a type
            // parameter's bound names it (`parameter_members`).
            model::Type::Projection {
                associated,
                arguments,
                ..
            } if matches!(
                arguments.first(),
                Some(model::Argument::Type(model::Type::ImplTrait { .. }))
            ) =>
            {
                let Some(model::Argument::Type(opaque)) = arguments.first() else {
                    return Err(representability::reject::Kind::Projection.into());
                };
                return self.convert_projected(&self.opaque_projection(opaque, associated));
            }
            model::Type::Projection { .. } => match self.projected(ty) {
                Some(item) => return self.convert(item),
                None => return Err(representability::reject::Kind::Projection.into()),
            },
            model::Type::Tuple(_) => return Err(representability::reject::Kind::TupleType.into()),
            model::Type::Slice(_) => return Err(representability::reject::Kind::SliceType.into()),
            model::Type::Array { .. } => {
                return Err(representability::reject::Kind::ArrayType.into())
            }
            model::Type::Never => return Err(representability::reject::Kind::NeverType.into()),
        })
    }

    /// An opaque `impl Trait` type (`convert`): a type of its own, named by
    /// its compiler definition, at the captures a caller's types fix and
    /// each associated type its bounds bind.
    fn convert_opaque(
        &self,
        id: &api::identity::Definition,
        bounds: &[model::Predicate],
        captures: &[model::Argument],
    ) -> Result<representability::convert::Type, Failure> {
        use representability::convert::Type;
        // The bounds name the opaque type's own parameters, which
        // its captures fill: a trait's provided method reached as a
        // member of a type, as `Button::explain` of `Framed`,
        // captures the trait's parameters at that type's arguments.
        let bindings = captures
            .iter()
            .enumerate()
            .filter_map(|(index, capture)| match capture {
                model::Argument::Type(ty) => Some((u32::try_from(index).ok()?, ty.clone())),
                _ => None,
            })
            .collect::<BTreeMap<_, _>>();
        // Their `Self` is the opaque type at these captures, which an
        // associated type that is the type itself reaches again (#1262).
        let this = model::Type::SelfType(id.clone());
        let opaque = model::Type::ImplTrait {
            id: id.clone(),
            bounds: bounds.into(),
            captures: captures.into(),
        };
        let bounds = &bounds
            .iter()
            .map(|predicate| {
                mapped_predicate(predicate, &|ty| {
                    type_replaced(&substitute_bindings(ty, &bindings), &this, &opaque)
                })
            })
            .collect::<Vec<_>>();
        let excluded = self.opaque_output.borrow().clone();
        let Some(excluded) = excluded else {
            return Err(representability::reject::Kind::OpaqueType.into());
        };
        // The captures a script callable or future fills, by their
        // positions: where the function that gives the opaque type
        // converts its output, those its excluded parameters are,
        // and the same positions wherever else the type is named,
        // as a member of it names it as its `Self`.
        let known = self.opaque_excluded.borrow().get(id).cloned();
        let left_out = match known {
            Some(positions) => positions,
            None => {
                let positions = captures
                    .iter()
                    .enumerate()
                    .filter(|(_, capture)| {
                        matches!(capture, model::Argument::Type(model::Type::Parameter { index, .. })
                            if excluded.contains(index))
                    })
                    .filter_map(|(position, _)| u32::try_from(position).ok())
                    .collect::<Box<[u32]>>();
                if !positions.is_empty() {
                    self.opaque_excluded
                        .borrow_mut()
                        .insert(id.clone(), positions.clone());
                }
                positions
            }
        };
        let mut arguments = Vec::new();
        for (position, capture) in captures.iter().enumerate() {
            let model::Argument::Type(ty) = capture else {
                continue;
            };
            if u32::try_from(position).is_ok_and(|position| left_out.contains(&position)) {
                continue;
            }
            arguments.push(self.convert(ty)?);
        }
        // Each associated type its bounds bind, as `impl Program<
        // State = S>` binds `State`, is one more argument, so a
        // call that takes the opaque type's value reads `P::State`
        // from it, as rustc does. So is each one a bound's
        // supertraits bind, as `impl Straw<(), Progress, Error>`
        // binds `Future::Output` through `Sipper` and `Core`.
        let mut associated = Vec::new();
        for (trait_id, item, ty) in self.bound_associated(bounds) {
            let Some(name) = self.definition_name(&item) else {
                continue;
            };
            let identity = *crate::surface::identity::Definition::of(&trait_id).as_bytes();
            if associated.iter().any(
                |(known, _): &(representability::signature::Associated, u32)| {
                    known.trait_identity() == identity && known.name() == name
                },
            ) {
                continue;
            }
            // A value that is a tuple, as `timed(..)`'s `impl
            // Program<Message = (Message, Instant)>` binds, is one
            // as a call's value is.
            let Ok(value) = self.convert_value(&ty) else {
                continue;
            };
            let Ok(position) = u32::try_from(arguments.len()) else {
                continue;
            };
            self.retain_str(name)?;
            associated.push((
                representability::signature::Associated::new(identity, name.into()),
                position,
            ));
            // The type is a future, whatever bound makes it one, as
            // `impl Straw<..>` is through `Core`: its value is taken
            // where a future of its `Output` is, as rustc takes it
            // for iced's `Task::future(impl Future<Output = T>)`.
            if name == "Output" && self.is_core_trait(&trait_id, "Future") {
                associated.push((
                    representability::signature::Associated::new(
                        *crate::surface::identity::Definition::future().as_bytes(),
                        name.into(),
                    ),
                    position,
                ));
            }
            // So is an iterator, whose `Item` a `for` loop and the
            // iterator's adaptors take, as rustc reads them from
            // `impl Iterator<Item = T>`.
            if name == "Item" && self.is_core_trait(&trait_id, "Iterator") {
                associated.push((
                    representability::signature::Associated::new(
                        *crate::surface::identity::Definition::iterator().as_bytes(),
                        name.into(),
                    ),
                    position,
                ));
            }
            arguments.push(value);
        }
        // An `impl Fn(A..) -> B`, as iced's `Function::with` gives:
        // each input one more argument, marked with the output's,
        // so the value fills a callable parameter of that shape
        // as itself (`Definition::callable`).
        if let Some((inputs, output)) = self.opaque_call(bounds) {
            let callable = *crate::surface::identity::Definition::callable().as_bytes();
            let names = (0..inputs.len())
                .map(|index| format!("Input{index}"))
                .chain(["Output".to_owned()]);
            for (name, ty) in names.zip(inputs.into_iter().chain([output])) {
                let position = u32::try_from(arguments.len()).map_err(|_| Error::Cardinality)?;
                self.retain_str(&name)?;
                associated.push((
                    representability::signature::Associated::new(callable, name.into()),
                    position,
                ));
                arguments.push(ty);
            }
        }
        let definition = crate::surface::identity::Definition::of(id);
        let traits = self.opaque_traits(bounds).unwrap_or_default();
        let forwards = (traits.len() == 1)
            .then(|| self.opaque_forwards(bounds))
            .flatten();
        if !associated.is_empty() || !traits.is_empty() {
            let mut opaques = self.opaque_associated.borrow_mut();
            if !opaques
                .iter()
                .any(|opaque| opaque.definition() == definition.as_bytes())
            {
                opaques.push(representability::signature::Opaque::new(
                    *definition.as_bytes(),
                    associated.into_boxed_slice(),
                    traits,
                    forwards,
                ));
            }
        }
        Ok(Type::Nominal {
            definition,
            arguments: arguments.into_boxed_slice(),
        })
    }

    /// The crate type aliases that are the only name of the type they
    /// name, by definition index: an exported alias with no parameters of
    /// a generic type at arguments some of which no import reaches, as
    /// iced's `pub type Renderer = fallback::Renderer<iced_wgpu::Renderer,
    /// iced_tiny_skia::Renderer>` is where the crate re-exports neither
    /// backend. Rust code can write that type only through the alias, so
    /// it is a type of its own, named by the alias, wherever a signature
    /// or a parameter's default names it. The test is per root crate: a
    /// graph whose other root re-exports both backends, as `iced_test`
    /// does, leaves `iced::Renderer` the only name `iced` has.
    fn instance_aliases(&self) -> &[usize] {
        self.instance_aliases
            .get_or_init(|| instance_aliases(self.graph))
    }

    /// The alias that is the only name of `ty` (`instance_aliases`).
    fn instance_alias(&self, ty: &model::Type) -> Option<&model::Definition> {
        self.instance_aliases()
            .iter()
            .filter_map(|index| self.graph.definitions().nth(*index))
            .find(|definition| {
                matches!(definition.kind(), model::Kind::TypeAlias { ty: target, .. } if target == ty)
            })
    }

    /// An instance alias's type: opaque, with no fields or constructor,
    /// and the core traits its target's definition implements.
    fn instance_alias_plan(&self, target: &model::Type) -> representability::structure::Plan {
        let index = match target {
            model::Type::Definition { id, .. } => self.graph.definition_index(id),
            _ => None,
        };
        let implements = |name| index.is_some_and(|index| self.implements_for_any(index, name));
        representability::structure::Plan::new(
            None,
            Box::default(),
            model::FieldForm::Unit,
            Box::default(),
            false,
            false,
            implements("Copy"),
            implements("Default"),
            implements("Debug"),
            implements("Clone"),
            implements("PartialEq"),
            implements("Eq"),
            implements("Hash"),
        )
        .with_order(implements("PartialOrd"), implements("Ord"))
        .with_error(implements("Error"))
    }

    /// A function's output. A Rust future, an `impl Future<Output = T>` or
    /// a type that implements `Future`, is iced's `Task<T>`: the bridge
    /// runs it natively and the facade's `Task` carries it.
    fn convert_output(&self, ty: &model::Type) -> Result<representability::convert::Type, Failure> {
        // A thread's `JoinHandle<T>`, as open's `that_in_background` gives:
        // rawr's thread handle, whose `join` makes the thread's `T` rawr's.
        // A `T` that holds a crate's value would need a call's arena, which
        // the join has none of (#1266).
        if let model::Type::Definition { id, arguments } = ty
            && let Some(definition) = self
                .graph
                .definition_index(id)
                .and_then(|index| self.graph.definitions().nth(index))
            && std_binding::of(definition) == Some(Std::JoinHandle)
        {
            let unrun =
                || representability::reject::Kind::StdNotRun(std_binding::Unrun::JoinHandle);
            let element =
                self.convert_value(std_binding::type_argument(arguments, 0).ok_or_else(unrun)?)?;
            if !crosses_by_value(&element) {
                return Err(unrun().into());
            }
            return Ok(representability::convert::Type::Nominal {
                definition: crate::surface::identity::Definition::join_handle(),
                arguments: Box::new([element]),
            });
        }
        let output = match ty {
            model::Type::ImplTrait { bounds, .. } => self.future_bound_output(bounds).cloned(),
            model::Type::Definition { id, arguments } if arguments.is_empty() => {
                self.future_impl_output(id).cloned()
            }
            // A generic type that implements `Future`, as tokio's
            // `JoinHandle<T>` is with `Output = Result<T, JoinError>`.
            model::Type::Definition { id, arguments } => {
                self.generic_future_impl_output(id, arguments)
            }
            _ => None,
        };
        let Some(output) = output else {
            return self.convert_value(ty);
        };
        // What the future gives is a value the bridge gives Rawr, as a
        // plain output is: changelog's `async fn list() -> Result<(Self,
        // Vec<Contribution>), Error>` gives a tuple holding a `Vec`.
        Ok(representability::convert::Type::Nominal {
            definition: crate::surface::identity::Definition::future(),
            arguments: Box::from([self.convert_value(&output)?]),
        })
    }

    /// The `Output` of a generic type's `impl<T> Future for Type<T>`, with
    /// the impl's parameters replaced by `arguments`.
    fn generic_future_impl_output(
        &self,
        id: &api::identity::Definition,
        arguments: &[model::Argument],
    ) -> Option<model::Type> {
        self.graph
            .implementations_for(id)
            .find_map(|implementation| {
                if implementation.polarity() != model::Polarity::Positive {
                    return None;
                }
                let model::Type::Definition {
                    id: actual,
                    arguments: declared,
                } = implementation.self_type()
                else {
                    return None;
                };
                if actual != id || declared.len() != arguments.len() {
                    return None;
                }
                let model::Trait::Trait { id: trait_id, .. } = implementation.trait_ref() else {
                    return None;
                };
                if !self.is_core_trait(trait_id, "Future") {
                    return None;
                }
                let mut substitutions = Vec::new();
                for (declared, argument) in declared.iter().zip(arguments) {
                    match (declared, argument) {
                        (
                            model::Argument::Type(model::Type::Parameter { owner, index }),
                            model::Argument::Type(argument),
                        ) => substitutions.push(((owner.clone(), *index), argument.clone())),
                        (model::Argument::Type(_), _) => return None,
                        _ => {}
                    }
                }
                let output = implementation.associated().find_map(|associated| {
                    let definition = self
                        .graph
                        .definition_index(associated)
                        .and_then(|index| self.graph.definitions().nth(index))?;
                    match definition.kind() {
                        model::Kind::AssociatedType {
                            default: model::Default::Present(ty),
                            ..
                        } if definition.display_path().last() == Some("Output") => Some(ty),
                        _ => None,
                    }
                })?;
                Some(substitute_parameters(output, &substitutions))
            })
    }

    /// An `async fn` whose own lifetimes only lend its inputs to its future,
    /// as fount's `async fn load(family: &str, catalog: Option<&Catalog>)`:
    /// the future borrows what its inputs borrow, whatever their types, as
    /// rustc sees it, and its output names none of the function's own
    /// lifetimes (D695.3). Each input is leased by what its value borrows
    /// (`convert_input`); one Rawr can't lend refuses there.
    fn async_lends_inputs(&self, signature: &model::Signature) -> bool {
        signature.async_function()
            && matches!(
                signature.receiver(),
                model::Receiver::None | model::Receiver::Value { .. }
            )
            && signature.inputs().any(names_late_lifetime)
            && matches!(signature.output(), model::Type::ImplTrait { bounds, .. }
                if self
                    .future_bound_output(bounds)
                    .is_some_and(|output| !names_late_lifetime(output)))
    }

    /// `T` of an `impl Future<Output = T>` bound list.
    fn future_bound_output<'a>(&self, bounds: &'a [model::Predicate]) -> Option<&'a model::Type> {
        let future = bounds.iter().any(|predicate| {
            matches!(predicate, model::Predicate::Bound {
                bound: model::Bound::Trait { trait_id, polarity: model::Polarity::Positive, .. },
                ..
            } if self.is_core_trait(trait_id, "Future"))
        });
        if !future {
            return None;
        }
        bounds.iter().find_map(|predicate| match predicate {
            model::Predicate::ProjectionEquality {
                projection: model::Type::Projection { trait_id, .. },
                ty,
            } if self.is_core_trait(trait_id, "Future") => Some(ty),
            _ => None,
        })
    }

    /// `T` of a type's own `impl Future<Output = T>`, such as tokio's `Sleep`.
    fn future_impl_output(&self, id: &api::identity::Definition) -> Option<&model::Type> {
        self.graph
            .implementations_for(id)
            .find_map(|implementation| {
                if implementation.polarity() != model::Polarity::Positive
                    || !exact_definition(implementation.self_type(), id)
                {
                    return None;
                }
                let model::Trait::Trait { id: trait_id, .. } = implementation.trait_ref() else {
                    return None;
                };
                if !self.is_core_trait(trait_id, "Future") {
                    return None;
                }
                implementation.associated().find_map(|associated| {
                    let definition = self
                        .graph
                        .definition_index(associated)
                        .and_then(|index| self.graph.definitions().nth(index))?;
                    match definition.kind() {
                        model::Kind::AssociatedType {
                            default: model::Default::Present(ty),
                            ..
                        } if definition.display_path().last() == Some("Output") => Some(ty),
                        _ => None,
                    }
                })
            })
    }

    /// The derivable std traits the bounds on type parameter `index` name.
    /// Whether a `&str` fills the type parameter at `index` only at
    /// `'static`: its bounds say `T: 'static`, one of its traits is
    /// implemented for `&'static str`, as iced's `Title<State>` is, or one
    /// names `'static` in its arguments, as iced's `Preset::new(name: impl
    /// Into<Cow<'static, str>>)` does, whose `From<&'a str> for Cow<'a,
    /// str>` asks the string for `'static` (#1050). A trait has at most one
    /// impl for a string reference, since two would overlap, so that impl
    /// is the one a `&str` argument selects.
    fn static_str_parameter(&self, generics: &model::Generics, index: u32) -> bool {
        let declared = generics
            .parameters()
            .filter(|parameter| parameter.index() == index)
            .flat_map(|parameter| match parameter.kind() {
                model::ParameterKind::Type { bounds, .. } => bounds.iter().collect::<Vec<_>>(),
                _ => Vec::new(),
            });
        let mut predicated = Vec::new();
        for predicate in generics.predicates() {
            match predicate {
                model::Predicate::Bound {
                    ty: model::Type::Parameter { index: bounded, .. },
                    bound,
                } if *bounded == index => predicated.push(bound),
                model::Predicate::TypeOutlives {
                    ty: model::Type::Parameter { index: bounded, .. },
                    lifetime: model::Lifetime::Static,
                } if *bounded == index => return true,
                _ => {}
            }
        }
        declared.chain(predicated).any(|bound| {
            let model::Bound::Trait {
                trait_id,
                arguments,
                polarity: model::Polarity::Positive,
                ..
            } = bound
            else {
                return false;
            };
            arguments.iter().any(argument_names_static)
                || self
                    .graph
                    .implementations_of(trait_id)
                    .any(|implementation| {
                        implementation.polarity() == model::Polarity::Positive
                            && matches!(
                                implementation.self_type(),
                                model::Type::Reference {
                                    lifetime: model::Lifetime::Static,
                                    ..
                                }
                            )
                            && is_str_reference(implementation.self_type())
                    })
        })
    }

    /// The derivable std traits the bounds of the parameter at `index` name,
    /// and `Iterator`, which a crate's iterator value or a program's
    /// iterator fills and an array doesn't (E0277).
    fn derivable_bounds(&self, generics: &model::Generics, index: u32) -> Box<[Box<str>]> {
        let declared = generics
            .parameters()
            .filter(|parameter| parameter.index() == index)
            .flat_map(|parameter| match parameter.kind() {
                model::ParameterKind::Type { bounds, .. } => bounds.iter().collect::<Vec<_>>(),
                _ => Vec::new(),
            });
        let predicated = generics
            .predicates()
            .filter_map(|predicate| match predicate {
                model::Predicate::Bound {
                    ty: model::Type::Parameter { index: bounded, .. },
                    bound,
                } if *bounded == index => Some(bound),
                _ => None,
            });
        let mut names: Vec<Box<str>> = Vec::new();
        for bound in declared.chain(predicated) {
            let model::Bound::Trait {
                trait_id,
                polarity: model::Polarity::Positive,
                ..
            } = bound
            else {
                continue;
            };
            let Some(name) = representability::signature::DERIVABLE
                .into_iter()
                .chain(["Iterator"])
                .find(|name| self.is_core_trait(trait_id, name))
            else {
                continue;
            };
            if !names.iter().any(|known| &**known == name) {
                names.push(name.into());
            }
        }
        names.into_boxed_slice()
    }

    /// The implementation whose type `Self` of `id` is: the implementation
    /// itself, or, for a trait's `Self` in a default method classified as a
    /// member of one implementation, that implementation.
    fn self_implementation(&self, id: &api::identity::Definition) -> Option<usize> {
        self.graph.implementation_index(id).or_else(|| {
            self.trait_self.get().filter(|_| {
                self.graph
                    .definition_index(id)
                    .and_then(|index| self.graph.definitions().nth(index))
                    .is_some_and(|definition| {
                        matches!(definition.kind(), model::Kind::Trait { .. })
                    })
            })
        })
    }

    /// The type an implementation of the projection's trait at exactly its
    /// arguments gives the projected associated type, as rustc normalizes
    /// `<T as Trait>::Name`: the crate's `impl Iterator for T` gives `Item`
    /// (D558 slice 2), and palette's blanket `Darken` applied to `Hsl`
    /// gives `Scalar` as `Hsl`'s `Lighten` impl sets it (#727). The driver
    /// keeps an iterator impl's `Item`, and an application's associated
    /// types, for this.
    fn projected<'g>(&'g self, ty: &model::Type) -> Option<&'g model::Type> {
        let model::Type::Projection {
            trait_id,
            associated,
            arguments,
        } = ty
        else {
            return None;
        };
        let name = self
            .graph
            .definition_index(associated)
            .and_then(|index| self.graph.definitions().nth(index))
            .and_then(|definition| definition.display_path().last())?;
        let model::Argument::Type(self_type) = arguments.first()? else {
            return None;
        };
        // A trait's `Self`, as a default method's `Self::Scalar` names it,
        // is the type of the implementation it is a member of.
        let self_type = match self_type {
            model::Type::SelfType(id) => self
                .graph
                .implementations()
                .nth(self.self_implementation(id)?)?
                .self_type(),
            self_type => self_type,
        };
        self.graph
            .implementations_of(trait_id)
            .find_map(|implementation| {
                let model::Trait::Trait {
                    id,
                    arguments: implemented,
                } = implementation.trait_ref()
                else {
                    return None;
                };
                if id != trait_id
                    || implementation.self_type() != self_type
                    || implemented.get(1..) != arguments.get(1..)
                {
                    return None;
                }
                self.associated_type(implementation, name)
            })
    }

    /// The type an implementation gives its associated type `name`.
    fn associated_type<'g>(
        &'g self,
        implementation: &'g model::Implementation,
        name: &str,
    ) -> Option<&'g model::Type> {
        implementation.associated().find_map(|associated| {
            let definition = self
                .graph
                .definition_index(associated)
                .and_then(|index| self.graph.definitions().nth(index))?;
            match definition.kind() {
                model::Kind::AssociatedType {
                    default: model::Default::Present(ty),
                    ..
                } if definition.display_path().last() == Some(name) => Some(ty),
                _ => None,
            }
        })
    }

    /// Whether `ty`, an implementation's self type, is the definition `id`
    /// with nothing but lifetimes filled in, or type arguments that are
    /// exactly their parameters' declared defaults.
    fn own_form(&self, ty: &model::Type, id: &api::identity::Definition) -> bool {
        let generics = self
            .graph
            .definition_index(id)
            .and_then(|index| self.graph.definitions().nth(index))
            .and_then(|definition| match definition.kind() {
                model::Kind::Struct { generics, .. }
                | model::Kind::Enum { generics, .. }
                | model::Kind::Union { generics, .. } => Some(generics),
                _ => None,
            });
        own_form(ty, id, generics)
    }

    /// `T` of an input `&mut Vec<T>` of a scalar `T`, which a call takes as
    /// a copy of Rawr's array and gives back (D558 slice 2).
    fn copied_vec<'t>(&self, ty: &'t model::Type) -> Option<&'t model::Type> {
        let model::Type::Reference {
            mutability: model::Mutability::Mutable,
            ty,
            ..
        } = ty
        else {
            return None;
        };
        let model::Type::Definition { id, arguments } = &**ty else {
            return None;
        };
        let definition = self
            .graph
            .definition_index(id)
            .and_then(|index| self.graph.definitions().nth(index))?;
        vec_element(definition, arguments).filter(|element| {
            matches!(element, model::Type::Primitive(_))
                // A `Vec<String>` crosses as a typed native place when
                // native code lends it (D651); a script's own array is
                // copied in and back out, as a scalar one is.
                || matches!(
                    self.convert(element),
                    Ok(representability::convert::Type::Builtin(
                        representability::convert::Builtin::String
                    ))
                )
        })
    }

    /// A method whose own lifetimes the output borrows only through inputs
    /// the call can copy, as quick-xml's `read_event_into<'b>(&mut self,
    /// buf: &'b mut Vec<u8>) -> Result<Event<'b>>`: it opens a scope frame
    /// that owns the copies and holds the output (D558 tier 1, slice 2).
    /// The receiver is borrowed for the call alone.
    fn late_binders_open_copies(&self, signature: &model::Signature) -> bool {
        let output = signature.output();
        if !names_late_lifetime(output) {
            return false;
        }
        if let model::Receiver::Reference {
            lifetime: receiver @ model::Lifetime::Late { .. },
            ..
        } = signature.receiver()
            && names_lifetime(output, receiver)
        {
            return false;
        }
        signature
            .inputs()
            .filter(|input| names_late_lifetime(input))
            .all(|input| {
                matches!(
                    input,
                    model::Type::Reference {
                        lifetime: model::Lifetime::Late { .. },
                        ..
                    }
                ) && (is_str_reference(input) || self.copied_vec(input).is_some())
            })
            && signature.inputs().any(|input| {
                matches!(
                    input,
                    model::Type::Reference {
                        lifetime: model::Lifetime::Late { .. },
                        ..
                    }
                )
            })
    }

    /// Whether `implementation` is one of several `AsRef` impls for one
    /// type.
    fn ambiguous_as_ref(&self, implementation: &model::Implementation) -> bool {
        let as_ref = |implementation: &model::Implementation| {
            matches!(implementation.trait_ref(), model::Trait::Trait { id, .. }
                if self.is_core_trait(id, "AsRef"))
        };
        if !as_ref(implementation) {
            return false;
        }
        let model::Type::Definition { id: target, .. } = implementation.self_type() else {
            return false;
        };
        self.graph
            .implementations()
            .filter(|other| {
                as_ref(other)
                    && matches!(other.self_type(), model::Type::Definition { id, .. } if id == target)
            })
            .count()
            > 1
    }

    /// Whether `definition`, a member of `implementation` of an instance of
    /// the type `target`, is the only member by its name that any
    /// implementation of the type declares.
    fn sole_instance_member(
        &self,
        implementation: &model::Implementation,
        definition: &model::Definition,
        target: &api::identity::Definition,
    ) -> bool {
        let instance =
            |ty: &model::Type| matches!(ty, model::Type::Definition { id, .. } if id == target);
        if !instance(implementation.self_type()) {
            return false;
        }
        let Some(name) = definition.display_path().last() else {
            return false;
        };
        self.graph
            .implementations()
            .filter(|other| instance(other.self_type()))
            .flat_map(model::Implementation::associated)
            .filter(|member| {
                self.graph
                    .definition_index(member)
                    .and_then(|index| self.graph.definitions().nth(index))
                    .and_then(|member| member.display_path().last())
                    == Some(name)
            })
            .count()
            == 1
    }

    /// `X` of an argument's `impl IntoIterator<Item = X>` at the parameter
    /// `index`, when that is its only bound besides `Sized`.
    fn iterated_item<'g>(
        &self,
        generics: &'g model::Generics,
        index: u32,
    ) -> Option<&'g model::Type> {
        let parameter = generics
            .parameters()
            .find(|parameter| parameter.index() == index)?;
        if !parameter.name().starts_with("impl ") {
            return None;
        }
        let mut item = None;
        for predicate in generics.predicates() {
            match predicate {
                model::Predicate::Bound {
                    ty: model::Type::Parameter { index: bounded, .. },
                    bound: model::Bound::Trait { trait_id, .. },
                } if *bounded == index => {
                    if !self.is_core_trait(trait_id, "IntoIterator")
                        && self.definition_name(trait_id) != Some("Sized")
                    {
                        return None;
                    }
                }
                model::Predicate::ProjectionEquality {
                    projection:
                        model::Type::Projection {
                            trait_id,
                            associated,
                            arguments,
                        },
                    ty,
                } if matches!(
                    arguments.first(),
                    Some(model::Argument::Type(model::Type::Parameter { index: bounded, .. }))
                        if *bounded == index
                ) =>
                {
                    if !self.is_core_trait(trait_id, "IntoIterator")
                        || self.definition_name(associated) != Some("Item")
                    {
                        return None;
                    }
                    item = Some(ty);
                }
                _ => {}
            }
        }
        item
    }

    /// Whether `ty` is a type parameter `generics` bounds by `Future`, or by
    /// a trait whose supertraits include it as rustc elaborates them, as a
    /// closure's `Fut` is in `F: FnOnce(Sender<P>) -> Fut, Fut:
    /// Future<Output = O>`, and sipper's `Sipper` is through `Core`.
    fn future_bounded(&self, generics: &model::Generics, ty: &model::Type) -> bool {
        /// Supertraits are read this deep, as [`Self::elaborate`] reads them.
        const DEPTH: usize = 8;
        // The positive trait bound `predicate` puts on what `bounded`
        // matches.
        fn bound(
            predicate: &model::Predicate,
            bounded: impl Fn(&model::Type) -> bool,
        ) -> Option<&api::identity::Definition> {
            match predicate {
                model::Predicate::Bound {
                    ty,
                    bound:
                        model::Bound::Trait {
                            trait_id,
                            polarity: model::Polarity::Positive,
                            ..
                        },
                } if bounded(ty) => Some(trait_id),
                _ => None,
            }
        }
        let model::Type::Parameter { index, .. } = ty else {
            return false;
        };
        let mut pending = generics
            .predicates()
            .filter_map(|predicate| {
                bound(predicate, |ty| {
                    matches!(ty, model::Type::Parameter { index: bounded, .. } if bounded == index)
                })
            })
            .map(|trait_id| (trait_id, 0))
            .collect::<Vec<_>>();
        let mut seen = Vec::new();
        while let Some((trait_id, depth)) = pending.pop() {
            if self.is_core_trait(trait_id, "Future") {
                return true;
            }
            if depth >= DEPTH || seen.contains(&trait_id) {
                continue;
            }
            seen.push(trait_id);
            let Some(model::Kind::Trait { supertraits, .. }) = self
                .graph
                .definition_index(trait_id)
                .and_then(|index| self.graph.definitions().nth(index))
                .map(model::Definition::kind)
            else {
                continue;
            };
            pending.extend(
                supertraits
                    .iter()
                    .filter_map(|predicate| {
                        bound(predicate, |ty| {
                            matches!(
                                ty,
                                model::Type::SelfType(_) | model::Type::Parameter { index: 0, .. }
                            )
                        })
                    })
                    .map(|trait_id| (trait_id, depth + 1)),
            );
        }
        false
    }

    fn is_core_trait(&self, id: &api::identity::Definition, name: &str) -> bool {
        id.unit().authority().sysroot().is_some()
            && id.unit().crate_name() == "core"
            && self
                .graph
                .definition_index(id)
                .and_then(|index| self.graph.definitions().nth(index))
                .is_some_and(|definition| definition.display_path().last() == Some(name))
    }

    /// The type a borrowing method's guard output dereferences to, and how
    /// it is reached, from rustc's evidence (`model::Guard`): `None` when
    /// the output is no guard or Rawr can't hold its target. A `DerefMut`
    /// guard is a place, as image's `get_pixel_mut(&mut self, x, y) -> &mut
    /// P` gives (#1318), whatever its other inputs: its method is called
    /// once, and the guard is kept with the receiver, in the arena (#1325)
    /// or in a scope frame's region (#1333).
    fn guard_output(
        &self,
        signature: &model::Signature,
    ) -> Result<
        Option<(
            representability::convert::Type,
            representability::signature::Guard,
        )>,
        Failure,
    > {
        // A reference the method lends from its receiver, as quick-xml's
        // `config_mut(&mut self) -> &mut Config`, is read and written as a
        // guard is (D558 slice 2, T012).
        let lent = match (signature.receiver(), signature.output()) {
            (
                model::Receiver::Reference {
                    lifetime: receiver @ model::Lifetime::Late { .. },
                    ..
                },
                model::Type::Reference {
                    lifetime,
                    mutability,
                    ty,
                },
            ) if lifetime == receiver => Some((&**ty, *mutability == model::Mutability::Mutable)),
            _ => None,
        };
        // An `Option<&T>` the method lends from its receiver, as pane_grid's
        // `State::get(&self, pane)` and `get_mut(&mut self, pane)` give:
        // `Some` of a copy or of a place, or `None` (T012).
        if signature.guard().is_none()
            && lent.is_none()
            && let model::Receiver::Reference {
                lifetime: receiver @ model::Lifetime::Late { .. },
                ..
            } = signature.receiver()
            && let model::Type::Definition { id, arguments } = signature.output()
            && let Some(model::Type::Reference {
                lifetime,
                mutability,
                ty,
            }) = self
                .graph
                .definition_index(id)
                .and_then(|index| self.graph.definitions().nth(index))
                .and_then(|definition| option_element(definition, arguments))
            && lifetime == receiver
        {
            use representability::convert::Type;
            let target = match self.convert_value(ty) {
                Ok(target) => target,
                Err(Failure::Reject(_)) => return Ok(None),
                Err(error) => return Err(error),
            };
            if !matches!(
                target,
                Type::Parameter(_) | Type::Nominal { .. } | Type::Builtin(_)
            ) || matches!(target, Type::Builtin(_) | Type::Nominal { .. } if !self.clones(&target))
            {
                return Ok(None);
            }
            return Ok(Some((
                Type::Option(Box::new(target)),
                if *mutability == model::Mutability::Mutable {
                    representability::signature::Guard::OptionMutable
                } else {
                    representability::signature::Guard::OptionShared
                },
            )));
        }
        let (target, mutable) = match (signature.guard(), lent) {
            (Some(evidence), _) => (evidence.target(), evidence.mutable()),
            (None, Some(lent)) => lent,
            (None, None) => return Ok(None),
        };
        if !matches!(signature.receiver(), model::Receiver::Reference { .. }) {
            return Ok(None);
        }
        use representability::convert::{Builtin, Type};
        let target = match self.convert_value(target) {
            Ok(target) => target,
            Err(Failure::Reject(_)) => return Ok(None),
            Err(error) => return Err(error),
        };
        // A copy of the target crosses as a value, so a `str` has none.
        if matches!(
            target,
            Type::Builtin(Builtin::Str | Builtin::Text)
                | Type::FunctionItem { .. }
                | Type::ProfileMessage { .. }
        ) {
            return Ok(None);
        }
        // A store crosses the other way, which a sequence or a tuple
        // doesn't yet.
        let stored = !matches!(target, Type::Sequence { .. } | Type::Tuple(_));
        let mode = if mutable && stored {
            representability::signature::Guard::Mutable
        } else {
            representability::signature::Guard::Shared
        };
        Ok(Some((target, mode)))
    }

    /// `Text` for an output of a crate type that implements `Display`.
    fn display_output(&self, ty: &model::Type) -> Option<representability::convert::Type> {
        let model::Type::Definition { id, .. } = ty else {
            return None;
        };
        let index = self.graph.definition_index(id)?;
        self.implements_for_any(index, "Display").then_some(
            representability::convert::Type::Builtin(representability::convert::Builtin::Text),
        )
    }

    fn convert_input(&self, ty: &model::Type) -> Result<representability::convert::Type, Failure> {
        // `&'static str`, as iced's `Font::new(name)` takes: Rawr's
        // string, pinned for as long as the runner lives.
        if is_str_reference(ty)
            && matches!(
                ty,
                model::Type::Reference {
                    lifetime: model::Lifetime::Static,
                    ..
                }
            )
        {
            return Ok(representability::convert::Type::Builtin(
                representability::convert::Builtin::StaticStr,
            ));
        }
        // `&str`: Rawr's string, lent to the call.
        if is_str_reference(ty) {
            return Ok(representability::convert::Type::Builtin(
                representability::convert::Builtin::Str,
            ));
        }
        // `fmt::Arguments`, as log's `Record` takes: Rawr's text of the
        // program's `format_args!`, formatted into the call.
        if let model::Type::Definition { id, .. } = ty
            && let Some(definition) = self
                .graph
                .definition_index(id)
                .and_then(|index| self.graph.definitions().nth(index))
            && std_binding::of(definition) == Some(Std::Arguments)
        {
            return Ok(representability::convert::Type::Builtin(
                representability::convert::Builtin::Arguments,
            ));
        }
        // A type that holds borrows, as fount's `load(family: &str, catalog:
        // Option<&Catalog>)` takes an `Option<&Catalog>`: each borrow in it
        // is lent as the borrow alone is, recursively, and `None` lends
        // nothing (#677). Nothing is special to `Option`: a `Result` lends
        // what its variant holds, as `Result<&Font, String>` does, and a
        // tuple each of its members. A member that borrows nothing moves.
        let definition = match ty {
            model::Type::Definition { id, arguments } => self
                .graph
                .definition_index(id)
                .and_then(|index| self.graph.definitions().nth(index))
                .map(|definition| (definition, arguments)),
            _ => None,
        };
        // So is a borrow at the signature's own lifetime, as inboard's
        // `nav_section<'a, ..>(heading: Option<&'a str>, ..) -> Element<'a,
        // ..>` takes (#1413).
        if let Some((definition, arguments)) = definition
            && let Some(element) = option_element(definition, arguments)
            && (names_late_lifetime(element) || matches!(element, model::Type::Reference { .. }))
        {
            return leased(representability::convert::Type::Option(Box::new(
                self.convert_input(element)?,
            )));
        }
        if let Some((definition, arguments)) = definition
            && let Some((ok, err)) = result_elements(definition, arguments)
            && (names_late_lifetime(ok) || names_late_lifetime(err))
        {
            return leased(representability::convert::Type::Result(
                Box::new(self.convert_input(ok)?),
                Box::new(self.convert_input(err)?),
            ));
        }
        if let model::Type::Tuple(members) = ty
            && members.iter().any(names_late_lifetime)
        {
            return leased(representability::convert::Type::Tuple(
                members
                    .iter()
                    .map(|member| self.convert_input(member))
                    .collect::<Result<_, _>>()?,
            ));
        }
        // An array, as `sum(&[i64; 3])` and `fill(&mut [u8; 4], u8)` take
        // and `Row::with_children([Slot; N])` moves: Rawr's array of the
        // length, lent or moved; a `&mut` one is copied in and back out, as
        // a `&mut Vec<T>` is (#1330).
        if let model::Type::Array { .. } = ty {
            return self.convert_array(ty);
        }
        if let model::Type::Reference {
            mutability,
            ty: target,
            ..
        } = ty
            && let model::Type::Array { .. } = &**target
        {
            return Ok(representability::convert::Type::Borrowed {
                mutable: *mutability == model::Mutability::Mutable,
                ty: Box::new(self.convert_array(target)?),
            });
        }
        // `&mut Vec<T>` of scalars, as quick-xml's `read_event_into` takes
        // its buffer: Rawr's array, copied in and back out (D558 slice 2).
        if let Some(element) = self.copied_vec(ty) {
            return Ok(representability::convert::Type::Borrowed {
                mutable: true,
                ty: Box::new(representability::convert::Type::Sequence {
                    element: Box::new(self.convert(element)?),
                    owned: true,
                    length: None,
                }),
            });
        }
        // `&mut String`, as pulldown-cmark's `push_html(&mut s, ..)` writes
        // into: Rawr's string, copied in and back out as `&mut Vec<T>` is.
        if let model::Type::Reference {
            mutability: model::Mutability::Mutable,
            ty: target,
            ..
        } = ty
            && let Ok(
                string @ representability::convert::Type::Builtin(
                    representability::convert::Builtin::String,
                ),
            ) = self.convert(target)
        {
            return Ok(representability::convert::Type::Borrowed {
                mutable: true,
                ty: Box::new(string),
            });
        }
        // `&[T]` or `&(A, B)`, as voronator's `from_tuple(&(f64, f64),
        // &(f64, f64), &[(f64, f64)])` and image's `save_buffer(path, &[u8],
        // ..)` take: a shared borrow in script code, as rustc sees it (D568),
        // which leaves the script's value in place. Rawr's arrays and tuples
        // don't hold Rust's layout, so the bridge makes the Rust value it
        // lends for the call.
        if let model::Type::Reference {
            mutability: model::Mutability::Shared,
            ty: target,
            ..
        } = ty
        {
            match target.as_ref() {
                // `&[&'static str]`, as serde's `deserialize_struct(name,
                // fields: &'static [&'static str], ..)` takes: Rawr's array
                // of strings, each pinned, as one slice the runner pins for
                // as long as it lives.
                model::Type::Slice(element)
                    if is_str_reference(element)
                        && matches!(
                            element.as_ref(),
                            model::Type::Reference {
                                lifetime: model::Lifetime::Static,
                                ..
                            }
                        ) =>
                {
                    return Ok(representability::convert::Type::Borrowed {
                        mutable: false,
                        ty: Box::new(representability::convert::Type::Sequence {
                            element: Box::new(representability::convert::Type::Builtin(
                                representability::convert::Builtin::StaticStr,
                            )),
                            owned: false,
                            length: None,
                        }),
                    });
                }
                model::Type::Slice(element) => {
                    return Ok(representability::convert::Type::Borrowed {
                        mutable: false,
                        ty: Box::new(representability::convert::Type::Sequence {
                            element: Box::new(self.convert_value(element)?),
                            owned: false,
                            length: None,
                        }),
                    });
                }
                model::Type::Tuple(members) if members.len() >= 2 => {
                    return Ok(representability::convert::Type::Borrowed {
                        mutable: false,
                        ty: Box::new(self.convert_value(target)?),
                    });
                }
                // `&String` or `&Vec<T>` of Rawr's own string or array, as
                // an `async fn shout(text: &String)` takes: the Rust value
                // made from it, lent as a `&[T]` is, since Rawr's string and
                // array don't hold Rust's layout.
                model::Type::Definition { .. }
                    if let Ok(
                        made @ (representability::convert::Type::Sequence { .. }
                        | representability::convert::Type::Builtin(
                            representability::convert::Builtin::String,
                        )),
                    ) = self.convert_value(target) =>
                {
                    // A `&Vec<T>` is lent as the slice it derefs to, which
                    // the made `Vec` is.
                    let made = match made {
                        representability::convert::Type::Sequence { element, .. } => {
                            representability::convert::Type::Sequence {
                                element,
                                owned: false,
                                length: None,
                            }
                        }
                        made => made,
                    };
                    return Ok(representability::convert::Type::Borrowed {
                        mutable: false,
                        ty: Box::new(made),
                    });
                }
                // `&f32` of a number, as `PartialOrd<f32>::lt(&self, &f32)`
                // and `PartialEq<f32>::eq` take the right-hand side of
                // clock's `rotation < 180.0` (T412): the number, lent.
                model::Type::Primitive(_) => {
                    if let Ok(
                        scalar @ representability::convert::Type::Builtin(
                            representability::convert::Builtin::Bool
                            | representability::convert::Builtin::Char
                            | representability::convert::Builtin::F32
                            | representability::convert::Builtin::F64
                            | representability::convert::Builtin::I8
                            | representability::convert::Builtin::I16
                            | representability::convert::Builtin::I32
                            | representability::convert::Builtin::I64
                            | representability::convert::Builtin::Isize
                            | representability::convert::Builtin::U8
                            | representability::convert::Builtin::U16
                            | representability::convert::Builtin::U32
                            | representability::convert::Builtin::U64
                            | representability::convert::Builtin::Usize,
                        ),
                    ) = self.convert(target)
                    {
                        return Ok(representability::convert::Type::Borrowed {
                            mutable: false,
                            ty: Box::new(scalar),
                        });
                    }
                }
                _ => {}
            }
        }
        // `&T` or `&mut T` of a value Rawr holds, leased from its slot for
        // the call (D558.5), as `render_stateful_widget`'s `&mut W::State`.
        if let model::Type::Reference { mutability, ty, .. } = ty
            && let converted @ (representability::convert::Type::Nominal { .. }
            | representability::convert::Type::Parameter(_)) = self.convert(ty)?
        {
            return Ok(representability::convert::Type::Borrowed {
                mutable: *mutability == model::Mutability::Mutable,
                ty: Box::new(converted),
            });
        }
        // A tuple by value, as `fn area(size: (u32, u32))` takes: Rawr's
        // tuple, converted as an output's is (#707).
        if let model::Type::Tuple(members) = ty
            && members.len() >= 2
        {
            return self.convert_value(ty);
        }
        let model::Type::FunctionPointer(signature) = ty else {
            return self.convert(ty);
        };
        if signature.safety() != model::Safety::Safe
            || signature.async_function()
            || signature.variadic()
            || signature.abi() != "\"Rust\""
            || signature.binders().len() != 0
            || !matches!(signature.receiver(), model::Receiver::None)
            || signature.inputs().len() != 1
        {
            return Err(representability::reject::Kind::FunctionPointer.into());
        }
        let input = signature
            .inputs()
            .next()
            .ok_or(representability::reject::Kind::FunctionPointer)?;
        Ok(representability::convert::Type::FunctionItem {
            input: Box::new(self.convert(input)?),
            output: Box::new(self.convert(signature.output())?),
        })
    }

    /// Whether the crate implements the `core` trait `name` for the
    /// definition with any arguments, as `impl<Tz> Debug for DateTime<Tz>`
    /// does. Bounds on the arguments are not read: `cargo check` holds the
    /// crate to them.
    fn implements_for_any(&self, definition: usize, name: &str) -> bool {
        let Some(definition) = self.graph.definitions().nth(definition) else {
            return false;
        };
        self.graph
            .implementations_for(definition.id())
            .any(|implementation| {
                if implementation.polarity() != model::Polarity::Positive {
                    return false;
                }
                let model::Trait::Trait { id, .. } = implementation.trait_ref() else {
                    return false;
                };
                let Some(trait_definition) = self
                    .graph
                    .definition_index(id)
                    .and_then(|index| self.graph.definitions().nth(index))
                else {
                    return false;
                };
                let core_trait = id.unit().authority().sysroot().is_some()
                    && id.unit().crate_name() == "core"
                    && trait_definition.display_path().last() == Some(name);
                core_trait && any_arguments(implementation.self_type(), definition.id())
            })
    }
}

/// Whether every definition `ty` names is the target of an import.
/// The struct or enum a generic alias names with each of its own type
/// parameters, in order, and nothing else, as `pub type Text<'a, Theme =
/// crate::Theme> = core::widget::Text<'a, Theme>;` does.
fn passing_target<'graph>(
    graph: &'graph api::Graph<impl api::Phase>,
    definition: &model::Definition,
) -> Option<&'graph model::Definition> {
    let model::Kind::TypeAlias { ty, generics } = definition.kind() else {
        return None;
    };
    let model::Type::Definition { id, arguments } = ty else {
        return None;
    };
    if generics.predicates().len() != 0 {
        return None;
    }
    let parameters = generics
        .parameters()
        .filter(|parameter| matches!(parameter.kind(), model::ParameterKind::Type { .. }))
        .collect::<Vec<_>>();
    let types = arguments
        .iter()
        .filter_map(|argument| match argument {
            model::Argument::Type(ty) => Some(ty),
            _ => None,
        })
        .collect::<Vec<_>>();
    if parameters.is_empty()
        || parameters.len() != types.len()
        || !parameters.iter().zip(&types).all(|(parameter, ty)| {
            matches!(ty, model::Type::Parameter { owner, index }
                if owner == parameter.owner() && *index == parameter.index())
        })
    {
        return None;
    }
    let target = graph.definitions().nth(graph.definition_index(id)?)?;
    matches!(
        target.kind(),
        model::Kind::Struct { .. } | model::Kind::Enum { .. }
    )
    .then_some(target)
}

/// The types no import names but through a crate's alias that passes its
/// own parameters on, as iced's `widget::Text<'a, Theme = crate::Theme>`
/// is the only name of `core::widget::Text<'a, Theme>` while `iced::
/// advanced` is off: each with the root and alias of an import that names
/// it. Rust code writes such a type through the alias, so the alias's path
/// declares the type, as the generated code spells it.
fn sole_passing_aliases(
    graph: &api::Graph<impl api::Phase>,
) -> Vec<(&str, &model::Definition, &model::Definition)> {
    let exported = exported_by_root(graph);
    graph
        .exports()
        .filter_map(|path| match path {
            ledger::Path::Imported(export) => Some(export),
            _ => None,
        })
        .filter_map(|export| {
            let alias = graph
                .definitions()
                .nth(graph.definition_index(export.target())?)?;
            let target = passing_target(graph, alias)?;
            let root = export.key().root_alias();
            (!exported.get(root).is_some_and(|exported| {
                exported.contains(&crate::surface::identity::Definition::of(target.id()))
            }))
            .then_some((root, alias, target))
        })
        .collect()
}

/// What the associated type `name` of `trait_id` is for the concrete
/// type `this`, read from the one implementation whose self type
/// `this` is an instance of, with the projections its value names read
/// the same way: `<fallback::Renderer<wgpu::Renderer,
/// tiny_skia::Renderer> as geometry::Renderer>::Geometry` is
/// `fallback::Geometry<wgpu::Geometry, tiny_skia::Geometry>`. `None`
/// when no implementation, or more than one, fits, or the trait takes
/// arguments besides its self type.
pub(crate) fn associated_at(
    graph: &api::Graph<impl api::Phase>,
    trait_id: &api::identity::Definition,
    name: &str,
    this: &model::Type,
    depth: usize,
) -> Option<model::Type> {
    /// Projections inside a value are read this deep.
    const DEPTH: usize = 4;
    if depth > DEPTH || names_type_parameter(this) {
        return None;
    }
    let mut found = None;
    for implementation in graph.implementations_of(trait_id) {
        let model::Trait::Trait { id, arguments } = implementation.trait_ref() else {
            continue;
        };
        if id != trait_id
            || implementation.polarity() != model::Polarity::Positive
            || arguments
                .iter()
                .skip(1)
                .any(|argument| !matches!(argument, model::Argument::Lifetime(_)))
        {
            continue;
        }
        let mut bindings = BTreeMap::new();
        if !bind_instance(implementation.self_type(), this, &mut bindings) {
            continue;
        }
        let value = implementation.associated().find_map(|associated| {
            let definition = graph
                .definition_index(associated)
                .and_then(|index| graph.definitions().nth(index))?;
            match definition.kind() {
                model::Kind::AssociatedType {
                    default: model::Default::Present(ty),
                    ..
                } if definition.display_path().last() == Some(name) => Some(ty),
                _ => None,
            }
        })?;
        if found.is_some() {
            return None;
        }
        let value = normalize_at(graph, &substitute_bindings(value, &bindings), depth + 1)?;
        // `this` binds each parameter the impl's self type names. One only
        // the impl's where clauses fix, as `E` of futures_util's `impl<St,
        // Si, Item, E> Future for Forward<St, Si, Item>` gives `Output =
        // Result<(), E>`, is still the impl's own parameter, which no
        // caller declares (#1295).
        if names_type_parameter(&value) {
            return None;
        }
        found = Some(value);
    }
    found
}

/// `ty` with each projection on a concrete type in it read by
/// [`Self::associated_at`]. `None` when one can't be read.
fn normalize_at(
    graph: &api::Graph<impl api::Phase>,
    ty: &model::Type,
    depth: usize,
) -> Option<model::Type> {
    let argument = |argument: &model::Argument| -> Option<model::Argument> {
        Some(match argument {
            model::Argument::Type(ty) => model::Argument::Type(normalize_at(graph, ty, depth)?),
            other => other.clone(),
        })
    };
    Some(match ty {
        model::Type::Projection {
            trait_id,
            associated,
            arguments,
        } => {
            let [model::Argument::Type(this), ..] = &arguments[..] else {
                return None;
            };
            let this = normalize_at(graph, this, depth)?;
            let name = definition_name(graph, associated)?;
            return associated_at(graph, trait_id, name, &this, depth);
        }
        model::Type::Definition { id, arguments } => model::Type::Definition {
            id: id.clone(),
            arguments: arguments.iter().map(argument).collect::<Option<_>>()?,
        },
        model::Type::Reference {
            lifetime,
            mutability,
            ty,
        } => model::Type::Reference {
            lifetime: lifetime.clone(),
            mutability: *mutability,
            ty: Box::new(normalize_at(graph, ty, depth)?),
        },
        model::Type::Tuple(members) => model::Type::Tuple(
            members
                .iter()
                .map(|member| normalize_at(graph, member, depth))
                .collect::<Option<_>>()?,
        ),
        model::Type::Slice(element) => {
            model::Type::Slice(Box::new(normalize_at(graph, element, depth)?))
        }
        model::Type::Array { element, length } => model::Type::Array {
            element: Box::new(normalize_at(graph, element, depth)?),
            length: length.clone(),
        },
        ty => ty.clone(),
    })
}

fn definition_name<'g>(
    graph: &'g api::Graph<impl api::Phase>,
    definition: &api::identity::Definition,
) -> Option<&'g str> {
    graph
        .definition_index(definition)
        .and_then(|index| graph.definitions().nth(index))
        .and_then(|definition| definition.display_path().last())
}

/// The concrete types whose associated types of `trait_id` a projection
/// bound reads: each concrete self type of the trait's impls, each type
/// only a crate's alias names (`instance_aliases`, given), and each generic
/// impl's self type at those concrete types, as iced's `impl<A, B>
/// image::Renderer for fallback::Renderer<A, B>` is at its backends.
pub(crate) fn projection_candidates(
    graph: &api::Graph<impl api::Phase>,
    trait_id: &api::identity::Definition,
    instances: &[usize],
) -> Vec<model::Type> {
    /// More instantiations of one generic impl than this are not read.
    const COMBINATIONS: usize = 64;
    let selves = graph
        .implementations_of(trait_id)
        .map(model::Implementation::self_type)
        .collect::<Vec<_>>();
    let mut concrete = selves
        .iter()
        .filter(|ty| !names_type_parameter(ty))
        .map(|ty| (*ty).clone())
        .collect::<Vec<_>>();
    let base = concrete.clone();
    concrete.extend(instances.iter().filter_map(|index| {
        match graph.definitions().nth(*index)?.kind() {
            model::Kind::TypeAlias { ty, .. } => Some(ty.clone()),
            _ => None,
        }
    }));
    for generic in selves.iter().filter(|ty| names_type_parameter(ty)) {
        let mut indices = BTreeSet::new();
        type_parameter_indices(generic, &mut indices);
        let indices = indices.into_iter().collect::<Vec<_>>();
        let Some(count) = u32::try_from(indices.len())
            .ok()
            .and_then(|length| base.len().checked_pow(length))
            .filter(|count| *count <= COMBINATIONS)
        else {
            continue;
        };
        for combination in 0..count {
            let mut rest = combination;
            let mut bindings = BTreeMap::new();
            for index in &indices {
                bindings.insert(*index, base[rest % base.len()].clone());
                rest /= base.len();
            }
            concrete.push(substitute_bindings(generic, &bindings));
        }
    }
    concrete
}

/// The indices of the type parameters `ty` names.
fn type_parameter_indices(ty: &model::Type, found: &mut BTreeSet<u32>) {
    match ty {
        model::Type::Parameter { index, .. } => {
            found.insert(*index);
        }
        model::Type::Definition { arguments, .. } => {
            for argument in arguments.iter() {
                if let model::Argument::Type(ty) = argument {
                    type_parameter_indices(ty, found);
                }
            }
        }
        model::Type::Tuple(members) => {
            for member in members.iter() {
                type_parameter_indices(member, found);
            }
        }
        model::Type::Reference { ty, .. }
        | model::Type::RawPointer { ty, .. }
        | model::Type::Slice(ty)
        | model::Type::Array { element: ty, .. } => type_parameter_indices(ty, found),
        _ => {}
    }
}

/// The crate type aliases that are the only name of the type they name, by
/// definition index (`Context::instance_aliases`).
pub(crate) fn instance_aliases(graph: &api::Graph<impl api::Phase>) -> Box<[usize]> {
    let exported = exported_by_root(graph);
    graph
        .definitions()
        .enumerate()
        .filter(|(_, definition)| {
            let identity = crate::surface::identity::Definition::of(definition.id());
            matches!(definition.visibility(), model::Visibility::Public)
                && matches!(
                    definition.kind(),
                    model::Kind::TypeAlias {
                        ty: target @ model::Type::Definition { id, arguments },
                        generics,
                    } if generics.parameters().len() == 0
                        && !matches!(id.unit().crate_name(), "core" | "alloc" | "std")
                        && graph
                            .definition_index(id)
                            .and_then(|index| graph.definitions().nth(index))
                            .is_some_and(|target| {
                                matches!(
                                    target.kind(),
                                    model::Kind::Struct { .. } | model::Kind::Enum { .. }
                                )
                            })
                        && generics.predicates().len() == 0
                        && arguments
                            .iter()
                            .any(|argument| matches!(argument, model::Argument::Type(_)))
                        && exported.values().any(|exported| {
                            exported.contains(&identity)
                                && !reaches_only_exported(target, exported)
                        })
                )
        })
        .map(|(index, _)| index)
        .collect()
}

/// The definitions each root crate exports, by root. Whether Rust code can
/// name a type is a question about one crate: a type another root of the
/// same graph re-exports, as `iced_test` re-exports iced's renderers, is
/// still unnamed in the crate that doesn't.
fn exported_by_root(
    graph: &api::Graph<impl api::Phase>,
) -> BTreeMap<&str, std::collections::BTreeSet<crate::surface::identity::Definition>> {
    let mut exported = BTreeMap::<_, std::collections::BTreeSet<_>>::new();
    for path in graph.exports() {
        if let ledger::Path::Imported(export) = path {
            exported
                .entry(export.key().root_alias())
                .or_default()
                .insert(crate::surface::identity::Definition::of(export.target()));
        }
    }
    exported
}

fn reaches_only_exported(
    ty: &model::Type,
    exported: &std::collections::BTreeSet<crate::surface::identity::Definition>,
) -> bool {
    match ty {
        model::Type::Definition { id, arguments } => {
            exported.contains(&crate::surface::identity::Definition::of(id))
                && arguments.iter().all(|argument| match argument {
                    model::Argument::Type(ty) => reaches_only_exported(ty, exported),
                    model::Argument::Lifetime(_) | model::Argument::Const(_) => true,
                })
        }
        model::Type::Tuple(members) => members
            .iter()
            .all(|member| reaches_only_exported(member, exported)),
        model::Type::Reference { ty, .. } | model::Type::Slice(ty) => {
            reaches_only_exported(ty, exported)
        }
        model::Type::Array { element, .. } => reaches_only_exported(element, exported),
        _ => true,
    }
}

/// `signature` and `generics` with each input that borrows a trait object
/// of traits alone, as serde's `invalid_length(len: usize, exp: &dyn
/// Expected)` takes, read as a borrow of an argument's `impl Trait` of its
/// own, bounded by the object's traits. The call's argument fills it at
/// its own type, a crate's, a std type the crate implements the trait for,
/// or a program type through its proxy, and the bridge's borrow of it
/// coerces to the object where it is passed, as rustc coerces it. An
/// object that binds an associated type, as `dyn Fn(A) -> B` does, stays
/// what it is. `None` when there is none.
fn object_inputs(
    signature: &model::Signature,
    generics: &model::Generics,
) -> Option<(model::Signature, model::Generics)> {
    fn object(
        ty: &model::Type,
    ) -> Option<(&model::Lifetime, model::Mutability, Vec<&model::Bound>)> {
        let model::Type::Reference {
            lifetime,
            mutability,
            ty,
        } = ty
        else {
            return None;
        };
        let model::Type::TraitObject { bounds, .. } = ty.as_ref() else {
            return None;
        };
        let traits = bounds
            .iter()
            .map(|bound| match bound {
                model::ObjectBound::Trait(
                    bound @ model::Bound::Trait {
                        polarity: model::Polarity::Positive,
                        ..
                    },
                ) => Some(bound),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()?;
        (!traits.is_empty()).then_some((lifetime, *mutability, traits))
    }
    if !signature.inputs().any(|input| object(input).is_some()) {
        return None;
    }
    let mut next = generics
        .parameters()
        .map(model::Parameter::index)
        .filter(|index| *index & representability::signature::FUTURE_OUTPUT == 0)
        .max()
        .map_or(0, |index| index + 1);
    let mut parameters = generics.parameters().cloned().collect::<Vec<_>>();
    let mut predicates = generics.predicates().cloned().collect::<Vec<_>>();
    let mut inputs = Vec::new();
    for input in signature.inputs() {
        let Some((lifetime, mutability, traits)) = object(input) else {
            inputs.push(input.clone());
            continue;
        };
        // A parameter is read by its index alone, so one an item with no
        // parameters of its own declares is owned by the object's trait.
        let owner = match generics.parameters().next() {
            Some(parameter) => parameter.owner().clone(),
            None => match traits[0] {
                model::Bound::Trait { trait_id, .. } => trait_id.clone(),
                model::Bound::MaybeSized => return None,
            },
        };
        let index = next;
        next += 1;
        let parameter = model::Type::Parameter {
            owner: owner.clone(),
            index,
        };
        parameters.push(model::Parameter::new(
            owner,
            index,
            format!("impl dyn#{index}").into(),
            model::ParameterKind::Type {
                bounds: Box::default(),
                default: model::Default::Absent,
            },
        ));
        for bound in traits {
            predicates.push(model::Predicate::Bound {
                ty: parameter.clone(),
                bound: bound.clone(),
            });
        }
        inputs.push(model::Type::Reference {
            lifetime: lifetime.clone(),
            mutability,
            ty: Box::new(parameter),
        });
    }
    let rewritten = model::Signature::new(
        signature.safety(),
        signature.const_function(),
        signature.async_function(),
        signature.abi().into(),
        signature.variadic(),
        signature.binders().cloned().collect(),
        signature.receiver().clone(),
        inputs.into_boxed_slice(),
        signature.output().clone(),
    )
    .with_guard(signature.guard().cloned());
    Some((
        rewritten,
        model::Generics::new(parameters.into_boxed_slice(), predicates.into_boxed_slice()),
    ))
}

fn receiver_binder_is_exact(signature: &model::Signature) -> bool {
    let mut binders = signature.binders();
    match signature.receiver() {
        model::Receiver::Reference {
            lifetime: model::Lifetime::Late { binder, index },
            ..
        } => {
            let Some(declared) = binders.next() else {
                return false;
            };
            declared == binder && *index == binder.index() && binders.next().is_none()
        }
        model::Receiver::None
        | model::Receiver::Value { .. }
        | model::Receiver::Reference { .. }
        | model::Receiver::Explicit { .. } => binders.next().is_none(),
    }
}

/// `ty`, an input type that holds borrows, when the call can lease each of
/// them as the generator's `Lease` does: every borrow `convert_input` gives
/// is one the call lends, alone or in an `Option`, a `Result` or a tuple,
/// beside members that borrow nothing and move. A `&mut` of Rawr's own
/// string or array, copied in and back out, or of one of std's values
/// Rawr's std holds, moved in and back out, has no place to go back to
/// from inside one.
fn leased(ty: representability::convert::Type) -> Result<representability::convert::Type, Failure> {
    use representability::convert::{Builtin, Type};
    fn leasable(ty: &Type) -> bool {
        match ty {
            Type::Borrowed { mutable: true, ty } => match &**ty {
                Type::Sequence { .. } | Type::Builtin(Builtin::String) => false,
                Type::Nominal { definition, .. } => {
                    crate::surface::identity::Definition::std_path(definition.as_bytes()).is_none()
                }
                _ => true,
            },
            Type::Option(element) => leasable(element),
            Type::Result(ok, err) => leasable(ok) && leasable(err),
            Type::Tuple(members) => members.iter().all(leasable),
            _ => true,
        }
    }
    if leasable(&ty) {
        Ok(ty)
    } else {
        Err(representability::reject::Kind::ReferenceType.into())
    }
}

/// Whether `ty` is a number, `bool` or `char`: the self type of a crate's
/// operator impl for a std type, as `impl Mul<Radians> for f32`.
fn is_number(ty: &model::Type) -> bool {
    matches!(ty, model::Type::Primitive(name) if !matches!(&**name, "str" | "!"))
}

fn is_str_reference(ty: &model::Type) -> bool {
    matches!(ty, model::Type::Reference { mutability: model::Mutability::Shared, ty, .. }
        if matches!(&**ty, model::Type::Primitive(name) if &**name == "str"))
}

/// The type of a `const` or `static` of `&str`, as inboard's `pub const
/// UI_FAMILY: &str`, which is `&'static str` in either: a borrow of Rawr's
/// string, read once where the bridge runs and kept for the runner's life,
/// which a call taking `&'static str` takes pinned as it is (#1146).
fn static_text(ty: &model::Type) -> Option<representability::convert::Type> {
    use representability::convert::{Builtin, Type};
    is_str_reference(ty).then(|| Type::Borrowed {
        mutable: false,
        ty: Box::new(Type::Builtin(Builtin::StaticStr)),
    })
}

/// Whether `ty` is a shared borrow of a type parameter, as `&'f F`.
fn is_parameter_reference(ty: &model::Type) -> bool {
    matches!(ty, model::Type::Reference { mutability: model::Mutability::Shared, ty, .. }
        if matches!(&**ty, model::Type::Parameter { .. }))
}

/// Whether a call reaches its receiver for the call alone: it has none, or
/// borrows it at a late-bound lifetime its output doesn't name.
fn receiver_lent_for_call(signature: &model::Signature) -> bool {
    match signature.receiver() {
        model::Receiver::None => true,
        model::Receiver::Reference {
            lifetime: lifetime @ model::Lifetime::Late { .. },
            ..
        } => !names_lifetime(signature.output(), lifetime),
        model::Receiver::Reference { .. }
        | model::Receiver::Value { .. }
        | model::Receiver::Explicit { .. } => false,
    }
}

/// A trait's provided method, as iced's `Widget::_boxed`, as a member of
/// `implementation`: it names the trait's parameters, which the
/// implementation's trait arguments are, and its own parameters follow the
/// trait's, so they follow the implementation's here. `None` for any other
/// definition.
fn provided_signature(
    implementation: &model::Implementation,
    definition: &model::Definition,
    signature: &model::Signature,
    generics: &model::Generics,
) -> Result<Option<(model::Signature, model::Generics)>, Error> {
    let (model::Owner::Definition(owner), model::Trait::Trait { id, arguments }) =
        (definition.owner(), implementation.trait_ref())
    else {
        return Ok(None);
    };
    if owner != id {
        return Ok(None);
    }
    let mut substitutions = arguments
        .iter()
        .enumerate()
        .filter_map(|(index, argument)| match argument {
            model::Argument::Type(ty) => {
                Some(((id.clone(), u32::try_from(index).ok()?), ty.clone()))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    let first = implementation
        .generics()
        .parameters()
        .map(|parameter| parameter.index().saturating_add(1))
        .max()
        .unwrap_or(0);
    let mut own = Vec::new();
    for (position, parameter) in generics.parameters().enumerate() {
        let index = first
            .checked_add(u32::try_from(position).map_err(|_| Error::IndexOverflow)?)
            .ok_or(Error::IndexOverflow)?;
        if matches!(parameter.kind(), model::ParameterKind::Type { .. }) {
            substitutions.push((
                (parameter.owner().clone(), parameter.index()),
                model::Type::Parameter {
                    owner: parameter.owner().clone(),
                    index,
                },
            ));
        }
        own.push(model::Parameter::new(
            parameter.owner().clone(),
            index,
            parameter.name().into(),
            parameter.kind().clone(),
        ));
    }
    let own = model::Generics::new(
        own.into_boxed_slice(),
        generics
            .predicates()
            .map(|predicate| substituted_predicate(predicate, &substitutions))
            .collect(),
    );
    Ok(Some((
        substituted_signature(signature, &substitutions),
        own,
    )))
}

/// A method whose own lifetimes only lend its `&str` inputs, as chrono's
/// `format<'a>(&self, fmt: &'a str)`: each input that borrows is a `&str`,
/// and the output does not borrow, or is a `Display` value Rawr keeps as
/// its text.
fn late_binders_lend_strings(signature: &model::Signature) -> bool {
    let receiver_binders = usize::from(matches!(
        signature.receiver(),
        model::Receiver::Reference {
            lifetime: model::Lifetime::Late { .. },
            ..
        }
    ));
    signature.binders().len() > receiver_binders
        && signature.inputs().any(is_str_reference)
        && signature.inputs().all(|input| match input {
            model::Type::Reference { .. } => is_str_reference(input),
            _ => true,
        })
        && !matches!(signature.output(), model::Type::Reference { .. })
}

/// A method whose own lifetimes only lend its inputs for the call, as
/// ratatui's `render_stateful_widget(&mut self, W, Rect, &mut W::State)`:
/// each input that borrows is a `&str` or a borrow of a value that holds
/// no borrow, which the call leases (D558.5), and the output names none of
/// the method's own lifetimes.
fn late_binders_lend_inputs(signature: &model::Signature) -> bool {
    let receiver_binders = usize::from(matches!(
        signature.receiver(),
        model::Receiver::Reference {
            lifetime: model::Lifetime::Late { .. },
            ..
        }
    ));
    signature.binders().len() > receiver_binders
        && signature.inputs().any(names_late_lifetime)
        && signature.inputs().all(lends_late_borrows)
        && !names_late_lifetime(signature.output())
}

/// Whether each late-bound lifetime `ty` names is the lifetime of a borrow
/// the call lends: a reference to a value that names none, alone or in a
/// type's arguments, as iced's `combo_box(.., selection: Option<&T>, ..)`
/// takes.
fn lends_late_borrows(ty: &model::Type) -> bool {
    let argument = |argument: &model::Argument| match argument {
        model::Argument::Type(ty) => lends_late_borrows(ty),
        model::Argument::Lifetime(lifetime) => !matches!(lifetime, model::Lifetime::Late { .. }),
        model::Argument::Const(_) => true,
    };
    match ty {
        model::Type::Reference { ty, .. } => !names_late_lifetime(ty),
        model::Type::Definition { arguments, .. } => arguments.iter().all(argument),
        model::Type::Tuple(types) => types.iter().all(lends_late_borrows),
        model::Type::Slice(ty) => lends_late_borrows(ty),
        model::Type::Array { element, .. } | model::Type::RawPointer { ty: element, .. } => {
            lends_late_borrows(element)
        }
        ty => !names_late_lifetime(ty),
    }
}

/// `signature`, of a function that borrows no receiver, with each of its
/// own late-bound lifetimes that its output names made early-bound, at
/// the index it would have after `generics`' parameters. rustc makes no
/// difference between the two at a direct call. `None` where the output
/// names none of them.
fn output_binders_early(
    signature: &model::Signature,
    generics: &model::Generics,
) -> Option<model::Signature> {
    if !matches!(
        signature.receiver(),
        model::Receiver::None | model::Receiver::Value { .. }
    ) {
        return None;
    }
    let declared = u32::try_from(generics.parameters().len()).ok()?;
    let mut early = signature.clone();
    let mut kept = Vec::new();
    for binder in signature.binders() {
        let late = model::Lifetime::Late {
            binder: binder.clone(),
            index: binder.index(),
        };
        let bound = model::Lifetime::Early {
            owner: binder.owner().clone(),
            index: declared.checked_add(binder.index())?,
        };
        let output = rebind_lifetime(early.output(), &late, &bound);
        if output == *early.output() {
            kept.push(binder.clone());
            continue;
        }
        early = map_signature_types(&early, |ty| rebind_lifetime(ty, &late, &bound));
    }
    (kept.len() < signature.binders().len()).then(|| early.with_binders(kept.into()))
}

/// `signature` with each of its own late-bound lifetimes that it names
/// only as a crate type's lifetime argument in its inputs, as serde's
/// `invalid_value(unexp: Unexpected<'_>, ..)` names one, at `'static`.
/// The call picks such a lifetime, and the values Rawr holds are at
/// `'static` (D558 tier 0), so rustc picks it there too. `None` where it
/// names none of them.
fn argument_binders_static(signature: &model::Signature) -> Option<model::Signature> {
    let mut chosen = signature.clone();
    let mut kept = Vec::new();
    for binder in signature.binders() {
        let late = model::Lifetime::Late {
            binder: binder.clone(),
            index: binder.index(),
        };
        let receiver = match signature.receiver() {
            model::Receiver::None => Naming::Not,
            model::Receiver::Value { ty } | model::Receiver::Explicit { ty } => naming(ty, &late),
            model::Receiver::Reference { lifetime, ty, .. } if *lifetime != late => {
                naming(ty, &late)
            }
            model::Receiver::Reference { .. } => Naming::Otherwise,
        };
        let named = signature
            .inputs()
            .fold(receiver, |named, input| named.and(naming(input, &late)))
            .and(match naming(signature.output(), &late) {
                Naming::Not => Naming::Not,
                Naming::Argument | Naming::Otherwise => Naming::Otherwise,
            });
        if named != Naming::Argument {
            kept.push(binder.clone());
            continue;
        }
        chosen = map_signature_types(&chosen, |ty| {
            rebind_lifetime(ty, &late, &model::Lifetime::Static)
        });
    }
    (kept.len() < signature.binders().len()).then(|| chosen.with_binders(kept.into()))
}

/// How a type names a lifetime ([`naming`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Naming {
    Not,
    /// Only as a lifetime argument of a crate's type, as `Unexpected<'a>`.
    Argument,
    /// As a reference's lifetime, or where its binders may be another's: a
    /// function pointer, a trait object, an opaque type or a projection.
    Otherwise,
}

impl Naming {
    const fn and(self, other: Self) -> Self {
        match (self, other) {
            (Self::Otherwise, _) | (_, Self::Otherwise) => Self::Otherwise,
            (Self::Argument, _) | (_, Self::Argument) => Self::Argument,
            (Self::Not, Self::Not) => Self::Not,
        }
    }
}

/// How `ty` names `lifetime`.
fn naming(ty: &model::Type, lifetime: &model::Lifetime) -> Naming {
    let argument = |argument: &model::Argument| match argument {
        model::Argument::Type(ty) => naming(ty, lifetime),
        model::Argument::Lifetime(named) if named == lifetime => Naming::Argument,
        model::Argument::Lifetime(_) | model::Argument::Const(_) => Naming::Not,
    };
    match ty {
        model::Type::Definition { arguments, .. } => arguments
            .iter()
            .fold(Naming::Not, |named, next| named.and(argument(next))),
        model::Type::Reference {
            lifetime: named, ..
        } if named == lifetime => Naming::Otherwise,
        model::Type::Reference { ty, .. }
        | model::Type::Slice(ty)
        | model::Type::Array { element: ty, .. }
        | model::Type::RawPointer { ty, .. } => naming(ty, lifetime),
        model::Type::Tuple(types) => types
            .iter()
            .fold(Naming::Not, |named, ty| named.and(naming(ty, lifetime))),
        model::Type::FunctionPointer(_)
        | model::Type::TraitObject { .. }
        | model::Type::ImplTrait { .. }
        | model::Type::Projection { .. } => {
            if names_lifetime(ty, lifetime) {
                Naming::Otherwise
            } else {
                Naming::Not
            }
        }
        model::Type::Primitive(_)
        | model::Type::Parameter { .. }
        | model::Type::SelfType(_)
        | model::Type::Never => Naming::Not,
    }
}

/// `ty` with each `from` lifetime in it as `to`, in an opaque type's
/// captures and bounds too. A function pointer's and a trait object's own
/// bounds are left as they are.
fn rebind_lifetime(ty: &model::Type, from: &model::Lifetime, to: &model::Lifetime) -> model::Type {
    let lifetime = |lifetime: &model::Lifetime| {
        if lifetime == from {
            to.clone()
        } else {
            lifetime.clone()
        }
    };
    let ty_of = |ty: &model::Type| rebind_lifetime(ty, from, to);
    let argument = |argument: &model::Argument| match argument {
        model::Argument::Lifetime(named) => model::Argument::Lifetime(lifetime(named)),
        model::Argument::Type(ty) => model::Argument::Type(ty_of(ty)),
        model::Argument::Const(_) => argument.clone(),
    };
    let predicate = |predicate: &model::Predicate| match predicate {
        model::Predicate::Bound { ty, bound } => model::Predicate::Bound {
            ty: ty_of(ty),
            bound: match bound {
                model::Bound::Trait {
                    trait_id,
                    arguments,
                    polarity,
                    binders,
                } => model::Bound::Trait {
                    trait_id: trait_id.clone(),
                    arguments: arguments.iter().map(argument).collect(),
                    polarity: *polarity,
                    binders: binders.clone(),
                },
                model::Bound::MaybeSized => model::Bound::MaybeSized,
            },
        },
        model::Predicate::ProjectionEquality { projection, ty } => {
            model::Predicate::ProjectionEquality {
                projection: ty_of(projection),
                ty: ty_of(ty),
            }
        }
        model::Predicate::LifetimeOutlives { longer, shorter } => {
            model::Predicate::LifetimeOutlives {
                longer: lifetime(longer),
                shorter: lifetime(shorter),
            }
        }
        model::Predicate::TypeOutlives {
            ty,
            lifetime: named,
        } => model::Predicate::TypeOutlives {
            ty: ty_of(ty),
            lifetime: lifetime(named),
        },
        model::Predicate::ConstEvaluatable(_) => predicate.clone(),
    };
    match ty {
        model::Type::Definition { id, arguments } => model::Type::Definition {
            id: id.clone(),
            arguments: arguments.iter().map(argument).collect(),
        },
        model::Type::Projection {
            trait_id,
            associated,
            arguments,
        } => model::Type::Projection {
            trait_id: trait_id.clone(),
            associated: associated.clone(),
            arguments: arguments.iter().map(argument).collect(),
        },
        model::Type::Tuple(members) => model::Type::Tuple(members.iter().map(ty_of).collect()),
        model::Type::Slice(element) => model::Type::Slice(Box::new(ty_of(element))),
        model::Type::Array { element, length } => model::Type::Array {
            element: Box::new(ty_of(element)),
            length: length.clone(),
        },
        model::Type::Reference {
            lifetime: named,
            mutability,
            ty,
        } => model::Type::Reference {
            lifetime: lifetime(named),
            mutability: *mutability,
            ty: Box::new(ty_of(ty)),
        },
        model::Type::RawPointer { mutability, ty } => model::Type::RawPointer {
            mutability: *mutability,
            ty: Box::new(ty_of(ty)),
        },
        model::Type::TraitObject {
            bounds,
            lifetime: named,
        } => model::Type::TraitObject {
            bounds: bounds.clone(),
            lifetime: lifetime(named),
        },
        model::Type::ImplTrait {
            id,
            bounds,
            captures,
        } => model::Type::ImplTrait {
            id: id.clone(),
            bounds: bounds.iter().map(predicate).collect(),
            captures: captures.iter().map(argument).collect(),
        },
        model::Type::Primitive(_)
        | model::Type::Parameter { .. }
        | model::Type::SelfType(_)
        | model::Type::FunctionPointer(_)
        | model::Type::Never => ty.clone(),
    }
}

/// A method whose output borrows its receiver, as quick-xml's
/// `BytesStart::name(&self) -> QName<'_>` does: it opens a scope frame that
/// holds the receiver borrowed while the output lives (D558 tier 1, slice
/// 2). A plain `&T` output is read as a borrow guard instead.
fn borrows_receiver(
    signature: &model::Signature,
    is_option: impl Fn(&api::identity::Definition) -> bool,
) -> bool {
    let model::Receiver::Reference {
        lifetime: receiver @ model::Lifetime::Late { .. },
        ..
    } = signature.receiver()
    else {
        return false;
    };
    let output = signature.output();
    // A nominal output, as `QName<'_>`: a borrow guard is read through its
    // target, an async method's future holds its receiver (D513.6), and an
    // opaque or reference output is held to its own rules. An input may
    // name the receiver's lifetime as a crate type's lifetime argument, as
    // inboard's `wizard::State::view<'a>(&'a self, catalog: View<'a>)`
    // takes a value of borrows by value (#1225): the frame's lifetime, as
    // the output's is. So may a shared reference at that lifetime, as its
    // `theme: &'a Theme` is, or a `&'a str`, as `titled(&'a self, title:
    // &'a str)` takes (#1503).
    matches!(output, model::Type::Definition { .. })
        && signature.guard().is_none()
        && !signature.async_function()
        && names_lifetime(output, receiver)
        && !names_opaque(output)
        && signature.binders().len() == 1
        && signature.inputs().all(|input| {
            !names_late_lifetime(input)
                || naming(input, receiver) == Naming::Argument
                || shares_at(input, receiver)
                || text_at(input, receiver)
        })
        && !lends_option_reference(output, receiver, is_option)
}

/// Whether `ty` is a `&'r str` at `lifetime`, which a frame's output keeps
/// as the program's text, pinned where the output is (#1503).
fn text_at(ty: &model::Type, lifetime: &model::Lifetime) -> bool {
    matches!(ty, model::Type::Reference {
        lifetime: named,
        mutability: model::Mutability::Shared,
        ty,
    } if named == lifetime && matches!(&**ty, model::Type::Primitive(name) if &**name == "str"))
}

/// Whether `ty` is a `&'r T` at `lifetime` of a crate's type `T` that
/// borrows nothing else, as inboard's `theme: &'a Theme` is.
fn shares_at(ty: &model::Type, lifetime: &model::Lifetime) -> bool {
    matches!(ty, model::Type::Reference {
        lifetime: named,
        mutability: model::Mutability::Shared,
        ty,
    } if named == lifetime
        && matches!(**ty, model::Type::Definition { .. })
        && !names_late_lifetime(ty))
}

/// Whether `output` is an `Option<&'r T>` of the receiver's lifetime `'r`,
/// as pane_grid's `State::get_mut` gives: a borrow guard's place, not a
/// frame (T012). Another type of one borrowed argument, as iced's
/// `Key::as_ref(&self) -> Key<&str>` gives, is a frame's value.
fn lends_option_reference(
    output: &model::Type,
    receiver: &model::Lifetime,
    is_option: impl Fn(&api::identity::Definition) -> bool,
) -> bool {
    matches!(output, model::Type::Definition { id, arguments }
        if matches!(&arguments[..], [model::Argument::Type(model::Type::Reference { lifetime, .. })]
            if lifetime == receiver)
            && is_option(id))
}

/// Whether `ty` holds an opaque, trait object or function pointer type.
fn names_opaque(ty: &model::Type) -> bool {
    let argument = |argument: &model::Argument| match argument {
        model::Argument::Type(ty) => names_opaque(ty),
        model::Argument::Lifetime(_) | model::Argument::Const(_) => false,
    };
    match ty {
        model::Type::FunctionPointer(_)
        | model::Type::TraitObject { .. }
        | model::Type::ImplTrait { .. }
        | model::Type::Projection { .. } => true,
        model::Type::Reference { ty, .. }
        | model::Type::Slice(ty)
        | model::Type::Array { element: ty, .. }
        | model::Type::RawPointer { ty, .. } => names_opaque(ty),
        model::Type::Definition { arguments, .. } => arguments.iter().any(argument),
        model::Type::Tuple(types) => types.iter().any(names_opaque),
        model::Type::Primitive(_)
        | model::Type::Parameter { .. }
        | model::Type::SelfType(_)
        | model::Type::Never => false,
    }
}

/// Whether `ty` names `lifetime` anywhere in it.
fn names_lifetime(ty: &model::Type, lifetime: &model::Lifetime) -> bool {
    let argument = |argument: &model::Argument| match argument {
        model::Argument::Type(ty) => names_lifetime(ty, lifetime),
        model::Argument::Lifetime(named) => named == lifetime,
        model::Argument::Const(_) => false,
    };
    match ty {
        model::Type::Reference {
            lifetime: named,
            ty,
            ..
        } => named == lifetime || names_lifetime(ty, lifetime),
        model::Type::Definition { arguments, .. } | model::Type::Projection { arguments, .. } => {
            arguments.iter().any(argument)
        }
        model::Type::Tuple(types) => types.iter().any(|ty| names_lifetime(ty, lifetime)),
        model::Type::Slice(ty) => names_lifetime(ty, lifetime),
        model::Type::Array { element, .. } | model::Type::RawPointer { ty: element, .. } => {
            names_lifetime(element, lifetime)
        }
        model::Type::FunctionPointer(_)
        | model::Type::TraitObject { .. }
        | model::Type::ImplTrait { .. } => true,
        model::Type::Primitive(_)
        | model::Type::Parameter { .. }
        | model::Type::SelfType(_)
        | model::Type::Never => false,
    }
}

/// Whether `ty` borrows at a late-bound lifetime anywhere in it.
fn names_late_lifetime(ty: &model::Type) -> bool {
    let argument = |argument: &model::Argument| match argument {
        model::Argument::Type(ty) => names_late_lifetime(ty),
        model::Argument::Lifetime(lifetime) => matches!(lifetime, model::Lifetime::Late { .. }),
        model::Argument::Const(_) => false,
    };
    match ty {
        model::Type::Reference { lifetime, ty, .. } => {
            matches!(lifetime, model::Lifetime::Late { .. }) || names_late_lifetime(ty)
        }
        model::Type::Definition { arguments, .. } | model::Type::Projection { arguments, .. } => {
            arguments.iter().any(argument)
        }
        model::Type::Tuple(types) => types.iter().any(names_late_lifetime),
        model::Type::Slice(ty) => names_late_lifetime(ty),
        model::Type::Array { element, .. } | model::Type::RawPointer { ty: element, .. } => {
            names_late_lifetime(element)
        }
        // A function pointer, a trait object or an opaque type is held to
        // its own rules elsewhere.
        model::Type::FunctionPointer(_)
        | model::Type::TraitObject { .. }
        | model::Type::ImplTrait { .. } => true,
        model::Type::Primitive(_)
        | model::Type::Parameter { .. }
        | model::Type::SelfType(_)
        | model::Type::Never => false,
    }
}

/// `ty` with each `<F as Future>::Output` of a future parameter `F` named
/// as its synthetic output parameter (D527).
/// Whether `ty` names an associated type of a type parameter, as
/// `(I::Item, J::Item)` does.
fn projects_parameter(ty: &model::Type) -> bool {
    let argument = |argument: &model::Argument| matches!(argument, model::Argument::Type(ty) if projects_parameter(ty));
    match ty {
        model::Type::Projection { arguments, .. } => match arguments.first() {
            Some(model::Argument::Type(model::Type::Parameter { .. })) => true,
            _ => arguments.iter().any(argument),
        },
        model::Type::Definition { arguments, .. } => arguments.iter().any(argument),
        model::Type::Tuple(members) => members.iter().any(projects_parameter),
        model::Type::Reference { ty, .. } | model::Type::Slice(ty) => projects_parameter(ty),
        model::Type::Array { element, .. } => projects_parameter(element),
        _ => false,
    }
}

fn future_projections(
    ty: &model::Type,
    futures: &[(u32, api::identity::Definition)],
    is_future: impl Fn(&api::identity::Definition) -> bool + Copy,
) -> model::Type {
    if futures.is_empty() {
        return ty.clone();
    }
    let argument = |argument: &model::Argument| match argument {
        model::Argument::Type(ty) => {
            model::Argument::Type(future_projections(ty, futures, is_future))
        }
        other => other.clone(),
    };
    match ty {
        model::Type::Projection {
            trait_id,
            arguments,
            ..
        } if is_future(trait_id) => {
            if let Some(model::Argument::Type(model::Type::Parameter { owner, index })) =
                arguments.first()
                && futures.iter().any(|(future, _)| future == index)
            {
                return model::Type::Parameter {
                    owner: owner.clone(),
                    index: representability::signature::future_output(*index),
                };
            }
            ty.clone()
        }
        model::Type::Definition { id, arguments } => model::Type::Definition {
            id: id.clone(),
            arguments: arguments.iter().map(argument).collect(),
        },
        model::Type::Tuple(members) => model::Type::Tuple(
            members
                .iter()
                .map(|member| future_projections(member, futures, is_future))
                .collect(),
        ),
        model::Type::Reference {
            lifetime,
            mutability,
            ty,
        } => model::Type::Reference {
            lifetime: lifetime.clone(),
            mutability: *mutability,
            ty: Box::new(future_projections(ty, futures, is_future)),
        },
        other => other.clone(),
    }
}

/// Whether `ty` names a type parameter other than the one at `except`.
/// Whether the implementation `from`'s self type `pattern` is `target`,
/// with `fixed` the type each of its parameters stands for there.
fn match_self_type(
    pattern: &model::Type,
    target: &model::Type,
    from: &api::identity::Definition,
    fixed: &mut Vec<(u32, model::Type)>,
) -> bool {
    match (pattern, target) {
        (model::Type::Parameter { owner, index }, target) if owner == from => {
            match fixed.iter().find(|(known, _)| known == index) {
                Some((_, known)) => known == target,
                None => {
                    fixed.push((*index, target.clone()));
                    true
                }
            }
        }
        (
            model::Type::Definition { id, arguments },
            model::Type::Definition {
                id: target_id,
                arguments: target_arguments,
            },
        ) if id == target_id && arguments.len() == target_arguments.len() => arguments
            .iter()
            .zip(target_arguments.iter())
            .all(|pair| match pair {
                (model::Argument::Type(pattern), model::Argument::Type(target)) => {
                    match_self_type(pattern, target, from, fixed)
                }
                (pattern, target) => pattern == target,
            }),
        (pattern, target) => pattern == target,
    }
}

/// `predicate` of the implementation `from` with each of its type
/// parameters the type `renamed` gives. `None` when it names one `renamed`
/// has no type for.
fn relabel_predicate(
    predicate: &model::Predicate,
    from: &api::identity::Definition,
    renamed: &impl Fn(u32) -> Option<model::Type>,
) -> Option<model::Predicate> {
    let ty = |ty: &model::Type| relabel_type(ty, from, renamed);
    Some(match predicate {
        model::Predicate::Bound { ty: bounded, bound } => model::Predicate::Bound {
            ty: ty(bounded)?,
            bound: relabel_bound(bound, from, renamed)?,
        },
        model::Predicate::ProjectionEquality {
            projection,
            ty: equal,
        } => model::Predicate::ProjectionEquality {
            projection: ty(projection)?,
            ty: ty(equal)?,
        },
        model::Predicate::TypeOutlives {
            ty: outliving,
            lifetime,
        } => model::Predicate::TypeOutlives {
            ty: ty(outliving)?,
            lifetime: lifetime.clone(),
        },
        model::Predicate::LifetimeOutlives { .. } | model::Predicate::ConstEvaluatable(_) => {
            predicate.clone()
        }
    })
}

fn relabel_bound(
    bound: &model::Bound,
    from: &api::identity::Definition,
    renamed: &impl Fn(u32) -> Option<model::Type>,
) -> Option<model::Bound> {
    Some(match bound {
        model::Bound::Trait {
            trait_id,
            arguments,
            polarity,
            binders,
        } => model::Bound::Trait {
            trait_id: trait_id.clone(),
            arguments: relabel_arguments(arguments, from, renamed)?,
            polarity: *polarity,
            binders: binders.clone(),
        },
        model::Bound::MaybeSized => model::Bound::MaybeSized,
    })
}

fn relabel_arguments(
    arguments: &[model::Argument],
    from: &api::identity::Definition,
    renamed: &impl Fn(u32) -> Option<model::Type>,
) -> Option<Box<[model::Argument]>> {
    arguments
        .iter()
        .map(|argument| match argument {
            model::Argument::Type(ty) => relabel_type(ty, from, renamed).map(model::Argument::Type),
            other => Some(other.clone()),
        })
        .collect()
}

fn relabel_type(
    ty: &model::Type,
    from: &api::identity::Definition,
    renamed: &impl Fn(u32) -> Option<model::Type>,
) -> Option<model::Type> {
    let inner = |ty: &model::Type| relabel_type(ty, from, renamed).map(Box::new);
    Some(match ty {
        model::Type::Parameter { owner, index } if owner == from => renamed(*index)?,
        model::Type::Definition { id, arguments } => model::Type::Definition {
            id: id.clone(),
            arguments: relabel_arguments(arguments, from, renamed)?,
        },
        model::Type::Projection {
            trait_id,
            associated,
            arguments,
        } => model::Type::Projection {
            trait_id: trait_id.clone(),
            associated: associated.clone(),
            arguments: relabel_arguments(arguments, from, renamed)?,
        },
        model::Type::Tuple(members) => model::Type::Tuple(
            members
                .iter()
                .map(|member| relabel_type(member, from, renamed))
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
            lifetime: lifetime.clone(),
            mutability: *mutability,
            ty: inner(ty)?,
        },
        model::Type::RawPointer { mutability, ty } => model::Type::RawPointer {
            mutability: *mutability,
            ty: inner(ty)?,
        },
        // An implementation's opaque type, as the `impl Fn(B) -> O` of
        // iced's `impl<F, A, B, O> Function<A, B, O> for F`, at the types
        // its parameters are given.
        model::Type::ImplTrait {
            id,
            bounds,
            captures,
        } => model::Type::ImplTrait {
            id: id.clone(),
            bounds: bounds
                .iter()
                .map(|bound| relabel_predicate(bound, from, renamed))
                .collect::<Option<_>>()?,
            captures: relabel_arguments(captures, from, renamed)?,
        },
        model::Type::SelfType(id) if id == from => return None,
        model::Type::FunctionPointer(_) | model::Type::TraitObject { .. } => return None,
        other => other.clone(),
    })
}

fn names_other_parameter(ty: &representability::convert::Type, except: u32) -> bool {
    use representability::convert::Type;
    let other = |ty: &Type| names_other_parameter(ty, except);
    match ty {
        Type::Parameter(index) => *index != except,
        Type::Inferred => false,
        Type::Unit | Type::Never | Type::Builtin(_) => false,
        Type::Nominal { arguments, .. }
        | Type::Tuple(arguments)
        | Type::Script { arguments, .. } => arguments.iter().any(other),
        Type::Option(element)
        | Type::Borrowed { ty: element, .. }
        | Type::Range(element)
        | Type::Sequence { element, .. } => other(element),
        Type::ConstArray { element, parameter } => *parameter != except || other(element),
        Type::Result(ok, err) => other(ok) || other(err),
        Type::FunctionItem { input, output } => other(input) || other(output),
        Type::ProfileMessage { payload, .. } => other(payload),
    }
}

/// Whether `ty` names the type parameter at `index` anywhere in it.
fn names_parameter(ty: &representability::convert::Type, index: u32) -> bool {
    use representability::convert::Type;
    match ty {
        Type::Parameter(parameter) => *parameter == index,
        Type::Inferred => false,
        Type::Unit | Type::Never | Type::Builtin(_) => false,
        Type::Nominal { arguments, .. } => arguments
            .iter()
            .any(|argument| names_parameter(argument, index)),
        Type::Option(element)
        | Type::Borrowed { ty: element, .. }
        | Type::Range(element)
        | Type::Sequence { element, .. } => names_parameter(element, index),
        Type::ConstArray { element, parameter } => {
            *parameter == index || names_parameter(element, index)
        }
        Type::Result(ok, err) => names_parameter(ok, index) || names_parameter(err, index),
        Type::Tuple(members)
        | Type::Script {
            arguments: members, ..
        } => members.iter().any(|member| names_parameter(member, index)),
        Type::FunctionItem { input, output } => {
            names_parameter(input, index) || names_parameter(output, index)
        }
        Type::ProfileMessage { payload, .. } => names_parameter(payload, index),
    }
}

/// Whether `ty` names a boxed script closure (T417).
fn output_boxes(ty: &representability::convert::Type) -> bool {
    use representability::convert::Type;
    match ty {
        Type::Nominal { definition, .. }
            if *definition == crate::surface::identity::Definition::callable() =>
        {
            true
        }
        Type::Nominal { arguments, .. }
        | Type::Tuple(arguments)
        | Type::Script { arguments, .. } => arguments.iter().any(output_boxes),
        Type::Option(inner)
        | Type::Range(inner)
        | Type::Borrowed { ty: inner, .. }
        | Type::Sequence { element: inner, .. } => output_boxes(inner),
        Type::Result(ok, err) => output_boxes(ok) || output_boxes(err),
        _ => false,
    }
}

/// `ty` with the type parameter `index` replaced by `with` at any depth.
fn replace_parameter(
    ty: representability::convert::Type,
    index: u32,
    with: &representability::convert::Type,
) -> representability::convert::Type {
    use representability::convert::Type;
    let all = |types: Box<[Type]>| {
        types
            .into_vec()
            .into_iter()
            .map(|ty| replace_parameter(ty, index, with))
            .collect::<Box<[_]>>()
    };
    let one = |ty: Box<Type>| Box::new(replace_parameter(*ty, index, with));
    match ty {
        Type::Parameter(parameter) if parameter == index => with.clone(),
        Type::Nominal {
            definition,
            arguments,
        } => Type::Nominal {
            definition,
            arguments: all(arguments),
        },
        Type::Script {
            path,
            arguments,
            holder,
        } => Type::Script {
            path,
            arguments: all(arguments),
            holder,
        },
        Type::Tuple(members) => Type::Tuple(all(members)),
        Type::Option(element) => Type::Option(one(element)),
        Type::Range(element) => Type::Range(one(element)),
        Type::Result(ok, err) => Type::Result(one(ok), one(err)),
        Type::Borrowed { mutable, ty } => Type::Borrowed {
            mutable,
            ty: one(ty),
        },
        Type::Sequence {
            element,
            owned,
            length,
        } => Type::Sequence {
            element: one(element),
            owned,
            length,
        },
        other => other,
    }
}

/// `ty` with each future parameter `F` given as the core future of its
/// synthetic output: a script future crosses there (D527).
fn futurize(
    ty: representability::convert::Type,
    futures: &[(u32, api::identity::Definition)],
) -> representability::convert::Type {
    use representability::convert::Type;
    match ty {
        Type::Parameter(index) if futures.iter().any(|(future, _)| *future == index) => {
            Type::Nominal {
                definition: crate::surface::identity::Definition::future(),
                arguments: Box::from([Type::Parameter(
                    representability::signature::future_output(index),
                )]),
            }
        }
        Type::Option(element) => Type::Option(Box::new(futurize(*element, futures))),
        Type::Result(ok, err) => Type::Result(
            Box::new(futurize(*ok, futures)),
            Box::new(futurize(*err, futures)),
        ),
        Type::Nominal {
            definition,
            arguments,
        } => Type::Nominal {
            definition,
            arguments: arguments
                .into_vec()
                .into_iter()
                .map(|argument| futurize(argument, futures))
                .collect(),
        },
        other => other,
    }
}

/// `signature` with each parameter in `substitutions` replaced in its
/// receiver, inputs and output.
/// The opaque `impl Trait` types `ty` holds, outermost first.
pub(crate) fn impl_traits<'t>(ty: &'t model::Type, found: &mut Vec<&'t model::Type>) {
    match ty {
        model::Type::ImplTrait { .. } => found.push(ty),
        model::Type::Definition { arguments, .. } => {
            for argument in arguments.iter() {
                if let model::Argument::Type(ty) = argument {
                    impl_traits(ty, found);
                }
            }
        }
        model::Type::Reference { ty, .. } | model::Type::Slice(ty) => impl_traits(ty, found),
        model::Type::Array { element, .. } => impl_traits(element, found),
        model::Type::Tuple(members) => {
            for member in members.iter() {
                impl_traits(member, found);
            }
        }
        _ => {}
    }
}

/// `ty` with the trait `trait_id`'s `Self` its parameter 0, as a trait's
/// method is a function of its `Self` (`classify_self_member`).
/// Whether each type parameter the inputs and output of `plan`'s signature
/// name is one the signature declares.
fn declares_its_parameters(plan: &representability::Representability) -> bool {
    use representability::Representability;
    fn declared(
        ty: &representability::convert::Type,
        parameters: &[representability::signature::Parameter],
    ) -> bool {
        use representability::convert::Type;
        match ty {
            Type::Parameter(index) => parameters
                .iter()
                .any(|parameter| parameter.index() == *index),
            Type::Unit | Type::Never | Type::Builtin(_) | Type::Inferred => true,
            Type::Nominal { arguments, .. } | Type::Script { arguments, .. } => arguments
                .iter()
                .all(|argument| declared(argument, parameters)),
            Type::Tuple(members) => members.iter().all(|member| declared(member, parameters)),
            Type::Option(element)
            | Type::Range(element)
            | Type::Borrowed { ty: element, .. }
            | Type::Sequence { element, .. }
            | Type::ProfileMessage {
                payload: element, ..
            } => declared(element, parameters),
            Type::ConstArray { element, parameter } => {
                parameters
                    .iter()
                    .any(|declared| declared.index() == *parameter)
                    && declared(element, parameters)
            }
            Type::Result(ok, err) => declared(ok, parameters) && declared(err, parameters),
            Type::FunctionItem { input, output } => {
                declared(input, parameters) && declared(output, parameters)
            }
        }
    }
    let (signature, item) = match plan {
        Representability::Function(plan) => (plan.signature(), None),
        Representability::Method(plan) => (plan.signature(), None),
        Representability::FunctionItem(item) => (item.signature(), Some(item)),
        Representability::Invocation(plan) => (plan.item().signature(), Some(plan.item())),
        _ => return true,
    };
    let parameters = signature.parameters();
    // A callable parameter's inputs and output, too, as rayon's
    // `ParallelIterator::any<P: Fn(Self::Item) -> bool>` takes a closure of
    // the item, which an opaque type of `from_par_iter<I:
    // IntoParallelIterator<Item = T>>` sets to the trait's `T`.
    let calls = item
        .into_iter()
        .flat_map(|item| item.call().into_iter().chain(item.crossings()))
        .flat_map(|call| call.inputs().iter().chain(std::iter::once(call.output())));
    signature
        .inputs()
        .iter()
        .chain(std::iter::once(signature.output()))
        .chain(calls)
        .all(|ty| declared(ty, parameters))
}

/// Whether `id` is the opaque type of a trait method's type parameter or
/// of one of its associated types (`parameter_opaque`).
fn parameter_opaque_id(id: &api::identity::Definition) -> bool {
    id.def_path_hash().contains('#')
}

/// Whether `signature` names `Self` of the trait `trait_id`.
fn names_self(signature: &model::Signature, trait_id: &api::identity::Definition) -> bool {
    let found = std::cell::Cell::new(false);
    let _ = map_signature_types(signature, |ty| {
        if type_names_self(ty, trait_id) {
            found.set(true);
        }
        ty.clone()
    });
    found.get()
}

/// Whether `ty` names `Self` of the trait `trait_id`, as `Self` or as the
/// trait's parameter 0.
fn type_names_self(ty: &model::Type, trait_id: &api::identity::Definition) -> bool {
    let this = model::Type::Parameter {
        owner: trait_id.clone(),
        index: 0,
    };
    self_replaced(ty, trait_id, &model::Type::Never) != *ty || type_names(ty, &this)
}

/// Whether `ty` names `target` at any depth.
fn type_names(ty: &model::Type, target: &model::Type) -> bool {
    if ty == target {
        return true;
    }
    let replaced = substitute_parameters(
        ty,
        &[(
            match target {
                model::Type::Parameter { owner, index } => (owner.clone(), *index),
                _ => return false,
            },
            model::Type::Never,
        )],
    );
    replaced != *ty
}

/// The opaque type the type parameter `parameter` of `method`, whose
/// generics are `generics`, is where native code calls a program's impl of
/// the method (`parameter_members`): bounded as the parameter is.
///
/// A projection its bounds set to a type that names the method's `Self`,
/// as image's `copy_from<O: GenericImageView<Pixel = Self::Pixel>>` sets
/// `O::Pixel`, stays open: that `Self` is the impl's type, which the opaque
/// type's members don't have, and inside them `Self` is the opaque type
/// itself. The projection is then an opaque type of its own
/// (`opaque_projection`), bounded as the trait declares it.
fn parameter_opaque(
    method: &model::Definition,
    parameter: &model::Parameter,
    generics: &model::Generics,
) -> model::Type {
    let this = model::Type::Parameter {
        owner: parameter.owner().clone(),
        index: parameter.index(),
    };
    let names = |ty: &model::Type| match ty {
        model::Type::Projection { arguments, .. } => {
            matches!(arguments.first(), Some(model::Argument::Type(first)) if *first == this)
        }
        ty => *ty == this,
    };
    let owner = match method.owner() {
        model::Owner::Definition(owner) => Some(owner),
        _ => None,
    };
    let names_owner = |ty: &model::Type| owner.is_some_and(|owner| type_names_self(ty, owner));
    let mut bounds = generics
        .predicates()
        .filter(|predicate| match predicate {
            model::Predicate::Bound { ty, .. } => *ty == this,
            model::Predicate::ProjectionEquality { projection, ty } => {
                names(projection) && !names_owner(ty)
            }
            _ => false,
        })
        .cloned()
        .collect::<Vec<_>>();
    if let model::ParameterKind::Type {
        bounds: declared, ..
    } = parameter.kind()
    {
        for bound in declared.iter() {
            let predicate = model::Predicate::Bound {
                ty: this.clone(),
                bound: bound.clone(),
            };
            if !bounds.contains(&predicate) {
                bounds.push(predicate);
            }
        }
    }
    model::Type::ImplTrait {
        id: api::identity::Definition::new(
            method.id().unit().clone(),
            format!("{}#{}", method.id().def_path_hash(), parameter.index()).into(),
            api::identity::Namespace::Type,
        ),
        bounds: bounds.into_boxed_slice(),
        captures: Box::default(),
    }
}

fn self_as_parameter(ty: &model::Type, trait_id: &api::identity::Definition) -> model::Type {
    self_replaced(
        ty,
        trait_id,
        &model::Type::Parameter {
            owner: trait_id.clone(),
            index: 0,
        },
    )
}

/// `ty` with the trait `trait_id`'s `Self` the type `this`: a parameter
/// (`self_as_parameter`), or the one type an impl's methods take it as, as
/// an opaque alias names it (`classify_alias_member`).
fn self_replaced(
    ty: &model::Type,
    trait_id: &api::identity::Definition,
    this: &model::Type,
) -> model::Type {
    let inner = |ty: &model::Type| Box::new(self_replaced(ty, trait_id, this));
    let arguments = |arguments: &[model::Argument]| {
        arguments
            .iter()
            .map(|argument| match argument {
                model::Argument::Type(ty) => {
                    model::Argument::Type(self_replaced(ty, trait_id, this))
                }
                other => other.clone(),
            })
            .collect()
    };
    match ty {
        model::Type::SelfType(owner) if owner == trait_id => this.clone(),
        model::Type::Definition { id, arguments: of } => model::Type::Definition {
            id: id.clone(),
            arguments: arguments(of),
        },
        model::Type::Projection {
            trait_id: projected,
            associated,
            arguments: of,
        } => model::Type::Projection {
            trait_id: projected.clone(),
            associated: associated.clone(),
            arguments: arguments(of),
        },
        model::Type::Tuple(members) => model::Type::Tuple(
            members
                .iter()
                .map(|member| self_replaced(member, trait_id, this))
                .collect(),
        ),
        model::Type::Slice(element) => model::Type::Slice(inner(element)),
        model::Type::Array { element, length } => model::Type::Array {
            element: inner(element),
            length: length.clone(),
        },
        model::Type::Reference {
            lifetime,
            mutability,
            ty,
        } => model::Type::Reference {
            lifetime: lifetime.clone(),
            mutability: *mutability,
            ty: inner(ty),
        },
        model::Type::RawPointer { mutability, ty } => model::Type::RawPointer {
            mutability: *mutability,
            ty: inner(ty),
        },
        // The opaque type a trait's method gives captures `Self`, as
        // iced's `Widget::explain` does.
        model::Type::ImplTrait {
            id,
            bounds,
            captures,
        } => model::Type::ImplTrait {
            id: id.clone(),
            bounds: bounds.clone(),
            captures: arguments(captures),
        },
        other => other.clone(),
    }
}

/// `predicate` with the trait `trait_id`'s `Self` its parameter 0
/// (`self_as_parameter`).
fn self_predicate_as_parameter(
    predicate: &model::Predicate,
    trait_id: &api::identity::Definition,
) -> model::Predicate {
    map_predicate_types(predicate, |ty| self_as_parameter(ty, trait_id))
}

fn map_predicate_types(
    predicate: &model::Predicate,
    ty: impl Fn(&model::Type) -> model::Type,
) -> model::Predicate {
    match predicate {
        model::Predicate::Bound { ty: bounded, bound } => model::Predicate::Bound {
            ty: ty(bounded),
            bound: map_bound_types(bound, &ty),
        },
        model::Predicate::ProjectionEquality {
            projection,
            ty: equal,
        } => model::Predicate::ProjectionEquality {
            projection: ty(projection),
            ty: ty(equal),
        },
        model::Predicate::TypeOutlives {
            ty: outliving,
            lifetime,
        } => model::Predicate::TypeOutlives {
            ty: ty(outliving),
            lifetime: lifetime.clone(),
        },
        other => other.clone(),
    }
}

/// `bound` with each type its trait's arguments name mapped by `ty`.
fn map_bound_types(bound: &model::Bound, ty: impl Fn(&model::Type) -> model::Type) -> model::Bound {
    match bound {
        model::Bound::Trait {
            trait_id,
            arguments,
            polarity,
            binders,
        } => model::Bound::Trait {
            trait_id: trait_id.clone(),
            arguments: arguments
                .iter()
                .map(|argument| match argument {
                    model::Argument::Type(value) => model::Argument::Type(ty(value)),
                    other => other.clone(),
                })
                .collect(),
            polarity: *polarity,
            binders: binders.clone(),
        },
        other => other.clone(),
    }
}

/// `generics` with each type its parameters' bounds and its predicates
/// name mapped by `ty`.
fn map_generics_types(
    generics: &model::Generics,
    ty: impl Fn(&model::Type) -> model::Type,
) -> model::Generics {
    let parameters = generics
        .parameters()
        .map(|parameter| match parameter.kind() {
            model::ParameterKind::Type { bounds, default } => model::Parameter::new(
                parameter.owner().clone(),
                parameter.index(),
                parameter.name().into(),
                model::ParameterKind::Type {
                    bounds: bounds
                        .iter()
                        .map(|bound| map_bound_types(bound, &ty))
                        .collect(),
                    default: default.clone(),
                },
            ),
            _ => parameter.clone(),
        })
        .collect();
    model::Generics::new(
        parameters,
        generics
            .predicates()
            .map(|predicate| map_predicate_types(predicate, &ty))
            .collect(),
    )
}

/// Match an impl's self type to an alias's exact instance. Repeated
/// parameters must agree and fixed arguments must match; unrelated impls
/// are excluded before their methods become members of the alias.
fn instance_substitutions(
    declared: &model::Type,
    actual: &model::Type,
    implementation: &api::identity::Definition,
) -> Option<Vec<((api::identity::Definition, u32), model::Type)>> {
    fn collect(
        declared: &model::Type,
        actual: &model::Type,
        implementation: &api::identity::Definition,
        substitutions: &mut Vec<((api::identity::Definition, u32), model::Type)>,
    ) -> bool {
        match (declared, actual) {
            (model::Type::Parameter { owner, index }, actual) if owner == implementation => {
                if let Some((_, previous)) = substitutions
                    .iter()
                    .find(|((id, at), _)| id == owner && at == index)
                {
                    previous == actual
                } else {
                    substitutions.push(((owner.clone(), *index), actual.clone()));
                    true
                }
            }
            (
                model::Type::Definition {
                    id: left,
                    arguments: declared,
                },
                model::Type::Definition {
                    id: right,
                    arguments: actual,
                },
            ) if left == right && declared.len() == actual.len() => declared
                .iter()
                .zip(actual)
                .all(|(declared, actual)| match (declared, actual) {
                    (model::Argument::Type(declared), model::Argument::Type(actual)) => {
                        collect(declared, actual, implementation, substitutions)
                    }
                    _ => declared == actual,
                }),
            (model::Type::Tuple(declared), model::Type::Tuple(actual))
                if declared.len() == actual.len() =>
            {
                declared.iter().zip(actual).all(|(declared, actual)| {
                    collect(declared, actual, implementation, substitutions)
                })
            }
            _ => declared == actual,
        }
    }
    let mut substitutions = Vec::new();
    collect(declared, actual, implementation, &mut substitutions).then_some(substitutions)
}

fn substituted_signature(
    signature: &model::Signature,
    substitutions: &[((api::identity::Definition, u32), model::Type)],
) -> model::Signature {
    map_signature_types(signature, |ty| substitute_parameters(ty, substitutions))
}

/// `signature` with each of its types mapped by `substitute`.
fn map_signature_types(
    signature: &model::Signature,
    substitute: impl Fn(&model::Type) -> model::Type,
) -> model::Signature {
    let receiver = match signature.receiver() {
        model::Receiver::None => model::Receiver::None,
        model::Receiver::Value { ty } => model::Receiver::Value { ty: substitute(ty) },
        model::Receiver::Reference {
            lifetime,
            mutability,
            ty,
        } => model::Receiver::Reference {
            lifetime: lifetime.clone(),
            mutability: *mutability,
            ty: substitute(ty),
        },
        model::Receiver::Explicit { ty } => model::Receiver::Explicit { ty: substitute(ty) },
    };
    model::Signature::new(
        signature.safety(),
        signature.const_function(),
        signature.async_function(),
        signature.abi().into(),
        signature.variadic(),
        signature.binders().cloned().collect(),
        receiver,
        signature.inputs().map(&substitute).collect(),
        substitute(signature.output()),
    )
    .with_guard(signature.guard().cloned())
}

/// `ty` with each impl parameter in `substitutions` replaced.
fn substitute_parameters(
    ty: &model::Type,
    substitutions: &[((api::identity::Definition, u32), model::Type)],
) -> model::Type {
    let argument = |argument: &model::Argument| match argument {
        model::Argument::Type(ty) => {
            model::Argument::Type(substitute_parameters(ty, substitutions))
        }
        other => other.clone(),
    };
    match ty {
        model::Type::Parameter { owner, index } => substitutions
            .iter()
            .find(|((candidate, at), _)| candidate == owner && at == index)
            .map_or_else(|| ty.clone(), |(_, replacement)| replacement.clone()),
        model::Type::Definition { id, arguments } => model::Type::Definition {
            id: id.clone(),
            arguments: arguments.iter().map(argument).collect(),
        },
        model::Type::Tuple(members) => model::Type::Tuple(
            members
                .iter()
                .map(|member| substitute_parameters(member, substitutions))
                .collect(),
        ),
        model::Type::Slice(element) => {
            model::Type::Slice(Box::new(substitute_parameters(element, substitutions)))
        }
        model::Type::Reference {
            lifetime,
            mutability,
            ty,
        } => model::Type::Reference {
            lifetime: lifetime.clone(),
            mutability: *mutability,
            ty: Box::new(substitute_parameters(ty, substitutions)),
        },
        model::Type::Projection {
            trait_id,
            associated,
            arguments,
        } => model::Type::Projection {
            trait_id: trait_id.clone(),
            associated: associated.clone(),
            arguments: arguments.iter().map(argument).collect(),
        },
        // What an opaque type captures, as `Widget::explain`'s captures the
        // trait's parameters, which a provided method takes at its impl's.
        // Its bounds name its own parameters.
        model::Type::ImplTrait {
            id,
            bounds,
            captures,
        } => model::Type::ImplTrait {
            id: id.clone(),
            bounds: bounds.clone(),
            captures: captures.iter().map(argument).collect(),
        },
        other => other.clone(),
    }
}

/// `predicate` with each parameter in `substitutions` replaced, as a
/// provided method's `F: Fn(Message) -> B` is at the indices its signature
/// takes in an impl (T417).
fn substituted_predicate(
    predicate: &model::Predicate,
    substitutions: &[((api::identity::Definition, u32), model::Type)],
) -> model::Predicate {
    mapped_predicate(predicate, &|ty| substitute_parameters(ty, substitutions))
}

/// `predicate` with each type it names mapped by `substitute`.
fn mapped_predicate(
    predicate: &model::Predicate,
    substitute: &dyn Fn(&model::Type) -> model::Type,
) -> model::Predicate {
    match predicate {
        model::Predicate::Bound { ty, bound } => model::Predicate::Bound {
            ty: substitute(ty),
            bound: match bound {
                model::Bound::Trait {
                    trait_id,
                    arguments,
                    polarity,
                    binders,
                } => model::Bound::Trait {
                    trait_id: trait_id.clone(),
                    arguments: arguments
                        .iter()
                        .map(|argument| match argument {
                            model::Argument::Type(ty) => model::Argument::Type(substitute(ty)),
                            other => other.clone(),
                        })
                        .collect(),
                    polarity: *polarity,
                    binders: binders.clone(),
                },
                other => other.clone(),
            },
        },
        model::Predicate::ProjectionEquality { projection, ty } => {
            model::Predicate::ProjectionEquality {
                projection: substitute(projection),
                ty: substitute(ty),
            }
        }
        model::Predicate::TypeOutlives { ty, lifetime } => model::Predicate::TypeOutlives {
            ty: substitute(ty),
            lifetime: lifetime.clone(),
        },
        other => other.clone(),
    }
}

fn exact_definition(ty: &model::Type, id: &api::identity::Definition) -> bool {
    matches!(ty, model::Type::Definition { id: actual, arguments } if actual == id
    && arguments.iter().all(|argument| matches!(
        argument,
        model::Argument::Lifetime(model::Lifetime::Static | model::Lifetime::Early { .. })
    )))
}

fn is_lifetime_parameter(parameter: &model::Parameter) -> bool {
    matches!(parameter.kind(), model::ParameterKind::Lifetime { .. })
}

/// How many lifetime parameters `generics` declares.
/// Whether a type is covariant in each lifetime it declares, as rustc's
/// variance says.
fn covariant_lifetimes(generics: &model::Generics) -> bool {
    generics
        .parameters()
        .filter(|parameter| is_lifetime_parameter(parameter))
        .all(model::Parameter::covariant)
}

/// The positions, among a type's type parameters, of those a field's
/// boxed callable gives, as `view: Box<dyn Fn(&D) -> W>` gives `W` of
/// iced's `Lazy<'a, W, D>`.
fn given_parameters(generics: &model::Generics) -> Vec<u32> {
    generics
        .parameters()
        .filter(|parameter| !is_lifetime_parameter(parameter))
        .zip(0..)
        .filter(|(parameter, _)| parameter.given())
        .map(|(_, position)| position)
        .collect()
}

fn lifetime_parameters(generics: &model::Generics) -> u32 {
    u32::try_from(
        generics
            .parameters()
            .filter(|parameter| is_lifetime_parameter(parameter))
            .count(),
    )
    .unwrap_or(u32::MAX)
}

/// Generics that declare only lifetimes, each instantiated at `'static`
/// (D558 tier 0), and bound nothing but lifetimes.
fn lifetime_only(generics: &model::Generics) -> bool {
    generics.parameters().all(is_lifetime_parameter)
        && generics
            .predicates()
            .all(|predicate| matches!(predicate, model::Predicate::LifetimeOutlives { .. }))
}

/// Whether `ty` holds a reference at an early-bound lifetime, which
/// `'static` would make a `'static` borrow.
/// The target of a type argument `&'a [T]` of scalars, or `&'a str`, at an
/// early or `'static` lifetime: `Reader<&'a [u8]>`'s.
fn borrowed_argument(ty: &model::Type) -> Option<&model::Type> {
    let model::Type::Reference {
        lifetime: model::Lifetime::Early { .. } | model::Lifetime::Static,
        mutability: model::Mutability::Shared,
        ..
    } = ty
    else {
        return None;
    };
    lent_target(ty)
}

/// As `borrowed_argument`, of a late-bound lifetime: a scope frame's,
/// which the receiver it holds borrowed outlives.
fn late_borrowed_argument(ty: &model::Type) -> Option<&model::Type> {
    let model::Type::Reference {
        lifetime: model::Lifetime::Late { .. },
        mutability: model::Mutability::Shared,
        ..
    } = ty
    else {
        return None;
    };
    lent_target(ty)
}

/// What the shared reference `ty` lends when it is a slice of scalars or a
/// `str`.
fn lent_target(ty: &model::Type) -> Option<&model::Type> {
    let model::Type::Reference { ty, .. } = ty else {
        return None;
    };
    match &**ty {
        model::Type::Slice(element) if matches!(&**element, model::Type::Primitive(_)) => Some(ty),
        model::Type::Primitive(name) if &**name == "str" => Some(ty),
        _ => None,
    }
}

/// Whether `ty` names an early-bound lifetime, as `Parser<'input>` does.
fn names_early_lifetime(ty: &model::Type) -> bool {
    let argument = |argument: &model::Argument| match argument {
        model::Argument::Type(ty) => names_early_lifetime(ty),
        model::Argument::Lifetime(lifetime) => matches!(lifetime, model::Lifetime::Early { .. }),
        model::Argument::Const(_) => false,
    };
    match ty {
        model::Type::Reference { lifetime, ty, .. } => {
            matches!(lifetime, model::Lifetime::Early { .. }) || names_early_lifetime(ty)
        }
        model::Type::Definition { arguments, .. } => arguments.iter().any(argument),
        model::Type::Tuple(types) => types.iter().any(names_early_lifetime),
        model::Type::Slice(ty) => names_early_lifetime(ty),
        model::Type::Array { element, .. } | model::Type::RawPointer { ty: element, .. } => {
            names_early_lifetime(element)
        }
        model::Type::Projection { arguments, .. } => arguments.iter().any(argument),
        model::Type::FunctionPointer(_)
        | model::Type::Primitive(_)
        | model::Type::Parameter { .. }
        | model::Type::SelfType(_)
        | model::Type::Never
        | model::Type::TraitObject { .. }
        | model::Type::ImplTrait { .. } => false,
    }
}

/// Whether `ty` holds a `&mut` at an early-bound lifetime.
fn borrows_mutably_at_early_lifetime(ty: &model::Type) -> bool {
    let argument = |argument: &model::Argument| match argument {
        model::Argument::Type(ty) => borrows_mutably_at_early_lifetime(ty),
        model::Argument::Lifetime(_) | model::Argument::Const(_) => false,
    };
    match ty {
        model::Type::Reference {
            lifetime,
            mutability,
            ty,
        } => {
            matches!(
                (lifetime, mutability),
                (model::Lifetime::Early { .. }, model::Mutability::Mutable)
            ) || borrows_mutably_at_early_lifetime(ty)
        }
        model::Type::Definition { arguments, .. } => arguments.iter().any(argument),
        model::Type::Tuple(types) => types.iter().any(borrows_mutably_at_early_lifetime),
        model::Type::Slice(ty) => borrows_mutably_at_early_lifetime(ty),
        model::Type::Array { element, .. } | model::Type::RawPointer { ty: element, .. } => {
            borrows_mutably_at_early_lifetime(element)
        }
        model::Type::FunctionPointer(_)
        | model::Type::Primitive(_)
        | model::Type::Parameter { .. }
        | model::Type::SelfType(_)
        | model::Type::Never
        | model::Type::TraitObject { .. }
        | model::Type::ImplTrait { .. }
        | model::Type::Projection { .. } => false,
    }
}

/// The inputs of `signature`, by position, written as a shared reference
/// at `'static`, as `State::new(blocks: &'static [Block])` takes one: Rust
/// fills it only with a value that lives as long as the program
/// (`Signature::statics`).
fn static_inputs(signature: &model::Signature) -> Box<[u32]> {
    signature
        .inputs()
        .enumerate()
        .filter(|(_, input)| {
            matches!(
                input,
                model::Type::Reference {
                    mutability: model::Mutability::Shared,
                    lifetime: model::Lifetime::Static,
                    ..
                }
            )
        })
        .filter_map(|(index, _)| u32::try_from(index).ok())
        .collect()
}

fn borrows_at_early_lifetime(ty: &model::Type) -> bool {
    let argument = |argument: &model::Argument| match argument {
        model::Argument::Type(ty) => borrows_at_early_lifetime(ty),
        model::Argument::Lifetime(_) | model::Argument::Const(_) => false,
    };
    match ty {
        model::Type::Reference { lifetime, ty, .. } => {
            matches!(lifetime, model::Lifetime::Early { .. }) || borrows_at_early_lifetime(ty)
        }
        model::Type::Definition { arguments, .. } => arguments.iter().any(argument),
        model::Type::Tuple(types) => types.iter().any(borrows_at_early_lifetime),
        model::Type::Slice(ty) => borrows_at_early_lifetime(ty),
        model::Type::Array { element, .. } | model::Type::RawPointer { ty: element, .. } => {
            borrows_at_early_lifetime(element)
        }
        model::Type::FunctionPointer(signature) => {
            signature.inputs().any(borrows_at_early_lifetime)
                || borrows_at_early_lifetime(signature.output())
        }
        model::Type::Primitive(_)
        | model::Type::Parameter { .. }
        | model::Type::SelfType(_)
        | model::Type::Never
        | model::Type::TraitObject { .. }
        | model::Type::ImplTrait { .. }
        | model::Type::Projection { .. } => false,
    }
}

/// Whether `ty`, an implementation's self type, is the definition `id`,
/// whose `generics` are given, with nothing but lifetimes filled in, or
/// type arguments that are exactly their parameters' declared defaults.
fn own_form(
    ty: &model::Type,
    id: &api::identity::Definition,
    generics: Option<&model::Generics>,
) -> bool {
    let model::Type::Definition {
        id: actual,
        arguments,
    } = ty
    else {
        return false;
    };
    actual == id
        && arguments
            .iter()
            .enumerate()
            .all(|(position, argument)| match argument {
                model::Argument::Lifetime(_) => true,
                model::Argument::Type(argument) => generics
                    .and_then(|generics| generics.parameters().nth(position))
                    .is_some_and(|parameter| {
                        matches!(
                            parameter.kind(),
                            model::ParameterKind::Type {
                                default: model::Default::Present(default),
                                ..
                            } if default == argument
                        )
                    }),
                model::Argument::Const(_) => false,
            })
}

/// `ty` is the definition `id` whose type arguments are all generic
/// parameters, as the self type of a blanket `impl<T> Trait for Type<T>`.
fn any_arguments(ty: &model::Type, id: &api::identity::Definition) -> bool {
    matches!(ty, model::Type::Definition { id: actual, arguments } if actual == id
    && arguments.iter().all(|argument| match argument {
        model::Argument::Type(ty) => matches!(ty, model::Type::Parameter { .. }),
        model::Argument::Lifetime(_) | model::Argument::Const(_) => true,
    }))
}

fn definition_is_public(definition: &model::Definition) -> Result<(), Failure> {
    if matches!(definition.visibility(), model::Visibility::Public) {
        Ok(())
    } else {
        Err(representability::reject::Kind::PrivateType.into())
    }
}

fn definition_is_publishable_nominal(definition: &model::Definition) -> Result<(), Failure> {
    if !definition.display_path().all(facade_segment_nameable) {
        return if definition
            .display_path()
            .next()
            .is_some_and(|segment| segment.starts_with("impl "))
        {
            Err(representability::reject::Kind::OpaqueType.into())
        } else {
            Err(representability::reject::Kind::UnnameableType.into())
        };
    }
    match definition.kind() {
        model::Kind::Struct { .. } | model::Kind::Enum { .. } => Ok(()),
        _ => Err(representability::reject::Kind::UnnameableType.into()),
    }
}

fn facade_segment_nameable(segment: &str) -> bool {
    let mut first_alphabetic = false;
    for ch in segment.chars() {
        match ch {
            '_' => {}
            _ if ch.is_ascii_alphabetic() => first_alphabetic = true,
            _ if !first_alphabetic => return false,
            _ if ch.is_ascii_alphanumeric() => {}
            _ => return false,
        }
    }
    first_alphabetic
}

fn receiver(kind: &model::Kind) -> Option<representability::ownership::Receiver> {
    let model::Kind::Function { signature, .. } = kind else {
        return None;
    };
    match signature.receiver() {
        model::Receiver::None | model::Receiver::Explicit { .. } => None,
        model::Receiver::Value { .. } => Some(representability::ownership::Receiver::Owned),
        model::Receiver::Reference { mutability, .. } => Some(match mutability {
            model::Mutability::Shared => representability::ownership::Receiver::Shared,
            model::Mutability::Mutable => representability::ownership::Receiver::Mutable,
        }),
    }
}

/// Whether `definition`, a member of `implementation`, is declared by the
/// trait it implements rather than by the impl itself.
fn trait_member(implementation: &model::Implementation, definition: &model::Definition) -> bool {
    matches!(
        (definition.owner(), implementation.trait_ref()),
        (model::Owner::Definition(owner), model::Trait::Trait { id, .. }) if owner == id
    )
}

fn export_name_ref(export: &api::identity::Export) -> &str {
    export.path().last().unwrap_or(export.root_alias())
}

fn selector(definition: &model::Definition) -> Result<crate::surface::identity::Selector, Error> {
    use crate::surface::identity::{Definition, Selector};
    use model::{Kind, Owner};

    let item = Definition::of(definition.id());
    Ok(match definition.kind() {
        Kind::Rejected { class, .. } => match class {
            model::RejectedClass::Declaration => Selector::Declaration,
            model::RejectedClass::Function => Selector::Function { item },
            model::RejectedClass::Constant => Selector::Constant { item },
            model::RejectedClass::Method(receiver) => Selector::Method {
                item,
                receiver: match receiver {
                    model::MethodReceiver::Owned => representability::ownership::Receiver::Owned,
                    model::MethodReceiver::Shared => representability::ownership::Receiver::Shared,
                    model::MethodReceiver::Mutable => {
                        representability::ownership::Receiver::Mutable
                    }
                },
            },
            model::RejectedClass::AssociatedFunction => Selector::AssociatedFunction { item },
            model::RejectedClass::AssociatedConstant => Selector::AssociatedConstant { item },
            model::RejectedClass::Unavailable => Selector::Unavailable,
        },
        Kind::Function { .. } => match (definition.owner(), receiver(definition.kind())) {
            (_, Some(receiver)) => Selector::Method { item, receiver },
            (Owner::Implementation(_), None) | (Owner::Definition(_), None) => {
                Selector::AssociatedFunction { item }
            }
            (Owner::Free, None) => Selector::Function { item },
        },
        Kind::Const { .. } => match definition.owner() {
            Owner::Implementation(_) | Owner::Definition(_) => {
                Selector::AssociatedConstant { item }
            }
            Owner::Free => Selector::Constant { item },
        },
        Kind::Struct { .. } | Kind::Enum { .. } => Selector::Declaration,
        Kind::Static {
            mutable: false,
            thread_local: false,
            ..
        } if matches!(definition.owner(), Owner::Free) => Selector::Constant { item },
        Kind::Module
        | Kind::Union { .. }
        | Kind::Static { .. }
        | Kind::TypeConst { .. }
        | Kind::TypeAlias { .. }
        | Kind::Trait { .. }
        | Kind::AssociatedType { .. }
        | Kind::Sysroot { .. }
        | Kind::MacroRules { .. } => Selector::Unavailable,
    })
}

/// A projection `W::Assoc` of a type parameter a signature names, or of a
/// projection of one, and the synthetic parameter that stands for it.
struct Projected {
    parameter: u32,
    /// Each projection from the parameter out: `W::Assoc` is one, and
    /// `<<S as RgbStandard>::Space as RgbSpace>::WhitePoint` is two.
    hops: Box<[Hop]>,
    synthetic: u32,
}

/// One projection of a [`Projected`] chain: the trait, its associated
/// type and that type's name.
struct Hop {
    trait_id: api::identity::Definition,
    associated: api::identity::Definition,
    name: String,
}

/// Each projection of a chain from its parameter out, by trait and
/// associated type.
type Chain = Vec<(api::identity::Definition, api::identity::Definition)>;

/// The type parameter a projection chain starts at, and each projection in
/// it from the parameter out: `<<S as RgbStandard>::Space as RgbSpace>::
/// WhitePoint` is `S`, then `RgbStandard::Space`, then `RgbSpace::
/// WhitePoint`. A parameter alone has no projection.
fn projection_chain(ty: &model::Type) -> Option<(u32, Chain)> {
    match ty {
        model::Type::Parameter { index, .. } => Some((*index, Vec::new())),
        model::Type::Projection {
            trait_id,
            associated,
            arguments,
        } => {
            let Some(model::Argument::Type(this)) = arguments.first() else {
                return None;
            };
            let (index, mut chain) = projection_chain(this)?;
            chain.push((trait_id.clone(), associated.clone()));
            Some((index, chain))
        }
        _ => None,
    }
}

/// The indices of `owner`'s type parameters `ty` names, wherever they are
/// inside it, added to `found` once each.
fn named_parameters(ty: &model::Type, owner: &api::identity::Definition, found: &mut Vec<u32>) {
    match ty {
        model::Type::Parameter {
            owner: named,
            index,
        } => {
            if named == owner && !found.contains(index) {
                found.push(*index);
            }
        }
        model::Type::Definition { arguments, .. } | model::Type::Projection { arguments, .. } => {
            for argument in arguments.iter() {
                if let model::Argument::Type(ty) = argument {
                    named_parameters(ty, owner, found);
                }
            }
        }
        model::Type::Tuple(members) => {
            for member in members.iter() {
                named_parameters(member, owner, found);
            }
        }
        model::Type::Reference { ty, .. }
        | model::Type::Slice(ty)
        | model::Type::Array { element: ty, .. } => named_parameters(ty, owner, found),
        _ => {}
    }
}

/// Whether `ty` names a type parameter or a projection anywhere in it.
fn names_type_parameter(ty: &model::Type) -> bool {
    let argument = |argument: &model::Argument| match argument {
        model::Argument::Type(ty) => names_type_parameter(ty),
        _ => false,
    };
    match ty {
        model::Type::Parameter { .. }
        | model::Type::SelfType(_)
        | model::Type::Projection { .. } => true,
        model::Type::Definition { arguments, .. } => arguments.iter().any(argument),
        model::Type::Tuple(members) => members.iter().any(names_type_parameter),
        model::Type::Reference { ty, .. }
        | model::Type::RawPointer { ty, .. }
        | model::Type::Slice(ty)
        | model::Type::Array { element: ty, .. } => names_type_parameter(ty),
        _ => false,
    }
}

/// Whether the concrete `ty` is an instance of `pattern`, an
/// implementation's self type, binding each of its type parameters by
/// index. Lifetimes are not compared.
fn bind_instance(
    pattern: &model::Type,
    ty: &model::Type,
    bindings: &mut BTreeMap<u32, model::Type>,
) -> bool {
    let arguments = |patterns: &[model::Argument],
                     arguments: &[model::Argument],
                     bindings: &mut BTreeMap<u32, model::Type>| {
        patterns.len() == arguments.len()
            && patterns
                .iter()
                .zip(arguments)
                .all(|(pattern, argument)| match (pattern, argument) {
                    (model::Argument::Type(pattern), model::Argument::Type(argument)) => {
                        bind_instance(pattern, argument, bindings)
                    }
                    (model::Argument::Lifetime(_), model::Argument::Lifetime(_)) => true,
                    (pattern, argument) => pattern == argument,
                })
    };
    match (pattern, ty) {
        (model::Type::Parameter { index, .. }, ty) => match bindings.get(index) {
            Some(bound) => bound == ty,
            None => {
                bindings.insert(*index, ty.clone());
                true
            }
        },
        (
            model::Type::Definition {
                id,
                arguments: patterns,
            },
            model::Type::Definition {
                id: other,
                arguments: actual,
            },
        ) => id == other && arguments(patterns, actual, bindings),
        (
            model::Type::Reference {
                mutability,
                ty: pattern,
                ..
            },
            model::Type::Reference {
                mutability: other,
                ty,
                ..
            },
        ) => mutability == other && bind_instance(pattern, ty, bindings),
        (model::Type::Tuple(patterns), model::Type::Tuple(members)) => {
            patterns.len() == members.len()
                && patterns
                    .iter()
                    .zip(members.iter())
                    .all(|(pattern, member)| bind_instance(pattern, member, bindings))
        }
        (model::Type::Slice(pattern), model::Type::Slice(element)) => {
            bind_instance(pattern, element, bindings)
        }
        (
            model::Type::Array {
                element: pattern,
                length,
            },
            model::Type::Array {
                element,
                length: other,
            },
        ) => length == other && bind_instance(pattern, element, bindings),
        (pattern, ty) => pattern == ty,
    }
}

/// `ty` with each type parameter `bindings` binds replaced by its type.
pub(crate) fn substitute_bindings(
    ty: &model::Type,
    bindings: &BTreeMap<u32, model::Type>,
) -> model::Type {
    let argument = |argument: &model::Argument| match argument {
        model::Argument::Type(ty) => model::Argument::Type(substitute_bindings(ty, bindings)),
        other => other.clone(),
    };
    match ty {
        model::Type::Parameter { index, .. } => {
            bindings.get(index).cloned().unwrap_or_else(|| ty.clone())
        }
        model::Type::Definition { id, arguments } => model::Type::Definition {
            id: id.clone(),
            arguments: arguments.iter().map(argument).collect(),
        },
        model::Type::Projection {
            trait_id,
            associated,
            arguments,
        } => model::Type::Projection {
            trait_id: trait_id.clone(),
            associated: associated.clone(),
            arguments: arguments.iter().map(argument).collect(),
        },
        model::Type::Reference {
            lifetime,
            mutability,
            ty,
        } => model::Type::Reference {
            lifetime: lifetime.clone(),
            mutability: *mutability,
            ty: Box::new(substitute_bindings(ty, bindings)),
        },
        model::Type::Tuple(members) => model::Type::Tuple(
            members
                .iter()
                .map(|member| substitute_bindings(member, bindings))
                .collect(),
        ),
        model::Type::Slice(element) => {
            model::Type::Slice(Box::new(substitute_bindings(element, bindings)))
        }
        model::Type::Array { element, length } => model::Type::Array {
            element: Box::new(substitute_bindings(element, bindings)),
            length: length.clone(),
        },
        ty => ty.clone(),
    }
}

/// `ty` with each `from` inside it written as `to`.
fn type_replaced(ty: &model::Type, from: &model::Type, to: &model::Type) -> model::Type {
    if ty == from {
        return to.clone();
    }
    let inner = |ty: &model::Type| Box::new(type_replaced(ty, from, to));
    let arguments = |arguments: &[model::Argument]| {
        arguments
            .iter()
            .map(|argument| match argument {
                model::Argument::Type(ty) => model::Argument::Type(type_replaced(ty, from, to)),
                other => other.clone(),
            })
            .collect()
    };
    match ty {
        model::Type::Definition { id, arguments: of } => model::Type::Definition {
            id: id.clone(),
            arguments: arguments(of),
        },
        model::Type::Projection {
            trait_id,
            associated,
            arguments: of,
        } => model::Type::Projection {
            trait_id: trait_id.clone(),
            associated: associated.clone(),
            arguments: arguments(of),
        },
        model::Type::Tuple(members) => model::Type::Tuple(
            members
                .iter()
                .map(|member| type_replaced(member, from, to))
                .collect(),
        ),
        model::Type::Slice(element) => model::Type::Slice(inner(element)),
        model::Type::Array { element, length } => model::Type::Array {
            element: inner(element),
            length: length.clone(),
        },
        model::Type::Reference {
            lifetime,
            mutability,
            ty,
        } => model::Type::Reference {
            lifetime: lifetime.clone(),
            mutability: *mutability,
            ty: inner(ty),
        },
        model::Type::RawPointer { mutability, ty } => model::Type::RawPointer {
            mutability: *mutability,
            ty: inner(ty),
        },
        other => other.clone(),
    }
}

/// The owner of the type parameter a projection chain starts at.
fn projection_owner(ty: &model::Type) -> Option<&api::identity::Definition> {
    match ty {
        model::Type::Parameter { owner, .. } => Some(owner),
        model::Type::Projection { arguments, .. } => match arguments.first() {
            Some(model::Argument::Type(this)) => projection_owner(this),
            _ => None,
        },
        _ => None,
    }
}

/// `ty` with each projection of `projected` written as its synthetic
/// parameter, wherever it is inside `ty`.
fn projected_parameters(ty: &model::Type, projected: &[Projected]) -> model::Type {
    if projected.is_empty() {
        return ty.clone();
    }
    let argument = |argument: &model::Argument| match argument {
        model::Argument::Type(ty) => model::Argument::Type(projected_parameters(ty, projected)),
        other => other.clone(),
    };
    match ty {
        model::Type::Projection { .. } => {
            if let Some(owner) = projection_owner(ty)
                && let Some((index, chain)) = projection_chain(ty)
                && let Some(projection) = projected.iter().find(|projection| {
                    projection.parameter == index
                        && projection.hops.len() == chain.len()
                        && projection.hops.iter().zip(&chain).all(
                            |(hop, (trait_id, associated))| {
                                hop.trait_id == *trait_id && hop.associated == *associated
                            },
                        )
                })
            {
                return model::Type::Parameter {
                    owner: owner.clone(),
                    index: projection.synthetic,
                };
            }
            ty.clone()
        }
        model::Type::Reference {
            lifetime,
            mutability,
            ty: inner,
        } => model::Type::Reference {
            lifetime: lifetime.clone(),
            mutability: *mutability,
            ty: Box::new(projected_parameters(inner, projected)),
        },
        model::Type::Definition { id, arguments } => model::Type::Definition {
            id: id.clone(),
            arguments: arguments.iter().map(argument).collect(),
        },
        model::Type::Tuple(members) => model::Type::Tuple(
            members
                .iter()
                .map(|member| projected_parameters(member, projected))
                .collect(),
        ),
        model::Type::Slice(element) => {
            model::Type::Slice(Box::new(projected_parameters(element, projected)))
        }
        model::Type::Array { element, length } => model::Type::Array {
            element: Box::new(projected_parameters(element, projected)),
            length: length.clone(),
        },
        // What an opaque type's bounds bind, as `impl Program<State =
        // P::State>` of `.subscription(..)`.
        model::Type::ImplTrait {
            id,
            bounds,
            captures,
        } => model::Type::ImplTrait {
            id: id.clone(),
            bounds: bounds
                .iter()
                .map(|predicate| match predicate {
                    model::Predicate::ProjectionEquality { projection, ty } => {
                        model::Predicate::ProjectionEquality {
                            projection: projection.clone(),
                            ty: projected_parameters(ty, projected),
                        }
                    }
                    other => other.clone(),
                })
                .collect(),
            captures: captures.clone(),
        },
        ty => ty.clone(),
    }
}

/// The length of an array type, when it is a literal, as the driver writes
/// rustc's value (`2_usize`).
fn array_length(length: &model::Const) -> Option<u32> {
    let model::Const::Literal { value, .. } = length else {
        return None;
    };
    value
        .trim_end_matches("_usize")
        .trim_end_matches("usize")
        .parse()
        .ok()
}

fn constant(value: &model::Const) -> Result<representability::convert::Constant, Failure> {
    let model::Const::Literal { ty, value } = value else {
        return Err(representability::reject::Kind::UnevaluatedConstant.into());
    };
    if representability::convert::Builtin::primitive(ty).is_none() {
        return Err(representability::reject::Kind::UnsupportedConstant.into());
    }
    Ok(representability::convert::Constant::literal(
        ty.clone(),
        value.clone(),
    ))
}

/// `T` of core's `Option<T>`, which Rawr represents as its own `Option`.
pub(crate) fn option_element<'a>(
    definition: &model::Definition,
    arguments: &'a [model::Argument],
) -> Option<&'a model::Type> {
    std_binding::argument_of(definition, arguments, Std::Option, 0)
}

/// `T` and `E` of core's `Result<T, E>`, which Rawr represents as its own
/// `Result`.
pub(crate) fn result_elements<'a>(
    definition: &model::Definition,
    arguments: &'a [model::Argument],
) -> Option<(&'a model::Type, &'a model::Type)> {
    Some((
        std_binding::argument_of(definition, arguments, Std::Result, 0)?,
        std_binding::argument_of(definition, arguments, Std::Result, 1)?,
    ))
}

/// `T` of alloc's `Vec<T>`, with its allocator or without.
pub(crate) fn vec_element<'a>(
    definition: &model::Definition,
    arguments: &'a [model::Argument],
) -> Option<&'a model::Type> {
    std_binding::argument_of(definition, arguments, Std::Vec, 0)
}

/// Whether a value of `ty` becomes rawr's with no call's arena: it holds
/// no crate value, only rawr's builtins and std's types rawr holds as its
/// own, as a thread's `io::Result<()>` does.
fn crosses_by_value(ty: &representability::convert::Type) -> bool {
    use representability::convert::Type;
    match ty {
        Type::Unit | Type::Builtin(_) => true,
        Type::Option(element)
        | Type::Sequence {
            element,
            owned: true,
            ..
        } => crosses_by_value(element),
        Type::Result(ok, err) => crosses_by_value(ok) && crosses_by_value(err),
        Type::Tuple(members) => members.iter().all(crosses_by_value),
        Type::Nominal {
            definition,
            arguments,
        } => {
            use crate::surface::identity::Definition;
            arguments.is_empty()
                && (Definition::std_path(definition.as_bytes()).is_some()
                    || *definition == Definition::duration()
                    || *definition == Definition::instant())
        }
        _ => false,
    }
}

/// The element type of std's `Rc<[T]>`, `Arc<[T]>` or `Box<[T]>`, whose
/// elements an output copies out as a `&[T]`'s are.
fn shared_slice_element<'a>(
    definition: &model::Definition,
    arguments: &'a [model::Argument],
) -> Option<&'a model::Type> {
    if !matches!(
        std_binding::of(definition),
        Some(Std::Box | Std::Counted { .. })
    ) {
        return None;
    }
    match std_binding::type_argument(arguments, 0)? {
        model::Type::Slice(element) => Some(element),
        _ => None,
    }
}

/// Classification failure that cannot expose a partial contract.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("classification ledger cardinality changed")]
    Cardinality,
    #[error("canonical contract index exceeds the versioned u32 schema")]
    IndexOverflow,
    #[error("{resource} exceeded limit {limit} with observed size {actual}")]
    Limit {
        resource: &'static str,
        limit: usize,
        actual: usize,
    },
    #[error("cannot allocate canonical classified contract")]
    Allocation,
    #[error("cannot encode canonical classified contract")]
    Encoding,
    #[error("stored classified contract is not canonical")]
    Canonical,
    #[error("content-addressed generic classification failed")]
    Node(#[source] Box<crate::node_parts::Error>),
}

impl Write for Measured {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let Some(actual) = self.bytes.checked_add(buffer.len()) else {
            self.overflow = true;
            return Err(io::Error::other("canonical contract byte count overflowed"));
        };
        self.bytes = actual;
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        representability::Representability, CanonicalSupport, Policy, SupportEdge, SupportOutcome,
        SupportRejection,
    };

    #[test]
    fn support_proofs_are_compact_ordered_and_precharged_exactly() {
        assert!(std::mem::size_of::<SupportEdge>() <= 72);
        assert!(std::mem::size_of::<SupportOutcome<'static>>() <= 96);
        assert!(std::mem::size_of::<SupportRejection>() > std::mem::size_of::<SupportEdge>());

        let standard = crate::limits::Limits::standard();
        let contract = crate::test_fixture::support_outcomes()
            .classify(&Policy::standard(), &standard)
            .expect("support outcomes classify");
        let plan = contract
            .definition_plans()
            .find(|plan| {
                contract
                    .definition_support(plan.definition())
                    .is_some_and(|support| support.len() == 2)
            })
            .expect("the support fixture retains its shared plan");
        let outcomes = contract
            .definition_support(plan.definition())
            .expect("the plan belongs to this contract")
            .collect::<Vec<_>>();
        assert!(matches!(outcomes[0], SupportOutcome::Complete { .. }));
        assert!(matches!(
            outcomes[1],
            SupportOutcome::Rejected {
                reason: crate::api::ledger::Reason::UnresolvedDefinition(_),
                span,
                ..
            } if span.file() == "fixture/src/lib.rs" && span.byte_range().start == 88
        ));
        assert_eq!(
            serde_json::to_value(CanonicalSupport(contract.support_ledger()))
                .expect("streamed support serializes"),
            serde_json::to_value(contract.support().collect::<Vec<_>>())
                .expect("logical support sequence serializes")
        );

        let support_items = contract.support().len();
        let support_rejections = contract
            .support()
            .filter(|outcome| matches!(outcome, SupportOutcome::Rejected { .. }))
            .count();
        let definition_items = contract
            .definition_plans()
            .len()
            .checked_add(contract.raw_graph().definitions().len())
            .and_then(|items| items.checked_add(contract.raw_graph().implementations().len()))
            .and_then(|items| items.checked_add(support_items))
            .and_then(|items| items.checked_add(support_rejections))
            .expect("fixture support item count fits usize");
        let outcome_items = contract
            .outcomes()
            .map(|outcome| {
                1 + outcome.implementations().len()
                    + outcome
                        .implementations()
                        .map(super::ImplementationOutcome::associated)
                        .map(|associated| associated.len())
                        .sum::<usize>()
            })
            .sum::<usize>();
        let expected_items = contract
            .raw_graph()
            .allocation()
            .items
            .checked_add(1)
            .and_then(|items| items.checked_add(definition_items))
            .and_then(|items| items.checked_add(outcome_items))
            .expect("fixture retained item count fits usize");
        let expected_bytes = contract
            .allocation(&standard)
            .expect("contract allocation starts")
            .usage()
            .1;
        assert_eq!(
            contract
                .allocation(&standard)
                .expect("contract allocation starts")
                .usage()
                .0,
            expected_items
        );

        let mut exact = crate::limits::Limits::fixture(usize::MAX, expected_items, usize::MAX);
        exact.classified_contract_items = expected_items;
        crate::test_fixture::support_outcomes()
            .classify(&Policy::standard(), &exact)
            .expect("the exact retained-item boundary accepts");

        let mut exact_bytes =
            crate::limits::Limits::fixture(expected_bytes, usize::MAX, usize::MAX);
        exact_bytes.classified_contract_bytes = expected_bytes;
        crate::test_fixture::support_outcomes()
            .classify(&Policy::standard(), &exact_bytes)
            .expect("the exact retained-byte boundary accepts");

        let repeated = crate::test_fixture::support_outcomes()
            .classify(&Policy::standard(), &standard)
            .expect("repeated support outcomes classify");
        assert_eq!(contract.canonical_bytes(), repeated.canonical_bytes());
        assert_eq!(contract.fingerprint(), repeated.fingerprint());
    }

    #[test]
    fn classification_is_total_positioned_and_cardinality_exact() {
        let graph = crate::test_fixture::graph();
        let contract = graph
            .classify(&Policy::standard(), &crate::limits::Limits::standard())
            .expect("fixture graph classifies");
        let outcomes = contract.outcomes().collect::<Vec<_>>();

        assert_eq!(outcomes.len(), contract.raw_graph().exports().len());
        assert!(
            matches!(outcomes[0].representability(), Representability::Struct(plan) if plan.fields().len() == 2 && plan.constructible())
        );
        assert!(
            matches!(outcomes[1].representability(), Representability::Enum(plan) if plan.variants().len() == 2)
        );
        assert!(matches!(
            outcomes[2].representability(),
            Representability::Constant(_)
        ));
        assert!(matches!(
            outcomes[3].representability(),
            Representability::Function(_)
        ));
        assert!(
            matches!(outcomes[4].representability(), Representability::Rejected(reason) if reason.kind() == super::representability::reject::Kind::UnsafeFunction && reason.span().file() == "fixture/src/lib.rs")
        );
        assert!(
            matches!(outcomes[5].representability(), Representability::Rejected(reason) if reason.kind() == super::representability::reject::Kind::Module)
        );
        assert!(matches!(
            outcomes[6].representability(),
            Representability::Struct(_)
        ));
        assert_eq!(contract.definition_plans().len(), 6);
        let graph_definitions = contract
            .raw_graph()
            .definitions()
            .map(|definition| crate::surface::identity::Definition::of(definition.id()))
            .collect::<std::collections::BTreeSet<_>>();
        let planned_definitions = contract
            .definition_plans()
            .map(super::DefinitionPlan::definition)
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(planned_definitions, graph_definitions);
        assert_eq!(
            outcomes[0].identity().definition(),
            outcomes[6].identity().definition()
        );
        assert_ne!(
            outcomes[0].identity().export(),
            outcomes[6].identity().export()
        );
        assert!(std::ptr::eq(
            outcomes[0].representability(),
            outcomes[6].representability(),
        ));
        let exports = outcomes
            .iter()
            .map(|outcome| outcome.identity().export())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(exports.len(), outcomes.len());
    }

    #[test]
    fn selectors_are_exact_to_item_receiver_and_member() {
        use crate::surface::identity::{Definition, Field, Selector};

        let standard = crate::limits::Limits::standard();
        let contract = crate::test_fixture::graph()
            .classify(&Policy::standard(), &standard)
            .expect("selector fixture classifies");
        let outcomes = contract.outcomes().collect::<Vec<_>>();
        let function = outcomes[3];
        let Selector::Function { item } = function.identity().selector() else {
            panic!("fixture function retains its exact selector")
        };
        let wrong_item = Definition::of(
            contract
                .raw_graph()
                .definitions()
                .find(|definition| definition.id().def_path_hash() == "03")
                .expect("fixture constant definition exists")
                .id(),
        );
        assert!(contract
            .select(function.identity(), Selector::Function { item: *item })
            .expect("exact selector seals")
            .is_some());
        assert!(contract
            .select(function.identity(), Selector::Function { item: wrong_item })
            .expect("wrong selector remains a semantic miss")
            .is_none());

        let structure = outcomes[0];
        assert!(contract
            .select(
                structure.identity(),
                Selector::StructField(Field::new(99, "missing".into()))
            )
            .expect("wrong member remains a semantic miss")
            .is_none());

        let methods = crate::test_fixture::representability_families()
            .classify(&Policy::standard(), &standard)
            .expect("method fixture classifies");
        let method = methods.outcomes().nth(1).expect("fixture method exists");
        let Selector::Method { item, receiver } = method.identity().selector() else {
            panic!("fixture method retains its exact selector")
        };
        let wrong_receiver = match receiver {
            super::representability::ownership::Receiver::Shared => {
                super::representability::ownership::Receiver::Mutable
            }
            _ => super::representability::ownership::Receiver::Shared,
        };
        assert!(methods
            .select(
                method.identity(),
                Selector::Method {
                    item: *item,
                    receiver: wrong_receiver,
                }
            )
            .expect("wrong receiver remains a semantic miss")
            .is_none());
    }

    #[test]
    fn type_constants_are_support_only_and_never_receive_runtime_selectors() {
        use crate::surface::identity::Selector;

        let id = crate::test_fixture::graph()
            .definitions()
            .next()
            .expect("fixture identity exists")
            .id()
            .clone();
        let definition = crate::api::model::Definition::new(
            id,
            Box::from([Box::<str>::from("fixture"), Box::<str>::from("LENGTH")]),
            crate::api::model::Owner::Free,
            crate::api::model::Visibility::Public,
            crate::api::model::Kind::TypeConst {
                ty: crate::api::model::Type::Primitive("usize".into()),
                rhs: crate::api::model::Default::Present(crate::api::model::Const::Literal {
                    ty: "usize".into(),
                    value: "4".into(),
                }),
            },
            rejection_span(),
            crate::api::model::Origin::Source,
        );
        assert_eq!(
            super::selector(&definition).expect("support-only selector classification is total"),
            Selector::Unavailable
        );
    }

    #[test]
    fn late_binders_the_output_names_bind_early_and_others_lend() {
        use crate::api::model;

        let id = crate::test_fixture::graph()
            .definitions()
            .next()
            .expect("fixture definition exists")
            .id()
            .clone();
        let binder = model::Binder::new(id.clone(), 0, 0, "'a".into());
        let late = model::Lifetime::Late {
            binder: binder.clone(),
            index: 0,
        };
        let free = |inputs: Box<[model::Type]>, output| {
            model::Signature::new(
                model::Safety::Safe,
                false,
                false,
                "\"Rust\"".into(),
                false,
                Box::from([binder.clone()]),
                model::Receiver::None,
                inputs,
                output,
            )
        };
        let reference = |lifetime: model::Lifetime, ty| model::Type::Reference {
            lifetime,
            mutability: model::Mutability::Shared,
            ty: Box::new(ty),
        };
        let parameter = model::Type::Parameter {
            owner: id.clone(),
            index: 0,
        };
        let unit = model::Type::Tuple(Box::default());

        // `combo_box(.., selection: Option<&T>, ..)`: the `&T` in a type's
        // arguments is lent for the call.
        let selection = free(
            Box::from([model::Type::Definition {
                id: id.clone(),
                arguments: Box::from([model::Argument::Type(reference(
                    late.clone(),
                    parameter.clone(),
                ))]),
            }]),
            unit.clone(),
        );
        assert!(super::late_binders_lend_inputs(&selection));
        let generics = model::Generics::new(Box::default(), Box::default());
        assert!(super::output_binders_early(&selection, &generics).is_none());

        // `markdown::view<'a>(items: &'a [Item]) -> impl Widget + 'a`: the
        // lifetime the opaque output captures is read as early-bound.
        let items = reference(
            late.clone(),
            model::Type::Slice(Box::new(parameter.clone())),
        );
        let view = free(
            Box::from([items]),
            model::Type::ImplTrait {
                id: id.clone(),
                bounds: Box::from([model::Predicate::TypeOutlives {
                    ty: parameter.clone(),
                    lifetime: late.clone(),
                }]),
                captures: Box::from([model::Argument::Lifetime(late.clone())]),
            },
        );
        assert!(!super::late_binders_lend_inputs(&view));
        let early = super::output_binders_early(&view, &generics).expect("the output names 'a");
        let bound = model::Lifetime::Early {
            owner: id.clone(),
            index: 0,
        };
        assert_eq!(early.binders().len(), 0);
        assert!(super::receiver_binder_is_exact(&early));
        assert_eq!(
            early.inputs().next(),
            Some(&reference(
                bound.clone(),
                model::Type::Slice(Box::new(parameter.clone()))
            ))
        );
        assert!(
            matches!(early.output(), model::Type::ImplTrait { bounds, captures, .. }
            if captures[..] == [model::Argument::Lifetime(bound.clone())]
                && matches!(&bounds[..], [model::Predicate::TypeOutlives { lifetime, .. }]
                    if *lifetime == bound))
        );

        // `cause(failure: &Failure) -> &io::Error`: a borrowed output.
        let cause = free(
            Box::from([reference(late.clone(), parameter.clone())]),
            reference(late.clone(), parameter.clone()),
        );
        let early = super::output_binders_early(&cause, &generics).expect("the output names 'a");
        assert_eq!(early.output(), &reference(bound.clone(), parameter.clone()));

        // A lifetime nothing names stays a higher-ranked binder.
        let unnamed = free(Box::from([parameter.clone()]), unit);
        assert!(!super::late_binders_lend_inputs(&unnamed));
        assert!(super::output_binders_early(&unnamed, &generics).is_none());
    }

    #[test]
    fn shared_and_mutable_receivers_admit_only_their_exact_late_binder() {
        use crate::api::model;

        let id = crate::test_fixture::graph()
            .definitions()
            .next()
            .expect("fixture definition exists")
            .id()
            .clone();
        let binder = model::Binder::new(id.clone(), 0, 0, "'#0".into());
        let signature = |binders, lifetime, mutability| {
            model::Signature::new(
                model::Safety::Safe,
                false,
                false,
                "\"Rust\"".into(),
                false,
                binders,
                model::Receiver::Reference {
                    lifetime,
                    mutability,
                    ty: model::Type::SelfType(id.clone()),
                },
                Box::default(),
                model::Type::Tuple(Box::default()),
            )
        };
        for mutability in [model::Mutability::Shared, model::Mutability::Mutable] {
            let exact = signature(
                Box::from([binder.clone()]),
                model::Lifetime::Late {
                    binder: binder.clone(),
                    index: 0,
                },
                mutability,
            );
            assert!(super::receiver_binder_is_exact(&exact));
        }

        let extra = model::Binder::new(id.clone(), 0, 1, "'#1".into());
        let with_extra = signature(
            Box::from([binder.clone(), extra]),
            model::Lifetime::Late {
                binder: binder.clone(),
                index: 0,
            },
            model::Mutability::Shared,
        );
        assert!(!super::receiver_binder_is_exact(&with_extra));

        let mismatched = model::Binder::new(id.clone(), 0, 1, "'#1".into());
        let wrong_receiver = signature(
            Box::from([binder]),
            model::Lifetime::Late {
                binder: mismatched,
                index: 1,
            },
            model::Mutability::Mutable,
        );
        assert!(!super::receiver_binder_is_exact(&wrong_receiver));
    }

    /// inboard's `wizard::State::view<'a>(&'a self, catalog: View<'a>,
    /// theme: &'a Theme) -> Element<'a, Message>`: a frame over its receiver
    /// that keeps the shared `&'a Theme` too, but not a `&'a mut` one.
    #[test]
    fn a_frame_over_its_receiver_takes_a_shared_reference_at_its_lifetime() {
        use crate::api::model;

        let id = crate::test_fixture::graph()
            .definitions()
            .next()
            .expect("fixture definition exists")
            .id()
            .clone();
        let binder = model::Binder::new(id.clone(), 0, 0, "'#0".into());
        let lifetime = model::Lifetime::Late {
            binder: binder.clone(),
            index: 0,
        };
        let borrowing = model::Type::Definition {
            id: id.clone(),
            arguments: Box::from([model::Argument::Lifetime(lifetime.clone())]),
        };
        let plain = model::Type::Definition {
            id: id.clone(),
            arguments: Box::default(),
        };
        let reference = |mutability| model::Type::Reference {
            lifetime: lifetime.clone(),
            mutability,
            ty: Box::new(plain.clone()),
        };
        let signature = |inputs: Box<[model::Type]>| {
            model::Signature::new(
                model::Safety::Safe,
                false,
                false,
                "\"Rust\"".into(),
                false,
                Box::from([binder.clone()]),
                model::Receiver::Reference {
                    lifetime: lifetime.clone(),
                    mutability: model::Mutability::Shared,
                    ty: model::Type::SelfType(id.clone()),
                },
                inputs,
                borrowing.clone(),
            )
        };
        let shared = signature(Box::from([
            borrowing.clone(),
            reference(model::Mutability::Shared),
        ]));
        assert!(super::borrows_receiver(&shared, |_| false));
        let mutable = signature(Box::from([reference(model::Mutability::Mutable)]));
        assert!(!super::borrows_receiver(&mutable, |_| false));
    }

    #[test]
    fn raw_and_contract_fingerprints_invalidate_independently() {
        let graph = crate::test_fixture::graph();
        let raw = graph.fingerprint();
        let standard = graph
            .classify(&Policy::standard(), &crate::limits::Limits::standard())
            .expect("standard policy classifies");
        let prior = crate::test_fixture::graph()
            .classify(
                &Policy::with_versions(super::CONTRACT_SCHEMA - 1, super::CLASSIFIER_VERSION - 1),
                &crate::limits::Limits::standard(),
            )
            .expect("the prior closed-vocabulary policy remains measurable in tests");

        assert_eq!(standard.raw_fingerprint(), raw);
        assert_eq!(prior.raw_fingerprint(), raw);
        assert_ne!(standard.fingerprint(), prior.fingerprint());
        assert_ne!(standard.canonical_bytes(), prior.canonical_bytes());
    }

    #[test]
    fn core_option_signatures_are_admitted_as_rawr_options() {
        use super::representability::convert::{Builtin, Type};
        let contract = crate::test_fixture::option_function()
            .classify(&Policy::standard(), &crate::limits::Limits::standard())
            .expect("the option graph classifies");
        let function = contract
            .outcomes()
            .find_map(|outcome| match outcome.representability() {
                Representability::Function(plan) => Some(plan.clone()),
                _ => None,
            })
            .expect("a function taking and returning `Option` is admitted");
        let option = |ty: Type| Type::Option(Box::new(ty));
        assert_eq!(
            function.signature().inputs(),
            [option(Type::Builtin(Builtin::I64))]
        );
        assert_eq!(
            function.signature().output(),
            &option(option(Type::Builtin(Builtin::Bool)))
        );
    }

    #[test]
    fn a_future_parameter_the_output_names_is_rejected_and_its_output_is_kept() {
        use super::representability::{reject, signature};
        let contract = crate::test_fixture::future_parameter_functions()
            .classify(&Policy::standard(), &crate::limits::Limits::standard())
            .expect("the future parameter graph classifies");
        let outcome = |name: &str| {
            contract
                .outcomes()
                .find(|outcome| outcome.name() == name)
                .unwrap_or_else(|| panic!("`{name}` is classified"))
                .representability()
                .clone()
        };
        // `run<F: Future>(f: F) -> F::Output` names `F` only through its
        // output, which the facade declares as `FOutput`.
        let Representability::FunctionItem(run) = outcome("run") else {
            panic!("`run` is a function item: {:?}", outcome("run"));
        };
        let kinds = run
            .signature()
            .parameters()
            .iter()
            .map(signature::Parameter::kind)
            .collect::<Vec<_>>();
        assert_eq!(kinds, [signature::Kind::Future, signature::Kind::Type]);
        // `scope<F: Future>(f: F) -> Wrap<F>` names the script future's own
        // type, which no facade signature can declare, as tokio's
        // `LocalKey::scope` does.
        assert!(
            matches!(
                outcome("scope"),
                Representability::Rejected(reason)
                    if reason.kind() == reject::Kind::ScriptFutureType
            ),
            "{:?}",
            outcome("scope")
        );
    }

    /// A closure that returns a future, as `sipper(|sender| async move {
    /// .. })` takes, has no Rust type the runner can write for the script's
    /// `async` block. The item is refused with that reason, which a script's
    /// call of it reports, and never reaches the generator (#545), as is one
    /// whose output's bound has `Future` among its supertraits (#1149). A
    /// closure that returns a value is made.
    #[test]
    fn a_closure_that_returns_a_future_is_refused_in_rust_terms() {
        use super::representability::reject;
        let contract = crate::test_fixture::future_closure_functions()
            .classify(&Policy::standard(), &crate::limits::Limits::standard())
            .expect("the future closure graph classifies");
        let outcome = |name: &str| {
            contract
                .outcomes()
                .find(|outcome| outcome.name() == name)
                .unwrap_or_else(|| panic!("`{name}` is classified"))
                .representability()
                .clone()
        };
        for name in ["sip", "sip_core"] {
            let Representability::Rejected(reason) = outcome(name) else {
                panic!("`{name}` is refused: {:?}", outcome(name));
            };
            assert_eq!(reason.kind(), reject::Kind::FutureClosure, "{name}");
        }
        let Representability::Rejected(reason) = outcome("sip") else {
            panic!("`sip` is refused: {:?}", outcome("sip"));
        };
        assert_eq!(
            reason.kind().describe(),
            "it takes a closure that returns a future"
        );
        let Representability::FunctionItem(ready) = outcome("ready") else {
            panic!("`ready` is a function item: {:?}", outcome("ready"));
        };
        assert!(ready.call().is_some(), "{ready:?}");
    }

    #[test]
    fn core_result_signatures_are_admitted_as_rawr_results() {
        use super::representability::convert::{Builtin, Type};
        let contract = crate::test_fixture::result_function()
            .classify(&Policy::standard(), &crate::limits::Limits::standard())
            .expect("the result graph classifies");
        let function = contract
            .outcomes()
            .find_map(|outcome| match outcome.representability() {
                Representability::Function(plan) => Some(plan.clone()),
                _ => None,
            })
            .expect("a function taking and returning `Result` is admitted");
        let result = |ok: Type, err: Type| Type::Result(Box::new(ok), Box::new(err));
        assert_eq!(
            function.signature().inputs(),
            [result(
                Type::Builtin(Builtin::I64),
                Type::Builtin(Builtin::String)
            )]
        );
        assert_eq!(
            function.signature().output(),
            &result(
                Type::Option(Box::new(Type::Builtin(Builtin::Bool))),
                Type::Builtin(Builtin::I64)
            )
        );
    }

    /// T204: palette's `Rgb`, voronator's `from_tuple`, image's
    /// `save_buffer` and changelog's `async fn list` are bound as Rust has
    /// them.
    #[test]
    fn upstream_example_shapes_are_bound_as_rust_has_them() {
        use super::representability::convert::{Builtin, Type};
        let contract = crate::test_fixture::native_shapes()
            .classify(&Policy::standard(), &crate::limits::Limits::standard())
            .expect("the native shapes graph classifies");
        let named = |name: &str| {
            contract
                .outcomes()
                .find(|outcome| outcome.name() == name)
                .unwrap_or_else(|| panic!("`{name}` has an outcome"))
                .representability()
                .clone()
        };
        let point = || {
            Type::Tuple(Box::from([
                Type::Builtin(Builtin::F64),
                Type::Builtin(Builtin::F64),
            ]))
        };
        let shared = |ty: Type| Type::Borrowed {
            mutable: false,
            ty: Box::new(ty),
        };

        // A `PhantomData<S>` field is Rawr's unit, and a struct whose
        // parameters all have defaults is made and read at that instance.
        let Representability::Struct(rgb) = named("Rgb") else {
            panic!("`Rgb` is a struct plan");
        };
        assert!(rgb.constructible());
        assert!(!rgb.declaration_only());
        assert!(rgb.constructor().is_some());
        assert_eq!(rgb.fields().len(), 2);
        assert_eq!(rgb.fields()[0].ty(), &Type::Parameter(1));
        assert_eq!(rgb.fields()[1].ty(), &Type::Unit);
        assert!(named("Rgb").is_demandable());

        // `&(f64, f64)` and `&[(f64, f64)]` are shared borrows of a tuple
        // and a slice.
        let Representability::Function(inside) = named("inside") else {
            panic!("`inside` is a function plan");
        };
        assert_eq!(
            inside.signature().inputs(),
            [
                shared(point()),
                shared(Type::Sequence {
                    element: Box::new(point()),
                    owned: false,
                    length: None,
                }),
            ]
        );

        // `&[u8]` is a shared borrow of a slice of bytes.
        let Representability::Function(checksum) = named("checksum") else {
            panic!("`checksum` is a function plan");
        };
        assert_eq!(
            checksum.signature().inputs(),
            [shared(Type::Sequence {
                element: Box::new(Type::Builtin(Builtin::U8)),
                owned: false,
                length: None,
            })]
        );

        // An async fn's future gives a `Result` of a tuple holding a `Vec`.
        let Representability::Function(list) = named("list") else {
            panic!("`list` is a function plan, not {:?}", named("list"));
        };
        let Type::Nominal {
            definition,
            arguments,
        } = list.signature().output()
        else {
            panic!("`list` gives a future");
        };
        assert_eq!(*definition, crate::surface::identity::Definition::future());
        let [Type::Result(ok, _)] = arguments.as_ref() else {
            panic!("the future gives a `Result`: {arguments:?}");
        };
        assert!(matches!(
            ok.as_ref(),
            Type::Tuple(members)
                if matches!(members.as_ref(), [Type::Nominal { .. }, Type::Sequence { owned: true, .. }])
        ));

        // An `impl Straw<(), i64, i64>` a function gives is an opaque type
        // of its own, declared beside the function, as any `impl Trait` is.
        // It crossed as a sipper type that only rawr-iced declared, so a
        // program's own `download` named a type no bindings declare (#561).
        let Representability::Function(download) = named("download") else {
            panic!("`download` is a function plan, not {:?}", named("download"));
        };
        let Type::Nominal { definition, .. } = download.signature().output() else {
            panic!("`download` gives its opaque type");
        };
        assert!(
            crate::surface::opaque::identities(contract.raw_graph())
                .contains(definition.as_bytes()),
            "{:?}",
            download.signature().output()
        );
        assert!(!crate::surface::identity::Definition::is_projected(
            definition.as_bytes()
        ));
    }

    #[test]
    fn remaining_representability_families_are_closed_and_positioned() {
        let contract = crate::test_fixture::representability_families()
            .classify(&Policy::standard(), &crate::limits::Limits::standard())
            .expect("the representability-family graph classifies");
        let outcomes = contract.outcomes().collect::<Vec<_>>();
        assert_eq!(outcomes.len(), 5);
        assert!(matches!(
            outcomes[0].representability(),
            Representability::Value(_)
        ));
        assert!(matches!(
            outcomes[1].representability(),
            Representability::Method(plan)
                if plan.receiver()
                    == super::representability::ownership::Receiver::Shared
        ));
        assert!(matches!(
            outcomes[2].representability(),
            Representability::FunctionItem(plan)
                if plan.signature().parameters().len() == 1
        ));
        assert!(matches!(
            outcomes[3].representability(),
            Representability::Excluded(reason)
                if reason.policy_span().file() == "fixture/src/lib.rs"
                    && reason.source_span().file() == "fixture/src/lib.rs"
        ));
        assert!(outcomes
            .iter()
            .all(|outcome| outcome.representability().is_demandable()
                == matches!(outcome.representability(), Representability::Method(_))));
        assert!(matches!(
            outcomes[4].representability(),
            Representability::Rejected(reason)
                if reason.kind() == super::representability::reject::Kind::Extraction
                    && matches!(
                        reason.extraction_reason(),
                        Some(crate::api::ledger::Reason::InvalidAliasChain)
                    )
        ));
    }

    #[test]
    fn every_classifier_rejection_family_executes_its_closed_branch() {
        use super::representability::reject::Kind;
        use crate::api::model;

        let graph = crate::test_fixture::graph();
        let limits = crate::limits::Limits::standard();
        let context = super::Context::new(&graph, super::Authority::production(&limits))
            .expect("fixture context is bounded");
        let definition = graph
            .definitions()
            .next()
            .expect("fixture retains a definition");
        let id = definition.id().clone();
        let foreign = crate::test_fixture::representability_families()
            .definitions()
            .next()
            .expect("family fixture retains a distinct definition")
            .id()
            .clone();
        let signature = model::Signature::new(
            model::Safety::Safe,
            false,
            false,
            "\"Rust\"".into(),
            false,
            Box::default(),
            model::Receiver::None,
            Box::default(),
            model::Type::Tuple(Box::default()),
        );
        let unsupported_types = [
            (
                model::Type::Primitive("u128".into()),
                Kind::UnsupportedPrimitive,
            ),
            (
                model::Type::Definition {
                    id: foreign.clone(),
                    arguments: Box::default(),
                },
                Kind::UnresolvedDefinition,
            ),
            (
                model::Type::Definition {
                    id: id.clone(),
                    // A `'static` lifetime argument is admitted (D558 tier 0);
                    // a const one is not.
                    arguments: Box::from([model::Argument::Const(model::Const::Literal {
                        ty: "usize".into(),
                        value: "1".into(),
                    })]),
                },
                Kind::UnsupportedGenericArgument,
            ),
            (
                model::Type::Reference {
                    lifetime: model::Lifetime::Static,
                    mutability: model::Mutability::Shared,
                    ty: Box::new(model::Type::Primitive("i64".into())),
                },
                Kind::ReferenceType,
            ),
            (
                model::Type::RawPointer {
                    mutability: model::Mutability::Shared,
                    ty: Box::new(model::Type::Primitive("i64".into())),
                },
                Kind::RawPointer,
            ),
            (
                model::Type::FunctionPointer(Box::new(signature)),
                Kind::FunctionPointer,
            ),
            (
                model::Type::TraitObject {
                    bounds: Box::default(),
                    lifetime: model::Lifetime::Static,
                },
                Kind::TraitObject,
            ),
            (
                model::Type::ImplTrait {
                    id: id.clone(),
                    bounds: Box::default(),
                    captures: Box::default(),
                },
                Kind::OpaqueType,
            ),
            (
                model::Type::Projection {
                    trait_id: id.clone(),
                    associated: foreign,
                    arguments: Box::default(),
                },
                Kind::Projection,
            ),
            (
                model::Type::Tuple(Box::from([model::Type::Primitive("i64".into())])),
                Kind::TupleType,
            ),
            (
                model::Type::Slice(Box::new(model::Type::Primitive("i64".into()))),
                Kind::SliceType,
            ),
            (
                model::Type::Array {
                    element: Box::new(model::Type::Primitive("i64".into())),
                    length: model::Const::Literal {
                        ty: "usize".into(),
                        value: "1".into(),
                    },
                },
                Kind::ArrayType,
            ),
            (model::Type::Never, Kind::NeverType),
        ];
        for (ty, expected) in unsupported_types {
            assert_eq!(rejection(context.convert(&ty)), expected);
        }

        let private = model::Definition::new(
            id.clone(),
            Box::from([Box::<str>::from("fixture"), Box::<str>::from("Private")]),
            model::Owner::Free,
            model::Visibility::Restricted(id.clone()),
            model::Kind::Module,
            rejection_span(),
            model::Origin::Source,
        );
        assert_eq!(
            rejection(super::definition_is_public(&private)),
            Kind::PrivateType
        );
        let opaque = model::Definition::new(
            id.clone(),
            Box::from([Box::<str>::from("impl Notifier")]),
            model::Owner::Free,
            model::Visibility::Public,
            model::Kind::Module,
            rejection_span(),
            model::Origin::Source,
        );
        assert_eq!(
            rejection(super::definition_is_publishable_nominal(&opaque)),
            Kind::OpaqueType
        );
        let non_nominal = model::Definition::new(
            id.clone(),
            Box::from([Box::<str>::from("fixture"), Box::<str>::from("Notifier")]),
            model::Owner::Free,
            model::Visibility::Public,
            model::Kind::Module,
            rejection_span(),
            model::Origin::Source,
        );
        assert_eq!(
            rejection(super::definition_is_publishable_nominal(&non_nominal)),
            Kind::UnnameableType
        );
        assert!(super::facade_segment_nameable("Notifier"));
        assert!(!super::facade_segment_nameable("impl Notifier"));

        let empty = rejection_generics();
        let definition_kinds = [
            (model::Kind::Module, Kind::Module),
            (
                model::Kind::Union {
                    fields: model::FieldSet::new(
                        model::FieldForm::Record,
                        Box::default(),
                        true,
                        false,
                    ),
                    generics: empty.clone(),
                },
                Kind::Union,
            ),
            (
                model::Kind::Static {
                    ty: model::Type::Primitive("i64".into()),
                    mutable: true,
                    thread_local: false,
                },
                Kind::Static,
            ),
            (
                model::Kind::Trait {
                    unsafe_trait: false,
                    auto_trait: false,
                    supertraits: Box::default(),
                    associated: Box::default(),
                    generics: empty.clone(),
                },
                Kind::Trait,
            ),
            (
                model::Kind::AssociatedType {
                    bounds: Box::default(),
                    default: model::Default::Absent,
                    generics: empty.clone(),
                },
                Kind::AssociatedType,
            ),
            (
                model::Kind::TypeConst {
                    ty: model::Type::Primitive("usize".into()),
                    rhs: model::Default::Present(model::Const::Literal {
                        ty: "usize".into(),
                        value: "4".into(),
                    }),
                },
                Kind::TypeConstant,
            ),
            (
                model::Kind::Sysroot {
                    compiler_kind: "foreign".into(),
                },
                Kind::SysrootDefinition,
            ),
        ];
        for (kind, expected) in definition_kinds {
            let rejected = model::Definition::new(
                id.clone(),
                Box::from([Box::<str>::from("fixture"), Box::<str>::from("Rejected")]),
                model::Owner::Free,
                model::Visibility::Public,
                kind,
                rejection_span(),
                model::Origin::Source,
            );
            assert!(matches!(
                context.classify_definition(0, &rejected),
                Ok(Representability::Rejected(reason)) if reason.kind() == expected
            ));
        }

        // An immutable static of a slice is read where the bridge runs, as
        // a Rawr array of its cloned elements; a pair crosses as a tuple.
        let pair = || {
            model::Type::Tuple(Box::from([
                model::Type::Primitive("isize".into()),
                model::Type::Primitive("isize".into()),
            ]))
        };
        let statik = model::Definition::new(
            id.clone(),
            Box::from([Box::<str>::from("fixture"), Box::<str>::from("ALL")]),
            model::Owner::Free,
            model::Visibility::Public,
            model::Kind::Static {
                ty: model::Type::Reference {
                    lifetime: model::Lifetime::Static,
                    mutability: model::Mutability::Shared,
                    ty: Box::new(model::Type::Slice(Box::new(pair()))),
                },
                mutable: false,
                thread_local: false,
            },
            rejection_span(),
            model::Origin::Source,
        );
        let Ok(Representability::Constant(plan)) = context.classify_definition(0, &statik) else {
            panic!("an immutable static of a slice is admitted");
        };
        let isize = || {
            super::representability::convert::Type::Builtin(
                super::representability::convert::Builtin::Isize,
            )
        };
        assert_eq!(
            plan.ty(),
            &super::representability::convert::Type::Sequence {
                element: Box::new(super::representability::convert::Type::Tuple(Box::from([
                    isize(),
                    isize()
                ]))),
                owned: false,
                length: None,
            }
        );
        assert!(plan.value().is_none());

        // A `const` or `static` of `&str` is `&'static str` in either, as
        // inboard's `fonts::UI_FAMILY` is: a borrow of Rawr's string read
        // where the bridge runs, kept for the runner's life (#1146).
        let text = || model::Type::Reference {
            lifetime: model::Lifetime::Static,
            mutability: model::Mutability::Shared,
            ty: Box::new(model::Type::Primitive("str".into())),
        };
        let static_text = super::representability::convert::Type::Borrowed {
            mutable: false,
            ty: Box::new(super::representability::convert::Type::Builtin(
                super::representability::convert::Builtin::StaticStr,
            )),
        };
        let texts = [
            model::Kind::Const {
                ty: text(),
                value: model::Default::Present(model::Const::Unevaluated {
                    definition: id.clone(),
                    arguments: Box::default(),
                }),
            },
            model::Kind::Static {
                ty: text(),
                mutable: false,
                thread_local: false,
            },
        ];
        for kind in texts {
            let definition = model::Definition::new(
                id.clone(),
                Box::from([Box::<str>::from("fixture"), Box::<str>::from("UI_FAMILY")]),
                model::Owner::Free,
                model::Visibility::Public,
                kind,
                rejection_span(),
                model::Origin::Source,
            );
            let Ok(Representability::Constant(plan)) = context.classify_definition(0, &definition)
            else {
                panic!("a constant of `&'static str` is admitted");
            };
            assert_eq!(plan.ty(), &static_text);
            assert!(plan.value().is_none());
        }
        // A `const` of a slice of numbers, as `BYTES: &[u8]`, is a borrow
        // of Rawr's array of them, as rustc's `&'static [u8]` is.
        let bytes = model::Definition::new(
            id.clone(),
            Box::from([Box::<str>::from("fixture"), Box::<str>::from("BYTES")]),
            model::Owner::Free,
            model::Visibility::Public,
            model::Kind::Const {
                ty: model::Type::Reference {
                    lifetime: model::Lifetime::Static,
                    mutability: model::Mutability::Shared,
                    ty: Box::new(model::Type::Slice(Box::new(model::Type::Primitive(
                        "u8".into(),
                    )))),
                },
                value: model::Default::Present(model::Const::Unevaluated {
                    definition: id.clone(),
                    arguments: Box::default(),
                }),
            },
            rejection_span(),
            model::Origin::Source,
        );
        let Ok(Representability::Constant(plan)) = context.classify_definition(0, &bytes) else {
            panic!("a constant of `&'static [u8]` is admitted");
        };
        assert_eq!(
            plan.ty(),
            &super::representability::convert::Type::Borrowed {
                mutable: false,
                ty: Box::new(super::representability::convert::Type::Sequence {
                    element: Box::new(super::representability::convert::Type::Builtin(
                        super::representability::convert::Builtin::U8,
                    )),
                    owned: false,
                    length: None,
                }),
            }
        );

        let safe = |async_function, variadic, abi: &str, binders, receiver| {
            model::Signature::new(
                model::Safety::Safe,
                false,
                async_function,
                abi.into(),
                variadic,
                binders,
                receiver,
                Box::default(),
                model::Type::Tuple(Box::default()),
            )
        };
        let unsafe_signature = model::Signature::new(
            model::Safety::Unsafe,
            false,
            false,
            "\"Rust\"".into(),
            false,
            Box::default(),
            model::Receiver::None,
            Box::default(),
            model::Type::Tuple(Box::default()),
        );
        let function_cases = [
            (unsafe_signature, Kind::UnsafeFunction),
            (
                safe(
                    false,
                    true,
                    "\"Rust\"",
                    Box::default(),
                    model::Receiver::None,
                ),
                Kind::VariadicFunction,
            ),
            (
                safe(false, false, "\"C\"", Box::default(), model::Receiver::None),
                Kind::ForeignAbi,
            ),
            (
                safe(
                    false,
                    false,
                    "\"Rust\"",
                    Box::from([model::Binder::new(id.clone(), 0, 0, "a".into())]),
                    model::Receiver::None,
                ),
                Kind::HigherRankedBinder,
            ),
            (
                safe(
                    false,
                    false,
                    "\"Rust\"",
                    Box::default(),
                    model::Receiver::Explicit {
                        ty: model::Type::Primitive("i64".into()),
                    },
                ),
                Kind::ExplicitReceiver,
            ),
        ];
        for (signature, expected) in function_cases {
            assert_eq!(rejection(context.function(&signature, &empty)), expected);
        }
        // An `async fn` that borrows its receiver crosses too: its future
        // holds a copy of the receiver, or the receiver lent (D513.6).
        let borrowing = safe(
            true,
            false,
            "\"Rust\"",
            Box::default(),
            model::Receiver::Reference {
                lifetime: model::Lifetime::Static,
                mutability: model::Mutability::Mutable,
                ty: model::Type::Tuple(Box::default()),
            },
        );
        assert!(matches!(
            context.function(&borrowing, &empty),
            Ok(Representability::Method(_))
        ));

        let lifetime_generics = model::Generics::new(
            Box::from([model::Parameter::new(
                id.clone(),
                0,
                "a".into(),
                model::ParameterKind::Lifetime {
                    bounds: Box::default(),
                },
            )]),
            Box::default(),
        );
        // A lifetime parameter is `'static` (D558 tier 0): it declares
        // nothing.
        assert!(context
            .parameters(&lifetime_generics)
            .is_ok_and(|parameters| parameters.is_empty()));
        let opaque_generics = model::Generics::new(
            Box::from([model::Parameter::new(
                id.clone(),
                0,
                "impl Notifier".into(),
                model::ParameterKind::Type {
                    bounds: Box::default(),
                    default: model::Default::Absent,
                },
            )]),
            Box::default(),
        );
        assert_eq!(
            rejection(context.parameters(&opaque_generics)),
            Kind::UnnameableType
        );
        let predicate_generics = model::Generics::new(
            Box::default(),
            Box::from([model::Predicate::LifetimeOutlives {
                longer: model::Lifetime::Static,
                shorter: model::Lifetime::Erased,
            }]),
        );
        // A bound is rustc's to check where the bridge names the type.
        assert!(context.parameters(&predicate_generics).is_ok());
        // `T = f32` is what a type written without `T` takes.
        let defaulted_generics = model::Generics::new(
            Box::from([model::Parameter::new(
                id.clone(),
                0,
                "T".into(),
                model::ParameterKind::Type {
                    bounds: Box::default(),
                    default: model::Default::Present(model::Type::Primitive("f32".into())),
                },
            )]),
            Box::default(),
        );
        let Ok(defaulted) = context.parameters(&defaulted_generics) else {
            panic!("a defaulted type parameter is admitted");
        };
        assert_eq!(
            defaulted[0].default(),
            Some(&super::representability::convert::Type::Builtin(
                super::representability::convert::Builtin::F32
            ))
        );
        assert_eq!(
            rejection(context.constant(
                &model::Type::Primitive("i64".into()),
                &model::Default::Absent,
            )),
            Kind::MissingConstantValue
        );
        assert_eq!(
            rejection(super::constant(&model::Const::Parameter {
                owner: id.clone(),
                index: 0,
            })),
            Kind::UnevaluatedConstant
        );
        assert_eq!(
            rejection(super::constant(&model::Const::Literal {
                ty: "u128".into(),
                value: "1".into(),
            })),
            Kind::UnsupportedConstant
        );

        for expected in [
            Kind::UnsafeImplementation,
            Kind::NegativeImplementation,
            Kind::ConstImplementation,
            Kind::DefaultImplementation,
            Kind::GenericImplementation,
        ] {
            assert_eq!(
                implementation_rejection(&context, definition, expected),
                expected
            );
        }
    }

    /// An implementation that declares only lifetimes for another type that
    /// names this one, as cosmic-text's `impl<'a> BorrowedWithFontSystem<'a,
    /// Buffer>`, gives this type no members: its `hit(&mut self)` must not
    /// be filed as `Buffer::hit` beside `Buffer::hit(&self)`. The same
    /// implementation of the type itself is this type's.
    #[test]
    fn a_lifetime_only_implementation_is_the_type_s_own_only_with_nothing_but_lifetimes_filled_in()
    {
        use super::representability::reject::Kind;
        use crate::api::model;

        let graph = crate::test_fixture::graph();
        let limits = crate::limits::Limits::standard();
        let context = super::Context::new(&graph, super::Authority::production(&limits))
            .expect("fixture context is bounded");
        let definition = graph
            .definitions()
            .next()
            .expect("fixture retains a definition");
        let wrapper = crate::test_fixture::representability_families()
            .definitions()
            .next()
            .expect("family fixture retains a distinct definition")
            .id()
            .clone();
        let generics = model::Generics::new(
            Box::from([model::Parameter::new(
                wrapper.clone(),
                0,
                "'a".into(),
                model::ParameterKind::Lifetime {
                    bounds: Box::default(),
                },
            )]),
            Box::default(),
        );
        let implementation = |self_type: model::Type| {
            model::Implementation::new(
                "fixture-impl".into(),
                wrapper.clone(),
                Box::from([definition.id().clone(), wrapper.clone()]),
                model::Trait::Inherent,
                self_type,
                model::Safety::Safe,
                model::Polarity::Positive,
                false,
                false,
                generics.clone(),
                Box::default(),
                rejection_span(),
                model::Origin::Source,
            )
        };
        let early = model::Argument::Lifetime(model::Lifetime::Early {
            owner: wrapper.clone(),
            index: 0,
        });
        // `impl<'a> Wrapper<'a, Definition>`, listed among the members of
        // `Definition`, which it names.
        let borrowed = implementation(model::Type::Definition {
            id: wrapper.clone(),
            arguments: Box::from([
                early.clone(),
                model::Argument::Type(model::Type::Definition {
                    id: definition.id().clone(),
                    arguments: Box::default(),
                }),
            ]),
        });
        let evidence = context
            .classify_associated(&borrowed, definition, &[], definition.id())
            .expect("the implementation is classified");
        let super::OutcomeEvidence::Inline(representability) = evidence else {
            panic!("a member of another type is rejected inline");
        };
        let Representability::Rejected(reason) = representability.as_ref() else {
            panic!("a member of another type is not this type's");
        };
        assert_eq!(reason.kind(), Kind::GenericImplementation);
        // The same implementation fixes `Wrapper`'s type argument, so it
        // gives the generic `Wrapper` no members either: cosmic-text's
        // `BorrowedWithFontSystem<'_, Buffer>` and `<'_, Editor>` both
        // declare `set_tab_width(&mut self, u16)`.
        let wrapper_definition = crate::test_fixture::representability_families();
        let wrapper_definition = wrapper_definition
            .definitions()
            .next()
            .expect("family fixture retains a distinct definition");
        let evidence = context
            .classify_associated(&borrowed, wrapper_definition, &[], &wrapper)
            .expect("the implementation is classified");
        let super::OutcomeEvidence::Inline(representability) = evidence else {
            panic!("a member of a fixed instance is rejected inline");
        };
        let Representability::Rejected(reason) = representability.as_ref() else {
            panic!("a member of a fixed instance is not the generic type's");
        };
        assert_eq!(reason.kind(), Kind::GenericImplementation);
        // `impl<'a> Definition<'a>` is the type's own: its member is read
        // as the definition's plan, which this fixture doesn't hold.
        let own = implementation(model::Type::Definition {
            id: definition.id().clone(),
            arguments: Box::from([early]),
        });
        assert!(matches!(
            context.classify_associated(&own, definition, &[], definition.id()),
            Err(super::Error::Cardinality)
        ));
    }

    #[test]
    fn a_concrete_impl_substitutes_the_traits_arguments_in_its_method() {
        use crate::api::{identity, model};

        let graph = crate::test_fixture::classification_implementation_outcomes();
        let limits = crate::limits::Limits::standard();
        let context = super::Context::new(&graph, super::Authority::production(&limits))
            .expect("fixture context is bounded");
        let target = graph.definitions().next().unwrap().id();
        let trait_id = identity::Definition::new(
            target.unit().clone(),
            "d0".into(),
            identity::Namespace::Type,
        );
        let method_id = identity::Definition::new(
            target.unit().clone(),
            "d1".into(),
            identity::Namespace::Value,
        );
        let self_type = model::Type::Definition {
            id: target.clone(),
            arguments: Box::default(),
        };
        let parameter = model::Type::Parameter {
            owner: trait_id.clone(),
            index: 1,
        };
        let definition = model::Definition::new(
            method_id.clone(),
            Box::from([Box::<str>::from("fixture"), Box::<str>::from("exchange")]),
            model::Owner::Definition(trait_id.clone()),
            model::Visibility::Public,
            model::Kind::Function {
                signature: model::Signature::new(
                    model::Safety::Safe,
                    false,
                    false,
                    "\"Rust\"".into(),
                    false,
                    Box::default(),
                    model::Receiver::Value {
                        ty: self_type.clone(),
                    },
                    Box::from([parameter.clone()]),
                    parameter,
                ),
                generics: rejection_generics(),
            },
            rejection_span(),
            model::Origin::Source,
        );
        let implementation = model::Implementation::new(
            "concrete-trait-application".into(),
            graph.implementations().next().unwrap().id().clone(),
            Box::from([target.clone()]),
            model::Trait::Trait {
                id: trait_id,
                arguments: Box::from([
                    model::Argument::Type(self_type.clone()),
                    model::Argument::Type(model::Type::Primitive("i64".into())),
                ]),
            },
            self_type,
            model::Safety::Safe,
            model::Polarity::Positive,
            false,
            false,
            rejection_generics(),
            Box::from([method_id]),
            rejection_span(),
            model::Origin::Source,
        );

        let evidence = context
            .classify_default_method(&implementation, &definition)
            .expect("the concrete trait application classifies");
        let super::OutcomeEvidence::Inline(plan) = evidence else {
            panic!("a trait application retains its substituted signature");
        };
        let Representability::Method(method) = plan.as_ref() else {
            panic!("the concrete receiver method remains callable: {plan:?}");
        };
        let integer = super::representability::convert::Type::Builtin(
            super::representability::convert::Builtin::I64,
        );
        assert!(method.signature().parameters().is_empty());
        assert_eq!(method.signature().inputs(), std::slice::from_ref(&integer));
        assert_eq!(method.signature().output(), &integer);
    }

    /// A trait's impl for another type that names this one, as crossterm's
    /// `impl<D> AsRef<ContentStyle> for StyledContent<D>` names
    /// `ContentStyle`, gives this type no members: its `as_ref` would
    /// repeat `ContentStyle::as_ref`'s input signature.
    #[test]
    fn a_trait_impl_for_another_type_gives_no_members() {
        use super::representability::reject::Kind;
        use crate::api::model;

        let graph = crate::test_fixture::graph();
        let limits = crate::limits::Limits::standard();
        let context = super::Context::new(&graph, super::Authority::production(&limits))
            .expect("fixture context is bounded");
        let definition = graph
            .definitions()
            .next()
            .expect("fixture retains a definition");
        let styled = crate::test_fixture::representability_families()
            .definitions()
            .next()
            .expect("family fixture retains a distinct definition")
            .id()
            .clone();
        let implementation = model::Implementation::new(
            "fixture-impl".into(),
            styled.clone(),
            Box::from([definition.id().clone(), styled.clone()]),
            model::Trait::Trait {
                id: styled.clone(),
                arguments: Box::from([model::Argument::Type(model::Type::Definition {
                    id: definition.id().clone(),
                    arguments: Box::default(),
                })]),
            },
            model::Type::Definition {
                id: styled,
                arguments: Box::default(),
            },
            model::Safety::Safe,
            model::Polarity::Positive,
            false,
            false,
            rejection_generics(),
            Box::default(),
            rejection_span(),
            model::Origin::Source,
        );
        let evidence = context
            .classify_associated(&implementation, definition, &[], definition.id())
            .expect("the implementation is classified");
        let super::OutcomeEvidence::Inline(representability) = evidence else {
            panic!("a member of another type is rejected inline");
        };
        let Representability::Rejected(reason) = representability.as_ref() else {
            panic!("a member of another type is not this type's");
        };
        assert_eq!(reason.kind(), Kind::GenericImplementation);
    }

    /// An inherent impl for another type that fixes one of its arguments
    /// to this one, as palette's `impl<T> Hsl<Srgb, T>` does to `Srgb`,
    /// gives this type no members: `new_srgb` is `Hsl`'s, not `Srgb`'s.
    /// So does one that fixes every argument and declares no parameter.
    #[test]
    fn an_inherent_impl_for_another_type_gives_no_members() {
        use super::representability::reject::Kind;
        use crate::api::model;

        let graph = crate::test_fixture::graph();
        let limits = crate::limits::Limits::standard();
        let context = super::Context::new(&graph, super::Authority::production(&limits))
            .expect("fixture context is bounded");
        let definition = graph
            .definitions()
            .next()
            .expect("fixture retains a definition");
        let hsl = crate::test_fixture::representability_families()
            .definitions()
            .next()
            .expect("family fixture retains a distinct definition")
            .id()
            .clone();
        let this = || {
            model::Argument::Type(model::Type::Definition {
                id: definition.id().clone(),
                arguments: Box::default(),
            })
        };
        let generic = model::Generics::new(
            Box::from([model::Parameter::new(
                hsl.clone(),
                0,
                "T".into(),
                model::ParameterKind::Type {
                    bounds: Box::default(),
                    default: model::Default::Absent,
                },
            )]),
            Box::default(),
        );
        let parameter = model::Argument::Type(model::Type::Parameter {
            owner: hsl.clone(),
            index: 0,
        });
        for (arguments, generics) in [
            (Box::from([this(), parameter]), generic),
            (Box::from([this()]), rejection_generics()),
        ] {
            let implementation = model::Implementation::new(
                "fixture-impl".into(),
                hsl.clone(),
                Box::from([definition.id().clone(), hsl.clone()]),
                model::Trait::Inherent,
                model::Type::Definition {
                    id: hsl.clone(),
                    arguments,
                },
                model::Safety::Safe,
                model::Polarity::Positive,
                false,
                false,
                generics,
                Box::default(),
                rejection_span(),
                model::Origin::Source,
            );
            let evidence = context
                .classify_associated(&implementation, definition, &[], definition.id())
                .expect("the implementation is classified");
            let super::OutcomeEvidence::Inline(representability) = evidence else {
                panic!("a member of another type is rejected inline");
            };
            let Representability::Rejected(reason) = representability.as_ref() else {
                panic!("a member of another type is not this type's: {representability:?}");
            };
            assert_eq!(reason.kind(), Kind::GenericImplementation);
        }
    }

    /// A lifetime-only implementation that fixes a type argument to its
    /// parameter's declared default is the type's own, as rustc resolves
    /// `Parser::new` in `impl<'input> Parser<'input,
    /// DefaultBrokenLinkCallback>` for `Parser`. Any other fixed argument
    /// keeps it refused.
    #[test]
    fn a_fixed_argument_that_is_its_default_is_the_type_s_own_form() {
        use crate::api::model;

        let graph = crate::test_fixture::graph();
        let mut definitions = graph.definitions();
        let parser = definitions
            .next()
            .expect("fixture retains a definition")
            .id()
            .clone();
        let callback = definitions
            .next()
            .expect("fixture retains a second definition")
            .id()
            .clone();
        let named = |id: &crate::api::identity::Definition| model::Type::Definition {
            id: id.clone(),
            arguments: Box::default(),
        };
        // `struct Parser<'input, F = DefaultBrokenLinkCallback>`.
        let generics = model::Generics::new(
            Box::from([
                model::Parameter::new(
                    parser.clone(),
                    0,
                    "'input".into(),
                    model::ParameterKind::Lifetime {
                        bounds: Box::default(),
                    },
                ),
                model::Parameter::new(
                    parser.clone(),
                    1,
                    "F".into(),
                    model::ParameterKind::Type {
                        bounds: Box::default(),
                        default: model::Default::Present(named(&callback)),
                    },
                ),
            ]),
            Box::default(),
        );
        let early = model::Argument::Lifetime(model::Lifetime::Early {
            owner: parser.clone(),
            index: 0,
        });
        let fixed = |argument: model::Type| model::Type::Definition {
            id: parser.clone(),
            arguments: Box::from([early.clone(), model::Argument::Type(argument)]),
        };
        assert!(super::own_form(
            &fixed(named(&callback)),
            &parser,
            Some(&generics)
        ));
        // `Parser<'input, Parser>`: a fixed argument that is no default.
        assert!(!super::own_form(
            &fixed(named(&parser)),
            &parser,
            Some(&generics)
        ));
        // A parameter without a default refuses any fixed argument.
        assert!(!super::own_form(&fixed(named(&callback)), &parser, None));
        // Lifetimes alone, and another type, as before.
        let lifetimes = model::Type::Definition {
            id: parser.clone(),
            arguments: Box::from([early.clone()]),
        };
        assert!(super::own_form(&lifetimes, &parser, Some(&generics)));
        assert!(!super::own_form(&lifetimes, &callback, Some(&generics)));
    }

    fn rejection<T>(result: Result<T, super::Failure>) -> super::representability::reject::Kind {
        match result {
            Err(super::Failure::Reject(kind)) => kind,
            Err(super::Failure::Operational(error)) => {
                panic!("expected a semantic rejection, got {error}")
            }
            Ok(_) => panic!("expected a semantic rejection"),
        }
    }

    fn rejection_span() -> crate::api::model::Span {
        crate::api::model::Span::new("fixture/reject.rs".into(), 0, 1, 1, 0, 1, 1)
    }

    fn rejection_generics() -> crate::api::model::Generics {
        crate::api::model::Generics::new(Box::default(), Box::default())
    }

    fn implementation_rejection(
        context: &super::Context<'_>,
        definition: &crate::api::model::Definition,
        expected: super::representability::reject::Kind,
    ) -> super::representability::reject::Kind {
        use super::representability::reject::Kind;
        use crate::api::model;

        let safety = if expected == Kind::UnsafeImplementation {
            model::Safety::Unsafe
        } else {
            model::Safety::Safe
        };
        let polarity = if expected == Kind::NegativeImplementation {
            model::Polarity::Negative
        } else {
            model::Polarity::Positive
        };
        let const_impl = expected == Kind::ConstImplementation;
        let default_impl = expected == Kind::DefaultImplementation;
        let generics = if expected == Kind::GenericImplementation {
            model::Generics::new(
                Box::from([model::Parameter::new(
                    definition.id().clone(),
                    0,
                    "T".into(),
                    model::ParameterKind::Type {
                        bounds: Box::default(),
                        default: model::Default::Absent,
                    },
                )]),
                Box::default(),
            )
        } else {
            rejection_generics()
        };
        let implementation = model::Implementation::new(
            "fixture-impl".into(),
            definition.id().clone(),
            Box::from([definition.id().clone()]),
            model::Trait::Inherent,
            model::Type::Definition {
                id: definition.id().clone(),
                arguments: Box::default(),
            },
            safety,
            polarity,
            const_impl,
            default_impl,
            generics,
            Box::default(),
            rejection_span(),
            model::Origin::Source,
        );
        let evidence = context
            .classify_associated(&implementation, definition, &[], definition.id())
            .expect("implementation rejection is semantic");
        let super::OutcomeEvidence::Inline(representability) = evidence else {
            panic!("rejected implementation must retain inline evidence");
        };
        match representability.as_ref() {
            Representability::Rejected(reason) => reason.kind(),
            _ => panic!("implementation should reject"),
        }
    }

    #[test]
    fn production_contract_authority_is_exact_atomic_and_stage_isolated() {
        let limits = crate::limits::Limits::standard();
        let authority = super::Authority::production(&limits);
        assert_eq!(authority.items, 6_500_000);
        assert_eq!(authority.bytes, 671_088_640);

        let graph = crate::test_fixture::graph();
        let context = super::Context::new(&graph, authority).expect("fixture context is bounded");
        let remaining_items = authority.items - context.retained_items.get();
        context
            .retain_items(remaining_items)
            .expect("the exact classified item boundary succeeds");
        let retained_items = context.retained_items.get();
        assert_eq!(retained_items, 6_500_000);

        let context = super::Context::new(&graph, authority).expect("fixture context is bounded");
        let remaining_bytes = authority.bytes - context.retained_bytes.get();
        context
            .retain(0, remaining_bytes)
            .expect("the exact classified byte boundary succeeds");
        let retained_bytes = context.retained_bytes.get();
        assert_eq!(retained_bytes, 671_088_640);

        let mut graph_only = limits;
        graph_only.retained_graph_items -= 1;
        graph_only.retained_graph_bytes -= 1;
        let unchanged = super::Authority::production(&graph_only);
        assert_eq!(unchanged.items, authority.items);
        assert_eq!(unchanged.bytes, authority.bytes);
        #[cfg(feature = "rustc-driver")]
        {
            assert_eq!(super::Authority::evidence().items, 6_000_000);
            assert_eq!(super::Authority::evidence().bytes, 3_221_225_472);
        }
    }

    #[cfg(feature = "rustc-driver")]
    #[test]
    fn function_item_audit_counts_callable_binders_and_zero_control() {
        let signature = super::representability::signature::Signature::new(
            Box::from([super::representability::signature::Parameter::new(
                0,
                "F".into(),
                super::representability::signature::Kind::Callable,
            )]),
            Box::from([super::representability::convert::Type::Parameter(0)]),
            super::representability::convert::Type::Unit,
        );
        let mut signature_audit = super::DriverAudit { items: 0, bytes: 0 };
        signature_audit
            .signature(&signature)
            .expect("the synthetic signature audit is bounded");

        let mut zero_audit = super::DriverAudit { items: 0, bytes: 0 };
        zero_audit
            .representability(&Representability::FunctionItem(
                super::representability::callable::Item::new(
                    signature.clone(),
                    None,
                    None,
                    Box::default(),
                    Box::default(),
                ),
            ))
            .expect("the zero-binder function item audit is bounded");
        assert_eq!(zero_audit.items, 0);
        assert_eq!(zero_audit.bytes, signature_audit.bytes);

        let mut one_audit = super::DriverAudit { items: 0, bytes: 0 };
        one_audit
            .representability(&Representability::FunctionItem(
                super::representability::callable::Item::new(
                    signature,
                    None,
                    None,
                    Box::from([crate::surface::instance::Binder::normalized(0, 0)]),
                    Box::default(),
                ),
            ))
            .expect("the one-binder function item audit is bounded");
        assert_eq!(one_audit.items, 0);
        assert_eq!(
            one_audit.bytes,
            signature_audit.bytes + std::mem::size_of::<crate::surface::instance::Binder>()
        );
    }

    #[cfg(feature = "rustc-driver")]
    #[test]
    fn classification_evidence_is_scalar_exact_and_nonpublishing() {
        let authority = super::Authority::evidence();
        assert_eq!(authority.items, super::EVIDENCE_RETAINED_ITEMS);
        assert_eq!(authority.bytes, super::EVIDENCE_RETAINED_BYTES);
        let evidence = crate::test_fixture::support_outcomes()
            .into_classification_evidence()
            .measure(&Policy::standard())
            .expect("the fixture classification evidence completes");

        let classifier_new = 1_usize
            .checked_add(evidence.definitions)
            .and_then(|value| value.checked_add(evidence.support_groups))
            .and_then(|value| value.checked_add(evidence.support_edges))
            .and_then(|value| value.checked_add(evidence.support_rejections))
            .and_then(|value| value.checked_add(evidence.exports))
            .and_then(|value| value.checked_add(evidence.implementation_ledgers))
            .and_then(|value| value.checked_add(evidence.associated))
            .expect("the fixture formula is bounded");
        assert_eq!(
            evidence.support_groups,
            evidence.definitions + evidence.implementations
        );
        assert_eq!(evidence.classifier_new_items, classifier_new);
        assert_eq!(
            evidence.final_items,
            evidence.graph_items + evidence.classifier_new_items
        );
        assert_eq!(
            evidence.final_bytes,
            evidence.bytes.graph
                + evidence.bytes.contract_inline
                + evidence.bytes.definition_plans
                + evidence.bytes.outcomes
                + evidence.bytes.support
                + evidence.bytes.canonical
        );
        assert_eq!(evidence.graph_bytes, evidence.bytes.graph);
        assert_eq!(
            evidence.graph_retained.category_sum(),
            Some(evidence.graph_retained.total)
        );
        assert_eq!(evidence.graph_retained.total.items, evidence.graph_items);
        assert_eq!(evidence.graph_retained.total.bytes, evidence.graph_bytes);
        assert!(evidence.graph_retained.kind.items > 0);
        assert!(evidence.graph_retained.ty.items > 0);
        assert_eq!(evidence.canonical_contract_bytes, evidence.bytes.canonical);
        assert_eq!(
            evidence.scratch_high_water_bytes,
            evidence.definitions * std::mem::size_of::<crate::surface::identity::Definition>() * 2
        );
        assert!(!std::mem::needs_drop::<super::ClassificationEvidence>());
    }

    #[cfg(feature = "rustc-driver")]
    #[test]
    fn classification_evidence_streams_exact_implementation_and_associated_counts() {
        let zero = crate::test_fixture::representability_families()
            .into_classification_evidence()
            .measure(&Policy::standard())
            .expect("zero-implementation classification evidence completes");
        assert_eq!(zero.implementations, 0);
        assert_eq!(zero.implementation_ledgers, 0);
        assert_eq!(zero.associated, 0);

        let evidence = crate::test_fixture::classification_implementation_outcomes()
            .into_classification_evidence()
            .measure(&Policy::standard())
            .expect("implementation classification evidence completes");

        assert!(evidence.implementations > 0);
        assert!(evidence.implementation_ledgers > 0);
        assert!(evidence.associated > 0);
        assert_eq!(
            evidence.classifier_new_items,
            1 + evidence.definitions
                + evidence.support_groups
                + evidence.support_edges
                + evidence.support_rejections
                + evidence.exports
                + evidence.implementation_ledgers
                + evidence.associated
        );
        assert_eq!(
            evidence.support_groups,
            evidence.definitions + evidence.implementations
        );
    }

    #[cfg(feature = "rustc-driver")]
    #[test]
    fn transaction_history_cannot_change_classification_identity() {
        let limits = crate::limits::Limits::standard();
        let first = crate::test_fixture::graph()
            .classify(&Policy::standard(), &limits)
            .expect("the first fixture classifies");
        let second = crate::test_fixture::graph()
            .with_transaction_activity_for_test(usize::MAX - 1, usize::MAX - 2)
            .classify(&Policy::standard(), &limits)
            .expect("the same retained graph with different activity classifies");

        assert_eq!(first.raw_fingerprint(), second.raw_fingerprint());
        assert_eq!(first.fingerprint(), second.fingerprint());
        assert_eq!(
            first.driver_retained_usage(),
            second.driver_retained_usage()
        );
    }
}
