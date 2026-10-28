//! Pinned rustc callback and atomically published external-metadata fragments.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt;
use std::fs::{self, OpenOptions};
use std::hash::{DefaultHasher, Hash as _, Hasher as _};
use std::io::{self, Cursor, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use rustc_driver::{Callbacks, Compilation};
use rustc_hir as hir;
use rustc_hir::def::{CtorKind, CtorOf, DefKind, Namespace as RustNamespace, Res};
use rustc_hir::definitions::DefPathData;
use rustc_infer::infer::TyCtxtInferExt;
use rustc_middle::metadata::Reexport as RustReexport;
use rustc_middle::mir;
use rustc_middle::ty::{self, Ty, TyCtxt, TypeVisitableExt};
use rustc_span::def_id::{CrateNum, DefId};
use rustc_span::symbol::kw;
use rustc_span::{Span as RustSpan, Symbol};
use rustc_trait_selection::infer::InferCtxtExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

#[path = "compiler/attempt.rs"]
mod attempt;

#[path = "compiler/completion.rs"]
mod completion;

#[path = "compiler/reuse.rs"]
mod reuse;

const MAX_BYTES: usize = 256 * 1024 * 1024;
const MAX_PACKAGES: usize = 4_096;
const MAX_DEFINITIONS: usize = 500_000;
const MAX_EDGES: usize = 1_000_000;
const MAX_STRING: usize = 1024 * 1024;
const MAX_DEPTH: usize = 256;
const DIAGNOSTIC_FILE: &str = "rawr-probe-recursion.journal";
const DIAGNOSTIC_SLOT_BYTES: usize = 512;
const DIAGNOSTIC_SLOTS: u64 = 512;
const MAX_DIAGNOSTIC_EVENTS: u64 = 4_000_000;
const MAX_DIAGNOSTIC_IDENTITIES: usize = MAX_DEFINITIONS;
const MAX_DIAGNOSTIC_INSTANTIATIONS: usize = 2_500_000;
const DIAGNOSTIC_IDENTITY_BYTES: usize = 224;
// Field-form and public-completeness proof entered at schema 8; exact sealed
// inner-rustc stack authority revisions enter at schemas 9 and 10; bounded
// evidence-only recursion diagnostics enter at schema 13; the separated
// canonicalization byte authority enters at schema 14; compiler-authenticated
// anonymous import support evidence enters at schema 15; distinct support-only
// type-constant proof enters at schema 16; closed scalar predicate completion
// and applicability evidence enters at schema 17. Schema 18 additionally requires
// the invocation-bound completion record after successful compiler return.
// Schema 19 adds the separate complete query-candidate observation population.
// Schema 20 adds the selected crate's name index for demanded units. Schema 21
// keeps a Future impl's `Output`; schema 22 keeps an Iterator impl's `Item` and
// `next`; schema 23 keeps an AsRef impl's `as_ref`. Schema 27 gives a
// constant of no scalar value as its own.
const PROTOCOL_SCHEMA: u32 = 27;

pub(super) struct Invocation {
    pub(super) package: Package,
    pub(super) root: Root,
    /// Paths, root alias first, that Rawr names in this unit. Empty seeds
    /// extraction from every public path; otherwise only these seed it
    /// (demand also covers crate re-exports).
    pub(super) demand: Vec<Vec<String>>,
    pub(super) unit_key: String,
    pub(super) dependencies: Vec<String>,
    pub(super) target: String,
    pub(super) target_kind: String,
    pub(super) target_kinds: Vec<String>,
    pub(super) crate_types: Vec<String>,
    pub(super) crate_name: String,
    pub(super) edition: String,
    pub(super) profile: String,
    pub(super) host: String,
    pub(super) selected_target: String,
    pub(super) features: Vec<String>,
    pub(super) invocation_nonce: String,
    pub(super) unit_correlation: String,
    pub(super) compiler_commit: String,
    pub(super) protocol_directory: PathBuf,
    pub(super) artifact: Artifact,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Artifact {
    pub(super) path: String,
    pub(super) length: u64,
    pub(super) digest: String,
    pub(super) profile: String,
    pub(super) fresh: bool,
    pub(super) probe_alias: String,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub(super) struct Package {
    pub(super) cargo_id: String,
    pub(super) name: String,
    pub(super) version: String,
    pub(super) source: String,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Root {
    Selected { aliases: Vec<String> },
}

struct Extractor {
    invocations: Option<Vec<Invocation>>,
    reuse: Option<PathBuf>,
    arguments_digest: String,
    budget: Option<Budget>,
    diagnostic_measurement: bool,
    diagnostic: Option<Diagnostic>,
    result: Option<Result<Budget, String>>,
}

struct Builder<'tcx, 'invocation, 'budget, 'diagnostic> {
    tcx: TyCtxt<'tcx>,
    invocation: &'invocation Invocation,
    selected: CrateNum,
    definitions: HashMap<(DefId, Namespace), Definition>,
    scheduler: Scheduler<SemanticKey, DefinitionRequest>,
    implementations: HashMap<DefId, Implementation>,
    reachable: Vec<Export>,
    foreign_roots: Vec<ForeignRoot>,
    sysroot: BTreeMap<CrateNum, Sysroot>,
    external: BTreeMap<CrateNum, ExternalCrate>,
    expanded_impls: HashSet<DefId>,
    expanded_surfaces: HashSet<DefId>,
    implementation_index: HashMap<CrateNum, HashMap<DefId, Vec<DefId>>>,
    implementation_requests: BTreeMap<[u8; 16], ImplementationRequest<'tcx>>,
    obligations: BTreeMap<TraitObligationOrderKey, ScheduledTraitObligation<'tcx>>,
    queued_obligations: usize,
    obligation_candidates: HashMap<TraitObligationKey<'tcx>, Vec<DefId>>,
    scalar_predicate_proofs: BTreeMap<ScalarPredicateKey, ScalarPredicateProof>,
    active_demand: Demand,
    /// `Type::member` paths Rawr demanded: a member the type may have
    /// through a trait it implements.
    member_demands: Vec<(DefId, String)>,
    /// `Trait::member` paths Rawr demanded, as `Widget::_boxed(..)` of a
    /// string: the member of each of the trait's impls for `&str`.
    trait_member_demands: Vec<(DefId, String)>,
    /// The roots of crates a macro's `$crate` demanded (`crate_segment`),
    /// named under their segment after extraction drains.
    crate_roots: Vec<(DefId, String)>,
    /// A blanket trait implementation applied to one concrete type, as if
    /// the crate wrote `impl Trait for Type`.
    applications: BTreeMap<String, Implementation>,
    /// The selected crate's public aliases with no parameters of a generic
    /// type at fixed arguments, found once (`instance_alias`).
    instance_aliases: Option<Vec<(DefId, Ty<'tcx>)>>,
    /// The selected crate's public aliases that pass their own parameters
    /// on to a type, by that type (`passing_alias`).
    passing_aliases: HashMap<DefId, DefId>,
    budget: &'budget mut Budget,
    diagnostic: &'diagnostic mut Option<Diagnostic>,
}

/// A re-exported foreign crate root (`pub use wgpu;`), named after the
/// selected crate's extraction drains and never extracted itself.
struct ForeignRoot {
    module: DefId,
    path: Vec<String>,
    reexports: Vec<Reexport>,
    depth: usize,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum Demand {
    ShapeOnly,
    SelectedRoot,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct SemanticKey {
    def_path_hash: [u8; 16],
    namespace: Namespace,
}

#[derive(Clone)]
struct DefinitionRequest {
    def_id: DefId,
    namespace: Namespace,
    owner: Owner,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct TraitObligation<'tcx> {
    trait_id: DefId,
    self_ty: Ty<'tcx>,
    arguments: ty::GenericArgsRef<'tcx>,
    polarity: ty::PredicatePolarity,
    associated: Option<DefId>,
    param_env: ty::ParamEnv<'tcx>,
    relevance: DefId,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct TraitObligationKey<'tcx> {
    trait_id: DefId,
    self_ty: Ty<'tcx>,
    arguments: ty::GenericArgsRef<'tcx>,
    polarity: ty::PredicatePolarity,
    associated: Option<DefId>,
    param_env: ty::ParamEnv<'tcx>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct TraitObligationOrderKey {
    trait_hash: [u8; 16],
    associated_hash: [u8; 16],
    substitution_hash: u64,
    relevance_hash: [u8; 16],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TraitObligationState {
    Queued,
    Complete,
}

struct ScheduledTraitObligation<'tcx> {
    obligation: TraitObligation<'tcx>,
    state: TraitObligationState,
    selected: bool,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct ScalarPredicateKey {
    owner_hash: [u8; 16],
    parameter: u32,
    trait_hash: [u8; 16],
    scalar: Scalar,
}

impl<'tcx> TraitObligation<'tcx> {
    fn key(self) -> TraitObligationKey<'tcx> {
        TraitObligationKey {
            trait_id: self.trait_id,
            self_ty: self.self_ty,
            arguments: self.arguments,
            polarity: self.polarity,
            associated: self.associated,
            param_env: self.param_env,
        }
    }
}

struct ImplementationRequest<'tcx> {
    impl_id: DefId,
    relevance: Vec<DefId>,
    associated: BTreeMap<AssociatedObligationOrderKey, AssociatedObligation<'tcx>>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
enum AssociatedDemandKind {
    SelectedMember,
    NominalSurface,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct AssociatedObligation<'tcx> {
    kind: AssociatedDemandKind,
    trait_item: Option<DefId>,
    arguments: Option<ty::GenericArgsRef<'tcx>>,
    param_env: ty::ParamEnv<'tcx>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct AssociatedObligationOrderKey {
    kind: AssociatedDemandKind,
    trait_item_hash: [u8; 16],
    substitution_hash: u64,
    param_env_hash: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AssociatedPathKind {
    Named,
    AnonymousSupport(AnonymousAssociatedSupport),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct AnonymousAssociatedSupport {
    parent: DefId,
    method: Symbol,
    disambiguator: u32,
}

fn take_owned_candidate_bucket<K, V>(index: &mut HashMap<K, Vec<V>>, key: &K) -> Option<Vec<V>>
where
    K: Eq + std::hash::Hash,
{
    index.remove(key)
}

fn effective_associated_item<I: Copy>(
    trait_item: I,
    implementor: Option<I>,
    has_default: bool,
) -> Result<I, String> {
    match implementor {
        Some(implementor) => Ok(implementor),
        None if has_default => Ok(trait_item),
        None => Err("selected required trait item has no implementor or default".to_owned()),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ScheduleState {
    Queued,
    Active,
    Complete { demand: Demand },
}

struct Scheduled<V> {
    value: V,
    demand: Demand,
    state: ScheduleState,
}

struct Scheduler<K, V> {
    entries: BTreeMap<K, Scheduled<V>>,
    ready: BTreeSet<K>,
}

impl<K, V> Scheduler<K, V>
where
    K: Clone + Ord,
    V: Clone,
{
    fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
            ready: BTreeSet::new(),
        }
    }

    fn request(
        &mut self,
        key: K,
        value: V,
        demand: Demand,
        budget: &mut Budget,
    ) -> Result<(), String> {
        match self.entries.get_mut(&key) {
            Some(entry) => {
                entry.demand = entry.demand.max(demand);
                if let ScheduleState::Complete { demand: completed } = entry.state
                    && entry.demand > completed
                {
                    budget.items(1)?;
                    entry.state = ScheduleState::Queued;
                    self.ready.insert(key);
                }
            }
            None => {
                // Charge both scheduler-owned nodes before either allocation.
                budget.items(2)?;
                self.entries.insert(
                    key.clone(),
                    Scheduled {
                        value,
                        demand,
                        state: ScheduleState::Queued,
                    },
                );
                self.ready.insert(key);
            }
        }
        Ok(())
    }

    fn next(&mut self) -> Result<Option<(K, V, Demand)>, String> {
        let Some(key) = self.ready.pop_first() else {
            return Ok(None);
        };
        let entry = self
            .entries
            .get_mut(&key)
            .ok_or_else(|| "traversal scheduler lost a queued identity".to_owned())?;
        if entry.state != ScheduleState::Queued {
            return Err("traversal scheduler selected a non-queued identity".to_owned());
        }
        entry.state = ScheduleState::Active;
        Ok(Some((key, entry.value.clone(), entry.demand)))
    }

    fn complete(&mut self, key: K, demand: Demand, budget: &mut Budget) -> Result<(), String> {
        let entry = self
            .entries
            .get_mut(&key)
            .ok_or_else(|| "traversal scheduler lost an active identity".to_owned())?;
        if entry.state != ScheduleState::Active {
            return Err("traversal scheduler completed a non-active identity".to_owned());
        }
        if entry.demand > demand {
            budget.items(1)?;
            entry.state = ScheduleState::Queued;
            self.ready.insert(key);
        } else {
            entry.state = ScheduleState::Complete { demand };
        }
        Ok(())
    }
}

pub(super) struct Budget {
    bytes: usize,
    items: usize,
    edges: usize,
    byte_limit: usize,
    edge_limit: usize,
}

struct Diagnostic {
    file: fs::File,
    sequence: u64,
    depth: u64,
    max_depth: u64,
    identities: HashSet<DiagnosticIdentity>,
    instantiations: HashSet<DiagnosticInstantiation>,
    buffer: Box<[u8]>,
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
struct DiagnosticIdentity {
    hash: [u8; 16],
    namespace: Namespace,
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
struct DiagnosticInstantiation {
    identity: DiagnosticIdentity,
    substitution: u64,
}

/// A sidecar is `{"handshake":…,` then this body's fields; the body is what
/// per-crate extraction reuse keeps for an unchanged crate.
#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct Body {
    reachable: Vec<Export>,
    definitions: Vec<Definition>,
    implementations: Vec<Implementation>,
    scalar_predicate_proofs: Vec<ScalarPredicateProof>,
    index: Vec<IndexName>,
}

/// One edge of a selected crate's name index: `name` is a public child
/// of module `parent`, of compiler `kind`, defining `id` (its def-path hash).
/// The crate root is the edge with an empty parent and name.
#[derive(Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
struct IndexName {
    parent: String,
    name: String,
    kind: &'static str,
    id: String,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
struct ScalarPredicateProof {
    owner: Id,
    parameter: u32,
    trait_id: Id,
    scalar: Scalar,
    outcome: ScalarPredicateOutcome,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
enum Scalar {
    Unit,
    Bool,
    Char,
    F64,
    I64,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum ScalarPredicateOutcome {
    Absent,
    Positive { implementation: Id },
    Unresolved,
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct Handshake {
    schema: u32,
    package: Package,
    root: Root,
    target: String,
    target_kind: String,
    target_kinds: Vec<String>,
    crate_types: Vec<String>,
    crate_name: String,
    edition: String,
    profile: String,
    compilation: TargetCompilation,
    host: String,
    selected_target: String,
    features: Vec<String>,
    probe_arguments_digest: String,
    compiler_commit: String,
    invocation_nonce: String,
    unit_correlation: String,
    unit_key: String,
    dependencies: Vec<String>,
    artifact: ArtifactHandshake,
    sysroot: Vec<Sysroot>,
    loaded_crates: Vec<ExternalCrate>,
    external_crates: Vec<ExternalCrate>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
struct ArtifactHandshake {
    path: String,
    length: u64,
    digest: String,
    profile: String,
    fresh: bool,
    crate_hash: String,
    stable_crate_id: String,
    probe_alias: String,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
enum TargetCompilation {
    Target,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
struct Sysroot {
    compiler_commit: String,
    selected_target: String,
    crate_name: String,
    stable_crate_id: String,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
struct ExternalCrate {
    crate_name: String,
    stable_crate_id: String,
    artifacts: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ImplementationCrateAuthority {
    Sysroot,
    External,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
struct Id {
    authority: Authority,
    def_path_hash: String,
    namespace: Namespace,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Authority {
    Compiler {
        crate_name: String,
        stable_crate_id: String,
    },
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
enum Namespace {
    Type,
    Value,
    Macro,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
struct Export {
    binding: Binding,
    root_alias: String,
    path: Vec<String>,
    namespace: Namespace,
    target: ExportTarget,
    aliases: Vec<Alias>,
    reexports: Vec<Reexport>,
    span: Span,
    /// A re-exported enum variant, as `pub use Alignment::Center;`: the
    /// variant's name, and `target` is its enum.
    #[serde(skip_serializing_if = "Option::is_none")]
    variant: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
enum Binding {
    Nameable,
    AnonymousImport,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum ExportTarget {
    Definition { id: Id },
    Unsupported { compiler_kind: String },
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
struct Reexport {
    kind: ReexportKind,
    evidence: ReexportEvidence,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum ReexportEvidence {
    Identified { item: ReexportId, span: Span },
    IdentitylessMacro,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
enum ReexportKind {
    Single,
    Glob,
    ExternCrate,
    MacroUse,
    MacroExport,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
struct ReexportId {
    crate_name: String,
    stable_crate_id: String,
    def_path_hash: String,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
struct Alias {
    from: Vec<String>,
    to: Vec<String>,
    span: Span,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
struct Definition {
    id: Id,
    display_path: Vec<String>,
    owner: Owner,
    visibility: Visibility,
    shape: Shape,
    span: Span,
    origin: Origin,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Owner {
    Free,
    Definition { id: Id },
    Implementation { id: Id },
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Visibility {
    Public,
    Restricted { scope: Id },
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
// Each item's shape is built once and serialized; boxing a variant would allocate per item.
#[allow(clippy::large_enum_variant)]
enum Shape {
    UnnameablePrivate,
    Module,
    Struct {
        field_form: FieldForm,
        fields: Vec<Field>,
        fields_complete: bool,
        non_exhaustive: bool,
        generics: Generics,
    },
    Union {
        field_form: FieldForm,
        fields: Vec<Field>,
        fields_complete: bool,
        non_exhaustive: bool,
        generics: Generics,
    },
    Enum {
        variants: Vec<Variant>,
        generics: Generics,
    },
    Const {
        ty: Type,
        value: Explicit<Constant>,
    },
    TypeConst {
        ty: Type,
        rhs: Explicit<Constant>,
    },
    Static {
        ty: Type,
        mutable: bool,
        thread_local: bool,
    },
    TypeAlias {
        ty: Type,
        generics: Generics,
    },
    Function {
        signature: Signature,
        generics: Generics,
    },
    Trait {
        unsafe_trait: bool,
        auto_trait: bool,
        supertraits: Vec<Predicate>,
        associated: Vec<Id>,
        generics: Generics,
    },
    AssociatedType {
        bounds: Vec<Predicate>,
        default: Explicit<Type>,
        generics: Generics,
    },
    Sysroot {
        compiler_kind: String,
    },
    /// A `macro_rules!` definition, as its crate's source writes it, and the
    /// path segment naming its crate's root for `$crate`.
    MacroRules {
        source: String,
        krate: String,
    },
    Unknown {
        compiler_kind: String,
    },
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
struct Field {
    name: String,
    index: u32,
    visibility: Visibility,
    ty: Type,
    span: Span,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
enum FieldForm {
    Record,
    Tuple,
    Unit,
}

struct Fields {
    form: FieldForm,
    public: Vec<Field>,
    complete: bool,
    non_exhaustive: bool,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
struct Variant {
    name: String,
    field_form: FieldForm,
    fields: Vec<Field>,
    fields_complete: bool,
    non_exhaustive: bool,
    discriminant: Explicit<Constant>,
    span: Span,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Explicit<T> {
    Absent,
    Present { value: T },
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
struct Signature {
    safety: Safety,
    const_function: bool,
    async_function: bool,
    abi: String,
    variadic: bool,
    binders: Vec<Binder>,
    receiver: Receiver,
    inputs: Vec<Type>,
    output: Type,
    #[serde(skip_serializing_if = "Option::is_none")]
    guard: Option<Guard>,
}

/// A method's output that borrows what it reaches through `Deref`, as
/// `RefCell::borrow_mut` and Freya's `State::write` give: rustc's
/// normalized `<Output as Deref>::Target`, and whether the output is also
/// `DerefMut`.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
struct Guard {
    target: Type,
    mutable: bool,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
enum Safety {
    Safe,
    Unsafe,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Receiver {
    None,
    Value {
        ty: Type,
    },
    Reference {
        lifetime: Lifetime,
        mutability: Mutability,
        ty: Type,
    },
    Explicit {
        ty: Type,
    },
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
enum Mutability {
    Shared,
    Mutable,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
struct Generics {
    parameters: Vec<Parameter>,
    predicates: Vec<Predicate>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
struct Parameter {
    owner: Id,
    index: u32,
    name: String,
    parameter_kind: ParameterKind,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum ParameterKind {
    Lifetime {
        bounds: Vec<Bound>,
    },
    Type {
        bounds: Vec<Bound>,
        default: Explicit<Type>,
    },
    Const {
        ty: Type,
        default: Explicit<Constant>,
    },
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Predicate {
    Bound { ty: Type, bound: Bound },
    ProjectionEquality { projection: Type, ty: Type },
    LifetimeOutlives { longer: Lifetime, shorter: Lifetime },
    TypeOutlives { ty: Type, lifetime: Lifetime },
    ConstEvaluatable { value: Constant },
    Unknown { compiler_kind: String },
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Bound {
    Trait {
        trait_id: Id,
        arguments: Vec<Argument>,
        polarity: Polarity,
        binders: Vec<Binder>,
    },
    MaybeSized,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
struct Binder {
    owner: Id,
    depth: u32,
    index: u32,
    name: String,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Lifetime {
    Static,
    Early { owner: Id, index: u32 },
    Late { binder: Binder, index: u32 },
    Erased,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Type {
    Primitive {
        name: String,
    },
    Definition {
        id: Id,
        arguments: Vec<Argument>,
    },
    Parameter {
        owner: Id,
        index: u32,
    },
    #[serde(rename = "self_type")]
    SelfTy {
        owner: Id,
    },
    Tuple {
        elements: Vec<Type>,
    },
    Slice {
        element: Box<Type>,
    },
    Array {
        element: Box<Type>,
        length: Constant,
    },
    Reference {
        lifetime: Lifetime,
        mutability: Mutability,
        ty: Box<Type>,
    },
    RawPointer {
        mutability: Mutability,
        ty: Box<Type>,
    },
    FunctionPointer {
        signature: Box<Signature>,
    },
    TraitObject {
        bounds: Vec<ObjectBound>,
        lifetime: Lifetime,
    },
    ImplTrait {
        id: Id,
        bounds: Vec<Predicate>,
        captures: Vec<Argument>,
    },
    Projection {
        trait_id: Id,
        associated: Id,
        arguments: Vec<Argument>,
    },
    Never,
    Unknown {
        compiler_kind: String,
    },
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum ObjectBound {
    Trait {
        bound: Bound,
    },
    ProjectionEquality {
        associated: Id,
        arguments: Vec<Argument>,
        ty: Type,
    },
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Argument {
    Lifetime { lifetime: Lifetime },
    Type { ty: Type },
    Const { value: Constant },
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Constant {
    Literal {
        ty: String,
        value: String,
    },
    Parameter {
        owner: Id,
        index: u32,
    },
    Unevaluated {
        definition: Id,
        arguments: Vec<Argument>,
    },
    Unknown {
        compiler_kind: String,
    },
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
struct Implementation {
    key: String,
    id: Id,
    relevance: Vec<Id>,
    trait_ref: Trait,
    self_type: Type,
    safety: Safety,
    polarity: Polarity,
    const_impl: bool,
    default_impl: bool,
    generics: Generics,
    associated: Vec<Id>,
    span: Span,
    origin: Origin,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Trait {
    Inherent,
    Trait { id: Id, arguments: Vec<Argument> },
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
enum Polarity {
    Positive,
    Negative,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
struct Span {
    file: String,
    byte_start: u32,
    byte_end: u32,
    line_start: u32,
    column_start: u32,
    line_end: u32,
    column_end: u32,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Origin {
    Source,
    Expansion {
        call_site: Span,
        definition_site: Span,
    },
    Synthesized {
        mechanism: String,
    },
}

pub(super) fn run(
    arguments: Vec<String>,
    invocations: Vec<Invocation>,
    mut budget: Budget,
    diagnose_recursion: bool,
    reuse: Option<PathBuf>,
) -> Result<ExitCode, String> {
    let arguments_digest = arguments_digest(&arguments, &mut budget)?;
    let completion = completion::Prepared::bind(&invocations, &arguments_digest, &mut budget)?;
    let collector = attempt::Collector::install(&mut budget)?;
    let mut diagnostic = diagnose_recursion
        .then(|| Diagnostic::create(Path::new(DIAGNOSTIC_FILE)))
        .transpose()?;
    if let Some(diagnostic) = &mut diagnostic {
        diagnostic.milestone("compiler", "run-compiler-enter", "global")?;
    }
    let mut callbacks = Extractor {
        invocations: Some(invocations),
        reuse,
        arguments_digest,
        budget: Some(budget),
        diagnostic_measurement: diagnose_recursion,
        diagnostic,
        result: None,
    };
    let exit = rustc_driver::catch_with_exit_code(|| {
        rustc_driver::run_compiler(&arguments, &mut callbacks);
    });
    match callbacks.result {
        Some(Ok(mut budget)) if exit == ExitCode::SUCCESS => {
            completion.publish(collector.complete()?, &mut budget)?;
            if diagnose_recursion {
                println!("rawr-compiler-items-v1\t{}", budget.items);
            }
            Ok(exit)
        }
        Some(Err(error)) => Err(error),
        Some(Ok(_)) => Ok(exit),
        None if exit == ExitCode::SUCCESS => {
            Err("metadata probe completed without analysis callback".to_owned())
        }
        None => Ok(exit),
    }
}

impl Callbacks for Extractor {
    fn after_analysis<'tcx>(
        &mut self,
        _compiler: &rustc_interface::interface::Compiler,
        tcx: TyCtxt<'tcx>,
    ) -> Compilation {
        let Some(invocations) = self.invocations.take() else {
            self.result = Some(Err("analysis callback ran more than once".to_owned()));
            return Compilation::Stop;
        };
        let Some(budget) = self.budget.take() else {
            self.result = Some(Err("analysis callback lost its allocation proof".to_owned()));
            return Compilation::Stop;
        };
        if let Some(diagnostic) = &mut self.diagnostic
            && let Err(error) = diagnostic.milestone("compiler", "after-analysis-enter", "global")
        {
            self.result = Some(Err(error));
            return Compilation::Stop;
        }
        self.result = Some(extract_all(
            tcx,
            invocations,
            self.reuse.as_deref(),
            &self.arguments_digest,
            budget,
            self.diagnostic_measurement,
            &mut self.diagnostic,
        ));
        Compilation::Stop
    }
}

fn extract_all(
    tcx: TyCtxt<'_>,
    invocations: Vec<Invocation>,
    reuse: Option<&Path>,
    arguments_digest: &str,
    mut budget: Budget,
    diagnostic_measurement: bool,
    diagnostic: &mut Option<Diagnostic>,
) -> Result<Budget, String> {
    // The diagnostic traversal owns its wider evidence envelope. Protocol
    // rendering and publication retain a fresh production budget, while the
    // ordinary probe keeps its original aggregate production budget.
    let mut publication_budget = diagnostic_measurement.then(Budget::standard);
    // A diagnostic traversal always extracts.
    let mut cache = match reuse {
        Some(directory) if !diagnostic_measurement => Some(reuse::Cache::open(directory)?),
        _ => None,
    };
    for invocation in invocations {
        let selected = selected_crate(tcx, &invocation.artifact.path, &mut budget)?;
        if let Some(diagnostic) = diagnostic {
            diagnostic.milestone(
                "compiler",
                "selected-crate-resolved",
                invocation.crate_name.as_str(),
            )?;
        }
        if tcx.crate_name(selected).as_str() != invocation.crate_name {
            return Err("selected rmeta crate name disagrees with Cargo authority".to_owned());
        }
        let loaded_crates = loaded_external_crates(tcx, &mut budget)?;
        if let Some(diagnostic) = diagnostic.as_mut() {
            diagnostic.milestone(
                "compiler",
                "loaded-crates-complete",
                invocation.crate_name.as_str(),
            )?;
        }
        let crate_hash = budget.formatted(format_args!("{:?}", tcx.crate_hash(selected)))?;
        let stable_crate_id =
            budget.formatted(format_args!("{:016x}", tcx.stable_crate_id(selected)))?;
        let key = cache
            .as_ref()
            .map(|cache| cache.key(&invocation, &crate_hash, &stable_crate_id));
        let reused = match (&mut cache, &key) {
            (Some(cache), Some(key)) => cache.load(key, &loaded_crates),
            _ => None,
        };
        let reuse::Reused {
            sysroot,
            external,
            body,
        } = match reused {
            Some(reused) => reused,
            None => {
                let extracted = extract(tcx, &invocation, selected, &mut budget, diagnostic)?;
                if let (Some(cache), Some(key)) = (&cache, &key) {
                    cache.store(
                        key,
                        &extracted.sysroot,
                        &extracted.external,
                        &extracted.body,
                    );
                }
                extracted
            }
        };
        let (protocol_directory, handshake) = handshake(
            invocation,
            arguments_digest,
            crate_hash,
            stable_crate_id,
            sysroot,
            loaded_crates,
            external,
            &mut budget,
        )?;
        publish(
            &protocol_directory,
            &handshake,
            &body,
            publication_budget.as_mut().unwrap_or(&mut budget),
        )?;
    }
    if let Some(cache) = &cache {
        cache.prune();
    }
    Ok(budget)
}

fn selected_crate(
    tcx: TyCtxt<'_>,
    artifact: &str,
    budget: &mut Budget,
) -> Result<CrateNum, String> {
    let artifact = canonical_path(Path::new(artifact), "selected rmeta", budget)?;
    let mut found = None;
    for crate_num in tcx.crates(()) {
        let source = tcx.used_crate_source(*crate_num);
        let mut matched = false;
        for path in source.paths() {
            if canonical_path(path, "compiler crate source", budget)? == artifact {
                matched = true;
                break;
            }
        }
        if matched && found.replace(*crate_num).is_some() {
            return Err("selected rmeta resolved to multiple compiler crates".to_owned());
        }
    }
    found.ok_or_else(|| "selected rmeta was not force-loaded by the metadata probe".to_owned())
}

/// `adt` at the instance its path names as a type, as `Rgb` is `Rgb<Srgb,
/// f32>`: each type parameter at its default and each lifetime at
/// `'static`. `None` when a type or const parameter has no default.
fn defaulted_instance<'tcx>(tcx: TyCtxt<'tcx>, adt: DefId) -> Option<Ty<'tcx>> {
    let mut complete = true;
    let arguments =
        ty::GenericArgs::for_item(tcx, adt, |parameter, previous| match parameter.kind {
            ty::GenericParamDefKind::Lifetime => tcx.lifetimes.re_static.into(),
            ty::GenericParamDefKind::Type {
                has_default: true, ..
            } => tcx
                .type_of(parameter.def_id)
                .instantiate(tcx, previous)
                .skip_norm_wip()
                .into(),
            _ => {
                complete = false;
                tcx.types.unit.into()
            }
        });
    complete.then(|| tcx.type_of(adt).instantiate(tcx, arguments).skip_norm_wip())
}

fn extract<'tcx>(
    tcx: TyCtxt<'tcx>,
    invocation: &Invocation,
    selected: CrateNum,
    budget: &mut Budget,
    diagnostic: &mut Option<Diagnostic>,
) -> Result<reuse::Reused, String> {
    // A demanded unit also publishes its complete name index, so Rawr
    // can tell a typo from an unextracted item without another probe.
    let index = if invocation.demand.is_empty() {
        Vec::new()
    } else {
        name_index(tcx, selected.as_def_id(), budget)?
    };
    let mut builder = Builder {
        tcx,
        invocation,
        selected,
        definitions: HashMap::new(),
        scheduler: Scheduler::new(),
        implementations: HashMap::new(),
        reachable: Vec::new(),
        foreign_roots: Vec::new(),
        sysroot: BTreeMap::new(),
        external: BTreeMap::new(),
        expanded_impls: HashSet::new(),
        expanded_surfaces: HashSet::new(),
        implementation_index: HashMap::new(),
        implementation_requests: BTreeMap::new(),
        obligations: BTreeMap::new(),
        queued_obligations: 0,
        obligation_candidates: HashMap::new(),
        scalar_predicate_proofs: BTreeMap::new(),
        active_demand: Demand::ShapeOnly,
        member_demands: Vec::new(),
        trait_member_demands: Vec::new(),
        crate_roots: Vec::new(),
        applications: BTreeMap::new(),
        instance_aliases: None,
        passing_aliases: HashMap::new(),
        budget,
        diagnostic,
    };
    if invocation.demand.is_empty() {
        // Whole-crate seeding: every public path of the selected crate.
        builder.collect_external_exports(selected.as_def_id(), Vec::new(), &[], 0, false)?;
        builder.drain_scheduler()?;
        for root in std::mem::take(&mut builder.foreign_roots) {
            builder.collect_external_exports(
                root.module,
                root.path,
                &root.reexports,
                root.depth,
                true,
            )?;
        }
    } else {
        // Seed from the paths Rawr names instead. Each lands on
        // a selected root whose closure (signatures, public fields, inherent
        // and trait impl tables) the scheduler drains as for a whole crate;
        // the names-only walk then names exactly what was extracted, through
        // every public spelling, wholesale crate re-exports included.
        for path in &invocation.demand {
            builder.select_path(selected.as_def_id(), &path[1..], 0)?;
        }
        builder.drain_scheduler()?;
        builder.resolve_member_demands()?;
        builder.resolve_trait_member_demands()?;
        builder.drain_scheduler()?;
        builder.collect_external_exports(selected.as_def_id(), Vec::new(), &[], 0, true)?;
        // A macro's `$crate` paths, under the segment naming its crate.
        for (root, segment) in std::mem::take(&mut builder.crate_roots) {
            builder.collect_external_exports(root, vec![segment], &[], 1, true)?;
        }
    }
    let mut definitions = Vec::new();
    builder.budget.items(builder.definitions.len())?;
    definitions
        .try_reserve_exact(builder.definitions.len())
        .map_err(|error| format!("definition ledger allocation failed: {error}"))?;
    definitions.extend(builder.definitions.into_values());
    let mut implementations = Vec::new();
    builder.budget.items(builder.implementations.len())?;
    implementations
        .try_reserve_exact(builder.implementations.len())
        .map_err(|error| format!("implementation ledger allocation failed: {error}"))?;
    implementations.extend(builder.implementations.into_values());
    builder.budget.items(builder.applications.len())?;
    implementations
        .try_reserve(builder.applications.len())
        .map_err(|error| format!("implementation ledger allocation failed: {error}"))?;
    implementations.extend(builder.applications.into_values());
    let mut scalar_predicate_proofs = Vec::new();
    builder
        .budget
        .items(builder.scalar_predicate_proofs.len())?;
    scalar_predicate_proofs
        .try_reserve_exact(builder.scalar_predicate_proofs.len())
        .map_err(|error| format!("scalar predicate proof ledger allocation failed: {error}"))?;
    scalar_predicate_proofs.extend(builder.scalar_predicate_proofs.into_values());
    let mut reachable = builder.reachable;
    let mut sysroot = Vec::new();
    builder.budget.items(builder.sysroot.len())?;
    sysroot
        .try_reserve_exact(builder.sysroot.len())
        .map_err(|error| format!("sysroot ledger allocation failed: {error}"))?;
    sysroot.extend(builder.sysroot.into_values());
    let mut external_crates = Vec::new();
    builder.budget.items(builder.external.len())?;
    external_crates
        .try_reserve_exact(builder.external.len())
        .map_err(|error| format!("external crate ledger allocation failed: {error}"))?;
    external_crates.extend(builder.external.into_values());
    definitions.sort_unstable();
    implementations.sort_unstable();
    scalar_predicate_proofs.sort_unstable();
    reachable.sort_unstable();
    sysroot.sort_unstable();
    external_crates.sort_unstable();
    let body = serde_json::to_vec(&Body {
        reachable,
        definitions,
        implementations,
        scalar_predicate_proofs,
        index,
    })
    .map_err(|error| format!("failed to encode compiler sidecar: {error}"))?;
    Ok(reuse::Reused {
        sysroot,
        external: external_crates,
        body,
    })
}

#[allow(
    clippy::too_many_arguments,
    reason = "the handshake joins the invocation, the session and the extraction"
)]
fn handshake(
    invocation: Invocation,
    arguments_digest: &str,
    crate_hash: String,
    stable_crate_id: String,
    sysroot: Vec<Sysroot>,
    loaded_crates: Vec<ExternalCrate>,
    external_crates: Vec<ExternalCrate>,
    budget: &mut Budget,
) -> Result<(PathBuf, Handshake), String> {
    let Invocation {
        package,
        root,
        demand: _,
        unit_key,
        dependencies,
        target,
        target_kind,
        target_kinds,
        crate_types,
        crate_name,
        edition,
        profile,
        host,
        selected_target,
        features,
        invocation_nonce,
        unit_correlation,
        compiler_commit,
        protocol_directory,
        artifact,
    } = invocation;
    let probe_arguments_digest = budget.owned(arguments_digest)?;
    Ok((
        protocol_directory,
        Handshake {
            schema: PROTOCOL_SCHEMA,
            package,
            root,
            target,
            target_kind,
            target_kinds,
            crate_types,
            crate_name,
            edition,
            profile,
            compilation: TargetCompilation::Target,
            host,
            selected_target,
            features,
            probe_arguments_digest,
            compiler_commit,
            invocation_nonce,
            unit_correlation,
            unit_key,
            dependencies,
            artifact: ArtifactHandshake {
                path: artifact.path,
                length: artifact.length,
                digest: artifact.digest,
                profile: artifact.profile,
                fresh: artifact.fresh,
                crate_hash,
                stable_crate_id,
                probe_alias: artifact.probe_alias,
            },
            sysroot,
            loaded_crates,
            external_crates,
        },
    ))
}

/// Every public path of the selected crate, names only, as module
/// edges. Each module is walked once, so a module public under two paths is
/// one alias edge (the same id); a re-exported foreign crate root
/// (`pub use other_crate;`) is an edge of kind `crate`, never walked.
fn name_index(tcx: TyCtxt<'_>, root: DefId, budget: &mut Budget) -> Result<Vec<IndexName>, String> {
    let mut index = vec![IndexName {
        parent: String::new(),
        name: String::new(),
        kind: "module",
        id: def_hash(tcx, root)?,
    }];
    let mut walked = HashSet::from([root]);
    let mut pending = vec![root];
    while let Some(module) = pending.pop() {
        let parent = def_hash(tcx, module)?;
        for child in tcx.module_children(module) {
            let Res::Def(kind, def_id) = child.res else {
                continue;
            };
            if !child.vis.is_public() || child.ident.name == kw::Underscore {
                continue;
            }
            let foreign = def_id.is_crate_root() && def_id.krate != root.krate;
            budget.items(1)?;
            budget.bytes(parent.len().saturating_add(32))?;
            index.push(IndexName {
                parent: parent.clone(),
                name: budget.owned(child.ident.name.as_str())?,
                kind: if foreign {
                    "crate"
                } else {
                    def_kind_text(kind)
                },
                id: def_hash(tcx, def_id)?,
            });
            if kind == DefKind::Mod && !foreign && walked.insert(def_id) {
                pending.push(def_id);
            }
        }
    }
    index.sort_unstable();
    Ok(index)
}

impl Diagnostic {
    fn create(path: &Path) -> Result<Self, String> {
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|error| format!("failed to create recursion diagnostic journal: {error}"))?;
        file.set_len(
            u64::try_from(DIAGNOSTIC_SLOT_BYTES)
                .unwrap_or(u64::MAX)
                .saturating_mul(DIAGNOSTIC_SLOTS),
        )
        .map_err(|error| format!("failed to size recursion diagnostic journal: {error}"))?;
        let mut buffer = Vec::new();
        buffer
            .try_reserve_exact(DIAGNOSTIC_SLOT_BYTES)
            .map_err(|error| format!("failed to allocate recursion diagnostic slot: {error}"))?;
        buffer.resize(DIAGNOSTIC_SLOT_BYTES, 0);
        Ok(Self {
            file,
            sequence: 0,
            depth: 0,
            max_depth: 0,
            identities: HashSet::new(),
            instantiations: HashSet::new(),
            buffer: buffer.into_boxed_slice(),
        })
    }

    fn milestone(
        &mut self,
        stage: &'static str,
        operation: &'static str,
        identity: &str,
    ) -> Result<(), String> {
        self.write_record(
            "milestone",
            stage,
            operation,
            "global",
            identity,
            None,
            None,
            "none",
            false,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn breadcrumb(
        &mut self,
        state: &'static str,
        stage: &'static str,
        operation: &'static str,
        namespace: &'static str,
        identity: &str,
        semantic: DiagnosticIdentity,
        substitution: Option<u64>,
        deduplication: &'static str,
        substitutions_change_key: bool,
    ) -> Result<(), String> {
        if state == "begin" {
            self.depth = self
                .depth
                .checked_add(1)
                .ok_or_else(|| "recursion diagnostic traversal depth overflowed".to_owned())?;
            self.max_depth = self.max_depth.max(self.depth);
        } else if state != "complete" {
            return Err("recursion diagnostic breadcrumb state is invalid".to_owned());
        } else if self.depth == 0 {
            return Err("recursion diagnostic traversal depth underflowed".to_owned());
        }

        if !self.identities.contains(&semantic) {
            if self.identities.len() >= MAX_DIAGNOSTIC_IDENTITIES {
                return Err("recursion diagnostic semantic identity limit exceeded".to_owned());
            }
            self.identities.try_reserve(1).map_err(|error| {
                format!("failed to reserve recursion diagnostic semantic identity: {error}")
            })?;
            self.identities.insert(semantic);
        }
        if let Some(substitution) = substitution {
            let instantiation = DiagnosticInstantiation {
                identity: semantic,
                substitution,
            };
            if !self.instantiations.contains(&instantiation) {
                if self.instantiations.len() >= MAX_DIAGNOSTIC_INSTANTIATIONS {
                    return Err("recursion diagnostic instantiation limit exceeded".to_owned());
                }
                self.instantiations.try_reserve(1).map_err(|error| {
                    format!("failed to reserve recursion diagnostic instantiation: {error}")
                })?;
                self.instantiations.insert(instantiation);
            }
        }

        let result = self.write_record(
            state,
            stage,
            operation,
            namespace,
            identity,
            Some(semantic),
            substitution,
            deduplication,
            substitutions_change_key,
        );
        if state == "complete" {
            self.depth = self.depth.saturating_sub(1);
        }
        result
    }

    #[allow(clippy::too_many_arguments)]
    fn write_record(
        &mut self,
        state: &'static str,
        stage: &'static str,
        operation: &'static str,
        namespace: &'static str,
        identity: &str,
        semantic: Option<DiagnosticIdentity>,
        substitution: Option<u64>,
        deduplication: &'static str,
        substitutions_change_key: bool,
    ) -> Result<(), String> {
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or_else(|| "recursion diagnostic sequence overflowed".to_owned())?;
        if self.sequence > MAX_DIAGNOSTIC_EVENTS {
            return Err("recursion diagnostic event limit exceeded".to_owned());
        }
        let mut identity_end = identity.len().min(DIAGNOSTIC_IDENTITY_BYTES);
        while !identity.is_char_boundary(identity_end) {
            identity_end = identity_end.saturating_sub(1);
        }
        let identity = &identity[..identity_end];
        self.buffer.fill(0);
        {
            let mut cursor = Cursor::new(self.buffer.as_mut());
            let substitution_present = substitution.is_some();
            write!(
                cursor,
                concat!(
                    "rawr-recursion-v2\t{:020}\t{}\t{}\t{}",
                    "\t{}\t{}\tsemantic={}\tinstantiation={:016x}",
                    "\tinstantiated={}\tdedup={}",
                    "\tsubstitutions_change_key={}",
                    "\tdepth={}\tmax_depth={}\tunique_identities={}",
                    "\tunique_instantiations={}\n"
                ),
                self.sequence,
                state,
                stage,
                operation,
                namespace,
                identity,
                semantic.is_some(),
                substitution.unwrap_or_default(),
                substitution_present,
                deduplication,
                substitutions_change_key,
                self.depth,
                self.max_depth,
                self.identities.len(),
                self.instantiations.len(),
            )
            .map_err(|error| format!("failed to encode recursion diagnostic record: {error}"))?;
        }
        let slot = self.sequence.saturating_sub(1) % DIAGNOSTIC_SLOTS;
        let offset = slot.saturating_mul(u64::try_from(DIAGNOSTIC_SLOT_BYTES).unwrap_or(u64::MAX));
        self.file
            .seek(SeekFrom::Start(offset))
            .map_err(|error| format!("failed to seek recursion diagnostic journal: {error}"))?;
        self.file
            .write_all(&self.buffer)
            .map_err(|error| format!("failed to write recursion diagnostic record: {error}"))?;
        self.file
            .flush()
            .map_err(|error| format!("failed to flush recursion diagnostic record: {error}"))
    }
}

impl Budget {
    pub(super) fn standard() -> Self {
        Self {
            bytes: 0,
            items: 0,
            edges: 0,
            byte_limit: MAX_BYTES,
            edge_limit: MAX_EDGES,
        }
    }

    #[cfg(test)]
    pub(super) fn with_limits(bytes: usize, edges: usize) -> Self {
        Self {
            bytes: 0,
            items: 0,
            edges: 0,
            byte_limit: bytes,
            edge_limit: edges,
        }
    }

    #[cfg(test)]
    pub(super) const fn usage(&self) -> (usize, usize, usize) {
        (self.bytes, self.items, self.edges)
    }

    /// Counts owned items for the diagnostic probe's report. The count never
    /// refuses an extraction: limits never decide whether Rawr runs.
    pub(super) fn items(&mut self, amount: usize) -> Result<(), String> {
        self.items = self.items.saturating_add(amount);
        Ok(())
    }

    fn edge(&mut self, amount: usize) -> Result<(), String> {
        self.edges = self.edges.saturating_add(amount);
        if self.edges > self.edge_limit {
            Err("compiler API edge limit exceeded".to_owned())
        } else {
            Ok(())
        }
    }

    pub(super) fn string(&mut self, value: &str) -> Result<(), String> {
        checked_string(value)?;
        self.string_length(value.len())
    }

    pub(super) fn bytes(&mut self, amount: usize) -> Result<(), String> {
        self.bytes = self.bytes.saturating_add(amount);
        if self.bytes > self.byte_limit {
            Err("compiler aggregate owned byte limit exceeded".to_owned())
        } else {
            Ok(())
        }
    }

    pub(super) fn path_capacity(&mut self, maximum: usize) -> Result<(), String> {
        if maximum > MAX_STRING {
            return Err("compiler path exceeds the protocol limit".to_owned());
        }
        self.items(1)?;
        self.bytes(maximum)
    }

    pub(super) fn validate_path(&self, path: &Path, maximum: usize) -> Result<(), String> {
        if path.as_os_str().len() > maximum {
            Err("compiler path exceeded its prospective allocation proof".to_owned())
        } else {
            Ok(())
        }
    }

    pub(super) fn owned(&mut self, value: &str) -> Result<String, String> {
        self.string(value)?;
        let mut owned = String::new();
        owned
            .try_reserve_exact(value.len())
            .map_err(|error| format!("compiler string allocation failed: {error}"))?;
        owned.push_str(value);
        Ok(owned)
    }

    pub(super) fn formatted(&mut self, arguments: fmt::Arguments<'_>) -> Result<String, String> {
        struct Counter(usize);

        impl fmt::Write for Counter {
            fn write_str(&mut self, value: &str) -> fmt::Result {
                self.0 = self.0.checked_add(value.len()).ok_or(fmt::Error)?;
                Ok(())
            }
        }

        let retained = arguments;
        let mut counter = Counter(0);
        fmt::write(&mut counter, arguments)
            .map_err(|_| "compiler formatted string length overflowed".to_owned())?;
        self.string_length(counter.0)?;
        let mut value = String::new();
        value
            .try_reserve_exact(counter.0)
            .map_err(|error| format!("compiler string allocation failed: {error}"))?;
        fmt::write(&mut value, retained)
            .map_err(|_| "compiler formatted string rendering failed".to_owned())?;
        Ok(value)
    }

    fn string_length(&mut self, length: usize) -> Result<(), String> {
        if length == 0 || length > MAX_STRING {
            return Err("compiler string is empty or exceeds the protocol limit".to_owned());
        }
        self.items(1)?;
        self.bytes(length)
    }

    fn hexadecimal(&mut self, bytes: &[u8]) -> Result<String, String> {
        let length = bytes.len().saturating_mul(2);
        self.string_length(length)?;
        let mut encoded = String::new();
        encoded
            .try_reserve_exact(length)
            .map_err(|error| format!("compiler hexadecimal allocation failed: {error}"))?;
        for byte in bytes {
            use std::fmt::Write as _;
            let _ = write!(encoded, "{byte:02x}");
        }
        Ok(encoded)
    }
}

fn loaded_external_crates(
    tcx: TyCtxt<'_>,
    budget: &mut Budget,
) -> Result<Vec<ExternalCrate>, String> {
    let mut loaded = Vec::new();
    for crate_num in tcx.crates(()) {
        if loaded.len() >= MAX_PACKAGES {
            return Err("loaded compiler crate limit exceeded".to_owned());
        }
        let source = tcx.used_crate_source(*crate_num);
        let sysroot = tcx.sess.opts.sysroot.path();
        let mut artifacts = Vec::new();
        let mut path_count = 0_usize;
        let mut all_sysroot = true;
        for path in source.paths() {
            path_count = path_count.saturating_add(1);
            if path_count > MAX_EDGES {
                return Err("loaded compiler artifact edge limit exceeded".to_owned());
            }
            budget.edge(1)?;
            all_sysroot &= path.starts_with(sysroot);
            let path = canonical_path(path, "loaded artifact", budget)?;
            let text = path
                .to_str()
                .ok_or_else(|| "loaded artifact path is not UTF-8".to_owned())?;
            budget.items(1)?;
            artifacts
                .try_reserve(1)
                .map_err(|error| format!("loaded artifact allocation failed: {error}"))?;
            artifacts.push(budget.owned(text)?);
        }
        if path_count == 0 {
            return Err("loaded compiler crate has no artifact evidence".to_owned());
        }
        if all_sysroot {
            continue;
        }
        artifacts.sort_unstable();
        artifacts.dedup();
        let crate_name = tcx.crate_name(*crate_num);
        let stable_crate_id =
            budget.formatted(format_args!("{:016x}", tcx.stable_crate_id(*crate_num)))?;
        budget.items(1)?;
        loaded
            .try_reserve(1)
            .map_err(|error| format!("loaded crate allocation failed: {error}"))?;
        loaded.push(ExternalCrate {
            crate_name: budget.owned(crate_name.as_str())?,
            stable_crate_id,
            artifacts,
        });
    }
    loaded.sort_unstable();
    if loaded.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err("duplicate loaded compiler crate evidence".to_owned());
    }
    Ok(loaded)
}

impl<'tcx> Builder<'tcx, '_, '_, '_> {
    fn breadcrumb(
        &mut self,
        state: &'static str,
        stage: &'static str,
        operation: &'static str,
        def_id: DefId,
        namespace: Namespace,
        deduplication: &'static str,
    ) -> Result<(), String> {
        let Some(diagnostic) = self.diagnostic.as_mut() else {
            return Ok(());
        };
        let raw_hash = self.tcx.def_path_hash(def_id).to_raw_def_path_hash().0;
        let hash = hex(&raw_hash)?;
        let path = self.tcx.def_path_str(def_id);
        let mut identity = String::new();
        let retained_path = path
            .len()
            .min(DIAGNOSTIC_IDENTITY_BYTES.saturating_sub(hash.len() + 1));
        identity
            .try_reserve_exact(hash.len().saturating_add(1).saturating_add(retained_path))
            .map_err(|error| {
                format!("failed to allocate recursion diagnostic identity: {error}")
            })?;
        identity.push_str(&hash);
        identity.push(':');
        let mut path_end = retained_path;
        while !path.is_char_boundary(path_end) {
            path_end = path_end.saturating_sub(1);
        }
        identity.push_str(&path[..path_end]);
        diagnostic.breadcrumb(
            state,
            stage,
            operation,
            match namespace {
                Namespace::Type => "type",
                Namespace::Value => "value",
                Namespace::Macro => "macro",
            },
            &identity,
            DiagnosticIdentity {
                hash: raw_hash,
                namespace,
            },
            None,
            deduplication,
            false,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn instantiated_breadcrumb(
        &mut self,
        state: &'static str,
        stage: &'static str,
        operation: &'static str,
        def_id: DefId,
        namespace: Namespace,
        ty: Ty<'tcx>,
        deduplication: &'static str,
    ) -> Result<(), String> {
        let Some(diagnostic) = self.diagnostic.as_mut() else {
            return Ok(());
        };
        let raw_hash = self.tcx.def_path_hash(def_id).to_raw_def_path_hash().0;
        let hash = hex(&raw_hash)?;
        let path = self.tcx.def_path_str(def_id);
        let mut identity = String::new();
        let retained_path = path
            .len()
            .min(DIAGNOSTIC_IDENTITY_BYTES.saturating_sub(hash.len() + 1));
        identity
            .try_reserve_exact(hash.len().saturating_add(1).saturating_add(retained_path))
            .map_err(|error| {
                format!("failed to allocate recursion diagnostic identity: {error}")
            })?;
        identity.push_str(&hash);
        identity.push(':');
        let mut path_end = retained_path;
        while !path.is_char_boundary(path_end) {
            path_end = path_end.saturating_sub(1);
        }
        identity.push_str(&path[..path_end]);
        let mut instantiation = DefaultHasher::new();
        ty.hash(&mut instantiation);
        diagnostic.breadcrumb(
            state,
            stage,
            operation,
            match namespace {
                Namespace::Type => "type",
                Namespace::Value => "value",
                Namespace::Macro => "macro",
            },
            &identity,
            DiagnosticIdentity {
                hash: raw_hash,
                namespace,
            },
            Some(instantiation.finish()),
            deduplication,
            false,
        )
    }

    /// Walk one public module. With `names_only`, the walk extracts nothing: it
    /// names only definitions the selected crate's extraction already holds.
    fn collect_external_exports(
        &mut self,
        module: DefId,
        prefix: Vec<String>,
        inherited: &[Reexport],
        depth: usize,
        names_only: bool,
    ) -> Result<(), String> {
        if depth > MAX_DEPTH {
            return Err("external public module traversal depth exceeded".to_owned());
        }
        self.breadcrumb(
            "begin",
            "selected-exports",
            "module-exports",
            module,
            Namespace::Type,
            "definition",
        )?;
        for child in self.tcx.module_children(module) {
            if !child.vis.is_public() {
                continue;
            }
            // A re-exported variant's constructor, in the value namespace,
            // is the variant the type namespace re-exports.
            if matches!(child.res, Res::Def(DefKind::Ctor(CtorOf::Variant, _), _)) {
                continue;
            }
            // A re-exported variant, as `pub use Alignment::Center;`, names
            // its enum's variant: the export is the enum's, with the
            // variant's name.
            let (res, variant) = match child.res {
                Res::Def(DefKind::Variant, variant) => {
                    let owner = self.tcx.parent(variant);
                    (
                        Res::Def(DefKind::Enum, owner),
                        Some(self.budget.owned(self.tcx.item_name(variant).as_str())?),
                    )
                }
                res => (res, None),
            };
            if names_only {
                match res {
                    Res::Def(DefKind::Mod, _) => {}
                    Res::Def(kind, def_id)
                        if kind.ns().is_some_and(|ns| {
                            self.definitions.contains_key(&(def_id, namespace(ns)))
                        }) => {}
                    _ => continue,
                }
            }
            self.edge(prefix.len().saturating_add(1))?;
            self.budget.items(prefix.len().saturating_add(1))?;
            for component in &prefix {
                self.string(component)?;
            }
            let mut path = Vec::new();
            path.try_reserve_exact(prefix.len().saturating_add(1))
                .map_err(|error| format!("public path allocation failed: {error}"))?;
            path.extend(prefix.iter().cloned());
            let child_name = child.ident.name.as_str();
            path.push(self.budget.owned(child_name)?);
            let binding = if child.ident.name == kw::Underscore {
                Binding::AnonymousImport
            } else {
                Binding::Nameable
            };
            let (namespace, target, aliases, nested, named) = match res {
                Res::Def(kind, def_id) => {
                    let Some(namespace) = kind.ns().map(namespace) else {
                        return Err(format!(
                            "external reachable definition has no namespace: {kind:?}"
                        ));
                    };
                    if matches!(
                        associated_path_kind(self.tcx, def_id)?,
                        Some(AssociatedPathKind::AnonymousSupport(_))
                    ) {
                        return Err(
                            "anonymous associated support cannot become a reachable export"
                                .to_owned(),
                        );
                    }
                    if binding == Binding::AnonymousImport
                        && (kind != DefKind::Trait
                            || child.reexport_chain.is_empty()
                            || !matches!(
                                child.reexport_chain.last(),
                                Some(RustReexport::Single(_))
                            ))
                    {
                        return Err(
                            "anonymous import lacks exact trait single-import evidence".to_owned()
                        );
                    }
                    let named = !names_only || self.definitions.contains_key(&(def_id, namespace));
                    if !names_only {
                        self.breadcrumb(
                            "begin",
                            "selected-root",
                            "selected-export",
                            def_id,
                            namespace,
                            "definition+namespace",
                        )?;
                        self.ensure_with_demand(
                            def_id,
                            namespace,
                            if binding == Binding::AnonymousImport {
                                Demand::ShapeOnly
                            } else {
                                Demand::SelectedRoot
                            },
                        )?;
                        self.breadcrumb(
                            "complete",
                            "selected-root",
                            "selected-export",
                            def_id,
                            namespace,
                            "definition+namespace",
                        )?;
                    }
                    let display_path = self.definition_path(def_id)?;
                    let aliases = if display_path == path {
                        Vec::new()
                    } else {
                        self.budget.items(1)?;
                        self.charge_strings(&path)?;
                        let mut values = Vec::new();
                        values
                            .try_reserve_exact(1)
                            .map_err(|error| format!("alias allocation failed: {error}"))?;
                        values.push(Alias {
                            from: display_path,
                            to: path.clone(),
                            span: self.span(child.ident.span)?,
                        });
                        values
                    };
                    (
                        namespace,
                        ExportTarget::Definition {
                            id: self.id(def_id, namespace)?,
                        },
                        aliases,
                        (kind == DefKind::Mod).then_some(def_id),
                        named,
                    )
                }
                Res::PrimTy(primitive) => (
                    Namespace::Type,
                    ExportTarget::Unsupported {
                        compiler_kind: {
                            self.budget
                                .formatted(format_args!("primitive:{primitive:?}"))?
                        },
                    },
                    Vec::new(),
                    None,
                    true,
                ),
                other => {
                    return Err(format!(
                        "external reachable unsupported compiler resolution: {other:?}"
                    ));
                }
            };
            if binding == Binding::AnonymousImport
                && !matches!(target, ExportTarget::Definition { .. })
            {
                return Err("anonymous import target is not a definition".to_owned());
            }
            let span = self.span(child.ident.span)?;
            self.edge(inherited.len())?;
            self.charge_reexports(inherited)?;
            self.budget.items(child.reexport_chain.len())?;
            let mut reexports = Vec::new();
            reexports
                .try_reserve_exact(inherited.len().saturating_add(child.reexport_chain.len()))
                .map_err(|error| format!("re-export allocation failed: {error}"))?;
            reexports.extend_from_slice(inherited);
            self.edge(child.reexport_chain.len())?;
            for reexport in &child.reexport_chain {
                // A names-only walk extracts nothing, so a hop through a crate
                // none of whose definitions were extracted registers it here.
                if names_only
                    && let Some(hop) = reexport.id()
                    && hop.krate != self.selected
                {
                    if self.is_sysroot_crate(hop.krate)? {
                        self.register_sysroot(hop.krate)?;
                    } else {
                        self.register_external(hop.krate)?;
                    }
                }
                reexports.push(self.reexport(*reexport)?);
            }
            let Root::Selected {
                aliases: root_aliases,
            } = &self.invocation.root;
            self.edge(root_aliases.len())?;
            for root_alias in root_aliases.iter().filter(|_| named) {
                self.charge_export_clone(root_alias, &path, &target, &aliases, &reexports, &span)?;
                self.reachable
                    .try_reserve(1)
                    .map_err(|error| format!("reachable export allocation failed: {error}"))?;
                self.reachable.push(Export {
                    binding,
                    root_alias: root_alias.clone(),
                    path: path.clone(),
                    namespace,
                    target: target.clone(),
                    aliases: aliases.clone(),
                    reexports: reexports.clone(),
                    span: span.clone(),
                    variant: variant.clone(),
                });
            }
            let depth = depth.saturating_add(1);
            match nested {
                // A re-exported foreign crate root such as `pub use wgpu;`
                // is not extracted; it is only named after extraction drains.
                Some(module) if !names_only && self.is_foreign_crate_root(module) => {
                    self.foreign_roots.try_reserve(1).map_err(|error| {
                        format!("foreign crate root allocation failed: {error}")
                    })?;
                    self.foreign_roots.push(ForeignRoot {
                        module,
                        path,
                        reexports,
                        depth,
                    });
                }
                Some(module) => {
                    self.collect_external_exports(module, path, &reexports, depth, names_only)?;
                }
                None => {}
            }
        }
        self.breadcrumb(
            "complete",
            "selected-exports",
            "module-exports",
            module,
            Namespace::Type,
            "definition",
        )?;
        Ok(())
    }

    /// Select what one demanded path names, resolved through
    /// public module children as Rust resolves it (globs and shadowing are
    /// already settled in the crate's metadata). Every item a segment lands
    /// on, in any namespace, is extracted as a selected root, so a module on
    /// the path is named and a type brings its impl tables; segments after a
    /// non-module (`Button::new`) are members its closure already holds. A
    /// path that names nothing extracts nothing and stays a missing item. A
    /// final `*` is a glob import: every public child of the module.
    fn select_path(&mut self, module: DefId, path: &[String], depth: usize) -> Result<(), String> {
        if depth > MAX_DEPTH {
            return Err("demanded path traversal depth exceeded".to_owned());
        }
        let Some((name, rest)) = path.split_first() else {
            return Ok(());
        };
        let tcx = self.tcx;
        // A macro's `$crate` (`crate_segment`): the root of the crate that
        // defines it, as the macro's own paths are written.
        if depth == 0 && name.starts_with(CRATE_SEGMENT) {
            if let Some(krate) = tcx
                .crates(())
                .iter()
                .copied()
                .find(|krate| crate_segment(tcx, *krate) == *name)
            {
                let root = krate.as_def_id();
                if !self.crate_roots.iter().any(|(known, _)| *known == root) {
                    self.crate_roots
                        .try_reserve(1)
                        .map_err(|error| format!("crate root allocation failed: {error}"))?;
                    self.crate_roots.push((root, name.clone()));
                }
                return self.select_path(root, rest, depth.saturating_add(1));
            }
            return Ok(());
        }
        for child in tcx.module_children(module) {
            if !child.vis.is_public() || (name != "*" && child.ident.name.as_str() != name) {
                continue;
            }
            let Res::Def(kind, def_id) = child.res else {
                continue;
            };
            // A re-exported variant, as `pub use Alignment::Center;`, selects
            // its enum.
            let (kind, def_id) = match kind {
                DefKind::Variant => (DefKind::Enum, tcx.parent(def_id)),
                DefKind::Ctor(CtorOf::Variant, _) => {
                    (DefKind::Enum, tcx.parent(tcx.parent(def_id)))
                }
                _ => (kind, def_id),
            };
            let Some(ns) = kind.ns() else {
                continue;
            };
            self.ensure_with_demand(def_id, namespace(ns), Demand::SelectedRoot)?;
            // `pub type Duration = TimeDelta;` names the type with its
            // members: selecting the alias selects the type.
            let (def_id, kind) = match kind {
                DefKind::TyAlias => match tcx
                    .type_of(def_id)
                    .instantiate_identity()
                    .skip_norm_wip()
                    .kind()
                {
                    ty::Adt(definition, arguments) if arguments.is_empty() => {
                        let target = definition.did();
                        self.ensure_with_demand(target, Namespace::Type, Demand::SelectedRoot)?;
                        (target, tcx.def_kind(target))
                    }
                    // A generic alias, as `pub type Element<'a, M, Theme =
                    // Plain> = element::Element<'a, M, Theme>;`, or one with
                    // fixed arguments, as `pub type Result = Result<(),
                    // Error>;`, selects each type it and its defaults name,
                    // with their impl tables, as rustc proves their bounds.
                    // `Alias::member` is its type's member.
                    aliased => {
                        let defaults = tcx
                            .generics_of(def_id)
                            .own_params
                            .iter()
                            .filter_map(|parameter| parameter.default_value(tcx))
                            .filter_map(|default| {
                                default.instantiate_identity().skip_norm_wip().as_type()
                            })
                            .collect::<Vec<_>>();
                        let named = tcx
                            .type_of(def_id)
                            .instantiate_identity()
                            .skip_norm_wip()
                            .walk()
                            .chain(defaults.into_iter().flat_map(|default| default.walk()))
                            .filter_map(|argument| argument.as_type())
                            .filter_map(|ty| match ty.kind() {
                                ty::Adt(definition, _) => Some(definition.did()),
                                _ => None,
                            })
                            .collect::<Vec<_>>();
                        for target in named {
                            self.ensure_with_demand(target, Namespace::Type, Demand::SelectedRoot)?;
                        }
                        match aliased {
                            ty::Adt(definition, _) => {
                                (definition.did(), tcx.def_kind(definition.did()))
                            }
                            _ => (def_id, kind),
                        }
                    }
                },
                _ => (def_id, kind),
            };
            if kind == DefKind::Mod && !rest.is_empty() {
                self.select_path(def_id, rest, depth.saturating_add(1))?;
            } else if matches!(kind, DefKind::Struct | DefKind::Enum)
                && let [member] = rest
                && member != "*"
            {
                // `Type::member`: the type's own member, or one a trait it
                // implements provides.
                self.budget.items(1)?;
                self.member_demands
                    .try_reserve(1)
                    .map_err(|error| format!("member demand allocation failed: {error}"))?;
                self.member_demands.push((def_id, member.clone()));
            } else if kind == DefKind::Trait
                && let [member] = rest
                && member != "*"
            {
                // `Trait::member`: the member of the trait's impls for a
                // type Rawr holds as its own string.
                self.budget.items(1)?;
                self.trait_member_demands
                    .try_reserve(1)
                    .map_err(|error| format!("member demand allocation failed: {error}"))?;
                self.trait_member_demands.push((def_id, member.clone()));
            }
        }
        Ok(())
    }

    /// Each demanded `Trait::member`: the member of each of the trait's
    /// impls for `&str`, as iced's `impl Widget<..> for &str` gives
    /// `Widget::_boxed("label")` that `column!` writes. Rawr's string is
    /// such a receiver, pinned as `&'static str`.
    fn resolve_trait_member_demands(&mut self) -> Result<(), String> {
        let tcx = self.tcx;
        let mut demands = std::mem::take(&mut self.trait_member_demands);
        demands.sort_unstable_by_key(|(trait_id, member)| {
            (
                tcx.def_path_hash(*trait_id).to_raw_def_path_hash().0,
                member.clone(),
            )
        });
        demands.dedup();
        for (trait_id, member) in demands {
            let name = Symbol::intern(&member);
            let Some(trait_item) = tcx
                .associated_items(trait_id)
                .filter_by_name_unhygienic(name)
                .find(|item| item.is_fn())
                .map(|item| item.def_id)
            else {
                continue;
            };
            let mut implementations = Vec::new();
            for impl_id in tcx.all_impls(trait_id) {
                self.budget.edge(1)?;
                let Some(reference) = tcx.impl_opt_trait_ref(impl_id) else {
                    continue;
                };
                let reference = reference.instantiate_identity().skip_norm_wip();
                if matches!(
                    reference.self_ty().kind(),
                    ty::Ref(_, target, ty::Mutability::Not) if target.is_str()
                ) && tcx.impl_polarity(impl_id) == ty::ImplPolarity::Positive
                {
                    implementations
                        .try_reserve(1)
                        .map_err(|error| format!("member demand allocation failed: {error}"))?;
                    implementations.push((impl_id, reference.args));
                }
            }
            implementations.sort_unstable_by_key(|(impl_id, _)| {
                tcx.def_path_hash(*impl_id).to_raw_def_path_hash().0
            });
            for (impl_id, arguments) in implementations {
                let obligation = AssociatedObligation {
                    kind: AssociatedDemandKind::SelectedMember,
                    trait_item: Some(trait_item),
                    arguments: Some(arguments),
                    param_env: tcx.param_env(impl_id),
                };
                self.request_implementation(impl_id, trait_id, Some(obligation))?;
            }
        }
        Ok(())
    }

    /// Each demanded `Type::member` that is no inherent member of the type:
    /// the member of every trait the type implements that declares it. A
    /// trait implemented for the type itself selects that member of its
    /// implementation. A blanket implementation (`impl<R: RngCore> Rng for
    /// R`) applies to the type as if the crate wrote `impl Rng for Type`.
    /// rustc proves the type implements the trait, under the type's own
    /// bounds for a generic type. Only a trait without generic parameters
    /// of its own is looked up.
    fn resolve_member_demands(&mut self) -> Result<(), String> {
        let tcx = self.tcx;
        let mut demands = std::mem::take(&mut self.member_demands);
        demands.sort_unstable_by_key(|(adt, member)| {
            (
                tcx.def_path_hash(*adt).to_raw_def_path_hash().0,
                member.clone(),
            )
        });
        demands.dedup();
        for (adt, member) in demands {
            let generic = tcx.generics_of(adt).count() != 0;
            let name = Symbol::intern(&member);
            let inherent = tcx.inherent_impls(adt).iter().any(|implementation| {
                tcx.associated_items(*implementation)
                    .filter_by_name_unhygienic(name)
                    .next()
                    .is_some()
            });
            if inherent {
                continue;
            }
            let self_ty = tcx.type_of(adt).instantiate_identity().skip_norm_wip();
            let param_env = tcx.param_env(adt);
            let mut traits = tcx
                .visible_traits()
                .filter(|trait_id| {
                    tcx.associated_items(*trait_id)
                        .filter_by_name_unhygienic(name)
                        .any(|item| item.is_fn())
                })
                .collect::<Vec<_>>();
            traits.sort_unstable_by_key(|trait_id| {
                tcx.def_path_hash(*trait_id).to_raw_def_path_hash().0
            });
            traits.dedup();
            // A generic type whose every parameter has a default is reached
            // at that instance, which its path names as a type: palette's
            // `impl<S, T: Stimulus> Default for Rgb<S, T>` applies to
            // `Rgb<Srgb, f32>`, though not to every `Rgb<S, T>`.
            let defaulted = generic.then(|| defaulted_instance(tcx, adt)).flatten();
            for trait_id in traits {
                self.edge(1)?;
                // A generic trait, as iced's `Widget<Message, Theme,
                // Renderer>`, is the type's own implementation of it, whose
                // trait arguments rustc proves where the bridge calls the
                // member: its provided `_boxed` is the type's.
                let trait_generic = tcx.generics_of(trait_id).count() != 1;
                let implements = |self_ty: ty::Ty<'tcx>| {
                    tcx.infer_ctxt()
                        .build(ty::TypingMode::non_body_analysis())
                        .type_implements_trait(trait_id, [self_ty], param_env)
                        .must_apply_modulo_regions()
                };
                let applies =
                    trait_generic || implements(self_ty) || defaulted.is_some_and(implements);
                if !applies {
                    continue;
                }
                let Some(trait_item) = tcx
                    .associated_items(trait_id)
                    .filter_by_name_unhygienic(name)
                    .find(|item| item.is_fn())
                    .map(|item| item.def_id)
                else {
                    continue;
                };
                let mut candidates = Vec::new();
                tcx.for_each_relevant_impl(trait_id, self_ty, |implementation| {
                    candidates.push(implementation);
                });
                let own = candidates.iter().copied().find(|implementation| {
                    tcx.impl_opt_trait_ref(*implementation)
                        .is_some_and(|reference| {
                            matches!(
                                reference.instantiate_identity().skip_norm_wip().self_ty().kind(),
                                ty::Adt(definition, _) if definition.did() == adt
                            )
                        })
                });
                let arguments = match own {
                    Some(implementation) if trait_generic => tcx
                        .impl_opt_trait_ref(implementation)
                        .map(|reference| reference.instantiate_identity().skip_norm_wip().args)
                        .unwrap_or_else(|| tcx.mk_args(&[self_ty.into()])),
                    _ => tcx.mk_args(&[self_ty.into()]),
                };
                let obligation = AssociatedObligation {
                    kind: AssociatedDemandKind::SelectedMember,
                    trait_item: Some(trait_item),
                    arguments: Some(arguments),
                    param_env,
                };
                if let Some(implementation) = own {
                    self.request_implementation(implementation, adt, Some(obligation))?;
                    continue;
                }
                // A blanket implementation of a generic trait is left
                // unresolved, as rustc would choose its arguments.
                if trait_generic {
                    continue;
                }
                let blankets = candidates
                    .iter()
                    .copied()
                    .filter(|implementation| {
                        tcx.generics_of(*implementation).count() == 1
                            && tcx
                                .impl_opt_trait_ref(*implementation)
                                .is_some_and(|reference| {
                                    matches!(
                                        reference
                                            .instantiate_identity()
                                            .skip_norm_wip()
                                            .self_ty()
                                            .kind(),
                                        ty::Param(_)
                                    )
                                })
                    })
                    .collect::<Vec<_>>();
                // Coherence admits one blanket implementation of a trait
                // for any one type. Several candidates whose bounds rustc
                // would have to choose between are left unresolved, as is
                // a blanket implementation for a generic type.
                if let [implementation] = blankets.as_slice()
                    && !generic
                {
                    self.apply_blanket(*implementation, adt, self_ty, obligation)?;
                }
            }
        }
        Ok(())
    }

    /// The blanket implementation `implementation` applied to the concrete
    /// type `adt`: an implementation of the trait for that type, with the
    /// demanded member and no generics of its own.
    fn apply_blanket(
        &mut self,
        implementation: DefId,
        adt: DefId,
        self_ty: Ty<'tcx>,
        obligation: AssociatedObligation<'tcx>,
    ) -> Result<(), String> {
        let tcx = self.tcx;
        let mut digest = Sha256::new();
        digest.update(b"rawr.blanket-application/v1");
        digest.update(tcx.def_path_hash(implementation).to_raw_def_path_hash().0);
        digest.update(tcx.def_path_hash(adt).to_raw_def_path_hash().0);
        let digest = digest.finalize();
        let key = hex(&digest[..16])?;
        let instantiated = tcx
            .impl_opt_trait_ref(implementation)
            .ok_or_else(|| "blanket implementation lost its trait".to_owned())?
            .instantiate(tcx, tcx.mk_args(&[self_ty.into()]))
            .skip_norm_wip();
        let identity = tcx
            .impl_opt_trait_ref(implementation)
            .map(|reference| reference.instantiate_identity().skip_norm_wip());
        match implementation_crate_authority(self.is_sysroot_crate(implementation.krate)?) {
            ImplementationCrateAuthority::Sysroot => self.register_sysroot(implementation.krate)?,
            ImplementationCrateAuthority::External => {
                self.register_external(implementation.krate)?
            }
        }
        let mut id = self.id(implementation, Namespace::Type)?;
        id.def_path_hash = key.clone();
        let mut obligations = BTreeMap::new();
        obligations.insert(self.associated_obligation_order_key(obligation), obligation);
        let associated =
            self.expand_associated_obligations(implementation, identity, &obligations, &id, true)?;
        if let Some(existing) = self.applications.get_mut(&key) {
            existing.associated.extend(associated);
            existing.associated.sort_unstable();
            existing.associated.dedup();
            return Ok(());
        }
        self.ensure(adt, Namespace::Type)?;
        self.ensure(instantiated.def_id, Namespace::Type)?;
        // The ledger keeps relevance in identity order.
        let mut relevance = vec![
            self.id(adt, Namespace::Type)?,
            self.id(instantiated.def_id, Namespace::Type)?,
        ];
        relevance.sort_unstable();
        relevance.dedup();
        let header = tcx.impl_trait_header(implementation);
        let record = Implementation {
            key: key.clone(),
            id,
            relevance,
            trait_ref: Trait::Trait {
                id: self.id(instantiated.def_id, Namespace::Type)?,
                arguments: self.arguments(instantiated.args, adt, 0)?,
            },
            self_type: self.type_of(self_ty, adt, 0)?,
            safety: safety(header.safety),
            polarity: Polarity::Positive,
            const_impl: false,
            default_impl: false,
            generics: Generics {
                parameters: Vec::new(),
                predicates: Vec::new(),
            },
            associated,
            span: self.span(tcx.def_span(implementation))?,
            origin: self.origin(tcx.def_span(implementation))?,
        };
        self.budget.items(1)?;
        self.applications.insert(key, record);
        Ok(())
    }

    fn is_foreign_crate_root(&self, def_id: DefId) -> bool {
        def_id.is_crate_root() && def_id.krate != self.selected
    }

    fn reexport(&mut self, reexport: RustReexport) -> Result<Reexport, String> {
        let identified =
            |builder: &mut Self, kind: ReexportKind, id: DefId| -> Result<Reexport, String> {
                let crate_name = builder.tcx.crate_name(id.krate);
                let stable_crate_id = builder.budget.formatted(format_args!(
                    "{:016x}",
                    builder.tcx.stable_crate_id(id.krate)
                ))?;
                Ok(Reexport {
                    kind,
                    evidence: ReexportEvidence::Identified {
                        item: ReexportId {
                            crate_name: builder.budget.owned(crate_name.as_str())?,
                            stable_crate_id,
                            def_path_hash: builder.def_hash(id)?,
                        },
                        span: builder.span(builder.tcx.def_span(id))?,
                    },
                })
            };
        match reexport {
            RustReexport::Single(id) => identified(self, ReexportKind::Single, id),
            RustReexport::Glob(id) => identified(self, ReexportKind::Glob, id),
            RustReexport::ExternCrate(id) => identified(self, ReexportKind::ExternCrate, id),
            RustReexport::MacroUse => Ok(Reexport {
                kind: ReexportKind::MacroUse,
                evidence: ReexportEvidence::IdentitylessMacro,
            }),
            RustReexport::MacroExport => Ok(Reexport {
                kind: ReexportKind::MacroExport,
                evidence: ReexportEvidence::IdentitylessMacro,
            }),
        }
    }

    /// The public alias of the selected crate with no parameters that
    /// names `ty`, a generic type at fixed arguments, as iced's
    /// `Renderer` names `fallback::Renderer<iced_wgpu::Renderer,
    /// iced_tiny_skia::Renderer>` (D604.5). The selected crate's modules
    /// are walked once.
    fn instance_alias(&mut self, ty: Ty<'tcx>) -> Option<DefId> {
        if !matches!(ty.kind(), ty::Adt(_, arguments) if !arguments.is_empty()) {
            return None;
        }
        self.find_aliases();
        self.instance_aliases
            .as_ref()?
            .iter()
            .find(|(_, aliased)| *aliased == ty)
            .map(|(alias, _)| *alias)
    }

    /// The selected crate's public alias that passes its own parameters on,
    /// all and in order, to the struct or enum `target`, as iced's
    /// `widget::Text<'a, Theme = ..> = core::widget::Text<'a, Theme>`: Rust
    /// code outside the crates may write the type only through it.
    fn passing_alias(&mut self, target: DefId) -> Option<DefId> {
        self.find_aliases();
        self.passing_aliases.get(&target).copied()
    }

    /// Walks the selected crate's modules once for its public aliases.
    fn find_aliases(&mut self) {
        if self.instance_aliases.is_none() {
            let tcx = self.tcx;
            let mut found = Vec::new();
            let mut passing = HashMap::new();
            let mut modules = vec![self.selected.as_def_id()];
            let mut seen = HashSet::new();
            while let Some(module) = modules.pop() {
                if !seen.insert(module) {
                    continue;
                }
                for child in tcx.module_children(module) {
                    if !child.vis.is_public() {
                        continue;
                    }
                    let Res::Def(kind, def_id) = child.res else {
                        continue;
                    };
                    match kind {
                        DefKind::Mod if def_id.krate == self.selected => modules.push(def_id),
                        DefKind::TyAlias if tcx.generics_of(def_id).count() == 0 => {
                            let aliased =
                                tcx.type_of(def_id).instantiate_identity().skip_norm_wip();
                            if matches!(aliased.kind(), ty::Adt(_, arguments) if !arguments.is_empty())
                                && !found.iter().any(|(known, _)| *known == def_id)
                            {
                                found.push((def_id, aliased));
                            }
                        }
                        DefKind::TyAlias => {
                            let generics = tcx.generics_of(def_id);
                            let aliased =
                                tcx.type_of(def_id).instantiate_identity().skip_norm_wip();
                            if generics.parent_count == 0
                                && let ty::Adt(definition, arguments) = aliased.kind()
                                && arguments.len() == generics.own_params.len()
                                && arguments.iter().enumerate().all(|(index, argument)| {
                                    match argument.kind() {
                                        ty::GenericArgKind::Type(ty) => matches!(
                                            ty.kind(),
                                            ty::Param(parameter) if parameter.index as usize == index
                                        ),
                                        ty::GenericArgKind::Lifetime(region) => matches!(
                                            region.kind(),
                                            ty::ReEarlyParam(parameter) if parameter.index as usize == index
                                        ),
                                        ty::GenericArgKind::Const(_) => false,
                                    }
                                })
                            {
                                passing.entry(definition.did()).or_insert(def_id);
                            }
                        }
                        _ => {}
                    }
                }
            }
            self.instance_aliases = Some(found);
            self.passing_aliases = passing;
        }
    }

    fn ensure_with_demand(
        &mut self,
        def_id: DefId,
        namespace: Namespace,
        demand: Demand,
    ) -> Result<(), String> {
        let owner = self.compiler_definition_owner(def_id)?;
        self.request_definition(def_id, namespace, owner, demand)
    }

    fn request_definition(
        &mut self,
        def_id: DefId,
        namespace: Namespace,
        owner: Owner,
        demand: Demand,
    ) -> Result<(), String> {
        // A type the root reaches only through an alias that passes its
        // parameters on is written through that alias, so the alias comes
        // with it (D604.5).
        if namespace == Namespace::Type
            && matches!(self.tcx.def_kind(def_id), DefKind::Struct | DefKind::Enum)
            && let Some(alias) = self.passing_alias(def_id)
        {
            let key = SemanticKey {
                def_path_hash: self.tcx.def_path_hash(alias).to_raw_def_path_hash().0,
                namespace: Namespace::Type,
            };
            if !self.scheduler.entries.contains_key(&key) {
                self.ensure_with_demand(alias, Namespace::Type, Demand::ShapeOnly)?;
            }
        }
        let key = SemanticKey {
            def_path_hash: self.tcx.def_path_hash(def_id).to_raw_def_path_hash().0,
            namespace,
        };
        if !self.scheduler.entries.contains_key(&key)
            && self.scheduler.entries.len() >= MAX_DEFINITIONS
        {
            return Err("compiler definition limit exceeded".to_owned());
        }
        if let Some(existing) = self.scheduler.entries.get(&key)
            && (existing.value.def_id != def_id || existing.value.namespace != namespace)
        {
            return Err("stable semantic traversal identity collision".to_owned());
        }
        if let Some(existing) = self.scheduler.entries.get(&key)
            && existing.value.owner != owner
        {
            return Err(format!(
                "stable definition owner traversal identity collision at `{}`",
                self.tcx.def_path_str(def_id)
            ));
        }
        self.scheduler.request(
            key,
            DefinitionRequest {
                def_id,
                namespace,
                owner,
            },
            demand,
            self.budget,
        )
    }

    fn drain_scheduler(&mut self) -> Result<(), String> {
        loop {
            if let Some((key, request, demand)) = self.scheduler.next()? {
                let operation = match demand {
                    Demand::ShapeOnly => "ensure-shape",
                    Demand::SelectedRoot => "ensure-selected",
                };
                let stage = match demand {
                    Demand::ShapeOnly => "shape-only",
                    Demand::SelectedRoot => "selected-root",
                };
                self.breadcrumb(
                    "begin",
                    stage,
                    operation,
                    request.def_id,
                    request.namespace,
                    "definition+namespace",
                )?;
                let previous_demand = self.active_demand;
                self.active_demand = demand;
                let operation_result: Result<(), String> = (|| {
                    self.materialize_shape(request.def_id, request.namespace, request.owner)?;
                    if matches!(demand, Demand::SelectedRoot) {
                        self.expand_implementations(request.def_id)?;
                        self.expand_selected_surface(request.def_id)?;
                    }
                    Ok(())
                })();
                self.active_demand = previous_demand;
                operation_result?;
                self.breadcrumb(
                    "complete",
                    stage,
                    operation,
                    request.def_id,
                    request.namespace,
                    "definition+namespace",
                )?;
                self.scheduler.complete(key, demand, self.budget)?;
                continue;
            }
            if self.queued_obligations != 0 {
                self.drain_trait_obligations()?;
                continue;
            }
            if let Some((_, request)) = self.implementation_requests.pop_first() {
                self.insert_external_implementation(request)?;
                continue;
            }
            break;
        }
        Ok(())
    }

    fn request_trait_obligation(
        &mut self,
        owner: DefId,
        trait_id: DefId,
        arguments: ty::GenericArgsRef<'tcx>,
        polarity: ty::PredicatePolarity,
        associated: Option<DefId>,
    ) -> Result<(), String> {
        let self_ty = arguments
            .iter()
            .next()
            .and_then(|argument| argument.as_type())
            .ok_or_else(|| "trait obligation Self argument is absent".to_owned())?;
        let obligation = TraitObligation {
            trait_id,
            self_ty,
            arguments,
            polarity,
            associated,
            param_env: self.tcx.param_env(owner),
            relevance: owner,
        };
        let key = self.trait_obligation_order_key(obligation);
        if let Some(existing) = self.obligations.get(&key) {
            if existing.obligation != obligation {
                return Err("stable trait obligation traversal identity collision".to_owned());
            }
            if self.active_demand == Demand::SelectedRoot && !existing.selected {
                let existing = self
                    .obligations
                    .get_mut(&key)
                    .ok_or_else(|| "trait obligation selection state disappeared".to_owned())?;
                existing.selected = true;
            }
            return Ok(());
        }
        self.budget.items(1)?;
        if self
            .obligations
            .insert(
                key,
                ScheduledTraitObligation {
                    obligation,
                    state: TraitObligationState::Queued,
                    selected: self.active_demand == Demand::SelectedRoot,
                },
            )
            .is_some()
        {
            return Err("duplicate trait obligation traversal identity".to_owned());
        }
        self.queued_obligations = self
            .queued_obligations
            .checked_add(1)
            .ok_or_else(|| "queued trait obligation count overflowed".to_owned())?;
        Ok(())
    }

    fn trait_obligation_order_key(
        &self,
        obligation: TraitObligation<'tcx>,
    ) -> TraitObligationOrderKey {
        let mut substitution = DefaultHasher::new();
        obligation.self_ty.hash(&mut substitution);
        obligation.arguments.hash(&mut substitution);
        obligation.polarity.hash(&mut substitution);
        obligation.param_env.hash(&mut substitution);
        TraitObligationOrderKey {
            trait_hash: self
                .tcx
                .def_path_hash(obligation.trait_id)
                .to_raw_def_path_hash()
                .0,
            associated_hash: obligation
                .associated
                .map(|associated| self.tcx.def_path_hash(associated).to_raw_def_path_hash().0)
                .unwrap_or_default(),
            substitution_hash: substitution.finish(),
            relevance_hash: self
                .tcx
                .def_path_hash(obligation.relevance)
                .to_raw_def_path_hash()
                .0,
        }
    }

    fn drain_trait_obligations(&mut self) -> Result<(), String> {
        let mut obligations = std::mem::take(&mut self.obligations);
        let mut queued = std::mem::take(&mut self.queued_obligations);
        let mut result = Ok(());
        for scheduled in obligations.values_mut() {
            if scheduled.state == TraitObligationState::Complete {
                continue;
            }
            if let Err(error) =
                self.process_trait_obligation(scheduled.obligation, scheduled.selected)
            {
                result = Err(error);
                break;
            }
            scheduled.state = TraitObligationState::Complete;
            queued = match queued.checked_sub(1) {
                Some(queued) => queued,
                None => {
                    result = Err("queued trait obligation count underflowed".to_owned());
                    break;
                }
            };
        }
        if result.is_ok() && (!self.obligations.is_empty() || self.queued_obligations != 0) {
            result = Err("trait obligation traversal recursively enqueued work".to_owned());
        }
        self.obligations = obligations;
        self.queued_obligations = queued;
        result
    }

    fn process_trait_obligation(
        &mut self,
        obligation: TraitObligation<'tcx>,
        selected: bool,
    ) -> Result<(), String> {
        if self.tcx.def_kind(obligation.trait_id) != DefKind::Trait {
            return Err("implementation obligation owner is not a trait".to_owned());
        }
        // A parameter, bound variable, or alias carries symbolic trait
        // evidence. It has no concrete receiver candidate until a later
        // instantiation and therefore must not fan out to every currently
        // loaded implementation. Concrete constructors containing
        // parameters (for example `Wrapper<T>` or `&T`) remain eligible.
        match obligation.self_ty.kind() {
            ty::Param(parameter) if selected => {
                self.request_relation_implementations(obligation)?;
                return self.process_scalar_predicate_obligation(obligation, *parameter);
            }
            ty::Param(_) | ty::Bound(..) | ty::Placeholder(_) | ty::Infer(_) | ty::Alias(..) => {
                return Ok(())
            }
            _ => {}
        }
        let obligation_key = obligation.key();
        let (mut candidates, cache_was_absent) =
            match take_owned_candidate_bucket(&mut self.obligation_candidates, &obligation_key) {
                Some(candidates) => (candidates, false),
                None => (Vec::new(), true),
            };
        if cache_was_absent {
            let mut candidate_error = None;
            let tcx = self.tcx;
            let budget = &mut *self.budget;
            tcx.for_each_relevant_impl(obligation.trait_id, obligation.self_ty, |impl_id| {
                if candidate_error.is_some() {
                    return;
                }
                if candidates.len() >= MAX_DEFINITIONS {
                    candidate_error =
                        Some("relevant implementation candidate limit exceeded".to_owned());
                    return;
                }
                if let Err(error) = budget.edge(1) {
                    candidate_error = Some(error);
                    return;
                }
                if let Err(error) = budget.items(1) {
                    candidate_error = Some(error);
                    return;
                }
                if let Err(error) = candidates.try_reserve(1) {
                    candidate_error = Some(format!(
                        "relevant implementation candidate allocation failed: {error}"
                    ));
                    return;
                }
                candidates.push(impl_id);
            });
            if let Some(error) = candidate_error {
                return Err(error);
            }
            candidates.sort_unstable_by_key(|impl_id| {
                self.tcx.def_path_hash(*impl_id).to_raw_def_path_hash().0
            });
            candidates.dedup();
            self.budget.items(1)?;
            self.obligation_candidates.try_reserve(1).map_err(|error| {
                format!("trait obligation candidate cache allocation failed: {error}")
            })?;
        }
        let result = (|| {
            for impl_id in candidates.iter().copied() {
                let candidate_trait = self
                    .tcx
                    .impl_opt_trait_ref(impl_id)
                    .map(|reference| reference.instantiate_identity().skip_norm_wip())
                    .ok_or_else(|| {
                        "rustc relevant trait candidate is an inherent implementation".to_owned()
                    })?;
                if candidate_trait.def_id != obligation.trait_id {
                    return Err("rustc relevant candidate trait identity disagrees".to_owned());
                }
                let associated = obligation
                    .associated
                    .map(|trait_item| AssociatedObligation {
                        kind: AssociatedDemandKind::SelectedMember,
                        trait_item: Some(trait_item),
                        arguments: Some(obligation.arguments),
                        param_env: obligation.param_env,
                    });
                self.request_implementation(impl_id, obligation.relevance, associated)?;
            }
            Ok(())
        })();
        if self
            .obligation_candidates
            .insert(obligation_key, candidates)
            .is_some()
        {
            return Err("trait obligation candidate cache identity collision".to_owned());
        }
        result
    }

    /// A selected item's bound `R: Trait<T>` that ties its parameter to
    /// another: the trait's implementations, so the classifier can read
    /// which self types give which trait arguments. rand's `random_range`
    /// has `R: SampleRange<T>`, implemented for `Range<T>`. Only a trait
    /// outside the sysroot with at most [`RELATION_IMPLEMENTATIONS`]
    /// implementations is read.
    fn request_relation_implementations(
        &mut self,
        obligation: TraitObligation<'tcx>,
    ) -> Result<(), String> {
        /// More implementations than this are not read.
        const RELATION_IMPLEMENTATIONS: usize = 64;
        // `P: Into<T>`, as Freya's `WindowConfig::new(impl Into<AppComponent>)`:
        // `T`'s implementations of `From<F>` for a parameter `F`, so the
        // classifier can read the callable one (D551.6), and those for a
        // concrete `S`, as Freya's `From<(u8, u8, u8)> for Fill`, so it can
        // read which argument shapes give `T`.
        if obligation.polarity == ty::PredicatePolarity::Positive
            && obligation.associated.is_none()
            && Some(obligation.trait_id) == self.tcx.get_diagnostic_item(rustc_span::sym::Into)
            && let Some(target) = obligation
                .arguments
                .get(1)
                .and_then(|argument| argument.as_type())
            && let ty::Adt(..) = target.kind()
            && let Some(from) = self.tcx.get_diagnostic_item(rustc_span::sym::From)
        {
            let mut implementations = Vec::new();
            self.tcx
                .for_each_relevant_impl(from, target, |impl_id| implementations.push(impl_id));
            implementations.sort_unstable_by_key(|impl_id| {
                self.tcx.def_path_hash(*impl_id).to_raw_def_path_hash().0
            });
            for impl_id in implementations.into_iter().take(RELATION_IMPLEMENTATIONS) {
                self.budget.edge(1)?;
                if self.tcx.impl_polarity(impl_id) == ty::ImplPolarity::Positive {
                    self.request_implementation(impl_id, obligation.relevance, None)?;
                }
            }
            return Ok(());
        }
        if obligation.polarity != ty::PredicatePolarity::Positive
            || obligation.associated.is_some()
            || obligation.arguments.len() < 2
            || obligation.trait_id.krate == rustc_span::def_id::LOCAL_CRATE
            || self.is_sysroot_crate(obligation.trait_id.krate)?
        {
            return Ok(());
        }
        let mut implementations = Vec::new();
        for impl_id in self.tcx.all_impls(obligation.trait_id) {
            if implementations.len() == RELATION_IMPLEMENTATIONS {
                return Ok(());
            }
            self.budget.edge(1)?;
            implementations
                .try_reserve(1)
                .map_err(|error| format!("relation implementation allocation failed: {error}"))?;
            implementations.push(impl_id);
        }
        implementations.sort_unstable_by_key(|impl_id| {
            self.tcx.def_path_hash(*impl_id).to_raw_def_path_hash().0
        });
        for impl_id in implementations {
            if self.tcx.impl_polarity(impl_id) == ty::ImplPolarity::Positive {
                self.request_implementation(impl_id, obligation.relevance, None)?;
            }
        }
        Ok(())
    }

    fn process_scalar_predicate_obligation(
        &mut self,
        obligation: TraitObligation<'tcx>,
        parameter: ty::ParamTy,
    ) -> Result<(), String> {
        if obligation.polarity != ty::PredicatePolarity::Positive
            || obligation.associated.is_some()
            || obligation.arguments.len() != 1
            || self.parameter_owner_def_id(
                obligation.relevance,
                parameter.index,
                ParameterClass::Type,
            )? != obligation.relevance
        {
            return Ok(());
        }

        for (scalar, scalar_ty) in [
            (Scalar::Unit, self.tcx.types.unit),
            (Scalar::Bool, self.tcx.types.bool),
            (Scalar::Char, self.tcx.types.char),
            (Scalar::F64, self.tcx.types.f64),
            (Scalar::I64, self.tcx.types.i64),
        ] {
            let concrete_arguments = self.tcx.mk_args(&[scalar_ty.into()]);
            let concrete = TraitObligation {
                trait_id: obligation.trait_id,
                self_ty: scalar_ty,
                arguments: concrete_arguments,
                polarity: obligation.polarity,
                associated: None,
                param_env: ty::ParamEnv::empty(),
                relevance: obligation.relevance,
            };
            self.process_trait_obligation(concrete, false)?;
            let candidates = self
                .obligation_candidates
                .get(&concrete.key())
                .ok_or_else(|| "completed scalar predicate query is absent".to_owned())?;
            let applies = self
                .tcx
                .infer_ctxt()
                .build(ty::TypingMode::non_body_analysis())
                .type_implements_trait(obligation.trait_id, [scalar_ty], ty::ParamEnv::empty())
                .must_apply_modulo_regions();
            let mut direct = None;
            let mut ambiguous = false;
            if applies {
                for impl_id in candidates.iter().copied() {
                    let Some(reference) = self
                        .tcx
                        .impl_opt_trait_ref(impl_id)
                        .map(|reference| reference.instantiate_identity().skip_norm_wip())
                    else {
                        continue;
                    };
                    if reference.def_id != obligation.trait_id
                        || reference.args.len() != 1
                        || reference.self_ty() != scalar_ty
                        || self.tcx.impl_polarity(impl_id) != ty::ImplPolarity::Positive
                        || self.tcx.generics_of(impl_id).count() != 0
                        || !self.tcx.predicates_of(impl_id).predicates.is_empty()
                    {
                        continue;
                    }
                    if direct.replace(impl_id).is_some() {
                        ambiguous = true;
                    }
                }
            }
            let outcome = match (applies, direct, ambiguous) {
                (false, _, _) => ScalarPredicateOutcome::Absent,
                (true, Some(implementation), false) => ScalarPredicateOutcome::Positive {
                    implementation: self.id(implementation, Namespace::Type)?,
                },
                (true, _, _) => ScalarPredicateOutcome::Unresolved,
            };
            let key = ScalarPredicateKey {
                owner_hash: self
                    .tcx
                    .def_path_hash(obligation.relevance)
                    .to_raw_def_path_hash()
                    .0,
                parameter: parameter.index,
                trait_hash: self
                    .tcx
                    .def_path_hash(obligation.trait_id)
                    .to_raw_def_path_hash()
                    .0,
                scalar,
            };
            let proof = ScalarPredicateProof {
                owner: self.id(
                    obligation.relevance,
                    definition_namespace(self.tcx.def_kind(obligation.relevance)),
                )?,
                parameter: parameter.index,
                trait_id: self.id(obligation.trait_id, Namespace::Type)?,
                scalar,
                outcome,
            };
            match self.scalar_predicate_proofs.get(&key) {
                Some(existing) if existing != &proof => {
                    return Err("scalar predicate proof identity collision".to_owned());
                }
                Some(_) => {}
                None => {
                    self.budget.items(1)?;
                    self.scalar_predicate_proofs.insert(key, proof);
                }
            }
        }
        Ok(())
    }

    fn request_implementation(
        &mut self,
        impl_id: DefId,
        relevance: DefId,
        associated: Option<AssociatedObligation<'tcx>>,
    ) -> Result<(), String> {
        let key = self.tcx.def_path_hash(impl_id).to_raw_def_path_hash().0;
        let associated = associated
            .map(|obligation| (self.associated_obligation_order_key(obligation), obligation));
        match self.implementation_requests.get_mut(&key) {
            Some(request) => {
                if request.impl_id != impl_id {
                    return Err("stable implementation traversal identity collision".to_owned());
                }
                if !request.relevance.contains(&relevance) {
                    self.budget.items(1)?;
                    request.relevance.try_reserve(1).map_err(|error| {
                        format!("implementation request allocation failed: {error}")
                    })?;
                    request.relevance.push(relevance);
                }
                if let Some((associated_key, obligation)) = associated {
                    match request.associated.get(&associated_key) {
                        Some(existing) if existing != &obligation => {
                            return Err(
                                "stable associated obligation identity collision".to_owned()
                            );
                        }
                        Some(_) => {}
                        None => {
                            self.budget.items(1)?;
                            request.associated.insert(associated_key, obligation);
                        }
                    }
                }
            }
            None => {
                self.budget
                    .items(3_usize.saturating_add(usize::from(associated.is_some())))?;
                let mut relevance_set = Vec::new();
                relevance_set.try_reserve_exact(1).map_err(|error| {
                    format!("implementation request allocation failed: {error}")
                })?;
                relevance_set.push(relevance);
                let mut associated_set = BTreeMap::new();
                if let Some((associated_key, obligation)) = associated {
                    associated_set.insert(associated_key, obligation);
                }
                self.implementation_requests.insert(
                    key,
                    ImplementationRequest {
                        impl_id,
                        relevance: relevance_set,
                        associated: associated_set,
                    },
                );
            }
        }
        Ok(())
    }

    fn associated_obligation_order_key(
        &self,
        obligation: AssociatedObligation<'tcx>,
    ) -> AssociatedObligationOrderKey {
        let mut substitution = DefaultHasher::new();
        obligation.arguments.hash(&mut substitution);
        let mut param_env = DefaultHasher::new();
        obligation.param_env.hash(&mut param_env);
        AssociatedObligationOrderKey {
            kind: obligation.kind,
            trait_item_hash: obligation
                .trait_item
                .map(|item| self.tcx.def_path_hash(item).to_raw_def_path_hash().0)
                .unwrap_or_default(),
            substitution_hash: substitution.finish(),
            param_env_hash: param_env.finish(),
        }
    }

    fn expand_selected_surface(&mut self, def_id: DefId) -> Result<(), String> {
        if self.expanded_surfaces.contains(&def_id) {
            return Ok(());
        }
        self.budget.items(1)?;
        self.expanded_surfaces
            .try_reserve(1)
            .map_err(|error| format!("selected surface allocation failed: {error}"))?;
        self.expanded_surfaces.insert(def_id);
        self.breadcrumb(
            "begin",
            "selected-surface",
            "selected-surface",
            def_id,
            definition_namespace(self.tcx.def_kind(def_id)),
            "definition",
        )?;
        let kind = self.tcx.def_kind(def_id);
        let associated_default =
            kind == DefKind::AssocTy && self.tcx.defaultness(def_id).has_value();
        match selected_surface_work(kind, associated_default) {
            SelectedSurfaceWork::StructLike => {
                for field in &self.tcx.adt_def(def_id).non_enum_variant().fields {
                    self.edge(1)?;
                    if field.vis.is_public() {
                        let ty = self
                            .tcx
                            .type_of(field.did)
                            .instantiate_identity()
                            .skip_norm_wip();
                        self.expand_selected_type(ty)?;
                    }
                }
                Ok(())
            }
            SelectedSurfaceWork::Enum => {
                for variant in self.tcx.adt_def(def_id).variants() {
                    self.edge(variant.fields.len())?;
                    for field in &variant.fields {
                        if field.vis.is_public() {
                            let ty = self
                                .tcx
                                .type_of(field.did)
                                .instantiate_identity()
                                .skip_norm_wip();
                            self.expand_selected_type(ty)?;
                        }
                    }
                }
                Ok(())
            }
            SelectedSurfaceWork::Typed => {
                let ty = self
                    .tcx
                    .type_of(def_id)
                    .instantiate_identity()
                    .skip_norm_wip();
                self.expand_selected_type(ty)
            }
            SelectedSurfaceWork::Function => {
                let signature = self
                    .tcx
                    .fn_sig(def_id)
                    .instantiate_identity()
                    .skip_norm_wip()
                    .skip_binder();
                for ty in signature
                    .inputs()
                    .iter()
                    .copied()
                    .chain(std::iter::once(signature.output()))
                {
                    self.expand_selected_type(ty)?;
                }
                Ok(())
            }
            SelectedSurfaceWork::Trait => {
                for item in self.tcx.associated_items(def_id).in_definition_order() {
                    self.edge(1)?;
                    self.ensure_with_demand(
                        item.def_id,
                        namespace(item.namespace()),
                        Demand::SelectedRoot,
                    )?;
                }
                Ok(())
            }
            SelectedSurfaceWork::AssociatedTypeDefault => {
                let ty = self
                    .tcx
                    .type_of(def_id)
                    .instantiate_identity()
                    .skip_norm_wip();
                self.expand_selected_type(ty)
            }
            SelectedSurfaceWork::Noop => Ok(()),
            SelectedSurfaceWork::Reject => Err(format!(
                "selected public surface has unsupported compiler kind `{}`",
                def_kind_text(kind)
            )),
        }?;
        self.breadcrumb(
            "complete",
            "selected-surface",
            "selected-surface",
            def_id,
            definition_namespace(kind),
            "definition",
        )?;
        Ok(())
    }

    fn expand_selected_type(&mut self, ty: Ty<'tcx>) -> Result<(), String> {
        for argument in ty.walk() {
            self.edge(1)?;
            if let Some(inner) = argument.as_type()
                && let ty::Adt(definition, _) = inner.kind()
                && !self.is_sysroot_crate(definition.did().krate)?
            {
                let def_id = definition.did();
                self.instantiated_breadcrumb(
                    "begin",
                    "selected-surface",
                    "selected-type-edge",
                    def_id,
                    Namespace::Type,
                    inner,
                    "definition",
                )?;
                if self.tcx.visibility(def_id).is_public() {
                    self.ensure_with_demand(def_id, Namespace::Type, Demand::SelectedRoot)?;
                } else {
                    self.ensure_with_demand(def_id, Namespace::Type, Demand::ShapeOnly)?;
                }
                self.instantiated_breadcrumb(
                    "complete",
                    "selected-surface",
                    "selected-type-edge",
                    def_id,
                    Namespace::Type,
                    inner,
                    "definition",
                )?;
            }
        }
        Ok(())
    }

    fn ensure(&mut self, def_id: DefId, namespace: Namespace) -> Result<(), String> {
        self.ensure_with_demand(def_id, namespace, Demand::ShapeOnly)
    }

    fn materialize_shape(
        &mut self,
        def_id: DefId,
        namespace: Namespace,
        owner: Owner,
    ) -> Result<(), String> {
        if self.definitions.contains_key(&(def_id, namespace)) {
            return Ok(());
        }
        if def_id.is_local() {
            Err("selected metadata refers to the Rawr probe crate".to_owned())
        } else if self.is_sysroot_crate(def_id.krate)? {
            self.insert_sysroot(def_id, namespace, owner)
        } else {
            self.insert_external(def_id, namespace, owner)
        }
    }

    fn insert_definition(
        &mut self,
        def_id: DefId,
        namespace: Namespace,
        owner: Owner,
    ) -> Result<(), String> {
        self.request_definition(def_id, namespace, owner, Demand::ShapeOnly)
    }

    fn insert_external(
        &mut self,
        def_id: DefId,
        namespace: Namespace,
        owner: Owner,
    ) -> Result<(), String> {
        if matches!(self.tcx.def_kind(def_id), DefKind::Impl { .. }) {
            // An impl is an anonymous coherence identity, not a nameable Rust
            // definition. External metadata is therefore not required to
            // encode visibility for it. The implementation ledger retains the
            // identity; its exact trait, Self-type, and associated-item
            // relevance is materialized by `insert_external_implementation`.
            return Ok(());
        }
        let owner = self.validate_definition_owner(def_id, owner)?;
        self.register_external(def_id.krate)?;
        if self.definitions.contains_key(&(def_id, namespace)) {
            return Ok(());
        }
        if self.definitions.len() >= MAX_DEFINITIONS {
            return Err("compiler definition limit exceeded".to_owned());
        }
        let anonymous_support = match associated_path_kind(self.tcx, def_id)? {
            Some(AssociatedPathKind::AnonymousSupport(support)) => {
                self.validate_anonymous_associated_owner(support, &owner)?;
                Some(support)
            }
            Some(AssociatedPathKind::Named) | None => None,
        };
        self.budget.items(1)?;
        let visibility = self.tcx.visibility(def_id);
        let nameable = visibility.is_public() || matches!(&owner, Owner::Implementation { .. });
        let shape = if nameable {
            self.local_shape(def_id)
        } else {
            Ok(Shape::UnnameablePrivate)
        };
        let definition = Definition {
            id: self.id(def_id, namespace)?,
            display_path: self.definition_path(def_id)?,
            owner,
            visibility: self.visibility(visibility)?,
            shape: shape?,
            span: self.span(self.tcx.def_span(def_id))?,
            origin: match anonymous_support {
                Some(_) => Origin::Synthesized {
                    mechanism: self.budget.owned("return-position-impl-trait-in-trait")?,
                },
                None => self.origin(self.tcx.def_span(def_id))?,
            },
        };
        self.budget.items(1)?;
        self.definitions
            .try_reserve(1)
            .map_err(|error| format!("definition ledger allocation failed: {error}"))?;
        if self
            .definitions
            .insert((def_id, namespace), definition)
            .is_some()
        {
            return Err("duplicate external definition identity".to_owned());
        }
        Ok(())
    }

    fn expand_implementations(&mut self, def_id: DefId) -> Result<(), String> {
        if self.expanded_impls.contains(&def_id) {
            return Ok(());
        }
        self.budget.items(1)?;
        self.expanded_impls
            .try_reserve(1)
            .map_err(|error| format!("expanded implementation allocation failed: {error}"))?;
        self.expanded_impls.insert(def_id);
        let kind = self.tcx.def_kind(def_id);
        self.breadcrumb(
            "begin",
            "implementation",
            "implementations",
            def_id,
            definition_namespace(kind),
            "definition",
        )?;
        match implementation_work(kind) {
            // A selected trait contributes its own associated surface below.
            // Implementations require an exact Self-bearing predicate,
            // projection, or demanded nominal edge; a trait definition alone
            // is not an unconstrained request for every implementation.
            ImplementationWork::Trait => {}
            ImplementationWork::Nominal => {
                let inherent_count = self.tcx.inherent_impls(def_id).len();
                self.edge(inherent_count)?;
                self.budget.items(inherent_count)?;
                let mut inherent = Vec::new();
                inherent
                    .try_reserve_exact(inherent_count)
                    .map_err(|error| {
                        format!("inherent implementation allocation failed: {error}")
                    })?;
                inherent.extend_from_slice(self.tcx.inherent_impls(def_id));
                inherent.sort_unstable_by_key(|impl_id| {
                    self.tcx.def_path_hash(*impl_id).to_raw_def_path_hash().0
                });
                inherent.dedup();
                let nominal_demand = Some(AssociatedObligation {
                    kind: AssociatedDemandKind::NominalSurface,
                    trait_item: None,
                    arguments: None,
                    param_env: self.tcx.param_env(def_id),
                });
                for implementation in inherent {
                    self.request_implementation(implementation, def_id, nominal_demand)?;
                }
                self.ensure_implementation_index(def_id.krate)?;
                let candidates = self
                    .implementation_index
                    .get_mut(&def_id.krate)
                    .and_then(|index| take_owned_candidate_bucket(index, &def_id))
                    .unwrap_or_default();
                for implementation in candidates {
                    self.request_implementation(implementation, def_id, nominal_demand)?;
                }
            }
            ImplementationWork::Noop => {}
            ImplementationWork::Reject => {
                return Err(format!(
                    "implementation expansion has unsupported compiler kind `{}`",
                    def_kind_text(kind)
                ));
            }
        }
        self.breadcrumb(
            "complete",
            "implementation",
            "implementations",
            def_id,
            definition_namespace(kind),
            "definition",
        )?;
        Ok(())
    }

    fn ensure_implementation_index(&mut self, crate_num: CrateNum) -> Result<(), String> {
        if self.implementation_index.contains_key(&crate_num) {
            return Ok(());
        }
        let mut index: HashMap<DefId, Vec<DefId>> = HashMap::new();
        for impl_id in self.tcx.trait_impls_in_crate(crate_num) {
            let self_type = self
                .tcx
                .type_of(*impl_id)
                .instantiate_identity()
                .skip_norm_wip();
            let trait_ref = self
                .tcx
                .impl_opt_trait_ref(*impl_id)
                .map(|reference| reference.instantiate_identity().skip_norm_wip());
            for argument in self_type.walk().chain(
                trait_ref
                    .into_iter()
                    .flat_map(|reference| reference.args.iter()),
            ) {
                self.edge(1)?;
                if let Some(inner) = argument.as_type()
                    && let ty::Adt(definition, _) = inner.kind()
                {
                    let nominal = definition.did();
                    if !index.contains_key(&nominal) {
                        self.budget.items(1)?;
                        index.try_reserve(1).map_err(|error| {
                            format!("implementation index allocation failed: {error}")
                        })?;
                    }
                    let candidates = index.entry(nominal).or_default();
                    if candidates.last() != Some(impl_id) {
                        self.budget.items(1)?;
                        candidates.try_reserve(1).map_err(|error| {
                            format!("implementation index allocation failed: {error}")
                        })?;
                        candidates.push(*impl_id);
                    }
                }
            }
        }
        for candidates in index.values_mut() {
            candidates.sort_unstable_by_key(|impl_id| {
                self.tcx.def_path_hash(*impl_id).to_raw_def_path_hash().0
            });
            candidates.dedup();
        }
        self.budget.items(1)?;
        self.implementation_index
            .try_reserve(1)
            .map_err(|error| format!("implementation index allocation failed: {error}"))?;
        self.implementation_index.insert(crate_num, index);
        Ok(())
    }

    fn insert_external_implementation(
        &mut self,
        request: ImplementationRequest<'tcx>,
    ) -> Result<(), String> {
        let ImplementationRequest {
            impl_id,
            relevance: mut requested_relevance,
            associated: requested_associated,
        } = request;
        requested_relevance
            .sort_unstable_by_key(|owner| self.tcx.def_path_hash(*owner).to_raw_def_path_hash().0);
        requested_relevance.dedup();
        match implementation_crate_authority(self.is_sysroot_crate(impl_id.krate)?) {
            ImplementationCrateAuthority::Sysroot => self.register_sysroot(impl_id.krate)?,
            ImplementationCrateAuthority::External => self.register_external(impl_id.krate)?,
        }
        let key = self.def_hash(impl_id)?;
        let implementation_id = self.id(impl_id, Namespace::Type)?;
        let compiler_self_ty = self
            .tcx
            .type_of(impl_id)
            .instantiate_identity()
            .skip_norm_wip();
        let compiler_trait_ref = self
            .tcx
            .impl_opt_trait_ref(impl_id)
            .map(|reference| reference.instantiate_identity().skip_norm_wip());
        let mut relevance = BTreeSet::new();
        for owner_id in requested_relevance {
            self.insert_implementation_relevance_owner(owner_id, &mut relevance)?;
        }
        for obligation in requested_associated.values() {
            if let Some(trait_item) = obligation.trait_item {
                self.insert_implementation_relevance_definition(trait_item, &mut relevance)?;
            }
        }
        if let Some(mut existing) = self.implementations.remove(&impl_id) {
            let associated = self.expand_associated_obligations(
                impl_id,
                compiler_trait_ref,
                &requested_associated,
                &implementation_id,
                false,
            )?;
            if existing.relevance.len().saturating_add(relevance.len()) > MAX_EDGES {
                return Err("implementation relevance edge limit exceeded".to_owned());
            }
            self.budget.items(relevance.len())?;
            existing
                .relevance
                .try_reserve(relevance.len())
                .map_err(|error| format!("implementation relevance allocation failed: {error}"))?;
            existing.relevance.extend(relevance);
            existing.relevance.sort_unstable();
            existing.relevance.dedup();
            if existing.associated.len().saturating_add(associated.len()) > MAX_EDGES {
                return Err("implementation associated edge limit exceeded".to_owned());
            }
            self.budget.items(associated.len())?;
            existing
                .associated
                .try_reserve(associated.len())
                .map_err(|error| format!("implementation associated allocation failed: {error}"))?;
            existing.associated.extend(associated);
            existing.associated.sort_unstable();
            existing.associated.dedup();
            if self.implementations.insert(impl_id, existing).is_some() {
                return Err("implementation ledger identity collision".to_owned());
            }
            return Ok(());
        }
        self.breadcrumb(
            "begin",
            "implementation",
            "implementation-body",
            impl_id,
            Namespace::Type,
            "implementation",
        )?;
        for argument in compiler_self_ty.walk() {
            if let Some(inner) = argument.as_type()
                && let ty::Adt(definition, _) = inner.kind()
            {
                self.edge(1)?;
                self.ensure(definition.did(), Namespace::Type)?;
                self.budget.items(1)?;
                relevance.insert(self.id(definition.did(), Namespace::Type)?);
            }
        }
        let self_type = self.type_of(compiler_self_ty, impl_id, 0)?;
        let trait_ref = match compiler_trait_ref {
            None => Trait::Inherent,
            Some(reference) => {
                let trait_id: DefId = reference.def_id;
                self.edge(1)?;
                self.ensure(trait_id, Namespace::Type)?;
                self.budget.items(1)?;
                relevance.insert(self.id(trait_id, Namespace::Type)?);
                Trait::Trait {
                    id: self.id(trait_id, Namespace::Type)?,
                    arguments: self.arguments(reference.args, impl_id, 0)?,
                }
            }
        };
        let associated = self.expand_associated_obligations(
            impl_id,
            compiler_trait_ref,
            &requested_associated,
            &implementation_id,
            false,
        )?;
        let (impl_safety, polarity, const_impl) = match compiler_trait_ref {
            Some(_) => {
                let header = self.tcx.impl_trait_header(impl_id);
                let polarity = match header.polarity {
                    ty::ImplPolarity::Positive => Polarity::Positive,
                    ty::ImplPolarity::Negative => Polarity::Negative,
                    ty::ImplPolarity::Reservation => {
                        self.breadcrumb(
                            "complete",
                            "implementation",
                            "implementation-body",
                            impl_id,
                            Namespace::Type,
                            "implementation",
                        )?;
                        return Ok(());
                    }
                };
                (
                    safety(header.safety),
                    polarity,
                    matches!(header.constness, hir::Constness::Const { .. }),
                )
            }
            None => (Safety::Safe, Polarity::Positive, false),
        };
        let generics = self.generics(impl_id)?;
        let span = self.span(self.tcx.def_span(impl_id))?;
        let origin = self.origin(self.tcx.def_span(impl_id))?;
        let implementation = Implementation {
            key,
            id: implementation_id,
            relevance: {
                let mut values = Vec::new();
                self.budget.items(relevance.len())?;
                values.try_reserve_exact(relevance.len()).map_err(|error| {
                    format!("implementation relevance allocation failed: {error}")
                })?;
                values.extend(relevance);
                values
            },
            trait_ref,
            self_type,
            safety: impl_safety,
            polarity,
            const_impl,
            default_impl: self.tcx.defaultness(impl_id).is_default(),
            generics,
            associated,
            span,
            origin,
        };
        self.budget.items(1)?;
        self.implementations
            .try_reserve(1)
            .map_err(|error| format!("implementation ledger allocation failed: {error}"))?;
        if self
            .implementations
            .insert(impl_id, implementation)
            .is_some()
        {
            return Err("duplicate external implementation identity".to_owned());
        }
        self.breadcrumb(
            "complete",
            "implementation",
            "implementation-body",
            impl_id,
            Namespace::Type,
            "implementation",
        )?;
        Ok(())
    }

    fn expand_associated_obligations(
        &mut self,
        impl_id: DefId,
        compiler_trait_ref: Option<ty::TraitRef<'tcx>>,
        obligations: &BTreeMap<AssociatedObligationOrderKey, AssociatedObligation<'tcx>>,
        implementation_id: &Id,
        application: bool,
    ) -> Result<Vec<Id>, String> {
        let mut selected = BTreeMap::new();
        match compiler_trait_ref {
            None => {
                if obligations
                    .values()
                    .any(|obligation| obligation.kind == AssociatedDemandKind::SelectedMember)
                {
                    return Err(
                        "trait associated obligation resolved to an inherent implementation"
                            .to_owned(),
                    );
                }
                if obligations
                    .values()
                    .any(|obligation| obligation.kind == AssociatedDemandKind::NominalSurface)
                {
                    for item in self.tcx.associated_items(impl_id).in_definition_order() {
                        if self.tcx.visibility(item.def_id).is_public() {
                            self.select_associated_definition(item.def_id, &mut selected)?;
                        }
                    }
                }
            }
            Some(reference) => {
                let trait_id: DefId = reference.def_id;
                let implementors = self.tcx.impl_item_implementor_ids(impl_id);
                for obligation in obligations.values() {
                    match obligation.kind {
                        // A demanded nominal admits a trait implementation header as
                        // applicability evidence. It does not make every trait member
                        // a selected source surface. A Rust future's `Output` is one
                        // exception: Rawr types the future as `Task<Output>`. An
                        // iterator's `Item` and `next` are the other: a `for` loop
                        // walks the type by its `next` (D558 slice 2).
                        AssociatedDemandKind::NominalSurface => {
                            if Some(trait_id) == self.tcx.lang_items().future_trait()
                                && let Some(output) = self.tcx.lang_items().future_output()
                            {
                                self.select_trait_item_for_impl(
                                    impl_id,
                                    trait_id,
                                    output,
                                    implementors,
                                    &mut selected,
                                )?;
                            }
                            let iterator = Some(trait_id) == self.tcx.lang_items().iterator_trait();
                            // `AsRef::as_ref`, as `e.name().as_ref()` calls it.
                            let as_ref = Some(trait_id)
                                == self.tcx.get_diagnostic_item(rustc_span::sym::AsRef);
                            if iterator || as_ref {
                                let members = self
                                    .tcx
                                    .associated_items(trait_id)
                                    .in_definition_order()
                                    .filter(|item| {
                                        matches!(item.name().as_str(), "Item" | "next" | "as_ref")
                                    })
                                    .map(|item| item.def_id)
                                    .collect::<Vec<_>>();
                                self.edge(members.len())?;
                                for member in members {
                                    self.select_trait_item_for_impl(
                                        impl_id,
                                        trait_id,
                                        member,
                                        implementors,
                                        &mut selected,
                                    )?;
                                }
                            }
                        }
                        AssociatedDemandKind::SelectedMember => {
                            let trait_item = obligation.trait_item.ok_or_else(|| {
                                "selected associated obligation lost its trait item".to_owned()
                            })?;
                            if obligation.arguments.is_none() {
                                return Err(
                                    "selected associated obligation lost substitutions".to_owned()
                                );
                            }
                            self.select_trait_item_for_impl(
                                impl_id,
                                trait_id,
                                trait_item,
                                implementors,
                                &mut selected,
                            )?;
                        }
                    }
                }
            }
        }

        let mut associated = Vec::new();
        self.budget.items(selected.len())?;
        associated
            .try_reserve_exact(selected.len())
            .map_err(|error| format!("associated item allocation failed: {error}"))?;
        for definition_id in selected.into_values() {
            let mut definition_id = definition_id;
            let mut item = self.tcx.associated_item(definition_id);
            if application && item.container_id(self.tcx) == impl_id {
                // A blanket implementation's own member (`ToString::to_string`
                // in `impl<T: Display> ToString for T`) is one definition that
                // every type it applies to shares. The application names the
                // trait's declaration instead, which dispatches to it.
                definition_id = item.trait_item_def_id().ok_or_else(|| {
                    "blanket application member has no trait declaration".to_owned()
                })?;
                item = self.tcx.associated_item(definition_id);
            }
            let item_namespace = namespace(item.namespace());
            let parent = item.container_id(self.tcx);
            let owner = if parent == impl_id {
                self.charge_id(implementation_id)?;
                Owner::Implementation {
                    id: implementation_id.clone(),
                }
            } else if self.tcx.def_kind(parent) == DefKind::Trait {
                Owner::Definition {
                    id: self.id(parent, Namespace::Type)?,
                }
            } else {
                return Err("selected associated item has an invalid compiler owner".to_owned());
            };
            self.insert_definition(definition_id, item_namespace, owner)?;
            self.breadcrumb(
                "begin",
                "implementation",
                "associated-obligation",
                definition_id,
                item_namespace,
                "definition+namespace",
            )?;
            self.ensure_with_demand(definition_id, item_namespace, Demand::SelectedRoot)?;
            self.breadcrumb(
                "complete",
                "implementation",
                "associated-obligation",
                definition_id,
                item_namespace,
                "definition+namespace",
            )?;
            associated.push(self.id(definition_id, item_namespace)?);
        }
        associated.sort_unstable();
        associated.dedup();
        Ok(associated)
    }

    fn select_trait_item_for_impl(
        &mut self,
        impl_id: DefId,
        trait_id: DefId,
        trait_item: DefId,
        implementors: &rustc_hir::def_id::DefIdMap<DefId>,
        selected: &mut BTreeMap<[u8; 16], DefId>,
    ) -> Result<(), String> {
        let item = self.tcx.associated_item(trait_item);
        if item.container_id(self.tcx) != trait_id {
            return Err("associated obligation item does not belong to its trait".to_owned());
        }
        self.select_effective_trait_item(trait_item, implementors, selected)?;
        if item.is_fn() {
            let linked = self
                .tcx
                .associated_types_for_impl_traits_in_trait_or_impl(trait_id)
                .get(&trait_item)
                .map(Vec::as_slice)
                .unwrap_or_default();
            self.edge(linked.len())?;
            for linked_item in linked {
                self.select_effective_trait_item(*linked_item, implementors, selected)?;
            }
        }
        if !matches!(self.tcx.def_kind(impl_id), DefKind::Impl { of_trait: true }) {
            return Err("associated obligation target is not a trait implementation".to_owned());
        }
        Ok(())
    }

    fn select_effective_trait_item(
        &mut self,
        trait_item: DefId,
        implementors: &rustc_hir::def_id::DefIdMap<DefId>,
        selected: &mut BTreeMap<[u8; 16], DefId>,
    ) -> Result<(), String> {
        let effective = effective_associated_item(
            trait_item,
            implementors.get(&trait_item).copied(),
            self.tcx.defaultness(trait_item).has_value(),
        )?;
        self.select_associated_definition(effective, selected)
    }

    fn select_associated_definition(
        &mut self,
        definition_id: DefId,
        selected: &mut BTreeMap<[u8; 16], DefId>,
    ) -> Result<(), String> {
        self.edge(1)?;
        let key = self
            .tcx
            .def_path_hash(definition_id)
            .to_raw_def_path_hash()
            .0;
        match selected.get(&key) {
            Some(existing) if *existing != definition_id => {
                Err("stable associated definition identity collision".to_owned())
            }
            Some(_) => Ok(()),
            None => {
                self.budget.items(1)?;
                selected.insert(key, definition_id);
                Ok(())
            }
        }
    }

    fn insert_implementation_relevance_owner(
        &mut self,
        owner_id: DefId,
        relevance: &mut BTreeSet<Id>,
    ) -> Result<(), String> {
        if !matches!(self.tcx.def_kind(owner_id), DefKind::Impl { .. }) {
            return self.insert_implementation_relevance_definition(owner_id, relevance);
        }

        let self_ty = self
            .tcx
            .type_of(owner_id)
            .instantiate_identity()
            .skip_norm_wip();
        for argument in self_ty.walk() {
            if let Some(inner) = argument.as_type()
                && let ty::Adt(definition, _) = inner.kind()
            {
                self.insert_implementation_relevance_definition(definition.did(), relevance)?;
            }
        }
        if let Some(reference) = self
            .tcx
            .impl_opt_trait_ref(owner_id)
            .map(|reference| reference.instantiate_identity().skip_norm_wip())
        {
            self.insert_implementation_relevance_definition(reference.def_id, relevance)?;
        }
        Ok(())
    }

    fn insert_implementation_relevance_definition(
        &mut self,
        def_id: DefId,
        relevance: &mut BTreeSet<Id>,
    ) -> Result<(), String> {
        if matches!(self.tcx.def_kind(def_id), DefKind::Impl { .. }) {
            return Err("implementation relevance did not resolve to nameable evidence".to_owned());
        }
        let namespace = definition_namespace(self.tcx.def_kind(def_id));
        self.edge(1)?;
        self.ensure(def_id, namespace)?;
        self.budget.items(1)?;
        relevance.insert(self.id(def_id, namespace)?);
        Ok(())
    }

    fn register_external(&mut self, crate_num: CrateNum) -> Result<(), String> {
        if self.external.contains_key(&crate_num) {
            return Ok(());
        }
        if self.external.len() >= MAX_PACKAGES {
            return Err("external compiler crate limit exceeded".to_owned());
        }
        let mut artifacts = Vec::new();
        for path in self.tcx.used_crate_source(crate_num).paths() {
            self.edge(1)?;
            let path = canonical_path(path, "external artifact", self.budget)?;
            let text = path
                .to_str()
                .ok_or_else(|| "external artifact path is not UTF-8".to_owned())?;
            self.budget.items(1)?;
            artifacts
                .try_reserve(1)
                .map_err(|error| format!("external artifact allocation failed: {error}"))?;
            artifacts.push(self.budget.owned(text)?);
        }
        artifacts.sort_unstable();
        artifacts.dedup();
        if artifacts.is_empty() {
            return Err("external compiler crate has no artifact evidence".to_owned());
        }
        let compiler_crate_name = self.tcx.crate_name(crate_num);
        let crate_name = compiler_crate_name.as_str();
        let stable_crate_id = self
            .budget
            .formatted(format_args!("{:016x}", self.tcx.stable_crate_id(crate_num)))?;
        self.budget.items(1)?;
        self.external.insert(
            crate_num,
            ExternalCrate {
                crate_name: self.budget.owned(crate_name)?,
                stable_crate_id,
                artifacts,
            },
        );
        Ok(())
    }

    fn insert_sysroot(
        &mut self,
        def_id: DefId,
        namespace: Namespace,
        owner: Owner,
    ) -> Result<(), String> {
        let owner = self.validate_definition_owner(def_id, owner)?;
        if self.definitions.contains_key(&(def_id, namespace)) {
            return Ok(());
        }
        if self.definitions.len() >= MAX_DEFINITIONS {
            return Err("compiler definition limit exceeded".to_owned());
        }
        self.register_sysroot(def_id.krate)?;
        let anonymous_support = match associated_path_kind(self.tcx, def_id)? {
            Some(AssociatedPathKind::AnonymousSupport(support)) => {
                self.validate_anonymous_associated_owner(support, &owner)?;
                Some(support)
            }
            Some(AssociatedPathKind::Named) | None => None,
        };
        let definition = Definition {
            id: self.id(def_id, namespace)?,
            display_path: self.definition_path(def_id)?,
            owner,
            visibility: Visibility::Public,
            shape: Shape::Sysroot {
                compiler_kind: self
                    .budget
                    .owned(def_kind_text(self.tcx.def_kind(def_id)))?,
            },
            span: self.span(self.tcx.def_span(def_id))?,
            origin: match anonymous_support {
                Some(_) => Origin::Synthesized {
                    mechanism: self.budget.owned("return-position-impl-trait-in-trait")?,
                },
                None => self.origin(self.tcx.def_span(def_id))?,
            },
        };
        self.budget.items(1)?;
        self.definitions
            .try_reserve(1)
            .map_err(|error| format!("sysroot definition allocation failed: {error}"))?;
        self.definitions.insert((def_id, namespace), definition);
        Ok(())
    }

    /// A `macro_rules!` macro's definition, read from its crate's source
    /// from its definition span to the close of its body: rawr expands it
    /// as rustc does. `None` for a procedural or `macro` 2.0 macro, or when
    /// the source is not on disk.
    fn macro_rules_shape(&mut self, def_id: DefId) -> Result<Option<Shape>, String> {
        let source_map = self.tcx.sess.source_map();
        let span = self.tcx.def_span(def_id);
        let file = source_map.lookup_source_file(span.lo());
        if !source_map.ensure_source_file_source_present(&file) {
            return Ok(None);
        }
        let text = match (file.src.as_deref(), file.external_src.read().get_source()) {
            (Some(text), _) => text.to_owned(),
            (None, Some(text)) => text.to_owned(),
            (None, None) => return Ok(None),
        };
        let start = file.original_relative_byte_pos(span.lo()).0 as usize;
        let Some(source) = text.get(start..).and_then(macro_rules_source) else {
            return Ok(None);
        };
        Ok(Some(Shape::MacroRules {
            source: self.budget.owned(source)?,
            krate: self.budget.owned(&crate_segment(self.tcx, def_id.krate))?,
        }))
    }

    fn compiler_definition_owner(&mut self, def_id: DefId) -> Result<Owner, String> {
        if !is_associated_definition_kind(self.tcx.def_kind(def_id)) {
            return Ok(Owner::Free);
        }
        let parent = self.tcx.associated_item(def_id).container_id(self.tcx);
        match self.tcx.def_kind(parent) {
            DefKind::Trait => Ok(Owner::Definition {
                id: self.id(parent, Namespace::Type)?,
            }),
            DefKind::Impl { .. } => Ok(Owner::Implementation {
                id: self.id(parent, Namespace::Type)?,
            }),
            _ => Err("associated definition has an unsupported compiler container".to_owned()),
        }
    }

    fn validate_definition_owner(&mut self, def_id: DefId, owner: Owner) -> Result<Owner, String> {
        let expected = self.compiler_definition_owner(def_id)?;
        if owner == expected {
            Ok(owner)
        } else {
            Err("definition owner does not match its exact compiler container".to_owned())
        }
    }

    fn local_shape(&mut self, def_id: DefId) -> Result<Shape, String> {
        let kind = self.tcx.def_kind(def_id);
        let namespace = definition_namespace(kind);
        self.breadcrumb(
            "begin",
            "definition-shape",
            "local-shape",
            def_id,
            namespace,
            "definition+namespace",
        )?;
        let shape = match kind {
            DefKind::Mod => Shape::Module,
            DefKind::Struct => {
                let fields = self.fields(def_id, self.tcx.adt_def(def_id).non_enum_variant())?;
                Shape::Struct {
                    field_form: fields.form,
                    fields: fields.public,
                    fields_complete: fields.complete,
                    non_exhaustive: fields.non_exhaustive,
                    generics: self.generics(def_id)?,
                }
            }
            DefKind::Union => {
                let fields = self.fields(def_id, self.tcx.adt_def(def_id).non_enum_variant())?;
                Shape::Union {
                    field_form: fields.form,
                    fields: fields.public,
                    fields_complete: fields.complete,
                    non_exhaustive: fields.non_exhaustive,
                    generics: self.generics(def_id)?,
                }
            }
            DefKind::Enum => {
                let mut variants = Vec::new();
                self.edge(self.tcx.adt_def(def_id).variants().len())?;
                for variant in self.tcx.adt_def(def_id).variants() {
                    let name = variant.name.as_str();
                    self.budget.items(1)?;
                    variants
                        .try_reserve(1)
                        .map_err(|error| format!("variant allocation failed: {error}"))?;
                    let fields = self.fields(def_id, variant)?;
                    variants.push(Variant {
                        name: self.budget.owned(name)?,
                        field_form: fields.form,
                        fields: fields.public,
                        fields_complete: fields.complete,
                        non_exhaustive: fields.non_exhaustive,
                        discriminant: match variant.discr {
                            ty::VariantDiscr::Explicit(discriminant) => Explicit::Present {
                                value: self.constant(discriminant)?,
                            },
                            ty::VariantDiscr::Relative(_) => Explicit::Absent,
                        },
                        span: self.span(self.tcx.def_span(variant.def_id))?,
                    });
                }
                Shape::Enum {
                    variants,
                    generics: self.generics(def_id)?,
                }
            }
            DefKind::Fn | DefKind::AssocFn => Shape::Function {
                signature: self.signature(def_id)?,
                generics: self.generics(def_id)?,
            },
            DefKind::Const {
                is_type_const: true,
            }
            | DefKind::AssocConst {
                is_type_const: true,
            } => self.type_constant_shape(def_id)?,
            DefKind::Const {
                is_type_const: false,
            } => Shape::Const {
                ty: self.type_of(
                    self.tcx
                        .type_of(def_id)
                        .instantiate_identity()
                        .skip_norm_wip(),
                    def_id,
                    0,
                )?,
                value: Explicit::Present {
                    value: self.constant(def_id)?,
                },
            },
            DefKind::AssocConst {
                is_type_const: false,
            } => Shape::Const {
                ty: self.type_of(
                    self.tcx
                        .type_of(def_id)
                        .instantiate_identity()
                        .skip_norm_wip(),
                    def_id,
                    0,
                )?,
                value: if self.tcx.defaultness(def_id).has_value() {
                    Explicit::Present {
                        value: self.constant(def_id)?,
                    }
                } else {
                    Explicit::Absent
                },
            },
            DefKind::Static { mutability, .. } => Shape::Static {
                ty: self.type_of(
                    self.tcx
                        .type_of(def_id)
                        .instantiate_identity()
                        .skip_norm_wip(),
                    def_id,
                    0,
                )?,
                mutable: mutability.is_mut(),
                thread_local: self.tcx.is_thread_local_static(def_id),
            },
            DefKind::TyAlias => Shape::TypeAlias {
                ty: self.type_of(
                    self.tcx
                        .type_of(def_id)
                        .instantiate_identity()
                        .skip_norm_wip(),
                    def_id,
                    0,
                )?,
                generics: self.generics(def_id)?,
            },
            DefKind::Trait => self.trait_shape(def_id)?,
            DefKind::AssocTy => self.associated_type_shape(def_id)?,
            DefKind::Macro(..) if let Some(shape) = self.macro_rules_shape(def_id)? => shape,
            kind => Shape::Unknown {
                compiler_kind: self.budget.owned(def_kind_text(kind))?,
            },
        };
        self.breadcrumb(
            "complete",
            "definition-shape",
            "local-shape",
            def_id,
            namespace,
            "definition+namespace",
        )?;
        Ok(shape)
    }

    fn type_constant_shape(&mut self, def_id: DefId) -> Result<Shape, String> {
        if !self.tcx.is_type_const(def_id) {
            return Err("type-constant kind disagrees with compiler metadata".to_owned());
        }
        let has_value = match self.tcx.def_kind(def_id) {
            DefKind::Const {
                is_type_const: true,
            } => true,
            DefKind::AssocConst {
                is_type_const: true,
            } => {
                let item = self.tcx.associated_item(def_id);
                let has_value = self.tcx.defaultness(def_id).has_value();
                if !has_value && !matches!(item.container, ty::AssocContainer::Trait) {
                    return Err("implementation-owned type constant has no compiler RHS".to_owned());
                }
                has_value
            }
            _ => return Err("type-constant shape has a non-type-constant DefKind".to_owned()),
        };
        let ty = self
            .tcx
            .type_of(def_id)
            .instantiate_identity()
            .skip_norm_wip();
        let rhs = if has_value {
            Explicit::Present {
                value: self.ty_constant(
                    self.tcx
                        .const_of_item(def_id)
                        .instantiate_identity()
                        .skip_norm_wip(),
                    def_id,
                )?,
            }
        } else {
            Explicit::Absent
        };
        Ok(Shape::TypeConst {
            ty: self.type_of(ty, def_id, 0)?,
            rhs,
        })
    }

    fn trait_shape(&mut self, def_id: DefId) -> Result<Shape, String> {
        let trait_definition = self.tcx.trait_def(def_id);
        let owner = self.id(def_id, Namespace::Type)?;
        let tcx = self.tcx;
        let associated_count = tcx.associated_items(def_id).in_definition_order().count();
        self.edge(associated_count)?;
        let mut associated = Vec::new();
        for item in tcx.associated_items(def_id).in_definition_order() {
            let associated_id = item.def_id;
            let associated_namespace = namespace(item.namespace());
            self.charge_id(&owner)?;
            self.insert_definition(
                associated_id,
                associated_namespace,
                Owner::Definition { id: owner.clone() },
            )?;
            self.budget.items(1)?;
            associated
                .try_reserve(1)
                .map_err(|error| format!("trait associated allocation failed: {error}"))?;
            associated.push(self.id(associated_id, associated_namespace)?);
        }
        let predicates = tcx.explicit_super_predicates_of(def_id).skip_binder();
        self.edge(predicates.len())?;
        let supertraits =
            self.predicates_from_clauses(def_id, predicates.iter().map(|(clause, _)| *clause))?;
        Ok(Shape::Trait {
            unsafe_trait: matches!(trait_definition.safety, hir::Safety::Unsafe),
            auto_trait: self.tcx.trait_is_auto(def_id),
            supertraits,
            associated,
            generics: self.generics(def_id)?,
        })
    }

    fn associated_type_shape(&mut self, def_id: DefId) -> Result<Shape, String> {
        let bounds_owner = self
            .tcx
            .associated_item(def_id)
            .trait_item_def_id()
            .unwrap_or(def_id);
        let default = match self.tcx.defaultness(def_id) {
            hir::Defaultness::Default { has_value: true } | hir::Defaultness::Final => {
                Explicit::Present {
                    value: self.type_of(
                        self.tcx
                            .type_of(def_id)
                            .instantiate_identity()
                            .skip_norm_wip(),
                        def_id,
                        0,
                    )?,
                }
            }
            hir::Defaultness::Default { has_value: false } => Explicit::Absent,
        };
        Ok(Shape::AssociatedType {
            bounds: self.item_bounds(bounds_owner, bounds_owner)?,
            default,
            generics: self.generics(def_id)?,
        })
    }

    fn fields(&mut self, owner: DefId, variant: &ty::VariantDef) -> Result<Fields, String> {
        self.edge(variant.fields.len())?;
        let complete = variant.fields.iter().all(|field| field.vis.is_public());
        let mut fields = Vec::new();
        for (index, field) in variant.fields.iter().enumerate() {
            if !field.vis.is_public() {
                continue;
            }
            let name = field.name.as_str();
            self.budget.items(1)?;
            fields
                .try_reserve(1)
                .map_err(|error| format!("field allocation failed: {error}"))?;
            fields.push(Field {
                name: self.budget.owned(name)?,
                index: u32::try_from(index)
                    .map_err(|_| "compiler field index exceeds u32".to_owned())?,
                visibility: self.visibility(field.vis)?,
                ty: self.type_of(
                    self.tcx
                        .type_of(field.did)
                        .instantiate_identity()
                        .skip_norm_wip(),
                    owner,
                    0,
                )?,
                span: self.span(self.tcx.def_span(field.did))?,
            });
        }
        Ok(Fields {
            form: match variant.ctor_kind() {
                None => FieldForm::Record,
                Some(CtorKind::Fn) => FieldForm::Tuple,
                Some(CtorKind::Const) => FieldForm::Unit,
            },
            public: fields,
            complete,
            non_exhaustive: variant.is_field_list_non_exhaustive(),
        })
    }

    fn signature(&mut self, def_id: DefId) -> Result<Signature, String> {
        let signature = self
            .tcx
            .fn_sig(def_id)
            .instantiate_identity()
            .skip_norm_wip();
        let method = self.tcx.def_kind(def_id) == DefKind::AssocFn
            && self.tcx.associated_item(def_id).is_method();
        self.poly_signature(signature, def_id, method)
    }

    fn poly_signature(
        &mut self,
        poly: ty::PolyFnSig<'tcx>,
        owner: DefId,
        method: bool,
    ) -> Result<Signature, String> {
        let binders = self.binders(owner, poly.bound_vars())?;
        let signature = poly.skip_binder();
        self.edge(signature.inputs().len().saturating_add(1))?;
        let mut inputs = Vec::new();
        for ty in signature.inputs() {
            self.budget.items(1)?;
            inputs
                .try_reserve(1)
                .map_err(|error| format!("signature input allocation failed: {error}"))?;
            inputs.push(self.type_of(*ty, owner, 0)?);
        }
        let receiver = if method {
            if inputs.is_empty() {
                return Err("method signature has no receiver input".to_owned());
            }
            let first = inputs.remove(0);
            match first {
                Type::Reference {
                    lifetime,
                    mutability,
                    ty,
                } => Receiver::Reference {
                    lifetime,
                    mutability,
                    ty: *ty,
                },
                // A trait's `self` is `Self`, as in Freya's default
                // `fn expanded(self) -> Self`.
                ty @ (Type::Definition { .. } | Type::SelfTy { .. }) => Receiver::Value { ty },
                ty => Receiver::Explicit { ty },
            }
        } else {
            Receiver::None
        };
        let guard = if method && matches!(receiver, Receiver::Reference { .. }) {
            self.guard(poly, owner)?
        } else {
            None
        };
        Ok(Signature {
            safety: if signature.safety().is_safe() {
                Safety::Safe
            } else {
                Safety::Unsafe
            },
            const_function: matches!(self.tcx.constness(owner), hir::Constness::Const { .. }),
            async_function: self.tcx.asyncness(owner).is_async(),
            abi: self.budget.formatted(format_args!("{}", signature.abi()))?,
            variadic: signature.c_variadic(),
            binders,
            receiver,
            inputs,
            output: self.type_of(signature.output(), owner, 0)?,
            guard,
        })
    }

    /// A borrowing method's output that is a guard: a nominal type holding
    /// a lifetime, not a reference, that implements `Deref`. rustc
    /// normalizes the output and its `Deref::Target` in the method's own
    /// environment, so `ReadRef<'_, T>`, Freya's projection through
    /// generational-box's `AnyStorage`, gives `T`. A target that still
    /// holds a lifetime or an alias is none.
    fn guard(&mut self, poly: ty::PolyFnSig<'tcx>, owner: DefId) -> Result<Option<Guard>, String> {
        let tcx = self.tcx;
        let output = poly.skip_binder().output();
        let holds_lifetime = output
            .walk()
            .any(|argument| matches!(argument.kind(), ty::GenericArgKind::Lifetime(_)));
        if matches!(output.kind(), ty::Ref(..)) || !holds_lifetime {
            return Ok(None);
        }
        let lang = tcx.lang_items();
        let (Some(deref), Some(deref_mut), Some(target)) = (
            lang.deref_trait(),
            lang.deref_mut_trait(),
            lang.deref_target(),
        ) else {
            return Ok(None);
        };
        let typing_env = ty::TypingEnv::non_body_analysis(tcx, owner);
        let output = tcx.instantiate_bound_regions_with_erased(poly.output());
        let Ok(output) =
            tcx.try_normalize_erasing_regions(typing_env, ty::Unnormalized::new(output))
        else {
            return Ok(None);
        };
        if !matches!(output.kind(), ty::Adt(..)) {
            return Ok(None);
        }
        let infcx = tcx.infer_ctxt().build(ty::TypingMode::non_body_analysis());
        if !infcx
            .type_implements_trait(deref, [output], typing_env.param_env)
            .must_apply_modulo_regions()
        {
            return Ok(None);
        }
        let mutable = infcx
            .type_implements_trait(deref_mut, [output], typing_env.param_env)
            .must_apply_modulo_regions();
        let projection = Ty::new_projection(tcx, ty::IsRigid::No, target, [output]);
        let Ok(target) =
            tcx.try_normalize_erasing_regions(typing_env, ty::Unnormalized::new(projection))
        else {
            return Ok(None);
        };
        if target.has_aliases()
            || target
                .walk()
                .any(|argument| matches!(argument.kind(), ty::GenericArgKind::Lifetime(_)))
        {
            return Ok(None);
        }
        self.budget.items(1)?;
        Ok(Some(Guard {
            target: self.type_of(target, owner, 0)?,
            mutable,
        }))
    }

    fn type_of(&mut self, ty: Ty<'tcx>, owner: DefId, depth: usize) -> Result<Type, String> {
        check_type_depth(depth)?;
        Ok(match ty.kind() {
            ty::Bool => self.primitive("bool")?,
            ty::Char => self.primitive("char")?,
            ty::Int(kind) => self.primitive(kind.name_str())?,
            ty::Uint(kind) => self.primitive(kind.name_str())?,
            ty::Float(kind) => self.primitive(kind.name_str())?,
            ty::Str => self.primitive("str")?,
            ty::Adt(definition, arguments) => {
                let def_id = definition.did();
                self.ensure(def_id, Namespace::Type)?;
                Type::Definition {
                    id: self.id(def_id, Namespace::Type)?,
                    arguments: self.arguments(arguments, owner, depth.saturating_add(1))?,
                }
            }
            ty::Param(parameter) => {
                let resolved =
                    self.parameter_owner(owner, parameter.index, ParameterClass::Type)?;
                if resolved.self_type {
                    Type::SelfTy {
                        owner: resolved.owner,
                    }
                } else {
                    Type::Parameter {
                        owner: resolved.owner,
                        index: parameter.index,
                    }
                }
            }
            ty::Tuple(elements) => Type::Tuple {
                elements: {
                    self.edge(elements.len())?;
                    let mut extracted = Vec::new();
                    for element in elements.iter() {
                        self.budget.items(1)?;
                        extracted
                            .try_reserve(1)
                            .map_err(|error| format!("tuple allocation failed: {error}"))?;
                        extracted.push(self.type_of(element, owner, depth.saturating_add(1))?);
                    }
                    extracted
                },
            },
            ty::Slice(element) => {
                self.budget.items(1)?;
                Type::Slice {
                    element: Box::new(self.type_of(*element, owner, depth.saturating_add(1))?),
                }
            }
            ty::Array(element, length) => {
                self.budget.items(1)?;
                Type::Array {
                    element: Box::new(self.type_of(*element, owner, depth.saturating_add(1))?),
                    length: self.ty_constant(*length, owner)?,
                }
            }
            ty::Ref(region, inner, mutability) => {
                self.budget.items(1)?;
                Type::Reference {
                    lifetime: self.lifetime(*region, owner)?,
                    mutability: mutability_of(*mutability),
                    ty: Box::new(self.type_of(*inner, owner, depth.saturating_add(1))?),
                }
            }
            ty::RawPtr(inner, mutability) => {
                self.budget.items(1)?;
                Type::RawPointer {
                    mutability: mutability_of(*mutability),
                    ty: Box::new(self.type_of(*inner, owner, depth.saturating_add(1))?),
                }
            }
            ty::FnPtr(signature, header) => {
                self.budget.items(1)?;
                Type::FunctionPointer {
                    signature: Box::new(self.poly_signature(
                        signature.with(*header),
                        owner,
                        false,
                    )?),
                }
            }
            ty::Dynamic(predicates, region) => {
                let mut bounds = Vec::new();
                self.edge(predicates.len())?;
                for predicate in predicates.iter() {
                    let binders = self.binders(owner, predicate.bound_vars())?;
                    match predicate.skip_binder() {
                        ty::ExistentialPredicate::Trait(reference) => {
                            self.ensure(reference.def_id, Namespace::Type)?;
                            self.budget.items(1)?;
                            bounds.try_reserve(1).map_err(|error| {
                                format!("trait object bound allocation failed: {error}")
                            })?;
                            bounds.push(ObjectBound::Trait {
                                bound: Bound::Trait {
                                    trait_id: self.id(reference.def_id, Namespace::Type)?,
                                    arguments: self.arguments(reference.args, owner, depth)?,
                                    polarity: Polarity::Positive,
                                    binders,
                                },
                            });
                        }
                        ty::ExistentialPredicate::AutoTrait(def_id) => {
                            self.ensure(def_id, Namespace::Type)?;
                            self.budget.items(1)?;
                            bounds.try_reserve(1).map_err(|error| {
                                format!("trait object bound allocation failed: {error}")
                            })?;
                            bounds.push(ObjectBound::Trait {
                                bound: Bound::Trait {
                                    trait_id: self.id(def_id, Namespace::Type)?,
                                    arguments: Vec::new(),
                                    polarity: Polarity::Positive,
                                    binders,
                                },
                            });
                        }
                        ty::ExistentialPredicate::Projection(projection) => {
                            let associated: DefId = projection.def_id;
                            self.ensure(self.tcx.parent(associated), Namespace::Type)?;
                            self.ensure(associated, Namespace::Type)?;
                            let Some(value) = projection.term.as_type() else {
                                return Ok(Type::Unknown {
                                    compiler_kind: self
                                        .budget
                                        .owned("trait-object-const-associated-binding")?,
                                });
                            };
                            self.budget.items(1)?;
                            bounds.try_reserve(1).map_err(|error| {
                                format!("trait object bound allocation failed: {error}")
                            })?;
                            bounds.push(ObjectBound::ProjectionEquality {
                                associated: self.id(associated, Namespace::Type)?,
                                arguments: self.arguments(projection.args, owner, depth)?,
                                ty: self.type_of(value, owner, depth.saturating_add(1))?,
                            });
                        }
                    }
                }
                Type::TraitObject {
                    bounds,
                    lifetime: self.lifetime(*region, owner)?,
                }
            }
            ty::Alias(_, alias) => match alias.kind {
                ty::AliasTyKind::Projection { def_id } => {
                    let associated: DefId = def_id;
                    self.request_trait_obligation(
                        owner,
                        self.tcx.parent(associated),
                        alias.args,
                        ty::PredicatePolarity::Positive,
                        Some(associated),
                    )?;
                    self.ensure(self.tcx.parent(associated), Namespace::Type)?;
                    self.ensure(associated, Namespace::Type)?;
                    Type::Projection {
                        trait_id: self.id(self.tcx.parent(associated), Namespace::Type)?,
                        associated: self.id(associated, Namespace::Type)?,
                        arguments: self.arguments(alias.args, owner, depth)?,
                    }
                }
                ty::AliasTyKind::Opaque { def_id } => {
                    let opaque: DefId = def_id;
                    Type::ImplTrait {
                        id: self.id(opaque, Namespace::Type)?,
                        // `explicit_item_bounds(opaque)` is expressed over
                        // `GenericArgs::identity_for_item(opaque)`: formal
                        // parameters use the opaque's own generics and exact
                        // parent chain. The alias arguments below remain
                        // actual substitutions in the defining-use context.
                        bounds: self.item_bounds(opaque, opaque)?,
                        captures: self.arguments(alias.args, owner, depth.saturating_add(1))?,
                    }
                }
                ty::AliasTyKind::Free { def_id } => {
                    let alias_id: DefId = def_id;
                    self.ensure(alias_id, Namespace::Type)?;
                    Type::Definition {
                        id: self.id(alias_id, Namespace::Type)?,
                        arguments: self.arguments(alias.args, owner, depth)?,
                    }
                }
                ty::AliasTyKind::Inherent { def_id } => {
                    let alias_id: DefId = def_id;
                    self.ensure(alias_id, Namespace::Type)?;
                    Type::Definition {
                        id: self.id(alias_id, Namespace::Type)?,
                        arguments: self.arguments(alias.args, owner, depth)?,
                    }
                }
            },
            ty::Never => Type::Never,
            kind => Type::Unknown {
                compiler_kind: self.budget.owned(ty_kind_text(kind))?,
            },
        })
    }

    fn arguments(
        &mut self,
        arguments: ty::GenericArgsRef<'tcx>,
        owner: DefId,
        depth: usize,
    ) -> Result<Vec<Argument>, String> {
        self.arguments_iter(arguments.iter(), owner, depth)
    }

    fn arguments_iter(
        &mut self,
        arguments: impl Iterator<Item = ty::GenericArg<'tcx>>,
        owner: DefId,
        depth: usize,
    ) -> Result<Vec<Argument>, String> {
        self.arguments_iter_with_opaque(arguments, owner, depth, None)
    }

    fn arguments_with_opaque(
        &mut self,
        arguments: ty::GenericArgsRef<'tcx>,
        owner: DefId,
        depth: usize,
        opaque: Option<DefId>,
    ) -> Result<Vec<Argument>, String> {
        self.arguments_iter_with_opaque(arguments.iter(), owner, depth, opaque)
    }

    fn arguments_iter_with_opaque(
        &mut self,
        arguments: impl Iterator<Item = ty::GenericArg<'tcx>>,
        owner: DefId,
        depth: usize,
        opaque: Option<DefId>,
    ) -> Result<Vec<Argument>, String> {
        let mut extracted = Vec::new();
        for argument in arguments {
            self.edge(1)?;
            self.budget.items(1)?;
            extracted
                .try_reserve(1)
                .map_err(|error| format!("generic argument allocation failed: {error}"))?;
            extracted.push(match argument.kind() {
                ty::GenericArgKind::Lifetime(region) => Argument::Lifetime {
                    lifetime: self.lifetime(region, owner)?,
                },
                ty::GenericArgKind::Type(ty) => Argument::Type {
                    ty: match opaque_argument_action(opaque, opaque_identity(ty)) {
                        OpaqueArgumentAction::SelfReference(opaque) => Type::SelfTy {
                            owner: self.id(opaque, Namespace::Type)?,
                        },
                        OpaqueArgumentAction::ConvertNormally => self.type_of(ty, owner, depth)?,
                    },
                },
                ty::GenericArgKind::Const(value) => Argument::Const {
                    value: self.ty_constant(value, owner)?,
                },
            });
        }
        Ok(extracted)
    }

    fn constant(&mut self, def_id: DefId) -> Result<Constant, String> {
        let ty = self
            .tcx
            .type_of(def_id)
            .instantiate_identity()
            .skip_norm_wip();
        let typing = ty::TypingEnv::post_analysis(self.tcx, def_id);
        let evaluated = self.tcx.const_eval_poly(def_id).ok();
        Ok(
            match evaluated.and_then(|value| value.try_to_bits_for_ty(self.tcx, typing, ty)) {
                Some(value) => {
                    let ty = primitive_name(ty).unwrap_or("scalar");
                    Constant::Literal {
                        ty: self.budget.owned(ty)?,
                        value: self.budget.formatted(format_args!("0x{value:x}"))?,
                    }
                }
                // A constant rustc evaluates to no scalar, as iced's
                // `Font::MONOSPACE: Font` or `Theme::ALL: &[Theme]`, is
                // the constant's own value: Rawr reads it by its path
                // where the bridge runs.
                None if evaluated.is_some() => Constant::Unevaluated {
                    definition: self.id(def_id, Namespace::Value)?,
                    arguments: Vec::new(),
                },
                None => Constant::Unknown {
                    compiler_kind: self.budget.owned("non-scalar-constant")?,
                },
            },
        )
    }

    fn ty_constant(&mut self, value: ty::Const<'tcx>, owner: DefId) -> Result<Constant, String> {
        Ok(match value.kind() {
            ty::ConstKind::Value(value) => Constant::Literal {
                ty: self.budget.owned("const")?,
                value: self.budget.formatted(format_args!("{value}"))?,
            },
            ty::ConstKind::Param(parameter) => Constant::Parameter {
                owner: self
                    .parameter_owner(owner, parameter.index, ParameterClass::Const)?
                    .owner,
                index: parameter.index,
            },
            ty::ConstKind::Alias(_rigidity, alias) => {
                let definition = match alias.kind {
                    ty::AliasConstKind::Projection { def_id } => {
                        let definition: DefId = def_id;
                        self.request_trait_obligation(
                            owner,
                            self.tcx.parent(definition),
                            alias.args,
                            ty::PredicatePolarity::Positive,
                            Some(definition),
                        )?;
                        definition
                    }
                    ty::AliasConstKind::Inherent { def_id } => def_id,
                    ty::AliasConstKind::Free { def_id } => def_id,
                    ty::AliasConstKind::Anon { def_id } => {
                        return self.anonymous_constant(
                            value,
                            def_id,
                            alias.args,
                            alias.type_of(self.tcx).skip_norm_wip(),
                            owner,
                        );
                    }
                };
                self.ensure(definition, Namespace::Value)?;
                Constant::Unevaluated {
                    definition: self.id(definition, Namespace::Value)?,
                    arguments: self.arguments(alias.args, owner, 0)?,
                }
            }
            kind => Constant::Unknown {
                compiler_kind: self.budget.owned(const_kind_text(kind))?,
            },
        })
    }

    fn anonymous_constant(
        &mut self,
        value: ty::Const<'tcx>,
        definition: DefId,
        arguments: ty::GenericArgsRef<'tcx>,
        constant_ty: Ty<'tcx>,
        owner: DefId,
    ) -> Result<Constant, String> {
        if !matches!(
            self.tcx.def_kind(definition),
            DefKind::AnonConst | DefKind::InlineConst
        ) {
            return Err("anonymous constant kind disagrees with compiler definition".to_owned());
        }
        // An anonymous constant Rawr cannot represent rejects only the items
        // whose shapes contain it; it never fails the whole extraction.
        if value.has_param()
            || value.has_infer()
            || value.has_placeholders()
            || value.has_bound_vars()
            || value.references_error()
        {
            return self.unknown_constant("anonymous-constant:generic");
        }
        let typing_env = ty::TypingEnv::post_analysis(self.tcx, owner);
        let unevaluated = mir::Const::Unevaluated(
            mir::UnevaluatedConst::new(definition, arguments),
            constant_ty,
        );
        // A `bool` or unsigned constant is its value, typed.
        if constant_ty != self.tcx.types.usize {
            let value = match constant_ty.kind() {
                ty::Bool => unevaluated
                    .try_eval_bool(self.tcx, typing_env)
                    .map(|value| value.to_string()),
                ty::Uint(_) => unevaluated
                    .try_eval_bits(self.tcx, typing_env)
                    .map(|value| value.to_string()),
                _ => return self.unknown_constant("anonymous-constant:literal-type"),
            };
            let Some(value) = value else {
                return self.unknown_constant("anonymous-constant:evaluation");
            };
            return Ok(Constant::Literal {
                ty: self
                    .budget
                    .owned(primitive_name(constant_ty).unwrap_or("const"))?,
                value: self.budget.owned(&value)?,
            });
        }
        let Some(evaluated) = unevaluated.try_eval_target_usize(self.tcx, typing_env) else {
            return self.unknown_constant("anonymous-constant:evaluation");
        };
        Ok(Constant::Literal {
            ty: self.budget.owned("const")?,
            value: self.budget.formatted(format_args!("{evaluated}"))?,
        })
    }

    fn unknown_constant(&mut self, compiler_kind: &str) -> Result<Constant, String> {
        Ok(Constant::Unknown {
            compiler_kind: self.budget.owned(compiler_kind)?,
        })
    }

    fn generics(&mut self, def_id: DefId) -> Result<Generics, String> {
        let generics = self.tcx.generics_of(def_id);
        let clauses = self.tcx.predicates_of(def_id).predicates;
        self.edge(clauses.len())?;
        let mut parameter_bounds = HashMap::<DefId, Vec<Bound>>::new();
        let mut own_type_parameters = HashMap::<u32, DefId>::new();
        for parameter in &generics.own_params {
            if matches!(parameter.kind, ty::GenericParamDefKind::Type { .. }) {
                self.budget.items(1)?;
                own_type_parameters.try_reserve(1).map_err(|error| {
                    format!("generic parameter index allocation failed: {error}")
                })?;
                own_type_parameters.insert(parameter.index, parameter.def_id);
            }
        }
        let sized = self
            .tcx
            .lang_items()
            .sized_trait()
            .ok_or_else(|| "compiler metadata has no Sized lang item".to_owned())?;
        let mut sized_parameters = HashSet::new();
        let mut predicates = Vec::new();
        for (clause, _) in clauses {
            let kind = clause.kind().skip_binder();
            if matches!(kind, ty::ClauseKind::ConstArgHasType(..)) {
                // The exact const-parameter type is already retained in
                // `ParameterKind::Const`; this compiler well-formedness clause
                // adds no independent public predicate.
                continue;
            }
            if let ty::ClauseKind::Trait(trait_predicate) = kind
                && trait_predicate.def_id() == sized
                && trait_predicate.polarity == ty::PredicatePolarity::Positive
                && let ty::Param(parameter) = trait_predicate.self_ty().kind()
                && let Some(parameter_id) = own_type_parameters.get(&parameter.index)
            {
                self.budget.items(1)?;
                sized_parameters
                    .try_reserve(1)
                    .map_err(|error| format!("Sized parameter allocation failed: {error}"))?;
                sized_parameters.insert(*parameter_id);
                continue;
            }
            let predicate = self.predicate(def_id, *clause)?;
            self.budget.items(1)?;
            predicates
                .try_reserve(1)
                .map_err(|error| format!("generic predicate allocation failed: {error}"))?;
            predicates.push(predicate);
        }
        for parameter in own_type_parameters.values() {
            if !sized_parameters.contains(parameter) {
                self.edge(1)?;
                if !parameter_bounds.contains_key(parameter) {
                    self.budget.items(1)?;
                    parameter_bounds
                        .try_reserve(1)
                        .map_err(|error| format!("generic bound map allocation failed: {error}"))?;
                    parameter_bounds.insert(*parameter, Vec::new());
                }
                let bounds = parameter_bounds
                    .get_mut(parameter)
                    .ok_or_else(|| "generic bound map lost its key".to_owned())?;
                self.budget.items(1)?;
                bounds
                    .try_reserve(1)
                    .map_err(|error| format!("generic bound allocation failed: {error}"))?;
                bounds.push(Bound::MaybeSized);
            }
        }
        self.edge(generics.own_params.len())?;
        let owner = self.id(def_id, definition_namespace(self.tcx.def_kind(def_id)))?;
        let mut parameters = Vec::new();
        for parameter in &generics.own_params {
            let name = self.budget.owned(parameter.name.as_str())?;
            let mut bounds = parameter_bounds
                .remove(&parameter.def_id)
                .unwrap_or_default();
            bounds.sort_unstable();
            bounds.dedup();
            // A default only an alias of the selected crate names, as
            // `Widget`'s `Renderer = crate::Renderer` names
            // `fallback::Renderer<iced_wgpu::Renderer,
            // iced_tiny_skia::Renderer>`: the alias is demanded, so Rust
            // code can write the default (D604.5).
            if let ty::GenericParamDefKind::Type {
                has_default: true, ..
            } = parameter.kind
            {
                let default = self
                    .tcx
                    .type_of(parameter.def_id)
                    .instantiate_identity()
                    .skip_norm_wip();
                if let Some(alias) = self.instance_alias(default) {
                    self.ensure_with_demand(alias, Namespace::Type, Demand::ShapeOnly)?;
                }
            }
            let parameter_kind = match parameter.kind {
                ty::GenericParamDefKind::Lifetime => ParameterKind::Lifetime { bounds },
                ty::GenericParamDefKind::Type { has_default, .. } => ParameterKind::Type {
                    bounds,
                    default: if has_default {
                        Explicit::Present {
                            value: self.type_of(
                                self.tcx
                                    .type_of(parameter.def_id)
                                    .instantiate_identity()
                                    .skip_norm_wip(),
                                def_id,
                                0,
                            )?,
                        }
                    } else {
                        Explicit::Absent
                    },
                },
                ty::GenericParamDefKind::Const { has_default } => {
                    if !bounds.is_empty() {
                        return Err("const generic parameter has declared bounds".to_owned());
                    }
                    ParameterKind::Const {
                        ty: self.type_of(
                            self.tcx
                                .type_of(parameter.def_id)
                                .instantiate_identity()
                                .skip_norm_wip(),
                            def_id,
                            0,
                        )?,
                        default: if has_default {
                            Explicit::Present {
                                value: self.ty_constant(
                                    self.tcx
                                        .const_param_default(parameter.def_id)
                                        .instantiate_identity()
                                        .skip_norm_wip(),
                                    def_id,
                                )?,
                            }
                        } else {
                            Explicit::Absent
                        },
                    }
                }
            };
            self.budget.items(1)?;
            self.charge_id(&owner)?;
            parameters
                .try_reserve(1)
                .map_err(|error| format!("generic parameter allocation failed: {error}"))?;
            parameters.push(Parameter {
                owner: owner.clone(),
                index: parameter.index,
                name,
                parameter_kind,
            });
        }
        if !parameter_bounds.is_empty() {
            return Err("declared bound parameter is absent from compiler generics".to_owned());
        }
        predicates.sort_unstable();
        predicates.dedup();
        Ok(Generics {
            parameters,
            predicates,
        })
    }

    fn predicate(&mut self, owner: DefId, clause: ty::Clause<'tcx>) -> Result<Predicate, String> {
        self.predicate_with_opaque(owner, clause, None)
    }

    fn predicate_with_opaque(
        &mut self,
        owner: DefId,
        clause: ty::Clause<'tcx>,
        opaque: Option<DefId>,
    ) -> Result<Predicate, String> {
        let bound = clause.kind();
        let binders = self.binders(owner, bound.bound_vars())?;
        Ok(match bound.skip_binder() {
            ty::ClauseKind::Trait(predicate) => {
                self.request_trait_obligation(
                    owner,
                    predicate.def_id(),
                    predicate.trait_ref.args,
                    predicate.polarity,
                    None,
                )?;
                self.ensure(predicate.def_id(), Namespace::Type)?;
                let opaque_self = if let ty::Alias(_, alias) = predicate.self_ty().kind() {
                    match alias.kind {
                        ty::AliasTyKind::Opaque { def_id } => {
                            opaque.is_some_and(|opaque| def_id == opaque)
                        }
                        ty::AliasTyKind::Projection { .. }
                        | ty::AliasTyKind::Inherent { .. }
                        | ty::AliasTyKind::Free { .. } => false,
                    }
                } else {
                    false
                };
                let self_ty = if opaque_self {
                    Type::SelfTy {
                        owner: self.id(
                            opaque.ok_or_else(|| "opaque self identity is absent".to_owned())?,
                            Namespace::Type,
                        )?,
                    }
                } else {
                    self.type_of(predicate.self_ty(), owner, 0)?
                };
                Predicate::Bound {
                    ty: self_ty,
                    bound: Bound::Trait {
                        trait_id: self.id(predicate.def_id(), Namespace::Type)?,
                        arguments: self.arguments_iter(
                            predicate.trait_ref.args.iter().skip(1),
                            owner,
                            0,
                        )?,
                        polarity: match predicate.polarity {
                            ty::PredicatePolarity::Positive => Polarity::Positive,
                            ty::PredicatePolarity::Negative => Polarity::Negative,
                        },
                        binders,
                    },
                }
            }
            ty::ClauseKind::RegionOutlives(predicate) => Predicate::LifetimeOutlives {
                longer: self.lifetime(predicate.0, owner)?,
                shorter: self.lifetime(predicate.1, owner)?,
            },
            ty::ClauseKind::TypeOutlives(predicate) => Predicate::TypeOutlives {
                ty: match opaque_argument_action(opaque, opaque_identity(predicate.0)) {
                    OpaqueArgumentAction::SelfReference(opaque) => Type::SelfTy {
                        owner: self.id(opaque, Namespace::Type)?,
                    },
                    OpaqueArgumentAction::ConvertNormally => self.type_of(predicate.0, owner, 0)?,
                },
                lifetime: self.lifetime(predicate.1, owner)?,
            },
            ty::ClauseKind::Projection(predicate) => {
                let associated: DefId = predicate.def_id();
                self.request_trait_obligation(
                    owner,
                    self.tcx.parent(associated),
                    predicate.projection_term.args,
                    ty::PredicatePolarity::Positive,
                    Some(associated),
                )?;
                self.ensure(self.tcx.parent(associated), Namespace::Type)?;
                self.ensure(associated, Namespace::Type)?;
                let projection = Type::Projection {
                    trait_id: self.id(self.tcx.parent(associated), Namespace::Type)?,
                    associated: self.id(associated, Namespace::Type)?,
                    arguments: self.arguments_with_opaque(
                        predicate.projection_term.args,
                        owner,
                        0,
                        opaque,
                    )?,
                };
                match predicate.term.as_type() {
                    Some(ty) => Predicate::ProjectionEquality {
                        projection,
                        ty: self.type_of(ty, owner, 0)?,
                    },
                    None => Predicate::Unknown {
                        compiler_kind: self.budget.owned("const-projection-predicate")?,
                    },
                }
            }
            ty::ClauseKind::ConstEvaluatable(value) => Predicate::ConstEvaluatable {
                value: self.ty_constant(value, owner)?,
            },
            ty::ClauseKind::ConstArgHasType(..) => Predicate::Unknown {
                compiler_kind: self.budget.owned("const-argument-has-type")?,
            },
            ty::ClauseKind::WellFormed(..) => Predicate::Unknown {
                compiler_kind: self.budget.owned("well-formed-clause")?,
            },
            ty::ClauseKind::HostEffect(..) => Predicate::Unknown {
                compiler_kind: self.budget.owned("host-effect-clause")?,
            },
            ty::ClauseKind::UnstableFeature(..) => Predicate::Unknown {
                compiler_kind: self.budget.owned("unstable-feature-clause")?,
            },
        })
    }

    fn predicates_from_clauses(
        &mut self,
        owner: DefId,
        clauses: impl IntoIterator<Item = ty::Clause<'tcx>>,
    ) -> Result<Vec<Predicate>, String> {
        let mut predicates = Vec::new();
        for clause in clauses {
            self.budget.items(1)?;
            predicates
                .try_reserve(1)
                .map_err(|error| format!("predicate allocation failed: {error}"))?;
            predicates.push(self.predicate(owner, clause)?);
        }
        predicates.sort_unstable();
        predicates.dedup();
        Ok(predicates)
    }

    fn item_bounds(&mut self, def_id: DefId, owner: DefId) -> Result<Vec<Predicate>, String> {
        let tcx = self.tcx;
        let predicates = tcx.explicit_item_bounds(def_id).skip_binder();
        self.edge(predicates.len())?;
        let mut extracted = Vec::new();
        for (clause, _) in predicates {
            self.budget.items(1)?;
            extracted
                .try_reserve(1)
                .map_err(|error| format!("item bound allocation failed: {error}"))?;
            extracted.push(self.predicate_with_opaque(owner, *clause, Some(def_id))?);
        }
        extracted.sort_unstable();
        extracted.dedup();
        Ok(extracted)
    }

    fn binders(
        &mut self,
        owner: DefId,
        variables: &'tcx ty::List<ty::BoundVariableKind<'tcx>>,
    ) -> Result<Vec<Binder>, String> {
        self.edge(variables.len())?;
        let owner = self.id(owner, definition_namespace(self.tcx.def_kind(owner)))?;
        let mut binders = Vec::new();
        self.budget.items(variables.len())?;
        binders
            .try_reserve_exact(variables.len())
            .map_err(|error| format!("compiler binder allocation failed: {error}"))?;
        for (index, variable) in variables.iter().enumerate() {
            let name = match variable {
                ty::BoundVariableKind::Region(region) => match region.get_name(self.tcx) {
                    Some(name) => self.budget.owned(name.as_str())?,
                    None => self.budget.formatted(format_args!("'#{index}"))?,
                },
                ty::BoundVariableKind::Ty(ty::BoundTyKind::Param(def_id)) => {
                    let name = self.tcx.item_name(def_id);
                    self.budget.owned(name.as_str())?
                }
                ty::BoundVariableKind::Ty(ty::BoundTyKind::Anon) => {
                    self.budget.formatted(format_args!("type#{index}"))?
                }
                ty::BoundVariableKind::Const => {
                    self.budget.formatted(format_args!("const#{index}"))?
                }
            };
            self.charge_id(&owner)?;
            binders.push(Binder {
                owner: owner.clone(),
                depth: 0,
                index: u32::try_from(index)
                    .map_err(|_| "compiler binder index exceeds u32".to_owned())?,
                name,
            });
        }
        Ok(binders)
    }

    fn lifetime(&mut self, region: ty::Region<'tcx>, owner: DefId) -> Result<Lifetime, String> {
        Ok(match region.kind() {
            ty::ReStatic => Lifetime::Static,
            ty::ReEarlyParam(parameter) => Lifetime::Early {
                owner: self
                    .parameter_owner(owner, parameter.index, ParameterClass::Lifetime)?
                    .owner,
                index: parameter.index,
            },
            ty::ReBound(depth, bound) => {
                let depth = match depth {
                    ty::BoundVarIndexKind::Bound(depth) => depth.as_u32(),
                    ty::BoundVarIndexKind::Canonical => {
                        return Err(
                            "canonical lifetime binder escaped compiler canonicalization"
                                .to_owned(),
                        );
                    }
                };
                let name = match bound.kind.get_name(self.tcx) {
                    Some(name) => self.budget.owned(name.as_str())?,
                    None => self
                        .budget
                        .formatted(format_args!("'#{}", bound.var.as_u32()))?,
                };
                Lifetime::Late {
                    binder: Binder {
                        owner: self.id(owner, definition_namespace(self.tcx.def_kind(owner)))?,
                        depth,
                        index: bound.var.as_u32(),
                        name,
                    },
                    index: bound.var.as_u32(),
                }
            }
            ty::ReLateParam(parameter) => {
                let index = match parameter.kind {
                    ty::LateParamRegionKind::Anon(index)
                    | ty::LateParamRegionKind::NamedAnon(index, _) => index,
                    ty::LateParamRegionKind::Named(def_id) => self
                        .tcx
                        .generics_of(parameter.scope)
                        .param_def_id_to_index(self.tcx, def_id)
                        .ok_or_else(|| {
                            "late lifetime parameter is absent from generics".to_owned()
                        })?,
                    ty::LateParamRegionKind::ClosureEnv => 0,
                };
                let name = match parameter.kind.get_name(self.tcx) {
                    Some(name) => self.budget.owned(name.as_str())?,
                    None => self.budget.formatted(format_args!("'#{index}"))?,
                };
                Lifetime::Late {
                    binder: Binder {
                        owner: self.id(
                            parameter.scope,
                            definition_namespace(self.tcx.def_kind(parameter.scope)),
                        )?,
                        depth: 0,
                        index,
                        name,
                    },
                    index,
                }
            }
            ty::ReErased => Lifetime::Erased,
            ty::ReVar(_) | ty::RePlaceholder(_) | ty::ReError(_) => {
                return Err("unstable compiler lifetime escaped into public API".to_owned());
            }
        })
    }

    fn parameter_owner(
        &mut self,
        owner: DefId,
        index: u32,
        expected: ParameterClass,
    ) -> Result<ResolvedParameter, String> {
        let owner = self.parameter_owner_def_id(owner, index, expected)?;
        let generics = self.tcx.generics_of(owner);
        Ok(ResolvedParameter {
            owner: self.id(owner, definition_namespace(self.tcx.def_kind(owner)))?,
            self_type: expected == ParameterClass::Type && generics.has_self && index == 0,
        })
    }

    fn parameter_owner_def_id(
        &self,
        mut owner: DefId,
        index: u32,
        expected: ParameterClass,
    ) -> Result<DefId, String> {
        loop {
            let generics = self.tcx.generics_of(owner);
            if let Some(parameter) = generics
                .own_params
                .iter()
                .find(|parameter| parameter.index == index)
            {
                validate_parameter_class(Some(parameter_class(&parameter.kind)), expected)?;
                return Ok(owner);
            }
            let Some(parent) = generics.parent else {
                return match validate_parameter_class(None, expected) {
                    Ok(()) => Err("generic parameter owner validation lost its failure".to_owned()),
                    Err(error) => Err(error),
                };
            };
            owner = parent;
        }
    }

    fn id(&mut self, def_id: DefId, namespace: Namespace) -> Result<Id, String> {
        let crate_name = self.tcx.crate_name(def_id.krate);
        let crate_name = self.budget.owned(crate_name.as_str())?;
        let stable_crate_id = self.budget.formatted(format_args!(
            "{:016x}",
            self.tcx.stable_crate_id(def_id.krate)
        ))?;
        let authority = Authority::Compiler {
            crate_name,
            stable_crate_id,
        };
        Ok(Id {
            authority,
            def_path_hash: self.def_hash(def_id)?,
            namespace,
        })
    }

    fn is_sysroot_crate(&self, crate_num: CrateNum) -> Result<bool, String> {
        let source = self.tcx.used_crate_source(crate_num);
        let mut paths = source.paths();
        let Some(first) = paths.next() else {
            return Err("compiler crate source contains no paths".to_owned());
        };
        let sysroot = self.tcx.sess.opts.sysroot.path();
        Ok(first.starts_with(sysroot) && paths.all(|path| path.starts_with(sysroot)))
    }

    fn register_sysroot(&mut self, crate_num: CrateNum) -> Result<(), String> {
        if self.sysroot.contains_key(&crate_num) {
            return Ok(());
        }
        if self.sysroot.len() >= MAX_PACKAGES {
            return Err("compiler sysroot package limit exceeded".to_owned());
        }
        if !self.is_sysroot_crate(crate_num)? {
            return Err("non-sysroot crate cannot enter compiler authority registry".to_owned());
        }
        let crate_name = self.tcx.crate_name(crate_num);
        let crate_name = self.budget.owned(crate_name.as_str())?;
        let compiler_commit = self.budget.owned(&self.invocation.compiler_commit)?;
        let selected_target = self.budget.owned(&self.invocation.selected_target)?;
        let stable_crate_id = self
            .budget
            .formatted(format_args!("{:016x}", self.tcx.stable_crate_id(crate_num)))?;
        self.budget.items(1)?;
        self.sysroot.insert(
            crate_num,
            Sysroot {
                compiler_commit,
                selected_target,
                crate_name,
                stable_crate_id,
            },
        );
        Ok(())
    }

    fn span(&mut self, span: RustSpan) -> Result<Span, String> {
        let source_map = self.tcx.sess.source_map();
        let start = source_map.lookup_char_pos(span.lo());
        let end = source_map.lookup_char_pos(span.hi());
        if start.file.name != end.file.name {
            return Err("compiler span crosses source files".to_owned());
        }
        let file = self.budget.formatted(format_args!(
            "{}",
            source_map.filename_for_diagnostics(&start.file.name)
        ))?;
        Ok(Span {
            file,
            byte_start: start.file.original_relative_byte_pos(span.lo()).0,
            byte_end: end.file.original_relative_byte_pos(span.hi()).0,
            line_start: u32::try_from(start.line)
                .map_err(|_| "source start line exceeds u32".to_owned())?,
            column_start: u32::try_from(start.col.0)
                .map_err(|_| "source start column exceeds u32".to_owned())?,
            line_end: u32::try_from(end.line)
                .map_err(|_| "source end line exceeds u32".to_owned())?,
            column_end: u32::try_from(end.col.0)
                .map_err(|_| "source end column exceeds u32".to_owned())?,
        })
    }

    fn visibility(&mut self, visibility: ty::Visibility<DefId>) -> Result<Visibility, String> {
        match visibility {
            ty::Visibility::Public => Ok(Visibility::Public),
            ty::Visibility::Restricted(scope) => {
                self.ensure(scope, Namespace::Type)?;
                Ok(Visibility::Restricted {
                    scope: self.id(scope, Namespace::Type)?,
                })
            }
        }
    }

    fn origin(&mut self, span: RustSpan) -> Result<Origin, String> {
        if !span.from_expansion() {
            return Ok(Origin::Source);
        }
        match span.source_callee() {
            Some(expansion) if !expansion.def_site.is_dummy() => Ok(Origin::Expansion {
                call_site: self.span(expansion.call_site.source_callsite())?,
                definition_site: self.span(expansion.def_site)?,
            }),
            Some(_) | None => Ok(Origin::Synthesized {
                mechanism: self
                    .budget
                    .owned("compiler-expansion-without-definition-site")?,
            }),
        }
    }

    fn validate_anonymous_associated_owner(
        &mut self,
        support: AnonymousAssociatedSupport,
        owner: &Owner,
    ) -> Result<(), String> {
        let expected = self.id(support.parent, Namespace::Type)?;
        let matches = match (self.tcx.def_kind(support.parent), owner) {
            (DefKind::Trait, Owner::Definition { id }) => id == &expected,
            (DefKind::Impl { .. }, Owner::Implementation { id }) => id == &expected,
            _ => false,
        };
        if matches {
            Ok(())
        } else {
            Err("anonymous associated support has an invalid compiler parent owner".to_owned())
        }
    }

    fn display_path(&mut self, def_id: DefId) -> Result<Vec<String>, String> {
        let associated = associated_path_kind(self.tcx, def_id)?;
        let path = self.tcx.def_path(def_id);
        let last = path.data.len().checked_sub(1);
        let mut parts = Vec::new();
        for (index, component) in path.data.into_iter().enumerate() {
            if Some(index) == last {
                match associated {
                    Some(AssociatedPathKind::Named) => {
                        let name = self.tcx.item_name(def_id);
                        self.push_definition_path_part(&mut parts, name.as_str())?;
                        continue;
                    }
                    Some(AssociatedPathKind::AnonymousSupport(support)) => {
                        let segment = anonymous_associated_segment(
                            support.method.as_str(),
                            support.disambiguator,
                            self.budget,
                        )?;
                        self.push_owned_definition_path_part(&mut parts, segment)?;
                        continue;
                    }
                    None => {}
                }
            }
            if let Some(name) = component.data.get_opt_name() {
                self.push_definition_path_part(&mut parts, name.as_str())?;
            }
        }
        if parts.is_empty() {
            if !definition_has_item_name(self.tcx.def_kind(def_id)) {
                return Err("compiler definition path has unsupported unnamed kind".to_owned());
            }
            let name = self.tcx.item_name(def_id);
            self.push_definition_path_part(&mut parts, name.as_str())?;
        }
        Ok(parts)
    }

    fn push_definition_path_part(
        &mut self,
        parts: &mut Vec<String>,
        part: &str,
    ) -> Result<(), String> {
        let part = self.budget.owned(part)?;
        self.push_owned_definition_path_part(parts, part)
    }

    fn push_owned_definition_path_part(
        &mut self,
        parts: &mut Vec<String>,
        part: String,
    ) -> Result<(), String> {
        if parts.len() >= MAX_DEPTH {
            return Err("compiler definition path depth exceeded".to_owned());
        }
        self.edge(1)?;
        self.budget.items(1)?;
        parts
            .try_reserve(1)
            .map_err(|error| format!("definition path allocation failed: {error}"))?;
        parts.push(part);
        Ok(())
    }

    fn definition_path(&mut self, def_id: DefId) -> Result<Vec<String>, String> {
        let qualify = def_id.krate != self.selected;
        let mut parts = self.display_path(def_id)?;
        if qualify {
            if parts.len() >= MAX_DEPTH {
                return Err("compiler definition path depth exceeded".to_owned());
            }
            self.edge(1)?;
            let crate_name = self.tcx.crate_name(def_id.krate);
            self.budget.items(1)?;
            parts
                .try_reserve(1)
                .map_err(|error| format!("definition path allocation failed: {error}"))?;
            parts.insert(0, self.budget.owned(crate_name.as_str())?);
        }
        Ok(parts)
    }

    fn def_hash(&mut self, def_id: DefId) -> Result<String, String> {
        self.budget.string_length(32)?;
        def_hash(self.tcx, def_id)
    }

    fn primitive(&mut self, name: &str) -> Result<Type, String> {
        Ok(Type::Primitive {
            name: self.budget.owned(name)?,
        })
    }

    fn charge_strings(&mut self, values: &[String]) -> Result<(), String> {
        self.budget.items(values.len())?;
        for value in values {
            self.string(value)?;
        }
        Ok(())
    }

    fn charge_id(&mut self, id: &Id) -> Result<(), String> {
        let Authority::Compiler {
            crate_name,
            stable_crate_id,
        } = &id.authority;
        self.string(crate_name)?;
        self.string(stable_crate_id)?;
        self.string(&id.def_path_hash)
    }

    fn charge_span(&mut self, span: &Span) -> Result<(), String> {
        self.string(&span.file)
    }

    fn charge_reexports(&mut self, values: &[Reexport]) -> Result<(), String> {
        self.budget.items(values.len())?;
        for value in values {
            if let ReexportEvidence::Identified { item, span } = &value.evidence {
                for text in [
                    item.crate_name.as_str(),
                    item.stable_crate_id.as_str(),
                    item.def_path_hash.as_str(),
                ] {
                    self.string(text)?;
                }
                self.charge_span(span)?;
            }
        }
        Ok(())
    }

    fn charge_export_clone(
        &mut self,
        root_alias: &str,
        path: &[String],
        target: &ExportTarget,
        aliases: &[Alias],
        reexports: &[Reexport],
        span: &Span,
    ) -> Result<(), String> {
        self.budget.items(1)?;
        self.string(root_alias)?;
        self.charge_strings(path)?;
        match target {
            ExportTarget::Definition { id } => self.charge_id(id)?,
            ExportTarget::Unsupported { compiler_kind } => self.string(compiler_kind)?,
        }
        self.budget.items(aliases.len())?;
        for alias in aliases {
            self.charge_strings(&alias.from)?;
            self.charge_strings(&alias.to)?;
            self.charge_span(&alias.span)?;
        }
        self.charge_reexports(reexports)?;
        self.charge_span(span)
    }

    fn edge(&mut self, amount: usize) -> Result<(), String> {
        self.budget.edge(amount)
    }

    fn string(&mut self, value: &str) -> Result<(), String> {
        self.budget.string(value)
    }
}

/// The path segment a `$crate` of a macro defined in `krate` reads as,
/// after the root of the dependency the macro was imported through:
/// `__rawr_crate_<name>_<stable crate id>`, which `select_path` resolves to
/// that crate's root whether or not the dependency re-exports it.
fn crate_segment(tcx: TyCtxt<'_>, krate: CrateNum) -> String {
    format!(
        "{CRATE_SEGMENT}{}_{:016x}",
        tcx.crate_name(krate),
        tcx.stable_crate_id(krate).as_u64()
    )
}

const CRATE_SEGMENT: &str = "__rawr_crate_";

/// `macro_rules! name { .. }` at the start of `text`, through the close of
/// its body (and the `;` after a `(..)` or `[..]` body). Strings, chars and
/// comments do not count toward the delimiters. `None` for anything else.
fn macro_rules_source(text: &str) -> Option<&str> {
    let rest = text.strip_prefix("macro_rules")?.trim_start();
    let rest = rest.strip_prefix('!')?;
    let bytes = text.as_bytes();
    let mut index = text.len() - rest.len();
    // The name, then the body's opening delimiter.
    while index < bytes.len() && !matches!(bytes[index], b'{' | b'(' | b'[') {
        index += 1;
    }
    let open = *bytes.get(index)?;
    let mut depth = 0_usize;
    while index < bytes.len() {
        match bytes[index] {
            b'{' | b'(' | b'[' => depth += 1,
            b'}' | b')' | b']' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    let mut end = index + 1;
                    if open != b'{' {
                        let after = text[end..].trim_start();
                        if after.starts_with(';') {
                            end = text.len() - after.len() + 1;
                        }
                    }
                    return text.get(..end);
                }
            }
            b'/' if bytes.get(index + 1) == Some(&b'/') => {
                while index < bytes.len() && bytes[index] != b'\n' {
                    index += 1;
                }
            }
            b'/' if bytes.get(index + 1) == Some(&b'*') => {
                let mut nested = 0_usize;
                while index + 1 < bytes.len() {
                    if bytes[index] == b'/' && bytes[index + 1] == b'*' {
                        nested += 1;
                        index += 2;
                    } else if bytes[index] == b'*' && bytes[index + 1] == b'/' {
                        nested -= 1;
                        index += 2;
                        if nested == 0 {
                            break;
                        }
                    } else {
                        index += 1;
                    }
                }
                continue;
            }
            b'"' => {
                // A raw string ends at a quote followed by its hashes, as
                // `r#".."#` does; any other string at an unescaped quote.
                let hashes = text[..index]
                    .bytes()
                    .rev()
                    .take_while(|byte| *byte == b'#')
                    .count();
                let raw = text[..index - hashes].ends_with('r');
                index += 1;
                while index < bytes.len() {
                    if !raw && bytes[index] == b'\\' {
                        index += 2;
                        continue;
                    }
                    if bytes[index] == b'"'
                        && bytes.len() > index + hashes
                        && bytes[index + 1..=index + hashes]
                            .iter()
                            .all(|byte| *byte == b'#')
                    {
                        index += hashes;
                        break;
                    }
                    index += 1;
                }
            }
            // A char literal; a lifetime's `'` has no closing quote.
            b'\'' => {
                if bytes.get(index + 1) == Some(&b'\\') {
                    index += 2;
                    while index < bytes.len() && bytes[index] != b'\'' {
                        index += 1;
                    }
                } else if let Some(width) = text[index + 1..].chars().next().map(char::len_utf8)
                    && bytes.get(index + 1 + width) == Some(&b'\'')
                {
                    index += 1 + width;
                }
            }
            _ => {}
        }
        index += 1;
    }
    None
}

fn def_hash(tcx: TyCtxt<'_>, def_id: DefId) -> Result<String, String> {
    let raw = tcx.def_path_hash(def_id).to_raw_def_path_hash();
    hex(&raw.0)
}

fn associated_path_kind(
    tcx: TyCtxt<'_>,
    def_id: DefId,
) -> Result<Option<AssociatedPathKind>, String> {
    if tcx.def_kind(def_id) != DefKind::AssocTy {
        return Ok(None);
    }
    let path = tcx.def_path(def_id);
    let leaf = path
        .data
        .last()
        .ok_or_else(|| "associated type has no compiler path component".to_owned())?;
    match leaf.data {
        DefPathData::TypeNs(_) => Ok(Some(AssociatedPathKind::Named)),
        DefPathData::AnonAssocTy(method) => {
            let parent = tcx
                .opt_parent(def_id)
                .ok_or_else(|| "anonymous associated type has no compiler parent".to_owned())?;
            if !matches!(tcx.def_kind(parent), DefKind::Trait | DefKind::Impl { .. }) {
                return Err(
                    "anonymous associated type has an unsupported compiler parent".to_owned(),
                );
            }
            Ok(Some(AssociatedPathKind::AnonymousSupport(
                AnonymousAssociatedSupport {
                    parent,
                    method,
                    disambiguator: leaf.disambiguator,
                },
            )))
        }
        _ => Err("associated type has unsupported compiler path data".to_owned()),
    }
}

const fn is_associated_definition_kind(kind: DefKind) -> bool {
    matches!(
        kind,
        DefKind::AssocFn | DefKind::AssocConst { .. } | DefKind::AssocTy
    )
}

fn anonymous_associated_segment(
    method: &str,
    disambiguator: u32,
    budget: &mut Budget,
) -> Result<String, String> {
    if method.is_empty() {
        return Err("anonymous associated type has an empty method anchor".to_owned());
    }
    budget.formatted(format_args!(
        "{{anonymous-associated-type:{method}#{disambiguator}}}"
    ))
}

fn definition_has_item_name(kind: DefKind) -> bool {
    matches!(
        kind,
        DefKind::Mod
            | DefKind::Struct
            | DefKind::Union
            | DefKind::Enum
            | DefKind::Variant
            | DefKind::Trait
            | DefKind::TyAlias
            | DefKind::ForeignTy
            | DefKind::TraitAlias
            | DefKind::AssocTy
            | DefKind::TyParam
            | DefKind::Fn
            | DefKind::Const { .. }
            | DefKind::ConstParam
            | DefKind::Static { .. }
            | DefKind::AssocFn
            | DefKind::AssocConst { .. }
            | DefKind::Macro(..)
            | DefKind::LifetimeParam
            | DefKind::ExternCrate
            | DefKind::Field
    )
}

fn namespace(namespace: RustNamespace) -> Namespace {
    match namespace {
        RustNamespace::TypeNS => Namespace::Type,
        RustNamespace::ValueNS => Namespace::Value,
        RustNamespace::MacroNS => Namespace::Macro,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OpaqueArgumentAction<I> {
    SelfReference(I),
    ConvertNormally,
}

fn opaque_argument_action<I: Copy + Eq>(
    active: Option<I>,
    argument: Option<I>,
) -> OpaqueArgumentAction<I> {
    match (active, argument) {
        (Some(active), Some(argument)) if active == argument => {
            OpaqueArgumentAction::SelfReference(active)
        }
        _ => OpaqueArgumentAction::ConvertNormally,
    }
}

const fn implementation_crate_authority(is_sysroot: bool) -> ImplementationCrateAuthority {
    if is_sysroot {
        ImplementationCrateAuthority::Sysroot
    } else {
        ImplementationCrateAuthority::External
    }
}

fn opaque_identity(ty: Ty<'_>) -> Option<DefId> {
    match ty.kind() {
        ty::Alias(_, alias) => match alias.kind {
            ty::AliasTyKind::Opaque { def_id } => Some(def_id),
            ty::AliasTyKind::Projection { .. }
            | ty::AliasTyKind::Inherent { .. }
            | ty::AliasTyKind::Free { .. } => None,
        },
        _ => None,
    }
}

fn check_type_depth(depth: usize) -> Result<(), String> {
    if depth > MAX_DEPTH {
        Err("compiler type depth exceeded".to_owned())
    } else {
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ParameterClass {
    Lifetime,
    Type,
    Const,
}

struct ResolvedParameter {
    owner: Id,
    self_type: bool,
}

fn parameter_class(kind: &ty::GenericParamDefKind) -> ParameterClass {
    match kind {
        ty::GenericParamDefKind::Lifetime => ParameterClass::Lifetime,
        ty::GenericParamDefKind::Type { .. } => ParameterClass::Type,
        ty::GenericParamDefKind::Const { .. } => ParameterClass::Const,
    }
}

fn validate_parameter_class(
    actual: Option<ParameterClass>,
    expected: ParameterClass,
) -> Result<(), String> {
    match actual {
        Some(actual) if actual == expected => Ok(()),
        Some(_) => Err("generic parameter kind disagrees with compiler use".to_owned()),
        None => Err("generic parameter owner is absent".to_owned()),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SelectedSurfaceWork {
    StructLike,
    Enum,
    Typed,
    Function,
    Trait,
    AssociatedTypeDefault,
    Noop,
    Reject,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ImplementationWork {
    Trait,
    Nominal,
    Noop,
    Reject,
}

fn selected_surface_work(kind: DefKind, associated_default: bool) -> SelectedSurfaceWork {
    match kind {
        DefKind::Struct | DefKind::Union => SelectedSurfaceWork::StructLike,
        DefKind::Enum => SelectedSurfaceWork::Enum,
        DefKind::TyAlias
        | DefKind::Const { .. }
        | DefKind::AssocConst { .. }
        | DefKind::Static { .. } => SelectedSurfaceWork::Typed,
        DefKind::Fn | DefKind::AssocFn => SelectedSurfaceWork::Function,
        DefKind::Trait => SelectedSurfaceWork::Trait,
        DefKind::AssocTy if associated_default => SelectedSurfaceWork::AssociatedTypeDefault,
        DefKind::AssocTy
        | DefKind::Mod
        | DefKind::Variant
        | DefKind::ForeignTy
        | DefKind::TraitAlias
        | DefKind::TyParam
        | DefKind::ConstParam
        | DefKind::Ctor(..)
        | DefKind::Macro(..)
        | DefKind::LifetimeParam
        | DefKind::ExternCrate
        | DefKind::Use
        | DefKind::ForeignMod
        | DefKind::AnonConst
        | DefKind::InlineConst
        | DefKind::OpaqueTy
        | DefKind::GlobalAsm
        | DefKind::Field => SelectedSurfaceWork::Noop,
        DefKind::Impl { .. } | DefKind::Closure | DefKind::SyntheticCoroutineBody => {
            SelectedSurfaceWork::Reject
        }
    }
}

fn implementation_work(kind: DefKind) -> ImplementationWork {
    match kind {
        DefKind::Trait => ImplementationWork::Trait,
        DefKind::Struct | DefKind::Union | DefKind::Enum | DefKind::ForeignTy => {
            ImplementationWork::Nominal
        }
        DefKind::Mod
        | DefKind::Variant
        | DefKind::TyAlias
        | DefKind::TraitAlias
        | DefKind::AssocTy
        | DefKind::TyParam
        | DefKind::Fn
        | DefKind::Const { .. }
        | DefKind::ConstParam
        | DefKind::Static { .. }
        | DefKind::Ctor(..)
        | DefKind::AssocFn
        | DefKind::AssocConst { .. }
        | DefKind::Macro(..)
        | DefKind::LifetimeParam
        | DefKind::ExternCrate
        | DefKind::Use
        | DefKind::ForeignMod
        | DefKind::AnonConst
        | DefKind::InlineConst
        | DefKind::OpaqueTy
        | DefKind::GlobalAsm
        | DefKind::Field => ImplementationWork::Noop,
        DefKind::Impl { .. } | DefKind::Closure | DefKind::SyntheticCoroutineBody => {
            ImplementationWork::Reject
        }
    }
}

fn definition_namespace(kind: DefKind) -> Namespace {
    match kind {
        DefKind::Mod
        | DefKind::Struct
        | DefKind::Union
        | DefKind::Enum
        | DefKind::Variant
        | DefKind::Trait
        | DefKind::TyAlias
        | DefKind::ForeignTy
        | DefKind::TraitAlias
        | DefKind::AssocTy
        | DefKind::TyParam
        | DefKind::OpaqueTy
        | DefKind::Impl { .. }
        | DefKind::Field => Namespace::Type,
        DefKind::Fn
        | DefKind::Const { .. }
        | DefKind::ConstParam
        | DefKind::Static { .. }
        | DefKind::Ctor(..)
        | DefKind::AssocFn
        | DefKind::AssocConst { .. }
        | DefKind::AnonConst
        | DefKind::InlineConst
        | DefKind::Closure
        | DefKind::SyntheticCoroutineBody => Namespace::Value,
        DefKind::Macro(..) => Namespace::Macro,
        DefKind::LifetimeParam
        | DefKind::ExternCrate
        | DefKind::Use
        | DefKind::ForeignMod
        | DefKind::GlobalAsm => Namespace::Type,
    }
}

fn safety(value: hir::Safety) -> Safety {
    match value {
        hir::Safety::Safe => Safety::Safe,
        hir::Safety::Unsafe => Safety::Unsafe,
    }
}

fn mutability_of(value: hir::Mutability) -> Mutability {
    match value {
        hir::Mutability::Not => Mutability::Shared,
        hir::Mutability::Mut => Mutability::Mutable,
    }
}

fn primitive_name(ty: Ty<'_>) -> Option<&'static str> {
    match ty.kind() {
        ty::Bool => Some("bool"),
        ty::Char => Some("char"),
        ty::Int(kind) => Some(kind.name_str()),
        ty::Uint(kind) => Some(kind.name_str()),
        ty::Float(kind) => Some(kind.name_str()),
        ty::Str => Some("str"),
        unsupported => {
            let _unsupported = unsupported;
            None
        }
    }
}

fn def_kind_text(kind: DefKind) -> &'static str {
    match kind {
        DefKind::Mod => "module",
        DefKind::Struct => "struct",
        DefKind::Union => "union",
        DefKind::Enum => "enum",
        DefKind::Variant => "variant",
        DefKind::Trait => "trait",
        DefKind::TyAlias => "type-alias",
        DefKind::ForeignTy => "foreign-type",
        DefKind::TraitAlias => "trait-alias",
        DefKind::AssocTy => "associated-type",
        DefKind::TyParam => "type-parameter",
        DefKind::Fn => "function",
        DefKind::Const { .. } => "constant",
        DefKind::ConstParam => "const-parameter",
        DefKind::Static { .. } => "static",
        DefKind::Ctor(..) => "constructor",
        DefKind::AssocFn => "associated-function",
        DefKind::AssocConst { .. } => "associated-constant",
        DefKind::Macro(..) => "macro",
        DefKind::LifetimeParam => "lifetime-parameter",
        DefKind::ExternCrate => "extern-crate",
        DefKind::Use => "use",
        DefKind::ForeignMod => "foreign-module",
        DefKind::AnonConst => "anonymous-constant",
        DefKind::InlineConst => "inline-constant",
        DefKind::OpaqueTy => "opaque-type",
        DefKind::GlobalAsm => "global-assembly",
        DefKind::Impl { .. } => "implementation",
        DefKind::Closure => "closure",
        DefKind::SyntheticCoroutineBody => "synthetic-coroutine-body",
        DefKind::Field => "field",
    }
}

fn ty_kind_text(kind: &ty::TyKind<'_>) -> &'static str {
    match kind {
        ty::Bool => "bool",
        ty::Char => "char",
        ty::Int(_) => "integer",
        ty::Uint(_) => "unsigned-integer",
        ty::Float(_) => "float",
        ty::Adt(..) => "adt",
        ty::Foreign(_) => "foreign",
        ty::Str => "str",
        ty::Array(..) => "array",
        ty::Pat(..) => "pattern-type",
        ty::Slice(_) => "slice",
        ty::RawPtr(..) => "raw-pointer",
        ty::Ref(..) => "reference",
        ty::FnDef(..) => "function-definition",
        ty::FnPtr(..) => "function-pointer",
        ty::UnsafeBinder(_) => "unsafe-binder",
        ty::Dynamic(..) => "trait-object",
        ty::Closure(..) => "closure",
        ty::CoroutineClosure(..) => "coroutine-closure",
        ty::Coroutine(..) => "coroutine",
        ty::CoroutineWitness(..) => "coroutine-witness",
        ty::Never => "never",
        ty::Tuple(_) => "tuple",
        ty::Alias(..) => "alias",
        ty::Param(_) => "parameter",
        ty::Bound(..) => "bound",
        ty::Placeholder(_) => "placeholder",
        ty::Infer(_) => "inference",
        ty::Error(_) => "error",
    }
}

fn const_kind_text(kind: ty::ConstKind<'_>) -> &'static str {
    match kind {
        ty::ConstKind::Param(_) => "parameter",
        ty::ConstKind::Infer(_) => "inference",
        ty::ConstKind::Bound(..) => "bound",
        ty::ConstKind::Placeholder(_) => "placeholder",
        ty::ConstKind::Alias(..) => "alias",
        ty::ConstKind::Value(_) => "value",
        ty::ConstKind::Expr(_) => "expression",
        ty::ConstKind::Error(_) => "error",
    }
}

fn arguments_digest(arguments: &[String], budget: &mut Budget) -> Result<String, String> {
    let mut digest = Sha256::new();
    for argument in arguments {
        let length = argument.len();
        if length > MAX_STRING {
            return Err("rustc argument exceeds the string limit".to_owned());
        }
        let length =
            u64::try_from(length).map_err(|_| "rustc argument length exceeds u64".to_owned())?;
        digest.update(length.to_le_bytes());
        digest.update(argument.as_bytes());
    }
    budget.hexadecimal(&digest.finalize())
}

fn canonical_path(path: &Path, label: &str, budget: &mut Budget) -> Result<PathBuf, String> {
    budget.path_capacity(super::CANONICAL_PATH_BYTES)?;
    let canonical = fs::canonicalize(path)
        .map_err(|error| format!("failed to canonicalize {label}: {error}"))?;
    budget.validate_path(&canonical, super::CANONICAL_PATH_BYTES)?;
    Ok(canonical)
}

fn join_path(parent: &Path, child: &Path, budget: &mut Budget) -> Result<PathBuf, String> {
    let maximum = parent
        .as_os_str()
        .len()
        .saturating_add(1)
        .saturating_add(child.as_os_str().len());
    budget.path_capacity(maximum)?;
    let path = parent.join(child);
    budget.validate_path(&path, maximum)?;
    Ok(path)
}

/// Write `{"handshake":…,` and the body's fields: the bytes serializing both
/// as one struct gives.
fn publish(
    directory: &Path,
    handshake: &Handshake,
    body: &[u8],
    budget: &mut Budget,
) -> Result<(), String> {
    let correlation = &handshake.unit_correlation;
    let fields = body
        .strip_prefix(b"{")
        .ok_or_else(|| "compiler sidecar body is not an object".to_owned())?;
    if correlation.len() != 64
        || !correlation
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("invalid planned sidecar correlation".to_owned());
    }
    let destination_name = budget.formatted(format_args!("{correlation}.json"))?;
    let destination = join_path(directory, Path::new(&destination_name), budget)?;
    let temporary_name =
        budget.formatted(format_args!(".{correlation}.tmp-{}", std::process::id()))?;
    let temporary = join_path(directory, Path::new(&temporary_name), budget)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|error| format!("failed to create sidecar temporary: {error}"))?;
    // serde_json emits one small write per token; buffer them so the sidecar
    // costs a few large writes instead of millions of system calls.
    let mut bounded = Bounded {
        file: io::BufWriter::with_capacity(1 << 20, &mut file),
        written: 0,
    };
    bounded
        .write_all(b"{\"handshake\":")
        .map_err(|error| format!("failed to write compiler sidecar: {error}"))?;
    serde_json::to_writer(&mut bounded, handshake)
        .map_err(|error| format!("failed to encode compiler sidecar: {error}"))?;
    bounded
        .write_all(b",")
        .and_then(|()| bounded.write_all(fields))
        .map_err(|error| format!("failed to write compiler sidecar: {error}"))?;
    bounded
        .file
        .flush()
        .map_err(|error| format!("failed to write compiler sidecar: {error}"))?;
    drop(bounded);
    file.sync_all()
        .map_err(|error| format!("failed to sync compiler sidecar: {error}"))?;
    fs::hard_link(&temporary, &destination)
        .map_err(|error| format!("failed to atomically publish compiler sidecar: {error}"))?;
    fs::remove_file(&temporary)
        .map_err(|error| format!("failed to retire compiler sidecar temporary: {error}"))
}

struct Bounded<'a> {
    file: io::BufWriter<&'a mut fs::File>,
    written: usize,
}

impl Write for Bounded<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let actual = self.written.saturating_add(bytes.len());
        if actual > MAX_BYTES {
            return Err(io::Error::other("compiler sidecar byte limit exceeded"));
        }
        let count = self.file.write(bytes)?;
        self.written = self.written.saturating_add(count);
        Ok(count)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

fn checked_string(value: &str) -> Result<(), String> {
    if value.is_empty() || value.len() > MAX_STRING {
        Err("compiler string is empty or exceeds the protocol limit".to_owned())
    } else {
        Ok(())
    }
}

fn hex(bytes: &[u8]) -> Result<String, String> {
    let length = bytes.len().saturating_mul(2);
    let mut encoded = String::new();
    encoded
        .try_reserve_exact(length)
        .map_err(|error| format!("compiler hexadecimal allocation failed: {error}"))?;
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(encoded, "{byte:02x}");
    }
    Ok(encoded)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::PathBuf;

    use super::{
        anonymous_associated_segment, check_type_depth, definition_has_item_name,
        effective_associated_item, implementation_crate_authority, implementation_work,
        opaque_argument_action, selected_surface_work, take_owned_candidate_bucket,
        validate_parameter_class, Artifact, Budget, Demand, Diagnostic, DiagnosticIdentity,
        ImplementationCrateAuthority, ImplementationWork, Invocation, Namespace,
        OpaqueArgumentAction, Package, ParameterClass, Root, ScheduleState, Scheduler,
        SelectedSurfaceWork, DIAGNOSTIC_SLOTS, DIAGNOSTIC_SLOT_BYTES, MAX_DEPTH, MAX_EDGES,
    };
    use rustc_hir::def::DefKind;

    #[test]
    fn buffered_sidecar_writer_keeps_bytes_and_counts_every_token() {
        use std::io::Write as _;
        let path = std::env::temp_dir().join(format!("rawr-sidecar-writer-{}", std::process::id()));
        let mut file = std::fs::File::create(&path).unwrap();
        let mut bounded = super::Bounded {
            file: std::io::BufWriter::with_capacity(1 << 20, &mut file),
            written: 0,
        };
        let fragment = serde_json::json!({"name": "a", "items": [1, 2, 3]});
        let value = serde_json::json!({ "fragments": vec![fragment; 1000] });
        serde_json::to_writer(&mut bounded, &value).unwrap();
        bounded.file.flush().unwrap();
        let written = bounded.written;
        drop(bounded);
        let expected = serde_json::to_vec(&value).unwrap();
        assert_eq!(written, expected.len());
        assert_eq!(std::fs::read(&path).unwrap(), expected);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn completion_binding_uses_the_original_budget_and_one_directory() {
        let mut row = invocation(&["fixture"]);
        let digest = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        row.invocation_nonce = digest.to_owned();
        row.unit_correlation = digest.to_owned();
        let mut measured = Budget::standard();
        super::completion::Prepared::bind(std::slice::from_ref(&row), digest, &mut measured)
            .unwrap();
        let (bytes, _, _) = measured.usage();
        let mut exact = Budget::with_limits(bytes + 7, 0);
        exact.bytes(7).unwrap();
        exact.items(2).unwrap();
        super::completion::Prepared::bind(std::slice::from_ref(&row), digest, &mut exact).unwrap();
        let mut short = Budget::with_limits(bytes + 6, 0);
        short.bytes(7).unwrap();
        short.items(2).unwrap();
        assert!(
            super::completion::Prepared::bind(std::slice::from_ref(&row), digest, &mut short)
                .is_err()
        );
        let mut other = invocation(&["other"]);
        other.invocation_nonce = digest.to_owned();
        other.unit_correlation = digest.to_owned();
        other.protocol_directory = PathBuf::from("other");
        assert!(
            super::completion::Prepared::bind(&[row, other], digest, &mut Budget::standard())
                .is_err()
        );
    }

    #[test]
    fn pinned_sysroot_search_selection_tracks_lib64_existence() {
        // Test the actual linked compiler configuration, not our copy of its
        // source: CFG_LIBDIR_RELATIVE can replace the fallback algorithm.
        let root =
            std::env::temp_dir().join(format!("rawr-sysroot-selection-{}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let target = "aarch64-apple-darwin";
        let selected = || rustc_session::filesearch::make_target_lib_path(&root, target);
        assert_eq!(
            selected(),
            root.join("lib/rustlib").join(target).join("lib")
        );
        let primary = root.join("lib64/rustlib");
        std::fs::create_dir_all(&primary).unwrap();
        assert_eq!(
            selected(),
            root.join("lib64/rustlib").join(target).join("lib")
        );
        std::fs::remove_dir(&primary).unwrap();
        // The selection tests exists(), not is_dir(): preserve this edge in any
        // parent-side determinant proof rather than silently preferring lib.
        std::fs::write(&primary, b"not a directory").unwrap();
        assert_eq!(
            selected(),
            root.join("lib64/rustlib").join(target).join("lib")
        );
        std::fs::remove_file(&primary).unwrap();
        assert_eq!(
            selected(),
            root.join("lib/rustlib").join(target).join("lib")
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn compiler_run_publishes_completion_only_after_success() {
        const CHILD: &str = "RAWR_COMPLETION_RUN_TEST";
        let Some(mode) = std::env::var_os(CHILD) else {
            for mode in ["success", "diagnostic", "failure"] {
                let status = std::process::Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "compiler::tests::compiler_run_publishes_completion_only_after_success",
                        "--nocapture",
                    ])
                    .env(CHILD, mode)
                    .status()
                    .unwrap();
                assert!(status.success(), "mode {mode}");
            }
            return;
        };
        let root = std::env::temp_dir().join(format!("rawr-completion-run-{}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let root = std::fs::canonicalize(root).unwrap();
        let library = root.join("library.rs");
        std::fs::write(&library, "#![no_std]\npub struct Example;\n").unwrap();
        let artifact = root.join("libfixture.rmeta");
        assert!(std::process::Command::new("rustc")
            .args([
                "--crate-name=fixture",
                "--crate-type=rlib",
                "--emit=metadata"
            ])
            .arg(&library)
            .arg("-o")
            .arg(&artifact)
            .status()
            .unwrap()
            .success());
        let source = root.join("probe.rs");
        std::fs::write(
            &source,
            if mode != "failure" {
                "#![no_std]\npub struct Probe;"
            } else {
                "invalid syntax !"
            },
        )
        .unwrap();
        let protocol = root.join("protocol");
        std::fs::create_dir(&protocol).unwrap();
        let identity = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let mut row = invocation(&["fixture"]);
        row.invocation_nonce = identity.to_owned();
        row.unit_correlation = identity.to_owned();
        row.protocol_directory = protocol.clone();
        row.artifact.path = artifact.to_str().unwrap().to_owned();
        row.artifact.length = std::fs::metadata(&artifact).unwrap().len();
        row.artifact.digest = identity.to_owned();
        row.artifact.probe_alias = "fixture".to_owned();
        let arguments = vec![
            "rustc".to_owned(),
            "--crate-name=probe".to_owned(),
            "--crate-type=rlib".to_owned(),
            "--emit=metadata".to_owned(),
            "-Zunstable-options".to_owned(),
            format!("--extern=force,noprelude:fixture={}", artifact.display()),
            source.to_str().unwrap().to_owned(),
        ];
        let digest = super::arguments_digest(&arguments, &mut Budget::standard()).unwrap();
        let previous_directory = std::env::current_dir().unwrap();
        std::env::set_current_dir(&root).unwrap();
        let result = super::run(
            arguments,
            vec![row],
            Budget::standard(),
            mode == "diagnostic",
            None,
        );
        std::env::set_current_dir(previous_directory).unwrap();
        if mode != "failure" {
            assert_eq!(result.unwrap(), std::process::ExitCode::SUCCESS);
            let bytes = std::fs::read(protocol.join("attempts")).unwrap();
            crate::attempt_protocol::Completion::decode(&bytes)
                .unwrap()
                .require_invocation(
                    &digest,
                    [crate::attempt_protocol::Invocation {
                        nonce: identity,
                        correlation: identity,
                    }],
                )
                .unwrap();
            assert!(protocol.join(format!("{identity}.json")).is_file());
        } else {
            assert!(!matches!(result, Ok(code) if code == std::process::ExitCode::SUCCESS));
            assert!(!protocol.join("attempts").exists());
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn an_unchanged_crate_reuses_its_extraction_and_a_changed_one_is_extracted() {
        use sha2::Digest as _;
        const CHILD: &str = "RAWR_EXTRACTION_REUSE_TEST";
        const ROOT: &str = "RAWR_EXTRACTION_REUSE_ROOT";
        let Some(step) = std::env::var_os(CHILD) else {
            let root =
                std::env::temp_dir().join(format!("rawr-extraction-reuse-{}", std::process::id()));
            std::fs::create_dir(&root).unwrap();
            let root = std::fs::canonicalize(root).unwrap();
            std::fs::create_dir(root.join("cache")).unwrap();
            // One compiler session per process: each step is a probe run in
            // its own bundle-like directory, sharing the reuse directory.
            for step in ["first", "unchanged", "changed"] {
                let status = std::process::Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "compiler::tests::an_unchanged_crate_reuses_its_extraction_and_a_changed_one_is_extracted",
                        "--nocapture",
                    ])
                    .env(CHILD, step)
                    .env(ROOT, &root)
                    .status()
                    .unwrap();
                assert!(status.success(), "step {step}");
            }
            std::fs::remove_dir_all(root).unwrap();
            return;
        };
        let step = step.to_str().unwrap();
        let root = PathBuf::from(std::env::var_os(ROOT).unwrap());
        let cache = root.join("cache");
        let bundle = root.join(step);
        std::fs::create_dir(&bundle).unwrap();
        let artifact = bundle.join("libfixture.rmeta");
        let library = root.join("library.rs");
        if step == "unchanged" {
            // The relocated clone of an unchanged unit: same bytes, new path.
            std::fs::copy(root.join("first/libfixture.rmeta"), &artifact).unwrap();
        } else {
            let item = if step == "first" {
                "Example"
            } else {
                "Changed"
            };
            std::fs::write(&library, format!("#![no_std]\npub struct {item};\n")).unwrap();
            assert!(std::process::Command::new("rustc")
                .args([
                    "--crate-name=fixture",
                    "--crate-type=rlib",
                    "--emit=metadata"
                ])
                .arg(&library)
                .arg("-o")
                .arg(&artifact)
                .status()
                .unwrap()
                .success());
        }
        let source = bundle.join("probe.rs");
        std::fs::write(&source, "#![no_std]\npub struct Probe;").unwrap();
        let protocol = bundle.join("protocol");
        std::fs::create_dir(&protocol).unwrap();
        let identity = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let bytes = std::fs::read(&artifact).unwrap();
        let mut row = invocation(&["fixture"]);
        row.invocation_nonce = identity.to_owned();
        row.unit_correlation = identity.to_owned();
        row.protocol_directory = protocol.clone();
        row.artifact.path = artifact.to_str().unwrap().to_owned();
        row.artifact.length = bytes.len() as u64;
        row.artifact.digest = super::hex(&sha2::Sha256::digest(&bytes)).unwrap();
        row.artifact.probe_alias = "fixture".to_owned();
        let arguments = vec![
            "rustc".to_owned(),
            "--crate-name=probe".to_owned(),
            "--crate-type=rlib".to_owned(),
            "--emit=metadata".to_owned(),
            "-Zunstable-options".to_owned(),
            format!("--extern=force,noprelude:fixture={}", artifact.display()),
            source.to_str().unwrap().to_owned(),
        ];
        let previous_directory = std::env::current_dir().unwrap();
        std::env::set_current_dir(&bundle).unwrap();
        let result = super::run(
            arguments,
            vec![row],
            Budget::standard(),
            false,
            Some(cache.clone()),
        );
        std::env::set_current_dir(previous_directory).unwrap();
        assert_eq!(result.unwrap(), std::process::ExitCode::SUCCESS);
        let sidecar = std::fs::read_to_string(protocol.join(format!("{identity}.json"))).unwrap();
        let kept: Vec<_> = std::fs::read_dir(&cache)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        assert_eq!(kept.len(), 1, "one body per probed crate: {kept:?}");
        assert!(std::fs::metadata(&kept[0])
            .unwrap()
            .permissions()
            .readonly());
        // The sidecar names this bundle's artifact, never the kept one's.
        assert!(sidecar.contains(&format!("\"path\":\"{}\"", artifact.display())));
        match step {
            "first" => {
                assert!(sidecar.contains("\"Example\""));
                // Mark the kept body, so only reuse can carry the mark into
                // the next sidecar, and reseal its digest.
                let mut kept_bytes = std::fs::read(&kept[0]).unwrap();
                let split = kept_bytes.iter().rposition(|byte| *byte == b'\n').unwrap();
                let mut header: serde_json::Value =
                    serde_json::from_slice(&kept_bytes[split + 1..]).unwrap();
                kept_bytes.truncate(split);
                let body = String::from_utf8(kept_bytes)
                    .unwrap()
                    .replace("\"Example\"", "\"Exampel\"");
                header["digest"] = super::hex(&sha2::Sha256::digest(body.as_bytes()))
                    .unwrap()
                    .into();
                std::fs::remove_file(&kept[0]).unwrap();
                std::fs::write(&kept[0], format!("{body}\n{header}")).unwrap();
                let mut permissions = std::fs::metadata(&kept[0]).unwrap().permissions();
                permissions.set_readonly(true);
                std::fs::set_permissions(&kept[0], permissions).unwrap();
                std::fs::write(root.join("kept"), kept[0].to_str().unwrap()).unwrap();
            }
            "unchanged" => {
                assert!(sidecar.contains("\"Exampel\""), "the kept body is reused");
                assert_eq!(
                    kept[0].to_str().unwrap(),
                    std::fs::read_to_string(root.join("kept")).unwrap()
                );
            }
            _ => {
                assert!(
                    sidecar.contains("\"Changed\""),
                    "a changed crate is extracted"
                );
                assert!(!sidecar.contains("Exampel"));
                assert_ne!(
                    kept[0].to_str().unwrap(),
                    std::fs::read_to_string(root.join("kept")).unwrap(),
                    "the stale body is retired"
                );
            }
        }
    }

    fn invocation(aliases: &[&str]) -> Invocation {
        Invocation {
            package: Package {
                cargo_id: "fixture 0.1.0".to_owned(),
                name: "fixture".to_owned(),
                version: "0.1.0".to_owned(),
                source: "path+fixture".to_owned(),
            },
            root: Root::Selected {
                aliases: aliases.iter().map(|alias| (*alias).to_owned()).collect(),
            },
            demand: Vec::new(),
            unit_key: "unit".to_owned(),
            dependencies: Vec::new(),
            target: "src/lib.rs".to_owned(),
            target_kind: "lib".to_owned(),
            target_kinds: Vec::new(),
            crate_types: Vec::new(),
            crate_name: "fixture".to_owned(),
            edition: "2024".to_owned(),
            profile: "dev".to_owned(),
            host: "aarch64-apple-darwin".to_owned(),
            selected_target: "aarch64-apple-darwin".to_owned(),
            features: Vec::new(),
            invocation_nonce: "nonce".to_owned(),
            unit_correlation: "correlation".to_owned(),
            compiler_commit: "commit".to_owned(),
            protocol_directory: PathBuf::from("protocol"),
            artifact: Artifact {
                path: "artifact.rmeta".to_owned(),
                length: 1,
                digest: "digest".to_owned(),
                profile: "dev".to_owned(),
                fresh: false,
                probe_alias: "probe".to_owned(),
            },
        }
    }

    #[test]
    fn allocation_budget_rejects_before_owned_string_growth() {
        let mut budget = Budget::with_limits(3, 8);
        assert_eq!(budget.owned("abc").expect("the exact byte boundary"), "abc");
        assert!(budget.owned("d").is_err());
    }

    #[test]
    fn edge_budget_rejects_at_the_aggregate_boundary() {
        let mut budget = Budget::with_limits(8, 2);
        budget.edge(2).expect("the exact edge boundary");
        assert!(budget.edge(1).is_err());
    }

    #[test]
    fn production_edge_budget_accepts_one_million_and_rejects_the_next_edge() {
        let mut budget = Budget::standard();
        budget
            .edge(MAX_EDGES)
            .expect("the production edge boundary remains accepted");
        assert_eq!(budget.usage().2, 1_000_000);
        assert_eq!(
            budget.edge(1),
            Err("compiler API edge limit exceeded".to_owned())
        );
        assert_eq!(budget.usage().2, 1_000_001);
    }

    #[test]
    fn anonymous_associated_support_segments_are_structured_stable_and_bounded() {
        let mut budget = Budget::with_limits(usize::MAX, usize::MAX);
        let first = anonymous_associated_segment("new", 0, &mut budget)
            .expect("the first RPITIT support segment is exact");
        let repeated = anonymous_associated_segment("new", 0, &mut budget)
            .expect("the same metadata produces the same segment");
        let sibling = anonymous_associated_segment("new", 1, &mut budget)
            .expect("the sibling disambiguator produces a segment");
        let method = anonymous_associated_segment("screenshot", 0, &mut budget)
            .expect("the method anchor contributes to the segment");
        assert_eq!(first, "{anonymous-associated-type:new#0}");
        assert_eq!(first, repeated);
        assert_ne!(first, sibling);
        assert_ne!(first, method);
        assert!(anonymous_associated_segment("", 0, &mut budget).is_err());
        assert!(definition_has_item_name(DefKind::AssocTy));
        assert!(!definition_has_item_name(DefKind::Impl { of_trait: true }));
        assert!(!definition_has_item_name(DefKind::OpaqueTy));
    }

    #[test]
    fn selected_trait_member_uses_implementor_then_default_and_rejects_missing_required_item() {
        assert_eq!(
            effective_associated_item("trait-required", Some("impl-required"), false),
            Ok("impl-required")
        );
        assert_eq!(
            effective_associated_item("trait-default", None, true),
            Ok("trait-default")
        );
        assert_eq!(
            effective_associated_item("trait-required", None, false),
            Err("selected required trait item has no implementor or default".to_owned())
        );
    }

    #[test]
    fn implementation_authority_keeps_sysroot_out_of_external_registration() {
        assert_eq!(
            implementation_crate_authority(true),
            ImplementationCrateAuthority::Sysroot
        );
        assert_eq!(
            implementation_crate_authority(false),
            ImplementationCrateAuthority::External
        );
    }

    #[test]
    fn traversal_scheduler_drains_a_deep_chain_without_call_stack_growth() {
        const DEPTH: u32 = 100_000;
        let mut budget = Budget::with_limits(usize::MAX, usize::MAX);
        let mut scheduler = Scheduler::new();
        scheduler
            .request(0_u32, 0_u32, Demand::ShapeOnly, &mut budget)
            .expect("the root is queued");
        let mut completed = 0_u32;
        while let Some((key, value, demand)) = scheduler.next().expect("queued work remains valid")
        {
            assert_eq!(key, value);
            if value < DEPTH {
                scheduler
                    .request(value + 1, value + 1, Demand::ShapeOnly, &mut budget)
                    .expect("the next identity is queued");
            }
            scheduler
                .complete(key, demand, &mut budget)
                .expect("the active identity completes");
            completed += 1;
        }
        assert_eq!(completed, DEPTH + 1);
    }

    #[test]
    fn traversal_scheduler_coalesces_active_cycles() {
        let mut budget = Budget::with_limits(usize::MAX, usize::MAX);
        let mut scheduler = Scheduler::new();
        scheduler
            .request(7_u32, 7_u32, Demand::ShapeOnly, &mut budget)
            .expect("the cycle root is queued");
        let (key, _, demand) = scheduler
            .next()
            .expect("the queue is valid")
            .expect("the root is active");
        scheduler
            .request(7_u32, 7_u32, Demand::ShapeOnly, &mut budget)
            .expect("the active back-edge is coalesced");
        scheduler
            .complete(key, demand, &mut budget)
            .expect("the cycle root completes once");
        assert!(scheduler.next().expect("the queue remains valid").is_none());
        assert_eq!(
            scheduler.entries.get(&7).map(|entry| entry.state),
            Some(ScheduleState::Complete {
                demand: Demand::ShapeOnly
            })
        );
    }

    #[test]
    fn traversal_scheduler_requeues_an_active_demand_upgrade_once() {
        let mut budget = Budget::with_limits(usize::MAX, usize::MAX);
        let mut scheduler = Scheduler::new();
        scheduler
            .request(11_u32, 11_u32, Demand::ShapeOnly, &mut budget)
            .expect("shape demand is queued");
        let (key, _, demand) = scheduler
            .next()
            .expect("the queue is valid")
            .expect("shape work is active");
        scheduler
            .request(11_u32, 11_u32, Demand::SelectedRoot, &mut budget)
            .expect("selected-root demand joins the active work");
        scheduler
            .complete(key, demand, &mut budget)
            .expect("shape completion retains the upgrade");
        let (key, _, upgraded) = scheduler
            .next()
            .expect("the queue is valid")
            .expect("the upgrade is queued");
        assert_eq!(upgraded, Demand::SelectedRoot);
        scheduler
            .complete(key, upgraded, &mut budget)
            .expect("selected-root work completes");
        assert!(scheduler.next().expect("the queue remains valid").is_none());
    }

    #[test]
    fn traversal_scheduler_uses_canonical_key_order() {
        let mut budget = Budget::with_limits(usize::MAX, usize::MAX);
        let mut scheduler = Scheduler::new();
        for key in [9_u32, 2, 5] {
            scheduler
                .request(key, key, Demand::ShapeOnly, &mut budget)
                .expect("the identity is queued");
        }
        let mut observed = Vec::new();
        while let Some((key, _, demand)) = scheduler.next().expect("the queue is valid") {
            observed.push(key);
            scheduler
                .complete(key, demand, &mut budget)
                .expect("the identity completes");
        }
        assert_eq!(observed, [2, 5, 9]);
    }

    #[test]
    fn canonical_candidate_bucket_moves_once_without_clone_or_reordering() {
        #[derive(Debug, Eq, PartialEq)]
        struct NonClone(u32);

        let mut index = HashMap::new();
        index.insert(7_u32, vec![NonClone(2), NonClone(5), NonClone(9)]);
        let allocation = index
            .get(&7)
            .expect("the canonical bucket is retained")
            .as_ptr();
        let candidates = take_owned_candidate_bucket(&mut index, &7)
            .expect("the canonical bucket is consumed once");
        assert_eq!(candidates.as_ptr(), allocation);
        assert_eq!(candidates, [NonClone(2), NonClone(5), NonClone(9)]);
        assert!(!index.contains_key(&7));
        assert!(take_owned_candidate_bucket(&mut index, &7).is_none());
    }

    #[test]
    fn opaque_projection_reuses_only_the_matching_self_identity() {
        assert_eq!(
            opaque_argument_action(Some(17_u32), Some(17_u32)),
            OpaqueArgumentAction::SelfReference(17)
        );
    }

    #[test]
    fn opaque_projection_preserves_nonmatching_and_unidentified_arguments() {
        assert_eq!(
            opaque_argument_action(Some(17_u32), Some(23_u32)),
            OpaqueArgumentAction::ConvertNormally
        );
        assert_eq!(
            opaque_argument_action(Some(17_u32), None),
            OpaqueArgumentAction::ConvertNormally
        );
        assert_eq!(
            opaque_argument_action::<u32>(None, Some(17_u32)),
            OpaqueArgumentAction::ConvertNormally
        );
    }

    #[test]
    fn opaque_type_outlives_reuses_only_the_matching_subject_identity() {
        assert_eq!(
            opaque_argument_action(Some(29_u32), Some(29_u32)),
            OpaqueArgumentAction::SelfReference(29)
        );
        assert_eq!(
            opaque_argument_action(Some(29_u32), Some(31_u32)),
            OpaqueArgumentAction::ConvertNormally
        );
        assert_eq!(
            opaque_argument_action::<u32>(None, Some(29_u32)),
            OpaqueArgumentAction::ConvertNormally
        );
    }

    #[test]
    fn type_depth_control_accepts_the_boundary_and_fails_closed_after_it() {
        check_type_depth(MAX_DEPTH).expect("the exact type-depth boundary remains accepted");
        assert_eq!(
            check_type_depth(MAX_DEPTH + 1),
            Err("compiler type depth exceeded".to_owned())
        );
    }

    #[test]
    fn generic_parameter_owner_control_rejects_absent_and_wrong_kind_slots() {
        assert_eq!(
            validate_parameter_class(None, ParameterClass::Lifetime),
            Err("generic parameter owner is absent".to_owned())
        );
        assert_eq!(
            validate_parameter_class(Some(ParameterClass::Type), ParameterClass::Const),
            Err("generic parameter kind disagrees with compiler use".to_owned())
        );
        validate_parameter_class(Some(ParameterClass::Lifetime), ParameterClass::Lifetime)
            .expect("an exact parameter kind remains accepted");
    }

    #[test]
    fn recursion_diagnostic_retains_fixed_crash_surviving_slots() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("the diagnostic fixture clock follows the epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "rawr-import-recursion-diagnostic-{}-{nonce}",
            std::process::id()
        ));
        let mut diagnostic = Diagnostic::create(&path).expect("the fixed journal is created");
        diagnostic
            .milestone("compiler", "after-analysis-enter", "global")
            .expect("the completed boundary is retained");
        diagnostic
            .breadcrumb(
                "begin",
                "selected-surface",
                "selected-type-edge",
                "type",
                "0123456789abcdef:iced::widget::text",
                DiagnosticIdentity {
                    hash: [0x17; 16],
                    namespace: Namespace::Type,
                },
                Some(0x42),
                "definition",
                false,
            )
            .expect("the unmatched operation is retained");
        drop(diagnostic);

        let bytes = std::fs::read(&path).expect("the fixed journal remains after writer exit");
        assert_eq!(
            bytes.len(),
            DIAGNOSTIC_SLOT_BYTES
                * usize::try_from(DIAGNOSTIC_SLOTS).expect("the slot count fits usize")
        );
        let first = String::from_utf8_lossy(&bytes[..DIAGNOSTIC_SLOT_BYTES]);
        let second =
            String::from_utf8_lossy(&bytes[DIAGNOSTIC_SLOT_BYTES..DIAGNOSTIC_SLOT_BYTES * 2]);
        assert!(first.contains("milestone\tcompiler\tafter-analysis-enter\tglobal\tglobal"));
        assert!(second.contains(concat!(
            "begin\tselected-surface\tselected-type-edge\ttype\t",
            "0123456789abcdef:iced::widget::text"
        )));
        assert!(second.contains("depth=1\tmax_depth=1\tunique_identities=1"));
        assert!(second.contains("unique_instantiations=1"));
        assert!(second.contains("substitutions_change_key=false"));
        std::fs::remove_file(path).expect("the diagnostic fixture is retired");
    }

    #[test]
    fn selected_surface_classification_is_explicit_for_work_noop_and_rejection() {
        assert_eq!(
            selected_surface_work(DefKind::Fn, false),
            SelectedSurfaceWork::Function
        );
        assert_eq!(
            selected_surface_work(DefKind::AssocTy, true),
            SelectedSurfaceWork::AssociatedTypeDefault
        );
        assert_eq!(
            selected_surface_work(DefKind::Mod, false),
            SelectedSurfaceWork::Noop
        );
        assert_eq!(
            selected_surface_work(DefKind::Closure, false),
            SelectedSurfaceWork::Reject
        );
    }

    #[test]
    fn implementation_classification_is_explicit_for_work_noop_and_rejection() {
        assert_eq!(
            implementation_work(DefKind::Trait),
            ImplementationWork::Trait
        );
        assert_eq!(
            implementation_work(DefKind::Struct),
            ImplementationWork::Nominal
        );
        assert_eq!(implementation_work(DefKind::Fn), ImplementationWork::Noop);
        assert_eq!(
            implementation_work(DefKind::Closure),
            ImplementationWork::Reject
        );
    }
}
