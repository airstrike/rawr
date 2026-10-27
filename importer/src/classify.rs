//! Closed, bounded classification of authoritative Rust API evidence.

use serde::ser::{SerializeSeq as _, SerializeStruct as _};
use serde::{Deserialize, Serialize, Serializer};
use sha2::{Digest as _, Sha256};
use std::cell::Cell;
use std::io::{self, Write};

use crate::api::{self, ledger, model};
use crate::contract::{self, Classified, Contract};

pub(crate) mod cached;
pub mod representability;

const CONTRACT_SCHEMA: u32 = 7;
const CLASSIFIER_VERSION: u32 = 33;
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
}

#[derive(Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct OutcomeRecord {
    export: api::identity::Export,
    identity: crate::surface::identity::Identity,
    name: Box<str>,
    origin: Option<representability::origin::Set>,
    evidence: OutcomeEvidence,
    implementations: Box<[ImplementationRecord]>,
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

/// One classification-stage account: accounting targets crossed by a
/// classification pass, a cached-contract decode, a checked-profile capture or
/// an import-context capture are recorded here and published, never refused.
pub(crate) struct Account {
    id: u64,
    exceeded: Cell<crate::exceedance::Record>,
}

impl Account {
    pub(crate) fn new() -> Self {
        Self {
            id: crate::exceedance::next_id(),
            exceeded: Cell::new(crate::exceedance::Record::new()),
        }
    }

    /// The axes this account crossed so far.
    #[cfg_attr(not(test), allow(dead_code, reason = "exceedance inspection"))]
    pub(crate) fn exceedances(&self) -> crate::exceedance::Record {
        self.exceeded.get()
    }

    /// Record one measurement against its target; the work continues.
    pub(crate) fn exceeded(&self, resource: &'static str, target: usize, actual: usize) {
        if actual <= target {
            return;
        }
        let mut record = self.exceeded.get();
        if record.exceeded(resource, target as u64, actual as u64, None) {
            self.exceeded.set(record);
            crate::exceedance::publish(crate::exceedance::Account::Classify, self.id, &record);
        }
    }
}

/// Checked-arithmetic overflow is a genuine failure and stays terminal.
fn arithmetic() -> Error {
    Error::Node(Box::new(crate::node::Error::Allocation))
}

struct Context<'graph, P: api::Phase = api::Extracted> {
    graph: &'graph api::Graph<P>,
    support: SupportLedger,
    item_limit: usize,
    byte_limit: usize,
    scratch_limit: usize,
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
    /// The classification account the retained and scratch targets record on.
    account: Account,
}

#[cfg(feature = "rustc-driver")]
pub(crate) const EVIDENCE_RETAINED_ITEMS: usize = 6_000_000;
#[cfg(feature = "rustc-driver")]
pub(crate) const EVIDENCE_RETAINED_BYTES: usize = 3_221_225_472;

#[derive(Clone, Copy)]
struct Authority {
    items: usize,
    bytes: usize,
    scratch_bytes: usize,
}

impl Authority {
    const fn production(limits: &crate::cargo::Limits) -> Self {
        Self {
            items: limits.classified_contract_items,
            bytes: limits.classified_contract_bytes,
            scratch_bytes: limits.json_bytes,
        }
    }

    #[cfg(feature = "rustc-driver")]
    const fn evidence(limits: &crate::cargo::Limits) -> Self {
        Self {
            items: EVIDENCE_RETAINED_ITEMS,
            bytes: EVIDENCE_RETAINED_BYTES,
            scratch_bytes: limits.json_bytes,
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
        model::Type::Definition { arguments, .. } => {
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
        | model::Type::ImplTrait { .. }
        | model::Type::Projection { .. } => Err(representability::reject::Kind::CallableProtocol),
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
        model::Type::TraitObject { .. }
        | model::Type::ImplTrait { .. }
        | model::Type::Projection { .. } => true,
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
            scratch_limit: authority.scratch_bytes,
            retained_items: Cell::new(graph.allocation().items),
            retained_bytes: Cell::new(graph.allocation().bytes),
            scratch_bytes: Cell::new(0),
            trait_self: Cell::new(None),
            scope_lifetimes: Cell::new(false),
            account: Account::new(),
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

    /// The classified retained targets are accounting targets recorded
    /// on the classification account; the charge continues. Only checked
    /// overflow is terminal.
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
        self.account
            .exceeded("classified retained items", self.item_limit, actual_items);
        self.account
            .exceeded("classified retained bytes", self.byte_limit, actual_bytes);
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

    /// The scratch target is recorded, never a gate; the peak is kept.
    fn scratch_arrays<T>(&self, count: usize, arrays: usize) -> Result<(), Error> {
        let bytes = std::mem::size_of::<T>()
            .checked_mul(count)
            .and_then(|bytes| bytes.checked_mul(arrays))
            .ok_or_else(arithmetic)?;
        self.account
            .exceeded("classification scratch bytes", self.scratch_limit, bytes);
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
        self.declared_parameters(generics, &[], false)
    }

    /// The generic parameters, but those in `strings`, which take a `String`.
    /// `function` admits a function's argument `impl Trait`, which no type
    /// has.
    fn declared_parameters(
        &self,
        generics: &model::Generics,
        strings: &[u32],
        function: bool,
    ) -> Result<Box<[representability::signature::Parameter]>, Failure> {
        // A lifetime parameter is instantiated at `'static` (D558 tier 0):
        // it is not declared, and `function` refuses a signature whose
        // inputs borrow at it.
        for parameter in generics
            .parameters()
            .filter(|parameter| !strings.contains(&parameter.index()))
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
            .filter(|parameter| !strings.contains(&parameter.index()))
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

    /// The synthetic parameters of `impl AsRef<str>` or `impl Into<String>`
    /// arguments. Rawr passes a `String`, which is both.
    fn string_parameters(&self, generics: &model::Generics) -> Vec<u32> {
        generics
            .parameters()
            .filter(|parameter| parameter.name().starts_with("impl "))
            .filter(|parameter| {
                let declared = match parameter.kind() {
                    model::ParameterKind::Type { bounds, .. } => bounds.iter().collect::<Vec<_>>(),
                    _ => return false,
                };
                let predicated = generics
                    .predicates()
                    .filter_map(|predicate| match predicate {
                        model::Predicate::Bound {
                            ty: model::Type::Parameter { index, .. },
                            bound,
                        } if *index == parameter.index() => Some(bound),
                        _ => None,
                    });
                let traits = declared
                    .into_iter()
                    .chain(predicated)
                    .filter_map(|bound| match bound {
                        model::Bound::Trait {
                            trait_id,
                            arguments,
                            polarity: model::Polarity::Positive,
                            ..
                        } => Some((trait_id, arguments)),
                        _ => None,
                    })
                    .filter(|(trait_id, _)| self.definition_name(trait_id) != Some("Sized"))
                    .collect::<Vec<_>>();
                !traits.is_empty()
                    && traits.iter().all(|(trait_id, arguments)| {
                        let target = arguments.iter().rev().find_map(|argument| match argument {
                            model::Argument::Type(ty) => Some(ty),
                            _ => None,
                        });
                        match (self.definition_name(trait_id), target) {
                            (Some("AsRef"), Some(model::Type::Primitive(name))) => &**name == "str",
                            (Some("Into"), Some(model::Type::Definition { id, .. })) => self
                                .graph
                                .definition_index(id)
                                .and_then(|index| self.graph.definitions().nth(index))
                                .is_some_and(is_string),
                            _ => false,
                        }
                    })
            })
            .map(model::Parameter::index)
            .collect()
    }

    fn function_parameters(
        &self,
        generics: &model::Generics,
        strings: &[u32],
    ) -> Result<FunctionParameters, Failure> {
        let mut parameters = self.declared_parameters(generics, strings, true)?;
        if generics.predicates().len() == 0 {
            return Ok(FunctionParameters {
                parameters,
                callable: None,
                binders: Box::default(),
                predicates: Box::default(),
                call: None,
            });
        }
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
                // output has: rustc checks it where the bridge names `R`.
                model::Predicate::TypeOutlives {
                    ty: model::Type::Parameter { .. },
                    lifetime: model::Lifetime::Static,
                } => {}
                // A default method's bound on `Self`, as Freya's `on_press`
                // has `where Self: AccessibilityExt`: rustc checks it where
                // the bridge calls the method on its type.
                model::Predicate::Bound {
                    ty: model::Type::SelfType(_),
                    ..
                } => {}
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
        let class = match (class, static_owned) {
            ("Fn", false) => crate::surface::callable::Class::Borrowed(
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
            ("FnMut", false) => {
                return Err(representability::reject::Kind::NonStaticCallable.into());
            }
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
        let mut found = self.graph.implementations().filter_map(|implementation| {
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
    /// other than a future's `Output`, with a synthetic type parameter
    /// appended to stand for it, as `render_stateful_widget`'s `&mut
    /// W::State`: `(W, trait, associated name, synthetic index)`.
    fn projected_parameters(
        &self,
        generics: &model::Generics,
        signature: &model::Signature,
        futures: &[(u32, api::identity::Definition)],
        parameters: &mut Box<[representability::signature::Parameter]>,
    ) -> Vec<Projected> {
        use representability::signature;
        fn find(
            ty: &model::Type,
            found: &mut Vec<(u32, api::identity::Definition, api::identity::Definition)>,
        ) {
            match ty {
                model::Type::Projection {
                    trait_id,
                    associated,
                    arguments,
                } => {
                    if let Some(model::Argument::Type(model::Type::Parameter { index, .. })) =
                        arguments.first()
                        && !found.iter().any(|(known, ..)| known == index)
                    {
                        found.push((*index, trait_id.clone(), associated.clone()));
                    }
                }
                model::Type::Reference { ty, .. } => find(ty, found),
                _ => {}
            }
        }
        let mut found = Vec::new();
        for input in signature.inputs() {
            find(input, &mut found);
        }
        let mut projected = Vec::new();
        let mut extended = parameters.to_vec();
        for (index, trait_id, associated) in found {
            if futures.iter().any(|(future, _)| *future == index)
                || !generics
                    .parameters()
                    .any(|parameter| parameter.index() == index)
            {
                continue;
            }
            let Some(name) = self.definition_name(&associated).map(str::to_owned) else {
                continue;
            };
            let Some(owner) = extended.iter().find(|parameter| parameter.index() == index) else {
                continue;
            };
            let synthetic = signature::future_output(index);
            let declared = format!("{}{name}", owner.name());
            extended.push(signature::Parameter::new(
                synthetic,
                declared.into_boxed_str(),
                signature::Kind::Type,
            ));
            projected.push(Projected {
                parameter: index,
                trait_id,
                name,
                synthetic,
            });
        }
        *parameters = extended.into_boxed_slice();
        projected
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
        /// More implementations than this are not read as cases.
        const CASES: usize = 64;
        let mut relations = Vec::new();
        for projection in projected {
            let mut cases = Vec::new();
            for implementation in self.graph.implementations() {
                let model::Trait::Trait { id, .. } = implementation.trait_ref() else {
                    continue;
                };
                if *id != projection.trait_id
                    || implementation.polarity() != model::Polarity::Positive
                {
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
                        } if definition.display_path().last() == Some(projection.name.as_str()) => {
                            Some(ty)
                        }
                        _ => None,
                    }
                });
                let Some(Ok(value)) = value.map(|value| self.convert(value)) else {
                    continue;
                };
                if names_other_parameter(&this, u32::MAX) || names_other_parameter(&value, u32::MAX)
                {
                    continue;
                }
                cases.push(Case::new(this, Box::new([value])));
                if cases.len() > CASES {
                    break;
                }
            }
            if cases.is_empty() || cases.len() > CASES {
                continue;
            }
            self.retain_array::<Case>(cases.len())?;
            relations.push(Relation::new(
                projection.parameter,
                Box::new([representability::convert::Type::Parameter(
                    projection.synthetic,
                )]),
                cases.into_boxed_slice(),
            ));
        }
        Ok(relations)
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
        /// More implementations than this are not read as cases.
        const CASES: usize = 64;
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
            if !binders.is_empty() || arguments.is_empty() {
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
            // `T: Mul<T>` ties nothing new; `R: SampleRange<T>` ties `T`.
            if !converted.iter().any(|ty| names_other_parameter(ty, *index)) {
                continue;
            }
            let mut cases = Vec::new();
            for implementation in self.graph.implementations() {
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
                cases.push(Case::new(marked(this), implemented));
                if cases.len() > CASES {
                    break;
                }
            }
            if cases.is_empty() || cases.len() > CASES {
                continue;
            }
            self.retain_array::<Case>(cases.len())?;
            relations.push(Relation::new(*index, converted, cases.into_boxed_slice()));
        }
        self.retain_array::<Relation>(relations.len())?;
        Ok(relations.into_boxed_slice())
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
        /// More implementations than this are not read as cases.
        const CASES: usize = 64;
        let mut cases = Vec::<Case>::new();
        for implementation in self.graph.implementations() {
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
            // A tuple, as `(u8, u8, u8)`, is a value Rawr holds.
            let Ok(source) = self.convert_value(source) else {
                continue;
            };
            if names_other_parameter(&source, u32::MAX) {
                continue;
            }
            let case = Case::new(source, Box::new([target.clone()]));
            if !cases.contains(&case) {
                if cases.len() == CASES {
                    return Ok(Box::default());
                }
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
    pub(crate) const fn implementations(&self) -> &[ImplementationRecord] {
        &self.implementations
    }
}

impl ImplementationRecord {
    pub(crate) const fn associated(&self) -> &[OutcomeRecord] {
        &self.associated
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
    let implementation_ledgers = outcomes.iter().try_fold(0_usize, |count, outcome| {
        count.checked_add(outcome.implementations.len())
    })?;
    let associated = outcomes.iter().try_fold(0_usize, |count, outcome| {
        outcome
            .implementations
            .iter()
            .try_fold(count, |count, implementation| {
                count.checked_add(implementation.associated.len())
            })
    })?;

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
        let mut state = serializer.serialize_struct("Outcome", 7)?;
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
    pub fn classify_with_facade(
        self,
        policy: &Policy,
        limits: &crate::cargo::Limits,
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
        limits: &crate::cargo::Limits,
    ) -> Result<Contract<Classified>, Error> {
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
        // The outcome count is an accounting target recorded on the
        // classification account; every export is still classified.
        context.account.exceeded(
            "classification outcomes",
            authority.items,
            self.exports().len(),
        );
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
            context
                .account
                .exceeded("canonical contract bytes", authority.bytes, measured.bytes);
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
        chain: Option<crate::node::Chain>,
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
        limits: &crate::cargo::Limits,
    ) -> Result<Self, Error> {
        // The cached contract's size and recounted usage are accounting
        // targets recorded on one classification account, never refusals.
        let account = Account::new();

        (|| {
            account.exceeded(
                "cached classified contract bytes",
                limits.classified_contract_bytes,
                bytes.len(),
            );
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
            account.exceeded(
                "classified retained items",
                limits.classified_contract_items,
                retained.0,
            );
            account.exceeded(
                "classified retained bytes",
                limits.classified_contract_bytes,
                retained.1,
            );
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
                    && span == *expected_span =>
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
    pub(crate) fn measure(
        self,
        policy: &Policy,
        limits: &crate::cargo::Limits,
    ) -> Result<ClassificationEvidence, EvidenceError> {
        let session = crate::process::EvidenceSession::new();
        let mut classified = None;
        session.observe_inline(crate::process::EvidenceInlineStage::Classification, || {
            classified = Some(
                self.graph
                    .classify_with_scratch(policy, Authority::evidence(limits)),
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
}

impl<'graph> Names<'graph> {
    fn new(graph: &'graph api::Graph<impl api::Phase>, plans: &'graph [DefinitionPlan]) -> Self {
        let models = graph
            .definitions()
            .map(|model| (crate::surface::identity::Definition::of(model.id()), model))
            .collect();
        let mut exported = std::collections::BTreeMap::<_, std::collections::BTreeSet<_>>::new();
        for path in graph.exports() {
            if let ledger::Path::Imported(export) = path {
                exported
                    .entry(export.key().root_alias())
                    .or_default()
                    .insert(crate::surface::identity::Definition::of(export.target()));
            }
        }
        Self {
            plans,
            models,
            exported,
            settled: std::collections::BTreeMap::new(),
            visiting: Vec::new(),
            resolved: Vec::new(),
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
    for plan in plans {
        let keep =
            |field: &representability::structure::Field| field_type_nameable(field.ty(), &nominal);
        match &mut plan.representability {
            R::Struct(structure) => structure.retain_fields(keep),
            R::Enum(enumeration) => enumeration.retain_fields(keep),
            _ => {}
        }
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
            nominal.contains(definition)
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
        } => field_type_nameable(payload, nominal),
        Type::Result(ok, err) => {
            field_type_nameable(ok, nominal) && field_type_nameable(err, nominal)
        }
        Type::Tuple(members) => members
            .iter()
            .all(|member| field_type_nameable(member, nominal)),
        Type::Unit | Type::Builtin(_) | Type::Parameter(_) => true,
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
            // iced's facade names a future or a sipper; no Rust definition does.
            if crate::surface::identity::Definition::is_projected(definition.as_bytes()) {
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
        } => type_nameable(payload, root, names),
        representability::convert::Type::Result(ok, err) => {
            all_nameable([&**ok, &**err], root, names)
        }
        representability::convert::Type::Tuple(members) => {
            all_nameable(members.iter(), root, names)
        }
        representability::convert::Type::Unit
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
                // type it names, with that type's members.
                let aliased = self.alias_target(definition);
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
                let selector = selector(definition)?;
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
                let implementations = self.implementations(export, definitions)?;
                Ok(OutcomeRecord {
                    export: export.key().clone(),
                    identity,
                    name: name.into(),
                    origin: Some(origin),
                    evidence,
                    implementations,
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

    fn implementations(
        &self,
        export: &ledger::Export,
        definitions: &[DefinitionPlan],
    ) -> Result<Box<[ImplementationRecord]>, Error> {
        let mut outcomes = Vec::new();
        for implementation in self.graph.implementations().filter(|implementation| {
            implementation
                .relevance()
                .any(|definition| definition == export.target())
        }) {
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
                let selector = selector(definition)?;
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
                let evidence = self.classify_associated(
                    implementation,
                    definition,
                    definitions,
                    export.target(),
                )?;
                associated.push(OutcomeRecord {
                    export: export.key().clone(),
                    identity: item_identity,
                    name: name.into(),
                    origin: Some(origin),
                    evidence,
                    implementations: Box::default(),
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
            _ if implementation.generics().parameters().next().is_some()
                && lifetime_only(implementation.generics())
                && !self.own_form(implementation.self_type(), target)
                && !self.sole_instance_member(implementation, definition, target) =>
            {
                Some(reject::Kind::GenericImplementation)
            }
            // A trait's impl for another type that names this one, as
            // crossterm's `impl<D> AsRef<ContentStyle> for StyledContent<D>`
            // names `ContentStyle`, has no members of this type: its `as_ref`
            // is `StyledContent`'s.
            _ if matches!(implementation.trait_ref(), model::Trait::Trait { .. })
                && matches!(implementation.self_type(), model::Type::Definition { id, .. } if id != target) =>
            {
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
            let mut own = parameters.iter();
            let mut fixed = false;
            for argument in arguments {
                match argument {
                    model::Argument::Type(model::Type::Parameter { owner, index }) => {
                        let Some(parameter) = own.next() else {
                            return Err(reject::Kind::GenericImplementation.into());
                        };
                        if owner != implementation.id()
                            || *index != parameter.index()
                            || !matches!(parameter.kind(), model::ParameterKind::Type { .. })
                        {
                            return Err(reject::Kind::GenericImplementation.into());
                        }
                    }
                    model::Argument::Type(_) => fixed = true,
                    _ => return Err(reject::Kind::GenericImplementation.into()),
                }
            }
            if own.next().is_some() {
                return Err(reject::Kind::GenericImplementation.into());
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
            let container_parameters = parameters.len();
            let inherited = model::Generics::inherited(implementation.generics(), generics)
                .map_err(|_| Error::Allocation)?;
            let Representability::FunctionItem(item) = self.function(signature, &inherited)? else {
                return Err(reject::Kind::GenericImplementation.into());
            };
            // An invocation's signature declares each of its parameters, and
            // an argument's `impl Fn(..)` has no name to declare, as in
            // Freya's `Callback<A, R>::new(impl FnMut(A) -> R + 'static)`,
            // unless Rawr makes its closure, as pane_grid's `PaneGrid::new(
            // &state, impl Fn(Pane, &S, bool) -> Content)`.
            if item.call().is_none()
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
        let parameters = self.parameters(generics)?;
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
        for field in fields.public_fields() {
            // A public field of a type Rawr cannot hold (palette's
            // `standard: PhantomData<S>`) is unavailable by itself. The
            // struct stays, without a constructor, as with a private field.
            let ty = match self.convert(field.ty()) {
                Ok(ty) => ty,
                Err(Failure::Reject(_)) => {
                    constructible = false;
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
        let constructor = (constructible && !declaration_only)
            .then_some(crate::surface::identity::Selector::StructConstructor(form));
        Ok(representability::structure::Plan::new(
            constructor,
            parameters,
            fields.form(),
            admitted.into_boxed_slice(),
            constructible,
            declaration_only,
            self.implements_for_any(definition, "Copy"),
            // A generic struct reached at its defaulted instance has the
            // `Default` of an impl for any arguments, as palette's `impl<S,
            // T: Stimulus> Default for Rgb<S, T>`: `cargo check` holds the
            // instance to the impl's bounds.
            if declaration_only {
                self.implements(definition, "Default")
            } else {
                self.implements_for_any(definition, "Default")
            },
            self.implements_for_any(definition, "Debug"),
            self.implements_for_any(definition, "Clone"),
            self.implements_for_any(definition, "PartialEq"),
            self.implements_for_any(definition, "Eq"),
            self.implements_for_any(definition, "Hash"),
        )
        .with_borrowing(
            lifetime_parameters(generics),
            self.implements_for_any(definition, "Iterator"),
        )
        .with_view(
            lifetime_parameters(generics) != 0 && self.renders(generics),
            self.implements_iced(definition, "Widget"),
        ))
    }

    /// An input native code gives a script closure: a value, or a copy of
    /// what a shared reference names, as pane_grid's `view(pane, &state,
    /// is_maximized)` gives `&S`.
    fn convert_closure_input(
        &self,
        input: &model::Type,
    ) -> Option<representability::convert::Type> {
        if let Ok(input) = self.convert_value(input) {
            return Some(input);
        }
        let model::Type::Reference {
            mutability: model::Mutability::Shared,
            ty,
            ..
        } = input
        else {
            return None;
        };
        let target = self.convert_value(ty).ok()?;
        matches!(
            target,
            representability::convert::Type::Parameter(_)
                | representability::convert::Type::Nominal { .. }
        )
        .then(|| representability::convert::Type::Borrowed {
            mutable: false,
            ty: Box::new(target),
        })
    }

    /// Whether `output` is an iced view type ([`Self::renders`]).
    fn view_output(&self, output: &model::Type) -> bool {
        let model::Type::Definition { id, .. } = output else {
            return false;
        };
        self.graph
            .definition_index(id)
            .and_then(|index| self.graph.definitions().nth(index))
            .is_some_and(|definition| match definition.kind() {
                model::Kind::Struct { generics, .. } => {
                    lifetime_parameters(generics) != 0 && self.renders(generics)
                }
                _ => false,
            })
    }

    /// Whether `generics` bound a type parameter by iced's `Renderer`, as
    /// an iced widget's do: its lifetime is iced's view lifetime.
    fn renders(&self, generics: &model::Generics) -> bool {
        generics.predicates().any(|predicate| {
            matches!(predicate, model::Predicate::Bound {
                ty: model::Type::Parameter { .. },
                bound: model::Bound::Trait { trait_id, .. },
            } if self.iced_trait(trait_id, "Renderer"))
        })
    }

    /// Whether `id` is iced's own trait `name`, in `iced_core`.
    fn iced_trait(&self, id: &api::identity::Definition, name: &str) -> bool {
        id.unit().crate_name() == "iced_core" && self.definition_name(id) == Some(name)
    }

    /// Whether iced's trait `name` is implemented for the definition at
    /// `definition`, at any arguments.
    fn implements_iced(&self, definition: usize, name: &str) -> bool {
        let Some(definition) = self.graph.definitions().nth(definition) else {
            return false;
        };
        self.graph.implementations().any(|implementation| {
            implementation.polarity() == model::Polarity::Positive
                && matches!(implementation.trait_ref(), model::Trait::Trait { id, .. }
                    if self.iced_trait(id, name))
                && any_arguments(implementation.self_type(), definition.id())
        })
    }

    fn enumeration(
        &self,
        definition: usize,
        variants: &[model::Variant],
        generics: &model::Generics,
    ) -> Result<representability::enumeration::Plan, Failure> {
        let parameters = self.parameters(generics)?;
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
            for field in variant.fields() {
                if !matches!(field.visibility(), model::Visibility::Public) {
                    constructible = false;
                    continue;
                }
                // A field of a type Rawr cannot hold is unavailable by
                // itself, as in a struct. The variant stays and cannot be
                // built.
                let ty = match self.convert(field.ty()) {
                    Ok(ty) => ty,
                    Err(Failure::Reject(_)) => {
                        constructible = false;
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
            admitted.push(representability::enumeration::Variant::new(
                variant_selector,
                variant.name().into(),
                variant.field_set().form(),
                fields.into_boxed_slice(),
                constructible,
                variant.span().clone(),
            ));
        }
        Ok(representability::enumeration::Plan::new(
            parameters,
            admitted.into_boxed_slice(),
            declaration_only,
            self.implements_for_any(definition, "Copy"),
            self.implements(definition, "Default"),
            self.implements_for_any(definition, "Debug"),
            self.implements_for_any(definition, "Clone"),
            self.implements_for_any(definition, "PartialEq"),
            self.implements_for_any(definition, "Eq"),
            self.implements_for_any(definition, "Hash"),
        )
        .with_borrowing(
            lifetime_parameters(generics),
            self.implements_for_any(definition, "Iterator"),
        ))
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
        let ty = self.convert_value(ty)?;
        // A constant of another type, as Freya's `Color::WHITE: Color =
        // Color::new(..)`, is read by its path where the bridge runs, as a
        // static is: each read is a new value, as Rust's is.
        if !matches!(ty, Type::Unit | Type::Builtin(_)) {
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
    fn statik(&self, ty: &model::Type) -> Result<representability::constant::Plan, Failure> {
        use representability::convert::Type;
        let ty = self.convert_value(ty)?;
        if !matches!(
            ty,
            Type::Unit | Type::Builtin(_) | Type::Sequence { owned: false, .. }
        ) {
            return Err(representability::reject::Kind::Static.into());
        }
        Ok(representability::constant::Plan::new(ty, None))
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
            ty => self.convert(ty),
        }
    }

    /// Whether the crate's values of `ty` clone, so a slice of them can
    /// be copied out.
    fn clones(&self, ty: &representability::convert::Type) -> bool {
        use representability::convert::Type;
        match ty {
            Type::Unit | Type::Builtin(_) => true,
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
            | Type::Sequence { element, .. } => self.clones(element),
            Type::Result(ok, err) => self.clones(ok) && self.clones(err),
            Type::Tuple(members) => members.iter().all(|member| self.clones(member)),
            Type::FunctionItem { .. } | Type::ProfileMessage { .. } | Type::Parameter(_) => false,
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
        let opens_copies = self.late_binders_open_copies(signature);
        let borrows_receiver = borrows_receiver(signature);
        let outer_scope = self
            .scope_lifetimes
            .replace(opens_copies || borrows_receiver);
        let classified = self.function_with(signature, generics, opens_copies, borrows_receiver);
        self.scope_lifetimes.set(outer_scope);
        classified
    }

    fn function_with(
        &self,
        signature: &model::Signature,
        generics: &model::Generics,
        opens_copies: bool,
        borrows_receiver: bool,
    ) -> Result<representability::Representability, Failure> {
        use representability::{callable, function, method, ownership, Representability};
        if !receiver_binder_is_exact(signature)
            && !late_binders_lend_strings(signature)
            && !late_binders_lend_inputs(signature)
            && !opens_copies
        {
            return Err(representability::reject::Kind::HigherRankedBinder.into());
        }
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
        let scope = if borrows_receiver {
            Some(representability::signature::Scope::Borrows)
        } else if opens_copies {
            Some(representability::signature::Scope::Opens)
        } else if signature.inputs().any(borrows_at_early_lifetime) {
            if matches!(signature.receiver(), model::Receiver::None)
                && signature
                    .inputs()
                    .filter(|input| borrows_at_early_lifetime(input))
                    .all(is_str_reference)
                && names_early_lifetime(signature.output())
            {
                Some(representability::signature::Scope::Opens)
            } else if self.view_output(signature.output()) {
                // An iced view type's constructor, as `PaneGrid::new(&state,
                // view)`: its value is a recipe, which borrows its inputs
                // again at each lowering (D558 tier 2).
                None
            } else {
                return Err(representability::reject::Kind::LifetimeOrConstGeneric.into());
            }
        } else {
            None
        };
        let converted = self.callable_conversion(generics);
        let generics = converted.as_ref().unwrap_or(generics);
        let strings = self.string_parameters(generics);
        let FunctionParameters {
            mut parameters,
            callable: callable_class,
            binders,
            mut predicates,
            call,
        } = self.function_parameters(generics, &strings)?;
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
                parameter.with_bounds(bounds)
            })
            .collect();
        if !futures.is_empty() {
            predicates = predicates
                .iter()
                .copied()
                .filter(|bound| !futures.iter().any(|(index, _)| *index == bound.parameter()))
                .collect();
        }
        // `W::State` of a type parameter `W: StatefulWidget` is a synthetic
        // type parameter the call infers from `W`'s implementation (D558.5).
        let projected = self.projected_parameters(generics, signature, &futures, &mut parameters);
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
                model::Type::Parameter { index, .. } if strings.contains(index) => {
                    representability::convert::Type::Builtin(
                        representability::convert::Builtin::String,
                    )
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
        let output = match signature.receiver() {
            model::Receiver::None | model::Receiver::Value { .. } => {
                self.convert_output(&rewritten)
            }
            model::Receiver::Reference { .. } if signature.async_function() => {
                self.convert_output(&rewritten)
            }
            // A `&[T]` the method lends from its receiver, as QName's
            // `as_ref(&self) -> &[u8]`: a copy, typed as the slice it is,
            // so a slice pattern matches it (D558 slice 2).
            model::Receiver::Reference {
                lifetime: lifetime @ model::Lifetime::Late { .. },
                ..
            } if matches!(&rewritten, model::Type::Reference { lifetime: lent, ty, .. }
                    if lent == lifetime && matches!(&**ty, model::Type::Slice(_))) =>
            {
                self.convert_value(&rewritten).map(|copied| {
                    representability::convert::Type::Borrowed {
                        mutable: false,
                        ty: Box::new(copied),
                    }
                })
            }
            _ => self.convert_value(&rewritten),
        };
        let mut guard = None;
        let output = match output {
            Ok(output) => output,
            // A borrow guard a method gives, as `RefCell::borrow_mut` and
            // Freya's `State::write` do, is the type it dereferences to,
            // reached only inside each call while the guard lives.
            Err(error) => match self.guard_output(signature, &inputs)? {
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
        // A script closure fills a callable parameter `B` as a type Rawr
        // has no name for, as it does a future parameter, so a signature
        // can name `B` only as the input that takes the closure. Freya's
        // `VirtualScrollView<D, B>::auto_hide_scrollbars(self, bool) -> Self`
        // names it in its receiver and output.
        if let Some((callable, ..)) = &call
            && inputs
                .iter()
                .filter(|input| **input != representability::convert::Type::Parameter(*callable))
                .chain([&output])
                .any(|ty| names_parameter(ty, *callable))
        {
            return Err(representability::reject::Kind::CallableProtocol.into());
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
        .with_scope(scope);
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
            // An argument's `impl Fn(..)` has no name a signature can
            // declare, so only an item whose closure Rawr can make keeps it.
            if call.is_none()
                && signature_plan
                    .parameters()
                    .iter()
                    .any(|parameter| parameter.name().starts_with("impl "))
            {
                return Err(representability::reject::Kind::CallableProtocol.into());
            }
            return Ok(Representability::FunctionItem(
                callable::Item::new(
                    signature_plan,
                    receiver,
                    callable_class,
                    binders,
                    predicates,
                )
                .with_call(call),
            ));
        }
        Ok(match receiver {
            Some(receiver) => Representability::Method(method::Plan::new(receiver, signature_plan)),
            None => Representability::Function(function::Plan::new(signature_plan)),
        })
    }

    fn convert(&self, ty: &model::Type) -> Result<representability::convert::Type, Failure> {
        use representability::convert::{Builtin, Type};
        Ok(match ty {
            model::Type::Tuple(members) if members.is_empty() => Type::Unit,
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
                if is_string(definition) && arguments.is_empty() {
                    Type::Builtin(Builtin::String)
                } else if is_phantom_data(definition) {
                    // `PhantomData<T>` holds nothing, and Rawr's is the unit
                    // type, as a script struct's is: palette's `Rgb` has
                    // `standard: PhantomData<S>`.
                    Type::Unit
                } else if id.unit().crate_name() == "core"
                    && definition.display_path().eq(["core", "time", "Duration"])
                {
                    Type::Nominal {
                        definition: crate::surface::identity::Definition::duration(),
                        arguments: Box::default(),
                    }
                } else if is_i64_range(definition, arguments) {
                    Type::Builtin(Builtin::InclusiveRange)
                } else if let Some(element) = range_element(definition, arguments) {
                    // A float range, as rand's `random_range(0.5..1.0)` takes,
                    // or the impl parameter a relation case binds.
                    let element = self.convert(element)?;
                    if !matches!(
                        element,
                        Type::Builtin(Builtin::F32 | Builtin::F64) | Type::Parameter(_)
                    ) {
                        return Err(
                            representability::reject::Kind::UnsupportedGenericArgument.into()
                        );
                    }
                    Type::Range(Box::new(element))
                } else if let Some(borrowed) = cow_target(definition, arguments) {
                    // alloc's `Cow<'a, [T]>` and `Cow<'a, str>`, as
                    // quick-xml's `Attribute::value` and `decode()` give:
                    // what they hold, copied out as Rawr's array or string,
                    // which no longer borrows (D558 slice 2).
                    match borrowed {
                        model::Type::Slice(element) => Type::Sequence {
                            element: Box::new(self.convert(element)?),
                            owned: false,
                        },
                        model::Type::Primitive(name) if &**name == "str" => {
                            Type::Builtin(Builtin::Text)
                        }
                        _ => {
                            return Err(
                                representability::reject::Kind::UnsupportedGenericArgument.into()
                            )
                        }
                    }
                } else if let Some(element) = option_element(definition, arguments) {
                    Type::Option(Box::new(self.convert(element)?))
                } else if let Some((ok, err)) = result_elements(definition, arguments) {
                    Type::Result(Box::new(self.convert(ok)?), Box::new(self.convert(err)?))
                } else {
                    definition_is_publishable_nominal(definition)?;
                    let mut converted = Vec::new();
                    self.retain_array::<Type>(arguments.len())?;
                    converted
                        .try_reserve_exact(arguments.len())
                        .map_err(|_| Error::Allocation)?;
                    for argument in arguments {
                        let argument = match argument {
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
                            _ => {
                                return Err(
                                    representability::reject::Kind::UnsupportedGenericArgument
                                        .into(),
                                )
                            }
                        };
                        converted.push(match borrowed_argument(argument) {
                            // `Reader<&'a [u8]>`: a shared slice of scalars
                            // or a `&str` a type argument names, which a
                            // scope frame's value borrows (D558 tier 1).
                            Some(target) => Type::Borrowed {
                                mutable: false,
                                ty: Box::new(match target {
                                    model::Type::Slice(element) => Type::Sequence {
                                        element: Box::new(self.convert(element)?),
                                        owned: false,
                                    },
                                    _ => Type::Builtin(Builtin::Str),
                                }),
                            },
                            None => self.convert(argument)?,
                        });
                    }
                    Type::Nominal {
                        definition: crate::surface::identity::Definition::of(id),
                        arguments: converted.into_boxed_slice(),
                    }
                }
            }
            model::Type::Parameter { index, .. } => Type::Parameter(*index),
            model::Type::SelfType(id) => {
                let index = self.graph.implementation_index(id).or_else(|| {
                    // A trait's `Self` in a default method classified as a
                    // member of one implementation is that type.
                    self.trait_self.get().filter(|_| {
                        self.graph
                            .definition_index(id)
                            .and_then(|index| self.graph.definitions().nth(index))
                            .is_some_and(|definition| {
                                matches!(definition.kind(), model::Kind::Trait { .. })
                            })
                    })
                });
                if let Some(index) = index {
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
            model::Type::ImplTrait { .. } => {
                return Err(representability::reject::Kind::OpaqueType.into())
            }
            // `<T as Iterator>::Item` of an `impl Iterator for T` the crate
            // declares, as `next`'s `Option<Self::Item>` names it (D558
            // slice 2).
            model::Type::Projection { .. } => match self.iterator_item(ty) {
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

    /// A function's output. A Rust future, an `impl Future<Output = T>` or
    /// a type that implements `Future`, is iced's `Task<T>`: the bridge
    /// runs it natively and the facade's `Task` carries it.
    fn convert_output(&self, ty: &model::Type) -> Result<representability::convert::Type, Failure> {
        if let model::Type::ImplTrait { bounds, .. } = ty
            && let Some(sipper) = self.sipper_bound(bounds)
        {
            return sipper;
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
        self.graph.implementations().find_map(|implementation| {
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

    /// An `impl Sipper<Output, Progress>`, or an `impl Straw<T, Progress,
    /// Error>`, which is a sipper of `Result<T, Error>`. It crosses as iced's
    /// `task::Sipper<Output, Progress>`. `Never`, sipper's `Infallible`, is
    /// `()` there, since such a sipper never finishes.
    fn sipper_bound(
        &self,
        bounds: &[model::Predicate],
    ) -> Option<Result<representability::convert::Type, Failure>> {
        use representability::convert::Type;
        bounds.iter().find_map(|predicate| {
            let model::Predicate::Bound {
                bound:
                    model::Bound::Trait {
                        trait_id,
                        arguments,
                        polarity: model::Polarity::Positive,
                        ..
                    },
                ..
            } = predicate
            else {
                return None;
            };
            let definition = self
                .graph
                .definition_index(trait_id)
                .and_then(|index| self.graph.definitions().nth(index))?;
            if trait_id.unit().crate_name() != "sipper" {
                return None;
            }
            let name = definition.display_path().last()?;
            let types = arguments
                .iter()
                .filter_map(|argument| match argument {
                    model::Argument::Type(ty) => Some(ty),
                    model::Argument::Lifetime(_) | model::Argument::Const(_) => None,
                })
                .collect::<Vec<_>>();
            let converted = (|| match (name, types.as_slice()) {
                ("Sipper", [.., output, progress]) => {
                    Ok((self.convert_never(output)?, self.convert(progress)?))
                }
                ("Straw", [.., output, progress, error]) => Ok((
                    Type::Result(
                        Box::new(self.convert_never(output)?),
                        Box::new(self.convert_never(error)?),
                    ),
                    self.convert(progress)?,
                )),
                _ => Err(representability::reject::Kind::OpaqueType.into()),
            })();
            Some(converted.map(|(output, progress)| Type::Nominal {
                definition: crate::surface::identity::Definition::sipper(),
                arguments: Box::from([output, progress]),
            }))
        })
    }

    /// `()` for `Never`, `core::convert::Infallible`, which has no values.
    fn convert_never(&self, ty: &model::Type) -> Result<representability::convert::Type, Failure> {
        if let model::Type::Definition { id, .. } = ty
            && id.unit().crate_name() == "core"
            && self
                .graph
                .definition_index(id)
                .and_then(|index| self.graph.definitions().nth(index))
                .is_some_and(|definition| definition.display_path().last() == Some("Infallible"))
        {
            return Ok(representability::convert::Type::Unit);
        }
        self.convert(ty)
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
        self.graph.implementations().find_map(|implementation| {
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

    /// The `Item` of the crate's `impl Iterator for T`, for the projection
    /// `<T as Iterator>::Item` with that exact `T` (D558 slice 2). The
    /// driver keeps an iterator impl's `Item` for this.
    fn iterator_item<'g>(&'g self, ty: &model::Type) -> Option<&'g model::Type> {
        let model::Type::Projection {
            trait_id,
            associated,
            arguments,
        } = ty
        else {
            return None;
        };
        if !self.is_core_trait(trait_id, "Iterator")
            || self
                .graph
                .definition_index(associated)
                .and_then(|index| self.graph.definitions().nth(index))
                .and_then(|definition| definition.display_path().last())
                != Some("Item")
        {
            return None;
        }
        let [model::Argument::Type(self_type)] = &arguments[..] else {
            return None;
        };
        self.graph.implementations().find_map(|implementation| {
            let model::Trait::Trait { id, .. } = implementation.trait_ref() else {
                return None;
            };
            if id != trait_id || implementation.self_type() != self_type {
                return None;
            }
            self.associated_type(implementation, "Item")
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
        vec_element(definition, arguments)
            .filter(|element| matches!(element, model::Type::Primitive(_)))
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
    /// guard is a place each read and store reaches by calling the method
    /// again, so only a method with no inputs besides its receiver is one;
    /// with inputs it is read through `Deref` alone.
    fn guard_output(
        &self,
        signature: &model::Signature,
        inputs: &[representability::convert::Type],
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
            // A place reached again by calling the method again needs its
            // inputs again: copies of values that clone.
            let mutable = *mutability == model::Mutability::Mutable
                && inputs.iter().all(|input| self.clones(input));
            return Ok(Some((
                Type::Option(Box::new(target)),
                if mutable {
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
        let mode = if mutable && inputs.is_empty() && stored {
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
        // `&str`: Rawr's string, lent to the call.
        if is_str_reference(ty) {
            return Ok(representability::convert::Type::Builtin(
                representability::convert::Builtin::Str,
            ));
        }
        // `&mut Vec<T>` of scalars, as quick-xml's `read_event_into` takes
        // its buffer: Rawr's array, copied in and back out (D558 slice 2).
        if let Some(element) = self.copied_vec(ty) {
            return Ok(representability::convert::Type::Borrowed {
                mutable: true,
                ty: Box::new(representability::convert::Type::Sequence {
                    element: Box::new(self.convert(element)?),
                    owned: true,
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
                model::Type::Slice(element) => {
                    return Ok(representability::convert::Type::Borrowed {
                        mutable: false,
                        ty: Box::new(representability::convert::Type::Sequence {
                            element: Box::new(self.convert_value(element)?),
                            owned: false,
                        }),
                    });
                }
                model::Type::Tuple(members) if members.len() >= 2 => {
                    return Ok(representability::convert::Type::Borrowed {
                        mutable: false,
                        ty: Box::new(self.convert_value(target)?),
                    });
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

    /// Whether the crate implements the `core` trait `name` (`Default`)
    /// for the definition itself, without generics.
    fn implements(&self, definition: usize, name: &str) -> bool {
        self.core_implementation(definition, name, false)
    }

    /// Whether the crate implements the `core` trait `name` for the
    /// definition with any arguments, as `impl<Tz> Debug for DateTime<Tz>`
    /// does. Bounds on the arguments are not read: `cargo check` holds the
    /// crate to them.
    fn implements_for_any(&self, definition: usize, name: &str) -> bool {
        self.core_implementation(definition, name, true)
    }

    fn core_implementation(&self, definition: usize, name: &str, generic: bool) -> bool {
        let Some(definition) = self.graph.definitions().nth(definition) else {
            return false;
        };
        self.graph.implementations().any(|implementation| {
            if implementation.polarity() != model::Polarity::Positive
                || (!generic && !lifetime_only(implementation.generics()))
            {
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
            core_trait
                && if generic {
                    any_arguments(implementation.self_type(), definition.id())
                } else {
                    exact_definition(implementation.self_type(), definition.id())
                }
        })
    }
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

/// `&str` or `&'a str`.
fn is_str_reference(ty: &model::Type) -> bool {
    matches!(ty, model::Type::Reference { mutability: model::Mutability::Shared, ty, .. }
        if matches!(&**ty, model::Type::Primitive(name) if &**name == "str"))
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
        && signature
            .inputs()
            .any(|input| matches!(input, model::Type::Reference { .. }))
        && signature.inputs().all(|input| match input {
            model::Type::Reference { ty, .. } => {
                is_str_reference(input) || !names_late_lifetime(ty)
            }
            input => !names_late_lifetime(input),
        })
        && !names_late_lifetime(signature.output())
}

/// A method whose output borrows its receiver, as quick-xml's
/// `BytesStart::name(&self) -> QName<'_>` does: it opens a scope frame that
/// holds the receiver borrowed while the output lives (D558 tier 1, slice
/// 2). A plain `&T` output is read as a borrow guard instead.
fn borrows_receiver(signature: &model::Signature) -> bool {
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
    // opaque or reference output is held to its own rules.
    matches!(output, model::Type::Definition { .. })
        && signature.guard().is_none()
        && !signature.async_function()
        && names_lifetime(output, receiver)
        && !names_opaque(output)
        && signature.binders().len() == 1
        && !signature.inputs().any(names_late_lifetime)
        && !lends_option_reference(output, receiver)
}

/// Whether `output` is an `Option<&'r T>` of the receiver's lifetime `'r`,
/// as pane_grid's `State::get_mut` gives: a borrow guard's place, not a
/// frame (T012).
fn lends_option_reference(output: &model::Type, receiver: &model::Lifetime) -> bool {
    matches!(output, model::Type::Definition { arguments, .. }
        if matches!(&arguments[..], [model::Argument::Type(model::Type::Reference { lifetime, .. })]
            if lifetime == receiver))
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
        model::Type::SelfType(id) if id == from => return None,
        model::Type::FunctionPointer(_)
        | model::Type::TraitObject { .. }
        | model::Type::ImplTrait { .. } => return None,
        other => other.clone(),
    })
}

fn names_other_parameter(ty: &representability::convert::Type, except: u32) -> bool {
    use representability::convert::Type;
    let other = |ty: &Type| names_other_parameter(ty, except);
    match ty {
        Type::Parameter(index) => *index != except,
        Type::Unit | Type::Builtin(_) => false,
        Type::Nominal { arguments, .. } | Type::Tuple(arguments) => arguments.iter().any(other),
        Type::Option(element)
        | Type::Borrowed { ty: element, .. }
        | Type::Range(element)
        | Type::Sequence { element, .. } => other(element),
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
        Type::Unit | Type::Builtin(_) => false,
        Type::Nominal { arguments, .. } => arguments
            .iter()
            .any(|argument| names_parameter(argument, index)),
        Type::Option(element)
        | Type::Borrowed { ty: element, .. }
        | Type::Range(element)
        | Type::Sequence { element, .. } => names_parameter(element, index),
        Type::Result(ok, err) => names_parameter(ok, index) || names_parameter(err, index),
        Type::Tuple(members) => members.iter().any(|member| names_parameter(member, index)),
        Type::FunctionItem { input, output } => {
            names_parameter(input, index) || names_parameter(output, index)
        }
        Type::ProfileMessage { payload, .. } => names_parameter(payload, index),
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
        ty,
    } = ty
    else {
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

/// `T` of alloc's `Vec<T>`, with its allocator or without.
fn vec_element<'a>(
    definition: &model::Definition,
    arguments: &'a [model::Argument],
) -> Option<&'a model::Type> {
    let unit = definition.id().unit();
    if unit.authority().sysroot().is_none()
        || !matches!(unit.crate_name(), "alloc" | "std")
        || definition.display_path().last() != Some("Vec")
    {
        return None;
    }
    match arguments {
        [model::Argument::Type(element)] | [model::Argument::Type(element), _] => Some(element),
        _ => None,
    }
}

/// A projection `W::Assoc` of a type parameter a signature names, and the
/// synthetic parameter that stands for it.
struct Projected {
    parameter: u32,
    trait_id: api::identity::Definition,
    name: String,
    synthetic: u32,
}

/// `ty` with each projection of `projected` written as its synthetic
/// parameter.
fn projected_parameters(ty: &model::Type, projected: &[Projected]) -> model::Type {
    match ty {
        model::Type::Projection {
            trait_id,
            arguments,
            ..
        } => {
            if let Some(model::Argument::Type(model::Type::Parameter { owner, index })) =
                arguments.first()
                && let Some(projection) = projected.iter().find(|projection| {
                    projection.parameter == *index && projection.trait_id == *trait_id
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
        ty => ty.clone(),
    }
}

/// The element type of std's `Rc<[T]>`, `Arc<[T]>` or `Box<[T]>`.
fn shared_slice_element<'a>(
    definition: &model::Definition,
    arguments: &'a [model::Argument],
) -> Option<&'a model::Type> {
    let unit = definition.id().unit();
    if unit.authority().sysroot().is_none()
        || !matches!(unit.crate_name(), "alloc" | "std")
        || !matches!(definition.display_path().last(), Some("Rc" | "Arc" | "Box"))
    {
        return None;
    }
    match arguments {
        [model::Argument::Type(model::Type::Slice(element))]
        | [model::Argument::Type(model::Type::Slice(element)), _] => Some(element),
        _ => None,
    }
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

fn is_string(definition: &model::Definition) -> bool {
    let unit = definition.id().unit();
    unit.authority().sysroot().is_some()
        && matches!(unit.crate_name(), "alloc" | "std")
        && definition.display_path().last() == Some("String")
}

/// core's `marker::PhantomData<T>`.
fn is_phantom_data(definition: &model::Definition) -> bool {
    let unit = definition.id().unit();
    unit.authority().sysroot().is_some()
        && matches!(unit.crate_name(), "core" | "std")
        && definition.display_path().last() == Some("PhantomData")
}

/// `T` of core's `ops::Range<T>`, which Rawr holds as a value when `T` is
/// a float.
fn range_element<'a>(
    definition: &model::Definition,
    arguments: &'a [model::Argument],
) -> Option<&'a model::Type> {
    let unit = definition.id().unit();
    let path = definition.display_path().collect::<Vec<_>>();
    if unit.authority().sysroot().is_none()
        || unit.crate_name() != "core"
        || path.last() != Some(&"Range")
        || !path.contains(&"ops")
    {
        return None;
    }
    match arguments {
        [model::Argument::Type(element)] => Some(element),
        _ => None,
    }
}

/// `B` of alloc's `Cow<'a, B>`.
fn cow_target<'a>(
    definition: &model::Definition,
    arguments: &'a [model::Argument],
) -> Option<&'a model::Type> {
    let unit = definition.id().unit();
    if unit.authority().sysroot().is_none()
        || !matches!(unit.crate_name(), "alloc" | "std")
        || definition.display_path().last() != Some("Cow")
    {
        return None;
    }
    match arguments {
        [model::Argument::Lifetime(_), model::Argument::Type(target)] => Some(target),
        _ => None,
    }
}

/// `T` of core's `Option<T>`, which Rawr represents as its own `Option`.
fn option_element<'a>(
    definition: &model::Definition,
    arguments: &'a [model::Argument],
) -> Option<&'a model::Type> {
    let unit = definition.id().unit();
    if unit.authority().sysroot().is_none()
        || !matches!(unit.crate_name(), "core" | "std")
        || definition.display_path().last() != Some("Option")
    {
        return None;
    }
    match arguments {
        [model::Argument::Type(element)] => Some(element),
        _ => None,
    }
}

/// `T` and `E` of core's `Result<T, E>`, which Rawr represents as its own
/// `Result`.
fn result_elements<'a>(
    definition: &model::Definition,
    arguments: &'a [model::Argument],
) -> Option<(&'a model::Type, &'a model::Type)> {
    let unit = definition.id().unit();
    if unit.authority().sysroot().is_none()
        || !matches!(unit.crate_name(), "core" | "std")
        || definition.display_path().last() != Some("Result")
    {
        return None;
    }
    match arguments {
        [model::Argument::Type(ok), model::Argument::Type(err)] => Some((ok, err)),
        _ => None,
    }
}

fn is_i64_range(definition: &model::Definition, arguments: &[model::Argument]) -> bool {
    let unit = definition.id().unit();
    unit.authority().sysroot().is_some()
        && unit.crate_name() == "core"
        && definition.display_path().last() == Some("RangeInclusive")
        && matches!(
            arguments,
            [model::Argument::Type(model::Type::Primitive(name))] if name.as_ref() == "i64"
        )
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
    Node(#[source] Box<crate::node::Error>),
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

        let standard = crate::cargo::Limits::standard();
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

        let mut exact = crate::cargo::Limits::fixture(usize::MAX, expected_items, usize::MAX);
        exact.classified_contract_items = expected_items;
        crate::test_fixture::support_outcomes()
            .classify(&Policy::standard(), &exact)
            .expect("the exact retained-item boundary accepts");
        let mut short = crate::cargo::Limits::fixture(
            usize::MAX,
            expected_items
                .checked_sub(1)
                .expect("fixture retains at least one item"),
            usize::MAX,
        );
        short.classified_contract_items = expected_items - 1;
        // The one-short retained-item target is recorded on the
        // classification account, never refused; the output is byte-identical.
        let short_contract = crate::test_fixture::support_outcomes()
            .classify(&Policy::standard(), &short)
            .expect("the one-item-short retained boundary records and continues");
        assert_eq!(contract.canonical_bytes(), short_contract.canonical_bytes());
        assert_eq!(contract.fingerprint(), short_contract.fingerprint());

        let mut exact_bytes = crate::cargo::Limits::fixture(expected_bytes, usize::MAX, usize::MAX);
        exact_bytes.classified_contract_bytes = expected_bytes;
        crate::test_fixture::support_outcomes()
            .classify(&Policy::standard(), &exact_bytes)
            .expect("the exact retained-byte boundary accepts");
        let mut short_bytes = crate::cargo::Limits::fixture(
            expected_bytes
                .checked_sub(1)
                .expect("fixture retains at least one byte"),
            usize::MAX,
            usize::MAX,
        );
        short_bytes.classified_contract_bytes = expected_bytes - 1;
        let short_bytes_contract = crate::test_fixture::support_outcomes()
            .classify(&Policy::standard(), &short_bytes)
            .expect("the one-byte-short retained boundary records and continues");
        assert_eq!(
            contract.canonical_bytes(),
            short_bytes_contract.canonical_bytes()
        );
        assert_eq!(contract.fingerprint(), short_bytes_contract.fingerprint());

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
            .classify(&Policy::standard(), &crate::cargo::Limits::standard())
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

        let standard = crate::cargo::Limits::standard();
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

    #[test]
    fn raw_and_contract_fingerprints_invalidate_independently() {
        let graph = crate::test_fixture::graph();
        let raw = graph.fingerprint();
        let standard = graph
            .classify(&Policy::standard(), &crate::cargo::Limits::standard())
            .expect("standard policy classifies");
        let prior = crate::test_fixture::graph()
            .classify(
                &Policy::with_versions(super::CONTRACT_SCHEMA - 1, super::CLASSIFIER_VERSION - 1),
                &crate::cargo::Limits::standard(),
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
            .classify(&Policy::standard(), &crate::cargo::Limits::standard())
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
            .classify(&Policy::standard(), &crate::cargo::Limits::standard())
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

    #[test]
    fn core_result_signatures_are_admitted_as_rawr_results() {
        use super::representability::convert::{Builtin, Type};
        let contract = crate::test_fixture::result_function()
            .classify(&Policy::standard(), &crate::cargo::Limits::standard())
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
            .classify(&Policy::standard(), &crate::cargo::Limits::standard())
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
    }

    #[test]
    fn remaining_representability_families_are_closed_and_positioned() {
        let contract = crate::test_fixture::representability_families()
            .classify(&Policy::standard(), &crate::cargo::Limits::standard())
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
        let limits = crate::cargo::Limits::standard();
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
            }
        );
        assert!(plan.value().is_none());

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
        let limits = crate::cargo::Limits::standard();
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

    /// A trait's impl for another type that names this one, as crossterm's
    /// `impl<D> AsRef<ContentStyle> for StyledContent<D>` names
    /// `ContentStyle`, gives this type no members: its `as_ref` would
    /// repeat `ContentStyle::as_ref`'s input signature.
    #[test]
    fn a_trait_impl_for_another_type_gives_no_members() {
        use super::representability::reject::Kind;
        use crate::api::model;

        let graph = crate::test_fixture::graph();
        let limits = crate::cargo::Limits::standard();
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

    /// The classified item and retained byte targets are accounting
    /// targets. Crossing them records on the classification account and the
    /// pass continues, producing a contract byte-identical to the one the
    /// standard limits produce.
    #[test]
    fn classification_records_item_and_retained_byte_targets_and_continues() {
        let standard = crate::test_fixture::graph()
            .classify(&Policy::standard(), &crate::cargo::Limits::standard())
            .expect("fixture classifies under the standard limits");

        let mut short_items = crate::cargo::Limits::fixture(usize::MAX, usize::MAX, usize::MAX);
        short_items.classified_contract_items = 5;
        let recorded_items = crate::test_fixture::graph()
            .classify(&Policy::standard(), &short_items)
            .expect("five classified items against seven exports records and continues");
        assert_eq!(standard.canonical_bytes(), recorded_items.canonical_bytes());
        assert_eq!(standard.fingerprint(), recorded_items.fingerprint());

        let mut short_bytes = crate::cargo::Limits::fixture(usize::MAX, usize::MAX, usize::MAX);
        short_bytes.classified_contract_bytes = 8;
        let recorded_bytes = crate::test_fixture::graph()
            .classify(&Policy::standard(), &short_bytes)
            .expect("eight classified bytes records and continues");
        assert_eq!(standard.canonical_bytes(), recorded_bytes.canonical_bytes());
        assert_eq!(standard.fingerprint(), recorded_bytes.fingerprint());

        // The outcome-count target is recorded on the classification account
        // inside `classify_parts`; every export is still classified.
        let graph = crate::test_fixture::graph();
        let exports = graph.exports().len();
        assert_eq!(exports, 7);
        let unbounded = super::Authority {
            items: usize::MAX,
            bytes: usize::MAX,
            scratch_bytes: usize::MAX,
        };
        let (reference, _) = graph
            .classify_parts(&Policy::standard(), unbounded)
            .expect("fixture classifies under an unbounded authority");
        let (recorded_outcomes, _) = graph
            .classify_parts(
                &Policy::standard(),
                super::Authority {
                    items: exports - 1,
                    ..unbounded
                },
            )
            .expect("one outcome short of the export count records and continues");
        assert_eq!(reference.canonical, recorded_outcomes.canonical);
        assert_eq!(reference.fingerprint, recorded_outcomes.fingerprint);
        assert_eq!(recorded_outcomes.outcomes.len(), exports);

        // Driving the context directly exposes the account: the exact target
        // admits silently, one short records the truthful high water.
        let settled = super::Context::new(&graph, unbounded)
            .expect("fixture context initializes unbounded")
            .retained_items
            .get();
        let exact = super::Context::new(
            &graph,
            super::Authority {
                items: settled,
                ..unbounded
            },
        )
        .expect("the exact classified item target initializes");
        assert_eq!(exact.retained_items.get(), settled);
        assert!(!exact.account.exceedances().is_exceeded());

        let short = super::Context::new(
            &graph,
            super::Authority {
                items: settled - 1,
                ..unbounded
            },
        )
        .expect("one classified item short of the settled usage initializes");
        assert_eq!(short.retained_items.get(), settled);
        let record = short.account.exceedances();
        let axis = record
            .axis("classified retained items")
            .expect("the item target records on the classification account");
        assert_eq!(axis.target, (settled - 1) as u64);
        assert_eq!(axis.first_requested, settled as u64);
        assert_eq!(axis.high_water, settled as u64);
        assert!(record.axis("classified retained bytes").is_none());
        assert!(record.axis("classification scratch bytes").is_none());

        short
            .retain_items(1)
            .expect("retaining past a recorded target continues");
        assert_eq!(short.retained_items.get(), settled + 1);
        let axis = *short
            .account
            .exceedances()
            .axis("classified retained items")
            .expect("the item axis persists");
        assert_eq!(axis.target, (settled - 1) as u64);
        assert_eq!(axis.first_requested, settled as u64);
        assert_eq!(axis.high_water, (settled + 1) as u64);
    }

    #[test]
    fn production_contract_authority_is_exact_atomic_and_stage_isolated() {
        let limits = crate::cargo::Limits::standard();
        let authority = super::Authority::production(&limits);
        assert_eq!(authority.items, 6_500_000);
        assert_eq!(authority.bytes, 671_088_640);
        assert_eq!(authority.scratch_bytes, 33_554_432);

        let graph = crate::test_fixture::graph();
        let context = super::Context::new(&graph, authority).expect("fixture context is bounded");
        let remaining_items = authority.items - context.retained_items.get();
        context
            .retain_items(remaining_items)
            .expect("the exact classified item boundary succeeds");
        let retained_items = context.retained_items.get();
        assert_eq!(retained_items, 6_500_000);
        assert!(!context.account.exceedances().is_exceeded());
        // One past the exact target records on the classification
        // account with the truthful high water; the charge continues.
        context
            .retain_items(1)
            .expect("one item past the classified item target records and continues");
        assert_eq!(context.retained_items.get(), retained_items + 1);
        let record = context.account.exceedances();
        let items_axis = record
            .axis("classified retained items")
            .expect("the item target records on the classification account");
        assert_eq!(items_axis.target, 6_500_000);
        assert_eq!(items_axis.first_requested, 6_500_001);
        assert_eq!(items_axis.high_water, 6_500_001);
        assert!(record.axis("classified retained bytes").is_none());

        let context = super::Context::new(&graph, authority).expect("fixture context is bounded");
        let remaining_bytes = authority.bytes - context.retained_bytes.get();
        context
            .retain(0, remaining_bytes)
            .expect("the exact classified byte boundary succeeds");
        let retained_bytes = context.retained_bytes.get();
        assert_eq!(retained_bytes, 671_088_640);
        assert!(!context.account.exceedances().is_exceeded());
        context
            .retain(0, 1)
            .expect("one byte past the classified byte target records and continues");
        assert_eq!(context.retained_bytes.get(), retained_bytes + 1);
        let record = context.account.exceedances();
        let bytes_axis = record
            .axis("classified retained bytes")
            .expect("the byte target records on the classification account");
        assert_eq!(bytes_axis.target, 671_088_640);
        assert_eq!(bytes_axis.first_requested, 671_088_641);
        assert_eq!(bytes_axis.high_water, 671_088_641);
        assert!(record.axis("classified retained items").is_none());

        let mut graph_only = limits;
        graph_only.retained_graph_items -= 1;
        graph_only.retained_graph_bytes -= 1;
        let unchanged = super::Authority::production(&graph_only);
        assert_eq!(unchanged.items, authority.items);
        assert_eq!(unchanged.bytes, authority.bytes);
        assert_eq!(super::Authority::evidence(&limits).items, 6_000_000);
        assert_eq!(super::Authority::evidence(&limits).bytes, 3_221_225_472);
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
        let production = crate::cargo::Limits::standard();
        let authority = super::Authority::evidence(&production);
        assert_eq!(authority.items, super::EVIDENCE_RETAINED_ITEMS);
        assert_eq!(authority.bytes, super::EVIDENCE_RETAINED_BYTES);
        assert_eq!(authority.scratch_bytes, production.json_bytes);
        let evidence = crate::test_fixture::support_outcomes()
            .into_classification_evidence()
            .measure(&Policy::standard(), &production)
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
            .measure(&Policy::standard(), &crate::cargo::Limits::standard())
            .expect("zero-implementation classification evidence completes");
        assert_eq!(zero.implementations, 0);
        assert_eq!(zero.implementation_ledgers, 0);
        assert_eq!(zero.associated, 0);

        let evidence = crate::test_fixture::classification_implementation_outcomes()
            .into_classification_evidence()
            .measure(&Policy::standard(), &crate::cargo::Limits::standard())
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
        let limits = crate::cargo::Limits::standard();
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
